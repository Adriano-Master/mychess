//! Contract tests of the opening tree engine, part 2: the four sort orders
//! (rule 2, D944, D956), the ECO code of each row (D950, rule 11) and the
//! training helpers `random_move` and `move_by_index` (rule 20, D945).
#![allow(clippy::unwrap_used, clippy::too_many_lines, clippy::assert_is_empty)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m5_1_support.rs"]
mod support;

use mychess_core::{Color, EcoBook, GameResult, Position};
use mychess_db::tree::{Sort, TreeOptions, TreeState, move_by_index, random_move};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{db_of, gb, pos_after, repeat, row, run, sans};

const W: GameResult = GameResult::White;
const BL: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;

fn sorted(
    db: &mut impl mychess_db::GameSource,
    pos: &Position,
    sort: Sort,
    eco: Option<&EcoBook>,
) -> mychess_db::tree::TreeResult {
    let opts = TreeOptions {
        sort,
        eco_book: eco,
        ..TreeOptions::default()
    };
    run(db, pos, &opts, &mut TreeState::default()).unwrap()
}

fn eco_book() -> EcoBook {
    EcoBook::parse(
        "A04 \"Reti\" 1.Nf3 *\nA10 \"English\" 1.c4 *\nC20 \"King pawn\" 1.e4 *\nD00 \"Queen pawn\" 1.d4 *\n",
    )
    .unwrap()
}

// ------------------------------------------------------------ Frequency

#[test]
fn frequency_sorts_by_games_descending_and_is_the_default() {
    let mut games = repeat(1, |_| gb("c2c4"));
    games.extend(repeat(3, |_| gb("e2e4")));
    games.extend(repeat(2, |_| gb("d2d4")));
    games.extend(repeat(4, |_| gb("g1f3")));
    let (_fx, mut db) = db_of(games);
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
    )
    .unwrap();
    assert_eq!(sans(&r), vec!["Nf3", "e4", "d4", "c4"]);
    assert_eq!(TreeOptions::default().sort, Sort::Frequency);
}

// ------------------------------------------------------------ Alpha

#[test]
fn alpha_sorts_by_san_bytes_ascending_and_distinguishes_case() {
    // D956: byte order, so "Nf3" (uppercase N) sorts before "c4".
    let (_fx, mut db) = db_of(vec![
        gb("e2e4"),
        gb("g1f3"),
        gb("d2d4"),
        gb("c2c4"),
        gb("b1c3"),
    ]);
    let r = sorted(&mut db, &Position::new(), Sort::Alpha, None);
    assert_eq!(sans(&r), vec!["Nc3", "Nf3", "c4", "d4", "e4"]);
}

#[test]
fn alpha_puts_the_end_node_before_lowercase_moves() {
    // "[end]": '[' (0x5B) is above uppercase and below lowercase letters.
    let (_fx, mut db) = db_of(vec![
        gb("e2e4 e7e5"),
        gb("e2e4 e7e5 g1f3"),
        gb("e2e4 e7e5 b1c3"),
    ]);
    let r = sorted(&mut db, &pos_after("e2e4 e7e5"), Sort::Alpha, None);
    assert_eq!(sans(&r), vec!["Nc3", "Nf3", "[end]"]);
}

// ------------------------------------------------------------ Eco

#[test]
fn eco_sorts_descending_with_moves_without_eco_last() {
    let book = eco_book();
    let (_fx, mut db) = db_of(vec![
        gb("c2c4"),
        gb("g1f3"),
        gb("g2g3"), // not in the book
        gb("e2e4"),
        gb("d2d4"),
        gb("b2b3"), // not in the book
    ]);
    let r = sorted(&mut db, &Position::new(), Sort::Eco, Some(&book));
    let order = sans(&r);
    assert_eq!(&order[..4], &["d4", "e4", "c4", "Nf3"]);
    let mut tail: Vec<_> = order[4..].to_vec();
    tail.sort_unstable();
    assert_eq!(tail, vec!["b3", "g3"]);
    assert!(r.rows[4].eco.is_none() && r.rows[5].eco.is_none());
    assert_eq!(row(&r, "e4").eco.unwrap().to_string(), "C20");
    assert_eq!(row(&r, "d4").eco.unwrap().to_string(), "D00");
}

#[test]
fn eco_is_absent_without_a_book_and_hidden_rows_keep_it() {
    let book = eco_book();
    let (_fx, mut db) = db_of(vec![gb("e2e4")]);
    let r = sorted(&mut db, &Position::new(), Sort::Frequency, None);
    assert!(r.rows[0].eco.is_none());
    let opts = TreeOptions {
        hide_moves: true,
        eco_book: Some(&book),
        ..TreeOptions::default()
    };
    let r = run(&mut db, &Position::new(), &opts, &mut TreeState::default()).unwrap();
    assert!(r.rows[0].hidden);
    assert_eq!(r.rows[0].eco.unwrap().to_string(), "C20");
}

#[test]
fn end_node_takes_the_eco_of_the_current_position() {
    let book = EcoBook::parse("C20 \"King pawn\" 1.e4 e5 *\n").unwrap();
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5")]);
    let r = sorted(
        &mut db,
        &pos_after("e2e4 e7e5"),
        Sort::Frequency,
        Some(&book),
    );
    assert_eq!(row(&r, "[end]").eco.unwrap().to_string(), "C20");
}

// ------------------------------------------------------------ Score

#[test]
fn score_sorts_descending_when_white_moves() {
    let (_fx, mut db) = db_of(vec![
        gb("d2d4").result(D),
        gb("e2e4").result(W),
        gb("c2c4").result(BL),
        gb("g1f3").result(W),
        gb("g1f3").result(D),
    ]);
    let r = sorted(&mut db, &Position::new(), Sort::Score, None);
    // e4 1000, Nf3 750, d4 500, c4 0.
    assert_eq!(sans(&r), vec!["e4", "Nf3", "d4", "c4"]);
}

#[test]
fn score_is_inverted_when_black_moves() {
    // After 1.e4 Black moves: lowest White score (best for Black) first.
    let (_fx, mut db) = db_of(vec![
        gb("e2e4 c7c5").result(D),
        gb("e2e4 e7e5").result(W),
        gb("e2e4 e7e6").result(BL),
        gb("e2e4 g8f6").result(W),
        gb("e2e4 g8f6").result(D),
    ]);
    let r = sorted(&mut db, &pos_after("e2e4"), Sort::Score, None);
    assert_eq!(r.turn, Color::Black);
    // e6 0, c5 500, Nf6 750, e5 1000.
    assert_eq!(sans(&r), vec!["e6", "c5", "Nf6", "e5"]);
}

#[test]
fn every_sort_keeps_the_same_rows_and_totals() {
    let (_fx, mut db) = db_of(vec![
        gb("e2e4").result(W),
        gb("e2e4").result(D),
        gb("d2d4").result(BL),
        gb("g1f3").result(W),
    ]);
    let book = eco_book();
    let base = sorted(&mut db, &Position::new(), Sort::Frequency, Some(&book));
    for sort in [Sort::Alpha, Sort::Eco, Sort::Score] {
        let r = sorted(&mut db, &Position::new(), sort, Some(&book));
        let mut a = sans(&r);
        let mut b = sans(&base);
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "{sort:?}");
        assert_eq!(r.total, base.total, "{sort:?}");
        for x in &r.rows {
            assert_eq!(x, row(&base, &x.san), "{sort:?}");
        }
    }
}

// ------------------------------------------------------------ random_move

fn lcg_free_result() -> mychess_db::tree::TreeResult {
    // e4 x3, d4 x2 (Frequency order: e4, d4).
    let mut games = repeat(3, |_| gb("e2e4"));
    games.extend(repeat(2, |_| gb("d2d4")));
    let (_fx, mut db) = db_of(games);
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut TreeState::default(),
    )
    .unwrap()
}

#[test]
fn random_move_is_proportional_to_the_frequency_without_edge_bias() {
    let r = lcg_free_result();
    let e4 = row(&r, "e4").mv.unwrap();
    let d4 = row(&r, "d4").mv.unwrap();
    // Every value of 0..total once: 3 for e4 and 2 for d4. The Scid's
    // `<=` would give e4 four of five (D945).
    let picks: Vec<_> = (0u32..5)
        .map(|v| random_move(&r, &mut || v).unwrap())
        .collect();
    assert_eq!(picks, vec![e4, e4, e4, d4, d4]);
}

#[test]
fn random_move_boundaries_of_the_range() {
    let r = lcg_free_result();
    let e4 = row(&r, "e4").mv.unwrap();
    let d4 = row(&r, "d4").mv.unwrap();
    assert_eq!(random_move(&r, &mut || 0), Some(e4));
    assert_eq!(
        random_move(&r, &mut || 2),
        Some(e4),
        "last of the first row"
    );
    assert_eq!(
        random_move(&r, &mut || 3),
        Some(d4),
        "first of the second row"
    );
    assert_eq!(
        random_move(&r, &mut || 4),
        Some(d4),
        "last value of the range"
    );
}

#[test]
fn random_move_consumes_the_injected_generator() {
    let r = lcg_free_result();
    let mut calls = 0;
    let mut rng = || {
        calls += 1;
        0u32
    };
    random_move(&r, &mut rng).unwrap();
    assert!(calls >= 1);
}

#[test]
fn random_move_skips_the_end_node() {
    // [end] x3 is the first row under Frequency; Nf3 x2.
    let mut games = repeat(3, |_| gb("e2e4 e7e5"));
    games.extend(repeat(2, |_| gb("e2e4 e7e5 g1f3")));
    let (_fx, mut db) = db_of(games);
    let r = sorted(&mut db, &pos_after("e2e4 e7e5"), Sort::Frequency, None);
    assert_eq!(sans(&r), vec!["[end]", "Nf3"]);
    let nf3 = row(&r, "Nf3").mv.unwrap();
    // The draw is over the 2 games with a move: every value gives Nf3.
    for v in 0u32..2 {
        assert_eq!(random_move(&r, &mut || v), Some(nf3), "value {v}");
    }
}

#[test]
fn random_move_is_none_for_only_the_end_node_and_for_an_empty_tree() {
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5")]);
    let r = sorted(&mut db, &pos_after("e2e4 e7e5"), Sort::Frequency, None);
    assert_eq!(sans(&r), vec!["[end]"]);
    assert_eq!(random_move(&r, &mut || 0), None);
    let r = sorted(&mut db, &pos_after("d2d4"), Sort::Frequency, None);
    assert!(r.rows.is_empty());
    assert_eq!(random_move(&r, &mut || 0), None);
}

#[test]
fn random_move_with_a_huge_generator_value_stays_in_range() {
    let r = lcg_free_result();
    for v in [u32::MAX, u32::MAX - 1, 1 << 31] {
        assert!(random_move(&r, &mut || v).is_some());
    }
}

// ------------------------------------------------------------ move_by_index

#[test]
fn move_by_index_is_one_based_and_bounded() {
    let r = lcg_free_result();
    let e4 = row(&r, "e4").mv.unwrap();
    let d4 = row(&r, "d4").mv.unwrap();
    assert_eq!(move_by_index(&r, 0), None);
    assert_eq!(move_by_index(&r, 1), Some(e4));
    assert_eq!(move_by_index(&r, 2), Some(d4));
    assert_eq!(move_by_index(&r, 3), None, "n + 1");
    assert_eq!(move_by_index(&r, usize::MAX), None);
}

#[test]
fn move_by_index_of_the_end_row_is_none() {
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5"), gb("e2e4 e7e5 g1f3")]);
    let r = sorted(&mut db, &pos_after("e2e4 e7e5"), Sort::Alpha, None);
    assert_eq!(sans(&r), vec!["Nf3", "[end]"]);
    assert!(move_by_index(&r, 1).is_some());
    assert_eq!(move_by_index(&r, 2), None);
}

// ------------------------------------------------------------ D944: the pair-swap order
//
// The Scid's `sortTreeMoves` (src/tkscid.cpp, 15050-15093) is:
//
//   for outer in 0..n-1 { for inner in outer+1..n {
//       result = key(outer) - key(inner)          // Frequency, Eco, Score
//       result = strCompare(inner, outer)         // Alpha
//       if result < 0 { swap(node[outer], node[inner]) }
//   } }
//
// It is NOT stable: when the element at `outer` is swapped with a bigger one
// far to the right, it jumps over equal elements. The initial order of the
// rows is the order in which the moves are first met in the database (the
// first game with each move), so the games below are written in that order.
// Every expected vector in this section was traced by hand, step by step, as
// shown in the comments; a stable sort or a `<=` instead of `<` must fail at
// least one of them.

/// First moves of the standard position used as rows A, B, C, D, E (in the
/// order of first appearance in the database) with their SAN.
const ROOT: [(&str, &str); 5] = [
    ("e2e4", "e4"),
    ("d2d4", "d4"),
    ("c2c4", "c4"),
    ("g1f3", "Nf3"),
    ("g2g3", "g3"),
];

/// Games such that the rows appear as `ROOT[0..totals.len()]` with the given
/// number of games each: one game per row first (fixing the node order), then
/// the remaining games.
fn games_with_totals(totals: &[usize]) -> Vec<base::B> {
    let mut games = Vec::new();
    let most = totals.iter().copied().max().unwrap_or(0);
    for round in 1..=most {
        for (i, &t) in totals.iter().enumerate() {
            if t >= round {
                games.push(gb(ROOT[i].0));
            }
        }
    }
    games
}

#[rstest]
// A3 B3 C1 D3. outer 0: A(3) vs B(3), C(1), D(3): no `<`, nothing moves.
// outer 1: B vs C, D: no. outer 2: C(1) < D(3): swap -> A B D C.
// (With `<=` the ties would swap and the result would be D B A C.)
#[case(&[3, 3, 1, 3], &["e4", "d4", "Nf3", "c4"])]
// A2 B2 C3. outer 0: A vs B: tie, no swap; A(2) < C(3): swap -> C B A.
// outer 1: B vs A: tie, no swap. Result C B A (a stable sort gives C A B).
#[case(&[2, 2, 3], &["c4", "d4", "e4"])]
// A2 B3 C2 D3 E2. outer 0: A<B swap -> B A C D E; B vs C(2), D(3), E(2):
// no. outer 1 (A=2): vs C(2): no; vs D(3): swap -> B D C A E; D vs E: no.
// outer 2 (C=2): vs A(2), E(2): no. outer 3 (A=2) vs E(2): no.
// Result B D C A E (stable: B D A C E).
#[case(&[2, 3, 2, 3, 2], &["d4", "Nf3", "c4", "e4", "g3"])]
// Five ties: nothing is ever `<`, so the database order is kept.
#[case(&[2, 2, 2, 2, 2], &["e4", "d4", "c4", "Nf3", "g3"])]
// A2 B2 C2 D2 E3. outer 0: A vs B, C, D: no; A(2) < E(3): swap -> E B C D A.
// outer 1: B vs C, D, A: ties. Result E B C D A (stable: E A B C D).
#[case(&[2, 2, 2, 2, 3], &["g3", "d4", "c4", "Nf3", "e4"])]
fn frequency_follows_the_scid_pair_swap_trace(#[case] totals: &[usize], #[case] expected: &[&str]) {
    let (_fx, mut db) = db_of(games_with_totals(totals));
    let r = sorted(&mut db, &Position::new(), Sort::Frequency, None);
    assert_eq!(sans(&r), expected);
    for (i, &t) in totals.iter().enumerate() {
        assert_eq!(row(&r, ROOT[i].1).total as usize, t);
    }
}

#[test]
fn score_with_ties_and_white_to_move_follows_the_scid_pair_swap_trace() {
    // Rows in database order: A e4 (1 draw, 500), B d4 (1 win + 1 loss, 500),
    // C c4 (1 loss, 0), D Nf3 (1 win, 1000), E g3 (1 draw, 500).
    // White moves: swap when score(outer) < score(inner).
    // outer 0: A(500) vs B, C: no; A < D(1000): swap -> D B C A E; D vs E: no.
    // outer 1: B(500) vs C(0) no, vs A(500) no, vs E(500) no.
    // outer 2: C(0) vs A(500): swap -> D B A C E; A(500) vs E(500): no.
    // outer 3: C(0) vs E(500): swap -> D B A E C.
    // Result Nf3 d4 e4 g3 c4 (a stable sort would put e4 before d4).
    let (_fx, mut db) = db_of(vec![
        gb("e2e4").result(D),
        gb("d2d4").result(W),
        gb("c2c4").result(BL),
        gb("g1f3").result(W),
        gb("g2g3").result(D),
        gb("d2d4").result(BL),
    ]);
    let r = sorted(&mut db, &Position::new(), Sort::Score, None);
    assert_eq!(r.turn, Color::White);
    assert_eq!(sans(&r), vec!["Nf3", "d4", "e4", "g3", "c4"]);
    assert_eq!(row(&r, "d4").score_permille, 500);
    assert_eq!(row(&r, "e4").score_permille, 500);
}

#[test]
fn score_with_ties_and_black_to_move_follows_the_scid_pair_swap_trace() {
    // After 1.e4. Rows in database order: A e5 (draw, 500), B c5 (win + loss,
    // 500), C e6 (loss, 0), D Nf6 (win, 1000), E d5 (draw, 500).
    // Black moves: the result is negated, so swap when score(outer) >
    // score(inner) (ascending White score).
    // outer 0: A(500) vs B(500): no; A > C(0): swap -> C B A D E; C(0) vs D,
    // E: no. outer 1: B(500) vs A(500) no, D(1000) no, E(500) no.
    // outer 2: A(500) vs D(1000) no, vs E(500) no.
    // outer 3: D(1000) > E(500): swap -> C B A E D.
    // Result e6 c5 e5 d5 Nf6 (a stable sort would put e5 before c5).
    let (_fx, mut db) = db_of(vec![
        gb("e2e4 e7e5").result(D),
        gb("e2e4 c7c5").result(W),
        gb("e2e4 e7e6").result(BL),
        gb("e2e4 g8f6").result(W),
        gb("e2e4 d7d5").result(D),
        gb("e2e4 c7c5").result(BL),
    ]);
    let r = sorted(&mut db, &pos_after("e2e4"), Sort::Score, None);
    assert_eq!(r.turn, Color::Black);
    assert_eq!(sans(&r), vec!["e6", "c5", "e5", "d5", "Nf6"]);
}

#[test]
fn eco_without_a_book_leaves_every_code_at_zero_and_keeps_the_database_order() {
    // All keys are 0: no `result < 0`, no swap, whatever the frequencies.
    let (_fx, mut db) = db_of(games_with_totals(&[1, 3, 2, 4]));
    let r = sorted(&mut db, &Position::new(), Sort::Eco, None);
    assert_eq!(sans(&r), vec!["e4", "d4", "c4", "Nf3"]);
    assert!(r.rows.iter().all(|x| x.eco.is_none()));
}

#[test]
fn eco_ties_among_rows_without_code_follow_the_scid_pair_swap_trace() {
    // Rows in database order: A g3 (0), B b3 (0), C e4 (C20). Descending code.
    // outer 0: A(0) vs B(0): no; A(0) < C: swap -> C B A. outer 1: B vs A: no.
    // Result e4 b3 g3 (a stable sort gives e4 g3 b3).
    let book = eco_book();
    let (_fx, mut db) = db_of(vec![gb("g2g3"), gb("b2b3"), gb("e2e4")]);
    let r = sorted(&mut db, &Position::new(), Sort::Eco, Some(&book));
    assert_eq!(sans(&r), vec!["e4", "b3", "g3"]);
}

#[test]
fn eco_with_five_rows_follows_the_scid_pair_swap_trace() {
    // Codes: A04 < A10 < C20. Rows: A g3 (0), B c4 (A10), C b3 (0), D e4 (C20),
    // E Nf3 (A04).
    // outer 0: A(0) < B: swap -> B A C D E; B(A10) vs C(0) no; B < D: swap ->
    // D A C B E; D vs E: no.
    // outer 1: A(0) vs C(0): no; A < B(A10): swap -> D B C A E; B(A10) vs
    // E(A04): no.
    // outer 2: C(0) vs A(0): no; C < E(A04): swap -> D B E A C.
    // outer 3: A(0) vs C(0): no.
    // Result e4 c4 Nf3 g3 b3.
    let book = eco_book();
    let (_fx, mut db) = db_of(vec![
        gb("g2g3"),
        gb("c2c4"),
        gb("b2b3"),
        gb("e2e4"),
        gb("g1f3"),
    ]);
    let r = sorted(&mut db, &Position::new(), Sort::Eco, Some(&book));
    assert_eq!(sans(&r), vec!["e4", "c4", "Nf3", "g3", "b3"]);
}

#[test]
fn alpha_orders_castling_knights_end_and_pawns_by_raw_bytes() {
    // After 1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5: 'N' (0x4E) < 'O' (0x4F) < '['
    // (0x5B) < lowercase. Keys are distinct, so the order is the sorted one
    // whatever the database order (which is scrambled here).
    let prefix = "e2e4 e7e5 g1f3 b8c6 f1c4 f8c5";
    let (_fx, mut db) = db_of(vec![
        gb(&format!("{prefix} d2d3")),
        gb(prefix),
        gb(&format!("{prefix} e1g1")),
        gb(&format!("{prefix} c2c3")),
        gb(&format!("{prefix} f3g5")),
        gb(&format!("{prefix} b1c3")),
    ]);
    let r = sorted(&mut db, &pos_after(prefix), Sort::Alpha, None);
    assert_eq!(sans(&r), vec!["Nc3", "Ng5", "O-O", "[end]", "c3", "d3"]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Frequency order equals the literal loops of `sortTreeMoves` on random
    /// totals (ties included).
    #[test]
    fn frequency_equals_a_literal_port_of_the_scid_loops(
        totals in proptest::collection::vec(1usize..=4, 2..=5)
    ) {
        let mut idx: Vec<usize> = (0..totals.len()).collect();
        for outer in 0..idx.len() - 1 {
            for inner in outer + 1..idx.len() {
                if totals[idx[outer]] < totals[idx[inner]] {
                    idx.swap(outer, inner);
                }
            }
        }
        let expected: Vec<&str> = idx.iter().map(|&i| ROOT[i].1).collect();
        let (_fx, mut db) = db_of(games_with_totals(&totals));
        let r = sorted(&mut db, &Position::new(), Sort::Frequency, None);
        prop_assert_eq!(sans(&r), expected);
    }
}
