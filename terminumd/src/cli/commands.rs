// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! The commands that talk to a running terminumd over its socket (AC-08 … AC-10, AC-87):
//! status, open, password, signout, hosts; and import and logs, which work on its files.

use std::time::Duration;

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::{importer, install, launch, sockhttp};

pub const HOST_ACTIONS: [&str; 6] = ["remove", "enable", "files-remove", "off", "on", "retry"];
const MIN_PASSWORD: usize = 8;

/// The answer of terminumd; Err with its message when it refused or is down.
fn checked(ctx: &Ctx, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    waited(ctx, method, path, body, Duration::from_secs(5))
}

/// How long `hosts <action>` waits for terminumd: the sum of the server's stages for that
/// action plus an answer's 5 s. enable: probe 30 + copy 300 + connect (ready 20, forward 5,
/// answer 5); files-remove: its ssh 60; agents-on: the helper brought up to date (probe 30 +
/// copy 300) and the install 300; agents-off: the hooks' removal 90 and the helper's 60 (AC-99).
pub fn host_budget(action: &str) -> Duration {
    Duration::from_secs(5 + match action {
        "enable" => 30 + 300 + 30,
        "files-remove" => 60,
        "agents-on" => 30 + 300 + 300,
        "agents-off" => 90 + 60,
        _ => 0,
    })
}

fn waited(ctx: &Ctx, method: &str, path: &str, body: Option<Value>, budget: Duration) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let (status, answer) = sockhttp::call(&ctx.sock(), method, path, body.as_ref(), budget).map_err(|_| {
        if start.elapsed() >= budget {
            timed_out(path, budget)
        } else {
            format!("{} (see: terminum status)", launch::describe(ctx, None, None))
        }
    })?;
    let refused = answer.as_ref().and_then(|a| a.get("error")).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    if status >= 400 || refused {
        return Err(sockhttp::message(answer.as_ref(), status));
    }
    Ok(answer.filter(Value::is_object).unwrap_or_else(|| json!({})))
}

/// The message when terminumd took the whole budget: the work may still finish there.
pub fn timed_out(path: &str, budget: Duration) -> String {
    format!("no answer from terminumd to {path} within {} s; it may still be working (see: terminum hosts)", budget.as_secs())
}

// ── status ──────────────────────────────────────────────────────────────────

pub fn uptime(seconds: u64) -> String {
    let (d, h, m, s) = (seconds / 86400, seconds % 86400 / 3600, seconds % 3600 / 60, seconds % 60);
    let parts: Vec<String> = [(d, "d"), (h, "h"), (m, "m")].iter().filter(|(v, _)| *v > 0).map(|(v, u)| format!("{v}{u}")).collect();
    if parts.is_empty() { format!("{s}s") } else { parts.join(" ") }
}

fn plural(n: u64, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "?".into(),
        other => other.to_string(),
    }
}

/// A server's state: a word, or {"state": word, "why": text} as terminumd tells it.
fn state_text(state: &Value) -> String {
    if state.is_object() {
        let word = state["state"].as_str().filter(|s| !s.is_empty()).unwrap_or("?");
        return match state["why"].as_str().filter(|s| !s.is_empty()) {
            Some(why) => format!("{word} ({why})"),
            None => word.into(),
        };
    }
    if state.is_null() || state == "" { "?".into() } else { text(state) }
}

/// Session names (terminumd lists them) or a count.
fn sessions_text(sessions: &Value) -> String {
    match sessions.as_array() {
        Some(list) if list.is_empty() => plural(0, "session"),
        Some(list) => format!("{} ({})", plural(list.len() as u64, "session"), list.iter().map(text).collect::<Vec<_>>().join(", ")),
        None => plural(sessions.as_u64().unwrap_or(0), "session"),
    }
}

/// The page's address on this Mac.
pub fn page_url(port: &Value) -> String {
    format!("http://127.0.0.1:{}/", if port.is_null() { crate::settings::DEFAULT_PORT.to_string() } else { text(port) })
}

/// How other devices reach the page: HTTPS through `tailscale serve` (README "Over Tailscale").
pub fn serve_hint(port: &Value) -> String {
    format!("Other devices: serve it over HTTPS on your tailnet: tailscale serve --bg --https=443 {} \
             (it takes this machine's name; a name of its own: README \"Over Tailscale\")", page_url(port).trim_end_matches('/'))
}

/// A desktop to open the page on: a Mac, or Linux with a display.
pub fn has_desktop(ctx: &Ctx) -> bool {
    cfg!(target_os = "macos") || ["DISPLAY", "WAYLAND_DISPLAY"].iter().any(|k| ctx.var(k).is_some_and(|v| !v.is_empty()))
}

/// How a computer reaches the page of a server with no desktop.
pub fn tunnel_hint(port: &Value) -> String {
    let p = if port.is_null() { crate::settings::DEFAULT_PORT.to_string() } else { text(port) };
    format!("From your computer: ssh -L {p}:127.0.0.1:{p} <this server>, then open http://127.0.0.1:{p}/")
}

/// Whether browsers can sign in: a password is set, or why not.
pub fn password_line(password: &Value) -> String {
    if let Some(e) = password["error"].as_str().filter(|e| !e.is_empty()) {
        return format!("password:   not usable — {e}");
    }
    if password["set"].as_bool() == Some(true) { "password:   set".into() } else { "password:   not set — no browser signs in: terminum password".into() }
}

/// The status report from /health (AC-08).
pub fn status_lines(h: &Value) -> Vec<String> {
    let field = |k: &str| if h[k].is_null() { "?".to_string() } else { text(&h[k]) };
    let mut out = vec![
        format!("terminumd:  running, build {}, pid {}, up {}", field("build"), field("pid"), uptime(h["uptime_s"].as_u64().unwrap_or(0))),
        format!("page:       {} (other devices: tailscale serve, HTTPS)", page_url(&h["port"])),
        password_line(&h["password"]),
    ];
    let tmux_ok = h["tmux"]["ok"].as_bool().unwrap_or(true);
    if !tmux_ok {
        out.push(format!("tmux:       {}", h["tmux"]["message"].as_str().filter(|m| !m.is_empty()).unwrap_or("not available")));
    }
    let servers = h["servers"].as_array().cloned().unwrap_or_default();
    for s in &servers {
        let name = s["name"].as_str().unwrap_or("?");
        out.push(format!("tmux:       {name}: {}, {}, {}", sessions_text(&s["sessions"]), plural(s["panes"].as_u64().unwrap_or(0), "pane"), state_text(&s["state"])));
    }
    if servers.is_empty() && tmux_ok {
        out.push("tmux:       no server yet (a new session starts one)".into());
    }
    for x in h["hosts"].as_array().into_iter().flatten() {
        let files = x["files"].as_str().filter(|f| !f.is_empty()).map(|f| format!(", files {f}")).unwrap_or_default();
        let why = x["why"].as_str().filter(|w| !w.is_empty()).map(|w| format!(" — {w}")).unwrap_or_default();
        out.push(format!("host:       {}: {}{files}{why}", x["name"].as_str().unwrap_or("?"), x["state"].as_str().unwrap_or("?")));
    }
    out.push(format!("pages:      {} connected", h["pages"].as_u64().unwrap_or(0)));
    out
}

pub fn status(ctx: &Ctx) {
    match launch::health(ctx) {
        Some(h) => {
            status_lines(&h).into_iter().for_each(|l| ctx.say(l));
            // under launchd on macOS only: a terminal's terminumd starts tmux as that terminal (AC-08)
            if cfg!(target_os = "macos") && h["service"] == true && let Some(line) = super::folders::restart_line(ctx, &h["tmux"]["started_by"]) {
                ctx.say(format!("folders:    {line}"));
            }
        }
        None => {
            ctx.say(format!("terminumd:  not running — {}", launch::describe(ctx, None, None)));
            ctx.say(format!("            log: {}", ctx.shown(&ctx.log())));
        }
    }
    if ctx.systemd && !ctx.direct && let Some(hint) = super::systemd::linger_hint(ctx) {
        ctx.say(format!("service:    {hint}"));
    }
    if !ctx.direct && !ctx.systemd && let Some(held) = launch::plist_port(ctx) {
        let wanted = launch::port(ctx);
        if wanted != held {
            ctx.say(format!("port:       settings.json says {wanted}, launchd holds {held}: run terminum restart"));
        }
    }
    let link = |n| install::target(ctx, n).unwrap_or_else(|| "-".into());
    ctx.say(format!("builds:     current {}, previous {}", link("current"), link("previous")));
}

// ── open, password, signout ─────────────────────────────────────────────────

/// `terminum open`: the page in this Mac's browser; it signs in with the password.
pub fn open_page(ctx: &Ctx, print_only: bool) -> Result<(), String> {
    let h = launch::health(ctx).ok_or_else(|| format!("{} (see: terminum status)", launch::describe(ctx, None, None)))?;
    if h["password"]["set"].as_bool() != Some(true) {
        return Err(match h["password"]["error"].as_str().filter(|e| !e.is_empty()) {
            Some(e) => format!("No browser can sign in: {e}"),
            None => "No password is set yet, so no browser can sign in: run terminum password, then terminum open".into(),
        });
    }
    let url = page_url(&h["port"]);
    ctx.say(format!("{url} (sign in with your password)"));
    if !has_desktop(ctx) {
        ctx.say(tunnel_hint(&h["port"]));
    } else if !print_only {
        let _ = std::process::Command::new(&ctx.opener).arg(&url).status();
    }
    Ok(())
}

pub fn signout(ctx: &Ctx) -> Result<(), String> {
    checked(ctx, "POST", "/signout", None)?;
    ctx.say("Signed out every browser; each signs in again with the password.");
    Ok(())
}

fn ask_password(ctx: &Ctx) -> Result<String, String> {
    let pw = match ctx.var("TM_WEB_PASSWORD") {
        Some(pw) => pw.to_string(),
        None => {
            let pw = super::term::read_secret("Password for terminum's page (8 or more characters): ")?;
            if super::term::read_secret("Again: ")? != pw {
                return Err("The two passwords differ; nothing changed.".into());
            }
            pw
        }
    };
    if pw.chars().count() < MIN_PASSWORD {
        return Err(format!("The password needs at least {MIN_PASSWORD} characters; nothing changed."));
    }
    Ok(pw)
}

/// `terminum password`: every browser, here and on other devices, signs in with it.
pub fn password(ctx: &Ctx) -> Result<(), String> {
    checked(ctx, "POST", "/password", Some(json!({"password": ask_password(ctx)?})))?;
    ctx.say("Password set; browsers signed in before sign in again with it. Open the page: terminum open");
    let port = launch::health(ctx).map(|h| h["port"].clone()).unwrap_or(Value::Null);
    ctx.say(serve_hint(&port));
    Ok(())
}

// ── hosts ───────────────────────────────────────────────────────────────────

/// A word as a POSIX shell reads it back (Python's shlex.quote).
pub fn quote(word: &str) -> String {
    let safe = |c: char| c.is_alphanumeric() || "_@%+=:,./-".contains(c);
    if !word.is_empty() && word.chars().all(safe) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

fn join(words: &[String]) -> String {
    words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

pub fn hosts_list(ctx: &Ctx) -> Result<(), String> {
    let answer = checked(ctx, "GET", "/hosts", None)?;
    let hosts = answer["hosts"].as_array().cloned().unwrap_or_default();
    if hosts.is_empty() {
        ctx.say("No hosts yet: terminum hosts add '<ssh destination and options>' [--files]");
    }
    for x in hosts {
        let recipe = match x["ssh"].as_array() {
            Some(a) => join(&a.iter().map(text).collect::<Vec<_>>()),
            None => x["ssh"].as_str().filter(|s| !s.is_empty()).unwrap_or("?").to_string(),
        };
        let files = x["files"].as_str().filter(|f| !f.is_empty()).unwrap_or("not enabled");
        let helper = x["helper"].as_str().filter(|h| !h.is_empty()).map(|h| format!(", helper {h}")).unwrap_or_default();
        let why = x["why"].as_str().filter(|w| !w.is_empty()).map(|w| format!(" — {w}")).unwrap_or_default();
        let agents = x["agents"].as_str().filter(|a| !a.is_empty()).unwrap_or("off");
        ctx.say(format!("{}: {recipe}  {}, files {files}{helper}, agents {agents}{why}", x["name"].as_str().unwrap_or("?"), x["state"].as_str().unwrap_or("?")));
    }
    Ok(())
}

/// `hosts add [<name>] '<recipe>' [--files]`: no name, terminumd names it after the destination.
pub fn hosts_add(ctx: &Ctx, name: Option<&str>, recipe: &[String], files: bool) -> Result<(), String> {
    if recipe.is_empty() {
        return Err("usage: terminum hosts add [<name>] '<ssh destination and options>' [--files]".into());
    }
    let text = if recipe.len() == 1 { recipe[0].clone() } else { join(recipe) };
    let answer = checked(ctx, "POST", "/hosts/add", Some(json!({"name": name.unwrap_or(""), "recipe": text, "files": files})))?;
    let added = answer["name"].as_str().or(name).unwrap_or("the host");
    let then = if files { "; its files are set up once it attaches" } else { "" };
    ctx.say(answer["message"].as_str().map_or_else(|| format!("Added {added}; it attaches within seconds (terminum hosts){then}"), str::to_string));
    Ok(())
}

pub fn hosts_action(ctx: &Ctx, action: &str, name: &str) -> Result<(), String> {
    if action == "agents-on" {
        ctx.say(format!("Enabling coding agents' progress on {name}… (copying the helper and installing)"));
    }
    let answer = waited(ctx, "POST", &format!("/hosts/{action}"), Some(json!({"name": name})), host_budget(action))?;
    ctx.say(answer["message"].as_str().map_or_else(|| format!("{name}: {action} done"), str::to_string));
    Ok(())
}

/// `terminum tmux`: the attached sessions of the user's own tmux and their state (AC-89).
pub fn tmux_list(ctx: &Ctx) -> Result<(), String> {
    let answer = checked(ctx, "GET", "/tmux", None)?;
    let list = answer["attached"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        ctx.say("No session of your tmux is attached: terminum tmux attach <session>");
    }
    for x in list {
        let on = match (x["host"].as_str(), x["socket"].as_str().unwrap_or("default")) {
            (Some(h), _) => format!(" (on {h})"),
            (None, "default") => String::new(),
            (None, socket) => format!(" (tmux -L {socket})"),
        };
        ctx.say(format!("{}{on}: {}", x["session"].as_str().unwrap_or("?"), x["state"].as_str().unwrap_or("?")));
    }
    Ok(())
}

/// `flag`: "socket" or "host" (empty: plain `tmux` here).
pub fn tmux_action(ctx: &Ctx, action: &str, session: &str, flag: &str, value: &str) -> Result<(), String> {
    let mut body = json!({"session": session});
    if !flag.is_empty() {
        body[flag] = json!(value);
    }
    checked(ctx, "POST", &format!("/tmux/{action}"), Some(body))?;
    ctx.say(if action == "attach" { format!("Attached {session}: it shows in the list while it runs") } else { format!("Detached {session}: it goes on running in your tmux") });
    Ok(())
}

// ── import, logs ────────────────────────────────────────────────────────────

/// terminumd holds its state in memory and would write it over the imported files: it is
/// stopped while they are written (under the install lock) and started again.
pub fn import_state(ctx: &Ctx) -> Result<(), String> {
    let _lock = install::lock(ctx)?;
    let h = launch::health(ctx);
    let stopped = |e: String| format!("terminumd could not be stopped or started: {e}");
    if h.is_some() {
        launch::stop(ctx).map_err(stopped)?;
    }
    let lines = importer::run(&ctx.home, &ctx.app_dir, &ctx.root.join("settings.json"));
    if h.is_some() {
        launch::start(ctx).map_err(stopped)?;
    }
    for line in lines.map_err(|e| format!("Not imported: {e}"))? {
        ctx.say(line);
    }
    if let Some(h) = h && !launch::wait_healthy(ctx, h["build"].as_str()) {
        return Err(format!("Imported, but terminumd did not come back: {}", launch::describe(ctx, None, None)));
    }
    Ok(())
}

pub fn logs(ctx: &Ctx, lines: u64, follow: bool) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    if !ctx.log().is_file() {
        return Err(format!("No log yet: {}", ctx.shown(&ctx.log())));
    }
    let mut cmd = std::process::Command::new("/usr/bin/tail");
    cmd.arg("-n").arg(lines.to_string());
    if follow {
        cmd.arg("-f");
    }
    Err(format!("tail: {}", cmd.arg(ctx.log()).exec()))
}
