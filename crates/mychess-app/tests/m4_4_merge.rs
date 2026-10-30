//! M4-4 contract tests of the merge of engine reports into the view (rules
//! 1-3, 16; D738-D746 and D790-D805: D794 order, D796 stale reports, D798 positions without
//! moves, D805 statistics). Synthetic `SearchInfo`s are fed through `update`.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_4_common;

use m4_4_common::*;
use mychess_app::{AnalysisMessage, MAX_HISTORY, Terminal};
use mychess_core::{Game, Position};
use mychess_engine::{BestMove, Bound, EngineEvent, Score, SearchId, SearchInfo, Wdl};
use proptest::prelude::*;

fn three() -> (mychess_app::AppState, mychess_app::engines::RunId, Position) {
    with_three_lines()
}

#[test]
fn reports_before_the_search_started_are_ignored() {
    let (mut state, run, _) = analysing(true, true);
    let start = Position::new();
    info(&mut state, run, report(&start, 1, 1, 5, 10, &["e2e4"]));
    assert!(lines_of(&state, run).is_empty());
}

#[test]
fn lines_arriving_out_of_order_end_up_sorted_by_multipv() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    let start = Position::new();
    started(&mut state, run, 1);
    for (n, uci) in [(3u8, "g1f3"), (1, "e2e4"), (2, "d2d4")] {
        info(&mut state, run, report(&start, 1, n, 9, 30, &[uci]));
    }
    assert_eq!(
        lines_of(&state, run),
        [(1, "1.e4".into()), (2, "1.d4".into()), (3, "1.Nf3".into())]
    );
}

#[test]
fn line_three_before_line_two_keeps_numbers_not_positions() {
    let (mut state, run, _) = analysing(true, true);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    let start = Position::new();
    started(&mut state, run, 1);
    info(&mut state, run, report(&start, 1, 1, 9, 30, &["e2e4"]));
    info(&mut state, run, report(&start, 1, 3, 9, 10, &["g1f3"]));
    let numbers: Vec<u8> = lines_of(&state, run).iter().map(|l| l.0).collect();
    assert_eq!(numbers, [1, 3], "a gap must not renumber the lines");
    info(&mut state, run, report(&start, 1, 2, 9, 20, &["d2d4"]));
    let numbers: Vec<u8> = lines_of(&state, run).iter().map(|l| l.0).collect();
    assert_eq!(numbers, [1, 2, 3]);
}

#[test]
fn a_newer_report_replaces_its_own_line_only() {
    let (mut state, run, start) = three();
    info(&mut state, run, report(&start, 1, 2, 14, 55, &["c2c4"]));
    let v = view(&state, run);
    assert_eq!(v.lines.len(), 3);
    assert_eq!(v.lines[1].depth, 14);
    assert_eq!(v.lines[1].text, "1.c4");
    assert_eq!(v.lines[0].depth, 12);
    assert_eq!(v.lines[2].text, "1.Nf3");
}

#[test]
fn a_line_beyond_the_configured_count_is_dropped() {
    let (mut state, run, start) = three();
    info(&mut state, run, report(&start, 1, 4, 20, 99, &["c2c4"]));
    info(&mut state, run, report(&start, 1, 200, 20, 99, &["c2c4"]));
    assert_eq!(lines_of(&state, run).len(), 3);
}

#[test]
fn a_report_of_an_old_search_is_dropped() {
    let (mut state, run, start) = three();
    info(&mut state, run, report(&start, 0, 1, 40, 99, &["c2c4"]));
    info(&mut state, run, report(&start, 2, 1, 40, 99, &["c2c4"]));
    assert_eq!(lines_of(&state, run)[0].1, "1.e4");
}

#[test]
fn a_report_without_pv_or_without_score_updates_only_the_numbers() {
    let (mut state, run, start) = three();
    let mut no_pv = report(&start, 1, 1, 13, 77, &["c2c4"]);
    no_pv.pv.clear();
    no_pv.nodes = Some(4_000_000);
    info(&mut state, run, no_pv);
    let mut no_score = report(&start, 1, 1, 13, 77, &["c2c4"]);
    no_score.score = None;
    no_score.nodes = Some(5_000_000);
    info(&mut state, run, no_score);
    let v = view(&state, run);
    assert_eq!(v.lines[0].text, "1.e4", "line 1 must be kept");
    assert_eq!(v.lines[0].score, Score::Cp(30));
    assert_eq!(v.stats.nodes, 5_000_000, "counters come from any report");
}

#[test]
fn depth_and_seldepth_come_only_from_a_report_with_line_one() {
    let (mut state, run, start) = three();
    // Line 2 at depth 20 does not move the headline depth.
    let mut deep2 = report(&start, 1, 2, 20, 10, &["d2d4"]);
    deep2.seldepth = Some(30);
    info(&mut state, run, deep2);
    assert_eq!(view(&state, run).stats.depth, 12);
    // A bare currmove at a smaller depth does not lower it either (D805).
    let bare = SearchInfo {
        search: SearchId(1),
        depth: Some(3),
        seldepth: Some(4),
        currmove: Some(start.parse_uci("e2e4").unwrap()),
        currmovenumber: Some(1),
        ..SearchInfo::default()
    };
    info(&mut state, run, bare);
    let v = view(&state, run);
    assert_eq!(v.stats.depth, 12);
    assert_eq!(v.lines[0].depth, 12);
    // Line 1 with depth 15 does.
    let mut l1 = report(&start, 1, 1, 15, 31, &["e2e4"]);
    l1.seldepth = Some(22);
    info(&mut state, run, l1);
    let v = view(&state, run);
    assert_eq!(v.stats.depth, 15);
    assert_eq!(v.stats.seldepth, Some(22));
}

#[test]
fn counters_nps_hash_cpu_tbhits_and_currmove_are_recorded() {
    let (mut state, run, start) = three();
    let bare = SearchInfo {
        search: SearchId(1),
        nps: Some(987_000),
        hashfull_permille: Some(123),
        cpuload_permille: Some(400),
        tbhits: Some(7),
        nodes: Some(1_234_000),
        time_ms: Some(1230),
        currmove: Some(start.parse_uci("g1f3").unwrap()),
        currmovenumber: Some(3),
        ..SearchInfo::default()
    };
    info(&mut state, run, bare);
    let s = &view(&state, run).stats;
    assert_eq!(s.nps, Some(987_000));
    assert_eq!(s.hashfull_permille, Some(123));
    assert_eq!(s.cpuload_permille, Some(400));
    assert_eq!(s.tbhits, Some(7));
    assert_eq!(s.nodes, 1_234_000);
    assert_eq!(s.time_ms, 1230);
    assert_eq!(s.currmove.as_deref(), Some("Nf3"));
    assert_eq!(s.currmove_number, Some(3));
    // maxmovenumber only grows within a position.
    let lower = SearchInfo {
        search: SearchId(1),
        currmove: Some(start.parse_uci("e2e4").unwrap()),
        currmovenumber: Some(1),
        ..SearchInfo::default()
    };
    info(&mut state, run, lower);
    assert_eq!(view(&state, run).stats.max_move_number, 3);
}

#[test]
fn a_report_never_asks_for_effects() {
    let (mut state, run, start) = three();
    let fx = info(
        &mut state,
        run,
        report(&start, 1, 1, 13, 31, &["e2e4", "e7e5"]),
    );
    assert!(
        fx.is_empty(),
        "update of an Info must not return effects: {fx:?}"
    );
}

#[test]
fn wdl_and_score_bound_are_kept_on_the_line() {
    let (mut state, run, start) = three();
    let mut r = report(&start, 1, 1, 13, 31, &["e2e4"]);
    r.score = Some((Score::Cp(31), Bound::Lower));
    r.wdl = Some(Wdl {
        win: 450,
        draw: 300,
        loss: 250,
    });
    info(&mut state, run, r);
    let l = &view(&state, run).lines[0];
    assert_eq!(l.bound, Bound::Lower);
    assert_eq!(
        l.wdl,
        Some(Wdl {
            win: 450,
            draw: 300,
            loss: 250
        })
    );
    assert_eq!(
        view(&state, run).main_score(),
        Some((Score::Cp(31), Bound::Lower))
    );
}

#[test]
fn the_best_move_of_the_current_search_finishes_it_and_an_old_one_does_not() {
    let (mut state, run, start) = three();
    let best = |s: u64| BestMove {
        search: SearchId(s),
        mv: Some(start.parse_uci("e2e4").unwrap()),
        ponder: None,
    };
    event(&mut state, run, EngineEvent::BestMove(best(9)));
    assert!(!view(&state, run).finished);
    event(&mut state, run, EngineEvent::BestMove(best(1)));
    assert!(view(&state, run).finished);
}

// ---- D796: stale reports and the window around a position change ----

#[test]
fn a_report_that_crossed_a_position_change_is_dropped() {
    let (mut state, run, start) = three();
    // 1.e4: a new search is sent; the engine confirms it as search 2.
    play(&mut state, "e2e4");
    started(&mut state, run, 2);
    assert!(
        lines_of(&state, run).is_empty(),
        "new position, lines forgotten"
    );
    // A late report of the old position (its first move d4 is not legal for
    // Black) tagged with the new search id.
    info(&mut state, run, report(&start, 2, 1, 30, 10, &["d2d4"]));
    assert!(
        lines_of(&state, run).is_empty(),
        "illegal first move must be dropped"
    );
    // A report that is legal in the new position is accepted and numbered
    // from it ("1... e5").
    let after = state.session.position().clone();
    info(&mut state, run, report(&after, 2, 1, 5, -20, &["e7e5"]));
    assert_eq!(lines_of(&state, run), [(1, "1... e5".into())]);
}

#[test]
fn between_a_new_search_and_its_start_the_old_search_still_feeds_the_old_position() {
    let (mut state, run, start) = three();
    play(&mut state, "e2e4");
    // No SearchStarted(2) yet: a report of search 1 is still about the start
    // position and must not corrupt anything.
    info(&mut state, run, report(&start, 1, 1, 13, 35, &["e2e4"]));
    started(&mut state, run, 2);
    assert!(lines_of(&state, run).is_empty());
    assert_eq!(view(&state, run).search, Some(SearchId(2)));
    // After the new search began, search 1 reports are dropped.
    info(&mut state, run, report(&start, 1, 1, 14, 36, &["e2e4"]));
    assert!(lines_of(&state, run).is_empty());
}

#[test]
fn the_same_position_keeps_its_lines_when_a_search_restarts() {
    let (mut state, run, _) = three();
    started(&mut state, run, 2);
    assert_eq!(
        lines_of(&state, run).len(),
        3,
        "lines of the same position stay"
    );
}

// ---- history (rule 3) ----

fn single_line() -> (mychess_app::AppState, mychess_app::engines::RunId, Position) {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    (state, run, Position::new())
}

#[test]
fn history_grows_only_when_the_pv_differs() {
    let (mut state, run, start) = single_line();
    info(
        &mut state,
        run,
        report(&start, 1, 1, 10, 20, &["e2e4", "e7e5"]),
    );
    info(
        &mut state,
        run,
        report(&start, 1, 1, 11, 21, &["e2e4", "e7e5"]),
    );
    info(
        &mut state,
        run,
        report(&start, 1, 1, 12, 25, &["e2e4", "c7c5"]),
    );
    let h = &view(&state, run).history;
    assert_eq!(h.len(), 2, "same PV must not add a line: {h:?}");
    assert_eq!(h[0].depth, 10);
    assert_eq!(h[1].depth, 12);
    assert_eq!(h[1].text, "1.e4 c5");
}

#[test]
fn history_restarts_when_the_depth_decreases() {
    let (mut state, run, start) = single_line();
    info(&mut state, run, report(&start, 1, 1, 10, 20, &["e2e4"]));
    info(&mut state, run, report(&start, 1, 1, 12, 20, &["d2d4"]));
    started(&mut state, run, 2);
    info(&mut state, run, report(&start, 2, 1, 2, 5, &["g1f3"]));
    let h = &view(&state, run).history;
    assert_eq!(h.len(), 1, "history must start again: {h:?}");
    assert_eq!(h[0].depth, 2);
}

#[test]
fn history_is_capped_at_the_maximum() {
    let (mut state, run, start) = single_line();
    let moves = ["e2e4", "d2d4", "g1f3", "c2c4"];
    for i in 0..(MAX_HISTORY + 50) {
        let depth = u32::try_from(i + 1).unwrap();
        info(
            &mut state,
            run,
            report(&start, 1, 1, depth, 1, &[moves[i % 4]]),
        );
    }
    assert_eq!(view(&state, run).history.len(), MAX_HISTORY);
    assert_eq!(MAX_HISTORY, 200);
}

#[test]
fn no_history_with_multiple_lines_and_changing_to_one_line_clears_it() {
    let (mut state, run, start) = single_line();
    info(&mut state, run, report(&start, 1, 1, 10, 20, &["e2e4"]));
    assert_eq!(view(&state, run).history.len(), 1);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    info(&mut state, run, report(&start, 1, 1, 11, 20, &["d2d4"]));
    assert_eq!(
        view(&state, run).history.len(),
        1,
        "no growth with MultiPV 3"
    );
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 1));
    assert!(view(&state, run).history.is_empty());
}

// ---- MultiPV change (rule 2) ----

#[test]
fn lowering_the_multipv_cuts_the_list_and_sends_set_multipv() {
    let (mut state, run, _) = three();
    let fx = msg(&mut state, AnalysisMessage::SetMultiPv(run, 2));
    assert_eq!(lines_of(&state, run).len(), 2);
    assert!(commands(&fx).contains(&mychess_engine::EngineCommand::SetMultiPv(2)));
    assert_eq!(view(&state, run).multipv, 2);
}

#[test]
fn multipv_is_clamped_to_the_capabilities_and_saved_to_the_engine_entry() {
    let (mut state, run, _) = analysing(true, true);
    let fx = msg(&mut state, AnalysisMessage::SetMultiPv(run, 200));
    assert!(commands(&fx).contains(&mychess_engine::EngineCommand::SetMultiPv(5)));
    assert_eq!(view(&state, run).multipv, 5);
    msg(&mut state, AnalysisMessage::SetMultiPv(run, 0));
    assert_eq!(view(&state, run).multipv, 1);
    let engine = state
        .engine_runs()
        .iter()
        .find(|r| r.id == run)
        .unwrap()
        .engine;
    assert_eq!(
        state.engines.get(engine).unwrap().multipv,
        1,
        "D426: value goes back"
    );
}

#[test]
fn an_engine_without_multipv_refuses_the_count_with_a_notice() {
    let (mut state, run, _) = analysing(false, true);
    let fx = msg(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    assert!(commands(&fx).is_empty());
    assert_eq!(view(&state, run).multipv, 1);
    assert_eq!(
        analysis_notice(&state),
        Some(&mychess_app::AnalysisNotice::MultiPvUnsupported)
    );
}

// ---- notation options ----

#[test]
fn the_pv_is_cut_at_max_ply_except_when_it_leads_to_mate() {
    let (mut state, run, start) = single_line();
    state.prefs.analysis.max_ply = 2;
    let pv = ["e2e4", "e7e5", "g1f3", "b8c6"];
    info(&mut state, run, report(&start, 1, 1, 10, 20, &pv));
    assert_eq!(lines_of(&state, run)[0].1, "1.e4 e5", "cut at 2 plies");
    // Full PV is still stored for adding to the game.
    assert_eq!(view(&state, run).lines[0].pv.len(), 4);
    let mut mate = report(&start, 1, 1, 11, 0, &pv);
    mate.score = Some((Score::Mate(4), Bound::Exact));
    info(&mut state, run, mate);
    assert_eq!(
        lines_of(&state, run)[0].1,
        "1.e4 e5 2.Nf3 Nc6",
        "mate lines are not cut"
    );
}

#[test]
fn the_move_number_space_preference_reformats_the_stored_lines() {
    let (mut state, run, start) = single_line();
    info(
        &mut state,
        run,
        report(&start, 1, 1, 10, 20, &["e2e4", "e7e5"]),
    );
    assert_eq!(lines_of(&state, run)[0].1, "1.e4 e5");
    state.prefs.notation.move_number_space = true;
    nav(&mut state, Nav::Start);
    assert_eq!(lines_of(&state, run)[0].1, "1. e4 e5");
}

use mychess_app::Nav;

// ---- positions without moves (D450, D798) ----

fn load_fen(state: &mut mychess_app::AppState, fen: &str) -> Vec<mychess_app::Effect> {
    state
        .session
        .replace_game(Game::from_position(Position::from_fen(fen).unwrap()));
    nav(state, Nav::Start)
}

#[test]
fn a_checkmate_position_is_not_searched_and_scores_327() {
    let (mut state, run, _) = analysing(true, true);
    // Mate by White (Qg7#), Black to move: White gives mate, so "+".
    let fx = load_fen(&mut state, "7k/6Q1/6K1/8/8/8/8/8 b - - 0 1");
    assert!(searches(&fx).is_empty(), "no search without legal moves");
    let v = view(&state, run);
    assert_eq!(v.terminal, Some(Terminal::Checkmate));
    assert_eq!(v.main_score(), Some((Score::Cp(32_700), Bound::Exact)));
}

#[test]
fn a_checkmate_by_black_scores_minus_327() {
    let (mut state, run, _) = analysing(true, true);
    for u in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        play(&mut state, u);
    }
    let v = view(&state, run);
    assert_eq!(v.terminal, Some(Terminal::Checkmate));
    assert_eq!(v.main_score(), Some((Score::Cp(-32_700), Bound::Exact)));
}

#[test]
fn a_stalemate_position_is_not_searched_and_scores_zero() {
    let (mut state, run, _) = analysing(true, true);
    let fx = load_fen(&mut state, "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert!(searches(&fx).is_empty());
    let v = view(&state, run);
    assert_eq!(v.terminal, Some(Terminal::Stalemate));
    assert_eq!(v.main_score(), Some((Score::Cp(0), Bound::Exact)));
}

#[test]
fn reaching_a_mate_while_searching_stops_the_engine_and_leaving_it_searches_again() {
    let (mut state, run, _) = analysing(true, true);
    started(&mut state, run, 1);
    for u in ["f2f3", "e7e5", "g2g4"] {
        play(&mut state, u);
    }
    started(&mut state, run, 2);
    let mv = state.session.position().parse_uci("d8h4").unwrap();
    let fx = game(
        &mut state,
        mychess_app::GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        },
    );
    assert!(searches(&fx).is_empty());
    assert_eq!(count_stops(&fx), 1, "the running search must be stopped");
    let fx = nav(&mut state, Nav::Back(1));
    assert_eq!(
        searches(&fx).len(),
        1,
        "a normal position is searched again"
    );
    assert_eq!(view(&state, run).terminal, None);
}

// ---- hostile input ----

#[test]
fn hostile_reports_do_not_panic() {
    let (mut state, run, start) = three();
    let mut r = report(&start, 1, 0, u32::MAX, i32::MIN, &["e2e4"]);
    r.nodes = Some(u64::MAX);
    r.time_ms = Some(u64::MAX);
    r.nps = Some(u64::MAX);
    r.hashfull_permille = Some(u16::MAX);
    r.cpuload_permille = Some(u16::MAX);
    r.seldepth = Some(u32::MAX);
    r.currmovenumber = Some(u32::MAX);
    r.wdl = Some(Wdl {
        win: u16::MAX,
        draw: u16::MAX,
        loss: u16::MAX,
    });
    info(&mut state, run, r);
    let mut r = report(&start, 1, 255, 1, 0, &["e2e4"]);
    r.score = Some((Score::Mate(i32::MIN), Bound::Upper));
    info(&mut state, run, r);
    let mut r = report(&start, 1, 1, 1, 0, &["e2e4"]);
    r.score = Some((Score::Mate(i32::MAX), Bound::Lower));
    r.pv_truncated = true;
    info(&mut state, run, r);
    let _ = view(&state, run).main_score();
    // Extra info on every toggle of the display still works.
    msg(&mut state, AnalysisMessage::ToggleInfo(run));
}

proptest! {
    #[test]
    fn lines_stay_sorted_unique_and_within_the_count(
        count in 1u8..=5,
        reports in proptest::collection::vec((0u8..8, 0u32..40, any::<bool>(), 0usize..4, 0u64..4), 0..60),
    ) {
        let (mut state, run, _) = analysing(true, true);
        msg(&mut state, AnalysisMessage::SetMultiPv(run, count));
        let start = Position::new();
        started(&mut state, run, 1);
        let moves = ["e2e4", "d2d4", "g1f3", "c2c4"];
        for (n, depth, with_pv, mv, search) in reports {
            let mut r = report(&start, search, n, depth, 5, &[moves[mv]]);
            if !with_pv {
                r.pv.clear();
            }
            info(&mut state, run, r);
            let v = view(&state, run);
            prop_assert!(v.lines.len() <= usize::from(count));
            prop_assert!(v.lines.windows(2).all(|w| w[0].multipv < w[1].multipv));
            prop_assert!(v.lines.iter().all(|l| l.multipv >= 1 && l.multipv <= count));
            prop_assert!(v.history.len() <= MAX_HISTORY);
        }
    }
}
