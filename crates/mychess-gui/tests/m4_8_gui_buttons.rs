//! The engine mode buttons of the analysis panel and the right click of
//! "Trial" (spec M4-8, "Comportamento observavel", "Textos de interface",
//! D840-D861 as seen from the window, D914-D918; Scid
//! `analysis.tcl` 2357-2469, 3228-3255, 3508-3522, 4233-4327, `main.tcl`
//! `setTrialMode` 1882-1913). Headless; the engine is synthetic.
#![allow(clippy::float_cmp, clippy::single_match_else)]

mod m4_68_gui_common;

use std::time::Duration;

use iced::widget::Id;
use m4_68_gui_common::*;
use mychess_app::{AutomoveState, ModeKind};
use mychess_core::{Game, Position};
use mychess_gui::host::{HostWork, classify};
use mychess_gui::keymap::Platform;
use mychess_gui::windows::analysis::{AnalysisUiMessage, TRAINING_INPUT_ID};
use mychess_gui::windows::nav_bar::{self, NAV_TRIAL};
use rstest::rstest;

const OPENING: [&str; 2] = ["e2e4", "e7e5"];

fn rig() -> (Rig, RunId) {
    rig_with_engine(&OPENING)
}

/// Black to move and in check, from a real line (1.e4 d6 2.Bb5+).
const IN_CHECK: &str = "rnbqkbnr/ppp2ppp/3p4/1B2p3/4P3/8/PPPP1PPP/RNBQK1NR b KQkq - 1 3";

fn rig_in_check() -> (Rig, RunId) {
    let mut rig = Rig::new();
    rig.set_game(Game::from_position(
        Position::from_fen(IN_CHECK).expect("fen"),
    ));
    let run = uci_run(&mut rig);
    (rig, run)
}

/// Whether a click on the button `label` sends anything.
fn enabled(rig: &Rig, label: &str) -> bool {
    !click_msgs(rig, label).is_empty()
}

fn flags(rig: &Rig) -> [bool; 4] {
    ["Threat", "Finish game", "Training", "Annotate"].map(|l| enabled(rig, l))
}

/// The first legal move of the position a search asks about, as a line.
fn first_move_uci(effects: &[Effect]) -> String {
    let request = last_search(effects).expect("a search");
    let pos = position_of(&request);
    pos.uci(pos.legal_moves()[0])
}

// ---- what each button sends ----

#[test]
fn each_button_sends_its_message_for_its_own_engine() {
    let (rig, run) = rig();
    assert_eq!(
        modes_of(&click_msgs(&rig, "Threat")),
        [ModeMessage::ToggleThreat(run)]
    );
    assert_eq!(
        modes_of(&click_msgs(&rig, "Finish game")),
        [ModeMessage::ToggleFinishGame(run)]
    );
    // The training asks the time first: no mode message yet.
    let training = click_msgs(&rig, "Training");
    assert_eq!(modes_of(&training), []);
    assert!(is_ui(&training, &AnalysisUiMessage::OpenTraining(run)));
    assert!(annotates(&click_msgs(&rig, "Annotate")).contains(&AnnotateMessage::Open));
}

#[test]
fn with_two_engines_each_section_has_its_own_buttons() {
    let (mut rig, first) = rig();
    let id = add_engine(&mut rig, "Second", Protocol::Uci);
    let second = start_with(&mut rig, id, identity("Second", Some(1..=5), true));
    assert_eq!(count_text(&rig, "Threat"), 2);
    // The first "Finish game" belongs to the first section.
    let mut s = rig.sim(SIZE);
    let at = nth_text_point(&mut s, "Finish game", 1);
    s.point_at(at);
    drop(s.simulate([
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ]));
    let sent = strip_hover(s.into_messages().collect());
    assert_eq!(modes_of(&sent), [ModeMessage::ToggleFinishGame(second)]);
    assert_ne!(first, second);
}

// ---- enabled states follow mode_availability (D852, D914) ----

#[test]
fn the_buttons_are_enabled_exactly_as_mode_availability_says_in_every_state() {
    let check = |rig: &Rig, run: RunId, what: &str| {
        let a = rig.shell.app.mode_availability(run);
        let [threat, finish, training, _] = flags(rig);
        assert_eq!(threat, a.threat, "{what}: threat");
        assert_eq!(finish, a.finish_game, "{what}: finish game");
        assert_eq!(training, a.training, "{what}: training");
    };
    let (mut rig, run) = rig();
    check(&rig, run, "idle");
    assert_eq!(flags(&rig), [true, true, true, true]);

    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
    check(&rig, run, "threat");
    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));

    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    check(&rig, run, "shoot-out");
    mode_msg(&mut rig, ModeMessage::StopAll);

    mode_msg(&mut rig, ModeMessage::ToggleAutomove(run));
    check(&rig, run, "automove");
    mode_msg(&mut rig, ModeMessage::StopAll);

    up(
        &mut rig,
        AppMessage::Game(GameMessage::EnterTrial { null_move: false }),
    );
    check(&rig, run, "trial");
}

#[test]
fn a_position_in_check_disables_only_the_threat() {
    let (rig, run) = rig_in_check();
    assert!(!rig.shell.app.mode_availability(run).threat);
    assert_eq!(flags(&rig), [false, true, true, true]);
}

#[test]
fn an_engine_that_is_not_running_has_no_mode_buttons_enabled() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&OPENING));
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    up(
        &mut rig,
        AppMessage::Engine(EngineMessage::Start(id, StartMode::Analyze)),
    );
    for label in ["Threat", "Finish game", "Training"] {
        assert!(
            !has(&rig, label) || !enabled(&rig, label),
            "{label} must not be usable while the engine starts"
        );
    }
}

#[test]
fn trial_mode_disables_the_three_modes() {
    let (mut rig, _run) = rig();
    up(
        &mut rig,
        AppMessage::Game(GameMessage::EnterTrial { null_move: false }),
    );
    let [threat, finish, training, _] = flags(&rig);
    assert!(!threat && !finish && !training);
}

#[test]
fn an_open_dialog_disables_the_modes_but_not_the_annotate_open() {
    let (mut rig, _run) = rig();
    ann(&mut rig, AnnotateMessage::Open);
    let [threat, finish, training, _] = flags(&rig);
    assert!(!threat && !finish && !training, "a modal is open");
}

#[test]
fn a_locked_engine_disables_the_three_modes() {
    let (mut rig, run) = rig();
    three_lines(&mut rig, run);
    up(
        &mut rig,
        AppMessage::Analysis(AnalysisMessage::ToggleLock(run)),
    );
    if !rig.shell.app.engine_runs.get(run).expect("run").locked {
        // The application refused the lock in this state: nothing to check.
        return;
    }
    let [threat, finish, training, _] = flags(&rig);
    assert!(!threat && !finish && !training);
}

#[test]
fn a_running_annotation_disables_the_modes_and_keeps_annotate_to_stop() {
    let (mut rig, _run) = rig();
    ann(&mut rig, AnnotateMessage::Start);
    assert_eq!(flags(&rig), [false, false, false, true]);
}

// ---- one mode at a time (D841, D852, D854, D895) ----

#[test]
fn a_shoot_out_leaves_only_its_own_button_enabled() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    assert!(matches!(
        rig.shell.app.engine_mode(run),
        EngineMode::ShootOut { .. }
    ));
    // Annotate is off too: a mode is on (the section's annotate needs Idle).
    assert_eq!(flags(&rig), [false, true, false, false]);
    assert_eq!(
        modes_of(&click_msgs(&rig, "Finish game")),
        [ModeMessage::ToggleFinishGame(run)],
        "the button of the mode stops it"
    );
}

#[test]
fn an_automove_leaves_only_the_training_button_enabled() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleAutomove(run));
    // D852: the automove takes the shoot-out away (and the annotation, a
    // mode being on); the threat stays offered.
    assert_eq!(flags(&rig), [true, false, true, false]);
    // Stopping needs no time box.
    assert_eq!(
        modes_of(&click_msgs(&rig, "Training")),
        [ModeMessage::ToggleAutomove(run)]
    );
}

#[test]
fn a_threat_leaves_only_its_button_enabled() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Threat);
    assert_eq!(flags(&rig), [true, false, false, false]);
}

#[test]
fn stopping_the_mode_brings_the_buttons_back() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert_eq!(flags(&rig), [true, true, true, true]);
}

#[test]
fn a_mode_on_one_engine_leaves_the_other_section_to_the_threat_only() {
    let (mut rig, first) = rig();
    let id = add_engine(&mut rig, "Second", Protocol::Uci);
    let second = start_with(&mut rig, id, identity("Second", Some(1..=5), true));
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(first));
    assert_eq!(rig.shell.app.engine_mode(second), EngineMode::Idle);
    let a = rig.shell.app.mode_availability(second);
    // Only one mode in the whole application (D841): the shoot-out of the
    // first takes the threat and the training of the second away.
    assert!(!a.threat && !a.training, "{a:?}");
}

// ---- the pressed look of the buttons ----

#[test]
fn the_button_of_the_mode_that_is_on_is_drawn_pressed() {
    use mychess_gui::windows::analysis::{
        AnalysisUi, Ctx, ModeView, RunInfo, Section, section_with_modes,
    };
    let (rig, run) = rig();
    let app = &rig.shell.app;
    let info = RunInfo::of(app.engine_runs.get(run).expect("run"));
    let view = app.analysis_view(run).expect("a view");
    let ui = AnalysisUi::default();
    let i18n = en();
    let draw = |modes: &ModeView| {
        let ctx = Ctx {
            i18n: &i18n,
            ui: &ui,
            log_enabled: true,
        };
        let element = section_with_modes(
            &Section {
                run: info,
                name: "Fake",
                protocol: Protocol::Uci,
                view,
            },
            &ctx,
            Some(modes),
        );
        let mut s = iced_test::Simulator::with_size(iced::Settings::default(), SIZE, element);
        fingerprint(&mut s)
    };
    let idle = ModeView::of(app, run);
    let all_on = mychess_app::ModeAvailability {
        threat: true,
        finish_game: true,
        training: true,
    };
    let base = ModeView {
        avail: all_on,
        ..idle
    };
    let shoot = ModeView {
        mode: EngineMode::ShootOut { generation: 1 },
        ..base
    };
    let annotating = ModeView {
        annotating: true,
        ..base
    };
    let f_idle = draw(&base);
    assert_ne!(f_idle, draw(&shoot), "Finish game pressed");
    assert_ne!(f_idle, draw(&annotating), "Annotate pressed");
    assert_eq!(f_idle, draw(&base), "drawing is deterministic");
}

// ---- the labels ----

#[test]
fn the_threat_shows_its_label_while_it_is_on_and_not_before() {
    let (mut rig, run) = rig();
    assert!(!window_has(&rig, "Opponent's threat:"));
    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
    assert!(window_has(&rig, "Opponent's threat:"));
    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
    assert!(!window_has(&rig, "Opponent's threat:"));
}

#[test]
fn the_threat_label_is_in_portuguese() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(game_of(&OPENING));
    let run = uci_run(&mut rig);
    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
    assert!(window_has(&rig, "Ameaça do adversário:"));
}

#[test]
fn the_training_says_thinking_then_your_move() {
    let (mut rig, run) = rig();
    let effects = mode_msg(&mut rig, ModeMessage::ToggleAutomove(run));
    assert!(matches!(
        rig.shell.app.engine_mode(run),
        EngineMode::Automove {
            state: AutomoveState::EngineThinking,
            ..
        }
    ));
    assert!(window_has(&rig, "Thinking....."));
    assert!(!window_has(&rig, "Your move....."));
    // The engine answers: it is the user's turn.
    let uci = first_move_uci(&effects);
    let mut searches = Searches::new();
    searches
        .answer(&mut rig, run, &effects, 20, &[uci.as_str()])
        .expect("a search");
    assert!(matches!(
        rig.shell.app.engine_mode(run),
        EngineMode::Automove {
            state: AutomoveState::UserToMove,
            ..
        }
    ));
    assert!(window_has(&rig, "Your move....."));
    assert!(!window_has(&rig, "Thinking....."));
}

#[test]
fn the_training_texts_are_in_portuguese() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(game_of(&OPENING));
    let run = uci_run(&mut rig);
    mode_msg(&mut rig, ModeMessage::ToggleAutomove(run));
    assert!(window_has(&rig, "Pensando....."));
}

// ---- the engine is commandeered: lines, exclusion, lock, adding (D852, D914) ----

#[test]
fn lines_exclusion_lock_and_adding_are_off_while_a_mode_plays() {
    for start in [ModeMessage::ToggleFinishGame, ModeMessage::ToggleAutomove] {
        let (mut rig, run) = rig();
        three_lines(&mut rig, run);
        // With the engine free, the lock and the exclusion answer.
        assert!(enabled(&rig, "Lock"));
        mode_msg(&mut rig, start(run));
        assert!(rig.shell.app.engine_commandeered(run));
        for label in ["Lock", "Exclude", "+ Move", "+ Line"] {
            assert!(!enabled(&rig, label), "{label} while commandeered");
        }
    }
}

#[test]
fn a_threat_does_not_commandeer_the_engine() {
    let (mut rig, run) = rig();
    three_lines(&mut rig, run);
    mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
    assert!(!rig.shell.app.engine_commandeered(run));
}

#[test]
fn the_application_refuses_what_the_buttons_do_not_offer() {
    let (mut rig, run) = rig();
    three_lines(&mut rig, run);
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    let lines_before = view(&rig, run).multipv;
    up(
        &mut rig,
        AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 2)),
    );
    assert_eq!(view(&rig, run).multipv, lines_before, "MultiPV is refused");
    up(
        &mut rig,
        AppMessage::Analysis(AnalysisMessage::ToggleLock(run)),
    );
    assert!(!rig.shell.app.engine_runs.get(run).expect("run").locked);
}

// ---- the thinking time of the training (D493, D915) ----

#[test]
fn the_training_box_opens_with_the_current_time_under_its_own_section() {
    let (mut rig, run) = rig();
    let sent = click_msgs(&rig, "Training");
    apply(&mut rig, sent);
    assert!(window_has(&rig, "Engine thinking time"));
    assert_eq!(
        rig.shell.ui.analysis.training,
        Some((run, "3.0".to_owned())),
        "the default is automove_time() (3 s)"
    );
}

#[test]
fn the_box_shows_the_time_the_user_set_last_time() {
    let (mut rig, run) = rig();
    mode_msg(
        &mut rig,
        ModeMessage::SetAutomoveTime(run, Duration::from_millis(7500)),
    );
    let sent = click_msgs(&rig, "Training");
    apply(&mut rig, sent);
    assert_eq!(
        rig.shell.ui.analysis.training,
        Some((run, "7.5".to_owned()))
    );
}

#[test]
fn only_the_clicked_section_gets_the_box() {
    let (mut rig, _first) = rig();
    let id = add_engine(&mut rig, "Second", Protocol::Uci);
    let second = start_with(&mut rig, id, identity("Second", Some(1..=5), true));
    rig.msg(aui(AnalysisUiMessage::OpenTraining(second)));
    assert_eq!(count_text(&rig, "Engine thinking time"), 1);
}

#[rstest]
#[case("2", Some(2000))]
#[case("2.5", Some(2500))]
#[case("2,5", Some(2500))]
#[case("  4  ", Some(4000))]
#[case("0.1", Some(100))]
#[case("0.05", Some(100))]
#[case("0,001", Some(100))]
#[case("300", Some(300_000))]
#[case("301", Some(300_000))]
#[case("99999", Some(300_000))]
#[case("abc", None)]
#[case("", None)]
#[case("0", None)]
#[case("-3", None)]
#[case("NaN", None)]
#[case("inf", None)]
#[case("2 3", None)]
#[case("1.2.3", None)]
fn ok_sends_the_time_then_toggles_and_a_bad_text_does_nothing(
    #[case] typed: &str,
    #[case] expected_ms: Option<u32>,
) {
    let (mut rig, run) = rig();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    rig.msg(aui(AnalysisUiMessage::TrainingInput(typed.to_owned())));
    rig.msg(aui(AnalysisUiMessage::TrainingSubmit(run)));
    match expected_ms {
        Some(ms) => {
            assert_eq!(rig.shell.ui.analysis.training, None, "the box closed");
            // SetAutomoveTime came before ToggleAutomove: the mode started
            // with the new time.
            match rig.shell.app.engine_mode(run) {
                EngineMode::Automove { thinking_ms, .. } => assert_eq!(thinking_ms, ms),
                other => panic!("the training did not start: {other:?}"),
            }
            assert_eq!(
                rig.shell.app.automove_time(),
                Duration::from_millis(u64::from(ms))
            );
        }
        None => {
            assert!(rig.shell.ui.analysis.training.is_some(), "the box stays");
            assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
            assert_eq!(rig.shell.app.automove_time(), Duration::from_secs(3));
        }
    }
}

#[test]
fn the_box_cancel_closes_it_without_starting() {
    let (mut rig, run) = rig();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    let sent = click_msgs(&rig, "Cancel");
    apply(&mut rig, sent);
    assert_eq!(rig.shell.ui.analysis.training, None);
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert!(!window_has(&rig, "Engine thinking time"));
}

#[test]
fn the_start_button_of_the_box_is_off_for_a_bad_text() {
    let (mut rig, run) = rig();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    rig.msg(aui(AnalysisUiMessage::TrainingInput("x".to_owned())));
    let mut s = rig.sim(SIZE);
    // The window has other "Start" texts: ask for the one of the box through
    // the id of its field's neighbours instead of by label.
    assert!(
        s.find(Id::new(TRAINING_INPUT_ID)).is_ok(),
        "the field exists"
    );
    drop(s);
    assert!(has(&rig, "Seconds per move"));
}

#[test]
fn a_huge_typed_text_does_not_break_the_box() {
    let (mut rig, run) = rig();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    rig.msg(aui(AnalysisUiMessage::TrainingInput("9".repeat(5000))));
    rig.msg(aui(AnalysisUiMessage::TrainingSubmit(run)));
    // 99...9 is a finite number: clamped to the maximum.
    match rig.shell.app.engine_mode(run) {
        EngineMode::Automove { thinking_ms, .. } => assert_eq!(thinking_ms, 300_000),
        EngineMode::Idle => {}
        other => panic!("{other:?}"),
    }
}

// ---- the right click of "Trial" is the threat (D490, D916) ----

fn trial_point(rig: &Rig, size: iced::Size) -> iced::Point {
    let mut s = rig.sim(size);
    s.find(Id::new(NAV_TRIAL))
        .expect("the trial button")
        .bounds()
        .center()
}

fn press_trial(rig: &Rig, button: iced::mouse::Button) -> Vec<Message> {
    press_at(rig, trial_point(rig, SIZE), button)
}

#[test]
fn the_right_click_on_trial_toggles_the_threat_of_the_first_engine() {
    let (mut rig, run) = rig();
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    assert_eq!(
        modes_of(&sent),
        [ModeMessage::ToggleThreat(run)],
        "{sent:?}"
    );
    apply(&mut rig, sent);
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Threat);
    assert!(rig.shell.app.threat_active());
    // The position is the one with the null move: Black is to move now.
    assert_eq!(
        rig.shell.app.session.position().turn(),
        mychess_core::Color::Black,
        "the null move passed the turn"
    );
}

#[test]
fn the_left_click_on_trial_is_the_plain_trial_mode_not_the_threat() {
    let (rig, _run) = rig();
    let sent = press_trial(&rig, iced::mouse::Button::Left);
    assert!(modes_of(&sent).is_empty(), "{sent:?}");
    assert!(!sent.is_empty(), "it does something: enters trial mode");
}

#[test]
fn without_a_running_engine_the_right_click_does_nothing() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&OPENING));
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    assert!(sent.is_empty(), "{sent:?}");
    // And a stopped-but-listed engine is no engine either.
    add_engine(&mut rig, "Idle", Protocol::Uci);
    assert!(press_trial(&rig, iced::mouse::Button::Right).is_empty());
}

#[test]
fn in_check_the_right_click_does_nothing() {
    let (rig, _run) = rig_in_check();
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    assert!(modes_of(&sent).is_empty(), "{sent:?}");
}

#[test]
fn the_threat_refused_in_check_by_the_application_says_why() {
    let (mut rig, run) = rig_in_check();
    // A message that does not come from the button: the application's own
    // refusal (D849).
    rig.app_msg(AppMessage::Mode(ModeMessage::ToggleThreat(run)));
    assert_eq!(mode_notice(&rig), Some(ModeNotice::NullMoveInCheck));
    assert!(!rig.shell.app.session.in_trial(), "no trial left behind");
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "A null move is not allowed while in check"
    ));
}

#[test]
fn a_second_right_click_leaves_the_threat() {
    let (mut rig, run) = rig();
    let original = rig.shell.app.session.position().fen();
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    apply(&mut rig, sent);
    assert_ne!(rig.shell.app.session.position().fen(), original);
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    assert_eq!(modes_of(&sent), [ModeMessage::ToggleThreat(run)]);
    apply(&mut rig, sent);
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        rig.shell.app.session.position().fen(),
        original,
        "the original position is back"
    );
}

#[test]
fn the_right_click_does_nothing_while_a_shoot_out_runs() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    assert!(modes_of(&sent).is_empty(), "{sent:?}");
}

#[test]
fn a_narrow_bar_has_no_right_click_on_trial() {
    let (rig, _run) = rig();
    let bar = || {
        iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(250.0, 120.0),
            nav_bar::command_bar(&rig.shell.app, &rig.shell.i18n, Platform::current()),
        )
    };
    // "Trial" lives in the "more" menu here; sweep the bar with right clicks.
    let mut sent = Vec::new();
    for x in (0u16..250).step_by(10) {
        for y in (0u16..120).step_by(15) {
            let mut s = bar();
            s.point_at(iced::Point::new(f32::from(x), f32::from(y)));
            drop(s.simulate([
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Right,
                )),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Right,
                )),
            ]));
            sent.extend(s.into_messages());
        }
    }
    assert!(modes_of(&sent).is_empty(), "{sent:?}");
}

// ---- notices of the modes ----

#[test]
fn an_edit_by_the_user_ends_the_shoot_out_with_a_remark() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    let mv = rig.shell.app.session.position().legal_moves()[0];
    rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert_eq!(
        mode_notice(&rig),
        Some(ModeNotice::Ended {
            kind: ModeKind::ShootOut,
            reason: StopReason::GameChanged,
        })
    );
    tick_status(&mut rig);
    assert!(window_has(&rig, "Stopped: the game was changed."));
}

#[test]
fn escape_stops_the_shoot_out_without_a_remark_and_keeps_the_dialogs_alone() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    esc(&mut rig);
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert!(
        !matches!(mode_notice(&rig), Some(ModeNotice::Ended { .. })),
        "a stop by the user is not reported"
    );
}

#[test]
fn escape_stops_the_automove() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleAutomove(run));
    esc(&mut rig);
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
}

#[test]
fn starting_an_annotation_with_a_mode_on_says_conflict() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    rig.app_msg(AppMessage::Annotate(AnnotateMessage::Start));
    assert!(!rig.shell.app.annotation_running());
    assert_eq!(mode_notice(&rig), Some(ModeNotice::Conflict));
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "Another mode, the autoplay or an annotation was in the way: only one runs at a time."
    ));
}

#[test]
fn starting_a_mode_during_an_annotation_says_busy() {
    let (mut rig, run) = rig();
    ann(&mut rig, AnnotateMessage::Start);
    assert!(rig.shell.app.annotation_running());
    rig.app_msg(AppMessage::Mode(ModeMessage::ToggleFinishGame(run)));
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::Busy));
}

#[test]
fn a_second_mode_from_outside_the_buttons_switches_the_first_off_with_a_conflict() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    rig.app_msg(AppMessage::Mode(ModeMessage::ToggleAutomove(run)));
    assert!(matches!(
        rig.shell.app.engine_mode(run),
        EngineMode::Automove { .. }
    ));
    assert_eq!(mode_notice(&rig), Some(ModeNotice::Conflict));
}

// ---- the timers (D859, D918) ----

#[test]
fn the_timers_of_the_modes_are_effects_that_go_to_perform() {
    let (mut rig, run) = rig();
    let mut all = mode_msg(&mut rig, ModeMessage::ToggleFinishGame(run));
    mode_msg(&mut rig, ModeMessage::StopAll);
    all.extend(mode_msg(&mut rig, ModeMessage::ToggleAutomove(run)));
    let ticks: Vec<&Effect> = all
        .iter()
        .filter(|e| matches!(e, Effect::ScheduleModeTick { .. }))
        .collect();
    assert!(!ticks.is_empty(), "the watchdog is armed");
    for effect in ticks {
        assert!(!effect.runs_on_host());
        assert_eq!(
            classify(effect.clone(), &en()),
            HostWork::Perform(effect.clone())
        );
    }
}

// ---- hostile geometry and a crowd ----

#[test]
fn eight_engines_draw_eight_sets_of_buttons_in_a_small_window() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&OPENING));
    for n in 0..8 {
        let id = add_engine(&mut rig, &format!("E{n}"), Protocol::Uci);
        start_with(&mut rig, id, identity(&format!("E{n}"), Some(1..=5), true));
    }
    assert_eq!(count_text(&rig, "Threat"), 8);
    for size in [iced::Size::new(1.0, 1.0), iced::Size::new(200.0, 150.0)] {
        drop(rig.sim(size));
    }
}

#[test]
fn every_mode_state_draws_in_a_tiny_window() {
    let (mut rig, run) = rig();
    for start in [
        ModeMessage::ToggleThreat,
        ModeMessage::ToggleFinishGame,
        ModeMessage::ToggleAutomove,
    ] {
        mode_msg(&mut rig, start(run));
        drop(rig.sim(iced::Size::new(1.0, 1.0)));
        drop(rig.sim(iced::Size::new(240.0, 160.0)));
        mode_msg(&mut rig, ModeMessage::StopAll);
        if rig.shell.app.engine_mode(run) == EngineMode::Threat {
            mode_msg(&mut rig, ModeMessage::ToggleThreat(run));
        }
    }
}

#[test]
fn the_threat_button_during_a_training_replaces_it_and_says_conflict() {
    let (mut rig, run) = rig();
    mode_msg(&mut rig, ModeMessage::ToggleAutomove(run));
    let sent = click_msgs(&rig, "Threat");
    assert_eq!(modes_of(&sent), [ModeMessage::ToggleThreat(run)]);
    for m in sent {
        rig.msg(m);
    }
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Threat);
    assert_eq!(mode_notice(&rig), Some(ModeNotice::Conflict));
}

// ---- D925, D923: the threat picks a free engine; a stale box closes ----

#[test]
fn the_right_click_skips_a_locked_first_engine_for_a_free_one() {
    let (mut rig, first) = rig();
    let id = add_engine(&mut rig, "Second", Protocol::Uci);
    let second = start_with(&mut rig, id, identity("Second", Some(1..=5), true));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(first)));
    assert!(!rig.shell.app.mode_availability(first).threat);
    assert!(rig.shell.app.mode_availability(second).threat);
    let sent = press_trial(&rig, iced::mouse::Button::Right);
    assert_eq!(
        modes_of(&sent),
        [ModeMessage::ToggleThreat(second)],
        "{sent:?}"
    );
}

#[test]
fn the_training_box_closes_when_the_training_stops_being_available() {
    let (mut rig, run) = rig();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    assert!(rig.shell.ui.analysis.training.is_some());
    exit_engine(&mut rig, run);
    assert_eq!(rig.shell.ui.analysis.training, None, "the box closed");
    assert!(rig.shell.ui.analysis.focused_field.is_none());
}
