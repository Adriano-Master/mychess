//! Contract tests of the opening tree engine, part 4: the best-games list
//! (rule 21, D946, D955e).
#![allow(clippy::unwrap_used, clippy::too_many_lines, clippy::assert_is_empty)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m5_1_support.rs"]
mod support;

use std::path::Path;

use mychess_core::{GameResult, Position};
use mychess_db::pgn_db::{OpenOptions, PgnDatabase};
use mychess_db::tree::{
    BestGame, BestOptions, BestOrder, ResultFilter, TreeOptions, TreeState, best_games,
};
use mychess_db::{CancelToken, GameSource, NoProgress, PlayerStats};
use pretty_assertions::assert_eq;
use support::{db_of, gb, pos_after, run};

const W: GameResult = GameResult::White;
const BL: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;
const NONE: GameResult = GameResult::Unknown;

const LINE: &str = "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5a4 g8f6";

fn ids(v: &[BestGame]) -> Vec<u32> {
    v.iter().map(|b| b.game).collect()
}

fn best(db: &mut impl GameSource, state: &TreeState, opts: &BestOptions) -> Vec<BestGame> {
    let stats = PlayerStats::from_database(db);
    best_games(db, state, &stats, opts).unwrap()
}

fn all(max: u32, order: BestOrder) -> BestOptions {
    BestOptions {
        max,
        order,
        ..BestOptions::default()
    }
}

/// Games 1..=6 after 1.e4 with Elos giving the averages
/// 2400, 2550, 2500, 2500, 2550, 2500 (the last one truncated 5001 / 2).
fn rated_db() -> (base::Fixture, mychess_db::si4::Si4Database, TreeState) {
    let (fx, mut db) = db_of(vec![
        gb(LINE).elo(2400, 2400),
        gb(LINE).elo(2600, 2500),
        gb(LINE).elo(2500, 2500),
        gb(LINE).elo(2500, 2500),
        gb(LINE).elo(2600, 2500),
        gb(LINE).elo(2501, 2500),
    ]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    (fx, db, state)
}

#[test]
fn best_is_empty_before_any_search_and_for_max_zero() {
    let (_fx, mut db, state) = rated_db();
    assert!(best(&mut db, &TreeState::default(), &all(10, BestOrder::Best)).is_empty());
    assert!(best(&mut db, &state, &all(0, BestOrder::Best)).is_empty());
    assert!(best(&mut db, &state, &all(0, BestOrder::Order)).is_empty());
}

#[test]
fn best_orders_by_average_elo_descending_ties_in_database_order() {
    let (_fx, mut db, state) = rated_db();
    let out = best(&mut db, &state, &all(10, BestOrder::Best));
    assert_eq!(ids(&out), vec![2, 5, 3, 4, 6, 1]);
    let elos: Vec<u32> = out.iter().map(|b| b.avg_elo).collect();
    assert_eq!(elos, vec![2550, 2550, 2500, 2500, 2500, 2400]);
}

#[test]
fn best_keeps_only_the_top_max_games() {
    let (_fx, mut db, state) = rated_db();
    assert_eq!(
        ids(&best(&mut db, &state, &all(3, BestOrder::Best))),
        vec![2, 5, 3]
    );
    assert_eq!(
        ids(&best(&mut db, &state, &all(1, BestOrder::Best))),
        vec![2]
    );
}

#[test]
fn order_gives_the_last_games_of_the_filter_in_database_order() {
    let (_fx, mut db, state) = rated_db();
    assert_eq!(
        ids(&best(&mut db, &state, &all(2, BestOrder::Order))),
        vec![5, 6]
    );
    assert_eq!(
        ids(&best(&mut db, &state, &all(100, BestOrder::Order))),
        vec![1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn best_only_lists_games_of_the_tree_filter() {
    let (_fx, mut db) = db_of(vec![
        gb(LINE).elo(2000, 2000),
        gb("d2d4 d7d5").elo(2800, 2800),
        gb(LINE).elo(2100, 2100),
    ]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    assert_eq!(
        ids(&best(&mut db, &state, &all(10, BestOrder::Best))),
        vec![3, 1]
    );
    assert_eq!(
        ids(&best(&mut db, &state, &all(10, BestOrder::Order))),
        vec![1, 3]
    );
}

fn results_db() -> (base::Fixture, mychess_db::si4::Si4Database, TreeState) {
    let (fx, mut db) = db_of(vec![
        gb(LINE).result(W),
        gb(LINE).result(BL),
        gb(LINE).result(D),
        gb(LINE).result(NONE),
        gb(LINE).result(W),
    ]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    (fx, db, state)
}

#[test]
fn result_filter_selects_by_result_in_both_orders() {
    let (_fx, mut db, state) = results_db();
    for order in [BestOrder::Best, BestOrder::Order] {
        let mut pick = |results| {
            let o = BestOptions {
                max: 10,
                results,
                order,
                ..BestOptions::default()
            };
            ids(&best(&mut db, &state, &o))
        };
        assert_eq!(pick(ResultFilter::All), vec![1, 2, 3, 4, 5], "{order:?}");
        assert_eq!(pick(ResultFilter::White), vec![1, 5], "{order:?}");
        assert_eq!(pick(ResultFilter::Black), vec![2], "{order:?}");
        assert_eq!(pick(ResultFilter::Decisive), vec![1, 2, 5], "{order:?}");
        assert_eq!(pick(ResultFilter::Draw), vec![3], "{order:?}");
        assert_eq!(pick(ResultFilter::None), vec![4], "{order:?}");
    }
}

#[test]
fn order_applies_the_result_filter_before_taking_the_last_games() {
    let (_fx, mut db, state) = results_db();
    let o = BestOptions {
        max: 1,
        results: ResultFilter::White,
        order: BestOrder::Order,
        ..BestOptions::default()
    };
    assert_eq!(ids(&best(&mut db, &state, &o)), vec![5]);
    let o = BestOptions {
        max: 2,
        results: ResultFilter::Decisive,
        order: BestOrder::Order,
        ..BestOptions::default()
    };
    assert_eq!(ids(&best(&mut db, &state, &o)), vec![2, 5]);
}

#[test]
fn missing_index_elo_comes_from_the_name_base() {
    // Game 3 has no Elo of its own for "Magnus", whose best Elo (2800) is in
    // game 2, which is not in the tree (1.d4).
    let (_fx, mut db) = db_of(vec![
        gb(LINE).elo(2450, 2450),
        {
            let mut b = gb("d2d4 d7d5").elo(2800, 2000);
            b.0.tags_mut().white = "Magnus".into();
            b
        },
        {
            let mut b = gb(LINE).elo(0, 2000);
            b.0.tags_mut().white = "Magnus".into();
            b
        },
        gb(LINE).elo(0, 0),
    ]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let out = best(&mut db, &state, &all(10, BestOrder::Best));
    // Game 3: (2800 from the name base + 2000) / 2 = 2400; game 1: 2450;
    // game 4: the players "W"/"B" have Elo 2450 in game 1 -> (2450 + 2450) / 2.
    assert_eq!(ids(&out), vec![1, 4, 3]);
    let elos: Vec<u32> = out.iter().map(|b| b.avg_elo).collect();
    assert_eq!(elos, vec![2450, 2450, 2400]);
}

#[test]
fn best_games_report_the_tree_ply_and_the_next_six_plies() {
    let (_fx, mut db, state) = rated_db();
    let out = best(&mut db, &state, &all(1, BestOrder::Best));
    assert_eq!(out[0].ply, 1);
    assert_eq!(
        out[0].next_moves,
        vec!["e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4"]
    );
    // From the start position the first six plies.
    let (_fx, mut db) = db_of(vec![gb(LINE)]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let out = best(&mut db, &state, &all(1, BestOrder::Best));
    assert_eq!(out[0].ply, 0);
    assert_eq!(
        out[0].next_moves,
        vec!["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]
    );
}

#[test]
fn best_games_next_moves_are_shorter_at_the_end_of_the_game() {
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5 g1f3")]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let out = best(&mut db, &state, &all(5, BestOrder::Best));
    assert_eq!(out[0].next_moves, vec!["e5", "Nf3"]);
    // A game that ends exactly at the position has no next moves.
    let (_fx, mut db) = db_of(vec![gb("e2e4")]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &pos_after("e2e4"),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let out = best(&mut db, &state, &all(5, BestOrder::Best));
    assert_eq!(ids(&out), vec![1]);
    assert!(out[0].next_moves.is_empty());
}

#[test]
fn current_ply_replaces_the_tree_ply_for_a_repetition() {
    // The start position occurs at ply 0 and at ply 4 of the game.
    let (_fx, mut db) = db_of(vec![gb("g1f3 g8f6 f3g1 f6g8 e2e4")]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let plain = best(&mut db, &state, &all(1, BestOrder::Best));
    assert_eq!(plain[0].ply, 0);
    let o = BestOptions {
        max: 1,
        current_ply: Some(4),
        ..BestOptions::default()
    };
    let out = best(&mut db, &state, &o);
    assert_eq!(out[0].ply, 4);
    assert_eq!(out[0].next_moves, vec!["e4"]);
}

#[test]
fn current_ply_is_ignored_when_the_game_is_not_there() {
    // The game never returns to the start position at ply 4.
    let (_fx, mut db) = db_of(vec![gb("e2e4 e7e5 g1f3 b8c6 f1b5")]);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let o = BestOptions {
        max: 1,
        current_ply: Some(4),
        ..BestOptions::default()
    };
    assert_eq!(best(&mut db, &state, &o)[0].ply, 0);
}

fn tiny_pgn_db(games: usize) -> PgnDatabase {
    let mut text = String::new();
    for _ in 0..games {
        text.push_str("[Event \"?\"]\n[Result \"*\"]\n\n1. e4 *\n\n");
    }
    PgnDatabase::from_reader(
        text.as_bytes(),
        Path::new("t.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .map(|(db, _)| db)
    .unwrap()
}

#[test]
fn best_is_capped_at_one_thousand_games() {
    let mut db = tiny_pgn_db(1100);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let best_out = best(&mut db, &state, &all(5000, BestOrder::Best));
    assert_eq!(best_out.len(), 1000);
    // All averages are equal: database order.
    assert_eq!(best_out.first().unwrap().game, 1);
    assert_eq!(best_out.last().unwrap().game, 1000);
    let order_out = best(&mut db, &state, &all(u32::MAX, BestOrder::Order));
    assert_eq!(order_out.len(), 1000);
    assert_eq!(order_out.first().unwrap().game, 101);
    assert_eq!(order_out.last().unwrap().game, 1100);
}
