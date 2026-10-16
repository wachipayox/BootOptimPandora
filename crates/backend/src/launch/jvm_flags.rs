//! Splitting of the player's free-form JVM flags / wrapper command into arguments.
//!
//! POSIX shell rules treat a backslash as an escape, so on Windows `C:\games\flags.txt` used to
//! arrive at the JVM as `C:gamesflags.txt`. Windows paths are common in JVM flags (`-Xlog`,
//! `-XX:AOTCache`, `-javaagent`), so on Windows a backslash is a plain character and only quotes
//! group words. On other platforms the shell rules stay as they were.

/// Splits `flags` into arguments. Never fails: an unterminated quote falls back to splitting on
/// whitespace, as before.
pub(crate) fn split(flags: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        split_literal_backslashes(flags)
    }
    #[cfg(not(windows))]
    {
        shell_words::split(flags).unwrap_or_else(|_| flags.split_whitespace().map(str::to_owned).collect())
    }
}

/// Whitespace-separated words; `"..."` and `'...'` group a word (the quotes are removed);
/// a backslash is always literal. `None` on an unterminated quote.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn try_split_literal_backslashes(flags: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for ch in flags.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                in_word = true;
            },
            None if ch.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            },
            None => {
                current.push(ch);
                in_word = true;
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if in_word {
        words.push(current);
    }
    Some(words)
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn split_literal_backslashes(flags: &str) -> Vec<String> {
    try_split_literal_backslashes(flags).unwrap_or_else(|| flags.split_whitespace().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backslashes_in_windows_paths_are_kept() {
        assert_eq!(
            split_literal_backslashes(r"-Xlog:class+load:file=C:\BootOptimBench\logs\x.log -XX:+UseG1GC"),
            vec![r"-Xlog:class+load:file=C:\BootOptimBench\logs\x.log", "-XX:+UseG1GC"]
        );
    }

    #[test]
    fn quotes_group_words_and_are_removed() {
        assert_eq!(
            split_literal_backslashes(r#"-Dpath="C:\My Games\a b" '-Dother=x y'"#),
            vec![r"-Dpath=C:\My Games\a b", "-Dother=x y"]
        );
        assert_eq!(split_literal_backslashes(r#"a "" b"#), vec!["a", "", "b"]);
    }

    #[test]
    fn unterminated_quote_falls_back_to_whitespace_split() {
        assert!(try_split_literal_backslashes(r#"-Da="b c"#).is_none());
        assert_eq!(split_literal_backslashes(r#"-Da="b c"#), vec![r#"-Da="b"#, r#"c"#]);
    }

    #[test]
    fn empty_and_blank_input_give_no_words() {
        assert!(split_literal_backslashes("").is_empty());
        assert!(split_literal_backslashes("  \t ").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_split_uses_literal_backslashes() {
        assert_eq!(split(r"-XX:AOTCache=C:\a\b.aot"), vec![r"-XX:AOTCache=C:\a\b.aot"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_split_keeps_shell_rules() {
        assert_eq!(split(r"-Da=b\ c"), vec!["-Da=b c"]);
    }
}
