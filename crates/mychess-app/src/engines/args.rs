//! Command line arguments as one line of text, without a shell (D428).
//!
//! Rules of [`split_args`]: spaces, tabs and line breaks separate arguments;
//! `"..."` and `'...'` group (single quotes take everything literally; inside
//! double quotes only `\\` and `\"` are escapes); outside quotes `\` escapes a
//! following `\`, `"`, `'` or whitespace; a `\` before anything else is kept,
//! so Windows paths such as `C:\engines\sf` survive. Quoted parts glue to the
//! text next to them (`a"b c"d` is one argument) and `""` is an empty argument.

use super::error::ArgsError;

/// Splits `text` into arguments.
///
/// ```
/// use mychess_app::engines::split_args;
///
/// assert_eq!(split_args(r#"-t 4 "my file""#).unwrap(), ["-t", "4", "my file"]);
/// assert!(split_args("'open").is_err());
/// ```
pub fn split_args(text: &str) -> Result<Vec<String>, ArgsError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    args.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => current.push(c),
                        None => return Err(ArgsError::UnclosedQuote),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek() {
                            Some(&next @ ('\\' | '"')) => {
                                current.push(next);
                                chars.next();
                            }
                            _ => current.push('\\'),
                        },
                        Some(c) => current.push(c),
                        None => return Err(ArgsError::UnclosedQuote),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.peek() {
                    Some(&next) if matches!(next, '\\' | '"' | '\'') || next.is_whitespace() => {
                        current.push(next);
                        chars.next();
                    }
                    _ => current.push('\\'),
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        args.push(current);
    }
    Ok(args)
}

/// Whether `arg` must be quoted to survive [`split_args`].
fn needs_quotes(arg: &str) -> bool {
    if arg.is_empty() {
        return true;
    }
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() || c == '"' || c == '\'' {
            return true;
        }
        // A backslash that is last would escape the separator that follows.
        if c == '\\'
            && chars
                .peek()
                .is_none_or(|n| matches!(n, '\\' | '"' | '\'') || n.is_whitespace())
        {
            return true;
        }
    }
    false
}

/// Joins arguments into one line that [`split_args`] reads back unchanged.
///
/// ```
/// use mychess_app::engines::{join_args, split_args};
///
/// let args = vec!["-x".to_owned(), "a b".to_owned()];
/// assert_eq!(join_args(&args), r#"-x "a b""#);
/// assert_eq!(split_args(&join_args(&args)).unwrap(), args);
/// ```
pub fn join_args(args: &[String]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if !needs_quotes(arg) {
            out.push_str(arg);
            continue;
        }
        out.push('"');
        let mut chars = arg.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' if chars.peek().is_none_or(|n| matches!(n, '\\' | '"')) => {
                    out.push_str("\\\\");
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn plain_and_empty() {
        assert_eq!(split_args("").unwrap(), Vec::<String>::new());
        assert_eq!(split_args("  \t\n ").unwrap(), Vec::<String>::new());
        assert_eq!(split_args("a  b\tc").unwrap(), s(&["a", "b", "c"]));
    }

    #[test]
    fn quotes_and_escapes() {
        assert_eq!(split_args(r#""a b" 'c d'"#).unwrap(), s(&["a b", "c d"]));
        assert_eq!(split_args(r#"a"b c"d"#).unwrap(), s(&["ab cd"]));
        assert_eq!(split_args(r#""" x"#).unwrap(), s(&["", "x"]));
        assert_eq!(split_args(r#""a\"b\\c""#).unwrap(), s(&[r#"a"b\c"#]));
        assert_eq!(split_args(r"a\ b").unwrap(), s(&["a b"]));
        assert_eq!(split_args(r"'a\b'").unwrap(), s(&[r"a\b"]));
    }

    #[test]
    fn windows_paths_survive() {
        assert_eq!(
            split_args(r"C:\engines\sf.exe").unwrap(),
            s(&[r"C:\engines\sf.exe"])
        );
        assert_eq!(split_args(r"trailing\").unwrap(), s(&[r"trailing\"]));
    }

    #[test]
    fn unclosed_quotes_are_errors() {
        assert_eq!(split_args("\"abc"), Err(ArgsError::UnclosedQuote));
        assert_eq!(split_args("x 'abc"), Err(ArgsError::UnclosedQuote));
    }

    #[test]
    fn unicode() {
        assert_eq!(split_args("\"é ü\" ñ").unwrap(), s(&["é ü", "ñ"]));
    }

    proptest! {
        #[test]
        fn join_then_split_is_identity(args in proptest::collection::vec(".{0,12}", 0..6)) {
            let line = join_args(&args);
            prop_assert_eq!(split_args(&line).unwrap(), args);
        }

        #[test]
        fn split_never_panics_and_is_stable(text in ".{0,40}") {
            if let Ok(args) = split_args(&text) {
                let once = join_args(&args);
                let again = join_args(&split_args(&once).unwrap());
                prop_assert_eq!(once, again);
            }
        }
    }
}
