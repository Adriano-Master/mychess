//! Contract tests of the mutual exclusion of the modes (spec M4-8, D495,
//! D841, D851, rule 4; Scid `toggleFinishGame` 3231-3233 and
//! `cancelAutoplay` 1866-1868) and of hostile input.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unnecessary_wraps,
    clippy::default_trait_access,
    clippy::assert_is_empty
)]

mod m4_8_common;

use m4_8_common::*;
use mychess_app::{
    AnalysisMessage, AnalysisNotice, AnnotateMessage, AnnotateNotice, AppNotice, EngineMode,
    GameMessage, Message, ModeMessage, ModeNotice,
};
use mychess_core::{Game, Position};
use mychess_engine::{BestMove, EngineEvent, SearchId};
use proptest::prelude::*;

fn shoot(rig: &mut Rig) -> Vec<mychess_app::Effect> {
    let run = rig.run;
    rig.mode(ModeMessage::ToggleFinishGame(run))
}

fn train(rig: &mut Rig) -> Vec<mychess_app::Effect> {
    let run = rig.run;
    rig.mode(ModeMessage::ToggleAutomove(run))
}

fn threat(rig: &mut Rig) -> Vec<mychess_app::Effect> {
    let run = rig.run;
    rig.mode(ModeMessage::ToggleThreat(run))
}

fn is_shoot(rig: &Rig) -> bool {
    matches!(rig.state.engine_mode(rig.run), EngineMode::ShootOut { .. })
}

fn is_auto(rig: &Rig) -> bool {
    matches!(rig.state.engine_mode(rig.run), EngineMode::Automove { .. })
}

fn conflict() -> Option<ModeNotice> {
    Some(ModeNotice::Conflict)
}

#[test]
fn starting_the_automove_turns_the_shootout_off_and_says_so() {
    let mut rig = Rig::new(Game::new());
    shoot(&mut rig);
    train(&mut rig);
    assert!(is_auto(&rig));
    assert!(!is_shoot(&rig));
    assert_eq!(rig.mode_notice(), conflict());
}

#[test]
fn starting_the_shootout_turns_the_automove_off_and_says_so() {
    let mut rig = Rig::new(Game::new());
    train(&mut rig);
    shoot(&mut rig);
    assert!(is_shoot(&rig));
    assert!(!is_auto(&rig));
    assert_eq!(rig.mode_notice(), conflict());
}

#[test]
fn a_cancelled_mode_does_not_play_a_late_best_move() {
    let mut rig = Rig::new(Game::new());
    let fx = shoot(&mut rig);
    let request = last_search(&fx).expect("search");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(600),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(600)));
    train(&mut rig);
    rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(600),
        mv: Some(mv(&Position::new(), "e2e4")),
        ponder: None,
    }));
    assert!(
        rig.mainline().is_empty(),
        "the old shoot-out's move is dropped"
    );
}

#[test]
fn starting_the_shootout_while_the_threat_is_on_leaves_the_threat_and_says_so() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    threat(&mut rig);
    assert!(rig.state.threat_active());
    shoot(&mut rig);
    assert!(!rig.state.threat_active());
    assert!(is_shoot(&rig));
    assert!(!rig.state.session.in_trial());
    assert_eq!(rig.mode_notice(), conflict());
}

#[test]
fn starting_the_automove_while_the_threat_is_on_leaves_the_threat_and_says_so() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    threat(&mut rig);
    train(&mut rig);
    assert!(!rig.state.threat_active());
    assert!(is_auto(&rig));
    assert_eq!(rig.mode_notice(), conflict());
}

#[test]
fn the_threat_takes_over_from_a_shootout() {
    let mut rig = Rig::new(Game::new());
    shoot(&mut rig);
    threat(&mut rig);
    assert!(!is_shoot(&rig));
    assert!(rig.state.threat_active());
    assert_eq!(rig.mode_notice(), conflict());
}

#[test]
fn starting_a_mode_stops_the_autoplay() {
    for which in 0..2 {
        let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3"]));
        rig.game_msg(GameMessage::StartAutoplay);
        assert!(rig.state.session.autoplay().running);
        if which == 0 {
            shoot(&mut rig);
        } else {
            train(&mut rig);
        }
        assert!(!rig.state.session.autoplay().running, "mode {which}");
        assert_eq!(rig.mode_notice(), conflict(), "mode {which}");
    }
}

#[test]
fn the_autoplay_is_refused_while_a_mode_plays() {
    for which in 0..2 {
        let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3"]));
        if which == 0 {
            shoot(&mut rig);
        } else {
            train(&mut rig);
        }
        for message in [GameMessage::StartAutoplay, GameMessage::ToggleAutoplay] {
            rig.game_msg(message);
            assert!(!rig.state.session.autoplay().running, "mode {which}");
        }
        assert!(is_shoot(&rig) || is_auto(&rig), "the mode keeps running");
    }
}

#[test]
fn a_running_annotation_refuses_every_mode_with_busy() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3", "b8c6"]));
    rig.send(Message::Annotate(AnnotateMessage::Start));
    assert!(rig.state.annotation_running());
    for which in 0..3 {
        rig.state.notice = None;
        match which {
            0 => shoot(&mut rig),
            1 => train(&mut rig),
            _ => threat(&mut rig),
        };
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::Annotate(AnnotateNotice::Busy)),
            "mode {which}"
        );
        assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
        assert!(!rig.state.session.in_trial());
        let a = rig.state.mode_availability(rig.run);
        assert!(!a.threat && !a.finish_game && !a.training, "{a:?}");
    }
}

#[test]
fn starting_an_annotation_with_a_mode_on_reports_a_conflict_and_never_both_run() {
    for which in 0..2 {
        let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3", "b8c6"]));
        if which == 0 {
            shoot(&mut rig);
        } else {
            train(&mut rig);
        }
        rig.send(Message::Annotate(AnnotateMessage::Start));
        assert_eq!(rig.mode_notice(), conflict(), "mode {which}");
        let both = rig.state.annotation_running()
            && (is_shoot(&rig) || is_auto(&rig) || rig.state.threat_active());
        assert!(!both, "mode {which}: two sources would edit the game");
    }
}

#[test]
fn a_locked_analysis_refuses_the_three_modes() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    let run = rig.run;
    rig.send(Message::Analysis(AnalysisMessage::ToggleLock(run)));
    for which in 0..3 {
        rig.state.notice = None;
        match which {
            0 => shoot(&mut rig),
            1 => train(&mut rig),
            _ => threat(&mut rig),
        };
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::Analysis(AnalysisNotice::Locked)),
            "mode {which}"
        );
        assert_eq!(rig.state.engine_mode(run), EngineMode::Idle);
    }
    let a = rig.state.mode_availability(run);
    assert!(!a.threat && !a.finish_game && !a.training, "{a:?}");
}

#[test]
fn the_user_trial_mode_refuses_the_shootout_and_the_automove() {
    for which in 0..2 {
        let mut rig = Rig::new(game_of(&["e2e4"]));
        rig.game_msg(GameMessage::EnterTrial { null_move: false });
        let fx = if which == 0 {
            shoot(&mut rig)
        } else {
            train(&mut rig)
        };
        assert_eq!(
            rig.mode_notice(),
            Some(ModeNotice::TrialMode),
            "mode {which}"
        );
        assert!(mode_searches(&fx).is_empty());
        assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
        assert!(rig.state.session.in_trial(), "the trial stays");
        let a = rig.state.mode_availability(rig.run);
        assert!(!a.finish_game && !a.training);
    }
}

#[test]
fn a_running_mode_keeps_its_own_button_enabled() {
    let mut rig = Rig::new(Game::new());
    shoot(&mut rig);
    assert!(rig.state.mode_availability(rig.run).finish_game);
    let mut rig = Rig::new(Game::new());
    train(&mut rig);
    assert!(rig.state.mode_availability(rig.run).training);
}

#[test]
fn stop_all_and_esc_with_nothing_running_do_nothing() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    let fx = rig.mode(ModeMessage::StopAll);
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.notice, None);
    rig.send(Message::Action(mychess_app::Action::ClearMoveEntry));
    assert_eq!(rig.state.notice, None);
}

#[test]
fn stop_all_leaves_the_threat_alone() {
    // StopAll is the Esc of the shoot-out and the automove (D850).
    let mut rig = Rig::new(game_of(&["e2e4"]));
    threat(&mut rig);
    rig.mode(ModeMessage::StopAll);
    assert!(rig.state.threat_active());
}

#[test]
fn the_mode_of_a_section_is_not_shown_in_another_one() {
    let mut rig = Rig::new(Game::new());
    shoot(&mut rig);
    let other = mychess_app::engines::RunId(rig.run.0 + 1000);
    assert_eq!(rig.state.engine_mode(other), EngineMode::Idle);
    assert_eq!(
        rig.state.mode_availability(other),
        mychess_app::ModeAvailability::default()
    );
}

#[test]
fn messages_for_a_run_that_does_not_exist_are_harmless() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    let ghost = mychess_app::engines::RunId(424_242);
    for m in [
        ModeMessage::ToggleThreat(ghost),
        ModeMessage::ToggleFinishGame(ghost),
        ModeMessage::ToggleAutomove(ghost),
        ModeMessage::SetAutomoveTime(ghost, std::time::Duration::from_secs(1)),
        ModeMessage::ModeTick {
            run: ghost,
            generation: 3,
        },
    ] {
        rig.mode(m);
    }
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert!(!rig.state.session.in_trial());
}

#[derive(Debug, Clone)]
enum Op {
    Threat,
    Shoot,
    Train,
    StopAll,
    Esc,
    Tick(u64),
    AnswerLegal,
    AnswerNone,
    AnswerIllegalWarned,
    StrayBestMove(u64),
    UserPlay,
    Back,
    Forward,
    Undo,
    Autoplay,
    Trial,
    SetTime(u64),
    Die,
    NewGame,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Threat),
        Just(Op::Shoot),
        Just(Op::Train),
        Just(Op::StopAll),
        Just(Op::Esc),
        (0u64..40).prop_map(Op::Tick),
        Just(Op::AnswerLegal),
        Just(Op::AnswerNone),
        Just(Op::AnswerIllegalWarned),
        any::<u64>().prop_map(Op::StrayBestMove),
        Just(Op::UserPlay),
        Just(Op::Back),
        Just(Op::Forward),
        Just(Op::Undo),
        Just(Op::Autoplay),
        Just(Op::Trial),
        any::<u64>().prop_map(Op::SetTime),
        Just(Op::Die),
        Just(Op::NewGame),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn no_sequence_of_events_panics_or_leaves_two_modes_on(ops in proptest::collection::vec(op(), 1..60)) {
        let mut rig = Rig::new(Game::new());
        let run = rig.run;
        let mut pending: Vec<mychess_app::Effect> = Vec::new();
        for op in ops {
            let fx = match op {
                Op::Threat => threat(&mut rig),
                Op::Shoot => shoot(&mut rig),
                Op::Train => train(&mut rig),
                Op::StopAll => rig.mode(ModeMessage::StopAll),
                Op::Esc => rig.send(Message::Action(mychess_app::Action::ClearMoveEntry)),
                Op::Tick(g) => rig.mode(ModeMessage::ModeTick { run, generation: g }),
                Op::AnswerLegal => {
                    let legal = last_search(&pending).and_then(|r| position_of(&r).legal_moves().first().copied());
                    rig.answer(&pending, legal).unwrap_or_default()
                }
                Op::AnswerNone => rig.answer(&pending, None).unwrap_or_default(),
                Op::AnswerIllegalWarned => rig.answer_with(&pending, None, true).unwrap_or_default(),
                Op::StrayBestMove(id) => rig.event(EngineEvent::BestMove(BestMove {
                    search: SearchId(id),
                    mv: Position::new().legal_moves().first().copied(),
                    ponder: None,
                })),
                Op::UserPlay => {
                    let legal = rig.position().legal_moves().first().copied();
                    match legal {
                        Some(mv) => rig.game_msg(GameMessage::PlayMove { mv, options: Default::default() }),
                        None => Vec::new(),
                    }
                }
                Op::Back => rig.game_msg(GameMessage::Navigate(mychess_app::Nav::Back(1))),
                Op::Forward => rig.game_msg(GameMessage::Navigate(mychess_app::Nav::Forward(1))),
                Op::Undo => rig.game_msg(GameMessage::Undo),
                Op::Autoplay => rig.game_msg(GameMessage::ToggleAutoplay),
                Op::Trial => rig.game_msg(GameMessage::ToggleTrial { null_move: false }),
                Op::SetTime(ms) => rig.mode(ModeMessage::SetAutomoveTime(run, std::time::Duration::from_millis(ms))),
                Op::Die => rig.event(exited()),
                Op::NewGame => {
                    rig.state.session.replace_game(Game::new());
                    rig.mode(ModeMessage::StopAll)
                }
            };
            if !mode_searches(&fx).is_empty() || !ticks(&fx).is_empty() {
                pending = fx;
            }
            // Invariants: one mode at most; the engine's mode never coexists
            // with the autoplay or the user's own trial mode.
            let on = [rig.state.threat_active(), is_shoot(&rig), is_auto(&rig)]
                .iter()
                .filter(|b| **b)
                .count();
            prop_assert!(on <= 1, "two modes on");
            if is_shoot(&rig) || is_auto(&rig) {
                prop_assert!(!rig.state.session.in_trial());
                prop_assert!(!rig.state.session.autoplay().running);
            }
            if rig.state.threat_active() {
                prop_assert!(rig.state.session.in_trial());
            }
            let time = rig.state.automove_time();
            prop_assert!(time >= std::time::Duration::from_millis(100));
            prop_assert!(time <= std::time::Duration::from_secs(300));
        }
    }
}
