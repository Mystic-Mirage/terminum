// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! AC-16 / AC-86: `terminum upgrade` and install.sh install a release only when it is signed
//! by the release key and its checksum matches, from a fake release served as local files (no
//! network), signed with a throwaway key made for the test.

use std::process::Command;

use sha2::{Digest, Sha256};

use super::*;
use crate::cli::upgrade::{self, NAMESPACE};

/// A tar member: a file (name, text, mode), or a link, hard link or device.
enum Member {
    File(&'static str, String, u32),
    Symlink(&'static str, String),
    Hardlink(&'static str, String),
    Device(&'static str),
}

fn octal(field: &mut [u8], v: u64) {
    let s = format!("{:0w$o}\0", v, w = field.len() - 1);
    field.copy_from_slice(s.as_bytes());
}

/// A gzip'd ustar archive of `members` (written here: the system's tar refuses to make some).
fn tar_gz(members: &[Member], dest: &Path) {
    let mut out = Vec::new();
    for m in members {
        let (name, kind, data, link, mode) = match m {
            Member::File(n, t, mode) => (*n, b'0', t.as_bytes().to_vec(), String::new(), *mode),
            Member::Symlink(n, l) => (*n, b'2', vec![], l.clone(), 0o777),
            Member::Hardlink(n, l) => (*n, b'1', vec![], l.clone(), 0o644),
            Member::Device(n) => (*n, b'3', vec![], String::new(), 0o644),
        };
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut h[100..108], mode as u64);
        octal(&mut h[108..116], 0);
        octal(&mut h[116..124], 0);
        octal(&mut h[124..136], data.len() as u64);
        octal(&mut h[136..148], 0);
        h[156] = kind;
        h[157..157 + link.len()].copy_from_slice(link.as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(&data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    let tar = dest.with_extension("tar-raw");
    std::fs::write(&tar, out).unwrap();
    let gz = Command::new("/usr/bin/gzip").args(["-c", "-n"]).arg(&tar).output().unwrap();
    std::fs::write(dest, gz.stdout).unwrap();
}

fn keypair(dir: &Path, name: &str) -> PathBuf {
    let key = dir.join(name);
    let ok = Command::new("/usr/bin/ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", name, "-f"]).arg(&key).status().unwrap();
    assert!(ok.success());
    key
}

struct T {
    home: Home,
    ctx: Ctx,
    rel: PathBuf,
    key: PathBuf,
    marker: PathBuf,
}

impl T {
    fn new() -> T {
        let home = Home::new();
        let rel = home.path("releases/latest/download");
        std::fs::create_dir_all(&rel).unwrap();
        let url = format!("file://{}", home.path("releases").display());
        let mut ctx = home.ctx(&[("TM_RELEASE_URL", &url)]);
        let key = keypair(&home.dir, "release");
        let build = home.path("build");
        std::fs::create_dir_all(&build).unwrap();
        let pubkey = std::fs::read_to_string(key.with_extension("pub")).unwrap();
        std::fs::write(build.join("release-signers"), format!("{NAMESPACE} namespaces=\"{NAMESPACE}\" {pubkey}")).unwrap();
        ctx.pkg = Some(build);
        let marker = home.path("installed");
        T { home, ctx, rel, key, marker }
    }

    fn signers(&self) -> PathBuf {
        self.ctx.pkg.as_ref().unwrap().join("release-signers")
    }

    /// The package's own installer: its terminumd records the command it was run with.
    fn package(&self) -> Vec<Member> {
        let installer = format!("#!/bin/sh\necho \"$1 $HOME\" > '{}'\n", self.marker.display());
        vec![
            Member::File("terminum/agents/macos-aarch64/terminumd", installer.clone(), 0o755),
            Member::File("terminum/agents/macos-x86_64/terminumd", installer.clone(), 0o755),
            Member::File("terminum/agents/linux-x86_64/terminumd", installer.clone(), 0o755),
            Member::File("terminum/agents/linux-aarch64/terminumd", installer, 0o755),
            Member::File("terminum/BUILD", "v9\n".into(), 0o644),
        ]
    }

    fn publish_at(&self, dir: &Path, members: &[Member], checksum: Option<&str>, key: Option<&Path>, release: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let tar = dir.join(upgrade::TARBALL);
        tar_gz(members, &tar);
        let digest: String = checksum.map(str::to_string).unwrap_or_else(|| Sha256::digest(std::fs::read(&tar).unwrap()).iter().map(|b| format!("{b:02x}")).collect());
        let sums = dir.join("SHA256SUMS");
        std::fs::write(&sums, format!("# release {release}\n{digest}  {}\n", upgrade::TARBALL)).unwrap();
        let _ = std::fs::remove_file(dir.join("SHA256SUMS.sig"));
        if let Some(key) = key {
            let ok = Command::new("/usr/bin/ssh-keygen").args(["-Y", "sign", "-q", "-f"]).arg(key).args(["-n", NAMESPACE]).arg(&sums).status().unwrap();
            assert!(ok.success());
        }
    }

    fn publish(&self, members: &[Member], release: &str) {
        self.publish_at(&self.rel, members, None, Some(&self.key), release);
    }

    fn installed(&self) -> Option<String> {
        std::fs::read_to_string(&self.marker).ok().map(|s| s.trim().to_string())
    }
}

#[test]
fn a_good_release_installs() {
    let t = T::new();
    t.publish(&t.package(), "v9.0.0");
    assert_eq!(upgrade::upgrade(&t.ctx, None), Ok(0));
    let want = format!("install {}", t.ctx.home.display());
    assert_eq!(t.installed(), Some(want), "the package's own installer ran, with the command's environment (its HOME)");
}

#[test]
fn a_wrong_checksum_installs_nothing() {
    let t = T::new();
    t.publish_at(&t.rel, &t.package(), Some(&"0".repeat(64)), Some(&t.key), "v9.0.0");
    assert!(upgrade::upgrade(&t.ctx, None).unwrap_err().contains("checksum mismatch"));
    assert_eq!(t.installed(), None);
}

#[test]
fn a_member_outside_the_package_is_refused() {
    let t = T::new();
    let mut members = t.package();
    members.push(Member::File("../escape", "x".into(), 0o644));
    t.publish(&members, "v9.0.0");
    assert!(upgrade::upgrade(&t.ctx, None).is_err());
    assert_eq!(t.installed(), None);
    assert!(!t.home.path("escape").exists() && !Path::new("/tmp/escape").exists());
}

#[test]
fn an_unsigned_or_foreign_release_installs_nothing() {
    let t = T::new();
    t.publish_at(&t.rel, &t.package(), None, None, "v9.0.0");
    assert!(upgrade::upgrade(&t.ctx, None).unwrap_err().contains("download failed"));
    let attacker = keypair(&t.home.dir, "attacker");
    t.publish_at(&t.rel, &t.package(), None, Some(&attacker), "v9.0.0");
    assert!(upgrade::upgrade(&t.ctx, None).unwrap_err().contains("not signed by the release key"));
    assert_eq!(t.installed(), None);
}

#[test]
fn an_install_without_a_release_key_refuses() {
    let t = T::new();
    t.publish(&t.package(), "v9.0.0");
    std::fs::write(t.signers(), "# no key yet\n").unwrap();
    assert!(upgrade::upgrade(&t.ctx, None).unwrap_err().contains("no release key"));
}

#[test]
fn links_and_devices_are_refused() {
    let t = T::new();
    let outside = t.home.path("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("victim");
    std::fs::write(&victim, "keep").unwrap();
    let bad: Vec<Vec<Member>> = vec![
        vec![Member::Symlink("terminum/d", "../../outside".into()), Member::File("terminum/d/planted", "x".into(), 0o644)],
        vec![Member::Hardlink("terminum/h", victim.display().to_string()), Member::File("terminum/h", "owned".into(), 0o644)],
        vec![Member::Symlink("terminum/s", "/etc/hosts".into())],
        vec![Member::Device("terminum/null")],
    ];
    for extra in bad {
        let mut members = t.package();
        members.extend(extra);
        t.publish(&members, "v9.0.0");
        let err = upgrade::upgrade(&t.ctx, None).unwrap_err();
        assert!(err.contains("not a plain file or folder"), "{err}");
    }
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(t.installed(), None);
}

#[test]
fn setuid_bits_are_stripped() {
    let home = Home::new();
    let tar = home.path("p.tar.gz");
    tar_gz(&[Member::File("terminum/tool", "x".into(), 0o6777)], &tar);
    upgrade::extract(&tar, &home.path("x")).unwrap();
    assert_eq!(mode(&home.path("x/terminum/tool")), 0o755);
}

#[test]
fn modes_do_not_follow_the_umask_or_the_package() {
    let home = Home::new();
    let tar = home.path("p.tar.gz");
    tar_gz(&[Member::File("terminum/tool", "x".into(), 0o700), Member::File("terminum/notes", "x".into(), 0o600)], &tar);
    upgrade::extract(&tar, &home.path("x")).unwrap();
    assert_eq!(mode(&home.path("x/terminum/tool")), 0o755);
    assert_eq!(mode(&home.path("x/terminum/notes")), 0o644);
    assert_eq!(mode(&home.path("x/terminum")), 0o755);
}

#[test]
fn a_hard_link_inside_the_package_is_refused_before_any_mode_changes() {
    let home = Home::new();
    let tar = home.path("p.tar.gz");
    let members = [Member::File("terminum/a", "x".into(), 0o600), Member::Hardlink("terminum/b", "terminum/a".into())];
    tar_gz(&members, &tar);
    let err = upgrade::extract(&tar, &home.path("x")).unwrap_err();
    assert!(err.contains("not a plain file or folder"), "{err}");
    assert_eq!(mode(&home.path("x/terminum/a")) & 0o077, 0, "as tar left it: nothing was widened");
}

#[test]
fn an_older_release_is_not_installed_as_the_latest() {
    let t = T::new();
    std::fs::write(t.ctx.pkg.as_ref().unwrap().join("BUILD"), "v9.1.0\n").unwrap();
    t.publish(&t.package(), "v9.0.0");
    assert!(upgrade::upgrade(&t.ctx, None).unwrap_err().contains("older than the installed v9.1.0"));
    assert_eq!(t.installed(), None);
    t.publish_at(&t.home.path("releases/download/v9.0.0"), &t.package(), None, Some(&t.key), "v9.0.0");
    assert_eq!(upgrade::upgrade(&t.ctx, Some("v9.0.0")), Ok(0), "on purpose, by name");
}

#[test]
fn a_release_must_be_the_one_asked_for() {
    let t = T::new();
    t.publish_at(&t.home.path("releases/download/v9.9.9"), &t.package(), None, Some(&t.key), "v8.0.0");
    assert!(upgrade::upgrade(&t.ctx, Some("v9.9.9")).unwrap_err().contains("the release says v8.0.0"));
    assert_eq!(t.installed(), None);
}

#[test]
fn a_tag_must_look_like_one_and_a_missing_release_says_so() {
    let t = T::new();
    assert!(upgrade::upgrade(&t.ctx, Some("latest/../../x")).unwrap_err().contains("not a release tag"));
    assert!(upgrade::upgrade(&t.ctx, Some("v0.0.0")).unwrap_err().contains("download failed"));
}

// install.sh: the same checks before the package's terminumd runs `install`.

fn install_sh(t: &T, signers: &str) -> std::process::Output {
    let url = format!("file://{}", t.home.path("releases").display());
    install_sh_from(t, signers, &url, |s| s)
}

fn install_sh_from(t: &T, signers: &str, url: &str, edit: impl Fn(String) -> String) -> std::process::Output {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let text = edit(std::fs::read_to_string(repo.join("scripts/install.sh")).unwrap());
    let start = text.find("\nSIGNERS='").unwrap() + 1;
    let end = start + text[start..].find('\n').unwrap();
    let sh = t.home.path("install.sh");
    std::fs::write(&sh, format!("{}SIGNERS='{signers}'{}", &text[..start], &text[end..])).unwrap();
    Command::new("/bin/sh").arg(&sh).env_clear().env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin").env("HOME", &t.home.dir)
        .env("TM_RELEASE_URL", url).output().unwrap()
}

#[test]
fn install_sh_gives_up_on_a_stalled_server() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli/upgrade.rs")).unwrap();
    assert!(src.contains(r#""--max-time", "300""#), "install.sh keeps the deadline of `terminum upgrade`");
    let t = T::new();
    let signers = std::fs::read_to_string(t.signers()).unwrap().trim().to_string();
    // a server that accepts and never answers; the listener lives until the test ends
    let stalled = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", stalled.local_addr().unwrap());
    let deadline = "--connect-timeout 30 --max-time 300";
    let started = std::time::Instant::now();
    let r = install_sh_from(&t, &signers, &url, |s| {
        assert!(s.contains(deadline), "install.sh downloads with a connect timeout and the upgrade's deadline");
        s.replace(deadline, "--connect-timeout 2 --max-time 2")
    });
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("download failed"), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "took {:?}", started.elapsed());
}

#[test]
fn install_sh_checks_the_signature_and_runs_the_packages_terminumd() {
    let t = T::new();
    let signers = std::fs::read_to_string(t.signers()).unwrap().trim().to_string();
    let attacker = keypair(&t.home.dir, "attacker");
    t.publish_at(&t.rel, &t.package(), None, Some(&attacker), "v9.0.0");
    assert!(String::from_utf8_lossy(&install_sh(&t, &signers).stderr).contains("not signed by the release key"));
    assert_eq!(t.installed(), None);
    assert!(String::from_utf8_lossy(&install_sh(&t, "# no release key yet").stderr).contains("carries no release key"));
    t.publish(&t.package(), "v9.0.0");
    let r = install_sh(&t, &signers);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(t.installed().as_deref().and_then(|s| s.split(' ').next()), Some("install"));
}

#[test]
fn install_sh_rejects_a_replayed_signed_release() {
    let t = T::new();
    let build = t.home.path(".terminum/builds/current/BUILD");
    std::fs::create_dir_all(build.parent().unwrap()).unwrap();
    std::fs::write(&build, "v9.1.0\n").unwrap();
    let signers = std::fs::read_to_string(t.signers()).unwrap().trim().to_string();
    t.publish(&t.package(), "v9.0.0");
    let r = install_sh(&t, &signers);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("older than the installed v9.1.0"));
    assert_eq!(t.installed(), None);
    t.publish(&t.package(), "v9.2.0");
    assert!(install_sh(&t, &signers).status.success());
    assert_eq!(t.installed().as_deref().and_then(|s| s.split(' ').next()), Some("install"));
}

#[test]
fn install_sh_and_release_signers_name_the_same_key() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let keys = |text: &str| -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        words.windows(2).filter(|w| w[0].starts_with("ssh-")).map(|w| format!("{} {}", w[0], w[1].trim_end_matches('\''))).collect()
    };
    let sh = std::fs::read_to_string(repo.join("scripts/install.sh")).unwrap();
    let line = sh.lines().find(|l| l.starts_with("SIGNERS='")).unwrap();
    assert_eq!(keys(line), keys(&std::fs::read_to_string(repo.join("release-signers")).unwrap()));
}
