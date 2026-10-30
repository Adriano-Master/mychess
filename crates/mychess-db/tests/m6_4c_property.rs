//! M6-4c: property tests of the piece tracker against an independent model.
//!
//! Random legal games are played with the rules engine of the core. The model
//! follows the pieces by the identity the engine gives each move (a normal
//! move, an en passant capture, a castling with king and rook squares), never
//! by the squares of the SI4 decoder, and the counts of `piece_track` must
//! match it. The sum of the time counts of one followed piece must equal the
//! number of plies it was followed (rule P6).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cloned_ref_to_slice_refs,
    clippy::needless_pass_by_value
)]

#[path = "common/m6_4bc_support.rs"]
mod support;

use std::collections::BTreeSet;

use mychess_core::{CastlingSide, Move, Position, Square};
use mychess_db::{
    CancelToken, NoProgress, PieceTrackQuery, TrackMode, piece_track, piece_track_plies,
};
use proptest::prelude::*;
use support::{all, g, open_text, pgn};

/// A random legal game: its SAN moves and its moves.
fn play(choices: &[u16]) -> (Vec<String>, Vec<Move>) {
    let mut pos = Position::new();
    let mut sans = Vec::new();
    let mut moves = Vec::new();
    for &choice in choices {
        let legal = pos.legal_moves();
        if legal.is_empty() {
            break;
        }
        let m = legal[usize::from(choice) % legal.len()];
        sans.push(pos.san(m).unwrap());
        pos.play(m).unwrap();
        moves.push(m);
    }
    (sans, moves)
}

/// What the model counts: `(raw time counts, games-mode counts, plies
/// followed)`.
fn model(
    moves: &[Move],
    start: &BTreeSet<Square>,
    first: u32,
    last: u32,
) -> ([u64; 64], [u64; 64], u64) {
    let mut tracked = start.clone();
    let mut raw = [0u64; 64];
    let mut games = [0u64; 64];
    let mut moved_to = BTreeSet::new();
    let mut followed = 0u64;
    for (i, m) in moves.iter().enumerate() {
        let ply = u32::try_from(i + 1).unwrap();
        if ply > last {
            break;
        }
        let (captured, steps): (Option<Square>, Vec<(Square, Square)>) = match *m {
            Move::Null => (None, vec![]),
            Move::Normal {
                from, to, capture, ..
            } => (capture.map(|_| to), vec![(from, to)]),
            Move::EnPassant { from, to } => (
                Some(Square::from_coords(to.file(), from.rank())),
                vec![(from, to)],
            ),
            Move::Castle {
                king_from,
                rook_from,
            } => {
                let rank = king_from.rank();
                let (king_to, rook_to) = match m.castling_side().unwrap() {
                    CastlingSide::KingSide => ("g", "f"),
                    CastlingSide::QueenSide => ("c", "d"),
                };
                let at = |file: &str| format!("{file}{}", rank.char()).parse::<Square>().unwrap();
                (
                    None,
                    vec![(king_from, at(king_to)), (rook_from, at(rook_to))],
                )
            }
        };
        if let Some(square) = captured {
            tracked.remove(&square);
        }
        for (from, to) in steps {
            if tracked.remove(&from) {
                tracked.insert(to);
                if ply >= first && moved_to.insert(to) {
                    games[to.to_usize()] += 1;
                }
            }
        }
        if tracked.is_empty() {
            break;
        }
        if ply >= first {
            followed += 1;
            for square in &tracked {
                raw[square.to_usize()] += 1;
            }
        }
    }
    (raw, games, followed)
}

fn run(
    sans: &[String],
    squares: &BTreeSet<Square>,
    start_move: u16,
    end_move: u16,
    mode: TrackMode,
) -> mychess_db::PieceTrackResult {
    let sans: Vec<&str> = sans.iter().map(String::as_str).collect();
    let text = pgn(&[g("A", "B", "*").moves(&support::movetext(&sans))]);
    let mut db = open_text(&text);
    let filter = all(&db);
    let query = PieceTrackQuery {
        squares: squares.iter().copied().collect(),
        start_move,
        end_move,
        mode,
    };
    piece_track(
        &mut db,
        &filter,
        &query,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

/// Squares occupied at the start: ranks 1, 2, 7 and 8.
fn start_square() -> impl Strategy<Value = Square> {
    (0u32..32).prop_map(|i| Square::new(if i < 16 { i } else { i + 32 }))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn the_counts_match_the_model_for_any_game_selection_and_window(
        choices in proptest::collection::vec(any::<u16>(), 1..70),
        squares in proptest::collection::btree_set(start_square(), 1..4),
        start in 0u16..14,
        extra in 0u16..30,
    ) {
        let (sans, moves) = play(&choices);
        let end = start + extra;
        let (first, last) = piece_track_plies(start, end);
        let (raw, games, _) = model(&moves, &squares, first, last);
        let skipped = u32::try_from(moves.len()).unwrap() < first;

        let time = run(&sans, &squares, start, end, TrackMode::Time);
        let by_games = run(&sans, &squares, start, end, TrackMode::Games);
        if skipped {
            // Fewer plies than the first counted one: ignored (rule P3).
            prop_assert_eq!(time.games_skipped, 1);
            prop_assert_eq!(by_games.counts, [0u32; 64]);
        } else {
            prop_assert_eq!(time.raw, raw);
            let halved: Vec<u32> = raw.iter().map(|&v| u32::try_from(v / 2).unwrap()).collect();
            prop_assert_eq!(time.counts.to_vec(), halved);
            let expected: Vec<u32> = games.iter().map(|&v| u32::try_from(v).unwrap()).collect();
            prop_assert_eq!(by_games.counts.to_vec(), expected);
        }
    }

    #[test]
    fn the_time_of_one_piece_sums_to_the_plies_it_was_followed(
        choices in proptest::collection::vec(any::<u16>(), 1..70),
        square in start_square(),
        start in 0u16..8,
        extra in 0u16..40,
    ) {
        let (sans, moves) = play(&choices);
        let end = start + extra;
        let (first, last) = piece_track_plies(start, end);
        prop_assume!(u32::try_from(moves.len()).unwrap() >= first);
        let squares = BTreeSet::from([square]);
        // Plies in the window up to the one before the capture of the piece
        // (the model finds the capture by identity).
        let (_, _, followed) = model(&moves, &squares, first, last);
        let result = run(&sans, &squares, start, end, TrackMode::Time);
        prop_assert_eq!(result.raw.iter().sum::<u64>(), followed);
        // And never more than the window or the game allow.
        let window = u64::from(last.min(u32::try_from(moves.len()).unwrap()) + 1 - first);
        prop_assert!(followed <= window);
    }
}
