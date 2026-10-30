//! Contract tests of the opening trainer machine (spec M8-5, D2213-D2231):
//! the machine is driven by hand, with the build run synchronously.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mychess_app::opening_trainer::{
    BuildSource, LineEnd, OpeningConfig, OpeningEffect, OpeningNotice, OpeningPhase,
    OpeningTrainer, Out, RepSourceKind, TrainSide, build,
};
use mychess_core::{Color, Move, Position};

const PGN: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 *\n";
const TWO_REPLIES: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 *\n\n\
[Event \"b\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 *\n\n\
[Event \"c\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 *\n\n\
[Event \"d\"]\n[Result \"*\"]\n\n1. e4 c5 2. Nf3 *\n";
const DUBIOUS: &str =
    "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 (2. f4?! exf4) 2... Nc6 *\n";

fn write(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("rep.pgn");
    std::fs::write(&path, text).expect("write");
    path
}

fn config(side: TrainSide, file: &Path) -> OpeningConfig {
    OpeningConfig {
        side,
        source: RepSourceKind::File,
        file: Some(file.to_path_buf()),
        ..OpeningConfig::default()
    }
}

fn keys() -> [String; 2] {
    ["rep|w".to_owned(), "rep|b".to_owned()]
}

fn uci(position: &Position, text: &str) -> Move {
    position.parse_uci(text).expect("uci")
}

/// Starts a session on a PGN text and runs the build.
fn session(
    text: &str,
    config: OpeningConfig,
    seed: u64,
) -> (OpeningTrainer, Vec<Out>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let path = write(dir.path(), text);
    let config = OpeningConfig {
        file: Some(path.clone()),
        ..config
    };
    let mut m = OpeningTrainer::new();
    let outs = m.start(config, seed, 100, Ok((BuildSource::File(path), keys())));
    let Some(Out::Effect(OpeningEffect::Build { request, job })) = outs.first().cloned() else {
        panic!("a build is asked for: {outs:?}");
    };
    assert_eq!(m.phase(), OpeningPhase::Building);
    let output = build(&job);
    let outs = m.built(request, output.map(Box::new));
    (m, outs, dir)
}

fn plays(outs: &[Out]) -> Vec<Move> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Play(m) => Some(*m),
            _ => None,
        })
        .collect()
}

fn default_white() -> OpeningConfig {
    OpeningConfig::default()
}

#[test]
fn a_session_builds_the_repertoire_and_shows_a_game_with_the_user_on_move() {
    let (m, outs, _d) = session(PGN, default_white(), 1);
    assert_eq!(m.phase(), OpeningPhase::UserTurn);
    assert_eq!(outs.first(), Some(&Out::ShowGame { flipped: false }));
    assert!(
        plays(&outs).is_empty(),
        "White moves first: nothing to play yet"
    );
    assert_eq!(m.color(), Some(Color::White));
}

#[test]
fn start_asks_for_the_build_and_saves_the_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), PGN);
    let mut m = OpeningTrainer::new();
    let outs = m.start(
        config(TrainSide::White, &path),
        1,
        100,
        Ok((BuildSource::File(path), keys())),
    );
    assert!(matches!(
        outs.first(),
        Some(Out::Effect(OpeningEffect::Build { .. }))
    ));
    assert!(outs.contains(&Out::SaveStore));
    assert!(m.phase().wants_ticks());
}

#[test]
fn a_refused_source_leaves_the_machine_closed_with_the_reason() {
    let mut m = OpeningTrainer::new();
    let outs = m.start(default_white(), 1, 100, Err(OpeningNotice::NoRepertoire));
    assert!(outs.is_empty());
    assert_eq!(m.phase(), OpeningPhase::Closed);
    assert_eq!(m.notice(), Some(OpeningNotice::NoRepertoire));
}

#[test]
fn a_right_move_is_played_and_the_machine_answers_from_the_repertoire() {
    let (mut m, _outs, _d) = session(PGN, default_white(), 1);
    let start = Position::new();
    let e4 = uci(&start, "e2e4");
    let outs = m.user_move(e4);
    let after = start.after(e4).unwrap();
    assert_eq!(plays(&outs), vec![e4, uci(&after, "e7e5")]);
    assert_eq!(m.phase(), OpeningPhase::UserTurn);
    assert_eq!(m.session().good, 1);
}

#[test]
fn a_move_out_of_the_repertoire_is_refused_counted_and_never_played() {
    let (mut m, _outs, _d) = session(PGN, default_white(), 1);
    let start = Position::new();
    let outs = m.user_move(uci(&start, "d2d4"));
    assert!(plays(&outs).is_empty());
    assert_eq!(m.notice(), Some(OpeningNotice::OutOfRepertoire));
    assert_eq!(m.phase(), OpeningPhase::UserTurn);
    assert_eq!(m.session().out, 1);
    let stat = m
        .store()
        .stat("rep|w", start.zobrist().value())
        .copied()
        .unwrap();
    assert_eq!((stat.seen, stat.out, stat.good), (1, 1, 0));
    // The user can try again and the right move is not "good" any more.
    let outs = m.user_move(uci(&start, "e2e4"));
    assert_eq!(plays(&outs).len(), 2);
    let stat = m
        .store()
        .stat("rep|w", start.zobrist().value())
        .copied()
        .unwrap();
    assert_eq!(
        (stat.good, stat.box_no),
        (0, 0),
        "an error spoils the first try"
    );
    assert_eq!(m.session().good, 0);
}

#[test]
fn a_dubious_move_is_an_error_when_strict_and_accepted_otherwise() {
    // After 1.e4 e5 the repertoire has 2.Nf3 and the dubious 2.f4?!.
    let play_to_the_choice = |m: &mut OpeningTrainer| {
        let start = Position::new();
        let e4 = uci(&start, "e2e4");
        m.user_move(e4);
        start
            .after(e4)
            .unwrap()
            .after(uci(&start.after(e4).unwrap(), "e7e5"))
            .unwrap()
    };
    let (mut strict, _o, _d) = session(DUBIOUS, default_white(), 1);
    let at = play_to_the_choice(&mut strict);
    let outs = strict.user_move(uci(&at, "f2f4"));
    assert!(plays(&outs).is_empty());
    assert_eq!(strict.notice(), Some(OpeningNotice::Dubious));

    let relaxed = OpeningConfig {
        strict: false,
        ..default_white()
    };
    let (mut loose, _o, _d) = session(DUBIOUS, relaxed, 1);
    let at = play_to_the_choice(&mut loose);
    let f4 = uci(&at, "f2f4");
    let outs = loose.user_move(f4);
    assert_eq!(plays(&outs).first(), Some(&f4));
    assert_eq!(loose.session().dubious, 1);
}

#[test]
fn the_machine_never_plays_a_dubious_reply_unless_told_to() {
    // As Black the opponent (White) has 2.Nf3 and the dubious 2.f4?!.
    let text = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 (2. f4?! exf4) 2... Nc6 *\n";
    let black = |best| OpeningConfig {
        side: TrainSide::Black,
        opponent_best: best,
        ..OpeningConfig::default()
    };
    let mut saw_dubious = false;
    for seed in 1..40u64 {
        let (mut m, outs, _d) = session(text, black(true), seed);
        let first = plays(&outs);
        let e4 = first[0];
        let start = Position::new();
        let after_e4 = start.after(e4).unwrap();
        let outs = m.user_move(uci(&after_e4, "e7e5"));
        let reply = plays(&outs)[1];
        let f4 = uci(&after_e4.after(uci(&after_e4, "e7e5")).unwrap(), "f2f4");
        assert_ne!(reply, f4, "never with opponent_best");
        let (mut m, outs, _d) = session(text, black(false), seed);
        let e4 = plays(&outs)[0];
        let after_e4 = start.after(e4).unwrap();
        let outs = m.user_move(uci(&after_e4, "e7e5"));
        saw_dubious |= plays(&outs)[1] == f4;
    }
    assert!(
        saw_dubious,
        "with the option off the refutation is drawn too"
    );
}

#[test]
fn the_machine_replies_in_proportion_to_how_often_a_move_is_played() {
    let (mut m, _o, _d) = session(TWO_REPLIES, default_white(), 11);
    let start = Position::new();
    let e4 = uci(&start, "e2e4");
    let after_e4 = start.after(e4).unwrap();
    let (e5, c5) = (uci(&after_e4, "e7e5"), uci(&after_e4, "c7c5"));
    let (mut n_e5, mut n_c5) = (0u32, 0u32);
    for _ in 0..400 {
        let outs = m.user_move(e4);
        match plays(&outs).get(1) {
            Some(r) if *r == e5 => n_e5 += 1,
            Some(r) if *r == c5 => n_c5 += 1,
            other => panic!("unexpected reply {other:?}"),
        }
        m.next_line();
    }
    assert!(n_c5 > 40, "the rare reply is still drawn: {n_c5}");
    assert!(
        n_e5 > 2 * n_c5,
        "e5 (3 games) beats c5 (1 game): {n_e5} to {n_c5}"
    );
}

#[test]
fn the_same_seed_draws_the_same_lines() {
    let run = |seed| {
        let (mut m, _o, _d) = session(TWO_REPLIES, default_white(), seed);
        let start = Position::new();
        let e4 = uci(&start, "e2e4");
        let mut replies = Vec::new();
        for _ in 0..30 {
            replies.push(plays(&m.user_move(e4)).get(1).copied());
            m.next_line();
        }
        replies
    };
    assert_eq!(run(5), run(5));
}

#[test]
fn a_line_ends_when_the_repertoire_ends_or_the_depth_is_reached() {
    let (mut m, _o, _d) = session(PGN, default_white(), 1);
    let mut position = Position::new();
    for text in ["e2e4", "e7e5", "g1f3", "b8c6"] {
        position.play(uci(&position, text)).unwrap();
    }
    let mut p = Position::new();
    for text in ["e2e4", "e7e5", "g1f3"] {
        let mv = uci(&p, text);
        if p.turn() == Color::White {
            m.user_move(mv);
        }
        p.play(mv).unwrap();
    }
    // After 3.Bb5 the repertoire has nothing more.
    let bb5 = uci(&position, "f1b5");
    let outs = m.user_move(bb5);
    assert!(outs.contains(&Out::SaveStore));
    assert_eq!(m.phase(), OpeningPhase::LineDone(LineEnd::Complete));
    assert_eq!(m.session().lines, 1);
    assert_eq!(m.session().perfect_lines, 1);

    let shallow = OpeningConfig {
        max_depth: 2,
        ..default_white()
    };
    let (mut m, _o, _d) = session(PGN, shallow, 1);
    let start = Position::new();
    m.user_move(uci(&start, "e2e4"));
    assert_eq!(m.phase(), OpeningPhase::LineDone(LineEnd::Depth));
}

#[test]
fn the_user_with_black_waits_for_the_first_move_of_the_machine() {
    let config = OpeningConfig {
        side: TrainSide::Black,
        ..OpeningConfig::default()
    };
    let (m, outs, _d) = session(PGN, config, 1);
    assert_eq!(outs.first(), Some(&Out::ShowGame { flipped: true }));
    assert_eq!(plays(&outs).len(), 1, "1.e4 is played for the user");
    assert_eq!(m.phase(), OpeningPhase::UserTurn);
    assert_eq!(m.color(), Some(Color::Black));
}

#[test]
fn both_sides_alternate_the_colour_line_after_line() {
    let config = OpeningConfig {
        side: TrainSide::Both,
        ..OpeningConfig::default()
    };
    let (mut m, _o, _d) = session(PGN, config, 3);
    let first = m.color().unwrap();
    m.next_line();
    let second = m.color().unwrap();
    m.next_line();
    let third = m.color().unwrap();
    assert_ne!(first, second);
    assert_eq!(first, third);
}

#[test]
fn a_repertoire_without_a_move_for_the_user_is_refused_with_the_dialog_back() {
    // Only White's first move: nothing for a Black user to answer.
    let config = OpeningConfig {
        side: TrainSide::Black,
        ..OpeningConfig::default()
    };
    let (m, outs, _d) = session("[Event \"a\"]\n[Result \"*\"]\n\n1. e4 *\n", config, 1);
    assert_eq!(m.phase(), OpeningPhase::Closed);
    assert_eq!(m.notice(), Some(OpeningNotice::EmptyRepertoire));
    assert!(m.dialog_open());
    assert!(outs.contains(&Out::Closed));
}

#[test]
fn hints_come_in_two_levels_and_spoil_the_first_try() {
    let (mut m, _o, _d) = session(PGN, default_white(), 1);
    assert!(m.hint().is_none());
    m.hint_step();
    let level1 = m.hint().unwrap();
    assert_eq!(level1.to, None);
    assert_eq!(level1.from.to_string(), "e2");
    m.hint_step();
    let level2 = m.hint().unwrap();
    assert_eq!(level2.to.map(|s| s.to_string()), Some("e4".to_owned()));
    let start = Position::new();
    m.user_move(uci(&start, "e2e4"));
    assert!(m.hint().is_none(), "the hint goes away with the move");
    let stat = m
        .store()
        .stat("rep|w", start.zobrist().value())
        .copied()
        .unwrap();
    assert_eq!(stat.good, 0, "help spoils the first try");
    assert_eq!(m.session().perfect_lines, 0);
}

#[test]
fn a_right_first_try_climbs_a_box_and_sets_the_due_day() {
    let (mut m, _o, _d) = session(PGN, default_white(), 1);
    let start = Position::new();
    m.user_move(uci(&start, "e2e4"));
    let stat = m
        .store()
        .stat("rep|w", start.zobrist().value())
        .copied()
        .unwrap();
    assert_eq!(
        (stat.seen, stat.good, stat.box_no, stat.due),
        (1, 1, 1, 101)
    );
}

#[test]
fn the_report_counts_the_positions_of_the_user() {
    let (mut m, _o, _d) = session(PGN, default_white(), 1);
    m.toggle_report();
    let report = m.view().report.unwrap();
    // The user moves at the start, after 2 plies and after 4: three positions.
    assert_eq!((report.positions, report.never, report.played), (3, 2, 1));
    m.toggle_report();
    assert!(m.view().report.is_none());
    let start = Position::new();
    m.user_move(uci(&start, "e2e4"));
    m.toggle_report();
    let report = m.view().report.unwrap();
    assert_eq!((report.played, report.good, report.learned), (2, 1, 0));
}

#[test]
fn candidates_are_listed_after_an_error_and_with_the_option() {
    let (mut m, _o, _d) = session(DUBIOUS, default_white(), 1);
    assert!(m.view().candidates.is_empty());
    let start = Position::new();
    m.user_move(uci(&start, "d2d4"));
    let candidates = m.view().candidates;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].san, "e4");
    assert_eq!(candidates[0].percent, 100);

    let config = OpeningConfig {
        show_candidates: true,
        ..default_white()
    };
    let (m, _o, _d) = session(PGN, config, 1);
    assert_eq!(m.view().candidates.len(), 1);
}

#[test]
fn not_repeating_runs_out_of_lines() {
    let config = OpeningConfig {
        no_repeat: true,
        ..default_white()
    };
    let (mut m, _o, _d) = session(PGN, config, 1);
    let mut p = Position::new();
    for text in ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"] {
        let mv = uci(&p, text);
        if p.turn() == Color::White {
            m.user_move(mv);
        }
        p.play(mv).unwrap();
    }
    assert!(matches!(m.phase(), OpeningPhase::LineDone(_)));
    m.next_line();
    assert_eq!(m.phase(), OpeningPhase::Exhausted);
}

#[test]
fn the_watchdog_drops_a_build_that_never_answers_and_a_late_answer_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), PGN);
    let mut m = OpeningTrainer::new();
    let outs = m.start(
        config(TrainSide::White, &path),
        1,
        100,
        Ok((BuildSource::File(path), keys())),
    );
    let Some(Out::Effect(OpeningEffect::Build { request, job })) = outs.first().cloned() else {
        panic!("build");
    };
    let mut now = Instant::now();
    m.tick(now);
    let mut closed = false;
    for _ in 0..40 {
        now += Duration::from_secs(2);
        if m.tick(now).contains(&Out::Closed) {
            closed = true;
            break;
        }
    }
    assert!(closed, "60 s of ticks end the wait");
    assert_eq!(m.notice(), Some(OpeningNotice::TimedOut));
    assert!(m.dialog_open());
    assert!(job.cancel.is_cancelled(), "the build is told to stop");
    // The late answer does nothing.
    let late = m.built(request, build(&job).map(Box::new));
    assert!(late.is_empty());
    assert_eq!(m.phase(), OpeningPhase::Closed);
}

#[test]
fn closing_ends_the_session_saves_and_cancels_the_build() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), PGN);
    let mut m = OpeningTrainer::new();
    let outs = m.start(
        config(TrainSide::White, &path),
        1,
        100,
        Ok((BuildSource::File(path), keys())),
    );
    let Some(Out::Effect(OpeningEffect::Build { job, .. })) = outs.first().cloned() else {
        panic!("build");
    };
    let outs = m.close();
    assert!(outs.contains(&Out::Closed) && outs.contains(&Out::SaveStore));
    assert!(job.cancel.is_cancelled());
    assert!(!m.is_live());
}

#[test]
fn a_garbage_repertoire_file_ends_the_session_with_a_notice_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rep.bin");
    std::fs::write(&path, b"nonsense").unwrap();
    let mut m = OpeningTrainer::new();
    let outs = m.start(
        config(TrainSide::White, &path),
        1,
        100,
        Ok((BuildSource::File(path), keys())),
    );
    let Some(Out::Effect(OpeningEffect::Build { request, job })) = outs.first().cloned() else {
        panic!("build");
    };
    m.built(request, build(&job).map(Box::new));
    assert_eq!(m.phase(), OpeningPhase::Closed);
    assert!(m.notice().is_some());
}

#[test]
fn retry_replays_the_same_line_and_next_draws_another() {
    let (mut m, _o, _d) = session(TWO_REPLIES, default_white(), 2);
    let start = Position::new();
    let e4 = uci(&start, "e2e4");
    let first = plays(&m.user_move(e4))[1];
    let outs = m.retry();
    assert_eq!(outs.first(), Some(&Out::ShowGame { flipped: false }));
    // The same target line: the opponent follows it again.
    assert_eq!(plays(&m.user_move(e4))[1], first);
    assert_eq!(m.view().line, 1, "a retry is not a new line");
    m.next_line();
    assert_eq!(m.view().line, 2);
}
