//! POSIX shell quoting.
//!
//! Every remote command is assembled as text and fed to `/bin/sh`, so quoting is the boundary
//! between "the user named a container `; rm -rf /`" and a security incident. Container names,
//! interface names, mount points and unit names all reach this function straight from remote
//! output, so it is the load-bearing piece of the transport.

/// Wrap `s` in single quotes so `/bin/sh` treats it as one literal argument.
///
/// Single quotes are absolute in POSIX sh — no expansion of any kind happens inside them — so the
/// only character needing care is `'` itself, which is emitted by closing the quote, escaping a
/// literal quote, and reopening.
///
/// ```
/// # use sg_transport::shell_quote;
/// assert_eq!(shell_quote("/proc/stat"), "'/proc/stat'");
/// assert_eq!(shell_quote("a b"), "'a b'");
/// assert_eq!(shell_quote("it's"), r#"'it'\''s'"#);
/// assert_eq!(shell_quote("; rm -rf /"), "'; rm -rf /'");
/// ```
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            // Close the quote, emit an escaped quote, reopen.
            out.push_str(r"'\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Quote and join an argv into a single command string.
pub fn shell_join<I, S>(argv: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    argv.into_iter()
        .map(|a| shell_quote(a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_empty_string_as_empty_argument() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn neutralises_shell_metacharacters() {
        for hostile in [
            "$(whoami)",
            "`whoami`",
            "a; rm -rf /",
            "a && b",
            "a | b",
            "a\nb",
            "*",
            "~/secret",
            "${HOME}",
            r"back\slash",
        ] {
            let quoted = shell_quote(hostile);
            assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
            // Nothing inside may terminate the quote except via the documented escape sequence.
            let inner = &quoted[1..quoted.len() - 1];
            assert!(
                !inner.contains('\''),
                "{hostile:?} leaked a bare quote: {quoted}"
            );
        }
    }

    #[test]
    fn escapes_embedded_quotes_so_the_shell_rejoins_them() {
        // sh sees: 'don' \' 't' -> concatenated into the single word: don't
        assert_eq!(shell_quote("don't"), r#"'don'\''t'"#);
        // A quote-only string still round-trips.
        assert_eq!(shell_quote("'"), r#"''\'''"#);
        assert_eq!(shell_quote("''"), r#"''\'''\'''"#);
    }

    #[test]
    fn joins_argv_with_each_element_quoted() {
        assert_eq!(
            shell_join(["cat", "--", "/proc/stat"]),
            "'cat' '--' '/proc/stat'"
        );
        assert_eq!(
            shell_join(["docker", "inspect", "a b"]),
            "'docker' 'inspect' 'a b'"
        );
    }
}
