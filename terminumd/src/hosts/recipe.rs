// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! A host's ssh recipe (AC-81, AC-87): the destination and the options allowed with it,
//! checked before any shell or ssh sees them. Allowed: -p, -l, -i, -J (with a value), -4,
//! -6, -C. Refused: everything else, -o and -F included (a config file or an option can
//! hold ProxyCommand), a destination starting with "-", control characters, a command.

/// Options that take a value.
const WITH_VALUE: [&str; 4] = ["-p", "-l", "-i", "-J"];
const FLAGS: [&str; 3] = ["-4", "-6", "-C"];

fn plain(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_control())
}

fn destination_ok(s: &str) -> bool {
    !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || "@._:[]-".contains(c)) && !s.is_empty()
}

/// Words of a recipe as typed ("alex@devbox.example -p 2222"): split on blanks, single and
/// double quotes keep blanks in a value (an identity file's path).
pub fn words(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                started = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return Err("a quote is not closed".into());
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// The checked ssh arguments (options first, the destination last), or why not.
pub fn check(args: &[String]) -> Result<Vec<String>, String> {
    let mut opts = Vec::new();
    let mut dest: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if !plain(a) {
            return Err("control characters are not allowed".into());
        }
        if a.starts_with('-') {
            let option: String = a.chars().take(2).collect();
            if FLAGS.contains(&a.as_str()) {
                opts.push(a.clone());
            } else if WITH_VALUE.contains(&option.as_str()) {
                let value = if a.len() > 2 {
                    a[2..].to_string()
                } else {
                    i += 1;
                    args.get(i).cloned().ok_or_else(|| format!("{option} needs a value"))?
                };
                if !plain(&value) || value.starts_with('-') {
                    return Err(format!("{option} needs a value"));
                }
                if option == "-p" && !value.parse::<u16>().is_ok_and(|p| p > 0) {
                    return Err(format!("-p {value} is not a port"));
                }
                if matches!(option.as_str(), "-l" | "-J") && !value.chars().all(|c| c.is_ascii_alphanumeric() || "@._:[],-".contains(c)) {
                    return Err(format!("{option} {value} is not a user or host"));
                }
                opts.push(option);
                opts.push(value);
            } else {
                return Err(format!("option not allowed: {option}"));
            }
        } else if dest.is_some() {
            return Err("only a destination and options: no command".into());
        } else if !destination_ok(a) {
            return Err(format!("not a destination: {a}"));
        } else {
            dest = Some(a.clone());
        }
        i += 1;
    }
    let dest = dest.ok_or("a destination is needed (user@host)")?;
    opts.push(dest);
    Ok(opts)
}

/// ssh's arguments for a checked recipe: its options, then `--` and the destination, so
/// nothing after them can be read as an option; the remote command follows.
pub fn argv(recipe: &[String]) -> Vec<String> {
    let mut out = recipe.to_vec();
    if let Some(dest) = out.pop() {
        out.push("--".into());
        out.push(dest);
    }
    out
}

/// `c` given the recipe's arguments and `script` as its remote command, run by /bin/sh whatever
/// the host's login shell: zsh's nomatch aborted an unmatched glob (issue #8), fish and csh
/// read no POSIX sh (AC-83). Every script terminumd runs on a host goes through here.
pub fn with_command(c: &mut tokio::process::Command, recipe: &[String], script: &str) {
    c.args(argv(recipe)).arg(remote_command(script));
}

/// The remote command: `script` under /bin/sh.
pub fn remote_command(script: &str) -> String {
    format!("exec /bin/sh -c {}", sq(script))
}

/// `s` as one single-quoted /bin/sh word.
pub fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A recipe typed as one string, as after `ssh` in a shell (a leading "ssh" is dropped).
pub fn parse(text: &str) -> Result<Vec<String>, String> {
    let mut w = words(text)?;
    if w.len() > 1 && w[0] == "ssh" {
        w.remove(0);
    }
    check(&w)
}

/// The name a host gets when none is given: its destination's host, its first label unless
/// it is an IPv4 address ("alex@devbox.example" -> "devbox", "192.0.2.5" as is).
pub fn default_name(recipe: &[String]) -> Result<String, String> {
    let dest = recipe.last().map_or("", String::as_str);
    let host = dest.rsplit_once('@').map_or(dest, |(_, h)| h);
    let ipv4 = host.chars().all(|c| c.is_ascii_digit() || c == '.');
    let name = if ipv4 { host } else { host.split('.').next().unwrap_or("") };
    name_ok(name).map(|()| name.to_string()).map_err(|_| "give this host a name".to_string())
}

/// A host's name: letters, digits, `.`, `-`, `_`, at most 32 characters (it names a key,
/// a socket and a group).
pub fn name_ok(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 32 || name == "local" || !name.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)) || name.starts_with(['.', '-']) {
        return Err(format!("not a host name: {name:?} (letters, digits, . - _; not \"local\")"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn allowed_options_and_a_destination() {
        assert_eq!(parse("alex@devbox.example -p 2222").unwrap(), s(&["-p", "2222", "alex@devbox.example"]));
        assert_eq!(parse("-J bastion -C -i '/Users/alex/my key' alex@devbox.example").unwrap(), s(&["-J", "bastion", "-C", "-i", "/Users/alex/my key", "alex@devbox.example"]));
        assert_eq!(parse("-p2222 devbox").unwrap(), s(&["-p", "2222", "devbox"]));
    }

    #[test]
    fn everything_else_is_refused_with_why() {
        assert_eq!(parse("alex@h -o ProxyCommand=x").unwrap_err(), "option not allowed: -o");
        assert_eq!(parse("-F /tmp/cfg alex@h").unwrap_err(), "option not allowed: -F");
        assert_eq!(parse("-L 1:a:2 h").unwrap_err(), "option not allowed: -L");
        assert!(parse("h uname -a").unwrap_err().contains("no command"));
        assert!(parse("-p 0 h").is_err() && parse("-p x h").is_err());
        assert!(parse("").unwrap_err().contains("destination"));
        assert!(parse("'unclosed").is_err());
        assert!(check(&s(&["-p", "-oProxyCommand=x", "h"])).is_err(), "a value that is an option");
        assert!(check(&s(&["h\u{7}"])).is_err());
        assert!(check(&s(&["h;rm"])).unwrap_err().contains("not a destination"));
    }

    #[test]
    fn a_leading_ssh_is_dropped() {
        assert_eq!(parse("ssh alex@devbox.example -p 2222").unwrap(), s(&["-p", "2222", "alex@devbox.example"]));
        assert_eq!(parse("ssh").unwrap(), s(&["ssh"]), "a host named ssh");
        assert!(parse("ssh ssh ls").unwrap_err().contains("no command"));
    }

    #[test]
    fn a_default_name_is_the_destinations_host() {
        let name = |t: &str| default_name(&parse(t).unwrap());
        assert_eq!(name("alex@devbox.example -p 2222").unwrap(), "devbox");
        assert_eq!(name("box2").unwrap(), "box2");
        assert_eq!(name("-l alex vm-1.lan").unwrap(), "vm-1");
        assert_eq!(name("root@192.0.2.5").unwrap(), "192.0.2.5");
        assert_eq!(name("alex@[fe80::1]").unwrap_err(), "give this host a name");
        assert_eq!(name("local").unwrap_err(), "give this host a name");
    }

    #[test]
    fn host_names() {
        assert!(name_ok("devbox").is_ok() && name_ok("vm-1.lan").is_ok());
        for bad in ["", "local", "a b", "-x", "x/y", &"a".repeat(33)] {
            assert!(name_ok(bad).is_err(), "{bad}");
        }
    }
}
