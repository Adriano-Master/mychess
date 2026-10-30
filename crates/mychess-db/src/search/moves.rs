// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_search_moves`) and
// src/game.cpp (`Game::MoveMatch`). Copyright (c) 1999-2003 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The search by a sequence of moves (`sc_search moves`; M6-1a, decisions
//! D3225 to D3230 of `docs/specs/m6-1-buscas-avancadas.md`).
//!
//! The query is a list of words, each a move in SAN or the wildcard `?` (any
//! move). A game matches when, somewhere in its main line, the moves played in
//! a row correspond to the words. The first move may be at any ply of the
//! [start range](MovesSearch::start_plies) (0 to 254 as in Scid) and, if
//! [`MovesSearch::side`] says so, only for one side; the other moves follow
//! without gaps. The filter value is the ply of the first move plus 2 (at
//! most 255), so the loaded game shows the position after that move.
//!
//! A word is compared with the SAN that Scid would write for the move
//! (`MakeSANString`; the core writes the same disambiguation) as plain text,
//! so the [`CheckTest`] decides whether `+` and `#` are in the generated text.
//!
//! ## Deviations from Scid
//!
//! - D3225: the first word cannot be `?` (it could never match in Scid) and
//!   an empty list is an error.
//! - D3226: words are read tolerantly: `1.e4`, `12...Nf6`, `0-0`, `exd5!?`
//!   and `e8Q` are accepted; a word that ends in `+` or `#` raises the
//!   check test to `Check` or `Mate` instead of never matching.
//! - D3227: `?` needs a real move; it does not match the end of the game.
//! - Options that Scid lists as "todo": search in variations
//!   ([`MovesSearch::in_variations`]) and the range of start plies.

use std::ops::ControlFlow;

use mychess_core::{Color, Move, Position, Role, SanSuffix, Square};

use super::header::{SearchError, SearchReport};
use super::scan::{GameMatcher, Shortcut, scan_games};
use crate::filter::{Filter, FilterOp};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::start_fen_of_record;
use crate::si4::{IndexEntry, ListPosition, RawGame, RawMove, RecordError, walk_mainline};
use crate::source::GameSource;

/// Most words in a move list.
pub const MAX_MOVE_TOKENS: usize = 64;

/// Longest word accepted, in bytes.
const MAX_TOKEN_LEN: usize = 32;

/// The last ply where the first move may be, by default (Scid: 255 plies).
pub const DEFAULT_LAST_START_PLY: u16 = 254;

/// One word of the move list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MoveToken {
    /// A move, in the text SAN is generated with: `Nf3`, `exd5`, `e8=Q`,
    /// `O-O`, `--`, with `+` or `#` only if typed.
    San(String),
    /// Any move.
    Any,
}

/// How much of the check state is in the SAN that words are compared with
/// (Scid's "Check Test").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum CheckTest {
    /// No marker: `Qh7` matches `Qh7+` of a game.
    #[default]
    No,
    /// `+` after a check (and after a mate).
    Check,
    /// `+` after a check and `#` after a mate.
    Mate,
}

/// The side the first move must belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SideFilter {
    /// White only.
    White,
    /// Black only.
    Black,
    /// Either side.
    #[default]
    Both,
}

impl SideFilter {
    fn allows(self, turn: Color) -> bool {
        matches!(
            (self, turn),
            (SideFilter::Both, _)
                | (SideFilter::White, Color::White)
                | (SideFilter::Black, Color::Black)
        )
    }
}

/// The options of a search by moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovesSearch {
    /// The words; the first is never [`MoveToken::Any`] and there is at
    /// least one (build it with [`MovesSearch::parse`]).
    pub tokens: Vec<MoveToken>,
    /// The side of the first move.
    pub side: SideFilter,
    /// The check test (already raised when a word carries `+` or `#`).
    pub check_test: CheckTest,
    /// How the result combines with the current filter.
    pub op: FilterOp,
    /// Also search the variations of the games that have them (off in
    /// Scid). The main line is searched first; for a match found in a
    /// variation the filter value uses the ply where the outermost
    /// variation starts if the first move is inside it.
    pub in_variations: bool,
    /// The plies where the first move may be: first and last, inclusive.
    /// Default 0 to [`DEFAULT_LAST_START_PLY`].
    pub start_plies: (u16, u16),
    /// Use the shape pre-test of D3230. It never changes the result; turn it
    /// off only to test or to measure.
    pub fast_path: bool,
}

impl MovesSearch {
    /// Reads a list of words separated by white space.
    ///
    /// Move numbers (`12.`, `12...`) are skipped, and so are the prefixes
    /// of words (`12.e4`). If a word ends in `+` or `#` and `check_test` is
    /// lower, the test is raised (`+` needs `Check`, `#` needs `Mate`):
    /// compare [`MovesSearch::check_test`] with the one asked for to tell the
    /// user.
    ///
    /// # Errors
    ///
    /// [`SearchError::EmptyMoveList`], [`SearchError::FirstMoveIsWildcard`]
    /// and [`SearchError::BadMoveToken`] (also for more than
    /// [`MAX_MOVE_TOKENS`] words, reporting the first extra one).
    pub fn parse(
        text: &str,
        side: SideFilter,
        check_test: CheckTest,
        op: FilterOp,
    ) -> Result<MovesSearch, SearchError> {
        let mut tokens = Vec::new();
        let mut check_test = check_test;
        for word in text.split_whitespace() {
            let Some((token, needs)) = parse_word(word)? else {
                continue;
            };
            if tokens.len() >= MAX_MOVE_TOKENS {
                return Err(bad_token(word));
            }
            check_test = check_test.max(needs);
            tokens.push(token);
        }
        match tokens.first() {
            None => return Err(SearchError::EmptyMoveList),
            Some(MoveToken::Any) => return Err(SearchError::FirstMoveIsWildcard),
            Some(MoveToken::San(_)) => {}
        }
        Ok(MovesSearch {
            tokens,
            side,
            check_test,
            op,
            in_variations: false,
            start_plies: (0, DEFAULT_LAST_START_PLY),
            fast_path: true,
        })
    }

    /// Checks what [`MovesSearch::parse`] guarantees, for a search built by
    /// hand.
    ///
    /// # Errors
    ///
    /// [`SearchError::EmptyMoveList`], [`SearchError::FirstMoveIsWildcard`],
    /// [`SearchError::BadMoveToken`] (a word that is not a canonical SAN or
    /// too many words) and [`SearchError::CheckTestTooLow`] (a word ends in
    /// `+` or `#` but [`MovesSearch::check_test`] is lower, D3257: such a
    /// search would never match).
    pub fn validate(&self) -> Result<(), SearchError> {
        match self.tokens.first() {
            None => return Err(SearchError::EmptyMoveList),
            Some(MoveToken::Any) => return Err(SearchError::FirstMoveIsWildcard),
            Some(MoveToken::San(_)) => {}
        }
        if self.tokens.len() > MAX_MOVE_TOKENS {
            return Err(bad_token("too many words"));
        }
        for token in &self.tokens {
            if let MoveToken::San(text) = token {
                match parse_word(text) {
                    Ok(Some((MoveToken::San(t), needs))) if &t == text => {
                        if needs > self.check_test {
                            return Err(SearchError::CheckTestTooLow {
                                token: text.chars().take(MAX_TOKEN_LEN).collect(),
                            });
                        }
                    }
                    _ => return Err(bad_token(text)),
                }
            }
        }
        Ok(())
    }
}

/// Most entries reserved up front for the moves of one game (the cap keeps
/// the reservation small whatever the start range).
const MAX_ENTRIES_CAPACITY: usize = 512;

fn bad_token(word: &str) -> SearchError {
    SearchError::BadMoveToken {
        token: word.chars().take(MAX_TOKEN_LEN).collect(),
    }
}

/// Reads one word. `Ok(None)` for a bare move number. The second value is
/// the check test the word needs.
fn parse_word(word: &str) -> Result<Option<(MoveToken, CheckTest)>, SearchError> {
    let fail = || bad_token(word);
    if word.len() > MAX_TOKEN_LEN || !word.is_ascii() {
        return Err(fail());
    }
    if word == "?" {
        return Ok(Some((MoveToken::Any, CheckTest::No)));
    }
    let mut text = word;
    // Move number: digits followed by dots.
    let after_digits = text.trim_start_matches(|c: char| c.is_ascii_digit());
    if after_digits.len() < text.len() && after_digits.starts_with('.') {
        text = after_digits.trim_start_matches('.');
        if text.is_empty() {
            return Ok(None);
        }
    }
    // Trailing en passant mark and annotations.
    let lower = text.to_ascii_lowercase();
    if lower.ends_with("e.p.") {
        text = text.get(..text.len() - 4).unwrap_or("");
    }
    let trimmed = text.trim_end_matches(['!', '?']);
    // A word that is only annotations (for example "!?") is not a move.
    if trimmed.is_empty() {
        return Err(fail());
    }
    text = trimmed;
    // At most one final marker (D3257): "Qh4++", "Qh4+#" and "Qh4#+" are
    // refused because the body then still holds a marker.
    let (body, needs) = if let Some(b) = text.strip_suffix('#') {
        (b, CheckTest::Mate)
    } else if let Some(b) = text.strip_suffix('+') {
        (b, CheckTest::Check)
    } else {
        (text, CheckTest::No)
    };
    let canonical = canonical_body(body).ok_or_else(fail)?;
    let suffix = match needs {
        CheckTest::No => "",
        CheckTest::Check => "+",
        CheckTest::Mate => "#",
    };
    Ok(Some((
        MoveToken::San(format!("{canonical}{suffix}")),
        needs,
    )))
}

fn is_file(b: u8) -> bool {
    (b'a'..=b'h').contains(&b)
}

fn is_rank(b: u8) -> bool {
    (b'1'..=b'8').contains(&b)
}

/// The SAN body (no check marker) in the form Scid writes, or `None` if the
/// text is not a move.
fn canonical_body(body: &str) -> Option<String> {
    match body {
        "O-O" | "o-o" | "0-0" => return Some("O-O".to_owned()),
        "O-O-O" | "o-o-o" | "0-0-0" => return Some("O-O-O".to_owned()),
        "--" => return Some("--".to_owned()),
        _ => {}
    }
    let bytes = body.as_bytes();
    let first = *bytes.first()?;
    if matches!(first, b'K' | b'Q' | b'R' | b'B' | b'N') {
        // Piece letter, optional disambiguation, optional x, destination.
        let (dest, before_x) = split_destination(bytes.get(1..)?)?;
        let (capture, before) = match before_x.split_last() {
            Some((b'x', head)) => (true, head),
            _ => (false, before_x),
        };
        let valid = match before {
            [] => true,
            [f] => is_file(*f) || is_rank(*f),
            [f, r] => is_file(*f) && is_rank(*r),
            _ => false,
        };
        if !valid {
            return None;
        }
        let mut out = String::new();
        out.push(char::from(first));
        out.extend(before.iter().map(|&b| char::from(b)));
        if capture {
            out.push('x');
        }
        out.extend(dest.iter().map(|&b| char::from(b)));
        return Some(out);
    }
    if !is_file(first) {
        return None;
    }
    // Pawn move: optional promotion piece at the end, with or without `=`.
    let (core, promotion) = match bytes {
        [head @ .., b'=', p] if matches!(p.to_ascii_uppercase(), b'Q' | b'R' | b'B' | b'N') => {
            (head, Some(p.to_ascii_uppercase()))
        }
        [.., r, p] if is_rank(*r) && matches!(*p, b'Q' | b'R' | b'B' | b'N') => {
            (bytes.split_last().map_or(bytes, |(_, h)| h), Some(*p))
        }
        _ => (bytes, None),
    };
    let (dest, before) = split_destination(core)?;
    let capture = match before {
        [] => false,
        [f, b'x'] if is_file(*f) => true,
        _ => return None,
    };
    let mut out = String::new();
    if capture {
        out.push(char::from(*before.first()?));
        out.push('x');
    }
    out.push_str(std::str::from_utf8(dest).ok()?);
    if let Some(p) = promotion {
        out.push('=');
        out.push(char::from(p));
    }
    Some(out)
}

/// Splits `...fr` into the destination `fr` and what precedes it.
fn split_destination(text: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = text.len().checked_sub(2)?;
    let (before, dest) = text.split_at(at);
    match dest {
        [f, r] if is_file(*f) && is_rank(*r) => Some((dest, before)),
        _ => None,
    }
}

/// What a word says about the move, without a position: used to skip the
/// SAN of moves that cannot match (D3230).
#[derive(Debug, Clone, Copy)]
enum Shape {
    Castle {
        kingside: bool,
    },
    Null,
    Move {
        role: Role,
        to: Square,
        promotion: Option<Role>,
    },
}

fn shape_of(text: &str) -> Option<Shape> {
    let body = text.trim_end_matches(['+', '#']);
    match body {
        "O-O" => return Some(Shape::Castle { kingside: true }),
        "O-O-O" => return Some(Shape::Castle { kingside: false }),
        "--" => return Some(Shape::Null),
        _ => {}
    }
    let (body, promotion) = match body.split_once('=') {
        Some((b, p)) => (
            b,
            Some(Role::from_char(p.chars().next()?.to_ascii_lowercase())?),
        ),
        None => (body, None),
    };
    let dest = body.get(body.len().checked_sub(2)?..)?;
    let to = dest.parse::<Square>().ok()?;
    let role = match body.chars().next()? {
        'K' => Role::King,
        'Q' => Role::Queen,
        'R' => Role::Rook,
        'B' => Role::Bishop,
        'N' => Role::Knight,
        _ => Role::Pawn,
    };
    Some(Shape::Move {
        role,
        to,
        promotion,
    })
}

/// A move of the main line with what the shape test needs.
#[derive(Clone, Copy)]
struct Entry {
    mv: RawMove,
    role: Role,
    turn: Color,
}

impl Entry {
    fn fits(self, shape: Shape) -> bool {
        match shape {
            Shape::Null => self.mv.is_null(),
            Shape::Castle { kingside } => {
                self.role == Role::King
                    && self.mv.from.file() == mychess_core::File::E
                    && self.mv.to.file()
                        == if kingside {
                            mychess_core::File::G
                        } else {
                            mychess_core::File::C
                        }
            }
            Shape::Move {
                role,
                to,
                promotion,
            } => {
                !self.mv.is_null()
                    && self.role == role
                    && self.mv.to == to
                    && self.mv.promotion == promotion
            }
        }
    }
}

/// Runs a search by moves over `db`, combining the result with `filter`
/// according to `query.op` (`sc_search moves`).
///
/// A cancelled search leaves `filter` unchanged; games that cannot be read
/// (including moves that are not legal, which the SAN needs) do not match and
/// are listed in the report.
///
/// # Errors
///
/// The errors of [`MovesSearch::validate`] (nothing is changed) and
/// [`SearchError::FilterSize`].
pub fn search_moves<D: GameSource + ?Sized>(
    db: &mut D,
    query: &MovesSearch,
    filter: &mut Filter,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<SearchReport, SearchError> {
    query.validate()?;
    let matcher = MovesMatcher::new(query);
    scan_games(db, &matcher, query.op, filter, progress, cancel)
}

struct MovesMatcher {
    /// Per word: the text and its shape (`None` for `?`).
    words: Vec<Option<(String, Option<Shape>)>>,
    side: SideFilter,
    suffix: SanSuffix,
    in_variations: bool,
    first: usize,
    last: usize,
    fast_path: bool,
}

impl MovesMatcher {
    fn new(query: &MovesSearch) -> MovesMatcher {
        MovesMatcher {
            words: query
                .tokens
                .iter()
                .map(|t| match t {
                    MoveToken::Any => None,
                    MoveToken::San(text) => Some((text.clone(), shape_of(text))),
                })
                .collect(),
            side: query.side,
            suffix: match query.check_test {
                CheckTest::No => SanSuffix::None,
                CheckTest::Check => SanSuffix::Check,
                CheckTest::Mate => SanSuffix::CheckOrMate,
            },
            in_variations: query.in_variations,
            first: usize::from(query.start_plies.0),
            last: usize::from(query.start_plies.1),
            fast_path: query.fast_path,
        }
    }

    /// Whether the words could start at `start` of `entries`: the moves
    /// exist, the side is allowed and the shapes fit.
    fn could_start(&self, entries: &[Entry], start: usize) -> bool {
        let Some(first) = entries.get(start) else {
            return false;
        };
        if !self.side.allows(first.turn) {
            return false;
        }
        self.words.iter().enumerate().all(|(j, word)| {
            let Some(entry) = entries.get(start + j) else {
                return false;
            };
            match word {
                Some((_, Some(shape))) if self.fast_path => entry.fits(*shape),
                // A word without a shape cannot be a legal SAN.
                Some((_, None)) => false,
                _ => true,
            }
        })
    }

    /// Compares the SAN of the moves from `start` with the words.
    fn confirm(
        &self,
        replay: &mut Replay<'_>,
        entries: &[Entry],
        start: usize,
    ) -> Result<bool, RecordError> {
        for (j, word) in self.words.iter().enumerate() {
            let Some((text, _)) = word else {
                continue;
            };
            let (pos, mv) = replay.at(entries, start + j)?;
            let san = pos
                .san_with(mv, self.suffix)
                .map_err(|_| RecordError::new(0, "move is not legal"))?;
            if &san != text {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The first start in `entries` (all of them, main line) that matches.
    fn first_match(
        &self,
        replay: &mut Replay<'_>,
        entries: &[Entry],
    ) -> Result<Option<usize>, RecordError> {
        let last = self.last.min(entries.len().saturating_sub(1));
        for start in self.first..=last {
            if self.could_start(entries, start) && self.confirm(replay, entries, start)? {
                return Ok(Some(start));
            }
        }
        Ok(None)
    }
}

/// The positions of a line, built when a candidate needs them.
struct Replay<'a> {
    positions: Vec<Position>,
    start: &'a Position,
}

impl<'a> Replay<'a> {
    fn new(start: &'a Position) -> Replay<'a> {
        Replay {
            positions: vec![start.clone()],
            start,
        }
    }

    /// The position before move `index` and the move in core form.
    fn at(&mut self, entries: &[Entry], index: usize) -> Result<(&Position, Move), RecordError> {
        if self.positions.is_empty() {
            self.positions.push(self.start.clone());
        }
        while self.positions.len() <= index {
            let k = self.positions.len() - 1;
            let (Some(pos), Some(entry)) = (self.positions.get(k), entries.get(k)) else {
                return Err(RecordError::new(0, "move out of range"));
            };
            let mv = to_core(pos, entry.mv)?;
            let next = pos
                .after(mv)
                .map_err(|_| RecordError::new(0, "move is not legal"))?;
            self.positions.push(next);
        }
        let (Some(pos), Some(entry)) = (self.positions.get(index), entries.get(index)) else {
            return Err(RecordError::new(0, "move out of range"));
        };
        let mv = to_core(pos, entry.mv)?;
        Ok((pos, mv))
    }
}

fn to_core(pos: &Position, mv: RawMove) -> Result<Move, RecordError> {
    if mv.is_null() {
        return Ok(Move::Null);
    }
    pos.find_move(mv.from, mv.to, mv.promotion)
        .ok_or(RecordError::new(0, "move is not legal"))
}

fn start_position(record: &[u8]) -> Result<Position, RecordError> {
    match start_fen_of_record(record)? {
        None => Ok(Position::new()),
        Some(fen) => Position::from_fen(&String::from_utf8_lossy(&fen))
            .map_err(|_| RecordError::new(0, "start position is not usable")),
    }
}

fn filter_value(ply: usize) -> u8 {
    u8::try_from(ply + 2).unwrap_or(u8::MAX)
}

impl GameMatcher for MovesMatcher {
    fn prefilter(&self, _entry: &IndexEntry) -> Shortcut {
        // The index says nothing about a sequence of moves (D3207).
        Shortcut::Examine
    }

    fn evaluate(&self, entry: &IndexEntry, record: &[u8]) -> Result<Option<u8>, RecordError> {
        // Moves needed: up to the last start plus the words.
        let limit = self.last.saturating_add(self.words.len());
        let mut entries: Vec<Entry> = Vec::with_capacity(limit.min(MAX_ENTRIES_CAPACITY));
        let walked = walk_mainline(record, |pos, next| {
            let Some(mv) = next else {
                return ControlFlow::Continue(());
            };
            let role = pos.piece_at(mv.from).map_or(Role::King, |p| p.role);
            entries.push(Entry {
                mv,
                role,
                turn: pos.turn(),
            });
            if entries.len() >= limit {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        // Damage after the match does not exclude the game.
        let mut shapes_possible = false;
        let last = self.last.min(entries.len().saturating_sub(1));
        for start in self.first..=last {
            if self.could_start(&entries, start) {
                shapes_possible = true;
                break;
            }
        }
        if shapes_possible {
            let begin = start_position(record)?;
            let mut replay = Replay::new(&begin);
            if let Some(start) = self.first_match(&mut replay, &entries)? {
                return Ok(Some(filter_value(start)));
            }
        }
        walked?;
        if self.in_variations && entry.variation_count > 0 {
            let raw = RawGame::parse(record)?;
            return Ok(self.search_variations(&raw, record)?.map(filter_value));
        }
        Ok(None)
    }
}

/// One open line of the depth-first walk.
struct Frame {
    line: usize,
    next: usize,
    var: usize,
    pos: ListPosition,
    path_base: usize,
    origin: Option<usize>,
}

impl MovesMatcher {
    /// Depth-first search of every line (main line included) for a match that
    /// ends at the move just played. Returns the ply for the filter value.
    fn search_variations(
        &self,
        raw: &RawGame,
        record: &[u8],
    ) -> Result<Option<usize>, RecordError> {
        let begin = start_position(record)?;
        let list_start = match &raw.start_fen {
            Some(fen) => ListPosition::from_fen(&String::from_utf8_lossy(fen))
                .map_err(|e| RecordError::new(0, e.reason()))?,
            None => ListPosition::standard(),
        };
        let mut path: Vec<Entry> = Vec::new();
        let mut stack = vec![Frame {
            line: 0,
            next: 0,
            var: 0,
            pos: list_start,
            path_base: 0,
            origin: None,
        }];
        while let Some(top) = stack.last_mut() {
            let node = raw.lines.get(top.line).and_then(|l| l.moves.get(top.next));
            let Some(node) = node else {
                path.truncate(top.path_base);
                stack.pop();
                continue;
            };
            if let Some(&child) = node.variations.get(top.var) {
                top.var += 1;
                let frame = Frame {
                    line: child,
                    next: 0,
                    var: 0,
                    pos: top.pos.clone(),
                    path_base: path.len(),
                    // The outermost variation starts at this many plies.
                    origin: top.origin.or(Some(path.len())),
                };
                stack.push(frame);
                continue;
            }
            let role = top
                .pos
                .piece_at(node.mv.from)
                .map_or(Role::King, |p| p.role);
            path.push(Entry {
                mv: node.mv,
                role,
                turn: top.pos.turn(),
            });
            top.pos
                .make(node.mv)
                .map_err(|reason| RecordError::new(0, reason))?;
            top.next += 1;
            top.var = 0;
            let end = path.len() - 1;
            let len = self.words.len();
            if end + 1 >= len {
                let start = end + 1 - len;
                if start >= self.first && start <= self.last && self.could_start(&path, start) {
                    let mut replay = Replay::new(&begin);
                    if self.confirm(&mut replay, &path, start)? {
                        let ply = top.origin.map_or(start, |o| o.min(start));
                        return Ok(Some(ply));
                    }
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
