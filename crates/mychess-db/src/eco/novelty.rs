// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_game_novelty`) and
// src/game.cpp (`Game::ExactMatch`). Copyright (c) 1999-2003 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! [`find_novelty`]: the first move of a game that no other game of a
//! database has played (D1190 to D1192).
//!
//! Scid scans the whole database once per position of the game, from the
//! deepest opening position on, and stops at the first position that no game
//! reaches. The result depends only on *which positions of the game occur in
//! some reference game*, so one pass is enough: every position of every
//! reference game is looked up in the set of positions of the game
//! (equivalence, D1192). The first position, from the starting one, that was
//! never seen is the novelty.

use std::collections::HashMap;
use std::ops::ControlFlow;

use mychess_core::{ClassifyOptions, Color, EcoBook, Game, Position};
use thiserror::Error;

use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{GameFlags, ListPosition, Si4Error, walk_mainline};
use crate::source::GameSource;
use crate::tree::same_board;

/// Games between two looks at the cancel token (Scid: 64k).
const CHECK_EVERY: u32 = 65_536;

/// Options of [`find_novelty`].
pub struct NoveltyOptions<'a> {
    /// Ignore reference games whose date is the same as or later than the
    /// date of the game (`-older`).
    pub older_only: bool,
    /// The number (1-based) of the game in the reference database, when the
    /// game comes from it: that game is ignored.
    pub current_game: Option<u32>,
    /// Stops the search.
    pub cancel: &'a CancelToken,
    /// Receives the number of reference games read.
    pub progress: &'a mut dyn ProgressSink,
}

/// The novelty found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Novelty {
    /// Half-moves played up to and including the novel move (`>= 1`).
    pub ply: u32,
    /// The move in standard algebraic notation.
    pub san: String,
    /// The move number as printed (`12` for `12...Nf6`).
    pub move_number: u32,
    /// The side that played the move.
    pub color: Color,
}

/// Why no novelty was returned.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NoveltyError {
    /// Every position of the game, up to its end, occurs in the reference
    /// games (or the game has no moves).
    #[error("no novelty was found for this game")]
    NoNovelty,
    /// The game starts from a position other than the standard one.
    #[error("the game has a non-standard start position")]
    NonStandardStart,
    /// The cancel token was set.
    #[error("the novelty search was interrupted")]
    Interrupted,
    /// A position of the game cannot be matched against the games of the
    /// database.
    #[error("unsupported position at ply {ply}")]
    UnsupportedPosition {
        /// The ply of the position.
        ply: u32,
    },
    /// A reference game cannot be read.
    #[error("cannot read reference game {game}")]
    Read {
        /// The number of the game.
        game: u32,
        /// The cause.
        #[source]
        source: Si4Error,
    },
}

/// What both algorithms need: the positions of the game from the deepest
/// opening position on.
struct Targets {
    /// Ply of the first target.
    first: usize,
    /// `positions[i]` is the position at ply `first + i`.
    positions: Vec<ListPosition>,
    /// The moves of the game, `moves[p]` leading to ply `p + 1`, with the
    /// position before them.
    line: Vec<(Position, mychess_core::Move)>,
    /// Date of the game, packed like Scid's `dateT`.
    date: u32,
}

fn prepare(game: &Game, book: Option<&EcoBook>) -> Result<Targets, NoveltyError> {
    if game.start_position() != &Position::new() {
        return Err(NoveltyError::NonStandardStart);
    }
    let mut line = Vec::new();
    let mut boards = vec![game.start_position().clone()];
    for node in game.mainline() {
        let (Ok(mv), Ok(after)) = (game.mv(node), game.position_at(node)) else {
            // An inconsistent tree: do not search on a silently shortened line.
            return Err(NoveltyError::UnsupportedPosition {
                ply: u32::try_from(line.len() + 1).unwrap_or(u32::MAX),
            });
        };
        let before = boards.last().cloned().unwrap_or_default();
        line.push((before, mv));
        boards.push(after.clone());
    }
    if line.is_empty() {
        return Err(NoveltyError::NoNovelty);
    }
    // The deepest opening position, or the start without a book.
    let first = book
        .and_then(|b| b.classify(game, ClassifyOptions::game()))
        .map_or(0, |m| m.ply as usize);
    let mut positions = Vec::new();
    for (i, board) in boards.iter().enumerate().skip(first) {
        let converted = ListPosition::from_fen(&board.fen()).map_err(|_| {
            NoveltyError::UnsupportedPosition {
                ply: u32::try_from(i).unwrap_or(u32::MAX),
            }
        })?;
        positions.push(converted);
    }
    Ok(Targets {
        first,
        positions,
        line,
        date: game.tags().date.to_scid_u32(),
    })
}

impl Targets {
    /// The novelty for the first target that no game reaches, if any.
    fn novelty(&self, matched: &[bool]) -> Result<Novelty, NoveltyError> {
        let index = matched
            .iter()
            .position(|&m| !m)
            .ok_or(NoveltyError::NoNovelty)?;
        // Scid reports the move that leads to the position; at the very
        // start it has none to report and shows the first move.
        let ply = (self.first + index).max(1);
        let (before, mv) = self.line.get(ply - 1).ok_or(NoveltyError::NoNovelty)?;
        Ok(Novelty {
            ply: u32::try_from(ply).unwrap_or(u32::MAX),
            san: before.san(*mv).map_err(|_| NoveltyError::NoNovelty)?,
            move_number: before.fullmove_number(),
            color: before.turn(),
        })
    }
}

/// Finds the novelty of `game` (`Game > Find Novelty`): the move that leads
/// to the first position of its main line, from the deepest position of
/// `book` on (or from the start without a book), that does not occur in the
/// main line of any other game of `reference`.
///
/// Positions match by squares and side to move; castling rights and en
/// passant do not count (as in [`crate::tree`]). Reference games that are
/// empty, start from a position of their own, or (with `older_only`) are not
/// older than the game are ignored. A reference game that breaks in the
/// middle still counts for the positions before the damage.
///
/// # Errors
///
/// [`NoveltyError::NoNovelty`] when every position occurs;
/// [`NoveltyError::NonStandardStart`] for a game with its own start;
/// [`NoveltyError::Interrupted`] when the token is set; [`NoveltyError::Read`]
/// when a reference game cannot be read.
pub fn find_novelty<D: GameSource + ?Sized>(
    reference: &mut D,
    game: &Game,
    book: Option<&EcoBook>,
    options: NoveltyOptions<'_>,
) -> Result<Novelty, NoveltyError> {
    let targets = prepare(game, book)?;
    let NoveltyOptions {
        older_only,
        current_game,
        cancel,
        progress,
    } = options;
    // Targets by the hash of their board, with the indices that share it.
    let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, position) in targets.positions.iter().enumerate() {
        by_hash.entry(board_hash(position)).or_default().push(i);
    }
    let mut matched = vec![false; targets.positions.len()];
    let mut remaining = matched.len();
    let total = reference.len();
    if cancel.is_cancelled() {
        return Err(NoveltyError::Interrupted);
    }
    for n in 1..=total {
        if n % CHECK_EVERY == 0 {
            progress.progress(u64::from(n), u64::from(total));
            if cancel.is_cancelled() {
                return Err(NoveltyError::Interrupted);
            }
        }
        if remaining == 0 {
            break;
        }
        let Some(record) = eligible(
            reference,
            n,
            current_game,
            older_only.then_some(targets.date),
        )?
        else {
            continue;
        };
        // A damaged record ends the walk: the positions seen so far count.
        walk_mainline(record, |position, _| {
            if let Some(indices) = by_hash.get(&board_hash(position)) {
                for &i in indices {
                    let hit = matched.get(i).is_some_and(|m| !m)
                        && targets
                            .positions
                            .get(i)
                            .is_some_and(|t| same_board(position, t));
                    if hit && let Some(m) = matched.get_mut(i) {
                        *m = true;
                        remaining -= 1;
                    }
                }
            }
            if remaining == 0 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .ok();
    }
    progress.progress(u64::from(total), u64::from(total));
    targets.novelty(&matched)
}

/// The record of game `n` if it takes part in the search.
fn eligible<D: GameSource + ?Sized>(
    reference: &mut D,
    n: u32,
    current: Option<u32>,
    older_than: Option<u32>,
) -> Result<Option<&[u8]>, NoveltyError> {
    if current == Some(n) {
        return Ok(None);
    }
    let read = |source| NoveltyError::Read { game: n, source };
    let entry = reference.entry(n).map_err(read)?;
    if entry.length == 0
        || entry.flags.contains(GameFlags::START)
        || older_than.is_some_and(|date| entry.date.to_scid_u32() >= date)
    {
        return Ok(None);
    }
    reference.record_bytes(n).map(Some).map_err(read)
}

/// `SplitMix64` step, to fill the hash table at compile time.
const fn splitmix(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn make_keys() -> [u64; 12 * 64 + 1] {
    let mut keys = [0u64; 12 * 64 + 1];
    let mut state = 0x4D79_4368_6573_7321u64;
    let mut i = 0;
    while i < keys.len() {
        state = splitmix(state);
        keys[i] = state;
        i += 1;
    }
    keys
}

/// Random keys for (colour, role, square) and, last, for Black to move.
static KEYS: [u64; 12 * 64 + 1] = make_keys();

/// A hash of the board and the side to move, independent of the order of the
/// piece lists. Equal boards give equal hashes; [`same_board`] settles the
/// rare collisions.
fn board_hash(position: &ListPosition) -> u64 {
    let mut hash = 0u64;
    for color in [Color::White, Color::Black] {
        let base = if color == Color::White { 0 } else { 6 };
        for &sq in position.piece_squares(color) {
            let Some(piece) = position.piece_at(sq) else {
                continue;
            };
            // `Role` is 1 (pawn) to 6 (king) and `Square` is 0 to 63.
            let role = (piece.role as usize).saturating_sub(1);
            let square = usize::try_from(sq.to_u32()).unwrap_or(0);
            let key = KEYS.get((base + role) * 64 + square).copied().unwrap_or(0);
            hash ^= key;
        }
    }
    if position.turn() == Color::Black {
        hash ^= KEYS.last().copied().unwrap_or(0);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{Rng, game_from_sans, make_db, random_game};
    use super::*;
    use crate::NoProgress;
    use crate::si4::Si4Database;

    /// The algorithm of Scid, literally: one scan of the reference games per
    /// position of the game, stopping at the first position nobody reaches.
    fn naive<D: GameSource + ?Sized>(
        reference: &mut D,
        game: &Game,
        book: Option<&EcoBook>,
        older_only: bool,
        current_game: Option<u32>,
    ) -> Result<Novelty, NoveltyError> {
        let targets = prepare(game, book)?;
        let mut matched = vec![false; targets.positions.len()];
        for (i, target) in targets.positions.iter().enumerate() {
            for n in 1..=reference.len() {
                let older = older_only.then_some(targets.date);
                let Some(record) = eligible(reference, n, current_game, older)? else {
                    continue;
                };
                let found = walk_mainline(record, |position, _| {
                    if same_board(position, target) {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                });
                if matches!(found, Ok(Some(()))) {
                    matched[i] = true;
                    break;
                }
            }
            if !matched[i] {
                break;
            }
        }
        targets.novelty(&matched)
    }

    fn options<'a>(
        cancel: &'a CancelToken,
        progress: &'a mut NoProgress,
        older_only: bool,
        current_game: Option<u32>,
    ) -> NoveltyOptions<'a> {
        NoveltyOptions {
            older_only,
            current_game,
            cancel,
            progress,
        }
    }

    fn novelty(
        db: &mut Si4Database,
        game: &Game,
        book: Option<&EcoBook>,
    ) -> Result<Novelty, NoveltyError> {
        let cancel = CancelToken::new();
        find_novelty(
            db,
            game,
            book,
            options(&cancel, &mut NoProgress, false, None),
        )
    }

    #[test]
    fn spec_example_is_3_nf6() {
        let dir = tempfile::tempdir().unwrap();
        let base = make_db(
            dir.path(),
            &[game_from_sans(&["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"])],
        );
        let mut db = Si4Database::open(&base).unwrap();
        let current = game_from_sans(&["e4", "e5", "Nf3", "Nc6", "Bb5", "Nf6"]);
        let found = novelty(&mut db, &current, None).unwrap();
        assert_eq!(
            (
                found.ply,
                found.san.as_str(),
                found.move_number,
                found.color
            ),
            (6, "Nf6", 3, Color::Black)
        );
        // The same game again has no novelty; an empty one neither.
        let same = game_from_sans(&["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
        assert!(matches!(
            novelty(&mut db, &same, None),
            Err(NoveltyError::NoNovelty)
        ));
        assert!(matches!(
            novelty(&mut db, &Game::new(), None),
            Err(NoveltyError::NoNovelty)
        ));
        // The current game is ignored when it is in the database.
        let cancel = CancelToken::new();
        let own = find_novelty(
            &mut db,
            &same,
            None,
            options(&cancel, &mut NoProgress, false, Some(1)),
        );
        assert_eq!(own.unwrap().ply, 1);
    }

    #[test]
    fn rejects_other_start_positions_and_honours_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let base = make_db(dir.path(), &[game_from_sans(&["e4"])]);
        let mut db = Si4Database::open(&base).unwrap();
        let fen = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut custom = Game::from_position(fen);
        let root = custom.root();
        let mv = custom.position_at(root).unwrap().legal_moves()[0];
        custom.add_move(root, mv).unwrap();
        assert!(matches!(
            novelty(&mut db, &custom, None),
            Err(NoveltyError::NonStandardStart)
        ));
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = find_novelty(
            &mut db,
            &game_from_sans(&["e4"]),
            None,
            options(&cancel, &mut NoProgress, false, None),
        );
        assert!(matches!(result, Err(NoveltyError::Interrupted)));
    }

    #[test]
    fn one_pass_equals_the_scan_per_position() {
        let book = EcoBook::parse(
            "A00a \"Start\" *\nC20 \"KP\" 1.e4 e5 *\nB00 \"KP\" 1.e4 *\nA40 \"QP\" 1.d4 *\n",
        )
        .unwrap();
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        for round in 0..12 {
            let games: Vec<Game> = (0..30)
                .map(|_| random_game(&mut rng, 14, 2 + round % 3))
                .collect();
            let dir = tempfile::tempdir().unwrap();
            let base = make_db(dir.path(), &games);
            let mut db = Si4Database::open(&base).unwrap();
            for k in 0..30 {
                let current = random_game(&mut rng, 14, 2 + round % 3);
                let own = (k % 3 == 0).then_some(1 + u32::try_from(k).unwrap());
                let book = (k % 2 == 0).then_some(&book);
                let cancel = CancelToken::new();
                let fast = find_novelty(
                    &mut db,
                    &current,
                    book,
                    options(&cancel, &mut NoProgress, false, own),
                );
                let slow = naive(&mut db, &current, book, false, own);
                match (fast, slow) {
                    (Ok(a), Ok(b)) => assert_eq!(a, b, "round {round} game {k}"),
                    (Err(a), Err(b)) => {
                        assert_eq!(a.to_string(), b.to_string(), "round {round} game {k}");
                    }
                    (a, b) => panic!("round {round} game {k}: {a:?} vs {b:?}"),
                }
            }
        }
    }
}
