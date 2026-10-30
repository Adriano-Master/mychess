//! Regression tests of the review of the sources and of the search for
//! mates (spec M8-4, D1983-D1999): one key for a database whatever file of
//! it was chosen, a search that stops when asked to, a file that changed,
//! and the budget, the precedence and the hints of the search for mates
//! checked one by one.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m3_4a_common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use m3_4a_common::*;
use mychess_app::tactics::{
    CLASSIFY_PER_REQUEST, Cancel, FindQuery, SourceKind, TacticsConfig, TacticsEffect,
    TacticsMessage, TacticsNotice, TacticsPhase, find_next, open_file, problem_id,
};

const MATE1_FEN: &str = "8/7R/3k4/8/3KQ3/8/8/7q w - - 0 1";
const MATE2_FEN: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";
const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn fen_only(event: &str, fen: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n[Result \"*\"]\n\n*\n\n")
}

/// The game of the report: Black mates with 14...Qxh2# (ply 28); the
/// position before it is ply 27.
fn real_game(comment: &str) -> String {
    format!(
        "[Event \"UCI Game\"]\n[Result \"0-1\"]\n\n\
         1. e4 e5 2. Nf3 Nc6 3. Bc4 Nf6 4. Nc3 Nxe4 5. Nxe4 d5 6. Bd3 dxe4 7. Bxe4 Bd6 \
         8. O-O O-O 9. b3 Nb4 10. Bb2 f5 11. Nxe5 fxe4 12. Qh5 Rf5 13. Qh4 Qxh4 \
         14. Nc4{comment} Qxh2# 0-1\n\n"
    )
}

const SCHOLAR: &str = "1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6";

fn query(config: TacticsConfig, max_games: usize, k: usize, solved: Vec<u64>) -> FindQuery {
    FindQuery {
        config: Box::new(config),
        start: 0,
        step: 1,
        k,
        max_games,
        solved: Arc::new(solved),
        stamp: None,
        cancel: Cancel::new(),
    }
}

fn mate_config() -> TacticsConfig {
    TacticsConfig::default()
}

fn handle_of_pgn(dir: &Path, text: &str) -> mychess_app::tactics::SourceHandle {
    open_file(&write(dir, "g.pgn", text)).expect("opens").handle
}

fn start_file(rig: &mut Rig, path: &Path) {
    let config = TacticsConfig {
        file: Some(path.to_path_buf()),
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
}

// ---- the key of a source (D1983) ----

#[test]
fn resetting_the_score_clears_a_database_chosen_by_any_of_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(
        dir.path(),
        "bases",
        &(fen_only("one", MATE1_FEN) + &fen_only("two", MATE2_FEN)),
    );
    let sg4 = si4.with_extension("sg4");
    let mut rig = Rig::new();
    start_file(&mut rig, &sg4);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    let mv = rig
        .state
        .session
        .position()
        .find_move("e4".parse().unwrap(), "g6".parse().unwrap(), None)
        .unwrap();
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(rig.state.tactics_view().solved_in_source, 1);
    // The reset names the database by another file of it.
    let config = TacticsConfig {
        file: Some(si4.clone()),
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::ResetStats(Box::new(
        config,
    ))));
    assert_eq!(rig.state.tactics_view().solved_in_source, 0);
}

#[test]
fn the_current_base_and_the_file_share_the_score_of_a_database() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "bases", &fen_only("one", MATE1_FEN));
    let mut rig = Rig::new();
    start_file(&mut rig, &si4);
    let mv = rig
        .state
        .session
        .position()
        .find_move("e4".parse().unwrap(), "g6".parse().unwrap(), None)
        .unwrap();
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    rig.send(Message::Tactics(TacticsMessage::Close));
    // The same database as the current base: its only problem is solved.
    rig.open(&si4);
    let config = TacticsConfig {
        source: SourceKind::CurrentBase,
        ..TacticsConfig::default()
    };
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Exhausted);
}

// ---- stopping the search (D1984) ----

fn pending_find(rig: &mut Rig, path: &Path) -> Cancel {
    let effects = rig.raw(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            file: Some(path.to_path_buf()),
            ..TacticsConfig::default()
        }),
        seed: 1,
    }));
    // Answer the opening, keep the search.
    let mut find = None;
    for effect in effects {
        if let Effect::Tactics(t @ TacticsEffect::OpenFile { .. }) = effect {
            let answer = rig.perform(Effect::Tactics(t));
            for next in rig.raw(answer) {
                if let Effect::Tactics(TacticsEffect::Find { query, .. }) = next {
                    find = Some(query.cancel);
                }
            }
        }
    }
    find.expect("the search was asked for")
}

#[test]
fn closing_the_session_stops_the_search_that_is_running() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "p.pgn", &fen_only("one", MATE1_FEN));
    let mut rig = Rig::new();
    let cancel = pending_find(&mut rig, &file);
    assert!(!cancel.is_cancelled());
    rig.raw(Message::Tactics(TacticsMessage::Close));
    assert!(cancel.is_cancelled(), "the thread is told to stop");
}

#[test]
fn the_watchdog_stops_the_search_it_gives_up_on() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "p.pgn", &fen_only("one", MATE1_FEN));
    let mut rig = Rig::new();
    let cancel = pending_find(&mut rig, &file);
    let t0 = Instant::now();
    for i in 0..40u64 {
        rig.raw(Message::Tactics(TacticsMessage::Tick(
            t0 + Duration::from_secs(i * 2),
        )));
    }
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert!(cancel.is_cancelled());
}

#[test]
fn a_search_that_was_told_to_stop_looks_at_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let handle = handle_of_pgn(dir.path(), &(real_game("") + &real_game("")));
    let q = query(mate_config(), 10, 0, vec![]);
    q.cancel.cancel();
    let out = find_next(&handle, &q);
    assert!(out.found.is_none());
    assert_eq!(out.examined, 0);
}

// ---- a file that changed (D1987) ----

#[test]
fn a_database_file_that_changed_ends_the_session_at_the_next_search() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(
        dir.path(),
        "bases",
        &(fen_only("one", MATE1_FEN) + &fen_only("two", MATE2_FEN)),
    );
    let mut rig = Rig::new();
    start_file(&mut rig, &si4);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    // Something else writes the database (the same base open in a slot).
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&si4).unwrap();
        file.write_all(&[0]).unwrap();
    }
    rig.send(Message::Tactics(TacticsMessage::Next));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::SourceChanged)
    );
    assert!(rig.state.tactics_dialog_open());
    assert_ne!(rig.state.session.game().tags().event, "Tactics");
}

#[test]
fn a_database_file_that_did_not_change_keeps_the_session_going() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(
        dir.path(),
        "bases",
        &(fen_only("one", MATE1_FEN) + &fen_only("two", MATE2_FEN)),
    );
    let mut rig = Rig::new();
    start_file(&mut rig, &si4);
    rig.send(Message::Tactics(TacticsMessage::Next));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
}

// ---- why a source did not open (D1988) ----

#[test]
fn an_old_scid_database_says_it_must_be_upgraded() {
    let dir = tempfile::tempdir().unwrap();
    let si3 = write(dir.path(), "old.si3", "x");
    let mut rig = Rig::new();
    start_file(&mut rig, &si3);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::SourceNeedsUpgrade)
    );
    assert!(rig.state.tactics_dialog_open());
}

#[test]
fn a_database_that_is_not_one_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let broken = write(dir.path(), "broken.si4", "this is not a database at all");
    let mut rig = Rig::new();
    start_file(&mut rig, &broken);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    let notice = rig.state.tactics_view().notice;
    assert!(
        matches!(
            notice,
            Some(TacticsNotice::SourceCorrupt | TacticsNotice::SourceUnreadable)
        ),
        "{notice:?}"
    );
}

// ---- the budget of the search for mates (D1978, D1985) ----

/// Ids of the four candidate positions of the real game, for `n` games.
fn solved_ids(games: u32) -> Vec<u64> {
    (1..=games)
        .flat_map(|g| [21, 23, 25, 27].map(|ply| problem_id(g, ply)))
        .collect()
}

fn many_games(dir: &Path, n: usize) -> mychess_app::tactics::SourceHandle {
    handle_of_pgn(dir, &(0..n).map(|_| real_game("")).collect::<String>())
}

#[test]
fn a_base_of_many_games_is_walked_in_requests_without_skipping_or_repeating_a_game() {
    let dir = tempfile::tempdir().unwrap();
    let handle = many_games(dir.path(), 40);
    let config = TacticsConfig {
        mate_max: 4,
        ..mate_config()
    };
    let mut k = 0;
    let mut calls = 0;
    let mut deferrals = 0;
    while k < 40 {
        let out = find_next(&handle, &query(config.clone(), 40 - k, k, solved_ids(40)));
        assert!(out.found.is_none(), "everything was solved");
        assert!(out.examined >= 1, "a call always makes progress");
        assert_eq!(out.next_k, k + out.examined, "the walk is contiguous");
        assert!(out.classified <= CLASSIFY_PER_REQUEST);
        if k + out.examined < 40 {
            // The call ended by budget, with the next game deferred.
            assert!(out.classified + 4 > CLASSIFY_PER_REQUEST, "{out:?}");
            deferrals += 1;
        }
        k = out.next_k;
        calls += 1;
        assert!(calls < 40, "the walk does not advance");
    }
    assert_eq!(k, 40);
    assert!(deferrals >= 1, "the fixture must exercise the deferral");
}

#[test]
fn the_game_that_was_deferred_is_the_first_one_of_the_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let handle = many_games(dir.path(), 40);
    let config = TacticsConfig {
        mate_max: 4,
        ..mate_config()
    };
    let first = find_next(&handle, &query(config.clone(), 40, 0, solved_ids(40)));
    let deferred = first.next_k;
    assert!(deferred > 0 && deferred < 40, "{first:?}");
    // Leave the deferred game (number `deferred + 1`) unsolved.
    let mut solved = solved_ids(40);
    solved.retain(|id| id >> 16 != u64::try_from(deferred + 1).unwrap());
    let again = find_next(&handle, &query(config, 40, deferred, solved));
    let found = again.found.expect("the deferred game is searched now");
    assert_eq!(
        usize::try_from(found.problem.game_number).unwrap(),
        deferred + 1
    );
}

// ---- the precedence of the markers and the hints (D1975-D1977) ----

#[test]
fn a_marker_wins_over_the_mate_at_the_end_of_the_same_game_in_mate_mode() {
    let dir = tempfile::tempdir().unwrap();
    let text = real_game("").replace("13. Qh4 Qxh4", "13. Qh4 {****D3} Qxh4");
    let handle = handle_of_pgn(dir.path(), &text);
    let out = find_next(&handle, &query(mate_config(), 10, 0, vec![]));
    // The marker is the problem of the game: no mate in three there, and the
    // mate in one at the end is not offered besides.
    assert!(
        out.found.is_none(),
        "{:?}",
        out.found.map(|f| f.problem.ply)
    );
}

#[test]
fn an_evaluation_of_a_mate_for_white_points_at_the_position_with_white_to_move() {
    let dir = tempfile::tempdir().unwrap();
    let handle = handle_of_pgn(
        dir.path(),
        &format!("[Event \"s\"]\n\n{SCHOLAR} {{[%eval #1]}} *\n"),
    );
    let out = find_next(&handle, &query(mate_config(), 10, 0, vec![]));
    let found = out.found.expect("Qxf7# is there");
    assert_eq!(found.problem.ply, 6);
    assert_eq!(found.mate_moves, Some(1));
    assert_eq!(
        found.line.len(),
        1,
        "no move of the game: the solver's line"
    );
}

#[test]
fn an_evaluation_of_a_mate_for_the_other_side_points_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let handle = handle_of_pgn(
        dir.path(),
        &format!("[Event \"s\"]\n\n{SCHOLAR} {{[%eval #-1]}} *\n"),
    );
    let out = find_next(&handle, &query(mate_config(), 10, 0, vec![]));
    assert!(out.found.is_none());
    assert_eq!(out.classified, 0, "the solver did not even run");
}

#[test]
fn a_game_with_a_standard_set_up_position_is_searched_like_any_other() {
    let dir = tempfile::tempdir().unwrap();
    let text =
        format!("[Event \"s\"]\n[SetUp \"1\"]\n[FEN \"{START_FEN}\"]\n\n{SCHOLAR} 4. Qxf7# 1-0\n");
    let handle = handle_of_pgn(dir.path(), &text);
    let found = find_next(&handle, &query(mate_config(), 10, 0, vec![]))
        .found
        .expect("the mate at the end");
    assert_eq!(found.problem.ply, 6);
}

#[test]
fn a_game_with_its_own_position_and_no_marker_is_the_problem_of_its_start() {
    for scan_mates in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let text = format!(
            "[Event \"f\"]\n[SetUp \"1\"]\n[FEN \"{MATE2_FEN}\"]\n\n1. Ra6 bxa6 2. b7# 1-0\n"
        );
        let handle = handle_of_pgn(dir.path(), &text);
        let config = TacticsConfig {
            scan_mates,
            ..mate_config()
        };
        let found = find_next(&handle, &query(config, 10, 0, vec![]))
            .found
            .expect("the start is the problem");
        assert_eq!(found.problem.ply, 0, "scan_mates = {scan_mates}");
        assert_eq!(found.mate_moves, Some(2));
    }
}

#[test]
fn without_the_search_a_marker_in_a_si4_is_still_a_problem() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "marked", &real_game("{****D3}"));
    let opened = open_file(&si4).expect("opens");
    let config = TacticsConfig {
        scan_mates: false,
        ..mate_config()
    };
    let found = find_next(&opened.handle, &query(config, 10, 0, vec![]))
        .found
        .expect("the marker is a declared problem");
    assert_eq!(found.problem.ply, 27);
    assert_eq!(found.mate_moves, Some(1));
}

#[test]
fn without_the_search_a_plain_game_in_a_si4_gives_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "plain", &real_game("{[%eval #-1]}"));
    let opened = open_file(&si4).expect("opens");
    let config = TacticsConfig {
        scan_mates: false,
        ..mate_config()
    };
    assert!(
        find_next(&opened.handle, &query(config, 10, 0, vec![]))
            .found
            .is_none()
    );
}

// ---- the cost, in release (D1985) ----

/// `cargo test --release -p mychess-app --test m8_4_scan_fixes -- --ignored --nocapture`
#[test]
#[ignore = "a measurement: run it in release"]
fn measure_the_search_for_mates_in_release() {
    let dir = tempfile::tempdir().unwrap();
    let handle = many_games(dir.path(), 300);
    let config = TacticsConfig {
        mate_max: 5,
        include_solved: true,
        ..mate_config()
    };
    // The worst case of a request: every game spends its candidates.
    let started = Instant::now();
    let out = find_next(&handle, &query(config.clone(), 300, 0, vec![]));
    println!(
        "one request: {:?}, {} games, {} positions, found {}",
        started.elapsed(),
        out.examined,
        out.classified,
        out.found.is_some()
    );
    // A whole lap without any problem accepted.
    let solved: Vec<u64> = (1..=300u32)
        .flat_map(|g| (1..=28).map(move |ply| problem_id(g, ply)))
        .collect();
    let config = TacticsConfig {
        include_solved: false,
        ..config
    };
    let started = Instant::now();
    let mut k = 0;
    let mut requests = 0;
    while k < 300 {
        let out = find_next(&handle, &query(config.clone(), 300, k, solved.clone()));
        k = out.next_k;
        requests += 1;
    }
    println!(
        "a lap of 300 games: {:?} in {requests} requests",
        started.elapsed()
    );
}
