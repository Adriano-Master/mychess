// Derived from Scid vs PC 4.27, src/misc.cpp (strAlphaContains) and the
// name matching of `sc_search_header` in src/tkscid.cpp. Copyright (c)
// 1999-2002 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Matching of names against a search text (decisions D330 and D331).
//!
//! - Plain text: the text is a substring of the name, ignoring case and the
//!   space character (U+0020) on both sides. Empty or all-space text means
//!   "no restriction".
//! - Text between double quotes: a Tcl glob (`*`, `?`, `[a-z]`, `\x`) over
//!   the whole name, case-sensitive, by Unicode character.

/// Simple case folding of a whole string: each character is lowercased when
/// that gives a single character, and kept otherwise.
pub(crate) fn fold_case(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// A compiled name criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NamePattern {
    /// No restriction.
    Any,
    /// Folded text without spaces, to find inside the folded name.
    Substring(String),
    /// A glob over the whole name.
    Glob(Vec<GlobToken>),
}

impl NamePattern {
    /// Compiles the text typed by the user.
    pub(crate) fn new(text: &str) -> NamePattern {
        let quoted = text.chars().count() >= 2 && text.starts_with('"') && text.ends_with('"');
        if quoted {
            let inner = &text[1..text.len() - 1];
            return NamePattern::Glob(compile_glob(inner));
        }
        let needle: String = fold_case(text).chars().filter(|c| *c != ' ').collect();
        if needle.is_empty() {
            NamePattern::Any
        } else {
            NamePattern::Substring(needle)
        }
    }

    /// Whether the pattern imposes no restriction.
    pub(crate) fn is_any(&self) -> bool {
        matches!(self, NamePattern::Any)
    }

    /// Whether `name` satisfies the pattern.
    pub(crate) fn matches(&self, name: &str) -> bool {
        match self {
            NamePattern::Any => true,
            NamePattern::Substring(needle) => {
                let hay: String = fold_case(name).chars().filter(|c| *c != ' ').collect();
                hay.contains(needle.as_str())
            }
            NamePattern::Glob(tokens) => {
                let chars: Vec<char> = name.chars().collect();
                glob_match(tokens, &chars)
            }
        }
    }
}

/// One element of a compiled glob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GlobToken {
    /// `*`: any run of characters.
    Star,
    /// `?`: any single character.
    Any,
    /// A literal character.
    Lit(char),
    /// `[...]`: inclusive ranges (a single character is a range of one).
    Set(Vec<(char, char)>),
}

fn compile_glob(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            '*' => {
                // Consecutive stars are one.
                if out.last() != Some(&GlobToken::Star) {
                    out.push(GlobToken::Star);
                }
            }
            '?' => out.push(GlobToken::Any),
            '\\' => match chars.get(i) {
                Some(&next) => {
                    out.push(GlobToken::Lit(next));
                    i += 1;
                }
                None => out.push(GlobToken::Lit('\\')),
            },
            '[' => {
                let mut set = Vec::new();
                // As in Tcl, an unterminated set runs to the end of the
                // pattern.
                while let Some(&s) = chars.get(i) {
                    i += 1;
                    if s == ']' {
                        break;
                    }
                    let lo = if s == '\\' {
                        let escaped = chars.get(i).copied().unwrap_or('\\');
                        i += usize::from(chars.get(i).is_some());
                        escaped
                    } else {
                        s
                    };
                    if chars.get(i) == Some(&'-') && chars.get(i + 1).is_some_and(|n| *n != ']') {
                        let mut hi = chars.get(i + 1).copied().unwrap_or(lo);
                        i += 2;
                        if hi == '\\' {
                            hi = chars.get(i).copied().unwrap_or('\\');
                            i += usize::from(chars.get(i).is_some());
                        }
                        set.push((lo.min(hi), lo.max(hi)));
                    } else {
                        set.push((lo, lo));
                    }
                }
                out.push(GlobToken::Set(set));
            }
            c => out.push(GlobToken::Lit(c)),
        }
    }
    out
}

fn token_accepts(token: &GlobToken, c: char) -> bool {
    match token {
        GlobToken::Star => false,
        GlobToken::Any => true,
        GlobToken::Lit(l) => *l == c,
        GlobToken::Set(ranges) => ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)),
    }
}

/// Matches the whole of `text`; backtracks only to the last star, so it runs
/// in `O(tokens x chars)`.
fn glob_match(tokens: &[GlobToken], text: &[char]) -> bool {
    let (mut t, mut s) = (0, 0);
    // Where to resume after the last star: (token after it, text position).
    let mut resume: Option<(usize, usize)> = None;
    loop {
        match tokens.get(t) {
            Some(GlobToken::Star) => {
                t += 1;
                resume = Some((t, s));
                continue;
            }
            Some(token) => {
                if let Some(&c) = text.get(s)
                    && token_accepts(token, c)
                {
                    t += 1;
                    s += 1;
                    continue;
                }
            }
            None => {
                if s == text.len() {
                    return true;
                }
            }
        }
        // Mismatch: let the last star swallow one more character.
        match resume {
            Some((rt, rs)) if rs < text.len() => {
                resume = Some((rt, rs + 1));
                t = rt;
                s = rs + 1;
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, name: &str) -> bool {
        NamePattern::new(pattern).matches(name)
    }

    #[test]
    fn plain_text_ignores_case_and_spaces() {
        assert!(m("th,j", "Smith, John"));
        assert!(m("SMITH,JOHN", "Smith, John"));
        assert!(m("smith, john", "Smith,John"));
        assert!(!m("smyth", "Smith, John"));
    }

    #[test]
    fn trailing_spaces_still_match() {
        assert!(m("smith ", "Smith, John"));
        assert!(m("   ", "anything"));
        assert!(NamePattern::new("  ").is_any());
        assert!(NamePattern::new("").is_any());
    }

    #[test]
    fn folding_is_unicode_aware() {
        assert!(m("\u{e9}", "\u{c9}douard"));
    }

    #[test]
    fn quoted_text_is_a_whole_name_glob() {
        assert!(m("\"*USA\"", "Dallas USA"));
        assert!(!m("\"*USA\"", "Lausanne SUI"));
        assert!(m("\"1\"", "1"));
        assert!(!m("\"1\"", "10"));
        assert!(!m("\"1\"", "21"));
    }

    #[test]
    fn glob_is_case_sensitive_and_unicode_aware() {
        assert!(!m("\"*usa\"", "Dallas USA"));
        assert!(m("\"Fr?d\"", "Fr\u{e9}d"));
        assert!(!m("\"[a-c]ob\"", "Bob"));
        assert!(m("\"[a-cB]ob\"", "Bob"));
        assert!(m("\"a\\*b\"", "a*b"));
        assert!(!m("\"a\\*b\"", "axb"));
        assert!(m("\"a*b*c\"", "aXXbYYc"));
        assert!(!m("\"a*b*c\"", "aXXbYY"));
    }

    #[test]
    fn unclosed_quote_is_plain_text() {
        assert!(m("\"smith", "\"Smith"));
        assert!(!m("\"smith", "Smith"));
    }
}
