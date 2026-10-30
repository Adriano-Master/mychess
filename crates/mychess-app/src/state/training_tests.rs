//! Tests of the engine modes: a scripted engine answers the searches
//! through synthetic events fed to `update` (spec M4-8).

use std::time::Duration;

use mychess_core::{Game, Move, Position};
use mychess_engine::{
    BestMove, Capabilities, EngineCommand, EngineEvent, EngineIdentity, EngineWarning, SearchId,
    SearchRequest,
};

use super::{AppState, update};
use crate::annotate::AnnotateMessage;
use crate::effect::Effect;
use crate::engines::{EngineDraft, EngineRunState, RunId};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::session::{GameMessage, Nav};
use crate::shortcuts::Action;
use crate::training::{AutomoveState, EngineMode, ModeKind, ModeMessage, ModeNotice, StopReason};

// ---- harness ----

fn setup(game: Game) -> (AppState, RunId) {
    let mut state = AppState::default();
    let id = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add");
    let run = state.engine_runs.begin(id, true);
    let r = state.engine_runs.get_mut(run).expect("run");
    r.state = EngineRunState::Running;
    r.identity = Some(Box::new(EngineIdentity {
        name: "Fake 1".to_owned(),
        author: None,
        options: Vec::new(),
        capabilities: Capabilities::default(),
    }));
    state.session.replace_game(game);
    (state, run)
}

fn game_of(ucis: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for uci in ucis {
        let mv = game
            .position_at(node)
            .expect("position")
            .parse_uci(uci)
            .expect("uci");
        node = game.add_move(node, mv).expect("legal");
    }
    game
}

fn game_from_fen(fen: &str) -> Game {
    Game::from_position(Position::from_fen(fen).expect("fen"))
}

fn send(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    update(state, message.into())
}

fn mode(state: &mut AppState, message: ModeMessage) -> Vec<Effect> {
    send(state, Message::Mode(message))
}

fn event(state: &mut AppState, run: RunId, event: EngineEvent) -> Vec<Effect> {
    send(
        state,
        crate::engines::EngineMessage::Event {
            run,
            event: Box::new(event),
        },
    )
}

fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

fn last_search(effects: &[Effect]) -> Option<SearchRequest> {
    commands(effects).into_iter().rev().find_map(|c| match c {
        EngineCommand::Search(request) => Some(request),
        _ => None,
    })
}

fn stopped(effects: &[Effect]) -> bool {
    commands(effects)
        .iter()
        .any(|c| matches!(c, EngineCommand::Stop))
}

fn position_of(request: &SearchRequest) -> Position {
    let mut pos = request.position.start.clone();
    for m in &request.position.moves {
        pos.play(*m).expect("legal");
    }
    pos
}

/// The engine answers the search of `effects` with `mv` (the search has
/// the id `id`); returns the effects of the answer.
fn answer(
    state: &mut AppState,
    run: RunId,
    effects: &[Effect],
    id: u64,
    mv: Option<Move>,
) -> Vec<Effect> {
    let request = last_search(effects).expect("a search was sent");
    let search = SearchId(id);
    event(
        state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(state, run, EngineEvent::SearchStarted(search));
    event(
        state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv,
            ponder: None,
        }),
    )
}

/// The engine starts the search of `effects`, warns that its move is
/// illegal and answers with no move.
fn answer_illegal(state: &mut AppState, run: RunId, effects: &[Effect], id: u64) -> Vec<Effect> {
    let request = last_search(effects).expect("a search was sent");
    let search = SearchId(id);
    event(
        state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(state, run, EngineEvent::SearchStarted(search));
    event(
        state,
        run,
        EngineEvent::Warning(EngineWarning::IllegalBestMove("e2e5".to_owned())),
    );
    event(
        state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: None,
            ponder: None,
        }),
    )
}

fn uci_move(request: &SearchRequest, uci: &str) -> Move {
    position_of(request).parse_uci(uci).expect("uci")
}

/// Answers the searches with the moves of `script` in order.
fn play_script(
    state: &mut AppState,
    run: RunId,
    mut effects: Vec<Effect>,
    script: &[&str],
) -> Vec<Effect> {
    for (n, uci) in script.iter().enumerate() {
        let Some(request) = last_search(&effects) else {
            break;
        };
        let mv = uci_move(&request, uci);
        let id = u64::try_from(n).expect("small") + 100;
        effects = answer(state, run, &effects, id, Some(mv));
    }
    effects
}

fn mode_notice(state: &AppState) -> Option<ModeNotice> {
    match &state.notice {
        Some(AppNotice::Mode(n)) => Some(n.clone()),
        _ => None,
    }
}

// Always `Some`: it is compared with the `Option` of `mode_notice`.
#[allow(clippy::unnecessary_wraps)]
fn ended(kind: ModeKind, reason: StopReason) -> Option<ModeNotice> {
    Some(ModeNotice::Ended { kind, reason })
}

fn ply(state: &AppState) -> u32 {
    state
        .session
        .game()
        .ply(state.session.cursor())
        .expect("ply")
}

fn play(state: &mut AppState, uci: &str) -> Vec<Effect> {
    let mv = state.session.position().parse_uci(uci).expect("uci");
    send(
        state,
        GameMessage::PlayMove {
            mv,
            options: crate::session::PlayOptions::default(),
        },
    )
}

// ---- threat ----

#[test]
fn threat_enters_trial_with_a_null_move_and_sends_it_without_moves() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5"]));
    send(&mut state, GameMessage::Navigate(Nav::End));
    let fen_before = state.session.position().fen();
    let effects = mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(state.session.in_trial());
    assert_eq!(state.engine_mode(run), EngineMode::Threat);
    assert!(state.threat_active());
    // The position of the null move: the other side to move, no en passant.
    let request = last_search(&effects).expect("search of the new position");
    // The engine layer turns a path with a null move into `position fen`
    // without moves (mychess-engine, uci/format.rs).
    assert_eq!(request.position.moves.last(), Some(&Move::Null));
    let after = position_of(&request);
    assert_eq!(after.turn(), mychess_core::Color::Black);
    assert_eq!(
        after.fen().split(' ').next(),
        fen_before.split(' ').next(),
        "same board"
    );
    assert_eq!(after.ep_square(), None);
}

#[test]
fn leaving_the_threat_restores_the_game_the_cursor_and_the_revision() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "g1f3"]));
    send(&mut state, GameMessage::Navigate(Nav::Back(1)));
    let game = state.session.game().clone();
    let cursor = state.session.cursor();
    let revision = state.session.revision();
    mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(state.session.in_trial());
    assert_ne!(state.session.cursor(), cursor);
    let effects = mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(!state.session.in_trial());
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert!(state.session.game().same_content(&game));
    assert_eq!(state.session.cursor(), cursor);
    assert_eq!(state.session.revision(), revision);
    assert!(!state.is_dirty(), "the real game was never touched");
    // The original position goes back to the engine, as a normal path.
    let request = last_search(&effects).expect("search of the restored position");
    assert!(!request.position.moves.contains(&Move::Null));
}

#[test]
fn threat_is_refused_in_check_and_leaves_no_trial_behind() {
    let (mut state, run) = setup(game_from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1"));
    let effects = mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(!state.session.in_trial());
    assert_eq!(mode_notice(&state), Some(ModeNotice::NullMoveInCheck));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    // Only the analysis of the board goes on; never a null move.
    assert!(last_search(&effects).is_none_or(|r| !r.position.moves.contains(&Move::Null)));
    assert!(!state.mode_availability(run).threat);
}

#[test]
fn threat_is_refused_with_a_locked_analysis() {
    let (mut state, run) = setup(Game::new());
    state.engine_runs.set_locked(run, true);
    mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(!state.session.in_trial());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Analysis(crate::analysis::AnalysisNotice::Locked))
    ));
    assert!(!state.mode_availability(run).threat);
}

#[test]
fn threat_ends_when_the_user_leaves_the_trial_or_the_null_move() {
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleThreat(run));
    send(&mut state, GameMessage::ExitTrial);
    assert_eq!(state.engine_mode(run), EngineMode::Idle);

    mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(state.threat_active());
    send(&mut state, GameMessage::Navigate(Nav::Back(1)));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert!(state.session.in_trial(), "trial mode is the user's now");
}

#[test]
fn threat_refuses_when_the_user_is_already_in_trial() {
    let (mut state, run) = setup(Game::new());
    send(&mut state, GameMessage::EnterTrial { null_move: false });
    mode(&mut state, ModeMessage::ToggleThreat(run));
    assert_eq!(mode_notice(&state), Some(ModeNotice::TrialMode));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
}

// ---- shoot-out ----

const KNIGHTS: [&str; 10] = [
    "g1f3", "b8c6", "b1c3", "g8f6", "e2e3", "e7e6", "f1e2", "f8e7", "e1g1", "e8g8",
];

#[test]
fn shootout_plays_one_move_per_bestmove_and_is_one_undo_point() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
    assert!(!state.engine_runs.get(run).expect("run").analyzing);
    let request = last_search(&effects).expect("first search");
    assert_eq!(
        request.limits.movetime,
        Some(state.session.autoplay().delay)
    );
    assert!(!request.limits.infinite);
    assert!(request.position.moves.is_empty());

    let effects = play_script(&mut state, run, effects, &KNIGHTS);
    assert_eq!(ply(&state), 10);
    // The next step was asked, from the position after the tenth move.
    assert_eq!(
        last_search(&effects).expect("next").position.moves.len(),
        10
    );
    assert_eq!(state.session.undo_depth(), 1, "one undo point");

    let stop = mode(&mut state, ModeMessage::StopAll);
    assert!(stopped(&stop));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert!(
        state.engine_runs.get(run).expect("run").analyzing,
        "restored"
    );
    assert_eq!(mode_notice(&state), None, "the user's stop is not reported");
    send(&mut state, GameMessage::Undo);
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn shootout_does_not_write_the_result_tag() {
    let (mut state, run) = setup(game_from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"));
    let before = state.session.game().tags().clone();
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    play_script(&mut state, run, effects, &["a1a8"]);
    assert_eq!(*state.session.game().tags(), before);
}

#[test]
fn shootout_starts_a_variation_when_the_cursor_is_not_at_the_end() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "g1f3"]));
    send(&mut state, GameMessage::Navigate(Nav::Back(2)));
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    // c2c4 is not the move of the game: it goes to a variation.
    play_script(&mut state, run, effects, &["c2c4"]);
    let game = state.session.game();
    assert_eq!(game.mainline().count(), 3, "the main line is intact");
    assert!(game.is_variation(state.session.cursor()).expect("node"));
}

#[test]
fn shootout_ends_at_checkmate_with_its_reason() {
    let (mut state, run) = setup(game_from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"));
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let effects = play_script(&mut state, run, effects, &["a1a8"]);
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::Checkmate)
    );
    assert!(last_search(&effects).is_none(), "no search in a mate");
    assert!(state.engine_runs.get(run).expect("run").analyzing);
}

#[test]
fn shootout_does_not_ask_the_engine_in_a_terminal_position() {
    let (mut state, run) = setup(game_from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"));
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert!(last_search(&effects).is_none());
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::Stalemate)
    );
}

#[test]
fn shootout_ends_by_the_fifty_move_rule() {
    let (mut state, run) = setup(game_from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 98 80"));
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    play_script(&mut state, run, effects, &["e1d1", "e8d8"]);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::FiftyMoves)
    );
}

#[test]
fn shootout_ends_by_threefold_repetition() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let shuffle = [
        "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
    ];
    play_script(&mut state, run, effects, &shuffle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::Repetition)
    );
    assert_eq!(ply(&state), 8);
}

#[test]
fn shootout_ends_without_a_move_when_the_engine_gives_none() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    answer(&mut state, run, &effects, 100, None);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::NoMove)
    );
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
}

#[test]
fn shootout_asks_again_once_after_an_illegal_move_and_then_fails() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let effects = answer_illegal(&mut state, run, &effects, 100);
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
    assert!(last_search(&effects).is_some(), "asked once more");
    answer_illegal(&mut state, run, &effects, 101);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::EngineError)
    );
    assert_eq!(ply(&state), 0);
}

#[test]
fn shootout_stops_when_the_engine_dies() {
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let failure = crate::engines::EngineFailure {
        kind: crate::engines::FailureKind::Crashed,
        exit_code: None,
        stderr_tail: Vec::new(),
    };
    state.engine_runs.get_mut(run).expect("run").state = EngineRunState::Failed(failure);
    send(&mut state, GameMessage::Navigate(Nav::Start));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::EngineError)
    );
}

#[test]
fn a_user_edit_ends_the_shootout_and_has_its_own_undo_point() {
    for edit in 0..2 {
        let (mut state, run) = setup(Game::new());
        let fx = mode(&mut state, ModeMessage::ToggleFinishGame(run));
        play_script(&mut state, run, fx, &["g1f3", "g8f6"]);
        assert_eq!(state.session.undo_depth(), 1);
        let node = state.session.cursor();
        let message = match edit {
            0 => GameMessage::SetComment {
                node,
                text: "hi".into(),
            },
            1 => GameMessage::AddNag(mychess_core::Nag::new(1).expect("nag")),
            _ => GameMessage::StripNags,
        };
        let fx = send(&mut state, message);
        assert!(stopped(&fx), "the search in flight is stopped");
        assert_eq!(state.engine_mode(run), EngineMode::Idle);
        assert_eq!(
            mode_notice(&state),
            ended(ModeKind::ShootOut, StopReason::GameChanged)
        );
        assert_eq!(state.session.undo_depth(), 2, "the edit has its own point");
        send(&mut state, GameMessage::Undo);
        assert_eq!(ply(&state), 2, "the engine's game stays");
        send(&mut state, GameMessage::Undo);
        assert_eq!(state.session.game().mainline().count(), 0);
    }
}

#[test]
fn an_open_question_of_the_user_ends_the_shootout_instead_of_failing() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5"]));
    send(&mut state, GameMessage::Navigate(Nav::Start));
    let fx = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let request = last_search(&fx).expect("search");
    // A question opened behind the mode's back (not through the funnel).
    state.session.update(GameMessage::Navigate(Nav::Start));
    state.session.update(GameMessage::PlayMove {
        mv: uci_move(&request, "d2d4"),
        options: crate::session::PlayOptions::default(),
    });
    assert!(state.session.prompt().is_some());
    let search = SearchId(971);
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::GameChanged)
    );
}

#[test]
fn shootout_stops_when_the_user_navigates_or_the_game_is_replaced() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    play_script(&mut state, run, effects, &["g1f3"]);
    send(&mut state, GameMessage::Navigate(Nav::Back(1)));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::GameChanged)
    );

    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert!(last_search(&effects).is_some());
    state.session.replace_game(Game::new());
    send(&mut state, GameMessage::Navigate(Nav::Start));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
}

#[test]
fn a_late_bestmove_after_the_stop_changes_nothing() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let request = last_search(&effects).expect("search");
    let mv = uci_move(&request, "e2e4");
    send(&mut state, Action::ClearMoveEntry);
    assert_eq!(state.engine_mode(run), EngineMode::Idle, "Esc stops it");
    answer(&mut state, run, &effects, 100, Some(mv));
    assert_eq!(ply(&state), 0);
}

#[test]
fn modes_exclude_autoplay_each_other_and_the_annotation() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "g1f3", "b8c6"]));
    // Autoplay on: the shoot-out switches it off and says so.
    send(&mut state, GameMessage::Navigate(Nav::Start));
    send(&mut state, GameMessage::StartAutoplay);
    assert!(state.session.autoplay().running);
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert!(!state.session.autoplay().running);
    assert_eq!(mode_notice(&state), Some(ModeNotice::Conflict));
    // And the autoplay is refused while the engine plays.
    send(&mut state, GameMessage::StartAutoplay);
    assert!(!state.session.autoplay().running);
    // Another mode replaces it.
    mode(&mut state, ModeMessage::ToggleAutomove(run));
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::Automove { .. }
    ));
    mode(&mut state, ModeMessage::StopAll);

    // Annotation in progress: refused with the annotation's own notice.
    send(&mut state, AnnotateMessage::Start);
    assert!(state.annotation_running());
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert!(matches!(state.notice, Some(AppNotice::Annotate(_))));
    send(&mut state, AnnotateMessage::Cancel);
}

#[test]
fn an_annotation_cannot_start_while_the_engine_plays() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "g1f3", "b8c6"]));
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    send(&mut state, AnnotateMessage::Start);
    assert!(!state.annotation_running());
    assert_eq!(mode_notice(&state), Some(ModeNotice::Conflict));
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
}

#[test]
fn shootout_and_automove_refuse_the_users_trial_mode() {
    let (mut state, run) = setup(Game::new());
    send(&mut state, GameMessage::EnterTrial { null_move: false });
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert_eq!(mode_notice(&state), Some(ModeNotice::TrialMode));
    mode(&mut state, ModeMessage::ToggleAutomove(run));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    let a = state.mode_availability(run);
    assert!(!a.finish_game && !a.training);
}

#[test]
fn a_locked_analysis_refuses_the_shootout() {
    let (mut state, run) = setup(Game::new());
    state.engine_runs.set_locked(run, true);
    let effects = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert!(last_search(&effects).is_none());
}

// ---- automove ----

fn thinking(state: &AppState, run: RunId) -> Option<AutomoveState> {
    match state.engine_mode(run) {
        EngineMode::Automove { state, .. } => Some(state),
        _ => None,
    }
}

#[test]
fn automove_answers_at_once_and_after_every_move_of_the_user() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    assert_eq!(thinking(&state, run), Some(AutomoveState::EngineThinking));
    let request = last_search(&effects).expect("search");
    assert_eq!(request.limits.movetime, Some(Duration::from_millis(3000)));
    assert!(!request.limits.infinite);

    answer(
        &mut state,
        run,
        &effects,
        100,
        Some(uci_move(&request, "e2e4")),
    );
    assert_eq!(ply(&state), 1);
    assert_eq!(thinking(&state, run), Some(AutomoveState::UserToMove));

    // The user answers; the engine thinks again, from the new position.
    let effects = play(&mut state, "e7e5");
    assert_eq!(thinking(&state, run), Some(AutomoveState::EngineThinking));
    let request = last_search(&effects).expect("second search");
    assert_eq!(request.position.moves.len(), 2);
    answer(
        &mut state,
        run,
        &effects,
        101,
        Some(uci_move(&request, "g1f3")),
    );
    assert_eq!(ply(&state), 3);
    assert_eq!(thinking(&state, run), Some(AutomoveState::UserToMove));
}

#[test]
fn automove_uses_the_time_of_the_dialog_clamped() {
    let (mut state, run) = setup(Game::new());
    mode(
        &mut state,
        ModeMessage::SetAutomoveTime(run, Duration::from_millis(10)),
    );
    assert_eq!(state.automove_time(), Duration::from_millis(100));
    mode(
        &mut state,
        ModeMessage::SetAutomoveTime(run, Duration::from_secs(9999)),
    );
    assert_eq!(state.automove_time(), Duration::from_secs(300));
    mode(
        &mut state,
        ModeMessage::SetAutomoveTime(run, Duration::from_millis(1500)),
    );
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    assert_eq!(
        last_search(&effects).expect("search").limits.movetime,
        Some(Duration::from_millis(1500))
    );
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::Automove {
            thinking_ms: 1500,
            ..
        }
    ));
}

#[test]
fn navigation_cancels_the_pending_move_of_the_engine() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    let request = last_search(&effects).expect("search");
    answer(
        &mut state,
        run,
        &effects,
        100,
        Some(uci_move(&request, "e2e4")),
    );
    let effects = play(&mut state, "e7e5");
    let request = last_search(&effects).expect("search");
    let late = uci_move(&request, "g1f3");
    let cancel = send(&mut state, GameMessage::Navigate(Nav::Back(1)));
    assert!(stopped(&cancel), "the search is stopped");
    assert_eq!(thinking(&state, run), Some(AutomoveState::UserToMove));
    // The answer of the cancelled search is not played.
    answer(&mut state, run, &effects, 101, Some(late));
    assert_eq!(ply(&state), 1);
    // The user plays again: the cycle starts again.
    // The move of the game: it advances, and that is a move of the user.
    let effects = play(&mut state, "e7e5");
    assert!(last_search(&effects).is_some());
    assert_eq!(thinking(&state, run), Some(AutomoveState::EngineThinking));
}

#[test]
fn an_answer_for_an_unknown_search_is_not_played() {
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    let request = last_search(&effects).expect("search");
    let mv = uci_move(&request, "e2e4");
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search: SearchId(555),
            mv: Some(mv),
            ponder: None,
        }),
    );
    assert_eq!(ply(&state), 0);
    assert_eq!(thinking(&state, run), Some(AutomoveState::EngineThinking));
}

#[test]
fn automove_asks_again_after_a_second_and_gives_up_after_ten_tries() {
    let (mut state, run) = setup(Game::new());
    let mut effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    let mut ticks = 0;
    for n in 0..20_u64 {
        effects = answer(&mut state, run, &effects, 100 + n, None);
        let tick = effects.iter().find_map(|e| match e {
            Effect::ScheduleModeTick {
                run: r,
                generation,
                delay,
            } => Some((*r, *generation, *delay)),
            _ => None,
        });
        let Some((r, generation, delay)) = tick else {
            break;
        };
        ticks += 1;
        assert_eq!((r, delay), (run, Duration::from_secs(1)));
        // A stale tick does nothing.
        let stale = mode(
            &mut state,
            ModeMessage::ModeTick {
                run,
                generation: generation.wrapping_sub(1),
            },
        );
        assert!(last_search(&stale).is_none());
        effects = mode(&mut state, ModeMessage::ModeTick { run, generation });
        assert!(last_search(&effects).is_some(), "asked again");
    }
    assert_eq!(ticks, 10);
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::Automove, StopReason::NoMove)
    );
}

#[test]
fn automove_ends_when_the_engine_mates_or_the_user_does() {
    let (mut state, run) = setup(game_from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"));
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    play_script(&mut state, run, effects, &["a1a8"]);
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::Automove, StopReason::Checkmate)
    );

    // The user delivers the mate: the engine has nothing to answer.
    let (mut state, run) = setup(game_from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"));
    send(&mut state, GameMessage::Navigate(Nav::Start));
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    // The engine moves the rook somewhere harmless; then black blunders.
    let effects = play_script(&mut state, run, effects, &["a1a2"]);
    assert!(last_search(&effects).is_none());
    play(&mut state, "g8h8");
    drop(play(&mut state, "a2a8"));
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::Automove, StopReason::Checkmate)
    );
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
}

#[test]
fn automove_stops_with_the_button_and_when_the_engine_fails() {
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleAutomove(run));
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    assert!(stopped(&effects));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert!(state.engine_runs.get(run).expect("run").analyzing);

    mode(&mut state, ModeMessage::ToggleAutomove(run));
    let failure = crate::engines::EngineFailure {
        kind: crate::engines::FailureKind::Crashed,
        exit_code: None,
        stderr_tail: Vec::new(),
    };
    state.engine_runs.get_mut(run).expect("run").state = EngineRunState::Failed(failure);
    send(&mut state, GameMessage::Navigate(Nav::Start));
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::Automove, StopReason::EngineError)
    );
}

#[test]
fn automove_works_with_any_engine_run() {
    let (mut state, first) = setup(Game::new());
    let second_id = state
        .engines
        .add(EngineDraft::new("Other", "/other"))
        .expect("add");
    let second = state.engine_runs.begin(second_id, false);
    let r = state.engine_runs.get_mut(second).expect("run");
    r.state = EngineRunState::Running;
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(second));
    assert!(last_search(&effects).is_some());
    assert!(matches!(
        state.engine_mode(second),
        EngineMode::Automove { .. }
    ));
    assert_eq!(
        state.engine_mode(first),
        EngineMode::Idle,
        "only its own section"
    );
}

#[test]
fn modes_ignore_a_run_that_is_not_ready() {
    let (mut state, _) = setup(Game::new());
    let ghost = RunId(9999);
    for message in [
        ModeMessage::ToggleThreat(ghost),
        ModeMessage::ToggleFinishGame(ghost),
        ModeMessage::ToggleAutomove(ghost),
    ] {
        drop(mode(&mut state, message));
        assert_eq!(state.engine_mode(ghost), EngineMode::Idle);
    }
    assert!(!state.session.in_trial());
    assert_eq!(
        state.mode_availability(ghost),
        crate::training::ModeAvailability::default()
    );
}

#[test]
fn an_illegal_move_notice_is_not_left_by_a_refused_engine_move() {
    // The engine answers with a move that is not legal in the position.
    let (mut state, run) = setup(Game::new());
    let effects = mode(&mut state, ModeMessage::ToggleAutomove(run));
    let other = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").expect("fen");
    let foreign = other.parse_uci("a1a8").expect("uci");
    answer(&mut state, run, &effects, 100, Some(foreign));
    assert_eq!(ply(&state), 0, "nothing was played");
    assert_eq!(thinking(&state, run), Some(AutomoveState::EngineThinking));
}

// ---- round 2: lost searches, watchdog, stale reports, exclusions ----

fn mode_ticks(effects: &[Effect]) -> Vec<(RunId, u64, Duration)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ScheduleModeTick {
                run,
                generation,
                delay,
            } => Some((*run, *generation, *delay)),
            _ => None,
        })
        .collect()
}

fn refused_go() -> EngineEvent {
    EngineEvent::Warning(EngineWarning::CommandRefused {
        command: "go",
        reason: "bad".to_owned(),
    })
}

fn search_failed(state: &mut AppState, run: RunId, error: crate::engines::CommandError) {
    send(
        state,
        crate::engines::EngineMessage::CommandFailed {
            run,
            kind: crate::engines::CommandKind::Search,
            error,
        },
    );
}

#[test]
fn a_refused_go_is_asked_again_once_in_the_shootout_and_then_fails() {
    let (mut state, run) = setup(Game::new());
    let first = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert!(last_search(&first).is_some());
    let again = event(&mut state, run, refused_go());
    assert!(last_search(&again).is_some(), "asked once more");
    event(&mut state, run, refused_go());
    assert_eq!(state.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::ShootOut, StopReason::EngineError)
    );
}

#[test]
fn a_busy_engine_follows_the_retry_table_in_both_modes() {
    use crate::engines::CommandError;
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    search_failed(&mut state, run, CommandError::Busy);
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
    search_failed(&mut state, run, CommandError::Busy);
    assert_eq!(state.engine_mode(run), EngineMode::Idle);

    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleAutomove(run));
    let mut asked = 1;
    for _ in 0..30 {
        search_failed(&mut state, run, CommandError::Busy);
        let fx = match mode_ticks_of(&state, run) {
            Some(generation) => mode(&mut state, ModeMessage::ModeTick { run, generation }),
            None => break,
        };
        if last_search(&fx).is_some() {
            asked += 1;
        }
    }
    assert_eq!(asked, 11, "the first ask plus ten retries");
    assert_eq!(
        mode_notice(&state),
        ended(ModeKind::Automove, StopReason::EngineError)
    );
}

/// The generation of the retry timer of the automove, while it is on.
fn mode_ticks_of(state: &AppState, run: RunId) -> Option<u64> {
    match state.engine_mode(run) {
        EngineMode::Automove { generation, .. } => Some(generation),
        _ => None,
    }
}

#[test]
fn a_dead_engine_command_is_a_lost_search_not_a_stuck_mode() {
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleAutomove(run));
    let fx = send(
        &mut state,
        crate::engines::EngineMessage::CommandDone {
            run,
            result: Err(crate::engines::CommandError::NotRunning),
        },
    );
    assert_eq!(mode_ticks(&fx).len(), 1, "a retry timer is set");
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::Automove { .. }
    ));
}

#[test]
fn the_watchdog_stops_a_mute_engine_even_if_it_keeps_talking() {
    for shoot in [true, false] {
        let (mut state, run) = setup(Game::new());
        let msg = if shoot {
            ModeMessage::ToggleFinishGame(run)
        } else {
            ModeMessage::ToggleAutomove(run)
        };
        let fx = mode(&mut state, msg);
        let request = last_search(&fx).expect("search");
        let movetime = request.limits.movetime.expect("movetime");
        let ticks = mode_ticks(&fx);
        assert_eq!(ticks.len(), 1);
        let (_, watch, delay) = ticks[0];
        assert_eq!(delay, movetime + crate::training::SEARCH_STALL_GRACE);
        // The engine reports forever and never answers.
        let search = SearchId(900);
        event(
            &mut state,
            run,
            EngineEvent::SearchTagged {
                search,
                tag: request.tag,
            },
        );
        event(&mut state, run, EngineEvent::SearchStarted(search));
        for _ in 0..5 {
            event(
                &mut state,
                run,
                EngineEvent::Info(mychess_engine::SearchInfo {
                    search,
                    multipv: 1,
                    ..Default::default()
                }),
            );
        }
        let fx = mode(
            &mut state,
            ModeMessage::ModeTick {
                run,
                generation: watch,
            },
        );
        assert!(stopped(&fx), "the engine is told to stop");
        assert_eq!(state.engine_mode(run), EngineMode::Idle);
        assert_eq!(
            mode_notice(&state),
            ended(
                if shoot {
                    ModeKind::ShootOut
                } else {
                    ModeKind::Automove
                },
                StopReason::EngineError
            )
        );
    }
}

#[test]
fn the_watchdog_of_an_answered_search_is_stale() {
    let (mut state, run) = setup(Game::new());
    let fx = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let (_, watch, _) = mode_ticks(&fx)[0];
    let request = last_search(&fx).expect("search");
    let mv = uci_move(&request, "e2e4");
    answer(&mut state, run, &fx, 100, Some(mv));
    mode(
        &mut state,
        ModeMessage::ModeTick {
            run,
            generation: watch,
        },
    );
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
}

#[test]
fn stopping_before_the_search_starts_leaves_the_late_reports_out_of_the_window() {
    for shoot in [true, false] {
        let (mut state, run) = setup(Game::new());
        let msg = if shoot {
            ModeMessage::ToggleFinishGame(run)
        } else {
            ModeMessage::ToggleAutomove(run)
        };
        let fx = mode(&mut state, msg);
        let request = last_search(&fx).expect("search");
        let stop = mode(&mut state, ModeMessage::StopAll);
        assert!(stopped(&stop), "Stop goes out");
        assert_eq!(state.engine_mode(run), EngineMode::Idle);
        let view = state.analysis_view(run).cloned();
        let search = SearchId(950);
        let mv = uci_move(&request, "e2e4");
        event(
            &mut state,
            run,
            EngineEvent::SearchTagged {
                search,
                tag: request.tag,
            },
        );
        event(&mut state, run, EngineEvent::SearchStarted(search));
        event(
            &mut state,
            run,
            EngineEvent::Info(mychess_engine::SearchInfo {
                search,
                multipv: 1,
                depth: Some(5),
                ..Default::default()
            }),
        );
        event(
            &mut state,
            run,
            EngineEvent::BestMove(BestMove {
                search,
                mv: Some(mv),
                ponder: None,
            }),
        );
        assert_eq!(ply(&state), 0, "the late move is not played");
        assert_eq!(state.analysis_view(run).cloned(), view);
    }
}

#[test]
fn stopping_with_the_search_started_drops_its_reports_too() {
    let (mut state, run) = setup(Game::new());
    let fx = mode(&mut state, ModeMessage::ToggleAutomove(run));
    let request = last_search(&fx).expect("search");
    let search = SearchId(960);
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(&mut state, run, EngineEvent::SearchStarted(search));
    mode(&mut state, ModeMessage::StopAll);
    let view = state.analysis_view(run).cloned();
    event(
        &mut state,
        run,
        EngineEvent::Info(mychess_engine::SearchInfo {
            search,
            multipv: 1,
            depth: Some(9),
            ..Default::default()
        }),
    );
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: Some(uci_move(&request, "e2e4")),
            ponder: None,
        }),
    );
    assert_eq!(state.analysis_view(run).cloned(), view);
    assert_eq!(ply(&state), 0);
}

#[test]
fn the_mode_buttons_follow_the_scid_exclusions() {
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let a = state.mode_availability(run);
    assert!(a.finish_game && !a.threat && !a.training, "{a:?}");
    assert!(state.engine_commandeered(run));
    mode(&mut state, ModeMessage::StopAll);

    mode(&mut state, ModeMessage::ToggleAutomove(run));
    let a = state.mode_availability(run);
    assert!(a.training && !a.finish_game, "{a:?}");
    mode(&mut state, ModeMessage::StopAll);

    mode(&mut state, ModeMessage::ToggleThreat(run));
    let a = state.mode_availability(run);
    assert!(a.threat && !a.finish_game && !a.training, "{a:?}");
    assert!(!state.engine_commandeered(run));
}

#[test]
fn multipv_exclusion_and_lock_are_refused_on_the_engine_of_a_mode() {
    use crate::analysis::AnalysisMessage;
    let (mut state, run) = setup(Game::new());
    state
        .engine_runs
        .get_mut(run)
        .expect("run")
        .identity
        .as_mut()
        .expect("identity")
        .capabilities
        .multipv = Some(1..=5);
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let fx = send(&mut state, AnalysisMessage::SetMultiPv(run, 3));
    assert!(commands(&fx).is_empty(), "no command: {fx:?}");
    send(&mut state, AnalysisMessage::ToggleLock(run));
    assert!(!state.engine_runs.get(run).expect("run").locked);
    send(&mut state, AnalysisMessage::OpenExclude(run));
    assert!(!state.analysis_view(run).is_some_and(|v| v.exclude_open));
}

#[test]
fn the_threat_gives_the_engine_back_as_it_was() {
    let (mut state, run) = setup(Game::new());
    state.engine_runs.get_mut(run).expect("run").analyzing = false;
    mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(state.engine_runs.get(run).expect("run").analyzing);
    let fx = mode(&mut state, ModeMessage::ToggleThreat(run));
    assert!(!state.engine_runs.get(run).expect("run").analyzing);
    assert!(stopped(&fx), "an engine that was idle is stopped");
}

#[test]
fn an_illegal_warning_before_the_search_began_is_not_this_searchs() {
    let (mut state, run) = setup(Game::new());
    let fx = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    event(
        &mut state,
        run,
        EngineEvent::Warning(EngineWarning::IllegalBestMove("old".to_owned())),
    );
    let request = last_search(&fx).expect("search");
    let mv = uci_move(&request, "e2e4");
    answer(&mut state, run, &fx, 100, Some(mv));
    assert_eq!(ply(&state), 1, "the legal move was played all the same");
}

#[test]
fn a_move_of_the_engine_waits_while_the_user_answers_a_question() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5"]));
    send(&mut state, GameMessage::Navigate(Nav::Start));
    let fx = mode(&mut state, ModeMessage::ToggleAutomove(run));
    let request = last_search(&fx).expect("search");
    // The user plays another move in the middle of the line: the question.
    play(&mut state, "d2d4");
    assert!(state.session.prompt().is_some(), "the replace question");
    let before = state.session.game().clone();
    let search = SearchId(970);
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(&mut state, run, EngineEvent::SearchStarted(search));
    let legal = uci_move(&request, "e2e4");
    event(
        &mut state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv: Some(legal),
            ponder: None,
        }),
    );
    assert!(state.session.prompt().is_some(), "the question stays open");
    assert!(state.session.game().same_content(&before));
}

#[test]
fn edits_that_skip_the_funnel_are_refused_during_the_shootout() {
    let (mut state, run) = setup(Game::new());
    let fx = mode(&mut state, ModeMessage::ToggleFinishGame(run));
    let fx = play_script(&mut state, run, fx, &["e2e4", "e7e5"]);
    assert_eq!(ply(&state), 2);
    let mut sink = Vec::new();
    state.apply_game(GameMessage::PlayNullMove, &mut sink);
    assert_eq!(mode_notice(&state), Some(ModeNotice::ShootOutBusy));
    assert_eq!(ply(&state), 2, "nothing was added");
    assert!(matches!(
        state.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
    // The engine goes on, and one Undo takes the whole game back.
    play_script(&mut state, run, fx, &["g1f3"]);
    mode(&mut state, ModeMessage::StopAll);
    assert_eq!(ply(&state), 3);
    send(&mut state, GameMessage::Undo);
    assert_eq!(ply(&state), 0, "one undo for the whole game of the engine");
}

#[test]
fn a_tournament_does_not_start_during_a_shootout() {
    // Regression (D1311).
    let (mut state, run) = setup(Game::new());
    mode(&mut state, ModeMessage::ToggleFinishGame(run));
    assert!(state.training_playing());
    let effects = send(
        &mut state,
        crate::tournament::TournamentMessage::Start {
            config: Box::default(),
            at: std::time::Instant::now(),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(
        state.notice,
        Some(AppNotice::Tournament(
            crate::tournament::TournamentNotice::Busy
        ))
    );
}
