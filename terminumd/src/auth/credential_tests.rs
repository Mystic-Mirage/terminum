// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! AC-09: the page's credential instead of its secret: made for this process's boot only, gone
//! with its sign-in; the secret refused from a browser.

use super::*;
use crate::testutil::fresh_dir;

const NONCE: &str = "00112233445566778899aabbccddeeff";

/// The credential a page makes from its secret and hello's answer.
fn made(s: &Signins, secret: &str) -> String {
    let (_, boot) = s.hello(&signin_id(secret), NONCE).unwrap();
    format!("{}.{}", signin_id(secret), credential(&proof_key(secret), &boot))
}

#[test]
fn a_credential_signs_in_only_with_the_terminumd_it_was_made_for() {
    let dir = fresh_dir("cred-restart");
    let s = Signins::load(&dir);
    let secret = s.create("password").unwrap();
    let cred = made(&s, &secret);
    assert!(s.check(&cred, true) && s.check(&cred.to_uppercase(), true), "from a browser, hex in either case");
    let again = Signins::load(&dir);
    assert_ne!(again.hello(&signin_id(&secret), NONCE).unwrap().1, s.hello(&signin_id(&secret), NONCE).unwrap().1, "each process draws its boot");
    assert!(again.check(&secret, false), "a restarted terminumd knows the sign-in");
    assert!(!again.check(&cred, true), "but not the credential: a program that took the port got nothing the next one accepts");
    assert!(again.check(&made(&again, &secret), true));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_browser_never_signs_in_with_the_secret_and_a_wrong_credential_is_no_one() {
    let dir = fresh_dir("cred-browser");
    let s = Signins::load(&dir);
    let (a, b) = (s.create("password").unwrap(), s.create("password").unwrap());
    assert!(!s.check(&a, true), "a page that sent its secret is refused, not leaked unnoticed");
    assert!(s.check(&a, false), "a script's (no Sec-Fetch-*) is taken");
    let (cred_a, cred_b) = (made(&s, &a), made(&s, &b));
    let (id_a, mac_b) = (cred_a.split_once('.').unwrap().0, cred_b.split_once('.').unwrap().1);
    for bad in [format!("{id_a}.{mac_b}"), format!("{id_a}."), format!(".{mac_b}"), format!("{id_a}.{}", "0".repeat(64)), format!("{}.{mac_b}", "0".repeat(16))] {
        assert!(!s.check(&bad, true) && !s.check(&bad, false), "{bad}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_credential_goes_with_its_sign_in() {
    let dir = fresh_dir("cred-revoke");
    let s = Signins::load(&dir);
    let (a, b) = (s.create("password").unwrap(), s.create("password").unwrap());
    let (ca, cb) = (made(&s, &a), made(&s, &b));
    s.revoke(&ca).unwrap();
    assert!(!s.check(&ca, true) && !s.check(&a, false), "signing out with a credential ends its sign-in");
    assert!(s.check(&cb, true) && s.check(&b, false), "not another's");
    s.revoke_all().unwrap();
    assert!(!s.check(&cb, true), "terminum signout, a new password");
    let now = store::unix_now();
    let old = s.create_at("password", now - SIGNIN_TTL_S).unwrap();
    assert!(!s.check(&made_at(&s, &old), true), "nor past its sign-in's 30 days");
    std::fs::remove_dir_all(&dir).ok();
}

/// A credential for a sign-in hello no longer answers (expired): made from the boot directly.
fn made_at(s: &Signins, secret: &str) -> String {
    format!("{}.{}", signin_id(secret), credential(&proof_key(secret), &s.boot))
}
