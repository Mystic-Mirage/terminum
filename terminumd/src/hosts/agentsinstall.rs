// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! terminum's hooks and MCP server put into, refreshed in or taken out of an ssh host's coding
//! agents: over the host's recipe one `exec /bin/sh -c '<script>'` (never the login shell's
//! syntax) that finds claude (known folders, then `$SHELL -lc`, then `$SHELL -ic`, each without
//! input and stopped after 10 s, its answer read after a random marker line: login files print to
//! stdout) and CODEX_HOME likewise, then runs the helper's `hooks … --host-mode` (cli/hostmode.rs).
//! Only the helper's `<marker> <agent> <result>` lines are read.

use std::future::Future;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::app::Shared;
use crate::cli::agentsetup::AGENTS;

/// Where claude is looked for before asking the login shell (globs expanded by /bin/sh).
pub const KNOWN: [&str; 7] = ["$HOME/.local/bin", "$HOME/.claude/local", "$HOME/.npm-global/bin", "$HOME/.nvm/versions/node/*/bin",
                              "$HOME/.bun/bin", "/usr/local/bin", "/opt/homebrew/bin"];
/// One whole run's limit: two shell probes per question of 10 s each, then `claude plugin …` calls.
const LIMIT: Duration = Duration::from_secs(300);

/// `s` as one single-quoted /bin/sh word.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A fresh marker: `TMH-` and 16 hex digits from the system's random source.
pub fn new_marker() -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 8];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("/dev/urandom: {e}"))?;
    Ok(format!("TMH-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>()))
}

/// The script /bin/sh runs on the host for `action` ("install", "remove"; `refresh`: install --refresh).
pub fn script(marker: &str, action: &str, refresh: bool) -> String {
    let dirs = KNOWN.iter().map(|d| if d.contains('*') { d.replace("$HOME", "\"$HOME\"") } else { format!("\"{d}\"") }).collect::<Vec<_>>().join(" ");
    let refresh = if refresh { " --refresh" } else { "" };
    format!(r#"M={marker}; export M
S="${{SHELL:-/bin/sh}}"
{CAP}
after() {{ awk -v m="$M" 'f {{ print; exit }} $0 == m {{ f = 1 }}' | tr -d '\r'; }}
ask() {{ v=$(cap "$S" -lc "echo \"\$M\"; $1" | after); [ -n "$v" ] || v=$(cap "$S" -ic "echo \"\$M\"; $1" | after); echo "$v"; }}
c=""
for d in {dirs}; do if [ -x "$d/claude" ] && [ ! -d "$d/claude" ]; then c="$d/claude"; break; fi; done
[ -n "$c" ] || c=$(ask 'command -v claude')
case "$c" in /*) [ -x "$c" ] || c="" ;; *) c="" ;; esac
[ -n "$CODEX_HOME" ] || CODEX_HOME=$(ask 'echo "$CODEX_HOME"')
case "$CODEX_HOME" in /*) export CODEX_HOME ;; *) unset CODEX_HOME ;; esac
H="$HOME/.terminum/bin/terminumd-agent"
if [ ! -x "$H" ]; then for a in {agents}; do echo "$M $a failed: no helper at ~/.terminum/bin/terminumd-agent"; done; exit 0; fi
if [ -n "$c" ]; then set -- --claude "$c"; else set --; fi
exec "$H" hooks {action} --host-mode --marker "$M"{refresh} "$@"
"#, agents = AGENTS.join(" "))
}

/// `cap <command…>`: the command without input, its stdout through a temporary file (a child
/// it leaves behind holding stdout, e.g. an rc's `sleep 60 &`, cannot keep a pipe open past the
/// cap), stopped after 10 s. `setsid` (Linux, busybox) starts it in its own process group, which
/// goes whole once it ended or at the cap; without setsid (macOS) its direct children go at the
/// cap. Not `set -m`: dash and busybox turn job control off without a terminal (`ssh -T`).
const CAP: &str = r#"cap() {
  t=$(mktemp) || return 1
  if command -v setsid >/dev/null 2>&1; then setsid "$@" </dev/null >"$t" 2>/dev/null &
  else "$@" </dev/null >"$t" 2>/dev/null &
  fi
  p=$!
  (sleep 10; pkill -9 -P $p; kill -9 -$p || kill -9 $p) >/dev/null 2>&1 & k=$!
  wait $p; r=$?; kill $k 2>/dev/null; kill -9 -$p 2>/dev/null
  cat "$t"; rm -f "$t"; return $r
}"#;

/// The remote command: the script under /bin/sh, whatever the login shell is.
pub fn remote_command(script: &str) -> String {
    format!("exec /bin/sh -c {}", sq(script))
}

/// Each agent's result from the helper's lines after `marker` (rc noise, `\r` and other lines
/// ignored); Err when no agent answered (`out`'s last line said).
pub fn parse(out: &str, marker: &str) -> Result<Vec<(String, String)>, String> {
    let prefix = format!("{marker} ");
    let got: Vec<(String, String)> = out.lines().map(|l| l.trim_end_matches('\r'))
        .filter_map(|l| l.strip_prefix(&prefix))
        .filter_map(|r| r.split_once(' '))
        .filter(|(a, _)| AGENTS.contains(a))
        .map(|(a, r)| (a.to_string(), r.trim().to_string())).collect();
    if got.is_empty() {
        let last = out.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("nothing");
        return Err(format!("the host's helper gave no result (last said: {last})"));
    }
    Ok(got)
}

/// The host's line for one result: "uses <host>'s own terminum" for skipped-own, else as given.
pub fn shown(host: &str, result: &str) -> String {
    match result {
        "skipped-own" => format!("uses {host}'s own terminum"),
        "not-found" => "not found".into(),
        "needs-trust" => "installed; trust terminum's hooks in Codex's /hooks once".into(),
        r => r.to_string(),
    }
}

/// `command` run on the host over its recipe; stdout (also when ssh failed after the helper
/// answered), or ssh's last words. A copy of tunnel.rs's `ssh` (private there) with ControlPath=none.
pub(super) async fn ssh(recipe: &[String], command: &str, marker: &str) -> Result<String, String> {
    let mut c = Command::new(super::ssh_bin());
    c.args(["-T", "-o", "BatchMode=yes", "-o", "ControlPath=none"]).args(super::connect_timeout(recipe)).env_remove("TMUX").env_remove("TMUX_PANE");
    c.args(super::recipe::argv(recipe)).arg(command);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let out = tokio::time::timeout(LIMIT, c.output()).await.map_err(|_| format!("ssh timed out after {} s", LIMIT.as_secs()))?
        .map_err(|e| format!("ssh: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() && !stdout.contains(marker) {
        let err = String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("").to_string();
        return Err(if err.is_empty() { format!("ssh exited {}", out.status) } else { err });
    }
    Ok(stdout)
}

async fn run(app: &Shared, host: &str, action: &str, refresh: bool) -> Result<Vec<(String, String)>, String> {
    let recipe = super::recipe_of(app, host).ok_or_else(|| format!("no host {host}"))?;
    run_on(&recipe, action, refresh).await
}

async fn run_on(recipe: &[String], action: &str, refresh: bool) -> Result<Vec<(String, String)>, String> {
    let marker = new_marker()?;
    let out = ssh(recipe, &remote_command(&script(&marker, action, refresh)), &marker).await?;
    parse(&out, &marker)
}

/// [`remove`] over a recipe kept from a host being removed.
pub async fn remove_on(recipe: &[String]) -> Result<Vec<(String, String)>, String> {
    run_on(recipe, "remove", false).await
}

/// Install on `host`: `helper` first puts the helper at ~/.terminum/bin/terminumd-agent (given the recipe).
pub async fn install<F, Fut>(app: &Shared, host: &str, helper: F) -> Result<Vec<(String, String)>, String>
where F: FnOnce(Vec<String>) -> Fut, Fut: Future<Output = Result<(), String>> {
    let recipe = super::recipe_of(app, host).ok_or_else(|| format!("no host {host}"))?;
    helper(recipe).await?;
    run(app, host, "install", false).await
}

/// terminum's entries (only those running terminum-host) taken out of `host`'s agents.
pub async fn remove(app: &Shared, host: &str) -> Result<Vec<(String, String)>, String> {
    run(app, host, "remove", false).await
}

/// After the helper was updated: rewritten only where the content differs.
pub async fn refresh(app: &Shared, host: &str) -> Result<Vec<(String, String)>, String> {
    run(app, host, "install", true).await
}

#[cfg(test)]
#[path = "agentsinstall_tests.rs"]
mod tests;
