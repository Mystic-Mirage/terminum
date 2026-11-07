// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! What runs under a pane, on this Mac or over a host's link (`terminum-dispatcher.md` AC-128, AC-130):
//! one `ps` a walk, never arguments beyond a program's name. Signals go by pid and start time read just
//! before, deepest first, the pane's own shell spared; never by name or pattern.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::app::Shared;
use crate::tmux::Server;

/// How long a host's `ps` is waited for: past it, nothing is known of that host this walk.
const HOST_WAIT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proc {
    pub pid: String,
    /// CPU seconds so far
    pub cpu: f64,
    pub rss_kib: u64,
    /// seconds it has run
    pub age: u64,
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

fn seconds(t: &str) -> f64 {
    let (days, rest) = t.split_once('-').map_or((0.0, t), |(d, r)| (d.parse().unwrap_or(0.0), r));
    rest.split(':').fold(0.0, |sum, part| sum * 60.0 + part.parse::<f64>().unwrap_or(0.0)) + days * 86400.0
}

/// `ps -o pid=,ppid=,time=,rss=,etime=`'s lines.
pub fn parse(out: &str) -> Tree {
    let mut tree = Tree::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let [pid, ppid, time, rss, etime] = f[..] {
            tree.entry(ppid.to_string()).or_default().push(Proc { pid: pid.to_string(), cpu: seconds(time), rss_kib: rss.parse().unwrap_or(0), age: seconds(etime) as u64 });
        }
    }
    tree
}

pub fn under(tree: &Tree, root: &str) -> Under {
    let (mut sum, mut stack) = (Under::default(), vec![root.to_string()]);
    while let Some(p) = stack.pop() {
        for kid in tree.get(&p).into_iter().flatten() {
            sum.any = true;
            sum.cpu += kid.cpu;
            sum.rss_kib += kid.rss_kib;
            sum.age = sum.age.max(kid.age);
            stack.push(kid.pid.clone());
        }
    }
    sum
}

/// `words` run on `server`'s machine: on this Mac, or over its host's link with a limit. Its output.
async fn run(app: &Shared, server: &Server, words: &[&str]) -> Option<String> {
    if server.is_local() {
        let out = tokio::process::Command::new(words[0]).args(&words[1..]).output().await.ok()?;
        return out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let recipe = crate::hosts::recipe_of(app, &server.name)?;
    let mut c = crate::hosts::ssh_command(crate::hosts::ssh_bin(), &recipe, &["-o", "ControlPath=none"]);
    crate::hosts::recipe::with_command(&mut c, &recipe, &words.join(" "));
    c.stdin(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(HOST_WAIT, c.output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// One `ps` of `server`'s machine; none: nothing is known.
pub async fn read(app: &Shared, server: &Server) -> Option<Tree> {
    run(app, server, &["ps", "-Ao", "pid=,ppid=,time=,rss=,etime="]).await.map(|o| parse(&o))
}

/// What runs under `root`, each by pid and start time; deepest first, so an agent's children go first.
async fn with_start(app: &Shared, server: &Server, root: &str) -> Vec<(String, String)> {
    let Some(text) = run(app, server, &["ps", "-Ao", "pid=,ppid=,lstart="]).await else { return Vec::new() };
    let rows: Vec<(String, String, String)> = text.lines().filter_map(|l| {
        let mut f = l.split_whitespace();
        Some((f.next()?.to_string(), f.next()?.to_string(), f.collect::<Vec<_>>().join(" ")))
    }).collect();
    let (mut found, mut stack) = (Vec::new(), vec![root.to_string()]);
    while let Some(p) = stack.pop() {
        for (pid, _, start) in rows.iter().filter(|(_, ppid, _)| *ppid == p) {
            found.push((pid.clone(), start.clone()));
            stack.push(pid.clone());
        }
    }
    found.reverse();
    found
}

async fn signal(app: &Shared, server: &Server, name: &str, pids: &[String]) {
    let pids: Vec<&str> = pids.iter().map(String::as_str).filter(|p| p.bytes().all(|b| b.is_ascii_digit())).collect();
    if pids.is_empty() {
        return;
    }
    let mut words = vec!["kill", name];
    words.extend(pids);
    let _ = run(app, server, &words).await;
}

/// SIGTERM to what runs under `root`, and after `grace` SIGKILL to those still the same (AC-126).
pub async fn end(app: &Shared, server: &Arc<Server>, root: &str, grace: Duration) {
    let targets = with_start(app, server, root).await;
    signal(app, server, "-TERM", &targets.iter().map(|t| t.0.clone()).collect::<Vec<_>>()).await;
    tokio::time::sleep(grace).await;
    let now = with_start(app, server, root).await;
    signal(app, server, "-KILL", &targets.iter().filter(|t| now.contains(t)).map(|t| t.0.clone()).collect::<Vec<_>>()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac128_a_tree_sums_what_runs_under_its_root() {
        let t = parse("  10 1 0:03.50 100 01:00\n  11 10 1:00.00 2000 1-00:00:10\n  12 11 0:00.25 300 00:05\n  20 1 0:09.00 999 00:01\n");
        let u = under(&t, "1");
        assert_eq!(u.rss_kib, 100 + 2000 + 300 + 999);
        let u = under(&t, "10");
        assert!(u.any);
        assert_eq!(u.rss_kib, 2300);
        assert!((u.cpu - 60.25).abs() < 0.01, "{}", u.cpu);
        assert_eq!(u.age, 86410, "the oldest: days too");
        assert_eq!(under(&t, "12"), Under::default(), "nothing under it: only its shell remains");
    }
}
