// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_search_board`) and
// src/game.cpp (`Game::ExactMatch`, `Game::VarExactMatch`). Copyright (c)
// 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The search by position (`sc_search board`; M6-1a, decisions D3208 to
//! D3214 of `docs/specs/m6-1-buscas-avancadas.md`).
//!
//! [`search_board`] looks, in the main line of each game, for the first
//! position that matches the searched one according to a [`BoardMatch`]:
//! the side to move and the piece counts must always be equal, and then
//!
//! - `Exact`: every piece is on the same square;
//! - `Pawns`: every pawn is on the same square;
//! - `Files`: every file has the same number of pawns of each colour;
//! - `Material`: nothing more.
//!
//! Castling rights, en passant, clocks and move numbers never count. With
//! [`BoardSearch::ignore_colors`] the position turned upside down with the
//! colours (and the side to move) swapped is searched too, after the normal
//! one in each game. With [`BoardSearch::in_variations`], games that have
//! variations are searched in them as well, after both main-line passes.
//!
//! The filter value of a match is its ply plus 1 (at most 255). For a match
//! inside a variation the ply is the one of the main-line position where the
//! outermost variation starts, as in Scid.
//!
//! The "search in another database" of Scid needs no special support: pass
//! that database and its filter; the position comes from the current game of
//! the caller.

use mychess_core::{Color, Piece, Position, Role};

use super::header::{SearchError, SearchReport};
use super::scan::{GameMatcher, Shortcut, scan_games};
use super::sig;
use crate::filter::{Filter, FilterOp};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::derive::{home_pawn_signature, material_signature};
use crate::si4::{IndexEntry, ListPosition, RawGame, RecordError, walk_mainline};
use crate::source::GameSource;

/// What a position must have in common with the searched one besides the
/// side to move and the piece counts (Scid's `searchType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BoardMatch {
    /// All pieces on the same squares.
    #[default]
    Exact,
    /// All pawns on the same squares; the pieces anywhere.
    Pawns,
    /// The same number of pawns of each colour on every file.
    Files,
    /// Only the material.
    Material,
}

/// The options of a search by position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardSearch {
    /// The searched position. Only the squares and the side to move count.
    pub position: Position,
    /// The kind of match.
    pub kind: BoardMatch,
    /// Also look in the variations of the games that have them.
    pub in_variations: bool,
    /// Also search the position with the colours swapped (Scid's "ignore
    /// colors"): the board is flipped vertically, the pieces change colour
    /// and so does the side to move. There is no left-right mirroring.
    pub ignore_colors: bool,
    /// How the result combines with the current filter.
    pub op: FilterOp,
    /// Use the shortcuts of the index (D3207). They never change the result;
    /// turn them off only to test or to measure.
    pub shortcuts: bool,
}

impl BoardSearch {
    /// An `Exact` search of `position`, operation [`FilterOp::Reset`].
    pub fn new(position: Position) -> BoardSearch {
        BoardSearch {
            position,
            kind: BoardMatch::Exact,
            in_variations: false,
            ignore_colors: false,
            op: FilterOp::Reset,
            shortcuts: true,
        }
    }
}

/// Searches `db` for the position of `query`, combining the result with
/// `filter` according to `query.op` (`sc_search board`).
///
/// Games that cannot be read go to [`SearchReport::unreadable`]; damage after
/// the match does not exclude the game. A cancelled search leaves `filter`
/// unchanged (D3202). The result does not depend on the number of threads.
///
/// ```no_run
/// use mychess_core::Position;
/// use mychess_db::si4::Si4Database;
/// use mychess_db::{BoardSearch, CancelToken, Filter, NoProgress, search_board};
///
/// let mut db = Si4Database::open("games/tactics".as_ref())?;
/// let mut filter = Filter::new(db.len());
/// let query = BoardSearch::new(Position::new());
/// let report = search_board(&mut db, &query, &mut filter, &mut NoProgress, &CancelToken::new())?;
/// println!("{} / {}", report.matched, report.start_count);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
///
/// [`SearchError::UnsupportedPosition`] if the position does not fit the SI4
/// piece lists (nothing is changed); [`SearchError::FilterSize`].
pub fn search_board<D: GameSource + ?Sized>(
    db: &mut D,
    query: &BoardSearch,
    filter: &mut Filter,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<SearchReport, SearchError> {
    let matcher = BoardMatcher::new(query)?;
    scan_games(db, &matcher, query.op, filter, progress, cancel)
}

/// The searched position with what the comparison needs, precomputed.
struct Target {
    board: [Option<Piece>; 64],
    turn: Color,
    counts: Counts,
    /// Pawns per file, for `Files`.
    files: [[u8; 8]; 2],
    hp_sig: u16,
    mat_sig: u32,
}

/// Piece counts of both colours: pawns, knights, bishops, rooks, queens and
/// the total (kings included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Counts {
    pieces: [[u8; 5]; 2],
    total: [u8; 2],
}

fn color_index(color: Color) -> usize {
    usize::from(color == Color::Black)
}

fn role_index(role: Role) -> Option<usize> {
    match role {
        Role::Pawn => Some(0),
        Role::Knight => Some(1),
        Role::Bishop => Some(2),
        Role::Rook => Some(3),
        Role::Queen => Some(4),
        Role::King => None,
    }
}

impl Counts {
    fn of(pos: &ListPosition) -> Counts {
        let mut counts = Counts::default();
        for color in [Color::White, Color::Black] {
            let c = color_index(color);
            let squares = pos.piece_squares(color);
            if let Some(total) = counts.total.get_mut(c) {
                *total = u8::try_from(squares.len()).unwrap_or(u8::MAX);
            }
            for &sq in squares {
                let slot = pos
                    .piece_at(sq)
                    .and_then(|p| role_index(p.role))
                    .and_then(|r| counts.pieces.get_mut(c).and_then(|row| row.get_mut(r)));
                if let Some(slot) = slot {
                    *slot += 1;
                }
            }
        }
        counts
    }

    /// Whether `self` has fewer pieces in total or fewer pawns than `other`
    /// for some colour: such a position can never be followed by one with
    /// the counts of `other` (captures only remove material).
    fn fewer_than(&self, other: &Counts) -> bool {
        (0..2).any(|c| {
            self.total.get(c) < other.total.get(c)
                || self.pieces.get(c).and_then(|p| p.first())
                    < other.pieces.get(c).and_then(|p| p.first())
        })
    }
}

fn pawn_files(pos: &ListPosition) -> [[u8; 8]; 2] {
    let mut files = [[0u8; 8]; 2];
    for color in [Color::White, Color::Black] {
        for &sq in pos.piece_squares(color) {
            if pos.piece_at(sq).is_some_and(|p| p.role == Role::Pawn)
                && let Some(slot) = files
                    .get_mut(color_index(color))
                    .and_then(|row| row.get_mut(sq.file().to_usize()))
            {
                *slot += 1;
            }
        }
    }
    files
}

impl Target {
    fn new(pos: &ListPosition) -> Target {
        let mut board = [None; 64];
        for (sq, cell) in mychess_core::Square::ALL.iter().zip(board.iter_mut()) {
            *cell = pos.piece_at(*sq);
        }
        Target {
            board,
            turn: pos.turn(),
            counts: Counts::of(pos),
            files: pawn_files(pos),
            hp_sig: home_pawn_signature(pos),
            mat_sig: material_signature(pos),
        }
    }

    fn piece_at(&self, sq: mychess_core::Square) -> Option<Piece> {
        self.board.get(sq.to_usize()).copied().flatten()
    }

    /// Whether `pos` (same side to move, same counts) matches.
    fn same_squares(&self, kind: BoardMatch, pos: &ListPosition) -> bool {
        match kind {
            BoardMatch::Material => true,
            BoardMatch::Files => pawn_files(pos) == self.files,
            BoardMatch::Exact | BoardMatch::Pawns => {
                let pawns_only = kind == BoardMatch::Pawns;
                [Color::White, Color::Black].iter().all(|&color| {
                    pos.piece_squares(color).iter().all(|&sq| {
                        let here = pos.piece_at(sq);
                        (pawns_only && here.is_none_or(|p| p.role != Role::Pawn))
                            || self.piece_at(sq) == here
                    })
                })
            }
        }
    }
}

/// `pos` seen from the other side of the board: ranks mirrored, colours and
/// side to move swapped (`PrintCompactStrFlipped`). `None` if it cannot be
/// represented (it always can for a position that was valid).
fn flipped(pos: &ListPosition) -> Option<ListPosition> {
    let mut fen = String::new();
    for rank in (0..8usize).rev() {
        let mut empty = 0u8;
        for file in 0..8usize {
            let sq = mychess_core::Square::try_from(((rank * 8) + file) ^ 0b11_1000).ok()?;
            match pos.piece_at(sq) {
                None => empty += 1,
                Some(piece) => {
                    if empty > 0 {
                        fen.push(char::from(b'0' + empty));
                        empty = 0;
                    }
                    let swapped = Piece {
                        color: !piece.color,
                        role: piece.role,
                    };
                    fen.push(swapped.char());
                }
            }
        }
        if empty > 0 {
            fen.push(char::from(b'0' + empty));
        }
        if rank > 0 {
            fen.push('/');
        }
    }
    fen.push_str(if pos.turn() == Color::White {
        " b"
    } else {
        " w"
    });
    ListPosition::from_fen(&fen).ok()
}

/// The first hits of the two candidates during a walk.
struct Probe<'a> {
    kind: BoardMatch,
    targets: &'a [Target],
    /// Per target: dead (can never match later in this line).
    dead: [bool; 2],
    prune: bool,
    hits: [Option<usize>; 2],
}

impl<'a> Probe<'a> {
    fn new(kind: BoardMatch, targets: &'a [Target], prune: bool) -> Probe<'a> {
        Probe {
            kind,
            targets,
            dead: [false; 2],
            prune,
            hits: [None; 2],
        }
    }

    /// Target `i` can never match later in this line.
    fn mark_dead(&mut self, i: usize) {
        if let Some(d) = self.dead.get_mut(i) {
            *d = true;
        }
    }

    fn visit(&mut self, pos: &ListPosition, at: usize) {
        let counts = Counts::of(pos);
        for (i, target) in self.targets.iter().enumerate() {
            if self.hits.get(i).copied().flatten().is_some()
                || self.dead.get(i).copied().unwrap_or(true)
            {
                continue;
            }
            if self.prune && counts.fewer_than(&target.counts) {
                if let Some(d) = self.dead.get_mut(i) {
                    *d = true;
                }
                continue;
            }
            if pos.turn() == target.turn
                && counts == target.counts
                && target.same_squares(self.kind, pos)
                && let Some(hit) = self.hits.get_mut(i)
            {
                *hit = Some(at);
            }
        }
    }

    /// Nothing more can change the answer: the normal candidate hit, or every
    /// candidate has hit or is dead.
    fn done(&self) -> bool {
        if self.hits[0].is_some() {
            return true;
        }
        (0..self.targets.len()).all(|i| {
            self.hits.get(i).copied().flatten().is_some() || self.dead.get(i).copied() == Some(true)
        })
    }

    /// The normal hit wins over the flipped one.
    fn first_hit(&self) -> Option<usize> {
        self.hits.iter().flatten().next().copied()
    }
}

struct BoardMatcher {
    kind: BoardMatch,
    in_variations: bool,
    shortcuts: bool,
    /// The normal target, then the flipped one if asked.
    targets: Vec<Target>,
}

impl BoardMatcher {
    fn new(query: &BoardSearch) -> Result<BoardMatcher, SearchError> {
        let list = ListPosition::from_fen(&query.position.fen())
            .map_err(|_| SearchError::UnsupportedPosition)?;
        let mut targets = vec![Target::new(&list)];
        if query.ignore_colors {
            let other = flipped(&list).ok_or(SearchError::UnsupportedPosition)?;
            targets.push(Target::new(&other));
        }
        Ok(BoardMatcher {
            kind: query.kind,
            in_variations: query.in_variations,
            shortcuts: query.shortcuts,
            targets,
        })
    }

    fn use_variations(&self, entry: &IndexEntry) -> bool {
        self.in_variations && entry.variation_count > 0
    }

    /// Whether the index lets `target` match this game (`possibleMatch`).
    fn possible(&self, target: &Target, entry: &IndexEntry) -> bool {
        if matches!(self.kind, BoardMatch::Exact | BoardMatch::Pawns)
            && !sig::hp_possible_match(target.hp_sig, entry)
        {
            return false;
        }
        let (promos, upromo) = sig::promotion_flags(entry);
        sig::is_reachable(target.mat_sig, entry.final_mat_sig, promos, upromo)
    }
}

impl GameMatcher for BoardMatcher {
    fn prefilter(&self, entry: &IndexEntry) -> Shortcut {
        if !self.shortcuts || !sig::index_is_trusted(entry) || self.use_variations(entry) {
            return Shortcut::Examine;
        }
        if self.targets.iter().any(|t| self.possible(t, entry)) {
            Shortcut::Examine
        } else {
            Shortcut::Skip
        }
    }

    fn evaluate(&self, entry: &IndexEntry, record: &[u8]) -> Result<Option<u8>, RecordError> {
        let mut probe = Probe::new(self.kind, &self.targets, true);
        if self.shortcuts && sig::index_is_trusted(entry) {
            // The index already discarded these targets for the main line
            // (they may still be reached in a variation, which gets a fresh
            // probe), so do not test them at every ply.
            for (i, target) in self.targets.iter().enumerate() {
                if !self.possible(target, entry) {
                    probe.mark_dead(i);
                }
            }
        }
        let mut ply = 0usize;
        let walked = walk_mainline(record, |pos, _| {
            probe.visit(pos, ply);
            ply += 1;
            if probe.done() {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        });
        if let Some(at) = probe.first_hit() {
            return Ok(Some(filter_value(at)));
        }
        walked?;
        if !self.use_variations(entry) {
            return Ok(None);
        }
        let raw = RawGame::parse(record)?;
        let mut probe = Probe::new(self.kind, &self.targets, false);
        search_variations(&raw, &mut probe)?;
        Ok(probe.first_hit().map(filter_value))
    }
}

/// Ply plus 1, limited to 255 (D3214).
fn filter_value(ply: usize) -> u8 {
    u8::try_from(ply + 1).unwrap_or(u8::MAX)
}

/// One open line of the depth-first walk.
struct Frame {
    line: usize,
    /// Index of the next move to play.
    next: usize,
    /// Index of the next variation of that move to enter.
    var: usize,
    /// Position before move `next`.
    pos: ListPosition,
    /// Whether the position was already offered to the probe.
    seen: bool,
    /// Main-line ply where the outermost variation starts; `None` in the
    /// main line itself.
    origin: Option<usize>,
}

/// `VarExactMatch`: every position of the main line and of all the
/// variations, in depth-first order (the position, then the variations of
/// the move played from it, then the move). Hits inside a variation are
/// reported at the ply where the outermost variation starts.
fn search_variations(raw: &RawGame, probe: &mut Probe<'_>) -> Result<(), RecordError> {
    let start = match &raw.start_fen {
        Some(fen) => ListPosition::from_fen(&String::from_utf8_lossy(fen))
            .map_err(|e| RecordError::new(0, e.reason()))?,
        None => ListPosition::standard(),
    };
    let mut stack = vec![Frame {
        line: 0,
        next: 0,
        var: 0,
        pos: start,
        seen: false,
        origin: None,
    }];
    while let Some(top) = stack.last_mut() {
        if !top.seen {
            top.seen = true;
            probe.visit(&top.pos, top.origin.unwrap_or(top.next));
            if probe.done() {
                return Ok(());
            }
        }
        let node = raw.lines.get(top.line).and_then(|l| l.moves.get(top.next));
        let Some(node) = node else {
            stack.pop();
            continue;
        };
        if let Some(&child) = node.variations.get(top.var) {
            top.var += 1;
            let origin = top.origin.or(Some(top.next));
            let frame = Frame {
                line: child,
                next: 0,
                var: 0,
                pos: top.pos.clone(),
                seen: true,
                origin,
            };
            stack.push(frame);
            continue;
        }
        top.pos
            .make(node.mv)
            .map_err(|reason| RecordError::new(0, reason))?;
        top.next += 1;
        top.var = 0;
        top.seen = false;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
