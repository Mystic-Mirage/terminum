// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! `terminum upgrade [--to vX.Y.Z]` (AC-16, AC-86): download a release's package, its
//! SHA256SUMS and their signature; check the signature with the release key this installed
//! build carries, then that the list names the release asked for (and no older one than this
//! build unless asked for by name), then the checksum; unpack plain files only and run the
//! package's own installer. Nothing from the download runs before every check passed. The
//! system's curl, ssh-keygen and tar do the network, the signature and the unpacking.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::ctx::Ctx;
use super::install::platform;

use crate::update::RELEASES;
/// The package of every OS; releases before Linux servers had only the Mac's name for it,
/// which every release still carries (the same bytes) for the installs of then.
pub const TARBALL: &str = "terminum.tar.gz";
pub const MAC_TARBALL: &str = "terminum-macos.tar.gz";
const NAME: &str = "terminum";
pub const NAMESPACE: &str = "terminum-release";

/// [0, 15, 0] for "v0.15.0"; None for a build of a checkout.
pub fn version(text: &str) -> Option<Vec<u64>> {
    let digits = text.trim().strip_prefix('v')?;
    digits.split('.').map(|p| if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None }).collect()
}

fn is_tag(tag: &str) -> bool {
    version(tag).is_some_and(|v| v.len() == 3) && tag == tag.trim()
}

/// A folder of our own for the download, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(ctx: &Ctx) -> Result<Scratch, String> {
        use std::os::unix::fs::DirBuilderExt;
        let base = ctx.var("TMPDIR").filter(|t| !t.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
        // macOS's clock moves in microseconds: the count keeps two at once in one process apart
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let dir = base.join(format!("tm-upgrade-{}-{nanos}-{n}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fetch(url: &str, dest: &Path) -> Result<(), String> {
    let out = Command::new("/usr/bin/curl")
        .args(["-q", "-fsSL", "--max-time", "300", "-o"])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("/usr/bin/curl: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if said.is_empty() { format!("curl exited {}", out.status.code().unwrap_or(-1)) } else { said })
}

/// True if `sums` is signed by the release key in `signers`.
fn verify(signers: &Path, sums: &Path, sig: &Path) -> Result<bool, String> {
    let key = std::fs::read_to_string(signers).unwrap_or_default();
    if !key.contains("ssh-") {
        return Err("this install carries no release key, so it cannot check a release: install with the one-line installer (README)".into());
    }
    let source = std::fs::File::open(sums).map_err(|e| e.to_string())?;
    let r = Command::new("/usr/bin/ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(signers)
        .args(["-I", NAMESPACE, "-n", NAMESPACE, "-s"])
        .arg(sig)
        .stdin(source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("/usr/bin/ssh-keygen: {e}"))?;
    Ok(r.success())
}

/// The signed SHA256SUMS names its release: the one asked for, and no older one than this
/// build unless asked for by name (a replayed old release, AC-86).
fn check_release(sums: &str, tag: Option<&str>, here: &str) -> Result<(), String> {
    let named = sums.lines().filter(|l| l.starts_with("# release ")).find_map(|l| l.split_whitespace().nth(2));
    let Some(named) = named else {
        return Err("the release does not say which version it is: nothing installed".into());
    };
    if let Some(tag) = tag.filter(|t| *t != named) {
        return Err(format!("asked for {tag}, the release says {named}: nothing installed"));
    }
    if let (None, Some(n), Some(h)) = (tag, version(named), version(here)) && n < h {
        return Err(format!(
            "the latest release says {named}, older than the installed {here}: nothing installed (to go back on purpose: terminum upgrade --to {named})"
        ));
    }
    Ok(())
}

/// Every entry under `dir` must be a folder or a regular file of one link (a hard link could be
/// a file outside the package); only then does each mode become 0755 (a folder or a program) or
/// 0644, whatever the package or the user's umask said (no setuid, setgid or sticky).
fn plain(dir: &Path, root: &Path) -> Result<(), String> {
    refuse_odd(dir, root)?;
    set_modes(dir)
}

fn refuse_odd(dir: &Path, root: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let name = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        if !(meta.is_file() && meta.nlink() == 1 || meta.is_dir()) {
            return Err(format!("refusing {name}: not a plain file or folder inside the package"));
        }
        if meta.is_dir() {
            refuse_odd(&path, root)?;
        }
    }
    Ok(())
}

fn set_modes(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let mode = if meta.is_dir() || meta.permissions().mode() & 0o100 != 0 { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            set_modes(&path)?;
        }
    }
    Ok(())
}

/// Unpacks a package with the system's tar (which refuses `..` and absolute names), then
/// refuses anything that is not a plain file or folder.
pub fn extract(tar: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let out = Command::new("/usr/bin/tar")
        .arg("-xzf")
        .arg(tar)
        .arg("-C")
        .arg(dest)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("/usr/bin/tar: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("refusing the package: not a plain file or folder inside the package ({said})"));
    }
    plain(dest, dest)
}

/// The package a release's signed list names for this machine: the one of every OS, else (a
/// release from before Linux servers) the Mac's, which a Linux server cannot install.
pub fn archive_of(sums: &str, mac: bool) -> Result<&'static str, String> {
    let names = |f: &str| sums.lines().any(|l| l.ends_with(&format!("  {f}")));
    match (names(TARBALL), names(MAC_TARBALL), mac) {
        (true, _, _) => Ok(TARBALL),
        (false, true, true) => Ok(MAC_TARBALL),
        (false, true, false) => Err("that release is from before terminum ran as a Linux server: nothing installed".into()),
        _ => Err("the signed list names no package: nothing installed".into()),
    }
}

/// The file's SHA-256 in lowercase hex, read in fixed chunks (a package is never held whole in memory).
fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(crate::auth::crypto::hex(&hasher.finalize()))
}

/// Downloads, checks and installs a release; the installer's exit status.
pub fn upgrade(ctx: &Ctx, tag: Option<&str>) -> Result<i32, String> {
    if let Some(t) = tag.filter(|t| !is_tag(t)) {
        return Err(format!("not a release tag: {t} (e.g. v0.3.0)"));
    }
    let pkg = ctx.pkg.clone().ok_or("cannot tell which installed build this command belongs to: nothing installed")?;
    let releases = ctx.var("TM_RELEASE_URL").filter(|u| !u.is_empty()).unwrap_or(RELEASES).trim_end_matches('/').to_string();
    let base = match tag {
        Some(t) => format!("{releases}/download/{t}"),
        None => format!("{releases}/latest/download"),
    };
    let tmp = Scratch::new(ctx)?;
    for f in ["SHA256SUMS", "SHA256SUMS.sig"] {
        fetch(&format!("{base}/{f}"), &tmp.0.join(f)).map_err(|e| format!("download failed ({base}): {e}"))?;
    }
    if !verify(&pkg.join("release-signers"), &tmp.0.join("SHA256SUMS"), &tmp.0.join("SHA256SUMS.sig"))? {
        return Err("the release is not signed by the release key: nothing installed".into());
    }
    let sums = std::fs::read_to_string(tmp.0.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    let here = std::fs::read_to_string(pkg.join("BUILD")).unwrap_or_default();
    check_release(&sums, tag, here.trim())?;
    let tarball = archive_of(&sums, cfg!(target_os = "macos"))?;
    fetch(&format!("{base}/{tarball}"), &tmp.0.join(tarball)).map_err(|e| format!("download failed ({base}): {e}"))?;
    let want = sums.lines().find(|l| l.ends_with(&format!("  {tarball}"))).and_then(|l| l.split_whitespace().next());
    let got = sha256_file(&tmp.0.join(tarball)).map_err(|e| format!("{tarball}: {e}"))?;
    if want != Some(got.as_str()) {
        return Err(format!("checksum mismatch for {tarball}: nothing installed"));
    }
    extract(&tmp.0.join(tarball), &tmp.0.join("pkg"))?;
    let installer = tmp.0.join("pkg").join(NAME).join("agents").join(platform()).join("terminumd");
    // the installer sees the command's own environment (ctx.env), as every other child does
    let status = Command::new(&installer).arg("install").env_clear().envs(&ctx.env).status().map_err(|e| format!("{}: {e}", installer.display()))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_hashes_in_chunks_to_its_whole_digest() {
        let dir = crate::testutil::fresh_dir("sha");
        // larger than the 64 KiB buffer, so several chunks and a partial last one are hashed
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let f = dir.join("pkg.tar.gz");
        std::fs::write(&f, &data).unwrap();
        assert_eq!(sha256_file(&f).unwrap(), crate::auth::crypto::sha256_hex(&data));
        std::fs::write(&f, b"").unwrap();
        assert_eq!(sha256_file(&f).unwrap(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(sha256_file(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_package_of_every_os_else_the_macs_only_on_a_mac() {
        let both = format!("# release v1\n00  {TARBALL}\n00  {MAC_TARBALL}\n");
        let old = format!("# release v0\n00  {MAC_TARBALL}\n");
        assert_eq!(archive_of(&both, true), Ok(TARBALL));
        assert_eq!(archive_of(&both, false), Ok(TARBALL));
        assert_eq!(archive_of(&old, true), Ok(MAC_TARBALL), "a release from before: the Mac's package");
        assert!(archive_of(&old, false).is_err_and(|e| e.contains("before terminum ran as a Linux server")));
        assert!(archive_of("# release v1\n", true).is_err());
    }

    #[test]
    fn versions_and_tags() {
        assert_eq!(version("v0.15.0"), Some(vec![0, 15, 0]));
        assert_eq!(version("abc-123"), None);
        assert_eq!(version("v1..2"), None);
        assert!(is_tag("v0.3.0") && !is_tag("latest/../../x") && !is_tag("v1.2") && !is_tag("v1.2.x"));
    }

    #[test]
    fn the_release_named_by_the_signed_list() {
        let sums = "# release v9.0.0\nabc  terminum-macos.tar.gz\n";
        assert!(check_release(sums, None, "v8.0.0").is_ok());
        assert!(check_release(sums, Some("v9.0.0"), "v9.1.0").is_ok(), "an older one by name, on purpose");
        assert!(check_release(sums, None, "v9.1.0").unwrap_err().contains("older than the installed v9.1.0"));
        assert!(check_release(sums, Some("v9.9.9"), "").unwrap_err().contains("the release says v9.0.0"));
        assert!(check_release("abc  x\n", None, "").unwrap_err().contains("does not say which version"));
        assert!(check_release(sums, None, "dev").is_ok(), "a checkout's build is never newer");
    }
}
