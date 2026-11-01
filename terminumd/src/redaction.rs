// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! Redact stored text without interpreting or executing shell syntax.

/// Names like a secret's, compared without case, `-` and `_` (`X-API-Key`, `api_key`).
const SECRET_WORD: [&str; 5] = ["token", "secret", "password", "passwd", "apikey"];
const SECRET_SHAPE: [&str; 9] = ["sk-", "ghp_", "gho_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "AKIA", "eyJ"];

/// A name like a secret's: a part of it (between '-', '_', quotes and other signs) holds a secret word (`MAX_TOKENS`,
/// `secretKey`), or parts side by side spell one whole (`X-API-Key`, `api_key`); never parts
/// that only run into one (`go_to_kennel`).
fn secretish(w: &str) -> bool {
    let parts = parts(w);
    SECRET_WORD.iter().any(|s| parts.iter().any(|p| p.contains(s)) || (0..parts.len()).any(|i| {
        let mut joined = String::new();
        parts[i..].iter().any(|p| { joined.push_str(p); joined == *s })
    }))
}

/// A key without quotes before a ':' in a word (`token:abc`) is a secret's only when it ends in
/// a secret word: an image's `secrets-manager:latest` is not one.
fn secret_at_end(w: &str) -> bool {
    let whole: String = parts(w).concat();
    SECRET_WORD.iter().any(|s| whole.ends_with(s))
}

fn parts(w: &str) -> Vec<String> {
    w.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty()).map(str::to_lowercase).collect()
}

/// `text` with secrets' values left out: `NAME=value`, `NAME = value`, `NAME: value`,
/// `"name":"value"` inside a word (compact JSON), `--name value` for a name like a secret's,
/// `Bearer value`, words of a known secret's shape and private key blocks.
pub fn redact(text: &str) -> String {
    let text = pem_out(text);
    let mut out = String::with_capacity(text.len());
    let (mut hide_next, mut named) = (false, false);
    let mut at = 0;
    while at < text.len() {
        let piece = text[at..].split_inclusive(char::is_whitespace).next().expect("remaining text");
        let word = piece.trim_end_matches(char::is_whitespace);
        let bare = word.trim_matches(|c: char| "'\"`(),;".contains(c));
        if word.is_empty() {
            out.push_str(piece);
            at += piece.len();
            continue;
        }
        // a value hidden: from where it starts to its end, its quotes and escapes with it
        let value = hide_next && (!bare.starts_with('-') || word.starts_with(['\'', '"']));
        // `NAME = value`: the "=" (Make's ":=", "?=", "+=") a word of its own, or the value's
        // first character; never "==" nor "=>"
        let named_before = std::mem::take(&mut named) && !value;
        let spaced = named_before && (matches!(word, ":=" | "?=" | "+=")
            || (word.starts_with('=') && !word.starts_with("==") && !word.starts_with("=>")));
        if spaced && word.starts_with('=') && word.len() > 1 {
            (hide_next, at) = (false, value_end(&text, at + 1, None));
            out.push_str("=[redacted]");
            continue;
        }
        // a key before a ':' in the same word (`{"token":"ab=="}`) is `keyed`'s: its value may hold '='
        let assigned = word.split_once('=').filter(|(k, v)| !v.is_empty() && !k.contains(':') && secretish(k)).map(|(k, _)| k);
        if let Some(k) = assigned.filter(|_| !value) {
            let opened = k.chars().next().filter(|q| matches!(q, '\'' | '"') && !k[1..].contains(*q));
            (hide_next, at) = (false, value_end(&text, at + k.len() + 1, opened));
            out.push_str(&format!("{k}=[redacted]"));
            continue;
        }
        if value || SECRET_SHAPE.iter().any(|s| bare.starts_with(s) && bare.len() >= s.len() + 12) {
            (hide_next, at) = (false, value_end(&text, at, None));
            out.push_str("[redacted]");
            continue;
        }
        if let Some(colon) = keyed(word) {
            (hide_next, at) = (false, json_value_end(&text, at + colon + 1));
            out.push_str(&word[..=colon]);
            out.push_str("[redacted]");
            continue;
        }
        let key = bare.trim_end_matches(['=', ':']).trim_matches('"');
        hide_next = spaced || (bare.starts_with("--") && !word.contains('=') && secretish(bare))
            || ((bare.ends_with(':') || word.ends_with('=')) && secretish(key)) || bare.eq_ignore_ascii_case("bearer");
        named = !hide_next && !word.contains(['=', ':']) && secretish(bare);
        out.push_str(piece);
        at += piece.len();
    }
    out
}

/// End of one secret value starting at `start`, including quoted/escaped whitespace; `inside`:
/// the quote its word opened before the value (`'API_KEY=a b'`). Only a quote at the value's
/// start or right after a closed one opens: text before the value and a mid-word apostrophe
/// (`it's`, `don't`) are prose, never a quote that hides what follows. An open quote hides to EOF.
fn value_end(text: &str, start: usize, inside: Option<char>) -> usize {
    let (mut quote, mut ansi, mut escaped) = (inside, false, false);
    let (mut may_open, mut dollar) = (inside.is_none(), false);
    for (offset, c) in text[start..].char_indices() {
        if escaped { escaped = false; continue; }
        if let Some(q) = quote {
            if c == '\\' && (q == '"' || ansi) { escaped = true; }
            else if c == q { (quote, ansi, may_open) = (None, false, true); }
            continue;
        }
        if c.is_whitespace() { return start + offset; }
        if c == '\\' { (escaped, may_open, dollar) = (true, false, false); continue; }
        if (may_open || dollar) && matches!(c, '\'' | '"') {
            (quote, ansi, may_open, dollar) = (Some(c), dollar && c == '\'', false, false);
            continue;
        }
        (dollar, may_open) = (may_open && c == '$', false);
    }
    text.len()
}

/// The `:` inside `word` after a key named like a secret's, with its value in the same word:
/// `{"user":"alex","password":"a b"}`, `token:abc`.
fn keyed(word: &str) -> Option<usize> {
    let b = word.as_bytes();
    // never a path's "::" (`std::token::parse`)
    word.char_indices().filter(|&(i, c)| c == ':' && i + 1 < b.len() && b[i + 1] != b':' && (i == 0 || b[i - 1] != b':')).map(|(i, _)| i).find(|&i| {
        let key = word[..i].rsplit(['{', ',', '[', '(']).next().unwrap_or("");
        let bare = key.trim_matches(|c: char| "'\"` ".contains(c));
        if key.starts_with(['"', '\'']) { secretish(bare) } else { secret_at_end(bare) }
    })
}

/// End of a value after a key inside a word: a quoted value through its closing quote (to the
/// end of the text when it never closes), else to the first `,`, `}`, `]` or whitespace.
fn json_value_end(text: &str, start: usize) -> usize {
    let rest = &text[start..];
    if let Some(q) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let mut escaped = false;
        for (i, c) in rest.char_indices().skip(1) {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                _ if c == q => return start + i + 1,
                _ => {}
            }
        }
        return text.len();
    }
    start + rest.find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']')).unwrap_or(rest.len())
}

/// `text` with every "-----BEGIN … PRIVATE KEY-----" block replaced by "[redacted]".
fn pem_out(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("-----BEGIN ") {
        let Some(end) = rest[i..].find("PRIVATE KEY-----").map(|e| i + e) else { break };
        let close = rest[end + 16..].find("PRIVATE KEY-----").map_or(rest.len(), |e| end + 16 + e + 16);
        out.push_str(&rest[..i]);
        out.push_str("[redacted]");
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_values_preserve_following_nonsecret_arguments() {
        for source in [
            "tool --password 'alpha beta' tail",
            "tool '--password' \"alpha beta\" tail",
            "tool --password \"alpha\\\" beta\" tail",
            "tool --password alpha\\ beta tail",
            "tool --password '-alpha beta' tail",
            "tool --password $'alpha\\' beta' tail",
            "tool --password $'alpha\\\\ beta' tail",
        ] {
            assert_eq!(redact(source).split("[redacted]").last(), Some(" tail"), "{source}");
            assert!(!redact(source).contains("alpha") && !redact(source).contains("beta"));
        }
        assert_eq!(redact("PASSWORD='alpha beta' tool"), "PASSWORD=[redacted] tool");
        assert_eq!(redact("PASSWORD=$'alpha\\' beta' tool"), "PASSWORD=[redacted] tool");
        assert_eq!(redact("tool --password 'alpha beta"), "tool --password [redacted]");
        assert_eq!(redact("curl -H 'Authorization: Bearer alpha' tail"), "curl -H 'Authorization: Bearer [redacted] tail");
        assert_eq!(redact("tool 'ordinary quoted words' tail"), "tool 'ordinary quoted words' tail");
        assert_eq!(redact("tool --password --user alex"), "tool --password --user alex");
    }

    #[test]
    fn apostrophes_in_prose_never_hide_the_rest_of_a_prompt() {
        for (source, kept) in [
            ("Fix the token: it's broken. Then rename the module", "Fix the token: [redacted] broken. Then rename the module"),
            ("We don't send a bearer token yet, so add one", "We don't send a bearer [redacted] yet, so add one"),
            ("don't print password: hunter2 and then run make test", "don't print password: [redacted] and then run make test"),
            ("it's set: API_TOKEN=abc123 and the user's tests pass", "it's set: API_TOKEN=[redacted] and the user's tests pass"),
            ("run \"tool --password alpha\" then report", "run \"tool --password [redacted] then report"),
        ] {
            assert_eq!(redact(source), kept, "{source}");
        }
    }

    #[test]
    fn a_value_quoted_from_before_its_name_or_in_parts_stays_hidden() {
        assert_eq!(redact("env 'API_KEY=alpha beta' tool"), "env 'API_KEY=[redacted] tool");
        assert_eq!(redact("env \"API_KEY=alpha \\\" beta\" tool"), "env \"API_KEY=[redacted] tool");
        assert_eq!(redact("tool --password 'alpha beta'\"gamma delta\" tail"), "tool --password [redacted] tail");
        assert_eq!(redact("tool --password $'alpha beta' tail"), "tool --password [redacted] tail");
    }

    #[test]
    fn spaced_assignments_hyphenated_names_and_compact_json() {
        for (source, kept) in [
            ("export API_KEY = \"abc def\" then", "export API_KEY = [redacted] then"),
            ("API_KEY =abc then", "API_KEY =[redacted] then"),
            ("API_KEY= abc then", "API_KEY= [redacted] then"),
            ("curl -H 'X-API-Key: abc' tail", "curl -H 'X-API-Key: [redacted] tail"),
            ("x-auth-token: abc tail", "x-auth-token: [redacted] tail"),
            ("--api-key abc tail", "--api-key [redacted] tail"),
            (r#"post {"password":"a b"} tail"#, r#"post {"password":[redacted]} tail"#),
            (r#"{"user":"alex","api_key":"x\"y","n":1}"#, r#"{"user":"alex","api_key":[redacted],"n":1}"#),
            (r#"{"token":12345,"user":"alex"}"#, r#"{"token":[redacted],"user":"alex"}"#),
            ("token:abc tail", "token:[redacted] tail"),
            (r#"{"token":"QWxhZGRpbg=="} tail"#, r#"{"token":[redacted]} tail"#),
            ("token:abc=def tail", "token:[redacted] tail"),
            (r#"{"password":"p=w","n":1}"#, r#"{"password":[redacted],"n":1}"#),
            ("TOKEN := abc tail", "TOKEN := [redacted] tail"),
            ("API_KEY ?= abc tail", "API_KEY ?= [redacted] tail"),
            ("client_secret:abc tail", "client_secret:[redacted] tail"),
            (r#"{"secretKey":"abc"}"#, r#"{"secretKey":[redacted]}"#),
            ("MAX_TOKENS=abc tail", "MAX_TOKENS=[redacted] tail"),
            (r#"{"password":"never closed"#, r#"{"password":[redacted]"#),
        ] {
            assert_eq!(redact(source), kept, "{source}");
        }
        for plain in ["a = b and x == y", "the token is new = fine", "https://example.com:8080/a", r#"{"user":"alex","n":1}"#, "token == x", "token => token.trim()", "cargo test std::token::parse",
                      "docker pull secrets-manager:latest", "go_to_kennel=1", "tokens:5"] {
            assert_eq!(redact(plain), plain, "{plain}");
        }
    }

    #[test]
    fn non_ascii_text_around_values_is_cut_on_character_boundaries() {
        // a quote opened inside the value's word is not followed (SECURITY.md: Sent history)
        assert_eq!(redact("é=x TOKEN=é'ü ö' tail"), "é=x TOKEN=[redacted] ö' tail");
        assert_eq!(redact("пароль --password 'ü ö' ещё"), "пароль --password [redacted] ещё");
        assert_eq!(redact("TOKEN=\"ü"), "TOKEN=[redacted]");
    }
}
