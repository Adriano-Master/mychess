//! Priming of the tree cache (M5-3a, D1051, D1055, D1060 of
//! `docs/specs/m5-3-cache-stc.md`): `prime_with_lines`, `prime_with_game`,
//! `prime_with_filter`, cancellation, the progress counts and the refusal of
//! databases that are not Scid files.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m5_3_support.rs"]
mod stc;
#[path = "common/m5_1_support.rs"]
mod support;

use std::collections::BTreeSet;
use std::path::Path;

use mychess_core::{Game, Position, SanMode};
use mychess_db::pgn_db::{OpenOptions, PgnDatabase};
use mychess_db::tree::stc::{
    PrimeError, PrimeReport, STANDARD_LINES, StcCache, prime_with_filter, prime_with_game,
    prime_with_lines,
};
use mychess_db::tree::{NoCache, TreeCache, TreeOptions, TreeState};
use mychess_db::{CancelToken, Filter, GameSource, NoProgress};
use pretty_assertions::assert_eq;
use stc::{Rec, key};
use support::{db_of, gb, pos_after, run, run_with};

use base::play;

fn games() -> Vec<base::B> {
    vec![
        gb("e2e4 e7e5 g1f3"),
        gb("e2e4 c7c5"),
        gb("d2d4 d7d5"),
        gb("e2e4 e7e5"),
        gb("c2c4 e7e5"),
    ]
}

fn keys_of(ucis: &[&str]) -> BTreeSet<String> {
    ucis.iter().map(|u| key(&pos_after(u))).collect()
}

fn set(rec: &Rec<'_>) -> BTreeSet<String> {
    rec.looked.iter().cloned().collect()
}

/// The game 1.e4 e5 2.Nf3 with the variations 1.d4 d5 and 1.d4 Nf6.
fn game_with_variations() -> Game {
    let mut g = Game::new();
    let root = g.root();
    let e4 = play(&mut g, root, "e2e4");
    let e5 = play(&mut g, e4, "e7e5");
    play(&mut g, e5, "g1f3");
    let d4 = play(&mut g, root, "d2d4");
    play(&mut g, d4, "d7d5");
    play(&mut g, d4, "g8f6");
    g
}

fn pgn_db() -> PgnDatabase {
    let pgn = "[Event \"?\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n";
    PgnDatabase::from_reader(
        pgn.as_bytes(),
        Path::new("t.pgn"),
        &OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
    .0
}

fn line_keys(line: &str) -> String {
    let mut pos = Position::new();
    for token in line.split_whitespace() {
        let san = token
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches('.');
        if san.is_empty() {
            continue;
        }
        let mv = pos.parse_san(san, SanMode::Tolerant).unwrap().mv;
        pos.play(mv).unwrap();
    }
    key(&pos)
}

// ------------------------------------------------------------ the standard lines

#[test]
fn the_standard_lines_are_110_short_and_include_the_start_position() {
    assert_eq!(STANDARD_LINES.len(), 110);
    assert!(
        STANDARD_LINES.contains(&""),
        "the empty line is the start position"
    );
    for line in STANDARD_LINES {
        let plies = line
            .split_whitespace()
            .filter(|t| !t.trim_end_matches('.').chars().all(|c| c.is_ascii_digit()))
            .count();
        assert!(plies <= 12, "{line}: {plies} plies");
    }
}

#[test]
fn every_standard_line_is_legal_and_each_one_is_a_hit_afterwards() {
    let (_fx, mut db) = db_of(games());
    let mut cache = StcCache::new(None);
    let report = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut cache,
        STANDARD_LINES,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    let distinct: BTreeSet<String> = STANDARD_LINES.iter().map(|l| line_keys(l)).collect();
    assert_eq!(report.searches, 110);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.searches, report.cache_hits + report.stored);
    assert_eq!(
        usize::try_from(report.stored).unwrap(),
        distinct.len(),
        "one tree per distinct position"
    );
    assert_eq!(cache.info().0, distinct.len());

    // Every line is now a hit of the next search.
    let mut state = TreeState::default();
    for line in STANDARD_LINES {
        let mut pos = Position::new();
        for token in line.split_whitespace() {
            let san = token
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches('.');
            if san.is_empty() {
                continue;
            }
            let mv = pos.parse_san(san, SanMode::Tolerant).unwrap().mv;
            pos.play(mv).unwrap();
        }
        let filter = Filter::new(db.len());
        let r = run_with(
            &mut db,
            &pos,
            0,
            &filter,
            &TreeOptions::default(),
            &mut state,
            &mut cache,
        )
        .unwrap();
        assert!(r.from_cache, "{line}");
    }
}

#[test]
fn a_second_pass_over_the_same_lines_only_hits() {
    let (_fx, mut db) = db_of(games());
    let mut cache = StcCache::new(None);
    let lines = ["", "1.e4", "1.e4 e5", "1.d4"];
    let once = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut cache,
        &lines,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(
        once,
        PrimeReport {
            searches: 4,
            cache_hits: 0,
            stored: 4,
            skipped: 0
        }
    );
    let twice = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut cache,
        &lines,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(
        twice,
        PrimeReport {
            searches: 4,
            cache_hits: 4,
            stored: 0,
            skipped: 0
        }
    );
}

#[test]
fn lines_are_read_tolerantly_with_or_without_move_numbers() {
    let (_fx, mut db) = db_of(games());
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    let r = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &[
            "1.e4 e5",
            "e4 e5",
            "1. e4 e5 ",
            "1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O",
        ],
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r.searches, 4);
    assert_eq!(r.stored, 2);
    assert_eq!(r.cache_hits, 2);
    assert_eq!(rec.looked[0], rec.looked[1]);
    assert_eq!(rec.looked[1], rec.looked[2]);
}

#[test]
fn an_illegal_line_is_an_error_before_any_search() {
    let (_fx, mut db) = db_of(games());
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    let r = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &["1.e4", "1.e4 e5 2.Ke3"],
        &CancelToken::new(),
        &mut NoProgress,
    );
    match r {
        Err(PrimeError::IllegalMove { line, token }) => {
            assert_eq!(line, "1.e4 e5 2.Ke3");
            assert!(token.contains("Ke3"), "{token}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rec.looked.is_empty(), "no search was done");
    assert_eq!(cache.info().0, 0);
}

#[test]
fn an_empty_list_of_lines_does_nothing() {
    let (_fx, mut db) = db_of(games());
    let r = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &[],
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r, PrimeReport::default());
}

#[test]
fn lines_report_progress_once_per_line() {
    let (_fx, mut db) = db_of(games());
    let mut seen = Vec::new();
    prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &["", "1.e4", "1.d4"],
        &CancelToken::new(),
        &mut |done: u64, total: u64| seen.push((done, total)),
    )
    .unwrap();
    assert_eq!(seen, [(1, 3), (2, 3), (3, 3)]);
}

#[test]
fn primed_trees_survive_a_write_and_serve_a_new_instance() {
    let (fx, mut db) = db_of(games());
    let mut cache = StcCache::new(Some(&fx.si4()));
    prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut cache,
        STANDARD_LINES,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    cache.write(None).unwrap();
    let mut fresh = StcCache::new(Some(&fx.si4()));
    let r = run_with(
        &mut db,
        &pos_after("e2e4 e7e5"),
        0,
        &Filter::new(5),
        &TreeOptions::default(),
        &mut TreeState::default(),
        &mut fresh,
    )
    .unwrap();
    assert!(r.from_cache);
}

// ------------------------------------------------------------ prime_with_game

#[test]
fn a_game_primes_the_start_the_main_line_and_every_variation() {
    let (_fx, mut db) = db_of(games());
    let game = game_with_variations();
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    let report = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &game,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(
        set(&rec),
        keys_of(&[
            "",
            "e2e4",
            "e2e4 e7e5",
            "e2e4 e7e5 g1f3",
            "d2d4",
            "d2d4 d7d5",
            "d2d4 g8f6",
        ])
    );
    assert_eq!(
        report,
        PrimeReport {
            searches: 7,
            cache_hits: 0,
            stored: 7,
            skipped: 0
        }
    );
    assert_eq!(cache.info().0, 7);
}

#[test]
fn a_game_that_starts_from_a_fen_primes_that_start() {
    let (_fx, mut db) = db_of(games());
    let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    let mut g = Game::from_position(Position::from_fen(fen).unwrap());
    let root = g.root();
    play(&mut g, root, "e2e4");
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    let r = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &g,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r.searches, 2);
    assert!(
        rec.looked[0].starts_with("4k3/8/8/8/8/8/4P3/4K3 w"),
        "{:?}",
        rec.looked
    );
    assert!(!rec.looked.iter().any(|k| *k == key(&Position::new())));
}

#[test]
fn a_game_with_only_the_start_position_is_one_search() {
    let (_fx, mut db) = db_of(games());
    let r = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &Game::new(),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r.searches, 1);
}

#[test]
fn a_game_primes_deep_nested_variations_without_a_depth_limit() {
    let (_fx, mut db) = db_of(games());
    let mut g = Game::new();
    let mut parent = g.root();
    for _ in 0..300 {
        let moves = g.position_at(parent).unwrap().legal_moves();
        if moves.len() < 2 {
            break;
        }
        g.add_move(parent, moves[0]).unwrap();
        parent = g.add_move(parent, moves[1]).unwrap();
    }
    let nodes = g.node_count();
    assert!(nodes > 500, "the chain was built: {nodes}");
    let r = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &g,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(usize::try_from(r.searches + r.skipped).unwrap(), nodes);
}

#[test]
fn a_game_reports_its_progress_once_at_the_end() {
    let (_fx, mut db) = db_of(games());
    let game = game_with_variations();
    let mut seen = Vec::new();
    prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &game,
        &CancelToken::new(),
        &mut |d: u64, t: u64| seen.push((d, t)),
    )
    .unwrap();
    assert_eq!(seen, [(7, 7)]);
}

#[test]
fn priming_a_game_leaves_the_game_alone() {
    let (_fx, mut db) = db_of(games());
    let game = game_with_variations();
    let before = game.node_count();
    prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &game,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(game.node_count(), before);
}

// ------------------------------------------------------------ prime_with_filter

#[test]
fn a_filter_primes_only_the_games_in_it() {
    let (_fx, mut db) = db_of(games());
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(2, 1);
    filter.set(4, 1);
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &filter,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(set(&rec), keys_of(&["", "e2e4", "e2e4 c7c5", "e2e4 e7e5"]));
    // game 2: 3 positions, game 4: 3 positions; 2 repeats are hits.
    assert_eq!(
        r,
        PrimeReport {
            searches: 6,
            cache_hits: 2,
            stored: 4,
            skipped: 0
        }
    );
}

#[test]
fn an_empty_filter_primes_nothing() {
    let (_fx, mut db) = db_of(games());
    let mut filter = Filter::new(5);
    filter.fill(0);
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &filter,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r, PrimeReport::default());
}

#[test]
fn a_filter_of_another_size_is_refused_with_both_sizes() {
    let (_fx, mut db) = db_of(games());
    let mut rec_cache = StcCache::new(None);
    let mut rec = Rec::new(&mut rec_cache);
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &Filter::new(6),
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(
        matches!(
            r,
            Err(PrimeError::FilterSize {
                filter: 6,
                games: 5
            })
        ),
        "{r:?}"
    );
    assert!(rec.looked.is_empty());
}

#[test]
fn the_filter_reports_progress_per_game() {
    let (_fx, mut db) = db_of(games());
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(1, 1);
    filter.set(3, 1);
    filter.set(5, 1);
    let mut seen = Vec::new();
    prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &filter,
        &CancelToken::new(),
        &mut |d: u64, t: u64| seen.push((d, t)),
    )
    .unwrap();
    assert_eq!(seen, [(1, 3), (2, 3), (3, 3)]);
}

// ------------------------------------------------------------ skipped

const MANY_MOVES: &str = "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1";

fn many_moves_db() -> (base::Fixture, mychess_db::si4::Si4Database, Position) {
    let pos = Position::from_fen(MANY_MOVES).unwrap();
    let moves = pos.legal_moves();
    assert!(moves.len() >= 61, "setup: {} legal moves", moves.len());
    let list: Vec<base::B> = moves
        .iter()
        .take(61)
        .map(|m| {
            let mut g = Game::from_position(pos.clone());
            let root = g.root();
            g.add_move(root, *m).unwrap();
            base::B::of(g)
        })
        .collect();
    let (fx, db) = db_of(list);
    (fx, db, pos)
}

#[test]
fn a_position_with_more_than_60_moves_in_the_tree_is_skipped_not_fatal() {
    let (_fx, mut db, pos) = many_moves_db();
    // The engine itself refuses this tree...
    let r = run(
        &mut db,
        &pos,
        &TreeOptions::default(),
        &mut TreeState::default(),
    );
    assert!(
        matches!(r, Err(mychess_db::tree::TreeError::TooManyMoves)),
        "{r:?}"
    );
    // ...which priming counts and goes on.
    let mut g = Game::from_position(pos.clone());
    let root = g.root();
    let m = pos.legal_moves()[0];
    g.add_move(root, m).unwrap();
    let report = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &g,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(report.skipped, 1, "the start position");
    assert_eq!(report.searches, 1, "the position after the move");
}

#[test]
fn a_filter_over_such_a_base_skips_the_start_of_each_game() {
    let (_fx, mut db, _pos) = many_moves_db();
    let filter = Filter::new(db.len());
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &filter,
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert_eq!(r.skipped, 61);
    assert_eq!(r.searches, 61);
}

// ------------------------------------------------------------ not a Scid base

#[test]
fn scid_databases_say_so_and_pgn_databases_do_not() {
    let (_fx, db) = db_of(games());
    assert!(db.is_scid_database());
    assert!(!pgn_db().is_scid_database());
}

#[test]
fn a_pgn_database_is_refused_by_all_three_functions_and_nothing_is_searched() {
    let mut db = pgn_db();
    let mut inner = StcCache::new(None);
    let mut rec = Rec::new(&mut inner);
    let token = CancelToken::new();
    let r = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &["1.e4"],
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::NotScidBase)), "{r:?}");
    let r = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &Game::new(),
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::NotScidBase)), "{r:?}");
    let filter = Filter::new(db.len());
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &filter,
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::NotScidBase)), "{r:?}");
    assert!(rec.looked.is_empty());
}

// ------------------------------------------------------------ cancellation

#[test]
fn a_token_cancelled_before_the_start_searches_nothing() {
    let (_fx, mut db) = db_of(games());
    let token = CancelToken::new();
    token.cancel();
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    let r = prime_with_lines(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        STANDARD_LINES,
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::Cancelled)), "{r:?}");
    assert!(rec.looked.is_empty());
    assert_eq!(cache.info().0, 0);
    let r = prime_with_game(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &game_with_variations(),
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::Cancelled)));
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut NoCache,
        &Filter::new(5),
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::Cancelled)));
}

#[test]
fn cancelling_in_the_middle_keeps_the_finished_trees_and_the_state_usable() {
    let (_fx, mut db) = db_of(games());
    let token = CancelToken::new();
    let mut cache = StcCache::new(None);
    let mut state = TreeState::default();
    {
        let mut rec = Rec::new(&mut cache);
        rec.cancel_after = Some((3, token.clone()));
        let r = prime_with_lines(
            &mut db,
            &mut state,
            &mut rec,
            STANDARD_LINES,
            &token,
            &mut NoProgress,
        );
        assert!(matches!(r, Err(PrimeError::Cancelled)), "{r:?}");
        assert_eq!(rec.stored.len(), 3, "the third search finished");
    }
    // The cache holds exactly the three finished trees, all valid.
    assert_eq!(cache.info().0, 3);
    let mut again = Rec::new(&mut cache);
    #[allow(clippy::needless_range_loop)]
    for k in 0..3 {
        // The same lines, searched again, are hits.
        let line = STANDARD_LINES[k];
        let mut pos = Position::new();
        for t in line.split_whitespace() {
            let san = t
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches('.');
            if san.is_empty() {
                continue;
            }
            let mv = pos.parse_san(san, SanMode::Tolerant).unwrap().mv;
            pos.play(mv).unwrap();
        }
        assert!(again.lookup(&pos, db.len()).is_some(), "line {k}: {line:?}");
    }

    // The state of the caller still works, with a fresh token.
    let pos = pos_after("e2e4");
    let r = run_with(
        &mut db,
        &pos,
        0,
        &Filter::new(5),
        &TreeOptions::default(),
        &mut state,
        &mut NoCache,
    )
    .unwrap();
    let expected = run(
        &mut db,
        &pos,
        &TreeOptions::default(),
        &mut TreeState::default(),
    )
    .unwrap();
    assert_eq!(r.rows, expected.rows);
    assert_eq!(r.total, expected.total);
}

#[test]
fn cancelling_a_filter_priming_in_the_middle_is_an_error_with_a_coherent_cache() {
    let (_fx, mut db) = db_of(games());
    let token = CancelToken::new();
    let mut cache = StcCache::new(None);
    let mut rec = Rec::new(&mut cache);
    rec.cancel_after = Some((2, token.clone()));
    let r = prime_with_filter(
        &mut db,
        &mut TreeState::default(),
        &mut rec,
        &Filter::new(5),
        &token,
        &mut NoProgress,
    );
    assert!(matches!(r, Err(PrimeError::Cancelled)), "{r:?}");
    assert_eq!(rec.stored.len(), 2);
    assert_eq!(cache.info().0, 2);
}

#[test]
fn priming_leaves_the_tree_filter_in_the_state_as_documented() {
    // D1060: the searches leave their filter in the TreeState; callers that
    // want to keep theirs pass a throwaway one.
    let (_fx, mut db) = db_of(games());
    let mut state = TreeState::default();
    prime_with_lines(
        &mut db,
        &mut state,
        &mut NoCache,
        &["1.e4"],
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    assert!(state.tree_filter().is_some());
}
