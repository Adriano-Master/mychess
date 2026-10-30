// Original MyChess code: nothing here is ported from Scid vs PC (Scid has no
// file of saved advanced searches; this is a TOML format of our own).

//! Saved position, material and moves searches (decisions D3235 and the
//! orchestrator's decision 6 of `docs/specs/m6-1-buscas-avancadas.md`).
//!
//! They live in the same directory and format as the saved header searches
//! (`format = "mychess-saved-search"`, `version = 1`) with an extra `kind`
//! key (`"position"`, `"material"` or `"moves"`; a file without it is a
//! header search) and one table with the options. The functions
//! [`save_advanced`], [`load_advanced`], [`list_advanced`] and
//! [`delete_advanced`] work on those files; the ones for header searches
//! ([`list`](super::list) and friends) skip them, and these skip the header
//! ones.

use std::fs;
use std::path::{Path, PathBuf};

use mychess_core::{Color, Piece, Position};
use serde::{Deserialize, Serialize};

use super::format::{FORMAT_NAME, FORMAT_VERSION, invalid};
use super::{
    SavedSearchEntry, SavedSearchError, SavedSearchList, SkippedFile, io_error, read_text,
    validate_created, validate_name, write_atomic,
};
use crate::filter::FilterOp;
use crate::search::{
    BishopColors, BoardMatch, BoardSearch, CheckTest, MAX_MOVE_TOKENS, MAX_PATTERNS,
    MaterialSearch, MoveToken, MovesSearch, Pattern, PieceRange, SideFilter, SideRanges,
};

/// A search of one of the advanced kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdvancedQuery {
    /// By position ([`BoardSearch`]).
    Position(BoardSearch),
    /// By material and patterns ([`MaterialSearch`]).
    Material(MaterialSearch),
    /// By a sequence of moves ([`MovesSearch`]).
    Moves(MovesSearch),
}

impl AdvancedQuery {
    /// The value of the `kind` key.
    pub fn kind_name(&self) -> &'static str {
        match self {
            AdvancedQuery::Position(_) => "position",
            AdvancedQuery::Material(_) => "material",
            AdvancedQuery::Moves(_) => "moves",
        }
    }
}

/// A named advanced search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAdvanced {
    /// Display name (same rules as [`SavedSearch::name`](super::SavedSearch::name)).
    pub name: String,
    /// Creation date `yyyy-mm-dd`, supplied by the caller.
    pub created: Option<String>,
    /// The search.
    pub query: AdvancedQuery,
}

impl SavedAdvanced {
    /// A saved search without a creation date.
    ///
    /// # Errors
    ///
    /// [`SavedSearchError::InvalidName`].
    pub fn new(name: &str, query: AdvancedQuery) -> Result<SavedAdvanced, SavedSearchError> {
        let name = name.trim();
        validate_name(name)?;
        Ok(SavedAdvanced {
            name: name.to_owned(),
            created: None,
            query,
        })
    }

    /// The TOML text of the search.
    ///
    /// # Errors
    ///
    /// Invalid name or date, or a serialization failure.
    pub fn to_toml(&self) -> Result<String, SavedSearchError> {
        validate_name(&self.name)?;
        if let Some(created) = &self.created {
            validate_created(created)?;
        }
        let mut dto = FileDto {
            format: FORMAT_NAME.to_owned(),
            version: FORMAT_VERSION,
            name: self.name.clone(),
            created: self.created.clone(),
            kind: self.query.kind_name().to_owned(),
            position: None,
            material: None,
            moves: None,
        };
        match &self.query {
            AdvancedQuery::Position(q) => dto.position = Some(PositionDto::from_query(q)),
            AdvancedQuery::Material(q) => dto.material = Some(MaterialDto::from_query(q)),
            AdvancedQuery::Moves(q) => dto.moves = Some(MovesDto::from_query(q)),
        }
        toml::to_string(&dto).map_err(|e| SavedSearchError::Toml {
            message: e.to_string(),
        })
    }

    /// Reads the TOML text of an advanced saved search.
    ///
    /// # Errors
    ///
    /// [`SavedSearchError::Toml`], [`WrongFormat`](SavedSearchError::WrongFormat),
    /// [`UnsupportedVersion`](SavedSearchError::UnsupportedVersion),
    /// [`WrongKind`](SavedSearchError::WrongKind) for a header search and
    /// [`InvalidValue`](SavedSearchError::InvalidValue).
    pub fn from_toml(text: &str) -> Result<SavedAdvanced, SavedSearchError> {
        let table: toml::Table = toml::from_str(text).map_err(|e| SavedSearchError::Toml {
            message: e.to_string(),
        })?;
        match table.get("format") {
            Some(toml::Value::String(s)) if s == FORMAT_NAME => {}
            _ => return Err(SavedSearchError::WrongFormat),
        }
        match table.get("version") {
            Some(toml::Value::Integer(v)) if *v == i64::from(FORMAT_VERSION) => {}
            Some(toml::Value::Integer(v)) => {
                return Err(SavedSearchError::UnsupportedVersion { found: *v });
            }
            _ => return Err(SavedSearchError::WrongFormat),
        }
        match table.get("kind") {
            Some(toml::Value::String(k)) if k != "header" => {}
            // No `kind` (or "header"): a header search, skipped by listings.
            None | Some(toml::Value::String(_)) => {
                return Err(SavedSearchError::WrongKind {
                    found: "header".to_owned(),
                });
            }
            // A `kind` that is not text is a damaged file, not a header
            // search: list_advanced reports it in `skipped`.
            Some(other) => {
                return Err(invalid("kind", &other.to_string(), "expected a string"));
            }
        }
        let dto: FileDto = table.try_into().map_err(|e| SavedSearchError::Toml {
            message: e.to_string(),
        })?;
        validate_name(&dto.name)?;
        if let Some(created) = &dto.created {
            validate_created(created)?;
        }
        let query = match (dto.kind.as_str(), dto.position, dto.material, dto.moves) {
            ("position", Some(p), None, None) => AdvancedQuery::Position(p.into_query()?),
            ("material", None, Some(m), None) => AdvancedQuery::Material(m.into_query()?),
            ("moves", None, None, Some(m)) => AdvancedQuery::Moves(m.into_query()?),
            (kind, ..) => return Err(invalid("kind", kind, "unknown kind or missing table")),
        };
        Ok(SavedAdvanced {
            name: dto.name,
            created: dto.created,
            query,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDto {
    format: String,
    version: u32,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<String>,
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<PositionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<MaterialDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    moves: Option<MovesDto>,
}

fn op_name(op: FilterOp) -> &'static str {
    match op {
        FilterOp::And => "and",
        FilterOp::Or => "or",
        FilterOp::Reset => "reset",
    }
}

fn op_from(text: &str) -> Result<FilterOp, SavedSearchError> {
    match text {
        "and" => Ok(FilterOp::And),
        "or" => Ok(FilterOp::Or),
        "reset" => Ok(FilterOp::Reset),
        other => Err(invalid("op", other, "expected and, or or reset")),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct PositionDto {
    fen: String,
    match_kind: String,
    in_variations: bool,
    ignore_colors: bool,
    op: String,
}

impl Default for PositionDto {
    fn default() -> PositionDto {
        PositionDto::from_query(&BoardSearch::new(Position::new()))
    }
}

impl PositionDto {
    fn from_query(q: &BoardSearch) -> PositionDto {
        PositionDto {
            fen: q.position.fen(),
            match_kind: match q.kind {
                BoardMatch::Exact => "exact",
                BoardMatch::Pawns => "pawns",
                BoardMatch::Files => "files",
                BoardMatch::Material => "material",
            }
            .to_owned(),
            in_variations: q.in_variations,
            ignore_colors: q.ignore_colors,
            op: op_name(q.op).to_owned(),
        }
    }

    fn into_query(self) -> Result<BoardSearch, SavedSearchError> {
        let position = Position::from_fen(&self.fen)
            .map_err(|_| invalid("position.fen", &self.fen, "not a FEN"))?;
        let kind = match self.match_kind.as_str() {
            "exact" => BoardMatch::Exact,
            "pawns" => BoardMatch::Pawns,
            "files" => BoardMatch::Files,
            "material" => BoardMatch::Material,
            other => {
                return Err(invalid(
                    "position.match_kind",
                    other,
                    "expected exact, pawns, files or material",
                ));
            }
        };
        Ok(BoardSearch {
            position,
            kind,
            in_variations: self.in_variations,
            ignore_colors: self.ignore_colors,
            op: op_from(&self.op)?,
            shortcuts: true,
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct RangeDto {
    min: u8,
    max: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SideDto {
    queen: RangeDto,
    rook: RangeDto,
    bishop: RangeDto,
    knight: RangeDto,
    minor: RangeDto,
    pawn: RangeDto,
}

impl SideDto {
    fn from_side(s: &SideRanges) -> SideDto {
        let r = |p: PieceRange| RangeDto {
            min: p.min,
            max: p.max,
        };
        SideDto {
            queen: r(s.queen),
            rook: r(s.rook),
            bishop: r(s.bishop),
            knight: r(s.knight),
            minor: r(s.minor),
            pawn: r(s.pawn),
        }
    }

    fn into_side(self) -> SideRanges {
        let r = |d: RangeDto| PieceRange::new(d.min, d.max);
        SideRanges {
            queen: r(self.queen),
            rook: r(self.rook),
            bishop: r(self.bishop),
            knight: r(self.knight),
            minor: r(self.minor),
            pawn: r(self.pawn),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatternDto {
    piece: String,
    file: String,
    rank: String,
    present: bool,
}

fn piece_code(p: Piece) -> String {
    let color = if p.color == Color::White { 'w' } else { 'b' };
    format!("{color}{}", p.role.char())
}

impl PatternDto {
    fn from_pattern(p: Pattern) -> PatternDto {
        PatternDto {
            piece: p.piece.map_or_else(|| "?".to_owned(), piece_code),
            file: p
                .file
                .map_or_else(|| "?".to_owned(), |f| f.char().to_string()),
            rank: p
                .rank
                .map_or_else(|| "?".to_owned(), |r| r.char().to_string()),
            present: p.present,
        }
    }

    fn into_pattern(self) -> Result<Pattern, SavedSearchError> {
        let present = if self.present { "yes" } else { "no" };
        Pattern::from_scid(&self.piece, &self.file, &self.rank, present)
            .map_err(|e| invalid("material.patterns", &self.piece, &e.to_string()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterialDto {
    white: SideDto,
    black: SideDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diff_min: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diff_max: Option<i16>,
    bishops: String,
    move_min: u16,
    move_max: u16,
    min_plies: u16,
    end_only: bool,
    ignore_colors: bool,
    op: String,
    #[serde(default)]
    patterns: Vec<PatternDto>,
}

impl MaterialDto {
    fn from_query(q: &MaterialSearch) -> MaterialDto {
        MaterialDto {
            white: SideDto::from_side(&q.white),
            black: SideDto::from_side(&q.black),
            diff_min: q.diff.map(|d| d.0),
            diff_max: q.diff.map(|d| d.1),
            bishops: match q.bishops {
                BishopColors::Either => "either",
                BishopColors::Same => "same",
                BishopColors::Opposite => "opposite",
            }
            .to_owned(),
            move_min: q.moves.0,
            move_max: q.moves.1,
            min_plies: q.min_plies,
            end_only: q.end_only,
            ignore_colors: q.ignore_colors,
            op: op_name(q.op).to_owned(),
            patterns: q
                .patterns
                .iter()
                .copied()
                .map(PatternDto::from_pattern)
                .collect(),
        }
    }

    fn into_query(self) -> Result<MaterialSearch, SavedSearchError> {
        if self.patterns.len() > MAX_PATTERNS {
            return Err(invalid("material.patterns", "...", "too many patterns"));
        }
        let diff = match (self.diff_min, self.diff_max) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => {
                return Err(invalid(
                    "material.diff_min",
                    "",
                    "diff_min and diff_max go together",
                ));
            }
        };
        let patterns = self
            .patterns
            .into_iter()
            .map(PatternDto::into_pattern)
            .collect::<Result<Vec<_>, _>>()?;
        let query = MaterialSearch {
            white: self.white.into_side(),
            black: self.black.into_side(),
            diff,
            bishops: match self.bishops.as_str() {
                "either" => BishopColors::Either,
                "same" => BishopColors::Same,
                "opposite" => BishopColors::Opposite,
                other => {
                    return Err(invalid(
                        "material.bishops",
                        other,
                        "expected either, same or opposite",
                    ));
                }
            },
            moves: (self.move_min, self.move_max),
            min_plies: self.min_plies,
            end_only: self.end_only,
            ignore_colors: self.ignore_colors,
            patterns,
            op: op_from(&self.op)?,
            shortcuts: true,
        };
        query
            .validate()
            .map_err(|e| invalid("material", "", &e.to_string()))?;
        Ok(query)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MovesDto {
    moves: String,
    side: String,
    check_test: String,
    in_variations: bool,
    start_min: u16,
    start_max: u16,
    op: String,
}

impl MovesDto {
    fn from_query(q: &MovesSearch) -> MovesDto {
        MovesDto {
            moves: q
                .tokens
                .iter()
                .map(|t| match t {
                    MoveToken::Any => "?",
                    MoveToken::San(text) => text.as_str(),
                })
                .collect::<Vec<_>>()
                .join(" "),
            side: match q.side {
                SideFilter::White => "white",
                SideFilter::Black => "black",
                SideFilter::Both => "both",
            }
            .to_owned(),
            check_test: match q.check_test {
                CheckTest::No => "no",
                CheckTest::Check => "check",
                CheckTest::Mate => "mate",
            }
            .to_owned(),
            in_variations: q.in_variations,
            start_min: q.start_plies.0,
            start_max: q.start_plies.1,
            op: op_name(q.op).to_owned(),
        }
    }

    fn into_query(self) -> Result<MovesSearch, SavedSearchError> {
        let side = match self.side.as_str() {
            "white" => SideFilter::White,
            "black" => SideFilter::Black,
            "both" => SideFilter::Both,
            other => {
                return Err(invalid(
                    "moves.side",
                    other,
                    "expected white, black or both",
                ));
            }
        };
        let check = match self.check_test.as_str() {
            "no" => CheckTest::No,
            "check" => CheckTest::Check,
            "mate" => CheckTest::Mate,
            other => {
                return Err(invalid(
                    "moves.check_test",
                    other,
                    "expected no, check or mate",
                ));
            }
        };
        if self.moves.split_whitespace().count() > MAX_MOVE_TOKENS {
            return Err(invalid("moves.moves", &self.moves, "too many words"));
        }
        let mut query = MovesSearch::parse(&self.moves, side, check, op_from(&self.op)?)
            .map_err(|e| invalid("moves.moves", &self.moves, &e.to_string()))?;
        query.in_variations = self.in_variations;
        query.start_plies = (self.start_min, self.start_max);
        Ok(query)
    }
}

fn load_advanced_file(path: &Path) -> Result<SavedAdvanced, SavedSearchError> {
    SavedAdvanced::from_toml(&read_text(path)?)
}

/// Lists the advanced saved searches of `dir`, sorted by name. Header
/// searches are not listed; a missing directory is empty.
///
/// # Errors
///
/// [`SavedSearchError::Io`] if the directory cannot be read.
pub fn list_advanced(dir: &Path) -> Result<SavedSearchList, SavedSearchError> {
    let mut out = SavedSearchList::default();
    for (path, result) in super::scan_with(dir, load_advanced_file)? {
        match result {
            Ok(s) => out.entries.push(SavedSearchEntry {
                path,
                name: s.name,
                created: s.created,
            }),
            Err(SavedSearchError::WrongKind { .. }) => {}
            Err(error) => out.skipped.push(SkippedFile { path, error }),
        }
    }
    out.entries
        .sort_by_cached_key(|e| (e.name.to_lowercase(), e.path.clone()));
    Ok(out)
}

/// Loads the advanced search called `name` from `dir`.
///
/// # Errors
///
/// [`SavedSearchError::NotFound`] if there is none, or a directory error.
pub fn load_advanced(dir: &Path, name: &str) -> Result<SavedAdvanced, SavedSearchError> {
    let name = name.trim();
    super::scan_with(dir, load_advanced_file)?
        .into_iter()
        .filter_map(|(_, r)| r.ok())
        .find(|s| s.name == name)
        .ok_or_else(|| SavedSearchError::NotFound {
            name: name.to_owned(),
        })
}

/// Deletes the advanced search called `name`; `false` if there was none.
///
/// # Errors
///
/// I/O errors.
pub fn delete_advanced(dir: &Path, name: &str) -> Result<bool, SavedSearchError> {
    let name = name.trim();
    let found = super::scan_with(dir, load_advanced_file)?
        .into_iter()
        .find(|(_, r)| r.as_ref().is_ok_and(|s| s.name == name));
    match found {
        Some((path, _)) => {
            fs::remove_file(&path).map_err(io_error(&path))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Saves `saved` in `dir` and returns the file path: an advanced search with
/// the same name is replaced; otherwise the file name comes from
/// [`file_stem_for`](super::file_stem_for).
///
/// # Errors
///
/// Invalid name or date, or I/O errors; on error the previous file is intact.
pub fn save_advanced(dir: &Path, saved: &SavedAdvanced) -> Result<PathBuf, SavedSearchError> {
    let text = saved.to_toml()?;
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let existing = super::scan_with(dir, load_advanced_file)?
        .into_iter()
        .find(|(_, r)| r.as_ref().is_ok_and(|s| s.name == saved.name))
        .map(|(p, _)| p);
    let path = match existing {
        Some(path) => path,
        None => super::free_path(dir, &super::file_stem_for(&saved.name))?,
    };
    write_atomic(&path, text.as_bytes())?;
    Ok(path)
}
