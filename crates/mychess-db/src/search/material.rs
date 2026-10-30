// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_search_material`,
// `parsePattern`, `flipPattern`) and src/game.cpp (`patternsMatch`,
// `Game::MaterialMatch`), and from tcl/search/material.tcl (presets).
// Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The search by material and patterns (`sc_search material`; M6-1a,
//! decisions D3215 to D3224 of `docs/specs/m6-1-buscas-avancadas.md`).
//!
//! [`search_material`] walks the main line of each game from the start
//! position to the end. A position (a "ply" `k` is the position after `k`
//! half moves) satisfies a [`MaterialSearch`] when all of this holds:
//!
//! - its ply is inside the move range;
//! - every piece count is inside its [`PieceRange`] (bishops plus knights
//!   also have their own "minor" range);
//! - if each side has exactly one bishop, they obey [`BishopColors`];
//! - the material difference (White minus Black, Q=9 R=5 B=3 N=3 P=1) is in
//!   the difference range;
//! - every [`Pattern`] holds.
//!
//! A game matches when at least [`MaterialSearch::min_plies`] consecutive
//! plies satisfy it; the filter value is the first ply of the run plus 1.
//! With [`MaterialSearch::end_only`] only the final position counts.
//!
//! ## Deviations from Scid (all documented decisions)
//!
//! - D3215: the normalisation of the minor-piece range is the same for both
//!   colours (Scid uses `max[BB]` for Black's minimum, a typo).
//! - D3218: a pattern with a piece but no file and no rank is an error
//!   ([`SearchError::InvalidPattern`]); Scid lets it pass silently.
//! - D3219: with "ignore colors" the second pass also negates and swaps the
//!   difference range; Scid leaves it as it was.
//! - D3220: a bishop-colour failure fails that ply only, not the game.
//! - D3222: the difference range is optional (no restriction by default).
//! - D3223: the filter value is limited to 255 instead of wrapping around.

use std::ops::ControlFlow;

use mychess_core::{Color, File, Piece, Position, Rank, Role};

use super::header::{SearchError, SearchReport};
use super::scan::{GameMatcher, Shortcut, scan_games};
use super::sig;
use crate::filter::{Filter, FilterOp};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{IndexEntry, ListPosition, RecordError, walk_mainline};
use crate::source::GameSource;

/// Most patterns a search may have (the window of Scid shows 10).
pub const MAX_PATTERNS: usize = 32;

/// An inclusive range of the number of pieces of one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PieceRange {
    /// Fewest pieces.
    pub min: u8,
    /// Most pieces.
    pub max: u8,
}

impl PieceRange {
    /// A range.
    pub const fn new(min: u8, max: u8) -> PieceRange {
        PieceRange { min, max }
    }
}

/// The ranges of one colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SideRanges {
    /// Queens.
    pub queen: PieceRange,
    /// Rooks.
    pub rook: PieceRange,
    /// Bishops.
    pub bishop: PieceRange,
    /// Knights.
    pub knight: PieceRange,
    /// Minor pieces: bishops plus knights.
    pub minor: PieceRange,
    /// Pawns.
    pub pawn: PieceRange,
}

impl SideRanges {
    /// The defaults of the Scid window: any material (0 to 2 of each piece,
    /// 0 to 4 minor pieces, 0 to 8 pawns).
    pub const ANY: SideRanges = SideRanges {
        queen: PieceRange::new(0, 2),
        rook: PieceRange::new(0, 2),
        bishop: PieceRange::new(0, 2),
        knight: PieceRange::new(0, 2),
        minor: PieceRange::new(0, 4),
        pawn: PieceRange::new(0, 8),
    };

    /// No piece and no pawn (only the king).
    pub const ZERO: SideRanges = SideRanges {
        queen: PieceRange::new(0, 0),
        rook: PieceRange::new(0, 0),
        bishop: PieceRange::new(0, 0),
        knight: PieceRange::new(0, 0),
        minor: PieceRange::new(0, 0),
        pawn: PieceRange::new(0, 0),
    };

    /// The ranges as an array in the order queen, rook, bishop, knight,
    /// pawn, minor.
    fn to_array(self) -> [PieceRange; 6] {
        [
            self.queen,
            self.rook,
            self.bishop,
            self.knight,
            self.pawn,
            self.minor,
        ]
    }

    fn from_array(a: [PieceRange; 6]) -> SideRanges {
        let [queen, rook, bishop, knight, pawn, minor] = a;
        SideRanges {
            queen,
            rook,
            bishop,
            knight,
            minor,
            pawn,
        }
    }

    /// The range of `role`, `None` for the king.
    pub fn get_mut(&mut self, role: Role) -> Option<&mut PieceRange> {
        match role {
            Role::Queen => Some(&mut self.queen),
            Role::Rook => Some(&mut self.rook),
            Role::Bishop => Some(&mut self.bishop),
            Role::Knight => Some(&mut self.knight),
            Role::Pawn => Some(&mut self.pawn),
            Role::King => None,
        }
    }
}

impl Default for SideRanges {
    fn default() -> SideRanges {
        SideRanges::ANY
    }
}

/// What the colours of the two bishops must be (only when each side has
/// exactly one bishop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BishopColors {
    /// No restriction.
    #[default]
    Either,
    /// Both bishops on squares of the same colour.
    Same,
    /// The bishops on squares of opposite colours.
    Opposite,
}

/// A condition on the board: a piece must (or must not) be on a square, on
/// a file or on a rank. A pattern without a piece is ignored; a pattern with
/// a piece needs a file or a rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pattern {
    /// The piece; `None` for an empty pattern.
    pub piece: Option<Piece>,
    /// The file, or any file.
    pub file: Option<File>,
    /// The rank, or any rank.
    pub rank: Option<Rank>,
    /// `true`: the piece must be there; `false`: it must not.
    pub present: bool,
}

impl Default for Pattern {
    fn default() -> Pattern {
        Pattern {
            piece: None,
            file: None,
            rank: None,
            present: true,
        }
    }
}

impl Pattern {
    /// A pattern for a piece on a square.
    pub fn on_square(piece: Piece, file: File, rank: Rank, present: bool) -> Pattern {
        Pattern {
            piece: Some(piece),
            file: Some(file),
            rank: Some(rank),
            present,
        }
    }

    /// A pattern for a piece on a file.
    pub fn on_file(piece: Piece, file: File, present: bool) -> Pattern {
        Pattern {
            piece: Some(piece),
            file: Some(file),
            rank: None,
            present,
        }
    }

    /// Reads the four fields as the Scid files and window write them: the
    /// piece as colour and type (`wp`, `bk`, ...) or `?`, the file as `a` to
    /// `h` or `?`, the rank as `1` to `8` or `?`, and the sense as `Yes`/`No`
    /// (first letter, any case; also `1`/`0`, `true`/`false`).
    ///
    /// # Errors
    ///
    /// [`SearchError::InvalidPattern`] for an unknown piece, file, rank or
    /// sense.
    pub fn from_scid(
        piece: &str,
        file: &str,
        rank: &str,
        present: &str,
    ) -> Result<Pattern, SearchError> {
        let bad = |reason| SearchError::InvalidPattern { reason };
        let piece = if piece.trim() == "?" {
            None
        } else {
            let mut chars = piece.trim().chars();
            let (Some(c), Some(r), None) = (chars.next(), chars.next(), chars.next()) else {
                return Err(bad("piece must be a colour and a type, such as wp"));
            };
            let color = match c.to_ascii_lowercase() {
                'w' => Color::White,
                'b' => Color::Black,
                _ => return Err(bad("piece colour must be w or b")),
            };
            let role = Role::from_char(r.to_ascii_lowercase())
                .ok_or(bad("piece type must be one of kqrbnp"))?;
            Some(Piece { color, role })
        };
        let file = match file.trim() {
            "?" => None,
            text => {
                let mut chars = text.chars();
                match (chars.next().and_then(File::from_char), chars.next()) {
                    (Some(f), None) => Some(f),
                    _ => return Err(bad("file must be a to h or ?")),
                }
            }
        };
        let rank = match rank.trim() {
            "?" => None,
            text => {
                let mut chars = text.chars();
                match (chars.next().and_then(Rank::from_char), chars.next()) {
                    (Some(r), None) => Some(r),
                    _ => return Err(bad("rank must be 1 to 8 or ?")),
                }
            }
        };
        let present = match present
            .trim()
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase())
        {
            Some('y' | 't' | '1') => true,
            Some('n' | 'f' | '0') => false,
            _ => return Err(bad("sense must be Yes or No")),
        };
        Ok(Pattern {
            piece,
            file,
            rank,
            present,
        })
    }

    /// The pattern with the board flipped: the rank `r` becomes `9 - r` and
    /// the piece changes colour (`flipPattern`).
    fn flipped(self) -> Pattern {
        Pattern {
            piece: self.piece.map(|p| Piece {
                color: !p.color,
                role: p.role,
            }),
            file: self.file,
            rank: self.rank.map(Rank::flip_vertical),
            present: self.present,
        }
    }

    /// The bit of the home-pawn signature this pattern excludes, if it says
    /// "no pawn of this colour on file f" (`hpExcludeMask`).
    fn home_pawn_bit(self) -> Option<u16> {
        let piece = self.piece?;
        if self.present || piece.role != Role::Pawn || self.rank.is_some() {
            return None;
        }
        let file = self.file?.to_u32();
        let bit = match piece.color {
            Color::White => 15 - file,
            Color::Black => 7 - file,
        };
        Some(1u16 << bit)
    }
}

/// The options of a material search.
///
/// `Default` is the "any material" search of the Scid window with the
/// operation [`FilterOp::Reset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialSearch {
    /// White's ranges.
    pub white: SideRanges,
    /// Black's ranges.
    pub black: SideRanges,
    /// The material difference (White minus Black) range; `None` = no
    /// restriction (D3222). Reversed ends are swapped.
    pub diff: Option<(i16, i16)>,
    /// The bishop-colour restriction.
    pub bishops: BishopColors,
    /// The first and last move number (not ply) of the positions tested,
    /// 0 to 999. A first move of 0 also tests the start position; 1 starts
    /// after White's first move. Default 1 to 999.
    pub moves: (u16, u16),
    /// The number of consecutive half moves that must satisfy the search
    /// ("match for at least"); 0 counts as 1.
    pub min_plies: u16,
    /// Test only the final position, ignoring the move range and
    /// [`MaterialSearch::min_plies`]. A game without moves never matches.
    pub end_only: bool,
    /// Also try the colours swapped (D3219).
    pub ignore_colors: bool,
    /// The patterns; all must hold.
    pub patterns: Vec<Pattern>,
    /// How the result combines with the current filter.
    pub op: FilterOp,
    /// Use the shortcuts of the index (D3207); never changes the result.
    ///
    /// One of them, the stop "minimum not reached and no promotion can
    /// refill it", trusts the promotion flags of the **index entry** (the
    /// walker does not expose those of the record, D3221): it is only used
    /// on games without a `START` position whose index is trusted. Turn the
    /// shortcuts off to rely on the record alone.
    pub shortcuts: bool,
}

/// Largest move number of the range of a material search (the window of
/// Scid stops at 999).
const MAX_MOVE_NUMBER: u16 = 999;

impl Default for MaterialSearch {
    fn default() -> MaterialSearch {
        MaterialSearch {
            white: SideRanges::ANY,
            black: SideRanges::ANY,
            diff: None,
            bishops: BishopColors::Either,
            moves: (1, 999),
            min_plies: 1,
            end_only: false,
            ignore_colors: false,
            patterns: Vec::new(),
            op: FilterOp::Reset,
            shortcuts: true,
        }
    }
}

impl MaterialSearch {
    /// Only the two kings: all the ranges are 0 to 0 (the window's "Zero").
    pub fn zero() -> MaterialSearch {
        MaterialSearch {
            white: SideRanges::ZERO,
            black: SideRanges::ZERO,
            ..MaterialSearch::default()
        }
    }

    /// The window's "Current Board": every range is exactly what `position`
    /// has (kings are not counted).
    pub fn from_position(position: &Position) -> MaterialSearch {
        let mut search = MaterialSearch::zero();
        for (_, piece) in position.pieces() {
            let side = match piece.color {
                Color::White => &mut search.white,
                Color::Black => &mut search.black,
            };
            if let Some(range) = side.get_mut(piece.role) {
                range.min = range.min.saturating_add(1);
                range.max = range.max.saturating_add(1);
            }
        }
        for side in [&mut search.white, &mut search.black] {
            let minors = side.bishop.min.saturating_add(side.knight.min);
            side.minor = PieceRange::new(minors, minors);
        }
        search.normalized()
    }

    /// The search with consistent ranges (D3215): `max >= min` for every
    /// range, the minor-piece minimum at least the sum of the bishop and
    /// knight minimums, and its maximum at most the sum of their maximums.
    /// The same rules for both colours.
    #[must_use]
    pub fn normalized(&self) -> MaterialSearch {
        let mut out = self.clone();
        for side in [&mut out.white, &mut out.black] {
            let mut r = side.to_array();
            for range in &mut r {
                range.max = range.max.max(range.min);
            }
            let [_, _, bishop, knight, _, minor] = &mut r;
            minor.min = minor.min.max(bishop.min.saturating_add(knight.min));
            minor.max = minor.max.min(bishop.max.saturating_add(knight.max));
            *side = SideRanges::from_array(r);
        }
        if let Some((a, b)) = out.diff
            && a > b
        {
            out.diff = Some((b, a));
        }
        out
    }

    /// Checks the patterns (D3218): at most [`MAX_PATTERNS`], and every one
    /// with a piece has a file or a rank; and the range of move numbers
    /// (D3258): `moves.0 <= moves.1 <= 999`.
    ///
    /// # Errors
    ///
    /// [`SearchError::InvalidPattern`] and [`SearchError::InvalidMoveRange`].
    pub fn validate(&self) -> Result<(), SearchError> {
        let (min, max) = self.moves;
        if min > max || max > MAX_MOVE_NUMBER {
            return Err(SearchError::InvalidMoveRange { min, max });
        }
        if self.patterns.len() > MAX_PATTERNS {
            return Err(SearchError::InvalidPattern {
                reason: "too many patterns",
            });
        }
        for pattern in &self.patterns {
            if pattern.piece.is_some() && pattern.file.is_none() && pattern.rank.is_none() {
                return Err(SearchError::InvalidPattern {
                    reason: "a pattern needs a file or a rank",
                });
            }
        }
        Ok(())
    }
}

/// Runs a material search over `db`, combining the result with `filter`
/// according to `query.op` (`sc_search material`).
///
/// The query is [normalised](MaterialSearch::normalized) first. See the
/// module documentation for the rules. A cancelled search leaves
/// `filter` unchanged; unreadable games do not match and are listed in the
/// report.
///
/// # Errors
///
/// [`SearchError::InvalidPattern`], [`SearchError::InvalidMoveRange`]
/// (nothing is changed) and [`SearchError::FilterSize`].
pub fn search_material<D: GameSource + ?Sized>(
    db: &mut D,
    query: &MaterialSearch,
    filter: &mut Filter,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<SearchReport, SearchError> {
    query.validate()?;
    let matcher = MaterialMatcher::new(query);
    scan_games(db, &matcher, query.op, filter, progress, cancel)
}

/// Roles in the order of the count arrays.
const ROLES: [Role; 5] = [
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];
const QUEEN: usize = 0;
const ROOK: usize = 1;
const BISHOP: usize = 2;
const KNIGHT: usize = 3;
const PAWN: usize = 4;
const MINOR: usize = 5;

/// Piece counts of both colours: queen, rook, bishop, knight, pawn.
type Counts = [[u8; 5]; 2];

fn counts_of(pos: &ListPosition) -> Counts {
    let mut counts = [[0u8; 5]; 2];
    for (c, color) in [Color::White, Color::Black].into_iter().enumerate() {
        for &sq in pos.piece_squares(color) {
            let slot = pos
                .piece_at(sq)
                .and_then(|p| ROLES.iter().position(|&r| r == p.role))
                .and_then(|r| counts.get_mut(c).and_then(|row| row.get_mut(r)));
            if let Some(slot) = slot {
                *slot += 1;
            }
        }
    }
    counts
}

/// The material of one colour (kings are worth 0).
fn value_of(counts: [u8; 5]) -> i32 {
    const VALUES: [i32; 5] = [9, 5, 3, 3, 1];
    counts
        .iter()
        .zip(VALUES)
        .map(|(&n, v)| i32::from(n) * v)
        .sum()
}

/// One pass over a game: the normal search or the colour-swapped one.
struct Pass {
    /// Per colour, per [queen, rook, bishop, knight, pawn, minor].
    min: [[u8; 6]; 2],
    max: [[u8; 6]; 2],
    diff: Option<(i32, i32)>,
    patterns: Vec<Pattern>,
}

impl Pass {
    fn new(query: &MaterialSearch, swapped: bool) -> Pass {
        let (first, second) = if swapped {
            (query.black, query.white)
        } else {
            (query.white, query.black)
        };
        let ranges = [first.to_array(), second.to_array()];
        let pick = |f: fn(&PieceRange) -> u8| {
            ranges.map(|side| {
                let mut out = [0u8; 6];
                for (slot, range) in out.iter_mut().zip(side.iter()) {
                    *slot = f(range);
                }
                out
            })
        };
        let diff = query.diff.map(|(a, b)| {
            let (a, b) = (i32::from(a.min(b)), i32::from(a.max(b)));
            if swapped { (-b, -a) } else { (a, b) }
        });
        Pass {
            min: pick(|r| r.min),
            max: pick(|r| r.max),
            diff,
            patterns: query
                .patterns
                .iter()
                .filter(|p| p.piece.is_some())
                .map(|p| if swapped { p.flipped() } else { *p })
                .collect(),
        }
    }

    fn pawns_below_min(&self, counts: &Counts) -> bool {
        (0..2).any(|c| {
            counts.get(c).and_then(|r| r.get(PAWN)) < self.min.get(c).and_then(|r| r.get(PAWN))
        })
    }

    /// Whether the count of `kind` of colour `c` is inside the range, and
    /// whether the minimum is the reason it is not.
    fn count_verdict(&self, counts: &Counts, c: usize) -> Verdict {
        let row = counts.get(c).copied().unwrap_or_default();
        let lo = self.min.get(c).copied().unwrap_or_default();
        let hi = self.max.get(c).copied().unwrap_or([u8::MAX; 6]);
        let minor = row[BISHOP].saturating_add(row[KNIGHT]);
        let have = |k: usize| if k == MINOR { minor } else { row[k] };
        // Pieces below their minimum might still come from promotions.
        if [QUEEN, ROOK, BISHOP, KNIGHT, MINOR]
            .iter()
            .any(|&k| have(k) < lo[k])
        {
            return Verdict::BelowMin;
        }
        if (0..6).any(|k| have(k) > hi[k]) {
            return Verdict::Fail;
        }
        Verdict::Pass
    }

    fn test(&self, pos: &ListPosition, counts: &Counts, bishops: BishopColors) -> Verdict {
        let mut verdict = Verdict::Pass;
        for c in 0..2 {
            match self.count_verdict(counts, c) {
                Verdict::BelowMin => return Verdict::BelowMin,
                Verdict::Fail => verdict = Verdict::Fail,
                Verdict::Pass => {}
            }
        }
        if verdict == Verdict::Fail {
            return verdict;
        }
        if bishops != BishopColors::Either
            && counts[0][BISHOP] == 1
            && counts[1][BISHOP] == 1
            && let (Some(white), Some(black)) = (
                bishop_square_light(pos, Color::White),
                bishop_square_light(pos, Color::Black),
            )
            && ((bishops == BishopColors::Same) != (white == black))
        {
            return Verdict::Fail;
        }
        if let Some((lo, hi)) = self.diff {
            let diff = value_of(counts[0]) - value_of(counts[1]);
            if diff < lo || diff > hi {
                return Verdict::Fail;
            }
        }
        if patterns_hold(&self.patterns, pos) {
            Verdict::Pass
        } else {
            Verdict::Fail
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pass,
    /// A maximum, the bishops, the difference or a pattern failed.
    Fail,
    /// A minimum count is not reached.
    BelowMin,
}

fn bishop_square_light(pos: &ListPosition, color: Color) -> Option<bool> {
    pos.piece_squares(color)
        .iter()
        .find(|&&sq| pos.piece_at(sq).is_some_and(|p| p.role == Role::Bishop))
        .map(|sq| sq.is_light())
}

/// `patternsMatch`: every pattern must hold.
fn patterns_hold(patterns: &[Pattern], pos: &ListPosition) -> bool {
    patterns.iter().all(|pattern| {
        let Some(piece) = pattern.piece else {
            return true;
        };
        if pattern.file.is_none() && pattern.rank.is_none() {
            return true;
        }
        let found = pos.piece_squares(piece.color).iter().any(|&sq| {
            pos.piece_at(sq) == Some(piece)
                && pattern.file.is_none_or(|f| sq.file() == f)
                && pattern.rank.is_none_or(|r| sq.rank() == r)
        });
        found == pattern.present
    })
}

struct MaterialMatcher {
    passes: Vec<Pass>,
    bishops: BishopColors,
    min_ply: usize,
    max_ply: usize,
    need: usize,
    end_only: bool,
    shortcuts: bool,
    /// The signature of the maximums per pass, when it can be used.
    max_sig: Option<[u32; 2]>,
    /// The home-pawn bits excluded by "no pawn on file" patterns, per pass.
    hp_exclude: [u16; 2],
}

impl MaterialMatcher {
    fn new(query: &MaterialSearch) -> MaterialMatcher {
        let query = query.normalized();
        let mut passes = vec![Pass::new(&query, false)];
        if query.ignore_colors {
            passes.push(Pass::new(&query, true));
        }
        let w = query.white.to_array();
        let b = query.black.to_array();
        let usable = [QUEEN, ROOK, BISHOP, KNIGHT]
            .iter()
            .all(|&k| w.get(k).is_some_and(|r| r.max <= 3) && b.get(k).is_some_and(|r| r.max <= 3));
        let max_sig = usable.then(|| {
            let cap3 = |r: &PieceRange| u32::from(r.max.min(3));
            let side = |s: &[PieceRange; 6], pawn: u32, n: u32, bi: u32, r: u32, q: u32| {
                let p = u32::from(s[PAWN].max.min(15));
                (p << pawn)
                    | (cap3(&s[KNIGHT]) << n)
                    | (cap3(&s[BISHOP]) << bi)
                    | (cap3(&s[ROOK]) << r)
                    | (cap3(&s[QUEEN]) << q)
            };
            let sig = side(&w, 12, 16, 18, 20, 22) | side(&b, 0, 4, 6, 8, 10);
            [sig, sig::flip_signature(sig)]
        });
        let mut hp_exclude = [0u16; 2];
        for p in &query.patterns {
            if let Some(bit) = p.home_pawn_bit() {
                hp_exclude[0] |= bit;
            }
            if let Some(bit) = p.flipped().home_pawn_bit() {
                hp_exclude[1] |= bit;
            }
        }
        let (first, last) = query.moves;
        let (min_ply, max_ply) = if query.end_only {
            (0, usize::MAX)
        } else {
            (
                if first == 0 {
                    0
                } else {
                    usize::from(first) * 2 - 1
                },
                usize::from(last) * 2,
            )
        };
        MaterialMatcher {
            passes,
            bishops: query.bishops,
            min_ply,
            max_ply,
            need: if query.end_only {
                1
            } else {
                usize::from(query.min_plies).max(1)
            },
            end_only: query.end_only,
            shortcuts: query.shortcuts,
            max_sig,
            hp_exclude,
        }
    }

    /// Whether the index lets pass `i` match (`possibleMatch`).
    fn possible(&self, i: usize, entry: &IndexEntry) -> bool {
        if !self.shortcuts || !sig::index_is_trusted(entry) {
            return true;
        }
        if let Some(sigs) = self.max_sig {
            let (promos, upromo) = sig::promotion_flags(entry);
            let max = sigs.get(i).copied().unwrap_or(0);
            if !sig::is_reachable(max, entry.final_mat_sig, promos, upromo) {
                return false;
            }
        }
        let mask = self.hp_exclude.get(i).copied().unwrap_or(0);
        mask == 0 || sig::hp_final(entry) & mask == 0
    }

    /// `MaterialMatch` for one pass: the first ply of the first run of
    /// `need` consecutive plies that satisfy the search.
    fn run(
        &self,
        pass: &Pass,
        entry: &IndexEntry,
        record: &[u8],
    ) -> Result<Option<usize>, RecordError> {
        // The "no promotions: a missing piece never comes back" stop relies
        // on the flags of the index (D3221).
        let can_promote = !(self.shortcuts && sig::index_is_trusted(entry))
            || sig::promotion_flags(entry) != (false, false);
        let mut ply = 0usize;
        let mut run = 0usize;
        let mut start = 0usize;
        let walked = walk_mainline(record, |pos, next| {
            let here = ply;
            ply += 1;
            // A game with fewer pawns than the minimum can never match.
            let counts = counts_of(pos);
            if pass.pawns_below_min(&counts) || here > self.max_ply {
                return ControlFlow::Break(None);
            }
            if self.end_only {
                if next.is_some() {
                    return ControlFlow::Continue(());
                }
                if here == 0 {
                    return ControlFlow::Break(None);
                }
                return match pass.test(pos, &counts, self.bishops) {
                    Verdict::Pass => ControlFlow::Break(Some(here)),
                    _ => ControlFlow::Break(None),
                };
            }
            if here < self.min_ply {
                run = 0;
                return ControlFlow::Continue(());
            }
            match pass.test(pos, &counts, self.bishops) {
                Verdict::Pass => {
                    if run == 0 {
                        start = here;
                    }
                    run += 1;
                    if run >= self.need {
                        return ControlFlow::Break(Some(start));
                    }
                }
                Verdict::BelowMin if !can_promote => return ControlFlow::Break(None),
                _ => run = 0,
            }
            ControlFlow::Continue(())
        })?;
        Ok(walked.flatten())
    }
}

impl GameMatcher for MaterialMatcher {
    fn prefilter(&self, entry: &IndexEntry) -> Shortcut {
        if (0..self.passes.len()).any(|i| self.possible(i, entry)) {
            Shortcut::Examine
        } else {
            Shortcut::Skip
        }
    }

    fn evaluate(&self, entry: &IndexEntry, record: &[u8]) -> Result<Option<u8>, RecordError> {
        let mut failure = None;
        for (i, pass) in self.passes.iter().enumerate() {
            if !self.possible(i, entry) {
                continue;
            }
            match self.run(pass, entry, record) {
                Ok(Some(at)) => return Ok(Some(u8::try_from(at + 1).unwrap_or(u8::MAX))),
                Ok(None) => {}
                Err(e) => failure = Some(e),
            }
        }
        failure.map_or(Ok(None), Err)
    }
}

#[cfg(test)]
mod tests;
