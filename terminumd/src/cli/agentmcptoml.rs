// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! terminum's MCP block in Codex's `config.toml` (AC-97, AC-98): text edits between two marker
//! lines, no TOML parser; only the lines terminum wrote are added, replaced or taken out.

pub(super) const MCP_START: &str = "# >>> terminum: MCP server (terminum hooks install; terminum hooks remove takes it out)";
pub(super) const MCP_END: &str = "# <<< terminum";

/// `s` as a TOML basic string.
fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out + "\""
}

/// terminum's table of Codex's config.toml: `terminum mcp`, given the pane's tmux variables
/// (Codex clears an MCP server's environment), its tools run without asking.
fn mcp_table(program: &str) -> String {
    format!("[mcp_servers.terminum]\ncommand = {}\nargs = [\"mcp\"]\nenv_vars = [\"TMUX\", \"TMUX_PANE\"]\n\
             default_tools_approval_mode = \"approve\"\n", toml_str(program))
}

/// terminum's block of Codex's config.toml: its table between the markers.
pub fn codex_mcp_block(program: &str) -> String {
    format!("{MCP_START}\n{}{MCP_END}\n", mcp_table(program))
}

/// The lines of one block of terminum's that terminum did not write (a table or key Codex put
/// there, a comment), kept in place, and where terminum's table stands among them (Err: a key of
/// terminum's table changed by hand: it is not guessed at).
fn foreign(block: &str) -> Result<(Vec<&str>, usize), String> {
    let known = |t: &str| (t.starts_with("command = \"") && t.ends_with('"'))
        || ["args = [\"mcp\"]", "env_vars = [\"TMUX\", \"TMUX_PANE\"]", "default_tools_approval_mode = \"approve\""].contains(&t);
    let lines: Vec<&str> = block.split_inclusive('\n').collect();
    let (mut kept, mut at, mut ours) = (vec![], None, false);
    for &line in &lines[1..lines.len() - 1] {
        let t = line.trim();
        if t == "[mcp_servers.terminum]" && at.is_none() {
            (at, ours) = (Some(kept.len()), true);
            continue;
        }
        if t.starts_with('[') {
            ours = false;
        } else if ours && known(t) {
            continue;
        } else if ours && !t.is_empty() && !t.starts_with('#') {
            return Err(format!("terminum's block was changed by hand ({t}): nothing changed; put that line back as terminum wrote it, or edit the block yourself"));
        }
        kept.push(line);
    }
    Ok((kept, at.unwrap_or(0)))
}

/// terminum's blocks in `text` as byte ranges, start line through end line (Err: a marker unpaired).
pub(super) fn blocks(text: &str) -> Result<Vec<(usize, usize)>, String> {
    let (mut out, mut open, mut at) = (vec![], None, 0);
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with("# >>> terminum:") {
            if open.replace(at).is_some() {
                return Err("terminum's block starts twice: nothing changed".into());
            }
        } else if t == MCP_END {
            out.push((open.take().ok_or("terminum's block ends where none starts: nothing changed")?, at + line.len()));
        }
        at += line.len();
    }
    if open.is_some() { Err(format!("terminum's block has no \"{MCP_END}\" line: nothing changed")) } else { Ok(out) }
}

/// The line outside terminum's blocks that defines an `mcp_servers.terminum` of its own (a table,
/// a dotted key, a key of `[mcp_servers]`), or an inline `mcp_servers` a table cannot extend.
fn own_server(text: &str, ours: &[(usize, usize)]) -> Option<String> {
    let key = |k: &str| k.chars().filter(|c| !c.is_whitespace() && *c != '"' && *c != '\'').collect::<String>();
    let (mut table, mut at) = (String::new(), 0);
    for line in text.split_inclusive('\n') {
        let inside = ours.iter().any(|&(s, e)| (s..e).contains(&at));
        at += line.len();
        let t = line.trim();
        if inside || t.starts_with('#') {
            continue;
        }
        let path = if let Some(h) = t.strip_prefix('[') {
            table = key(h.trim_start_matches('[').split(']').next().unwrap_or(""));
            if table == "mcp_servers" { continue } else { table.clone() }
        } else if let Some((k, _)) = t.split_once('=') {
            if table.is_empty() { key(k) } else { format!("{table}.{}", key(k)) }
        } else {
            continue;
        };
        if path == "mcp_servers" || path == "mcp_servers.terminum" || path.starts_with("mcp_servers.terminum.") {
            return Some(t.to_string());
        }
    }
    None
}

/// Codex's config.toml with terminum's block: appended, or the first one's lines of terminum's
/// replaced in place and any other's taken out, others' lines inside kept (Err: a server named
/// terminum of the user's, a broken block, a line of terminum's changed by hand).
pub fn codex_mcp_add(text: Option<&str>, program: &str) -> Result<String, String> {
    let text = text.unwrap_or("");
    let found = blocks(text)?;
    if let Some(line) = own_server(text, &found) {
        return Err(format!("it has an MCP server named terminum of its own ({line}): nothing changed; take it out, then run terminum hooks install again"));
    }
    if found.is_empty() {
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        return Ok(format!("{text}{sep}{}", codex_mcp_block(program)));
    }
    edit_blocks(text, &found, |i, kept, at| if i > 0 { kept.concat() } else {
        format!("{MCP_START}\n{}{}{}{MCP_END}\n", kept[..at].concat(), mcp_table(program), kept[at..].concat())
    })
}

/// Codex's config.toml without terminum's lines (others' lines inside its block kept), the rest byte for byte.
pub fn codex_mcp_remove(text: &str) -> Result<String, String> {
    edit_blocks(text, &blocks(text)?, |_, kept, _| kept.concat())
}

/// `text` with block `i` of `found` given as `with(i, its foreign lines, where terminum's table stands)`.
fn edit_blocks(text: &str, found: &[(usize, usize)], with: impl Fn(usize, &[&str], usize) -> String) -> Result<String, String> {
    let (mut out, mut at) = (String::new(), 0);
    for (i, &(s, e)) in found.iter().enumerate() {
        let (kept, table) = foreign(&text[s..e])?;
        out.push_str(&text[at..s]);
        out.push_str(&with(i, &kept, table));
        at = e;
    }
    Ok(out + &text[at..])
}
