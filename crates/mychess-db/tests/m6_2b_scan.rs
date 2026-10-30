//! Tests of the scan of a database for a tactical pattern (spec
//! `docs/specs/m6-2-taticas-e-padroes.md`, "Varredura", D3253, D3258,
//! D3294 to D3296): filter values, operations, folding by result,
//! cancellation, unreadable games, determinism across thread counts, PGN
//! and SI4 sources, and the occurrences of one loaded game.
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    dead_code
)]

#[path = "common/m3_3a_support.rs"]
mod base;
#[path = "common/m6_1a_support.rs"]
mod support;

use mychess_db::pattern::model::RoleName;
use mychess_db::pattern::model::{
    Condition, MateCond, MateMode, Pattern, ResultKind, Scope, Side, Who,
};
use mychess_db::pattern::presets::{self, Params};
use mychess_db::pattern::{
    HighlightKind, ScanError, ScanLimits, estimate_cost, hits_in_game, search_pattern,
    search_pattern_with,
};
use mychess_db::{CancelToken, Filter, FilterOp, GameSource, NoProgress};
use pretty_assertions::assert_eq;
use support::{pgn_fixture, values};

fn db() -> mychess_db::PgnDatabase {
    pgn_fixture("m6_2b_tactics.pgn")
}

fn run(
    db: &mut impl GameSource,
    pattern: &Pattern,
    op: FilterOp,
    filter: &mut Filter,
) -> mychess_db::pattern::PatternReport {
    search_pattern(
        db,
        pattern,
        op,
        filter,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

fn reset(
    db: &mut impl GameSource,
    pattern: &Pattern,
) -> (Vec<(u32, u8)>, mychess_db::pattern::PatternReport) {
    let mut filter = Filter::new(db.len());
    let report = run(db, pattern, FilterOp::Reset, &mut filter);
    (values(&filter), report)
}

fn preset(id: &str, params: &Params) -> Pattern {
    presets::build(id, params).unwrap()
}

fn mate(by: Who, n: u8, mode: MateMode, forcing_only: bool, max_nodes: u32) -> Pattern {
    Pattern {
        name: "mate".into(),
        description: String::new(),
        scope: Scope {
            min_move: 0,
            ..Scope::default()
        },
        root: Condition::Mate(MateCond {
            by,
            moves_min: n,
            moves_max: n,
            mode,
            forcing_only,
            max_nodes,
        }),
    }
}

#[test]
fn mate_in_one_available_gives_ply_plus_one() {
    let mut db = db();
    let white = Params {
        by: Who::White,
        n: 1,
        ..Params::default()
    };
    let (got, report) = reset(&mut db, &preset("mate_in", &white));
    // Scholar's mate at ply 6 and Legal's mate at ply 12 (white to move).
    assert_eq!(got, vec![(1, 7), (5, 13)]);
    assert_eq!(report.matched, 2);
    assert_eq!(report.start_count, 8);
    assert!(report.uses_available_mate);
    assert!(!report.interrupted);
    assert!(report.positions_examined > 0);

    let black = Params {
        by: Who::Black,
        n: 1,
        ..Params::default()
    };
    assert_eq!(reset(&mut db, &preset("mate_in", &black)).0, vec![(2, 4)]);
    let any = Params {
        n: 1,
        ..Params::default()
    };
    assert_eq!(
        reset(&mut db, &preset("mate_in", &any)).0,
        vec![(1, 7), (2, 4), (5, 13)]
    );
}

#[test]
fn played_mate_only_looks_at_the_end_of_the_game() {
    let mut db = db();
    let p = Params {
        by: Who::White,
        n: 1,
        mate_mode: MateMode::Played,
        ..Params::default()
    };
    // Same games here, because those games end in the mate.
    let (got, _) = reset(&mut db, &preset("mate_in", &p));
    assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![1, 5]);
}

#[test]
fn tactical_presets_find_their_games() {
    let mut db = db();
    let white = Params {
        by: Who::White,
        ..Params::default()
    };
    let fork = Params {
        roles: vec![RoleName::Knight],
        ..white.clone()
    };
    assert_eq!(reset(&mut db, &preset("fork", &fork)).0, vec![(3, 2)]);

    let pin = Params {
        roles: vec![RoleName::Bishop, RoleName::Rook, RoleName::Queen],
        target_roles: vec![RoleName::Knight, RoleName::Bishop, RoleName::Pawn],
        ..white.clone()
    };
    // Game 1: Qh5 pins the f7 pawn against the king (ply 5); game 4: Bb5
    // pins the c6 knight once the d pawn has moved (ply 6).
    assert_eq!(reset(&mut db, &preset("pin", &pin)).0, vec![(1, 6), (4, 7)]);

    let sac = Params {
        roles: vec![RoleName::Queen],
        min_net: 5,
        outcome: mychess_db::pattern::model::Outcome::Mate,
        require_win: true,
        ..white.clone()
    };
    assert_eq!(reset(&mut db, &preset("sacrifice", &sac)).0, vec![(5, 9)]);

    let any_promotion = Params::default();
    assert_eq!(
        reset(&mut db, &preset("promotion", &any_promotion)).0,
        vec![(7, 2), (8, 2)]
    );
    let under = Params {
        roles: vec![RoleName::Rook, RoleName::Bishop, RoleName::Knight],
        ..Params::default()
    };
    assert_eq!(reset(&mut db, &preset("promotion", &under)).0, vec![(8, 2)]);
}

#[test]
fn result_conditions_fold_games_out_without_reading_them() {
    let mut db = db();
    let pattern = Pattern {
        name: "black wins, white to move".into(),
        description: String::new(),
        scope: Scope::default(),
        root: Condition::All {
            items: vec![
                Condition::Result {
                    results: vec![ResultKind::BlackWins],
                },
                Condition::SideToMove { side: Side::White },
            ],
        },
    };
    let (got, report) = reset(&mut db, &pattern);
    assert_eq!(got, vec![(2, 3)]);
    assert_eq!(report.games_folded_out, 7);
}

#[test]
fn ignore_colors_tries_the_mirrored_game_with_the_same_ply() {
    let mut db = db();
    let mut p = mate(Who::Black, 1, MateMode::Available, false, 20_000);
    assert_eq!(reset(&mut db, &p).0, vec![(2, 4)]);
    p.scope.ignore_colors = true;
    assert_eq!(reset(&mut db, &p).0, vec![(1, 7), (2, 4), (5, 13)]);
}

#[test]
fn and_or_and_reset_follow_the_filter_operations() {
    let mut db = db();
    let p = mate(Who::Any, 1, MateMode::Available, false, 20_000);
    let mut filter = Filter::new(8);
    filter.fill(0);
    filter.set(1, 3);
    filter.set(2, 3);
    run(&mut db, &p, FilterOp::And, &mut filter);
    assert_eq!(values(&filter), vec![(1, 7), (2, 4)]);

    let mut filter = Filter::new(8);
    filter.fill(0);
    filter.set(1, 3);
    let report = run(&mut db, &p, FilterOp::Or, &mut filter);
    // Game 1 keeps its value; the others are examined.
    assert_eq!(values(&filter), vec![(1, 3), (2, 4), (5, 13)]);
    assert_eq!(report.start_count, 8);

    let mut filter = Filter::new(8);
    filter.fill(0);
    run(&mut db, &p, FilterOp::Reset, &mut filter);
    assert_eq!(filter.count(), 3);
}

#[test]
fn cancelling_keeps_the_filter() {
    let mut db = db();
    let p = mate(Who::Any, 1, MateMode::Available, false, 20_000);
    let mut filter = Filter::new(8);
    filter.fill(0);
    filter.set(6, 9);
    let before = filter.clone();
    let cancel = CancelToken::new();
    cancel.cancel();
    let report = search_pattern(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &cancel,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(filter, before);
    assert_eq!(report.matched, 1);
}

#[test]
fn invalid_pattern_and_wrong_filter_size_are_errors() {
    let mut db = db();
    let mut bad = mate(Who::Any, 1, MateMode::Available, false, 20_000);
    bad.root = Condition::All { items: vec![] };
    let mut filter = Filter::new(8);
    filter.fill(0);
    filter.set(3, 5);
    let err = search_pattern(
        &mut db,
        &bad,
        FilterOp::Reset,
        &mut filter,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(err, Err(ScanError::Pattern(_))));
    assert_eq!(values(&filter), vec![(3, 5)]);

    let ok = mate(Who::Any, 1, MateMode::Available, false, 20_000);
    let mut small = Filter::new(3);
    let err = search_pattern(
        &mut db,
        &ok,
        FilterOp::Reset,
        &mut small,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(err, Err(ScanError::Search(_))));
}

#[test]
fn a_damaged_record_is_unreadable_and_does_not_stop_the_scan() {
    let games: Vec<_> = (1..=4).map(|_| support::game("e2e4 e7e5 g1f3")).collect();
    let (fx, db) = support::db_of(games);
    let entry = db.entry(2).unwrap();
    drop(db);
    let sg4 = fx.base.with_extension("sg4");
    let mut bytes = std::fs::read(&sg4).unwrap();
    let from = entry.offset as usize;
    for b in &mut bytes[from..from + entry.length as usize] {
        *b = 0xAA;
    }
    std::fs::write(&sg4, bytes).unwrap();
    let mut db = fx.open();
    let p = Pattern {
        name: "white to move".into(),
        description: String::new(),
        scope: Scope::default(),
        root: Condition::SideToMove { side: Side::White },
    };
    let (got, report) = reset(&mut db, &p);
    assert_eq!(report.unreadable, vec![2]);
    assert_eq!(report.unreadable_total, 1);
    // Plies 2 (white to move) of the other three games.
    assert_eq!(got, vec![(1, 3), (3, 3), (4, 3)]);
}

#[test]
fn a_game_cut_by_the_frame_limit_only_shows_its_first_part() {
    let mut db = db();
    let p = mate(Who::White, 1, MateMode::Available, false, 20_000);
    let mut filter = Filter::new(8);
    let limits = ScanLimits {
        max_frames: 4,
        ..ScanLimits::default()
    };
    search_pattern_with(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut filter,
        &limits,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(filter.count(), 0);
}

#[test]
fn the_mate_budget_cap_makes_positions_unknown() {
    let mut db = db();
    // Big budget: the pattern is exact. With a tiny cap the same pattern
    // loses answers instead of costing more.
    let p = mate(Who::White, 3, MateMode::Available, false, 2_000_000);
    let mut loose = Filter::new(8);
    let a = search_pattern(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut loose,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    let mut tight = Filter::new(8);
    let limits = ScanLimits {
        mate_nodes_cap: 1_000,
        ..ScanLimits::default()
    };
    let b = search_pattern_with(
        &mut db,
        &p,
        FilterOp::Reset,
        &mut tight,
        &limits,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(b.unknown_positions >= a.unknown_positions);
    assert!(b.unknown_positions > 0, "{b:?}");
}

fn many_games(copies: usize) -> String {
    let text = std::fs::read_to_string(support::fixture_path("m6_2b_tactics.pgn")).unwrap();
    text.repeat(copies)
}

#[test]
fn threads_do_not_change_the_result() {
    let (_dir, mut db) = support::pgn_db(&many_games(25));
    assert_eq!(db.len(), 200);
    let patterns = [
        mate(Who::Any, 2, MateMode::Available, true, 1_000),
        mate(Who::Any, 4, MateMode::Available, false, 1_000),
        preset(
            "fork",
            &Params {
                by: Who::White,
                ..Params::default()
            },
        ),
    ];
    for p in &patterns {
        let mut outcomes = Vec::new();
        for threads in [1usize, 4] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let (got, report) = pool.install(|| reset(&mut db, p));
            outcomes.push((
                got,
                report.matched,
                report.unreadable_total,
                report.positions_examined,
                report.unknown_positions,
                report.games_folded_out,
            ));
        }
        assert_eq!(outcomes[0], outcomes[1], "{}", p.name);
    }
    // The tiny budget really produced unknown positions to compare.
    let (_, report) = reset(&mut db, &patterns[1]);
    assert!(report.unknown_positions > 0);
}

#[test]
fn a_pgn_and_an_si4_source_agree() {
    let mut pgn = db();
    let games: Vec<_> = (1..=pgn.len()).map(|n| pgn.game(n).unwrap()).collect();
    let (_fx, mut si4) = support::db_of(games);
    for p in [
        mate(Who::Any, 1, MateMode::Available, false, 20_000),
        preset("promotion", &Params::default()),
        preset(
            "pin",
            &Params {
                by: Who::White,
                ..Params::default()
            },
        ),
    ] {
        assert_eq!(reset(&mut pgn, &p).0, reset(&mut si4, &p).0, "{}", p.name);
    }
}

#[test]
fn hits_in_game_recomputes_the_ply_and_the_highlights() {
    let mut db = db();
    let sac = Params {
        by: Who::White,
        roles: vec![RoleName::Queen],
        min_net: 5,
        outcome: mychess_db::pattern::model::Outcome::Mate,
        ..Params::default()
    };
    let pattern = preset("sacrifice", &sac);
    let game = db.game(5).unwrap();
    let hits = hits_in_game(&pattern, &game).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![8]);
    assert!(
        hits[0]
            .highlights
            .iter()
            .any(|h| matches!(h.kind, HighlightKind::Square(_)))
    );
    // The filter value of the scan is that ply plus one.
    assert_eq!(reset(&mut db, &pattern).0, vec![(5, 9)]);

    // A game without the pattern has no hits; an invalid pattern is refused.
    let quiet = db.game(6).unwrap();
    assert!(hits_in_game(&pattern, &quiet).unwrap().is_empty());
    let mut bad = pattern.clone();
    bad.root = Condition::All { items: vec![] };
    assert!(hits_in_game(&bad, &quiet).is_err());

    // The pin lasts two positions (6 and 7); only the start of the run counts.
    let pin = preset(
        "pin",
        &Params {
            by: Who::White,
            ..Params::default()
        },
    );
    let pinned = db.game(4).unwrap();
    let hits = hits_in_game(&pin, &pinned).unwrap();
    assert_eq!(hits.iter().map(|h| h.ply).collect::<Vec<_>>(), vec![6]);
}

#[test]
fn the_cost_estimate_asks_for_confirmation_only_when_big() {
    let cheap = preset(
        "fork",
        &Params {
            by: Who::White,
            ..Params::default()
        },
    );
    assert!(!estimate_cost(&cheap, 5_000_000).needs_confirmation);
    let costly = mate(Who::Any, 2, MateMode::Available, true, 20_000);
    assert!(!estimate_cost(&costly, 30_000).needs_confirmation);
    assert!(estimate_cost(&costly, 40_000).needs_confirmation);
    let played = mate(Who::Any, 2, MateMode::Played, true, 20_000);
    assert!(!estimate_cost(&played, 5_000_000).uses_available_mate);
}

#[test]
fn cancelling_reaches_the_mate_solver_quickly() {
    use std::time::{Duration, Instant};
    // Four-move mates with the biggest budget: one position can cost many
    // seconds in a debug build, so a cancel that waited for it would be seen.
    let game = "[Event \"long\"]\n[Result \"1/2-1/2\"]\n\n\
        1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O \
        9. h3 Nb8 10. d4 Nbd7 11. Nbd2 Bb7 12. Bc2 Re8 13. Nf1 Bf8 14. Ng3 g6 15. a4 c5 \
        16. d5 c4 17. Bg5 h6 18. Be3 Bg7 1/2-1/2\n\n";
    let (_dir, mut db) = support::pgn_db(&game.repeat(6));
    assert_eq!(db.len(), 6);
    let p = mate(Who::Any, 4, MateMode::Available, false, 2_000_000);
    let mut filter = Filter::new(db.len());
    filter.fill(0);
    filter.set(3, 7);
    let before = filter.clone();
    let cancel = CancelToken::new();
    let (report, took) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            search_pattern(
                &mut db,
                &p,
                FilterOp::Reset,
                &mut filter,
                &mut NoProgress,
                &cancel,
            )
        });
        std::thread::sleep(Duration::from_millis(400));
        assert!(!worker.is_finished(), "the scan must still be running");
        let t0 = Instant::now();
        cancel.cancel();
        let report = worker.join().unwrap().unwrap();
        (report, t0.elapsed())
    });
    assert!(report.interrupted);
    assert!(took < Duration::from_secs(2), "answered in {took:?}");
    assert_eq!(filter, before);
}

#[test]
fn hits_in_game_with_applies_the_frame_limit() {
    use mychess_db::pattern::hits_in_game_with;
    let g = support::game("e2e4 e7e5 d1h5 b8c6 f1c4 g8f6 h5f7");
    let p = mate(Who::White, 1, MateMode::Played, false, 20_000);
    assert_eq!(hits_in_game(&p, &g).unwrap().len(), 1);
    let short = ScanLimits {
        max_frames: 4,
        ..ScanLimits::default()
    };
    assert!(hits_in_game_with(&p, &g, &short).unwrap().is_empty());
    assert_eq!(
        hits_in_game_with(&p, &g, &ScanLimits::default())
            .unwrap()
            .len(),
        1
    );
}
