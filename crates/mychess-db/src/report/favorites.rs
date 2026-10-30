// Scid: tcl/tools/optable.tcl 1156-1393 (favorites) and the file
// `reports.dat` it writes (`set reportFavorites {{name moves} ...}`).
// Copyright (c) 2001-2003 Shane Hudson. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The reader of the Scid's `reports.dat` (decision D3619, ADR 0006): a Tcl
//! script with a `set reportFavorites {{name moves} ...}` command. It is read
//! as data: nothing is evaluated, no `$variable` or `[command]` is expanded,
//! and anything that is not that `set` is ignored and counted.

/// A favorite report: a name and the moves of its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportFavorite {
    /// The name shown in the menu.
    pub name: String,
    /// The moves from the start (`1.e4 c5 2.Nf3`).
    pub moves: String,
}

/// What a `reports.dat` holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportFavorites {
    /// The favorites, in file order.
    pub favorites: Vec<ReportFavorite>,
    /// Commands and entries that were not understood and left out.
    pub ignored: u32,
}

/// Most favorites read from one file.
const MAX_FAVORITES: usize = 1000;

/// The whitespace of a Tcl list or command. `skip_blanks` and `word_ends`
/// must agree on it, or a word could end without the lexer advancing.
fn is_tcl_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r')
}

struct Lexer {
    s: Vec<char>,
    i: usize,
}

/// A word of a Tcl command or list.
enum Word {
    Text(String),
    /// The word is malformed (unbalanced braces or quotes, extra characters
    /// after a closing brace).
    Bad,
    End,
}

impl Lexer {
    fn new(text: &str) -> Lexer {
        Lexer {
            s: text.chars().collect(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    /// Skips blanks; in a command also a backslash-newline.
    fn skip_blanks(&mut self, newline_is_blank: bool) {
        while let Some(c) = self.peek() {
            let blank = (is_tcl_space(c) && c != '\n') || (newline_is_blank && c == '\n');
            if blank {
                self.i += 1;
            } else if c == '\\' && self.s.get(self.i + 1) == Some(&'\n') {
                self.i += 2;
            } else {
                break;
            }
        }
    }

    fn word_ends(&self, in_command: bool) -> bool {
        match self.peek() {
            None => true,
            Some(c) => is_tcl_space(c) || (in_command && c == ';'),
        }
    }

    fn next_word(&mut self, in_command: bool) -> Word {
        self.skip_blanks(!in_command);
        match self.peek() {
            None => Word::End,
            Some('\n' | ';') if in_command => Word::End,
            Some('{') => self.braced(in_command),
            Some('"') => self.quoted(in_command),
            Some(_) => {
                let mut out = String::new();
                while !self.word_ends(in_command) {
                    if let Some(c) = self.peek() {
                        out.push(c);
                    }
                    self.i += 1;
                }
                Word::Text(out)
            }
        }
    }

    fn braced(&mut self, in_command: bool) -> Word {
        self.i += 1;
        let mut depth = 1usize;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '\\' => {
                    out.push(c);
                    if let Some(next) = self.peek() {
                        out.push(next);
                        self.i += 1;
                    }
                }
                '{' => {
                    depth += 1;
                    out.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return self.closed(out, in_command);
                    }
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        Word::Bad
    }

    fn quoted(&mut self, in_command: bool) -> Word {
        self.i += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '"' => return self.closed(out, in_command),
                '\\' => {
                    match self.peek() {
                        Some('n' | 't') => out.push(' '),
                        Some(next) => out.push(next),
                        None => {}
                    }
                    if self.peek().is_some() {
                        self.i += 1;
                    }
                }
                _ => out.push(c),
            }
        }
        Word::Bad
    }

    /// After a closing brace or quote only a separator may follow.
    fn closed(&mut self, text: String, in_command: bool) -> Word {
        if self.word_ends(in_command) {
            Word::Text(text)
        } else {
            while !self.word_ends(in_command) {
                self.i += 1;
            }
            Word::Bad
        }
    }
}

/// The words of a list; `None` if it is malformed.
fn list_words(text: &str) -> Option<Vec<String>> {
    let mut lexer = Lexer::new(text);
    let mut words = Vec::new();
    loop {
        match lexer.next_word(false) {
            Word::Text(w) => {
                words.push(w);
                if words.len() > 4 * MAX_FAVORITES {
                    return None;
                }
            }
            Word::Bad => return None,
            Word::End => return Some(words),
        }
    }
}

/// Reads the favorites of a `reports.dat` text. Never fails and never
/// evaluates anything.
pub fn parse_reports_dat(text: &str) -> ReportFavorites {
    let mut out = ReportFavorites::default();
    let mut lexer = Lexer::new(text);
    loop {
        // One command.
        lexer.skip_blanks(true);
        while lexer.peek() == Some(';') {
            lexer.i += 1;
            lexer.skip_blanks(true);
        }
        if lexer.peek().is_none() {
            break;
        }
        if lexer.peek() == Some('#') {
            while lexer.peek().is_some_and(|c| c != '\n') {
                lexer.i += 1;
            }
            continue;
        }
        let mut words: Vec<String> = Vec::new();
        let mut bad = false;
        loop {
            match lexer.next_word(true) {
                Word::Text(w) => words.push(w),
                Word::Bad => bad = true,
                Word::End => break,
            }
        }
        if bad {
            out.ignored = out.ignored.saturating_add(1);
            continue;
        }
        match words.as_slice() {
            [set, var, value] if set == "set" && var == "reportFavorites" => {
                read_list(value, &mut out);
            }
            _ => out.ignored = out.ignored.saturating_add(1),
        }
        if lexer.peek().is_none() {
            break;
        }
    }
    out
}

fn read_list(value: &str, out: &mut ReportFavorites) {
    let Some(entries) = list_words(value) else {
        out.ignored = out.ignored.saturating_add(1);
        return;
    };
    for entry in entries {
        let parsed = list_words(&entry).and_then(|w| match <[String; 2]>::try_from(w) {
            Ok([name, moves]) if !name.trim().is_empty() => Some(ReportFavorite {
                name: name.trim().to_owned(),
                moves: moves.trim().to_owned(),
            }),
            _ => None,
        });
        match parsed {
            Some(f) if out.favorites.len() < MAX_FAVORITES => out.favorites.push(f),
            _ => out.ignored = out.ignored.saturating_add(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    fn fav(name: &str, moves: &str) -> ReportFavorite {
        ReportFavorite {
            name: name.into(),
            moves: moves.into(),
        }
    }

    #[test]
    fn reads_braced_and_quoted_entries() {
        let r = parse_reports_dat("set reportFavorites {{Nome {1.e4 e5}} {Outro \"1.d4 d5\"}}\n");
        assert_eq!(
            r.favorites,
            vec![fav("Nome", "1.e4 e5"), fav("Outro", "1.d4 d5")]
        );
        assert_eq!(r.ignored, 0);
    }

    #[test]
    fn names_with_spaces_between_braces() {
        let r = parse_reports_dat("set reportFavorites {{{Siciliana, Najdorf} {1.e4 c5 2.Nf3}}}");
        assert_eq!(
            r.favorites,
            vec![fav("Siciliana, Najdorf", "1.e4 c5 2.Nf3")]
        );
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let r = parse_reports_dat("# header\n\n  # more\nset reportFavorites {{A {1.e4}}}\n");
        assert_eq!(r.favorites, vec![fav("A", "1.e4")]);
        assert_eq!(r.ignored, 0);
    }

    #[test]
    fn anything_else_is_ignored_and_counted_not_run() {
        let text = "exec rm -rf /\nputs [exit]\nset other {x}\nset reportFavorites {{A {1.e4}}}\n";
        let r = parse_reports_dat(text);
        assert_eq!(r.favorites, vec![fav("A", "1.e4")]);
        assert_eq!(r.ignored, 3);
    }

    #[test]
    fn substitutions_stay_literal() {
        let r = parse_reports_dat("set reportFavorites {{$x [cmd]}}");
        assert_eq!(r.favorites, vec![fav("$x", "[cmd]")]);
        assert_eq!(r.ignored, 0);
        let r = parse_reports_dat("set reportFavorites {{onlyname}}");
        assert_eq!(r.favorites, vec![]);
        assert_eq!(r.ignored, 1, "a one-word entry is not a favorite");
        let r = parse_reports_dat("set reportFavorites {{N {[evil] $y}}}");
        assert_eq!(r.favorites, vec![fav("N", "[evil] $y")]);
    }

    #[test]
    fn malformed_input_is_counted() {
        let r = parse_reports_dat("set reportFavorites {{A {1.e4}");
        assert_eq!(r.favorites, vec![]);
        assert!(r.ignored >= 1);
        let r = parse_reports_dat("set reportFavorites {{ {1.e4}} {B}}");
        assert_eq!(r.favorites, vec![]);
        assert_eq!(r.ignored, 2);
    }

    #[test]
    fn whitespace_the_lexer_does_not_skip_cannot_loop_forever() {
        // A form feed, a vertical tab and a no-break space used to end a
        // word without advancing, so the command loop pushed empty words
        // until memory ran out.
        for stray in ["\x0B", "\x0C", "\u{A0}", "\u{85}", "\u{2028}"] {
            let r = parse_reports_dat(&format!("set{stray}reportFavorites {{{{A {{1.e4}}}}}}"));
            assert!(r.favorites.len() <= 1, "{stray:?}");
            let r = parse_reports_dat(stray);
            assert!(r.favorites.is_empty(), "{stray:?}");
        }
        let r = parse_reports_dat("set\x0BreportFavorites\x0C{{A {1.e4}}}");
        assert_eq!(r.favorites, vec![fav("A", "1.e4")]);
    }

    #[test]
    fn empty_text_gives_nothing() {
        assert_eq!(parse_reports_dat(""), ReportFavorites::default());
    }

    #[test]
    fn at_most_a_thousand_favorites() {
        let mut text = String::from("set reportFavorites {");
        for i in 0..1200 {
            let _ = write!(text, "{{n{i} {{1.e4}}}} ");
        }
        text.push('}');
        let r = parse_reports_dat(&text);
        assert_eq!(r.favorites.len(), MAX_FAVORITES);
        assert_eq!(r.ignored, 200);
    }
}
