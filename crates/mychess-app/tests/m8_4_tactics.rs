//! Contract tests of the tactics training in the application (spec M8-4,
//! D1900-D1946): the whole flow through `update`, with the effects run by
//! `perform` (the real solver, the real files in a temporary directory).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mychess_app::tactics::{
    FailReason, Feedback, ProblemOutcome, SourceKind, TacticsConfig, TacticsMessage, TacticsMode,
    TacticsNotice, TacticsPhase,
};
use mychess_app::{AppState, Command, Effect, GameMessage, Message, PlayOptions, perform, update};
use mychess_core::{Color, Position, Square};

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
const MATE2: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";
const NO_MATE: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

fn fen_game(event: &str, fen: &str, moves: &str, comment: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{comment}{moves} *\n\n")
}

fn write_problems(dir: &std::path::Path, body: &str) -> PathBuf {
    let path = dir.join("problems.pgn");
    std::fs::write(&path, body).expect("write the problems");
    path
}

fn standard_pgn() -> String {
    format!(
        "{}{}{}",
        fen_game("one", MATE1, "", ""),
        fen_game("two", MATE2, "", ""),
        fen_game("three", NO_MATE, "", "")
    )
}

fn config(path: &std::path::Path) -> TacticsConfig {
    TacticsConfig {
        file: Some(path.to_path_buf()),
        ..TacticsConfig::default()
    }
}

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new(pgn: &str) -> Rig {
        let dir = tempfile::tempdir().expect("dir");
        write_problems(dir.path(), pgn);
        let prefs = dir.path().join("preferences.toml");
        Rig {
            state: AppState::with_preferences_path(Some(prefs)),
            dir,
        }
    }

    fn pgn(&self) -> PathBuf {
        self.dir.path().join("problems.pgn")
    }

    fn store_file(&self) -> PathBuf {
        self.dir.path().join("tactics.toml")
    }

    async fn send(&mut self, message: Message) {
        let effects = update(&mut self.state, message);
        self.pump(effects).await;
    }

    async fn pump(&mut self, effects: Vec<Effect>) {
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        let mut guard = 0;
        while let Some(effect) = queue.pop_front() {
            guard += 1;
            assert!(guard < 500, "the effects do not settle");
            if matches!(effect, Effect::Tactics(_)) {
                let answer = perform(effect).await;
                queue.extend(update(&mut self.state, answer));
            }
        }
    }

    async fn start(&mut self, config: TacticsConfig) {
        self.send(Message::Tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        }))
        .await;
    }

    async fn play(&mut self, from: &str, to: &str) {
        let (from, to): (Square, Square) = (from.parse().expect("sq"), to.parse().expect("sq"));
        self.send(Message::Game(GameMessage::PlaySquares {
            from,
            to,
            promotion: None,
            options: PlayOptions::default(),
        }))
        .await;
    }

    async fn tactics(&mut self, message: TacticsMessage) {
        self.send(Message::Tactics(message)).await;
    }

    fn phase(&self) -> TacticsPhase {
        self.state.tactics_phase()
    }

    fn plies(&self) -> usize {
        self.state.session.game().mainline().count()
    }
}

#[tokio::test]
async fn starting_shows_the_first_problem_with_the_mover_at_the_bottom() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(
        *rig.state.session.position(),
        Position::from_fen(MATE1).expect("fen")
    );
    assert!(!rig.state.board_flipped);
    let view = rig.state.tactics_view();
    assert_eq!(view.mate_in, Some(1));
    assert_eq!(view.attempts_left, 3);
    assert_eq!(view.stats.shown, 1);
    assert!(!view.dialog_open);
}

#[tokio::test]
async fn black_to_move_flips_the_board() {
    let fen = "r5k1/8/8/8/8/8/5PPP/6K1 b - - 0 1";
    let mut rig = Rig::new(&fen_game("black", fen, "", ""));
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.session.position().turn(), Color::Black);
    assert!(rig.state.board_flipped);
}

#[tokio::test]
async fn a_wrong_move_is_not_played_and_costs_an_attempt() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.play("g1", "f1").await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.plies(), 0, "the wrong move stays out of the game");
    let view = rig.state.tactics_view();
    assert_eq!(view.attempts_left, 2);
    assert!(matches!(view.feedback, Feedback::Wrong(_)));
}

#[tokio::test]
async fn the_mating_move_solves_a_mate_in_one_and_marks_it_solved_on_disk() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.play("a1", "a8").await;
    assert_eq!(rig.phase(), TacticsPhase::Solved(ProblemOutcome::Perfect));
    assert_eq!(rig.plies(), 1);
    let view = rig.state.tactics_view();
    assert_eq!(view.stats.perfect, 1);
    assert_eq!(view.stats.streak, 1);
    assert_eq!(view.solved_in_source, 1);
    let saved = std::fs::read_to_string(rig.store_file()).expect("tactics.toml written");
    assert!(saved.contains("solved"), "{saved}");
}

#[tokio::test]
async fn next_goes_to_the_mate_in_two_and_the_opponent_answers() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.play("a1", "a8").await;
    rig.tactics(TacticsMessage::Next).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.tactics_view().mate_in, Some(2));
    rig.play("a1", "a6").await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.plies(), 2, "the move and the reply of the opponent");
    assert_eq!(rig.state.tactics_view().mate_in, Some(1));
    assert_eq!(rig.state.tactics_view().feedback, Feedback::Correct);
    rig.play("b6", "b7").await;
    assert_eq!(rig.phase(), TacticsPhase::Solved(ProblemOutcome::Perfect));
}

#[tokio::test]
async fn a_problem_without_mate_is_never_offered_and_the_end_is_reported() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.play("a1", "a8").await;
    rig.tactics(TacticsMessage::Next).await;
    rig.play("a1", "a6").await;
    rig.play("b6", "b7").await;
    rig.tactics(TacticsMessage::Next).await;
    assert_eq!(rig.phase(), TacticsPhase::Exhausted);
}

#[tokio::test]
async fn a_hint_makes_the_result_with_help() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    assert!(rig.state.tactics_view().hint.is_none());
    rig.tactics(TacticsMessage::Hint).await;
    let first = rig.state.tactics_view().hint.expect("level one");
    assert_eq!(first.from, "a1".parse::<Square>().expect("sq"));
    assert!(first.to.is_none());
    rig.tactics(TacticsMessage::Hint).await;
    let second = rig.state.tactics_view().hint.expect("level two");
    assert_eq!(second.to, Some("a8".parse::<Square>().expect("sq")));
    rig.play("a1", "a8").await;
    assert_eq!(rig.phase(), TacticsPhase::Solved(ProblemOutcome::WithHelp));
}

#[tokio::test]
async fn hints_can_be_turned_off() {
    let mut rig = Rig::new(&standard_pgn());
    let mut cfg = config(&rig.pgn());
    cfg.hints = false;
    rig.start(cfg).await;
    rig.tactics(TacticsMessage::Hint).await;
    assert!(rig.state.tactics_view().hint.is_none());
    assert!(!rig.state.tactics_view().can_hint);
}

#[tokio::test]
async fn solving_after_a_mistake_is_with_mistakes() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.play("g1", "f1").await;
    rig.play("a1", "a8").await;
    assert_eq!(
        rig.phase(),
        TacticsPhase::Solved(ProblemOutcome::WithMistakes)
    );
}

#[tokio::test]
async fn using_every_attempt_loses_the_problem_and_shows_the_solution() {
    let mut rig = Rig::new(&standard_pgn());
    let mut cfg = config(&rig.pgn());
    cfg.max_attempts = 2;
    rig.start(cfg).await;
    rig.play("g1", "f1").await;
    rig.play("g1", "h1").await;
    assert_eq!(rig.phase(), TacticsPhase::Failed(FailReason::NoAttempts));
    assert_eq!(rig.plies(), 1, "the solution was added to the game");
    assert_eq!(rig.state.tactics_view().stats.failed, 1);
    assert_eq!(rig.state.tactics_view().stats.streak, 0);
}

#[tokio::test]
async fn show_solution_counts_as_a_failure_and_then_the_game_is_free() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.tactics(TacticsMessage::ShowSolution).await;
    assert_eq!(rig.phase(), TacticsPhase::Failed(FailReason::GaveUp));
    assert_eq!(rig.plies(), 1);
    // The editor is free to navigate now.
    rig.send(Message::Game(GameMessage::Navigate(
        mychess_app::Nav::GameEnd,
    )))
    .await;
    assert_eq!(
        rig.state.session.cursor(),
        rig.state.session.game().mainline().last().expect("move")
    );
}

#[tokio::test]
async fn try_again_restores_the_problem_and_the_attempts() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.tactics(TacticsMessage::ShowSolution).await;
    rig.tactics(TacticsMessage::Retry).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.plies(), 0);
    assert_eq!(rig.state.tactics_view().attempts_left, 3);
    assert_eq!(
        rig.state.tactics_view().stats.shown,
        2,
        "the new try is shown again, so shown never falls behind ended"
    );
    rig.play("a1", "a8").await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
}

#[tokio::test]
async fn skipping_counts_as_skipped_and_leaves_the_problem_unsolved() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.tactics(TacticsMessage::Next).await;
    let view = rig.state.tactics_view();
    assert_eq!(view.stats.skipped, 1);
    assert_eq!(view.solved_in_source, 0);
    assert_eq!(view.mate_in, Some(2), "the next problem");
}

#[tokio::test]
async fn editing_the_game_is_locked_while_a_problem_is_open() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.send(Message::Game(GameMessage::Navigate(
        mychess_app::Nav::GameEnd,
    )))
    .await;
    assert_eq!(rig.state.tactics_view().notice, Some(TacticsNotice::Locked));
    rig.send(Message::Game(GameMessage::TruncateEnd)).await;
    assert_eq!(rig.plies(), 0);
}

#[tokio::test]
async fn closing_puts_the_previous_game_back() {
    let mut rig = Rig::new(&standard_pgn());
    let before = rig.state.session.generation();
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    assert_ne!(*rig.state.session.position(), Position::new());
    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(*rig.state.session.position(), Position::new());
    assert_ne!(rig.state.session.generation(), before);
    assert!(!rig.state.is_dirty());
    assert!(!rig.state.board_flipped);
}

#[tokio::test]
async fn a_game_with_unsaved_changes_asks_before_the_start() {
    // Since M8-4d the start asks whether to save (D2145) instead of
    // refusing; the answers are tested in `m8_4d_app.rs`.
    let mut rig = Rig::new(&standard_pgn());
    rig.play("e2", "e4").await;
    assert!(rig.state.is_dirty());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(rig.state.discard_prompt.is_some(), "the question is open");
    assert_eq!(rig.plies(), 1, "the game of the user is untouched");
}

#[tokio::test]
async fn replacing_the_game_by_other_means_ends_the_session() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    rig.send(Message::Command(Command::NewGame)).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(*rig.state.session.position(), Position::new());
}

#[tokio::test]
async fn no_file_means_no_source() {
    let mut rig = Rig::new(&standard_pgn());
    rig.start(TacticsConfig::default()).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::NoSource)
    );
}

#[tokio::test]
async fn a_missing_file_reports_the_source_and_reopens_the_dialog() {
    let mut rig = Rig::new(&standard_pgn());
    let missing = rig.dir.path().join("missing.pgn");
    rig.start(config(&missing)).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    let view = rig.state.tactics_view();
    assert_eq!(view.notice, Some(TacticsNotice::SourceUnreadable));
    assert!(view.dialog_open);
    assert_eq!(*rig.state.session.position(), Position::new());
}

#[tokio::test]
async fn best_move_mode_uses_the_marker_and_the_next_move_of_the_game() {
    // A normal game, with the marker on the move before the tactic.
    let pgn = "[Event \"G\"]\n\n1. e4 e5 2. Nf3 {****D3 0.2->1.5} Nc6 3. Bc4 Nf6 4. Ng5 *\n\n";
    let mut rig = Rig::new(pgn);
    let mut cfg = config(&rig.pgn());
    cfg.mode = TacticsMode::BestMove;
    rig.start(cfg).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    // The position is after 2.Nf3; the solution is 2...Nc6.
    assert_eq!(rig.state.session.position().turn(), Color::Black);
    rig.play("g8", "f6").await;
    assert_eq!(
        rig.phase(),
        TacticsPhase::Solving,
        "not the move of the game"
    );
    assert_eq!(rig.state.tactics_view().attempts_left, 2);
    rig.play("b8", "c6").await;
    assert_eq!(
        rig.phase(),
        TacticsPhase::Solved(ProblemOutcome::WithMistakes)
    );
    assert!(rig.plies() >= 2, "the rest of the line is added for study");
}

#[tokio::test]
async fn the_difficulty_filter_skips_easier_markers() {
    let pgn = "[Event \"G\"]\n\n1. e4 {****D2 0.0->1.0} e5 2. Nf3 {****D6 0.0->1.0} Nc6 *\n\n";
    let mut rig = Rig::new(pgn);
    let mut cfg = config(&rig.pgn());
    cfg.mode = TacticsMode::BestMove;
    cfg.difficulty_min = 5;
    rig.start(cfg).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.session.position().turn(), Color::Black);
    assert_eq!(rig.state.session.position().fullmove_number(), 2);
}

#[tokio::test]
async fn solved_problems_survive_a_restart_and_reset_forgets_them() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg.clone()).await;
    rig.play("a1", "a8").await;
    rig.tactics(TacticsMessage::Close).await;
    // A new run of the program reads tactics.toml and skips the solved one.
    let prefs = rig.dir.path().join("preferences.toml");
    let mut again = AppState::with_preferences_path(Some(prefs));
    let effects = update(&mut again, Message::Tactics(TacticsMessage::Open));
    let mut second = Rig {
        state: again,
        dir: rig.dir,
    };
    second.pump(effects).await;
    second.start(cfg.clone()).await;
    assert_eq!(
        second.state.tactics_view().mate_in,
        Some(2),
        "problem one is solved"
    );
    assert_eq!(
        second.state.tactics_view().stats.perfect,
        1,
        "the score came back"
    );
    second.tactics(TacticsMessage::Close).await;
    second
        .tactics(TacticsMessage::ResetStats(Box::new(cfg.clone())))
        .await;
    second.start(cfg).await;
    assert_eq!(
        second.state.tactics_view().mate_in,
        Some(1),
        "reset brought it back"
    );
    assert_eq!(second.state.tactics_view().stats.perfect, 0);
}

#[tokio::test]
async fn the_time_limit_fails_the_problem() {
    let mut rig = Rig::new(&standard_pgn());
    let mut cfg = config(&rig.pgn());
    cfg.time_limit_s = 5;
    rig.start(cfg).await;
    let t0 = Instant::now();
    for i in 0..8u64 {
        rig.tactics(TacticsMessage::Tick(t0 + Duration::from_secs(i * 2)))
            .await;
    }
    assert_eq!(rig.phase(), TacticsPhase::Failed(FailReason::TimeUp));
}

#[tokio::test]
async fn a_suspended_computer_does_not_inflate_the_clock() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    let t0 = Instant::now();
    rig.tactics(TacticsMessage::Tick(t0)).await;
    rig.tactics(TacticsMessage::Tick(t0 + Duration::from_secs(3600)))
        .await;
    assert!(rig.state.tactics_view().elapsed_s <= 2);
}

#[tokio::test]
async fn the_assistance_lock_drops_everything_that_helps_but_not_the_exit() {
    use mychess_app::online::{Family, family_of};
    for message in [
        TacticsMessage::Open,
        TacticsMessage::Hint,
        TacticsMessage::ShowSolution,
        TacticsMessage::Next,
        TacticsMessage::Retry,
        TacticsMessage::Start {
            config: Box::default(),
            seed: 0,
        },
    ] {
        assert_eq!(
            family_of(&Message::Tactics(message.clone())),
            Some(Family::Engines),
            "{message:?}"
        );
    }
    for message in [
        TacticsMessage::Close,
        TacticsMessage::CloseDialog,
        TacticsMessage::Tick(Instant::now()),
    ] {
        assert_eq!(
            family_of(&Message::Tactics(message.clone())),
            None,
            "{message:?}"
        );
    }
}

#[tokio::test]
async fn the_trainer_action_belongs_to_the_engines_family_and_has_a_default_key() {
    use mychess_app::online::{Family, family_of};
    use mychess_app::{Action, Shortcuts};
    assert_eq!(
        family_of(&Message::Action(Action::TacticsTrainer)),
        Some(Family::Engines)
    );
    let shortcuts = Shortcuts::defaults();
    assert_eq!(shortcuts.bindings(Action::TacticsTrainer).len(), 1);
}

#[tokio::test]
async fn stale_answers_are_ignored() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    rig.start(cfg).await;
    let before = rig.state.tactics_view();
    let stale = Message::Tactics(TacticsMessage::Found {
        request: mychess_app::RequestId(9999),
        outcome: Box::default(),
    });
    let effects = update(&mut rig.state, stale);
    assert_eq!(
        effects,
        Vec::<Effect>::new(),
        "a stale answer asks for nothing"
    );
    assert_eq!(rig.state.tactics_view(), before);
}

#[tokio::test]
async fn a_request_that_never_answers_is_dropped_by_the_watchdog() {
    let mut rig = Rig::new(&standard_pgn());
    let cfg = config(&rig.pgn());
    // Start without running the effects: the answer never comes.
    let effects = update(
        &mut rig.state,
        Message::Tactics(TacticsMessage::Start {
            config: Box::new(cfg),
            seed: 1,
        }),
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Tactics(_))));
    assert_eq!(rig.phase(), TacticsPhase::Opening);
    let t0 = Instant::now();
    for i in 0..40u64 {
        drop(update(
            &mut rig.state,
            Message::Tactics(TacticsMessage::Tick(t0 + Duration::from_secs(i * 2))),
        ));
    }
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    let view = rig.state.tactics_view();
    assert_eq!(view.notice, Some(TacticsNotice::TimedOut));
    assert!(view.dialog_open);
}

#[tokio::test]
async fn the_random_order_visits_every_problem_before_repeating() {
    let mut rig = Rig::new(&format!(
        "{}{}{}{}",
        fen_game("a", MATE1, "", ""),
        fen_game("b", "7k/8/6K1/8/8/8/8/5Q2 w - - 0 1", "", ""),
        fen_game("c", "6k1/5ppp/8/8/8/8/1R6/R5K1 w - - 0 1", "", ""),
        fen_game("d", MATE2, "", "")
    ));
    let mut cfg = config(&rig.pgn());
    cfg.order = mychess_app::tactics::TacticsOrder::Random;
    cfg.include_solved = true;
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(cfg),
        seed: 77,
    }))
    .await;
    let mut seen = Vec::new();
    for _ in 0..4 {
        assert_eq!(rig.phase(), TacticsPhase::Solving);
        seen.push(rig.state.session.position().fen());
        rig.tactics(TacticsMessage::Next).await;
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 4, "four different problems in one lap");
    let _ = SourceKind::File;
}
