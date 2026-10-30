// The variables read here are the ones written by Scid vs PC 4.27,
// tcl/search/material.tcl (`::search::material::save`). Copyright (c) Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License; this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Reading Scid `.sso` files of a **material** search without running Tcl
//! (decision D3235 of `docs/specs/m6-1-buscas-avancadas.md`).
//!
//! The file is the one the window of Scid writes: `set pMin(wq) 0`,
//! `set pMax(wq) 2` for the twelve ranges, `ignoreColors`, `matchEnd`,
//! `minMoveNum`, `maxMoveNum`, `minHalfMoves`, `oppBishops`, the filter
//! operation and ten patterns (`pattPiece(i)`, `pattFyle(i)`, `pattRank(i)`
//! and `pattBool(i)`). The window does not save the material difference, so
//! it is left at the default (no restriction).

use std::collections::BTreeMap;
use std::path::Path;

use super::format::invalid;
use super::sso::{
    Lexer, MAX_WARNINGS, SsoReport, SsoWarning, SsoWarningKind, Statement, parse_bool, parse_in,
    split_name,
};
use super::{SavedSearchError, decode_text, read_text};
use crate::filter::FilterOp;
use crate::search::{BishopColors, MAX_PATTERNS, MaterialSearch, Pattern, PieceRange};

/// The result of reading a material `.sso` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialSsoReport {
    /// The search. The ranges are exactly the ones in the file; use
    /// [`MaterialSearch::normalized`] to make them consistent.
    pub search: MaterialSearch,
    /// What was ignored (at most 100).
    pub warnings: Vec<SsoWarning>,
    /// Warnings beyond the 100 kept.
    pub warnings_dropped: u64,
}

/// A `.sso` file of either kind.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnySsoReport {
    /// `searchType Header` (or no `searchType`).
    Header(Box<SsoReport>),
    /// `searchType Material`.
    Material(MaterialSsoReport),
}

/// Reads a material `.sso` file.
///
/// # Errors
///
/// I/O, size and text errors, [`SavedSearchError::Syntax`],
/// [`UnsupportedSearchType`](SavedSearchError::UnsupportedSearchType) for a
/// file that is not a material search and
/// [`InvalidValue`](SavedSearchError::InvalidValue).
pub fn import_material_sso(path: &Path) -> Result<MaterialSsoReport, SavedSearchError> {
    parse(&read_text(path)?)
}

/// Like [`import_material_sso`] for bytes already in memory.
///
/// # Errors
///
/// Same as [`import_material_sso`] except for I/O.
pub fn import_material_sso_bytes(bytes: &[u8]) -> Result<MaterialSsoReport, SavedSearchError> {
    parse(&decode_text(bytes)?)
}

/// Reads a `.sso` file and tells by its `searchType` whether it is a header
/// or a material search.
///
/// # Errors
///
/// As [`import_sso`](super::import_sso) and [`import_material_sso`].
pub fn import_any_sso(path: &Path) -> Result<AnySsoReport, SavedSearchError> {
    any(&read_text(path)?)
}

/// Like [`import_any_sso`] for bytes already in memory.
///
/// # Errors
///
/// Same as [`import_any_sso`] except for I/O.
pub fn import_any_sso_bytes(bytes: &[u8]) -> Result<AnySsoReport, SavedSearchError> {
    any(&decode_text(bytes)?)
}

fn any(text: &str) -> Result<AnySsoReport, SavedSearchError> {
    if search_type(text)?.is_some_and(|t| t.eq_ignore_ascii_case("material")) {
        parse(text).map(AnySsoReport::Material)
    } else {
        super::sso::import_sso_text(text).map(|r| AnySsoReport::Header(Box::new(r)))
    }
}

/// The value of the last `set searchType` statement.
fn search_type(text: &str) -> Result<Option<String>, SavedSearchError> {
    let mut lexer = Lexer::new(text);
    let mut found = None;
    while let Some(st) = lexer.next_statement()? {
        if let [set, name, value] = st.words.as_slice()
            && set == "set"
            && name == "searchType"
        {
            found = Some(value.clone());
        }
    }
    Ok(found)
}

const KINDS: [&str; 12] = [
    "wq", "bq", "wr", "br", "wb", "bb", "wn", "bn", "wm", "bm", "wp", "bp",
];

#[derive(Default)]
struct PatternFields {
    line: usize,
    piece: Option<String>,
    file: Option<String>,
    rank: Option<String>,
    present: Option<String>,
}

struct Builder {
    search: MaterialSearch,
    patterns: BTreeMap<u32, PatternFields>,
    warnings: Vec<SsoWarning>,
    dropped: u64,
}

fn parse(text: &str) -> Result<MaterialSsoReport, SavedSearchError> {
    let mut lexer = Lexer::new(text);
    let mut b = Builder {
        search: MaterialSearch::default(),
        patterns: BTreeMap::new(),
        warnings: Vec::new(),
        dropped: 0,
    };
    while let Some(st) = lexer.next_statement()? {
        if !st.words.is_empty() {
            b.statement(&st)?;
        }
    }
    Ok(b.finish())
}

fn range_mut<'a>(search: &'a mut MaterialSearch, kind: &str) -> Option<&'a mut PieceRange> {
    let side = if kind.starts_with('w') {
        &mut search.white
    } else {
        &mut search.black
    };
    match kind.get(1..)? {
        "q" => Some(&mut side.queen),
        "r" => Some(&mut side.rook),
        "b" => Some(&mut side.bishop),
        "n" => Some(&mut side.knight),
        "m" => Some(&mut side.minor),
        "p" => Some(&mut side.pawn),
        _ => None,
    }
}

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
        match index {
            Some(idx) => self.assign_indexed(st.line, name, base, idx, value),
            None => self.assign(st.line, name, base, value),
        }
    }

    fn assign(
        &mut self,
        line: usize,
        full: &str,
        base: &str,
        value: &str,
    ) -> Result<(), SavedSearchError> {
        let search = &mut self.search;
        match base {
            "searchType" => {
                if !value.trim().eq_ignore_ascii_case("material") {
                    return Err(SavedSearchError::UnsupportedSearchType {
                        found: value.chars().take(32).collect(),
                    });
                }
            }
            "ignoreColors" => search.ignore_colors = parse_bool(base, value)?,
            "matchEnd" => search.end_only = parse_bool(base, value)?,
            "minMoveNum" => search.moves.0 = limited(base, value, 999)?,
            "maxMoveNum" => search.moves.1 = limited(base, value, 999)?,
            "minHalfMoves" => search.min_plies = limited(base, value, 99)?,
            "oppBishops" => {
                search.bishops = match value.trim().chars().next().map(|c| c.to_ascii_uppercase()) {
                    Some('S') => BishopColors::Same,
                    Some('O') => BishopColors::Opposite,
                    _ => BishopColors::Either,
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
            "pMin" | "pMax" if KINDS.contains(&idx) => {
                let n: u8 = limited(&format!("{base}({idx})"), value, 99)?;
                if let Some(range) = range_mut(&mut self.search, idx) {
                    if base == "pMin" {
                        range.min = n;
                    } else {
                        range.max = n;
                    }
                }
            }
            "pattPiece" | "pattFyle" | "pattRank" | "pattBool" => match idx.parse::<u32>() {
                Ok(i) if usize::try_from(i).is_ok_and(|i| (1..=MAX_PATTERNS).contains(&i)) => {
                    let fields = self.patterns.entry(i).or_default();
                    if fields.line == 0 {
                        fields.line = line;
                    }
                    let text = Some(value.trim().to_owned());
                    match base {
                        "pattPiece" => fields.piece = text,
                        "pattFyle" => fields.file = text,
                        "pattRank" => fields.rank = text,
                        _ => fields.present = text,
                    }
                }
                _ => self.warn(line, full, SsoWarningKind::Unknown),
            },
            _ => self.warn(line, full, SsoWarningKind::Unknown),
        }
        Ok(())
    }

    fn finish(mut self) -> MaterialSsoReport {
        let patterns = std::mem::take(&mut self.patterns);
        for (i, f) in patterns {
            let piece = f.piece.as_deref().unwrap_or("?");
            if piece == "?" {
                continue;
            }
            let pattern = Pattern::from_scid(
                piece,
                f.file.as_deref().unwrap_or("?"),
                f.rank.as_deref().unwrap_or("?"),
                f.present.as_deref().unwrap_or("Yes"),
            );
            match pattern {
                Ok(p) if p.file.is_some() || p.rank.is_some() => self.search.patterns.push(p),
                _ => self.warn(
                    f.line,
                    &format!("pattPiece({i})"),
                    SsoWarningKind::InvalidPattern,
                ),
            }
        }
        MaterialSsoReport {
            search: self.search,
            warnings: self.warnings,
            warnings_dropped: self.dropped,
        }
    }
}

/// An integer in `0..=max`.
fn limited<T: TryFrom<i64>>(field: &str, text: &str, max: i64) -> Result<T, SavedSearchError> {
    let n: i64 = parse_in(field, text)?;
    if !(0..=max).contains(&n) {
        return Err(invalid(field, text, "out of range"));
    }
    T::try_from(n).map_err(|_| invalid(field, text, "out of range"))
}
