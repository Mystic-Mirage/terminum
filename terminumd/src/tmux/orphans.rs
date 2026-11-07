// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Control clients a dropped link left on a host (AC-81). When ssh's end goes (the network, a
//! sleeping Mac), tmux keeps the client's pipes and its unsent output, and the client never
//! exits: `detach-client` cannot end it (measured on tmux 3.4), a signal does. Once a
//! connection, terminumd ends the host's control clients that are terminum's and orphaned:
//! their parent is init, or a shell whose parent is init. A live client (this Mac's, another
//! Mac's, whatever its ssh server) has a live parent and is never touched; a client under a
//! subreaper is left alone.
//!
//! terminum's are found two ways: in tmux's own list, control clients with `ignore-size` (also
//! where tmux rewrites its command line: FreeBSD, OpenBSD); and in `ps`, those named
//! `tmux: client` (Linux names them so) with terminum's command line, also those stuck before
//! tmux listed them. Never the server: its pid is skipped, and a server started by terminum's
//! command keeps that command line (it is named `tmux: server`). The script tells what it
//! ended through a user option of the server, read and removed by terminumd. Where no control
//! client answers, the same script goes over a plain ssh (`over_ssh`, hosts/silent.rs).

use std::sync::Arc;
use std::time::Duration;

use super::args::quote;
use super::Server;

/// What the sweep needs of the server: its pid and socket; its clients, for `listed`.
pub(crate) const SERVER_FORMAT: &str = "#{pid} #{socket_path}";
pub(crate) const CLIENTS_FORMAT: &str = "#{client_pid} #{client_control_mode} #{client_flags}";

/// The command for a plain ssh, when no control client can run the sweep: it prints `ended`'s input.
pub(crate) fn over_ssh(pid: u32, socket: &str, clients: &[String]) -> String {
    let opt = answer_option();
    format!("{}; tmux show-options -gqv {opt}; tmux set-option -gu {opt}", script(pid, socket, &listed(clients), &opt))
}

/// The pids the script says it ended; None before it is done.
pub(crate) fn ended(answer: &str) -> Option<Vec<u32>> {
    Some(answer.lines().next()?.strip_prefix("done")?.split_whitespace().filter_map(|p| p.parse().ok()).collect())
}

/// Once a connection of a host's server: its orphaned terminum control clients are ended.
pub fn sweep_once(server: &Arc<Server>) {
    if server.is_local() || !server.connected() || !super::first_on_link("orphans", server) {
        return;
    }
    let server = server.clone();
    tokio::spawn(async move { sweep(&server).await });
}

/// The control clients tmux lists with `ignore-size`, as terminum's are, from
/// `list-clients -F '#{client_pid} #{client_control_mode} #{client_flags}'`.
fn listed(lines: &[String]) -> Vec<u32> {
    lines.iter().filter_map(|l| {
        let mut f = l.split(' ');
        let (pid, control, flags) = (f.next()?, f.next()?, f.next()?);
        (control == "1" && flags.split(',').any(|x| x == "ignore-size")).then(|| pid.parse().ok()).flatten()
    }).collect()
}

/// The server's user option the script answers in: one per sweep, its name random (a pid and a
/// connection's number repeat across Macs and across host names of one server), so no sweep reads
/// another's and a late answer is never taken for a later sweep's.
fn answer_option() -> String {
    format!("@tm-orphans-{}", crate::auth::crypto::random_hex::<8>())
}

async fn sweep(server: &Server) {
    // the server's own pid, never a candidate: without it, nothing is swept; its socket names
    // its clients where tmux rewrote their command line
    let Some((pid, socket)) = server.run(format!("display-message -p '{SERVER_FORMAT}'")).await.ok().and_then(|l| {
        let (p, s) = l.first()?.trim().split_once(' ')?;
        Some((p.parse::<u32>().ok()?, s.to_string()))
    }) else { return };
    let lines = server.run(format!("list-clients -F '{CLIENTS_FORMAT}'")).await.unwrap_or_default();
    let opt = answer_option();
    let _ = server.run(format!("set-option -gu {opt}")).await;
    // in the background (a slow `ps` never holds up the host's commands), quiet: a control
    // client gets run-shell's output outside its answer
    if let Err(e) = server.run(format!("run-shell -b {}", quote(&script(pid, &socket, &listed(&lines), &opt)))).await {
        tracing::warn!(event = "clients.orphans", server = %server.id(), error = e);
        return;
    }
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let Ok(got) = server.run(format!("show-options -gqv {opt}")).await else { return };
        let Some(ended) = got.first().and_then(|l| ended(l)) else { continue };
        let _ = server.run(format!("set-option -gu {opt}")).await;
        if !ended.is_empty() {
            tracing::info!(event = "clients.orphans", server = %server.id(), ended = ?ended);
        }
        return;
    }
    let _ = server.run(format!("set-option -gu {opt}")).await;
    tracing::warn!(event = "clients.orphans", server = %server.id(), error = "no answer in 10 s: clients may have been ended unreported");
}

/// A shell, as `ps -o comm=` names it (a login shell starts with `-`; macOS gives a path).
const SHELLS: &str = "sh|bash|dash|ash|zsh|ksh|mksh|fish|-*|*/sh|*/bash|*/dash|*/zsh|*/ksh|*/fish";

/// terminum's host commands (tmux/command.rs) as `ps` shows them, after any options before them.
const SHAPES: &str = "*\" -u -C new-session -A -f ignore-size -s terminum \"|*\" -u -C attach-session -E -f ignore-size -t \"*";

/// The host's `/bin/sh` script: of the user's processes but the tmux server `server`, those that
/// are terminum's control clients and orphaned get SIGTERM; their pids are set in the server's
/// option `opt` after "done". A control client is one `listed` by tmux (a tmux process still: a
/// pid not reused), one Linux names `tmux: client` with terminum's command line (also one stuck
/// before tmux listed it), or where tmux rewrote its command line (FreeBSD, OpenBSD: `tmux:
/// client (socket)`) one of this server's `socket` with no terminal: an interactive client has
/// one (a socket path of other characters than [A-Za-z0-9/._-] leaves this rule out). Processes are
/// read with `ps`, else (busybox: no -U, no -p) from /proc; every read that fails skips its
/// process. No `#` (run-shell expands formats) and no `'` (it is quoted for the host's shell).
fn script(server: u32, socket: &str, listed: &[u32], opt: &str) -> String {
    let listed = match listed {
        [] => "\" - \"".to_string(),                                   // a pattern no pid matches
        l => l.iter().map(|p| format!("\" {p} \"")).collect::<Vec<_>>().join("|"),
    };
    let plain = !socket.is_empty() && socket.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c));
    let rewritten = if plain { format!("\" tmux: client ({socket}) \"") } else { "\" - \"".to_string() };
    let body = [
        r#"exec >/dev/null 2>&1"#,
        r#"set -f"#,
        r#"u=$(id -u)"#,
        // a process's name, parent and terminal, and the user's processes: by ps, else by /proc
        // (only what a ps that worked says: busybox's prints its usage to stdout)
        r#"pc() { x=$(ps -o comm= -p $1) && echo "$x" || cat /proc/$1/comm; }"#,
        r#"pp() { x=$(ps -o ppid= -p $1) && echo "$x" || sed -n "s/^PPid:[[:space:]]*//p" /proc/$1/status; }"#,
        // no terminal by /proc: its stat's tty_nr 0 (fields after the name's ")"); unknown: one
        r#"pt() { x=$(ps -o tty= -p $1) && echo "$x" || { n=$(sed "s/.*) //" /proc/$1/stat | cut -d" " -f5); [ "$n" = 0 ] && echo "?" || echo tty; }; }"#,
        r#"pl() { x=$(ps -ww -U $u -o pid= -o ppid= -o args=) && echo "$x" || for i in $(ls /proc); do case "$i" in *[!0-9]*) continue ;; esac; [ "$(sed -n "s/^Uid:[[:space:]]*\([0-9]*\).*/\1/p" /proc/$i/status)" = "$u" ] && echo "$i $(pp $i) $(tr "\0" " " < /proc/$i/cmdline)"; done; }"#,
        r#"pl | { e="""#,
        r#"while read -r p q a"#,
        r#"do case "$p" in ""|*[!0-9]*) continue ;; esac"#,
        &format!(r#"case " $p " in " {server} ") continue ;; {listed}) case "$(pc $p)" in *tmux*) ;; *) continue ;; esac ;; *) case " $a " in {SHAPES}) [ "$(pc $p)" = "tmux: client" ] || continue ;; {rewritten}) case "$(echo $(pt $p))" in ""|"?"|"??"|"-") ;; *) continue ;; esac ;; *) continue ;; esac ;; esac"#),
        r#"case "$q" in ""|*[!0-9]*) q=$(echo $(pp $p)) ;; esac"#,
        r#"case "$q" in ""|*[!0-9]*) continue ;; esac"#,
        // a parent other than init: orphaned only when it is a shell whose parent is init
        r#"if [ $q -ne 1 ]"#,
        &format!(r#"then case "$(pc $q)" in {SHELLS}) ;; *) continue ;; esac"#),
        r#"[ $(pp $q) -eq 1 ] || continue"#,
        r#"fi"#,
        r#"kill -TERM $p && e="$e $p""#,
        r#"done"#,
        &format!(r#"tmux set-option -g {opt} "done$e""#),
        r#"}"#,
        r#"true"#,
    ];
    format!("/bin/sh -c '{}'", body.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_are_control_clients_with_terminums_flags() {
        let lines: Vec<String> = [
            "123 1 attached,focused,control-mode,ignore-size,pause-after=2,UTF-8",
            "456 0 attached,focused,UTF-8",
            "789 1 attached,control-mode,UTF-8",
            "x 1 attached,control-mode,ignore-size,pause-after=2",
            "321 1 control-mode,ignore-size",
        ].iter().map(|s| s.to_string()).collect();
        assert_eq!(listed(&lines), vec![123, 321]);
    }

    /// AC-81: two sweeps (two Macs, two host names of one server) never answer in the same option.
    #[test]
    fn each_sweep_answers_in_an_option_of_its_own() {
        let (a, b) = (answer_option(), answer_option());
        assert!(a.starts_with("@tm-orphans-") && a.len() == "@tm-orphans-".len() + 16 && a != b, "{a} {b}");
    }

    /// AC-81: the script is plain for run-shell (no format, no quote of its own) and names only pids.
    #[test]
    fn the_script_names_only_pids_and_passes_run_shell() {
        for s in [script(99, "/tmp/tmux-1000/default", &[12, 345], "@tm-orphans-7"), script(99, "/tmp/tmux-1000/default", &[], "@tm-orphans-7")] {
            assert!(s.starts_with("/bin/sh -c '") && s.ends_with('\''));
            assert_eq!(s.matches('\'').count(), 2);
            assert!(!s.contains('#'));
            // the server is never a candidate; by `ps` only a process tmux named its client
            assert!(s.contains("case \" $p \" in \" 99 \") continue ;; "));
            assert!(s.contains("[ \"$(pc $p)\" = \"tmux: client\" ] || continue ;; "));
            assert!(s.contains("kill -TERM $p && e=\"$e $p\""));
            assert!(s.contains("tmux set-option -g @tm-orphans-7 \"done$e\""));
            // a ppid that is not one number (a ps that failed or ignored -p) skips its process
            assert!(s.contains("case \"$q\" in \"\"|*[!0-9]*) continue ;; esac; if [ $q -ne 1 ]"));
            // busybox: processes, names and parents from /proc when ps cannot say
            // (by `ls`: the script globs nothing, set -f)
            assert!(s.contains("|| for i in $(ls /proc); do") && s.contains("&& echo \"$x\" || cat /proc/$1/comm"));
        }
        assert!(script(99, "/tmp/tmux-1000/default", &[12, 345], "@o").contains("continue ;; \" 12 \"|\" 345 \") case "));
        assert!(script(99, "/tmp/tmux-1000/default", &[], "@o").contains("continue ;; \" - \") case "));
    }

    /// The script itself under /bin/sh, `ps` a stand-in that answers from `list`, `comm`, `ppid`
    /// and `tty` (or fails: `fails`); nothing is signalled (`kill` and `tmux` print). Its output.
    fn run(server: u32, listed: &[u32], list: &[String], comm: &str, ppid: &str, tty: &str, fails: bool) -> String {
        let dir = crate::testutil::temp_root().join(format!("tm-orphans-{}-{}", std::process::id(), fails));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("list"), list.join("\n") + "\n").unwrap();
        for (f, t) in [("comm", comm), ("ppid", ppid), ("tty", tty)] {
            std::fs::write(dir.join(f), t).unwrap();
        }
        let ps = dir.join("ps");
        let answer = "d=$(dirname \"$0\"); for n; do :; done\ncase \"$*\" in *-U*) cat \"$d/list\" ;; *comm=*) grep \"^$n \" \"$d/comm\" | cut -d\" \" -f2- ;; *ppid=*) grep \"^$n \" \"$d/ppid\" | cut -d\" \" -f2- ;; *tty=*) grep \"^$n \" \"$d/tty\" | cut -d\" \" -f2- ;; esac\n";
        std::fs::write(&ps, format!("#!/bin/sh\n{}", if fails { "echo usage >&2; exit 1\n" } else { answer })).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ps, std::fs::Permissions::from_mode(0o755)).unwrap();
        let s = script(server, "/tmp/tmux-1001/default", listed, "@o");
        let inner = s.strip_prefix("/bin/sh -c '").and_then(|x| x.strip_suffix('\'')).unwrap()
            .replace("exec >/dev/null 2>&1; ", "exec 2>/dev/null; ").replace("kill -TERM $p", "echo KILL $p").replace("tmux set-option -g @o", "echo ANSWER");
        let out = std::process::Command::new("/bin/sh").arg("-c").arg(&inner)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display())).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// AC-81: ended: an orphan of init's, one under a shell of init's, one tmux lists whose command
    /// line tmux rewrote, one of a BSD whose command line tmux rewrote with no terminal. Left: the
    /// server, a client under sshd, under ssh, under a shell that lives, a listed pid now another
    /// program, a line a backslash ends, a pid that is not a number, a BSD client with a terminal,
    /// one of another server's socket.
    #[test]
    fn the_script_ends_only_orphaned_clients() {
        let shape = "tmux -u -C new-session -A -f ignore-size -s terminum";
        let list = [
            format!("   10     1 {shape}"),
            format!("   11     1 {shape}"),
            "   12    50 tmux -u -C attach-session -E -f ignore-size -t $0".into(),
            format!("   13    60 {shape}"),
            "   14     1 tmux -C attach \\".into(),
            format!("   15     1 {shape}"),
            "   16     1 tmux: client (/tmp/tmux-1000/default)".into(),
            "   17     1 python3 serve.py".into(),
            format!("  abc     1 {shape}"),
            format!("   18    70 {shape}"),
            format!("   20    80 {shape}"),
            "   21     1 tmux: client (/tmp/tmux-1001/default)".into(),
            "   22     1 tmux: client (/tmp/tmux-1001/default)".into(),
            "   23     1 tmux: client (/tmp/tmux-1001/other)".into(),
        ];
        let comm = "10 tmux: server\n11 tmux: client\n12 tmux: client\n13 tmux: client\n14 tmux: client\n15 vim\n16 tmux\n17 python3\n18 tmux: client\n20 tmux: client\n21 tmux\n22 tmux\n23 tmux\n50 sh\n60 sshd\n70 ssh\n80 bash\n";
        let out = run(10, &[16, 17], &list, comm, "50 1\n60 1\n70 1\n80 2\n", "21 -\n22 ttys001\n23 -\n", false);
        let killed: Vec<&str> = out.lines().filter_map(|l| l.strip_prefix("KILL ")).collect();
        assert_eq!(killed, ["11", "12", "16", "21"], "{out}");
        assert!(out.contains("ANSWER done 11 12 16 21"), "{out}");
    }

    /// AC-81: a ps that cannot say (busybox: no -U, no -p) gives way to /proc; where there is none
    /// (this Mac) nothing is ended, and the script still answers.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn a_ps_that_cannot_say_ends_nothing_without_proc() {
        let list = vec!["   11     1 tmux -u -C new-session -A -f ignore-size -s terminum".to_string()];
        let out = run(10, &[], &list, "11 tmux: client\n", "", "", true);
        assert!(!out.contains("KILL") && out.contains("ANSWER done"), "{out}");
    }
}
