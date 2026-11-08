// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Sign-ins (AC-09). The right password (`POST /auth/login`, websignin.rs) gets the browser a
//! sign-in: a 256-bit secret the page keeps in its origin's storage and sends as
//! `X-TM-Session` (not a cookie: a cookie on 127.0.0.1 would reach every local port).
//! `Signins` keeps at most 20 of them for 30 days in `signins.json` (0600), never the secret
//! itself.
//!
//! The hello proof: another program may hold the port while terminumd is down, so before the
//! page sends its secret it asks `GET /api/hello?n=<nonce>&id=<id>` and checks the answer.
//! - `id` = the first 16 hex digits of SHA-256(secret): names the sign-in, reveals nothing of it.
//! - `proof_key` = HMAC-SHA256(secret, "tm-proof-key"), hex: derived by the page from its secret
//!   and kept by terminumd beside the sign-in's SHA-256 (terminumd never keeps the secret).
//! - the answer `{"proof"}` = hex(HMAC-SHA256(proof_key's 32 bytes, "tm-hello-v1:" + n)).
//!
//! The secret is 64 lowercase hex characters, keyed as their UTF-8 bytes. An unknown `id` is
//! 404 `unknown_signin`, with no proof.
//!
//! - beside it, `{"boot"}`: 256 random bits this process drew when it started.
//!
//! The credential: after the proof the page sends `<id>.<hex(HMAC-SHA256(proof_key's 32 bytes,
//! "tm-cred-v1:" + boot))>` as X-TM-Session (and in the WebSocket's subprotocols), never the
//! secret. A program that takes the port once terminumd let it go gets a credential for a boot
//! that is gone: the next terminumd draws another, so it never accepts it. The secret itself is
//! refused from a browser that sends Sec-Fetch-Site (Chrome, Firefox, Safari 16.4 and later on
//! fetch; on a WebSocket's upgrade WebKit does, Chromium not), so a page that sent it fails loud instead of leaking it unnoticed; a script or test
//! (none sends it; Node's fetch sends only Sec-Fetch-Mode) may still use it.
//!
//! Only a terminumd that issued that sign-in holds its proof key, so another program cannot
//! answer, and the nonce (fresh per ask) keeps an old answer from being replayed. The fixed
//! prefixes keep the proof key from signing anything else. Reading the file gives not the
//! secret (SHA-256 and HMAC do not invert) but the proof key, which makes a credential: whoever
//! reads the 0600 file is this user already, who can read the browser's storage too.

use std::io;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::checks::Denial;
use super::crypto::{hex, hmac_sha256, random_hex, same, sha256_hex, unhex};
use super::store;

/// Sign-ins kept; a new one beyond drops the oldest.
pub const MAX_SIGNINS: usize = 20;
/// How long a sign-in lasts, in seconds (30 days).
pub const SIGNIN_TTL_S: u64 = 30 * 86400;
pub const FILE: &str = "signins.json";
/// The sign-ins of 0.5.0 and before (one-time codes, origin 127.0.0.1:47900): their secrets
/// stay in that origin's storage, which any program may serve once the port is free, so they
/// are all dropped, never carried over.
const OLD_FILE: &str = "local-sessions.json";

/// The sign-in's name in `/api/hello`: the first 16 hex digits of its SHA-256.
pub fn signin_id(secret: &str) -> String {
    sha256_hex(secret.as_bytes())[..16].to_string()
}

/// The key the hello proof is made with, derived from the secret (see the module doc).
pub fn proof_key(secret: &str) -> String {
    hex(&hmac_sha256(secret.as_bytes(), b"tm-proof-key"))
}

/// The hello proof for nonce `nonce` under `proof_key`.
pub fn proof(proof_key: &str, nonce: &str) -> String {
    keyed(proof_key, "tm-hello-v1:", nonce)
}

/// The credential of the sign-in whose proof key is `proof_key` in the process of `boot`.
pub fn credential(proof_key: &str, boot: &str) -> String {
    keyed(proof_key, "tm-cred-v1:", boot)
}

/// hex(HMAC-SHA256(proof key's bytes, prefix + text)): the prefix keeps one use from standing in for another.
fn keyed(proof_key: &str, prefix: &str, text: &str) -> String {
    let key = unhex(proof_key).unwrap_or_default();
    hex(&hmac_sha256(&key, format!("{prefix}{text}").as_bytes()))
}

/// A nonce is 16 to 128 hex digits.
pub fn valid_nonce(n: &str) -> bool {
    (16..=128).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Item {
    sha256: String,
    id: String,
    proof_key: String,
    /// Seconds since 1970.
    expires: u64,
    /// What made it ("password").
    by: String,
}

impl Item {
    fn well_formed(&self) -> bool {
        is_hex(&self.sha256, 64) && is_hex(&self.id, 16) && is_hex(&self.proof_key, 64)
    }
}

/// The sign-ins, in memory and in `<app dir>/signins.json`, and this process's boot.
/// `revoked` ticks whenever sign-ins leave memory: every socket of one rechecks its credential then.
/// `create`, `revoke` and `revoke_all` write the file (blocking: an async caller runs them off
/// its workers); `check` and `hello` read memory only, never waiting for a write (`writer`
/// serializes the writes; `items` is held only to read or swap).
pub struct Signins {
    path: PathBuf,
    items: Mutex<Vec<Item>>,
    writer: Mutex<()>,
    boot: String,
    revoked: tokio::sync::watch::Sender<u64>,
}

impl Signins {
    /// The sign-ins in `dir`'s file, the expired ones dropped. A file that cannot be read is
    /// set aside as `signins.json.bak` and terminumd starts without sign-ins; the file of
    /// 0.5.0 and before is removed.
    pub fn load(dir: &Path) -> Signins {
        if let Err(e) = std::fs::remove_file(dir.join(OLD_FILE))
            && e.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(event = "signin.old_not_removed", error = %e);
        }
        let path = dir.join(FILE);
        let items = store::read_or_set_aside(&path, |v| {
            serde_json::from_value::<Vec<Item>>(v.get("items").cloned().unwrap_or_default())
                .map_err(|e| format!("{FILE}: {e}"))
        });
        let now = store::unix_now();
        let mut items: Vec<Item> = items.unwrap_or_default().into_iter().filter(|i| i.well_formed() && i.expires > now).collect();
        keep_newest(&mut items, MAX_SIGNINS);
        Signins { path, items: Mutex::new(items), writer: Mutex::new(()), boot: random_hex::<32>(), revoked: tokio::sync::watch::channel(0).0 }
    }

    /// A new sign-in made by `by`: its secret (256 random bits, hex), which only the page keeps.
    /// The oldest go beyond 20. Not made when the file cannot be written.
    pub fn create(&self, by: &str) -> io::Result<String> {
        self.create_at(by, store::unix_now())
    }

    fn create_at(&self, by: &str, now: u64) -> io::Result<String> {
        let secret = random_hex::<32>();
        let item = Item {
            sha256: sha256_hex(secret.as_bytes()),
            id: signin_id(&secret),
            proof_key: proof_key(&secret),
            expires: now + SIGNIN_TTL_S,
            by: by.chars().filter(|c| !c.is_control()).take(100).collect(),
        };
        let _writing = self.writer.lock();
        let mut next: Vec<Item> = self.items.lock().iter().filter(|i| i.expires > now).cloned().collect();
        next.push(item);
        keep_newest(&mut next, MAX_SIGNINS);
        self.save(&next)?;
        *self.items.lock() = next;
        Ok(secret)
    }

    /// The credential is a live sign-in's: its credential for this boot, or (`browser`: false
    /// only) its secret.
    pub fn check(&self, credential: &str, browser: bool) -> bool {
        self.check_at(credential, browser, store::unix_now())
    }

    fn check_at(&self, credential: &str, browser: bool, now: u64) -> bool {
        if credential.is_empty() || (browser && !credential.contains('.')) {
            return false;
        }
        let digest = self.signin_of(credential);
        // every item compared, so the time says nothing of which one matched
        self.items.lock().iter().fold(false, |found, i| found | (same(&i.sha256, &digest) && i.expires > now))
    }

    /// The SHA-256 of the sign-in a credential presents: an `<id>.<mac>`'s when its MAC is
    /// right for this boot, else the presented secret's own.
    fn signin_of(&self, credential: &str) -> String {
        if let Some((id, mac)) = credential.split_once('.') {
            let items = self.items.lock();
            let item = items.iter().find(|i| same(&i.id, &id.to_ascii_lowercase()));
            return match item {
                Some(i) if same(&self::credential(&i.proof_key, &self.boot), &mac.to_ascii_lowercase()) => i.sha256.clone(),
                _ => String::new(),
            };
        }
        sha256_hex(credential.as_bytes())
    }

    /// The page signs out: the sign-in it presents (its credential or secret) goes, in memory at
    /// once and in the file; false when it presents none kept here (a credential of another boot).
    /// Err: gone from memory all the same (no page uses it a moment longer), still in the file,
    /// which a restart would read back; the next write of the file (any sign-in) leaves it out.
    pub fn revoke(&self, credential: &str) -> io::Result<bool> {
        let digest = self.signin_of(credential);
        let _writing = self.writer.lock();
        let items = self.items.lock().clone();
        let next: Vec<Item> = items.iter().filter(|i| !same(&i.sha256, &digest)).cloned().collect();
        if next.len() == items.len() {
            return Ok(false);
        }
        *self.items.lock() = next.clone();
        self.revoked.send_modify(|n| *n += 1);
        self.save(&next).map(|()| true)
    }

    /// `terminum signout`, a new password: every page signs in again. Err: gone from memory all
    /// the same, still in the file, as for `revoke`.
    pub fn revoke_all(&self) -> io::Result<()> {
        let _writing = self.writer.lock();
        self.items.lock().clear();
        self.revoked.send_modify(|n| *n += 1);
        self.save(&[])
    }

    /// Ticks when sign-ins are revoked: a socket taken before it rechecks its credential at each
    /// tick, so it closes at once (AC-09).
    pub fn revocations(&self) -> tokio::sync::watch::Receiver<u64> {
        self.revoked.subscribe()
    }

    pub fn count(&self) -> usize {
        let now = store::unix_now();
        self.items.lock().iter().filter(|i| i.expires > now).count()
    }

    /// `GET /api/hello?n=<nonce>&id=<id>` (no sign-in): the proof for the sign-in named `id`,
    /// and this process's boot, which its credential is made with.
    pub fn hello(&self, id: &str, nonce: &str) -> Result<(String, String), Denial> {
        if !valid_nonce(nonce) {
            return Err(Denial::new(400, "bad_nonce", "n must be 16 to 128 hex digits"));
        }
        if !is_hex(id, 16) {
            return Err(Denial::new(400, "bad_id", "id must be 16 hex digits"));
        }
        let now = store::unix_now();
        let items = self.items.lock();
        let key = items.iter().find(|i| same(&i.id, &id.to_ascii_lowercase()) && i.expires > now).map(|i| i.proof_key.clone());
        key.map(|k| (proof(&k, nonce), self.boot.clone()))
            .ok_or_else(|| Denial::new(404, "unknown_signin", "Signed out: sign in again"))
    }

    fn save(&self, items: &[Item]) -> io::Result<()> {
        store::write_private(&self.path, &json!({ "items": items }))
    }
}

/// Keeps the `max` sign-ins that expire last; of those expiring in the same second, the ones
/// added last (the sort is stable).
fn keep_newest(items: &mut Vec<Item>, max: usize) {
    items.sort_by_key(|i| i.expires);
    items.drain(..items.len().saturating_sub(max));
}

#[cfg(test)]
#[path = "signin_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "credential_tests.rs"]
mod credential_tests;
