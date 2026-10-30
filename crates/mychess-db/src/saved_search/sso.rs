// The variables read here are the ones written by Scid vs PC 4.27,
// tcl/search/header.tcl (`::search::header::save`, `options`, `defaults`).
// Copyright (c) Shane Hudson; Scid vs PC contributors. Scid is distributed
// under the GNU General Public License; this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Reading Scid `.sso` files without running Tcl (D643 to D646).

use std::collections::BTreeMap;
use std::path::Path;

use mychess_core::Eco;

use super::format::{FLAGS, TITLES, invalid};
use super::{SavedSearch, SavedSearchError, decode_text, read_text};
use crate::filter::FilterOp;
use crate::search::{
    EcoRange, GameEndKind, HeaderSearch, MinMax, TitleSet, Tri, complete_max_date,
    complete_min_date,
};

/// Longest statement accepted, in bytes (D643).
const MAX_STATEMENT: usize = 16 * 1024;
/// Most warnings kept (D645).
pub(super) const MAX_WARNINGS: usize = 100;
/// Most `sPgntext(n)` entries kept.
const MAX_PGN_TEXT: usize = 64;

/// Why a variable was not applied (D645).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SsoWarningKind {
    /// A material or position search variable (milestone M6).
    Material,
    /// A header option the MyChess search does not have yet.
    Unsupported,
    /// A name Scid does not write for header searches.
    Unknown,
    /// A material pattern that is not valid (a piece without a file and
    /// a rank, or unreadable fields); it was ignored (D3218).
    InvalidPattern,
}

/// A variable that was read but not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoWarning {
    /// Line of the statement (from 1).
    pub line: usize,
    /// The variable, with its index if any (`sHeaderFlags(Comments)`).
    pub name: String,
    /// Why.
    pub kind: SsoWarningKind,
}

/// The result of reading a `.sso` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoReport {
    /// The search.
    pub search: HeaderSearch,
    /// What was ignored (at most 100).
    pub warnings: Vec<SsoWarning>,
    /// Warnings beyond the 100 kept.
    pub warnings_dropped: u64,
}

impl SsoReport {
    /// Builds the [`SavedSearch`] to give to [`save`](super::save) (D646).
    ///
    /// # Errors
    ///
    /// [`SavedSearchError::InvalidName`] or [`InvalidDate`](SavedSearchError::InvalidDate).
    pub fn into_saved(
        self,
        name: &str,
        created: Option<&str>,
    ) -> Result<SavedSearch, SavedSearchError> {
        let mut saved = SavedSearch::new(name, self.search)?;
        if let Some(created) = created {
            super::validate_created(created)?;
            saved.created = Some(created.to_owned());
        }
        Ok(saved)
    }
}

/// Reads a `.sso` file (D643 to D645).
///
/// # Errors
///
/// I/O, size and text errors, [`SavedSearchError::Syntax`],
/// [`UnsupportedSearchType`](SavedSearchError::UnsupportedSearchType) and
/// [`InvalidValue`](SavedSearchError::InvalidValue).
pub fn import_sso(path: &Path) -> Result<SsoReport, SavedSearchError> {
    parse(&read_text(path)?)
}

/// Like [`import_sso`] for bytes already in memory.
///
/// # Errors
///
/// Same as [`import_sso`] except for I/O.
pub fn import_sso_bytes(bytes: &[u8]) -> Result<SsoReport, SavedSearchError> {
    parse(&decode_text(bytes)?)
}

/// [`import_sso`] for text already decoded.
pub(super) fn import_sso_text(text: &str) -> Result<SsoReport, SavedSearchError> {
    parse(text)
}

/// One `set name value` statement.
pub(super) struct Statement {
    pub(super) line: usize,
    pub(super) words: Vec<String>,
}

pub(super) struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    pub(super) fn new(src: &'a str) -> Lexer<'a> {
        Lexer {
            src,
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.as_bytes().get(self.pos + offset).copied()
    }

    fn bump(&mut self) {
        if self.peek() == Some(b'\n') {
            self.line += 1;
        }
        self.pos += 1;
    }

    /// Skips blanks and backslash-newline continuations between words.
    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | 0x0B | 0x0C) => self.bump(),
                Some(b'\\') if self.peek_at(1) == Some(b'\n') => {
                    self.bump();
                    self.bump();
                }
                _ => return,
            }
        }
    }

    /// Next statement, or `None` at the end of the text.
    pub(super) fn next_statement(&mut self) -> Result<Option<Statement>, SavedSearchError> {
        // Skip separators, empty lines and comments.
        loop {
            match self.peek() {
                None => return Ok(None),
                Some(b' ' | b'\t' | b'\r' | b'\n' | b';' | 0x0B | 0x0C) => self.bump(),
                Some(b'#') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.bump();
                    }
                }
                _ => break,
            }
        }
        let line = self.line;
        let start = self.pos;
        let mut words = Vec::new();
        loop {
            self.skip_blanks();
            if self.pos - start > MAX_STATEMENT {
                return Err(SavedSearchError::Syntax {
                    line,
                    reason: "statement too long",
                });
            }
            match self.peek() {
                None | Some(b'\n' | b';') => break,
                Some(_) => words.push(self.word(line)?),
            }
        }
        Ok(Some(Statement { line, words }))
    }

    fn word(&mut self, line: usize) -> Result<String, SavedSearchError> {
        let word = match self.peek() {
            Some(b'{') => self.braced(line)?,
            Some(b'"') => self.quoted(line)?,
            _ => self.bare(line)?,
        };
        Ok(word)
    }

    fn after_closing(&self, line: usize) -> Result<(), SavedSearchError> {
        match self.peek() {
            None | Some(b' ' | b'\t' | b'\r' | b'\n' | b';' | 0x0B | 0x0C) => Ok(()),
            Some(b'\\') if self.peek_at(1) == Some(b'\n') => Ok(()),
            _ => Err(SavedSearchError::Syntax {
                line,
                reason: "characters after the closing brace or quote",
            }),
        }
    }

    /// `{...}`: no substitution; nested braces; backslash-newline is a space.
    fn braced(&mut self, line: usize) -> Result<String, SavedSearchError> {
        self.bump();
        let mut depth = 1usize;
        let mut out = String::new();
        let mut from = self.pos;
        loop {
            if self.pos - from > MAX_STATEMENT || out.len() > MAX_STATEMENT {
                return Err(SavedSearchError::Syntax {
                    line,
                    reason: "statement too long",
                });
            }
            match self.peek() {
                None => {
                    return Err(SavedSearchError::Syntax {
                        line,
                        reason: "missing close brace",
                    });
                }
                Some(b'\\') => {
                    if self.peek_at(1) == Some(b'\n') {
                        out.push_str(&self.src[from..self.pos]);
                        self.bump();
                        self.bump();
                        while matches!(self.peek(), Some(b' ' | b'\t')) {
                            self.bump();
                        }
                        out.push(' ');
                        from = self.pos;
                    } else {
                        self.bump();
                        if self.peek().is_some() {
                            self.skip_char();
                        }
                    }
                }
                Some(b'{') => {
                    depth += 1;
                    self.bump();
                }
                Some(b'}') => {
                    depth -= 1;
                    if depth == 0 {
                        out.push_str(&self.src[from..self.pos]);
                        self.bump();
                        self.after_closing(line)?;
                        return Ok(out);
                    }
                    self.bump();
                }
                Some(_) => self.skip_char(),
            }
        }
    }

    /// Advances over one whole character (never splits UTF-8).
    fn skip_char(&mut self) {
        if let Some(c) = self.src[self.pos..].chars().next() {
            if c == '\n' {
                self.line += 1;
            }
            self.pos += c.len_utf8();
        }
    }

    /// `"..."`: backslash escapes, no substitution.
    fn quoted(&mut self, line: usize) -> Result<String, SavedSearchError> {
        self.bump();
        let mut out = String::new();
        loop {
            if out.len() > MAX_STATEMENT {
                return Err(SavedSearchError::Syntax {
                    line,
                    reason: "statement too long",
                });
            }
            match self.peek() {
                None => {
                    return Err(SavedSearchError::Syntax {
                        line,
                        reason: "missing close quote",
                    });
                }
                Some(b'"') => {
                    self.bump();
                    self.after_closing(line)?;
                    return Ok(out);
                }
                Some(b'$' | b'[') => {
                    return Err(SavedSearchError::Syntax {
                        line,
                        reason: "substitution is not supported",
                    });
                }
                Some(b'\\') => self.escape(&mut out),
                Some(_) => self.take_char(&mut out),
            }
        }
    }

    /// A word without quoting: ends at a blank, newline or `;`.
    fn bare(&mut self, line: usize) -> Result<String, SavedSearchError> {
        let mut out = String::new();
        loop {
            if out.len() > MAX_STATEMENT {
                return Err(SavedSearchError::Syntax {
                    line,
                    reason: "statement too long",
                });
            }
            match self.peek() {
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b';' | 0x0B | 0x0C) => return Ok(out),
                Some(b'\\') if self.peek_at(1) == Some(b'\n') => return Ok(out),
                Some(b'$' | b'[') => {
                    return Err(SavedSearchError::Syntax {
                        line,
                        reason: "substitution is not supported",
                    });
                }
                Some(b'\\') => self.escape(&mut out),
                Some(_) => self.take_char(&mut out),
            }
        }
    }

    fn take_char(&mut self, out: &mut String) {
        if let Some(c) = self.src[self.pos..].chars().next() {
            out.push(c);
        }
        self.skip_char();
    }

    /// A backslash sequence outside braces (Tcl rules for the common cases).
    fn escape(&mut self, out: &mut String) {
        self.bump(); // the backslash
        let Some(c) = self.src[self.pos..].chars().next() else {
            out.push('\\');
            return;
        };
        self.skip_char();
        match c {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'x' => self.hex_escape(out, 2, 'x'),
            'u' => self.hex_escape(out, 4, 'u'),
            other => out.push(other),
        }
    }

    fn hex_escape(&mut self, out: &mut String, max: usize, letter: char) {
        let mut value = 0u32;
        let mut digits = 0;
        while digits < max {
            match self.peek().and_then(|b| char::from(b).to_digit(16)) {
                Some(d) => {
                    value = value * 16 + d;
                    digits += 1;
                    self.bump();
                }
                None => break,
            }
        }
        match (digits, char::from_u32(value)) {
            (0, _) | (_, None) => out.push(letter),
            (_, Some(c)) => out.push(c),
        }
    }
}

/// Settings collected before they become a [`HeaderSearch`].
struct Builder {
    search: HeaderSearch,
    eco_min: Eco,
    eco_max: Eco,
    eco_none: bool,
    length: (u16, u16),
    pgn: BTreeMap<u32, String>,
    warnings: Vec<SsoWarning>,
    dropped: u64,
}

fn parse(text: &str) -> Result<SsoReport, SavedSearchError> {
    let mut lexer = Lexer::new(text);
    let mut b = Builder {
        search: HeaderSearch::default(),
        eco_min: eco_default("A00")?,
        eco_max: eco_default("E99")?,
        eco_none: true,
        length: (0, 999),
        pgn: BTreeMap::new(),
        warnings: Vec::new(),
        dropped: 0,
    };
    while let Some(st) = lexer.next_statement()? {
        if st.words.is_empty() {
            continue;
        }
        b.statement(&st)?;
    }
    b.finish()
}

fn eco_default(text: &str) -> Result<Eco, SavedSearchError> {
    text.parse::<Eco>()
        .map_err(|_| invalid("eco", text, "not an ECO code"))
}

pub(super) fn parse_bool(field: &str, text: &str) -> Result<bool, SavedSearchError> {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(invalid(field, text, "expected a boolean")),
    }
}

pub(super) fn parse_int(field: &str, text: &str) -> Result<i64, SavedSearchError> {
    let t = text.trim();
    t.strip_prefix('+')
        .unwrap_or(t)
        .parse::<i64>()
        .map_err(|_| invalid(field, text, "expected an integer"))
}

pub(super) fn parse_in<T: TryFrom<i64>>(field: &str, text: &str) -> Result<T, SavedSearchError> {
    T::try_from(parse_int(field, text)?).map_err(|_| invalid(field, text, "out of range"))
}

fn parse_tri(field: &str, text: &str) -> Result<Tri, SavedSearchError> {
    Tri::from_scid_str(text.trim()).ok_or_else(|| invalid(field, text, "expected yes, no or both"))
}

fn check_date(field: &str, text: &str) -> Result<(), SavedSearchError> {
    if mychess_core::PartialDate::is_valid_str(text.trim()) {
        Ok(())
    } else {
        Err(invalid(field, text, "not a date (yyyy.mm.dd)"))
    }
}

fn parse_eco(field: &str, text: &str, default: Eco) -> Result<Eco, SavedSearchError> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(default);
    }
    t.parse::<Eco>()
        .map_err(|_| invalid(field, text, "not an ECO code"))
}

/// Splits `name(index)`; `None` for the index of a plain name.
pub(super) fn split_name(name: &str) -> Option<(&str, Option<&str>)> {
    let name = name.strip_prefix("::").unwrap_or(name);
    match name.split_once('(') {
        None => Some((name, None)),
        Some((base, rest)) => rest.strip_suffix(')').map(|idx| (base, Some(idx))),
    }
}

const MATERIAL_VARS: [&str; 12] = [
    "pMin",
    "pMax",
    "ignoreColors",
    "matchEnd",
    "minMoveNum",
    "maxMoveNum",
    "minHalfMoves",
    "oppBishops",
    "pattPiece",
    "pattFyle",
    "pattRank",
    "pattBool",
];

impl Builder {
    fn warn(&mut self, line: usize, name: &str, kind: SsoWarningKind) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(SsoWarning {
                line,
                name: name.chars().take(80).collect(),
                kind,
            });
        } else {
            self.dropped += 1;
        }
    }

    fn statement(&mut self, st: &Statement) -> Result<(), SavedSearchError> {
        let syntax = |reason| SavedSearchError::Syntax {
            line: st.line,
            reason,
        };
        if st.words.first().map(String::as_str) != Some("set") {
            return Err(syntax("only the set command is supported"));
        }
        let [_, name, value] = st.words.as_slice() else {
            return Err(syntax("expected: set name value"));
        };
        let Some((base, index)) = split_name(name) else {
            return Err(syntax("malformed variable name"));
        };
        self.assign(st.line, name, base, index, value)
    }

    fn assign(
        &mut self,
        line: usize,
        full: &str,
        base: &str,
        index: Option<&str>,
        value: &str,
    ) -> Result<(), SavedSearchError> {
        let search = &mut self.search;
        if let Some(idx) = index {
            return self.assign_indexed(line, full, base, idx, value);
        }
        match base {
            "searchType" => match value.trim().to_ascii_lowercase().as_str() {
                "header" => {}
                "material" => {
                    return Err(SavedSearchError::UnsupportedSearchType {
                        found: value.chars().take(32).collect(),
                    });
                }
                _ => return Err(invalid(base, value, "expected Header")),
            },
            "sWhite" => value.clone_into(&mut search.white),
            "sBlack" => value.clone_into(&mut search.black),
            "sEvent" => value.clone_into(&mut search.event),
            "sSite" => value.clone_into(&mut search.site),
            "sRound" => value.clone_into(&mut search.round),
            "sTag" => value.clone_into(&mut search.extra_tag),
            "sTagValue" => value.clone_into(&mut search.extra_tag_value),
            "sDateMin" => {
                check_date(base, value)?;
                search.date.min = complete_min_date(value);
            }
            "sDateMax" => {
                check_date(base, value)?;
                search.date.max = complete_max_date(value);
            }
            "sIgnoreCol" => search.ignore_colors = parse_bool(base, value)?,
            "sWhiteEloMin" => search.white_elo.min = parse_in(base, value)?,
            "sWhiteEloMax" => search.white_elo.max = parse_in(base, value)?,
            "sBlackEloMin" => search.black_elo.min = parse_in(base, value)?,
            "sBlackEloMax" => search.black_elo.max = parse_in(base, value)?,
            "sEloDiffMin" => search.elo_diff.min = parse_in(base, value)?,
            "sEloDiffMax" => search.elo_diff.max = parse_in(base, value)?,
            "sGlMin" => self.length.0 = parse_in(base, value)?,
            "sGlMax" => self.length.1 = parse_in(base, value)?,
            "sGnumMin" => search.game_number.min = parse_int(base, value)?,
            "sGnumMax" => search.game_number.max = parse_int(base, value)?,
            "sEco" => self.eco_none = parse_bool(base, value)?,
            "sEcoMin" => self.eco_min = parse_eco(base, value, eco_default("A00")?)?,
            "sEcoMax" => self.eco_max = parse_eco(base, value, eco_default("E99")?)?,
            "sSideToMove" => {
                let t = value.trim().to_ascii_lowercase();
                let (w, b) = match t.as_str() {
                    "w" => (true, false),
                    "b" => (false, true),
                    "wb" | "bw" => (true, true),
                    _ => return Err(invalid(base, value, "expected w, b or wb")),
                };
                search.end_white_to_move = w;
                search.end_black_to_move = b;
            }
            "preComment" => search.pre_comment = parse_bool(base, value)?,
            "postComment" => search.post_comment = parse_bool(base, value)?,
            "sResWin" => search.results.white = parse_bool(base, value)?,
            "sResLoss" => search.results.black = parse_bool(base, value)?,
            "sResDraw" => search.results.draw = parse_bool(base, value)?,
            "sResOther" => search.results.none = parse_bool(base, value)?,
            "sPgncase" => search.ignore_case = parse_bool(base, value)?,
            "sGameEnd" => {
                search.game_end = match value.trim().to_ascii_lowercase().as_str() {
                    "any" => GameEndKind::Any,
                    "checkmate" => GameEndKind::Checkmate,
                    "stalemate" => GameEndKind::Stalemate,
                    _ => return Err(invalid(base, value, "expected Any, Checkmate or Stalemate")),
                };
            }
            "search::filter::operation" => {
                search.op = match value.trim() {
                    "0" => FilterOp::And,
                    "1" => FilterOp::Or,
                    "2" => FilterOp::Reset,
                    t if matches!(t.to_ascii_lowercase().as_str(), "and" | "or" | "reset") => {
                        FilterOp::from_scid_str(t)
                    }
                    _ => return Err(invalid(base, value, "expected 0, 1 or 2")),
                };
            }
            other if MATERIAL_VARS.contains(&other) => {
                self.warn(line, full, SsoWarningKind::Material);
            }
            _ => self.warn(line, full, SsoWarningKind::Unknown),
        }
        Ok(())
    }

    fn assign_indexed(
        &mut self,
        line: usize,
        full: &str,
        base: &str,
        idx: &str,
        value: &str,
    ) -> Result<(), SavedSearchError> {
        match base {
            "sHeaderFlags" => {
                let field = format!("{base}({idx})");
                if let Some((_, flag, _)) = FLAGS.iter().find(|(_, _, tcl)| *tcl == idx) {
                    let tri = parse_tri(&field, value)?;
                    self.search.flags.set(*flag, tri);
                } else if matches!(idx, "Comments" | "Variations" | "Annotations") {
                    if parse_tri(&field, value)? != Tri::Both {
                        self.warn(line, full, SsoWarningKind::Unsupported);
                    }
                } else {
                    self.warn(line, full, SsoWarningKind::Unknown);
                }
            }
            "sPgntext" => match idx.parse::<u32>() {
                Ok(n) if self.pgn.len() < MAX_PGN_TEXT || self.pgn.contains_key(&n) => {
                    self.pgn.insert(n, value.to_owned());
                }
                _ => self.warn(line, full, SsoWarningKind::Unknown),
            },
            "sTitles" => {
                let field = format!("{base}({idx})");
                let parsed = idx.split_once(':').and_then(|(side, title)| {
                    let bit = TITLES.iter().find(|(n, _)| *n == title)?.1;
                    match side {
                        "w" => Some((true, bit)),
                        "b" => Some((false, bit)),
                        _ => None,
                    }
                });
                match parsed {
                    Some((white, bit)) => {
                        let on = parse_bool(&field, value)?;
                        let set: &mut TitleSet = if white {
                            &mut self.search.white_titles
                        } else {
                            &mut self.search.black_titles
                        };
                        set.set(bit, on);
                    }
                    None => self.warn(line, full, SsoWarningKind::Unknown),
                }
            }
            other if MATERIAL_VARS.contains(&other) => {
                self.warn(line, full, SsoWarningKind::Material);
            }
            _ => self.warn(line, full, SsoWarningKind::Unknown),
        }
        Ok(())
    }

    fn finish(mut self) -> Result<SsoReport, SavedSearchError> {
        let full_eco = self.eco_min == eco_default("A00")?
            && self.eco_max == eco_default("E99")?
            && self.eco_none;
        self.search.eco = (!full_eco).then_some(EcoRange {
            min: self.eco_min,
            max: self.eco_max,
            include_none: self.eco_none,
        });
        // D334: the dialog default 0..999 means "no restriction".
        self.search.length =
            (self.length != (0, 999)).then(|| MinMax::new(self.length.0, self.length.1));
        self.search.pgn_text = self.pgn.into_values().filter(|t| !t.is_empty()).collect();
        Ok(SsoReport {
            search: self.search,
            warnings: self.warnings,
            warnings_dropped: self.dropped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::si4::GameFlags;

    fn ok(text: &str) -> SsoReport {
        import_sso_bytes(text.as_bytes()).expect("valid sso")
    }

    #[test]
    fn empty_file_is_the_default_search() {
        assert_eq!(ok("").search, HeaderSearch::default());
        assert_eq!(ok("# comment\n\n").search, HeaderSearch::default());
    }

    #[test]
    fn reads_the_scid_dialog_defaults_as_default_search() {
        let text = "# Scid vs PC search options file\nset searchType Header\n\
            set sWhite {}\nset sGlMin 0\nset sGlMax 999\nset sEco Yes\nset sEcoMin A00\n\
            set sEcoMax E99\nset sDateMin 0000.00.00\nset sDateMax 2047.12.31\n\
            set sWhiteEloMin 0\nset sWhiteEloMax 4000\nset sEloDiffMin -4000\n\
            set sEloDiffMax +4000\nset sSideToMove wb\nset ::search::filter::operation 2\n\
            set sHeaderFlags(StdStart) both\nset sPgntext(1) {}\nset sTitles(w:gm) 1\n";
        let r = ok(text);
        assert_eq!(r.search, HeaderSearch::default());
        assert_eq!(r.warnings, vec![]);
    }

    #[test]
    fn reads_values_in_every_tcl_word_form() {
        let r = ok(concat!(
            "set sWhite {Kasparov, G}\n",
            "set sBlack \"Karpov, A\\\"x\"\n",
            "set sEvent Linares\n",
            "set sSite Rio\\ de\\ Janeiro\n",
            "set sTag {Source}\nset sTagValue {a {nested} b}\n",
        ));
        assert_eq!(r.search.white, "Kasparov, G");
        assert_eq!(r.search.black, "Karpov, A\"x");
        assert_eq!(r.search.event, "Linares");
        assert_eq!(r.search.site, "Rio de Janeiro");
        assert_eq!(r.search.extra_tag_value, "a {nested} b");
    }

    #[test]
    fn reads_a_typical_scid_file() {
        let r = ok(concat!(
            "# Scid vs PC search options file\n",
            "set searchType Header\n",
            "set sWhite {Carlsen, Magnus}\n",
            "set sDateMin 2010.01.01\nset sDateMax 2020\n",
            "set sResWin 1\nset sResLoss 0\nset sResDraw 1\nset sResOther 0\n",
            "set sWhiteEloMin 2700\nset sWhiteEloMax 4000\n",
            "set sEloDiffMin -100\nset sEloDiffMax 100\n",
            "set sGlMin 40\nset sGlMax 100\n",
            "set sEco Yes\nset sEcoMin B00\nset sEcoMax B99\n",
            "set sSideToMove b\nset sIgnoreCol Yes\n",
            "set sGameEnd Checkmate\nset sPgncase 1\nset preComment 1\n"
        ));
        let s = &r.search;
        assert_eq!(s.white, "Carlsen, Magnus");
        assert_eq!(s.date.min.to_string(), "2010.??.??");
        assert_eq!(s.date.max.to_string(), "2020.12.31");
        assert!(s.results.white && !s.results.black && s.results.draw && !s.results.none);
        assert_eq!(s.white_elo, MinMax::new(2700, 4000));
        assert_eq!(s.elo_diff, MinMax::new(-100, 100));
        assert_eq!(s.length, Some(MinMax::new(40, 100)));
        let eco = s.eco.expect("eco range");
        assert_eq!(eco.min.to_string(), "B00");
        assert_eq!(eco.max.to_string(), "B99");
        assert!(eco.include_none);
        assert!(!s.end_white_to_move && s.end_black_to_move);
        assert!(s.ignore_colors && s.ignore_case && s.pre_comment);
        assert_eq!(s.game_end, GameEndKind::Checkmate);
    }

    #[test]
    fn reads_flags_titles_pgn_text_and_operation() {
        let r = ok(concat!(
            "set sHeaderFlags(TacticsFlag) yes\nset sHeaderFlags(DeleteFlag) no\n",
            "set sHeaderFlags(CustomFlag3) both\n",
            "set sPgntext(2) {Bxh7+}\nset sPgntext(1) {lost on time}\nset sPgntext(3) {}\n",
            "set sTitles(w:gm) 0\nset sTitles(b:none) 0\n",
            "set ::search::filter::operation 0\n",
        ));
        assert_eq!(r.search.flags.get(GameFlags::TACTICS), Tri::Yes);
        assert_eq!(r.search.flags.get(GameFlags::DELETE), Tri::No);
        assert_eq!(r.search.pgn_text, vec!["lost on time", "Bxh7+"]);
        assert!(!r.search.white_titles.contains(TitleSet::GM));
        assert!(r.search.white_titles.contains(TitleSet::IM));
        assert!(!r.search.black_titles.contains(TitleSet::NONE));
        assert_eq!(r.search.op, FilterOp::And);
    }

    #[test]
    fn eco_not_full_range_keeps_the_range() {
        let r = ok("set sEco No\n");
        let eco = r.search.eco.expect("range");
        assert!(!eco.include_none);
    }

    #[test]
    fn last_assignment_wins() {
        assert_eq!(ok("set sWhite a\nset sWhite b\n").search.white, "b");
    }

    #[test]
    fn material_unknown_and_unsupported_variables_warn() {
        let r = ok(concat!(
            "set pMin(wq) 1\nset pattPiece(1) K\nset ignoreColors 0\n",
            "set sHeaderFlags(Comments) yes\nset sHeaderFlags(Variations) both\n",
            "set somethingElse 3\nset sHeaderFlags(Nope) yes\n",
        ));
        let kinds: Vec<_> = r.warnings.iter().map(|w| (w.line, w.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (1, SsoWarningKind::Material),
                (2, SsoWarningKind::Material),
                (3, SsoWarningKind::Material),
                (4, SsoWarningKind::Unsupported),
                (6, SsoWarningKind::Unknown),
                (7, SsoWarningKind::Unknown),
            ]
        );
        assert_eq!(r.warnings[0].name, "pMin(wq)");
        assert_eq!(r.search, HeaderSearch::default());
    }

    #[test]
    fn warnings_are_capped() {
        let text = (0..150)
            .map(|i| format!("set v{i} 1\n"))
            .collect::<Vec<_>>()
            .concat();
        let r = ok(&text);
        assert_eq!(r.warnings.len(), MAX_WARNINGS);
        assert_eq!(r.warnings_dropped, 50);
    }

    #[test]
    fn material_search_type_is_refused() {
        let err = import_sso_bytes(b"set searchType Material\n").unwrap_err();
        assert!(matches!(
            err,
            SavedSearchError::UnsupportedSearchType { .. }
        ));
    }

    #[test]
    fn invalid_values_are_typed_errors() {
        for text in [
            "set sHeaderFlags(StdStart) maybe\n",
            "set sDateMin 20x4\n",
            "set sWhiteEloMin abc\n",
            "set sWhiteEloMin -5\n",
            "set sWhiteEloMax 70000\n",
            "set sEcoMin Z99\n",
            "set sSideToMove x\n",
            "set sGameEnd Draw\n",
            "set sIgnoreCol maybe\n",
            "set sGlMax -1\n",
            "set ::search::filter::operation 7\n",
            "set searchType Foo\n",
            "set sTitles(w:gm) x\n",
        ] {
            let err = import_sso_bytes(text.as_bytes()).unwrap_err();
            assert!(
                matches!(err, SavedSearchError::InvalidValue { .. }),
                "{text:?}: {err:?}"
            );
        }
    }

    #[test]
    fn rejects_code_and_substitution() {
        for text in [
            "puts hello\n",
            "exec rm -rf /\n",
            "set sWhite $env(HOME)\n",
            "set sWhite [exec ls]\n",
            "set sWhite \"a$b\"\n",
            "set sWhite\n",
            "set a b c\n",
            "set sWhite {unclosed\n",
            "set sWhite \"unclosed\n",
            "set sWhite {a}b\n",
            "set sWhite(x y\n",
            "proc x {} {\nset sWhite a\n}\n",
        ] {
            let err = import_sso_bytes(text.as_bytes()).unwrap_err();
            assert!(
                matches!(err, SavedSearchError::Syntax { .. }),
                "{text:?}: {err:?}"
            );
        }
    }

    #[test]
    fn semicolons_separate_set_commands() {
        let r = ok("set sWhite a; set sBlack b\n");
        assert_eq!(
            (r.search.white.as_str(), r.search.black.as_str()),
            ("a", "b")
        );
        assert!(import_sso_bytes(b"set sWhite a; puts x\n").is_err());
    }

    #[test]
    fn syntax_error_reports_the_line() {
        let err = import_sso_bytes(b"set sWhite a\n\nputs x\n").unwrap_err();
        assert!(matches!(err, SavedSearchError::Syntax { line: 3, .. }));
    }

    #[test]
    fn braced_values_may_span_lines() {
        let r = ok("set sWhite {a\nb}\nset sBlack x\n");
        assert_eq!(r.search.white, "a\nb");
        assert_eq!(r.search.black, "x");
        let r = ok("set sWhite {a\\\n   b}\n");
        assert_eq!(r.search.white, "a b");
    }

    #[test]
    fn escapes_in_bare_and_quoted_words() {
        let r = ok("set sWhite a\\{b\\ c\\\\d\nset sBlack \"t\\tn\\u00e9\\x41\"\n");
        assert_eq!(r.search.white, "a{b c\\d");
        assert_eq!(r.search.black, "t\tné".to_owned() + "A");
    }

    #[test]
    fn decodes_bom_and_latin1() {
        let mut bytes = b"\xEF\xBB\xBFset sWhite {Jos\xC3\xA9}\n".to_vec();
        assert_eq!(import_sso_bytes(&bytes).unwrap().search.white, "José");
        bytes = b"set sWhite {Jos\xE9}\n".to_vec();
        assert_eq!(import_sso_bytes(&bytes).unwrap().search.white, "José");
    }

    #[test]
    fn accepts_crlf_and_old_mac_cr_blanks() {
        let r = ok("set sWhite a\r\nset sBlack b\r\n");
        assert_eq!(r.search.white, "a");
        assert_eq!(r.search.black, "b");
    }

    #[test]
    fn binary_huge_and_long_inputs_fail_cleanly() {
        assert!(matches!(
            import_sso_bytes(b"set sWhite \0\n").unwrap_err(),
            SavedSearchError::NotText
        ));
        let huge = vec![b'a'; usize::try_from(super::super::MAX_FILE_BYTES).unwrap() + 1];
        assert!(matches!(
            import_sso_bytes(&huge).unwrap_err(),
            SavedSearchError::TooLarge { .. }
        ));
        let long = format!("set sWhite {{{}}}\n", "a".repeat(20_000));
        assert!(matches!(
            import_sso_bytes(long.as_bytes()).unwrap_err(),
            SavedSearchError::Syntax { .. }
        ));
        let bare = format!("set sWhite {}\n", "a".repeat(20_000));
        assert!(import_sso_bytes(bare.as_bytes()).is_err());
    }

    #[test]
    fn garbage_bytes_never_panic() {
        let mut seed = 12345u32;
        for _ in 0..200 {
            let bytes: Vec<u8> = (0..300)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    b"set {}\"\\$[];\n #abc(:)"[(seed >> 16) as usize % 21]
                })
                .collect();
            drop(import_sso_bytes(&bytes));
        }
    }

    #[test]
    fn truncated_files_fail_or_parse_without_panic() {
        let full = "set searchType Header\nset sWhite {Carlsen, Magnus}\nset sHeaderFlags(TacticsFlag) yes\n";
        for n in 0..full.len() {
            drop(import_sso_bytes(&full.as_bytes()[..n]));
        }
    }

    #[test]
    fn into_saved_names_the_search() {
        let r = ok("set sWhite a\n");
        let saved = r.into_saved("Mine", Some("2026-10-23")).unwrap();
        assert_eq!(saved.name, "Mine");
        assert_eq!(saved.created.as_deref(), Some("2026-10-23"));
        assert_eq!(saved.search.white, "a");
        assert!(ok("").into_saved("  ", None).is_err());
        assert!(ok("").into_saved("x", Some("yesterday")).is_err());
    }
}
