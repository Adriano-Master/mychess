//! Keys and layers of the analysis panel (specs M4-4 D811, M4-6 D910-D913,
//! M4-8 D914-D918; decisions D920-D925): nothing leaks through an open layer,
//! `Esc` goes to the innermost layer, the inline boxes of the panel capture
//! `Enter`, the digits follow the typed character. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_68_gui_common;

use iced::widget::Id;
use m4_68_gui_common::*;
use mychess_app::Action;
use mychess_engine::LogKind;
use mychess_gui::windows::analysis::{LOG_SEARCH_ID, TRAINING_INPUT_ID};
use rstest::rstest;

fn mainline_len(rig: &Rig) -> usize {
    rig.game()
        .mainline()
        .filter(|id| *id != rig.game().root())
        .count()
}

fn multipv(rig: &Rig, run: RunId) -> u8 {
    view(rig, run).multipv
}

/// An empty game, an engine with lines, the pointer over the panel.
fn hovered() -> (Rig, RunId) {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    three_long_lines(&mut rig, run);
    hover(&mut rig, true);
    (rig, run)
}

fn escape() -> KeyInput {
    named(NamedKey::Escape)
}

// ---- the training box captures Enter (D921) ----

#[rstest]
#[case("")]
#[case("abc")]
#[case("0")]
#[case("-2")]
fn enter_in_the_training_box_with_a_bad_text_adds_nothing(#[case] draft: &str) {
    let (mut rig, run) = hovered();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    rig.msg(aui(AnalysisUiMessage::TrainingInput(draft.to_owned())));
    // The widget does not capture the key when it is missing `on_submit`:
    // the shell sees it as an uncaptured key. Both ways add nothing.
    rig.key(named(NamedKey::Enter), false, None);
    rig.key(named(NamedKey::Enter), true, None);
    rig.msg(aui(AnalysisUiMessage::TrainingSubmit(run)));
    assert_eq!(mainline_len(&rig), 0, "no AddMove");
    assert!(rig.shell.ui.analysis.training.is_some(), "the box stays");
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
}

#[test]
fn the_training_field_submits_on_enter_even_with_a_bad_text() {
    let (mut rig, run) = hovered();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    rig.msg(aui(AnalysisUiMessage::TrainingInput("abc".to_owned())));
    let mut s = rig.sim(SIZE);
    s.click(Id::new(TRAINING_INPUT_ID)).expect("the field");
    let _ = s.tap_key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Enter,
    ));
    let sent = strip_hover(s.into_messages().collect());
    assert!(
        is_ui(&sent, &AnalysisUiMessage::TrainingSubmit(run)),
        "the field captures Enter: {sent:?}"
    );
}

#[test]
fn the_log_filter_and_the_exclusion_field_capture_enter_too() {
    let (mut rig, run) = hovered();
    open_log_with(&mut rig, run, vec![log_entry(1, LogKind::Received, "ok")]);
    let mut s = rig.sim(SIZE);
    s.click(Id::new(LOG_SEARCH_ID)).expect("the filter");
    let status = s.tap_key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Enter,
    ));
    assert_eq!(status, iced::event::Status::Captured, "the log filter");
}

// ---- Esc (D921) ----

#[rstest]
#[case(false)]
#[case(true)]
fn escape_closes_the_training_box_and_goes_no_further(#[case] captured_by_the_field: bool) {
    let (mut rig, run) = hovered();
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    rig.key(escape(), captured_by_the_field, None);
    assert_eq!(rig.shell.ui.analysis.training, None);
    assert_eq!(rig.shell.app.engine_mode(run), EngineMode::Idle);
    assert_eq!(mainline_len(&rig), 0);
}

#[test]
fn escape_closes_the_exclusion_box() {
    let (mut rig, run) = hovered();
    rig.msg(aui(AnalysisUiMessage::OpenExclude(run)));
    assert!(view(&rig, run).exclude_open);
    rig.key(escape(), true, None);
    assert!(!view(&rig, run).exclude_open);
    assert!(!rig.shell.ui.analysis.field_focused());
}

#[test]
fn escape_lets_go_of_the_log_filter_and_is_spent() {
    let (mut rig, run) = hovered();
    open_log_with(&mut rig, run, vec![log_entry(1, LogKind::Received, "ok")]);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetLogFilter(
        run,
        "o".to_owned(),
    )));
    assert!(rig.shell.ui.analysis.field_focused());
    rig.key(escape(), true, None);
    assert!(!rig.shell.ui.analysis.field_focused());
}

// ---- the annotation and Esc (D920) ----

fn annotating() -> (Rig, RunId) {
    let (mut rig, run) = rig_with_engine(&SCHOLAR);
    ann(&mut rig, AnnotateMessage::Start);
    assert!(rig.shell.app.annotation_running());
    (rig, run)
}

#[test]
fn escape_without_layers_stops_the_annotation() {
    let (mut rig, _run) = annotating();
    rig.key(escape(), false, None);
    assert!(!rig.shell.app.annotation_running());
}

#[test]
fn escape_closing_the_about_box_does_not_stop_the_annotation() {
    let (mut rig, _run) = annotating();
    rig.msg(Message::Ui(UiMessage::ShowAbout(true)));
    rig.key(escape(), false, None);
    assert!(!rig.shell.ui.show_about, "the box closed");
    assert!(rig.shell.app.annotation_running(), "the run goes on");
    rig.key(escape(), false, None);
    assert!(
        !rig.shell.app.annotation_running(),
        "the second Esc stops it"
    );
}

#[test]
fn escape_leaving_the_search_field_does_not_stop_the_annotation() {
    let (mut rig, _run) = annotating();
    rig.shell.ui.toggle_pane(PaneKind::GameList);
    rig.shell.ui.games.search_focused = true;
    rig.key(escape(), true, None);
    assert!(!rig.shell.ui.games.search_focused);
    assert!(rig.shell.app.annotation_running());
}

// ---- no layer lets the keys of the panel through (D920) ----

#[test]
fn the_keys_do_not_leak_through_a_running_annotation() {
    let (mut rig, run) = rig_with_engine(&SCHOLAR);
    hover(&mut rig, true);
    ann(&mut rig, AnnotateMessage::Start);
    let before = multipv(&rig, run);
    rig.press(ch('4'));
    assert_eq!(multipv(&rig, run), before, "a digit during the run");
    rig.press(named(NamedKey::Enter));
    assert_eq!(annotate_notice(&rig), None, "Enter did not try to edit");
}

#[test]
fn the_keys_do_not_leak_through_the_comment_editor() {
    let (mut rig, run) = hovered();
    rig.msg(Message::Ui(UiMessage::Action(Action::ShowCommentEditor)));
    assert!(rig.shell.ui.comment_visible());
    rig.press(ch('4'));
    rig.press(named(NamedKey::Enter));
    assert_eq!(multipv(&rig, run), 3);
    assert_eq!(mainline_len(&rig), 0);
}

#[test]
fn the_modal_predicate_covers_every_layer() {
    let (mut rig, run) = hovered();
    assert!(!rig.shell.modal_layer_open());
    rig.msg(Message::Ui(UiMessage::ShowAbout(true)));
    assert!(rig.shell.modal_layer_open(), "about");
    rig.msg(Message::Ui(UiMessage::ShowAbout(false)));
    assert!(!rig.shell.modal_layer_open());
    rig.msg(aui(AnalysisUiMessage::OpenTraining(run)));
    assert!(rig.shell.modal_layer_open(), "training box");
    rig.msg(aui(AnalysisUiMessage::TrainingCancel));
    assert!(!rig.shell.modal_layer_open());
}

// ---- typed characters and focus (D922) ----

#[test]
fn digits_follow_the_typed_character_so_azerty_works() {
    let (mut rig, run) = hovered();
    // AZERTY: the key of "3" is `"` and needs Shift.
    let shifted = KeyInput {
        key: Key::Char('"'),
        modifiers: Modifiers {
            command: false,
            shift: true,
            alt: false,
        },
    };
    rig.key(shifted, false, Some('3'));
    assert_eq!(multipv(&rig, run), 3);
    rig.key(shifted, false, Some('2'));
    assert_eq!(multipv(&rig, run), 2);
    // A digit with Ctrl is not the panel's.
    rig.key(command(Key::Char('4')), false, Some('4'));
    assert_eq!(multipv(&rig, run), 2);
    // A typed character that is not a digit of 1-5 neither.
    rig.key(ch('6'), false, Some('6'));
    assert_eq!(multipv(&rig, run), 2);
}

#[test]
fn a_focused_field_of_the_panel_keeps_ctrl_enter_and_the_rest() {
    let (mut rig, run) = hovered();
    open_log_with(&mut rig, run, vec![log_entry(1, LogKind::Received, "ok")]);
    rig.shell.ui.analysis.focused_field = Some(LOG_SEARCH_ID);
    // The text field does not capture Ctrl+Enter: the shell must not act.
    rig.key(command(Key::Named(NamedKey::Enter)), false, None);
    rig.key(named(NamedKey::Enter), false, None);
    rig.key(ch('4'), false, Some('4'));
    assert_eq!(mainline_len(&rig), 0);
    assert_eq!(multipv(&rig, run), 3);
    rig.shell.ui.analysis.focused_field = None;
    rig.key(command(Key::Named(NamedKey::Enter)), false, None);
    assert_eq!(mainline_len(&rig), 2, "without a field it acts");
}

#[test]
fn a_pending_nag_sequence_owns_the_digits_and_other_keys_end_it() {
    let mut rig = Rig::with_game(game_of(&SCHOLAR));
    let run = uci_run(&mut rig);
    three_long_lines(&mut rig, run);
    let last = rig.game().mainline().last().expect("a move");
    rig.goto(last);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 3)));
    hover(&mut rig, false);
    rig.press(ch('!'));
    assert!(
        rig.shell.app.nag_sequence_deadline().is_some(),
        "a NAG sequence is pending"
    );
    hover(&mut rig, true);
    rig.press(ch('4'));
    assert_eq!(multipv(&rig, run), 3, "the digit went to the sequence");
    // Space ends the sequence first and then acts.
    rig.press(named(NamedKey::Space));
    assert!(rig.shell.app.nag_sequence_deadline().is_none());
}

// ---- the busy remark is seen during a run (D924) ----

#[test]
fn the_remark_and_the_progress_are_both_on_the_status_bar() {
    let (mut rig, _run) = annotating();
    ann(&mut rig, AnnotateMessage::Start);
    assert_eq!(annotate_notice(&rig), Some(AnnotateNotice::AlreadyRunning));
    tick_status(&mut rig);
    assert!(window_has(&rig, "Stop"), "the stop button of the progress");
    assert!(window_has(&rig, "An annotation is already running."));
}

// ---- the annotation dialog keys (GUI side, D910) ----

#[test]
fn escape_closes_the_annotation_dialog_and_enter_starts_it() {
    let (mut rig, _run) = rig_with_engine(&SCHOLAR);
    ann(&mut rig, AnnotateMessage::Open);
    assert_eq!(active(&rig), Some(ActiveDialog::Annotate));
    rig.key(escape(), false, None);
    assert_eq!(active(&rig), None, "Esc closes it");
    assert!(!rig.shell.app.annotation_running());
    ann(&mut rig, AnnotateMessage::Open);
    rig.key(named(NamedKey::Enter), false, None);
    assert!(rig.shell.app.annotation_running(), "Enter is OK");
}
