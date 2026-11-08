// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What runs under a pane, on this Mac or over a host's link (`terminum-dispatcher.md` AC-128, AC-130):
//! one `ps` a walk, never arguments beyond a program's name. Signals go by pid and start time read just
//! before, deepest first, the pane's own shell spared; never by name or pattern.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::app::Shared;
use crate::tmux::Server;

/// How long a `ps` or `kill` is waited for: past it, nothing is known of that machine this walk.
const WAIT: Duration = Duration::from_secs(12);
const PS: [&str; 3] = ["ps", "-Ao", "pid=,ppid=,time=,rss=,etime=,lstart="];

#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: String,
    /// CPU seconds so far
    pub cpu: f64,
    pub rss_kib: u64,
    /// seconds it has run
    pub age: u64,
    pub start: String,
}

/// Children by parent.
pub type Tree = HashMap<String, Vec<Proc>>;

/// What runs under a pane's root.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Under {
    pub any: bool,
    pub cpu: f64,
    pub rss_kib: u64,
    /// the oldest of them: the agent's running time
    pub age: u64,
}

impl Under {
    pub fn gib(&self) -> f64 {
        self.rss_kib as f64 / 1_048_576.0
    }
}

fn seconds(t: &str) -> f64 {
    let (days, rest) = t.split_once('-').map_or((0.0, t), |(d, r)| (d.parse().unwrap_or(0.0), r));
    rest.split(':').fold(0.0, |sum, part| sum * 60.0 + part.parse::<f64>().unwrap_or(0.0)) + days * 86400.0
}

/// `ps`'s lines; what is not of its shape (a `ps` that took other options) makes no row.
pub fn parse(out: &str) -> Tree {
    let mut tree = Tree::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 6 {
            tree.entry(f[1].to_string()).or_default().push(Proc { pid: f[0].to_string(), cpu: seconds(f[2]), rss_kib: f[3].parse().unwrap_or(0), age: seconds(f[4]) as u64, start: f[5..].join(" ") });
        }
    }
    tree
}

/// What runs under `root`, parents before their children.
fn descendants<'t>(tree: &'t Tree, root: &str) -> Vec<&'t Proc> {
    let (mut found, mut stack) = (Vec::new(), vec![root.to_string()]);
    while let Some(p) = stack.pop() {
        for kid in tree.get(&p).into_iter().flatten() {
            found.push(kid);
            stack.push(kid.pid.clone());
        }
    }
    found
}

pub fn under(tree: &Tree, root: &str) -> Under {
    descendants(tree, root).into_iter().fold(Under::default(), |sum, p| Under { any: true, cpu: sum.cpu + p.cpu, rss_kib: sum.rss_kib + p.rss_kib, age: sum.age.max(p.age) })
}

/// `words` run on `server`'s machine, within WAIT: on this Mac, or by one ssh command with its recipe.
async fn run(app: &Shared, server: &Server, words: &[&str]) -> Option<String> {
    if !server.is_local() {
        let recipe = crate::hosts::recipe_of(app, &server.name)?;
        return crate::hosts::tunnel::ssh(&recipe, &["-o", "ControlPath=none"], &words.join(" "), None, WAIT).await.ok();
    }
    let mut c = tokio::process::Command::new(words[0]);
    c.args(&words[1..]).stdin(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(WAIT, c.output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether `dir` is a folder on `server`'s machine (`terminum.md` AC-92): tmux opens a window in its home
/// folder, saying nothing, when the one asked is not there. `None`: the host did not answer.
pub async fn is_folder(app: &Shared, server: &Server, dir: &str) -> Option<bool> {
    let path = crate::tasks::folder::folder_in(&server.home_folder().await?, dir);
    if server.is_local() {
        // off the async workers: a stale mount can hang
        return crate::http::off(move || Ok(std::path::Path::new(&path).is_dir())).await.ok();
    }
    let out = run(app, server, &["test", "-d", &crate::tmux::layout::sh(&path), "&&", "echo", YES, "||", "echo", NO]).await?;
    // what the host's rc files print comes before
    out.lines().rev().find_map(|l| match l.trim() { YES => Some(true), NO => Some(false), _ => None })
}

const YES: &str = "tm-folder-yes";
const NO: &str = "tm-folder-no";

/// One `ps` of `server`'s machine; none: nothing is known.
pub async fn read(app: &Shared, server: &Server) -> Option<Tree> {
    run(app, server, &PS).await.map(|o| parse(&o))
}

/// A real pid: what `kill` may be given (0 would be the caller's own group, 1 is never ours).
fn is_pid(pid: &str) -> bool {
    pid.bytes().all(|b| b.is_ascii_digit()) && pid.parse::<u32>().is_ok_and(|p| p > 1)
}

async fn signal(app: &Shared, server: &Server, name: &str, pids: &[String]) {
    let pids: Vec<&str> = pids.iter().map(String::as_str).filter(|p| is_pid(p)).collect();
    if !pids.is_empty() {
        let mut words = vec!["kill", name];
        words.extend(pids);
        let _ = run(app, server, &words).await;
    }
}

/// SIGTERM to what runs under `root`, deepest first, and after `grace` SIGKILL to those still the
/// same pid and start time (AC-126). How many were signalled; `Err`: `ps` did not answer, none were.
pub async fn end(app: &Shared, server: &Arc<Server>, root: &str, grace: Duration) -> Result<usize, String> {
    if !is_pid(root) {
        return Err(format!("{root} is no pane's process"));
    }
    let tree = read(app, server).await.ok_or("ps did not answer")?;
    let targets: Vec<(String, String)> = descendants(&tree, root).into_iter().rev().map(|p| (p.pid.clone(), p.start.clone())).collect();
    signal(app, server, "-TERM", &targets.iter().map(|t| t.0.clone()).collect::<Vec<_>>()).await;
    tokio::time::sleep(grace).await;
    if let Some(now) = read(app, server).await {
        let still: Vec<(String, String)> = descendants(&now, root).into_iter().map(|p| (p.pid.clone(), p.start.clone())).collect();
        signal(app, server, "-KILL", &targets.iter().filter(|t| still.contains(t)).map(|t| t.0.clone()).collect::<Vec<_>>()).await;
    }
    Ok(targets.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "Thu Oct  8 16:17:25 2026";

    #[test]
    fn ac128_a_tree_sums_what_runs_under_its_root() {
        let t = parse(&format!("  10 1 0:03.50 100 01:00 {START}\n  11 10 1:00.00 2000 1-00:00:10 {START}\n  12 11 0:00.25 300 00:05 {START}\n  20 1 0:09.00 999 00:01 {START}\n"));
        assert_eq!(under(&t, "1").rss_kib, 100 + 2000 + 300 + 999);
        let u = under(&t, "10");
        assert!(u.any);
        assert_eq!(u.rss_kib, 2300);
        assert!((u.cpu - 60.25).abs() < 0.01, "{}", u.cpu);
        assert_eq!(u.age, 86410, "the oldest: days too");
        assert_eq!(under(&t, "12"), Under::default(), "nothing under it: only its shell remains");
    }

    #[test]
    fn ac126_a_parent_comes_before_its_children_and_its_start_is_kept() {
        let t = parse(&format!("  11 10 0:01 1 00:05 {START}\n  12 11 0:01 1 00:05 {START}\n"));
        let order: Vec<&str> = descendants(&t, "10").iter().map(|p| p.pid.as_str()).collect();
        assert_eq!(order, ["11", "12"], "reversed for signals: deepest first");
        assert_eq!(descendants(&t, "10")[0].start, "Thu Oct 8 16:17:25 2026");
    }

    #[test]
    fn ac126_only_a_real_pid_is_signalled() {
        assert!(is_pid("4321"));
        for not in ["0", "1", "", "-1", "12 34", "1;rm", "$(x)"] {
            assert!(!is_pid(not), "{not}");
        }
    }
}
