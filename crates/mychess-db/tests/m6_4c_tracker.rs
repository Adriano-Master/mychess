//! M6-4c: the piece tracker (spec 2.5, rules P1 to P13, decisions D3385 to
//! D3390, criteria 15 to 17). The expected counts are worked out by hand,
//! ply by ply, from `sc_base_piecetrack` and the decisions of the spec.
//!
//! The line used (`support::LINE`), with its plies:
//! 1 e4, 2 e5, 3 Nf3, 4 Nc6, 5 Bb5, 6 a6, 7 Ba4, 8 Nf6, 9 O-O, 10 Be7,
//! 11 Re1, 12 b5, 13 Bb3, 14 d6, 15 c3, 16 O-O, 17 h3, 18 Nb8, 19 d4,
//! 20 Nbd7.
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

use mychess_core::Square;
use mychess_db::{
    CancelToken, ColorScheme, Filter, GameSource, NoProgress, PieceTrackQuery, PieceTrackResult,
    StatsError, TrackMode, piece_track,
};
use pretty_assertions::assert_eq;
use support::{G, all, g, only, open};

fn sq(name: &str) -> Square {
    name.parse().unwrap()
}

fn query(squares: &[&str], start: u16, end: u16, mode: TrackMode) -> PieceTrackQuery {
    PieceTrackQuery {
        squares: squares.iter().map(|s| sq(s)).collect(),
        start_move: start,
        end_move: end,
        mode,
    }
}

fn track(games: &[G], q: &PieceTrackQuery) -> PieceTrackResult {
    let mut db = open(games);
    let filter = all(&db);
    piece_track(&mut db, &filter, q, &mut NoProgress, &CancelToken::new()).unwrap()
}

/// The non-zero counts as `(square, count)` in a1 to h8 order.
fn nonzero(r: &PieceTrackResult) -> Vec<(String, u32)> {
    r.counts
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c > 0)
        .map(|(i, &c)| (Square::new(i as u32).to_string(), c))
        .collect()
}

fn pairs(list: &[(&str, u32)]) -> Vec<(String, u32)> {
    let mut v: Vec<_> = list.iter().map(|&(s, c)| (s.to_owned(), c)).collect();
    // a1 to h8 order = rank then file: sort by the square index.
    v.sort_by_key(|(s, _)| u32::from(sq(s)));
    v
}

#[test]
fn games_mode_counts_each_square_a_piece_moved_to() {
    // Bishop f1: Bb5 (5), Ba4 (7), Bb3 (13).
    let games = vec![g("A", "B", "1-0").plies(20); 3];
    let r = track(&games, &query(&["f1"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("b5", 3), ("a4", 3), ("b3", 3)]));
    assert_eq!((r.games, r.games_used, r.games_skipped), (3, 3, 0));
    assert!(!r.interrupted);
}

#[test]
fn time_mode_counts_the_plies_a_piece_stays_and_halves_the_total() {
    // Bishop f1 over plies 1 to 20: f1 for 4 plies, b5 for 2 (5, 6), a4 for 6
    // (7 to 12), b3 for 8 (13 to 20): raw 4, 2, 6, 8 for one game.
    let one = track(
        &[g("A", "B", "1-0").plies(20)],
        &query(&["f1"], 1, 10, TrackMode::Time),
    );
    assert_eq!(
        nonzero(&one),
        pairs(&[("f1", 2), ("b5", 1), ("a4", 3), ("b3", 4)])
    );
    assert_eq!(one.raw.iter().sum::<u64>(), 20);
    // The halving is applied to the total of all games, so two games with a
    // raw count of 1 per square give 1, where one game gives 0.
    let r1 = track(
        &[g("A", "B", "1-0").plies(2)],
        &query(&["f1"], 1, 1, TrackMode::Time),
    );
    assert_eq!(r1.raw[sq("f1").to_usize()], 2);
    assert_eq!(r1.counts[sq("f1").to_usize()], 1);
    let r2 = track(
        &[g("A", "B", "1-0").plies(1)],
        &query(&["f1"], 1, 1, TrackMode::Time),
    );
    assert_eq!(r2.raw[sq("f1").to_usize()], 1);
    assert_eq!(r2.counts[sq("f1").to_usize()], 0);
}

#[test]
fn the_window_is_converted_to_plies_and_the_piece_is_followed_from_move_one() {
    let games = vec![g("A", "B", "1-0").plies(20)];
    // Moves 2 to 3 are the plies 3 to 6: Nf3 (ply 3) is counted.
    let r = track(&games, &query(&["g1"], 2, 3, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("f3", 1)]));
    // Moves 3 to 3 are the plies 5 and 6: Nf3 happened before, nothing moves
    // to a new square, but in time mode the knight is on f3 for both plies.
    let r = track(&games, &query(&["g1"], 3, 3, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[]));
    let r = track(&games, &query(&["g1"], 3, 3, TrackMode::Time));
    assert_eq!(r.raw[sq("f3").to_usize()], 2);
    assert_eq!(r.counts[sq("f3").to_usize()], 1);
    // Move 0 is read as move 1; an end before the start is the start.
    let zero = track(&games, &query(&["e2"], 0, 0, TrackMode::Games));
    assert_eq!(nonzero(&zero), pairs(&[("e4", 1)]));
    assert_eq!(zero.end_move, 0);
    let swapped = track(&games, &query(&["g1"], 5, 2, TrackMode::Games));
    assert_eq!(swapped.end_move, 5);
}

#[test]
fn games_shorter_than_the_first_ply_are_skipped_without_error() {
    let games = [
        g("A", "B", "1-0").plies(4), // fewer plies than the first counted (9)
        g("A", "B", "1-0").plies(20),
        g("A", "B", "1-0").moves(""), // no moves at all
    ];
    let r = track(&games, &query(&["f1"], 5, 20, TrackMode::Games));
    assert_eq!((r.games, r.games_used, r.games_skipped), (3, 1, 2));
    // First counted ply is 9: only Bb3 (13) is counted.
    assert_eq!(nonzero(&r), pairs(&[("b3", 1)]));
}

#[test]
fn games_with_their_own_start_position_are_ignored() {
    // A real start position (a standard one is not stored as a FEN).
    let fen = "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - 1 1";
    let games = [
        g("A", "B", "1-0").plies(20),
        g("A", "B", "1-0")
            .moves("1... e5 2. e4 Nc6 3. Bb5 a6 ")
            .fen(fen),
    ];
    let r = track(&games, &query(&["f1"], 1, 20, TrackMode::Games));
    assert_eq!((r.games, r.games_used, r.games_skipped), (2, 1, 1));
    assert_eq!(r.counts[sq("b5").to_usize()], 1);
}

#[test]
fn empty_inputs_give_64_zeros() {
    let games = vec![g("A", "B", "1-0").plies(20)];
    let mut db = open(&games);
    let run = |db: &mut _, filter: &Filter, q: &PieceTrackQuery| {
        piece_track(db, filter, q, &mut NoProgress, &CancelToken::new()).unwrap()
    };
    let q = query(&["f1"], 1, 20, TrackMode::Games);
    let none = only(&db, &[]);
    let r = run(&mut db, &none, &q);
    assert_eq!(r.counts, [0; 64]);
    assert_eq!((r.games, r.games_used, r.games_skipped), (0, 0, 0));
    let filter = all(&db);
    let r = run(&mut db, &filter, &query(&[], 1, 20, TrackMode::Games));
    assert_eq!(r.counts, [0; 64]);
    assert_eq!(r.games, 1);
    let mut empty = open(&[]);
    let r = run(&mut empty, &Filter::new(0), &q);
    assert_eq!(r.counts, [0; 64]);
}

#[test]
fn castling_moves_the_king_and_the_rook() {
    let games = vec![g("A", "B", "1-0").plies(20)];
    // The rook h1 goes to f1 with O-O (9) and to e1 with Re1 (11).
    let r = track(&games, &query(&["h1"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("f1", 1), ("e1", 1)]));
    // The king e1 goes to g1.
    let r = track(&games, &query(&["e1"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("g1", 1)]));
    // Both: king and rook (the same result for any pair the window allows).
    let r = track(&games, &query(&["e1", "h1"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("g1", 1), ("f1", 1), ("e1", 1)]));
    // Black castles at ply 16: the rook h8 goes to f8.
    let r = track(&games, &query(&["h8"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("f8", 1)]));
    // The rook a1 does not move in this game.
    let r = track(&games, &query(&["a1"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[]));
}

#[test]
fn a_capture_ends_the_following_and_the_capture_ply_is_not_counted() {
    // 1 e4 d5 2 exd5 Qxd5 3 Nc3 Qa5
    let game = g("A", "B", "1-0").moves("1. e4 d5 2. exd5 Qxd5 3. Nc3 Qa5 ");
    // The pawn d7 moves to d5 (ply 2) and is captured at ply 3.
    let r = track(&[game.clone()], &query(&["d7"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("d5", 1)]));
    // Time: d7 after ply 1, d5 after ply 2, then it is gone: raw 1 and 1.
    let r = track(&[game.clone()], &query(&["d7"], 1, 20, TrackMode::Time));
    assert_eq!(nonzero(&r), pairs(&[]));
    assert_eq!(r.raw[sq("d7").to_usize()], 1);
    assert_eq!(r.raw[sq("d5").to_usize()], 1);
    // The queen captures on d5 (ply 4) and goes on to a5 (ply 6).
    let r = track(&[game.clone()], &query(&["d8"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("d5", 1), ("a5", 1)]));
    // Pawn and queen together: d5 is counted once per game, although two
    // followed pieces arrive there (rule P5).
    let r = track(
        &[game.clone()],
        &query(&["d7", "d8"], 1, 20, TrackMode::Games),
    );
    assert_eq!(nonzero(&r), pairs(&[("d5", 1), ("a5", 1)]));
    // The white pawn e2 captures on d5 (ply 3) and is captured by the queen
    // at ply 4.
    let r = track(&[game], &query(&["e2"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("e4", 1), ("d5", 1)]));
}

#[test]
fn en_passant_removes_the_captured_pawn() {
    // 1 e4 a6 2 e5 d5 3 exd6 Nf6 4 Nf3 Nc6 (en passant at ply 5)
    let game = g("A", "B", "1-0").moves("1. e4 a6 2. e5 d5 3. exd6 Nf6 4. Nf3 Nc6 ");
    // The black pawn d7: d7 for plies 1 to 3, d5 after ply 4, then captured
    // en passant at ply 5. Scid leaves a ghost on d5 for the later plies.
    let r = track(&[game.clone()], &query(&["d7"], 1, 20, TrackMode::Time));
    assert_eq!(r.raw[sq("d7").to_usize()], 3);
    assert_eq!(r.raw[sq("d5").to_usize()], 1);
    assert_eq!(r.raw.iter().sum::<u64>(), 4);
    // The white pawn e2 ends on d6, counted as a move there.
    let r = track(&[game], &query(&["e2"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("e4", 1), ("e5", 1), ("d6", 1)]));
}

#[test]
fn a_promoted_pawn_stays_followed_until_it_is_captured() {
    let game =
        g("A", "B", "1-0").moves("1. h4 g5 2. hxg5 h6 3. gxh6 Nf6 4. h7 Rg8 5. hxg8=Q Nxg8 ");
    let r = track(&[game.clone()], &query(&["h2"], 1, 20, TrackMode::Games));
    assert_eq!(
        nonzero(&r),
        pairs(&[("h4", 1), ("g5", 1), ("h6", 1), ("h7", 1), ("g8", 1)])
    );
    // Time: two plies on each of h4, g5, h6 and h7, one on g8 (the capture
    // of the new queen at ply 10 is not counted).
    let r = track(&[game], &query(&["h2"], 1, 20, TrackMode::Time));
    for s in ["h4", "g5", "h6", "h7"] {
        assert_eq!(r.raw[sq(s).to_usize()], 2, "{s}");
    }
    assert_eq!(r.raw[sq("g8").to_usize()], 1);
    assert_eq!(r.raw.iter().sum::<u64>(), 9);
}

#[test]
fn percentages_the_ranking_and_the_grid() {
    // Three games: two with Nf3, one with Nh3 (1. e4 e5 2. Nh3).
    let games = [
        g("A", "B", "1-0").plies(4),
        g("A", "B", "1-0").plies(4),
        g("A", "B", "1-0").moves("1. e4 e5 2. Nh3 Nc6 "),
    ];
    let r = track(&games, &query(&["g1"], 1, 20, TrackMode::Games));
    assert_eq!(nonzero(&r), pairs(&[("f3", 2), ("h3", 1)]));
    assert_eq!(r.divisor(), 3);
    // round(2 * 10000 / 3) / 100 = 66.67 and 33.33.
    assert_eq!(r.percent(sq("f3")), 66.67);
    assert_eq!(r.percent(sq("h3")), 33.33);
    assert_eq!(r.percent(sq("a1")), 0.0);
    let ranking = r.ranking();
    assert_eq!(ranking.len(), 64);
    assert_eq!((ranking[0].square, ranking[0].count), (sq("f3"), 2));
    assert_eq!((ranking[1].square, ranking[1].count), (sq("h3"), 1));
    // Ties keep a1 to h8 order (rule P11).
    assert_eq!(ranking[2].square, sq("a1"));
    assert_eq!(ranking[3].square, sq("b1"));
    assert_eq!(ranking[63].square, sq("h8"));
    let grid = r.grid();
    assert_eq!(grid.max, 2);
    assert_eq!(grid.games, 3);
    assert_eq!(grid.squares[sq("f3").to_usize()], 1.0);
    assert_eq!(grid.squares[sq("h3").to_usize()], 0.5);
    assert_eq!(grid.squares[sq("a1").to_usize()], 0.0);
    // x = 100 in blue: (9, 9, 159); an empty square is white.
    assert_eq!(
        grid.rgb(ColorScheme::Blue, sq("f3").to_usize()),
        [9, 9, 159]
    );
    assert_eq!(
        grid.rgb(ColorScheme::Blue, sq("a1").to_usize()),
        [255, 255, 255]
    );
}

#[test]
fn the_divisor_in_time_mode_is_games_times_the_window() {
    let games = vec![g("A", "B", "1-0").plies(20); 4];
    let r = track(&games, &query(&["f1"], 2, 6, TrackMode::Time));
    // 4 games x (6 + 1 - 2) moves.
    assert_eq!(r.divisor(), 20);
    // An empty result still divides by at least 1.
    let empty = track(&[], &query(&["f1"], 1, 20, TrackMode::Time));
    assert_eq!(empty.divisor(), 1);
    assert_eq!(empty.percent(sq("f1")), 0.0);
}

#[test]
fn the_grid_of_an_empty_result_is_all_zero_with_max_one() {
    let r = track(&[], &query(&["f1"], 1, 20, TrackMode::Games));
    let grid = r.grid();
    assert_eq!(grid.max, 1);
    assert!(grid.squares.iter().all(|&v| v == 0.0));
}

#[test]
fn a_filter_of_another_size_is_an_error() {
    let mut db = open(&[g("A", "B", "1-0")]);
    let bad = Filter::new(4);
    let q = query(&["f1"], 1, 20, TrackMode::Games);
    assert_eq!(
        piece_track(&mut db, &bad, &q, &mut NoProgress, &CancelToken::new()).unwrap_err(),
        StatsError::FilterSize {
            filter: 4,
            games: 1
        }
    );
}

#[test]
fn only_filter_games_are_followed_and_the_divisor_is_the_whole_filter() {
    let games = [
        g("A", "B", "1-0").plies(4),
        g("A", "B", "1-0").plies(4),
        g("A", "B", "1-0").plies(1), // in the filter, too short: counts in the divisor (P13)
    ];
    let mut db = open(&games);
    let filter = only(&db, &[2, 3]);
    let q = query(&["g1"], 2, 20, TrackMode::Games);
    let r = piece_track(&mut db, &filter, &q, &mut NoProgress, &CancelToken::new()).unwrap();
    assert_eq!((r.games, r.games_used, r.games_skipped), (2, 1, 1));
    assert_eq!(r.counts[sq("f3").to_usize()], 1);
    assert_eq!(r.divisor(), 2);
    assert_eq!(r.percent(sq("f3")), 50.0);
}

#[test]
fn a_token_cancelled_before_the_start_gives_an_empty_interrupted_result() {
    let mut db = open(&vec![g("A", "B", "1-0").plies(20); 20]);
    let filter = all(&db);
    let cancel = CancelToken::new();
    cancel.cancel();
    let q = query(&["f1"], 1, 20, TrackMode::Games);
    let r = piece_track(&mut db, &filter, &q, &mut NoProgress, &cancel).unwrap();
    assert!(r.interrupted);
    assert_eq!(r.counts, [0; 64]);
    assert_eq!((r.games, r.games_used, r.games_skipped), (20, 0, 0));
    assert_eq!(db.len(), 20);
}

#[test]
fn cancelling_in_the_middle_keeps_the_partial_result() {
    // 12 000 games: the scan reports at game 5000 (or after 100 ms), where
    // the sink cancels. The games read so far are all counted.
    let games = vec![g("A", "B", "1-0").plies(6); 12_000];
    let mut db = open(&games);
    let filter = all(&db);
    let cancel = CancelToken::new();
    let handle = cancel.clone();
    let mut sink = move |_: u64, _: u64| handle.cancel();
    let q = query(&["f1"], 1, 20, TrackMode::Games);
    let r = piece_track(&mut db, &filter, &q, &mut sink, &cancel).unwrap();
    assert!(r.interrupted);
    assert_eq!(r.games, 12_000);
    assert!(
        r.games_used > 0 && r.games_used < 12_000,
        "{}",
        r.games_used
    );
    // Every game followed counted its Bb5 once.
    assert_eq!(r.counts[sq("b5").to_usize()], r.games_used);
    // The divisor stays the whole filter (rule P13).
    assert_eq!(r.divisor(), 12_000);
}

#[test]
fn the_result_is_the_same_with_one_thread_and_with_many() {
    let lines = [
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 ",
        "1. e4 d5 2. exd5 Qxd5 3. Nc3 Qa5 4. d4 c6 5. Nf3 Bg4 ",
        "1. e4 a6 2. e5 d5 3. exd6 Nf6 4. Nf3 Nc6 5. Bb5 Bxd6 ",
        "1. d4 d5 2. c4 e6 3. Nc3 Nf6 4. Bg5 Be7 5. e3 O-O 6. Nf3 Nbd7 ",
    ];
    let games: Vec<G> = (0..1500)
        .map(|i| g("A", "B", "1-0").moves(lines[i % lines.len()]))
        .collect();
    let mut db = open(&games);
    let mut filter = Filter::new(db.len());
    for n in (1..=db.len()).filter(|n| n % 7 != 0) {
        filter.set(n, 1);
    }
    for mode in [TrackMode::Games, TrackMode::Time] {
        let q = query(&["d1", "e1", "h1"], 1, 20, mode);
        let mut run = |threads| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| {
                    piece_track(&mut db, &filter, &q, &mut NoProgress, &CancelToken::new()).unwrap()
                })
        };
        let one = run(1);
        assert_eq!(one, run(4), "{mode:?}");
        assert_eq!(one.games_used, filter.count());
    }
}
