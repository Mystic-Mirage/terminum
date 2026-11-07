// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! terminum: the one command a user needs after the one-line install (AC-08 … AC-10, AC-16,
//! AC-87). It is terminumd itself, run by the name `terminum` (~/.terminum/bin/terminum links
//! the newest build's terminumd) or with a command as its first argument (`terminumd install`,
//! as install.sh runs it from the package).

pub(crate) mod agentrefresh;
pub mod agentsetup;
mod commands;
pub mod hook;
mod hookplan;
mod hooktokens;
pub(crate) mod hostmode;
mod ctx;
mod direct;
mod folders;
mod importer;
mod install;
mod launch;
mod mcp;
mod mcp_tasks;
mod sinks;
mod sockhttp;
mod systemd;
mod term;
mod upgrade;

#[cfg(test)]
mod tests;

pub use ctx::Ctx;

const ADD_USAGE: &str = "usage: terminum hosts add [<name>] '<ssh destination and options>' [--files]";

const USAGE: &str = "\
usage: terminum [command]

  status                  what runs: build, pid, port, password, tmux, hosts, pages
  folders                 have macOS ask now which protected folders (Documents, cloud
                          folders…) programs in terminum's terminals may read
  open [--print]          the page in this Mac's browser (http://127.0.0.1:48080/)
  password                the password every browser signs in with (8+ characters)
  signout                 sign out every browser
  hosts                   the remote hosts and their state
  hosts add [<name>] '<ssh destination and options>' [--files]
                          as after ssh in a shell; no name: the destination's host;
                          --files: browse its files (copies a helper once attached)
  hosts remove|enable|files-remove|off|on|retry <name>
  hosts agents <name> on|off
                          coding agents' progress from the host: terminum's hooks and
                          MCP server in its Claude Code and Codex
  tmux                    the sessions of your own tmux attached by name, here and on hosts
  tmux attach|detach <session> [--socket S | --host H]
                          show a session of your tmux (tmux -L S, or on host H) in terminum, or stop
  upgrade [--to vX.Y.Z]   download a release, check it, install it
  rollback                back to the build before
  restart                 load the launchd agent again (a new port; a port taken back)
  uninstall               remove it (keeps settings, state and logs; tmux untouched)
  hooks [install|remove] [claude|codex]
                          the coding agents found and terminum's hooks in them: exact
                          states, prompts and reminders (no agent named: every one found)
  mcp                     the MCP server a coding agent starts (plans, notes: hooks install)
  import                  workspaces, bookmarks and hosts of iterm-enhancer
  logs [-n N] [-f]        terminumd's log
  install                 install the package this terminumd came in";

const COMMANDS: [&str; 19] = [
    "status", "folders", "open", "password", "signout", "hosts", "tmux", "hooks", "mcp", "upgrade", "rollback", "restart", "uninstall", "import", "logs", "install", "help", "-h", "--help",
];

/// Is this run the command rather than the server?
pub fn wanted(args: &[String]) -> bool {
    let name = args.first().map(|a| a.rsplit('/').next().unwrap_or(a)).unwrap_or("");
    name == "terminum" || args.get(1).is_some_and(|a| COMMANDS.contains(&a.as_str()))
}

/// A parsed command line.
#[derive(Debug, PartialEq)]
pub enum Cmd {
    Status,
    Folders,
    Open { print: bool },
    Password,
    Signout,
    Hosts(Option<String>, Option<String>, Vec<String>),
    /// `hosts add [<name>] '<recipe>' [--files]`
    HostsAdd { name: Option<String>, recipe: Vec<String>, files: bool },
    /// `tmux [attach|detach <session> [--socket S | --host H]]` (AC-89): action, session, and
    /// where ("socket" or "host", and its value; empty: plain `tmux` here)
    Tmux(Option<(String, String, String, String)>),
    /// `hooks [status|install|remove] [claude|codex]` (AC-39)
    Hooks(String, Option<String>),
    /// `hooks … --host-mode …` on an ssh host, run by its helper (hostmode.rs)
    HostHooks(hostmode::HostArgs),
    /// `mcp`: the MCP server a coding agent starts, on stdin and stdout (AC-97, AC-98)
    Mcp,
    Upgrade(Option<String>),
    Rollback,
    Restart,
    Uninstall,
    Import,
    Logs { lines: u64, follow: bool },
    Install,
    Help,
    Version,
}

fn usage(msg: &str) -> String {
    format!("{msg}\n(terminum help lists the commands)")
}

/// The command line after the program's name; Err(usage message).
pub fn parse(args: &[String]) -> Result<Cmd, String> {
    let Some((cmd, rest)) = args.split_first() else { return Ok(Cmd::Status) };
    let none = |c: Cmd| if rest.is_empty() { Ok(c) } else { Err(usage(&format!("usage: terminum {cmd}"))) };
    match cmd.as_str() {
        "status" => none(Cmd::Status),
        "folders" => none(Cmd::Folders),
        "signout" => none(Cmd::Signout),
        "password" => none(Cmd::Password),
        "rollback" => none(Cmd::Rollback),
        "restart" => none(Cmd::Restart),
        "uninstall" => none(Cmd::Uninstall),
        "import" => none(Cmd::Import),
        "install" => none(Cmd::Install),
        "mcp" => none(Cmd::Mcp),
        "help" | "-h" | "--help" => Ok(Cmd::Help),
        "--version" => Ok(Cmd::Version),
        "open" => match rest {
            [] => Ok(Cmd::Open { print: false }),
            [p] if p == "--print" => Ok(Cmd::Open { print: true }),
            _ => Err(usage("usage: terminum open [--print]")),
        },
        "hosts" => match rest {
            [] => Ok(Cmd::Hosts(None, None, vec![])),
            [h, more @ ..] if h == "add" => {
                let files = more.iter().any(|a| a == "--files");
                let words: Vec<String> = more.iter().filter(|a| *a != "--files").cloned().collect();
                match words.as_slice() {
                    [] => Err(usage(ADD_USAGE)),
                    [recipe] => Ok(Cmd::HostsAdd { name: None, recipe: vec![recipe.clone()], files }),
                    [name, recipe @ ..] => Ok(Cmd::HostsAdd { name: Some(name.clone()), recipe: recipe.to_vec(), files }),
                }
            }
            [h, name, v] if h == "agents" && (v == "on" || v == "off") => Ok(Cmd::Hosts(Some(format!("agents-{v}")), Some(name.clone()), vec![])),
            [h, ..] if h == "agents" => Err(usage("usage: terminum hosts agents <name> on|off")),
            [h, more @ ..] if commands::HOST_ACTIONS.contains(&h.as_str()) => {
                Ok(Cmd::Hosts(Some(h.clone()), more.first().cloned(), more.iter().skip(1).cloned().collect()))
            }
            _ => Err(usage("usage: terminum hosts [add|remove|enable|files-remove|off|on|retry] <name>")),
        },
        "tmux" => match rest {
            [] => Ok(Cmd::Tmux(None)),
            [a, session] if a == "attach" || a == "detach" => Ok(Cmd::Tmux(Some((a.clone(), session.clone(), String::new(), String::new())))),
            [a, session, f, v] if (a == "attach" || a == "detach") && (f == "--socket" || f == "--host") =>
                Ok(Cmd::Tmux(Some((a.clone(), session.clone(), f[2..].to_string(), v.clone())))),
            _ => Err(usage("usage: terminum tmux [attach|detach <session> [--socket S | --host H]]")),
        },
        "hooks" => match rest {
            r if r.iter().any(|a| a == "--host-mode") => hostmode::parse(r).map(Cmd::HostHooks),
            [] => Ok(Cmd::Hooks("status".into(), None)),
            [a, more @ ..] if ["status", "install", "remove"].contains(&a.as_str()) && more.len() <= 1
                && more.first().is_none_or(|x| agentsetup::AGENTS.contains(&x.as_str())) => Ok(Cmd::Hooks(a.clone(), more.first().cloned())),
            _ => Err(usage("usage: terminum hooks [status|install|remove] [claude|codex]")),
        },
        "upgrade" => match rest {
            [] => Ok(Cmd::Upgrade(None)),
            [to, tag] if to == "--to" => Ok(Cmd::Upgrade(Some(tag.clone()))),
            [t] if t.starts_with("--to=") => Ok(Cmd::Upgrade(Some(t["--to=".len()..].to_string()))),
            _ => Err(usage("usage: terminum upgrade [--to vX.Y.Z]")),
        },
        "logs" => {
            let (mut lines, mut follow, mut it) = (50, false, rest.iter());
            while let Some(a) = it.next() {
                match a.as_str() {
                    "-f" => follow = true,
                    "-n" => lines = it.next().and_then(|n| n.parse().ok()).ok_or_else(|| usage("usage: terminum logs [-n N] [-f]"))?,
                    _ => return Err(usage("usage: terminum logs [-n N] [-f]")),
                }
            }
            Ok(Cmd::Logs { lines, follow })
        }
        other => Err(usage(&format!("terminum: unknown command {other}"))),
    }
}

pub fn refuse_root(euid: u32) -> Result<(), String> {
    if euid == 0 {
        return Err("Run as your user, not root: terminum lives in your home folder".into());
    }
    Ok(())
}

/// Refused as root, private files from here on (the commands that change the install).
fn as_user() -> Result<(), String> {
    // SAFETY: geteuid has no preconditions
    refuse_root(unsafe { libc::geteuid() })?;
    crate::umask::private();
    Ok(())
}

/// Runs `cmd`; Ok(exit status) or Err(message for stderr, status 1).
pub fn run(ctx: &Ctx, cmd: Cmd) -> Result<i32, String> {
    match cmd {
        Cmd::Help => ctx.say(USAGE),
        Cmd::Mcp => return Ok(mcp::main()),
        Cmd::Version => ctx.say(crate::config::build_id()),
        Cmd::Status => { commands::status(ctx); hooks_tip(ctx); }
        Cmd::Folders => folders::command(ctx)?,
        Cmd::Open { print } => { commands::open_page(ctx, print)?; hooks_tip(ctx); }
        Cmd::Signout => commands::signout(ctx)?,
        Cmd::Password => commands::password(ctx)?,
        Cmd::Hosts(None, ..) => commands::hosts_list(ctx)?,
        Cmd::Hosts(Some(h), None, _) => return Err(format!("usage: terminum hosts {h} <name>")),
        Cmd::HostsAdd { name, recipe, files } => commands::hosts_add(ctx, name.as_deref(), &recipe, files)?,
        Cmd::Hosts(Some(h), Some(_), recipe) if !recipe.is_empty() => return Err(format!("usage: terminum hosts {h} <name>")),
        Cmd::Hosts(Some(h), Some(name), _) => commands::hosts_action(ctx, &h, &name)?,
        Cmd::Tmux(None) => commands::tmux_list(ctx)?,
        Cmd::Tmux(Some((action, session, flag, value))) => commands::tmux_action(ctx, &action, &session, &flag, &value)?,
        Cmd::Hooks(action, agent) => {
            as_user()?;
            agentsetup::run(ctx, &action, agent.as_deref())?;
        }
        Cmd::HostHooks(args) => {
            as_user()?;
            hostmode::run(ctx, &args)?;
        }
        Cmd::Upgrade(tag) => return upgrade::upgrade(ctx, tag.as_deref()),
        Cmd::Import => commands::import_state(ctx)?,
        Cmd::Logs { lines, follow } => commands::logs(ctx, lines, follow)?,
        Cmd::Install | Cmd::Rollback | Cmd::Uninstall | Cmd::Restart => {
            as_user()?;
            let _lock = install::lock(ctx)?;
            match cmd {
                Cmd::Install => {
                    let pkg = ctx.pkg.clone().ok_or("cannot tell which package this terminumd came in")?;
                    install::install(ctx, &pkg)?;
                    agentrefresh::refresh(ctx);
                    install::path_tip(ctx);
                    hooks_tip(ctx);
                }
                Cmd::Rollback => install::rollback(ctx)?,
                Cmd::Uninstall => {
                    agentrefresh::remove(ctx);
                    install::uninstall(ctx)?;
                }
                _ => install::restart(ctx)?,
            }
        }
    }
    Ok(0)
}

/// The agents found without terminum's hooks, said once after status, open and install.
fn hooks_tip(ctx: &Ctx) {
    if let Some(tip) = agentsetup::hint(ctx) {
        ctx.say(if ctx.color() { format!("\x1b[38;5;69mTip: {tip}\x1b[0m") } else { format!("Tip: {tip}") });
    }
}

/// The command line of this process; the exit status.
pub fn main(args: &[String]) -> i32 {
    let argv0 = args.first().map_or("terminum", String::as_str);
    let result = parse(args.get(1..).unwrap_or(&[])).map_err(|e| (2, e)).and_then(|cmd| {
        let mut ctx = Ctx::from_env(argv0).map_err(|e| (1, e))?;
        if let Cmd::HostHooks(a) = &cmd {
            hostmode::prepare(&mut ctx, a);
        }
        run(&ctx, cmd).map_err(|e| (1, e))
    });
    match result {
        Ok(code) => code,
        Err((code, msg)) => {
            eprintln!("{msg}");
            code
        }
    }
}
