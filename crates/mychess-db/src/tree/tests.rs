use std::path::Path;

use mychess_core::Position;

use super::*;
use crate::PlayerStats;
use crate::pgn_db::{OpenOptions, PgnDatabase};
use crate::progress::NoProgress;

fn db(pgn: &str) -> PgnDatabase {
    PgnDatabase::from_reader(
        pgn.as_bytes(),
        Path::new("t.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .map(|(db, _)| db)
    .expect("test pgn opens")
}

fn game(white_elo: u32, result: &str, moves: &str) -> String {
    format!(
        "[Event \"?\"]\n[Date \"2000.01.01\"]\n[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"{white_elo}\"]\n[BlackElo \"2000\"]\n[Result \"{result}\"]\n\n{moves} {result}\n\n"
    )
}

fn run(
    db: &mut PgnDatabase,
    pos: &Position,
    opts: &TreeOptions<'_>,
    state: &mut TreeState,
) -> Result<TreeResult, TreeError> {
    let filter = db.new_filter();
    let query = TreeQuery {
        position: pos,
        ply: 0,
        main_filter: &filter,
    };
    search(
        db,
        &query,
        opts,
        state,
        &mut NoCache,
        &CancelToken::new(),
        &mut NoProgress,
    )
}

fn sample() -> PgnDatabase {
    let pgn = [
        game(2500, "1-0", "1. e4 e5 2. Nf3"),
        game(2500, "0-1", "1. e4 c5"),
        game(2500, "1/2-1/2", "1. d4 d5"),
        game(2500, "1-0", "1. e4 e5"),
    ]
    .concat();
    db(&pgn)
}

#[test]
fn start_position_has_one_row_per_first_move() {
    let mut db = sample();
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let rows: Vec<(&str, u32)> = r.rows.iter().map(|r| (r.san.as_str(), r.total)).collect();
    assert_eq!(rows[0], ("e4", 3));
    assert_eq!(rows[1], ("d4", 1));
    assert_eq!(r.total.as_ref().unwrap().total, 4);
    // e4: W, B, W -> 2 wins + 0 = 4 points * 500 / 3
    assert_eq!(r.rows[0].score_permille, 666);
    assert_eq!(state.tree_filter().unwrap().count(), 4);
}

#[test]
fn end_of_game_row_and_first_occurrence() {
    let mut db = sample();
    let pos = {
        let mut p = Position::new();
        for m in ["e2e4", "e7e5"] {
            let mv = p.parse_uci(m).unwrap();
            p.play(mv).unwrap();
        }
        p
    };
    let mut state = TreeState::default();
    let r = run(&mut db, &pos, &TreeOptions::default(), &mut state).unwrap();
    let san: Vec<&str> = r.rows.iter().map(|r| r.san.as_str()).collect();
    assert_eq!(san.len(), 2);
    assert!(san.contains(&"Nf3") && san.contains(&"[end]"));
    assert_eq!(move_by_index(&r, 1).is_some(), !r.rows[0].is_end());
}

#[test]
fn adjust_and_clean() {
    let mut db = sample();
    let mut state = TreeState::default();
    let opts = TreeOptions {
        adjust: true,
        ..TreeOptions::default()
    };
    let mut p = Position::new();
    let mv = p.parse_uci("d2d4").unwrap();
    p.play(mv).unwrap();
    let r = run(&mut db, &p, &opts, &mut state).unwrap();
    let adj = r.adjusted_filter.unwrap();
    assert_eq!(adj.count(), 1);
    assert_eq!(adj.get(3), 2);
    let restored = state.clean().unwrap();
    assert_eq!(restored.count(), 4);
    assert!(state.tree_filter().is_none());
}

#[test]
fn cancelled_search_leaves_state_untouched() {
    let mut db = sample();
    let mut state = TreeState::default();
    let filter = db.new_filter();
    let cancel = CancelToken::new();
    cancel.cancel();
    let pos = Position::new();
    let query = TreeQuery {
        position: &pos,
        ply: 0,
        main_filter: &filter,
    };
    let r = search(
        &mut db,
        &query,
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
        &cancel,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(TreeError::Cancelled)));
    assert!(state.tree_filter().is_none() && state.db_filter().is_none());
}

#[test]
fn random_move_is_weighted_and_unbiased() {
    let mut db = sample();
    let mut state = TreeState::default();
    let r = run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let pick = |x: u32| {
        random_move(&r, &mut || x).and_then(|m| r.rows.iter().position(|w| w.mv == Some(m)))
    };
    // rows: e4 (3), d4 (1): r in 0..3 is e4, r = 3 is d4.
    assert_eq!(pick(0), Some(0));
    assert_eq!(pick(2), Some(0));
    assert_eq!(pick(3), Some(1));
}

#[test]
fn best_games_orders_by_rating() {
    let pgn = [
        game(2100, "1-0", "1. e4 e5"),
        game(2700, "1-0", "1. e4 c5"),
        game(2100, "0-1", "1. e4 e6"),
    ]
    .concat();
    let mut db = db(&pgn);
    let mut state = TreeState::default();
    run(
        &mut db,
        &Position::new(),
        &TreeOptions::default(),
        &mut state,
    )
    .unwrap();
    let stats = PlayerStats::from_database(&db);
    let best = best_games(
        &mut db,
        &state,
        &stats,
        &BestOptions {
            max: 2,
            ..BestOptions::default()
        },
    )
    .unwrap();
    let numbers: Vec<u32> = best.iter().map(|b| b.game).collect();
    assert_eq!(numbers, [2, 1]);
    assert_eq!(best[0].next_moves, ["e4", "c5"]);
}

#[test]
fn commit_without_search_fails() {
    let mut state = TreeState::default();
    assert!(matches!(
        state.commit_filter(&Filter::new(3)),
        Err(TreeError::NothingToCommit)
    ));
}
