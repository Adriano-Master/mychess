//! Regression tests of the sources and of the search of the tactics
//! training on Scid databases (spec M8-4, D1970-D1982): a `.si4` as the
//! file of the problems, the tactics bases of the Scid (games that only
//! bring a FEN), and the mates found in the main line of ordinary games
//! without markers. The databases are generated here from PGN text.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m3_4a_common;

use m3_4a_common::*;
use mychess_app::tactics::{
    SourceKind, TacticsConfig, TacticsMessage, TacticsMode, TacticsNotice, TacticsPhase, find_next,
    open_file,
};
use mychess_core::{Color, Position};

/// A mate in one for White, in the style of the "matein1" base of the
/// Scid: a FEN, no moves, no result.
const MATE1_FEN: &str = "8/7R/3k4/8/3KQ3/8/8/7q w - - 0 1";
const MATE2_FEN: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";

fn fen_only(event: &str, fen: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n[Result \"*\"]\n\n*\n\n")
}

fn fen_with(event: &str, fen: &str, moves: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n[Result \"*\"]\n\n{moves} *\n\n")
}

/// The game of the report: Black mates with 14...Qxh2#; the engine saw it
/// as `[%eval #-1]` after 14. Nc4.
fn real_game(with_eval: bool) -> String {
    let mark = if with_eval { " {[%eval #-1]}" } else { "" };
    format!(
        "[Event \"UCI Game\"]\n[White \"?\"]\n[Black \"Stockfish 19\"]\n[Result \"0-1\"]\n\n\
         1. e4 e5 2. Nf3 Nc6 3. Bc4 Nf6 4. Nc3 Nxe4 5. Nxe4 d5 6. Bd3 dxe4 7. Bxe4 Bd6 \
         8. O-O O-O 9. b3 Nb4 10. Bb2 f5 11. Nxe5 fxe4 12. Qh5 Rf5 13. Qh4 Qxh4 \
         14. Nc4{mark} Qxh2# 0-1\n\n"
    )
}

/// A quiet game: nothing to mate.
fn quiet_game() -> String {
    "[Event \"quiet\"]\n[Result \"1/2-1/2\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 1/2-1/2\n\n"
        .to_owned()
}

fn config_for(source: SourceKind, file: Option<&std::path::Path>) -> TacticsConfig {
    TacticsConfig {
        source,
        file: file.map(std::path::Path::to_path_buf),
        ..TacticsConfig::default()
    }
}

fn start(rig: &mut Rig, config: TacticsConfig) {
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
}

fn view(rig: &Rig) -> mychess_app::tactics::TacticsView {
    rig.state.tactics_view()
}

// ---- (1) a Scid database as the file of the problems ----

#[test]
fn a_si4_file_is_a_source_that_takes_no_slot() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(
        dir.path(),
        "bases",
        &(fen_only("one", MATE1_FEN) + &fen_only("two", MATE2_FEN)),
    );
    let mut rig = Rig::new();
    start(&mut rig, config_for(SourceKind::File, Some(&si4)));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(view(&rig).mate_in, Some(1));
    assert_eq!(
        *rig.state.session.position(),
        Position::from_fen(MATE1_FEN).unwrap()
    );
    assert_eq!(rig.open_count(), 1, "only the Clipbase: no slot was taken");
    rig.send(Message::Tactics(TacticsMessage::Close));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
}

#[test]
fn the_other_files_of_a_database_and_upper_case_names_open_it_too() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "bases", &fen_only("one", MATE1_FEN));
    for ext in ["sg4", "sn4"] {
        let opened = open_file(&si4.with_extension(ext)).expect("opens");
        assert_eq!(opened.handle.len(), 1);
    }
    let broken = dir.path().join("nothing.si4");
    std::fs::write(&broken, b"not a database").unwrap();
    assert!(open_file(&broken).is_err());
    assert!(open_file(&dir.path().join("missing.si4")).is_err());
}

#[test]
fn a_problem_of_a_si4_file_is_solved_and_remembered_by_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(
        dir.path(),
        "bases",
        &(fen_only("one", MATE1_FEN) + &fen_only("two", MATE2_FEN)),
    );
    let mut rig = Rig::new();
    start(&mut rig, config_for(SourceKind::File, Some(&si4)));
    rig.send(GameMessage::PlayMove {
        mv: rig
            .state
            .session
            .position()
            .find_move("e4".parse().unwrap(), "g6".parse().unwrap(), None)
            .unwrap(),
        options: PlayOptions::default(),
    });
    assert!(matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)));
    rig.send(Message::Tactics(TacticsMessage::Next));
    assert_eq!(view(&rig).mate_in, Some(2), "the solved one is skipped");
}

// ---- (2) the tactics bases of the Scid ----

#[test]
fn games_that_only_bring_a_fen_are_problems_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(
        dir.path(),
        "matein",
        &(fen_only("matein1", MATE1_FEN) + &fen_only("matein2", MATE2_FEN)),
    );
    for mode in [TacticsMode::Mate, TacticsMode::BestMove] {
        let mut rig = Rig::new();
        rig.open(&si4);
        let mut config = config_for(SourceKind::CurrentBase, None);
        config.mode = mode;
        start(&mut rig, config);
        assert_eq!(
            rig.state.tactics_phase(),
            TacticsPhase::Solving,
            "{mode:?} on the current base"
        );
        assert_eq!(rig.state.session.position().turn(), Color::White);
    }
}

#[test]
fn best_move_gets_the_line_of_the_solver_when_the_game_has_no_moves() {
    // The bases "matein2" and "matein3" of the Scid carry no solution.
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "matein2", &fen_only("matein2", MATE2_FEN));
    let mut rig = Rig::new();
    let mut config = config_for(SourceKind::File, Some(&si4));
    config.mode = TacticsMode::BestMove;
    start(&mut rig, config);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    // Ra6 is the key; the reply and the mate follow by themselves.
    let key = rig
        .state
        .session
        .position()
        .find_move("a1".parse().unwrap(), "a6".parse().unwrap(), None)
        .unwrap();
    rig.send(GameMessage::PlayMove {
        mv: key,
        options: PlayOptions::default(),
    });
    assert!(
        matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)),
        "{:?}",
        rig.state.tactics_phase()
    );
}

#[test]
fn a_fen_game_with_a_short_solution_is_recognised_by_the_solver_in_mate_mode() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "short", &fen_with("short", MATE2_FEN, "1. Ra6"));
    let mut rig = Rig::new();
    start(&mut rig, config_for(SourceKind::File, Some(&si4)));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(
        view(&rig).mate_in,
        Some(2),
        "the solver, not the game, says N"
    );
}

// ---- (3) mates in the main line of ordinary games ----

#[test]
fn the_mate_of_a_game_with_an_evaluation_is_offered_before_the_last_move() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "game", &real_game(true));
    let mut rig = Rig::new();
    rig.open(&si4);
    start(&mut rig, config_for(SourceKind::CurrentBase, None));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(view(&rig).mate_in, Some(1));
    assert_eq!(rig.state.session.position().turn(), Color::Black);
    assert_eq!(rig.state.session.position().fullmove_number(), 14);
    // The game has 27 plies up to this position.
    let mv = rig
        .state
        .session
        .position()
        .find_move("h4".parse().unwrap(), "h2".parse().unwrap(), None)
        .expect("Qxh2 is legal");
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(
        matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)),
        "{:?}",
        rig.state.tactics_phase()
    );
}

#[test]
fn a_game_that_ends_in_mate_is_offered_even_without_any_evaluation() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "game", &real_game(false));
    let mut rig = Rig::new();
    rig.open(&si4);
    start(&mut rig, config_for(SourceKind::CurrentBase, None));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(view(&rig).mate_in, Some(1));
}

#[test]
fn a_pgn_file_gets_the_same_search() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "games.pgn", &(quiet_game() + &real_game(true)));
    let mut rig = Rig::new();
    start(&mut rig, config_for(SourceKind::File, Some(&pgn)));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(view(&rig).mate_in, Some(1));
    assert_eq!(view(&rig).title, "UCI Game");
}

#[test]
fn the_option_off_leaves_the_games_without_markers_alone() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "game", &real_game(true));
    let mut rig = Rig::new();
    rig.open(&si4);
    let mut config = config_for(SourceKind::CurrentBase, None);
    config.scan_mates = false;
    start(&mut rig, config);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Exhausted);
}

#[test]
fn a_game_without_a_mate_gives_no_problem() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "quiet", &quiet_game());
    let mut rig = Rig::new();
    rig.open(&si4);
    start(&mut rig, config_for(SourceKind::CurrentBase, None));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Exhausted);
}

#[test]
fn the_mate_range_is_respected_by_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "game", &real_game(true));
    let mut rig = Rig::new();
    rig.open(&si4);
    let mut config = config_for(SourceKind::CurrentBase, None);
    config.mate_min = 2;
    config.mate_max = 3;
    start(&mut rig, config);
    assert_eq!(
        rig.state.tactics_phase(),
        TacticsPhase::Exhausted,
        "the only mate of the game is in one move"
    );
}

#[test]
fn a_mate_that_was_solved_is_not_offered_again_with_the_real_solver() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "game", &real_game(true));
    let mut rig = Rig::new();
    rig.open(&si4);
    start(&mut rig, config_for(SourceKind::CurrentBase, None));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    let mv = rig
        .state
        .session
        .position()
        .find_move("h4".parse().unwrap(), "h2".parse().unwrap(), None)
        .unwrap();
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)));
    // Solved: the game has no other position that is not inside the line.
    rig.send(Message::Tactics(TacticsMessage::Next));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Exhausted);
    assert_ne!(view(&rig).notice, Some(TacticsNotice::Busy));
}

#[test]
fn in_best_move_mode_the_marker_is_the_problem_of_its_game() {
    // A game with a marker is the problem of its marker; the mate at its
    // end is not offered besides (the criteria of `****D` do not change).
    let dir = tempfile::tempdir().unwrap();
    let game = real_game(false).replace("13. Qh4 Qxh4", "13. Qh4 {****D3} Qxh4");
    let si4 = make_si4(dir.path(), "marked", &game);
    let mut rig = Rig::new();
    rig.open(&si4);
    let mut config = config_for(SourceKind::CurrentBase, None);
    config.mode = TacticsMode::BestMove;
    start(&mut rig, config);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.session.position().fullmove_number(), 13);
}

#[test]
fn one_request_never_classifies_more_than_the_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    // Many games that end in mate: each has up to three candidates.
    let games: String = (0..40).map(|_| real_game(false)).collect();
    let si4 = make_si4(dir.path(), "many", &games);
    let opened = open_file(&si4).expect("opens");
    let query = mychess_app::tactics::FindQuery {
        config: Box::new(TacticsConfig {
            include_solved: true,
            ..TacticsConfig::default()
        }),
        start: 0,
        step: 1,
        k: 0,
        max_games: 40,
        solved: std::sync::Arc::new(Vec::new()),
        stamp: None,
        cancel: mychess_app::tactics::Cancel::new(),
    };
    let outcome = find_next(&opened.handle, &query);
    assert!(outcome.found.is_some());
    assert!(outcome.classified <= mychess_app::tactics::CLASSIFY_PER_REQUEST);
}
