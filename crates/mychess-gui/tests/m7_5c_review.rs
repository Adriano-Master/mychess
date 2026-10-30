//! M7-5c (GUI): the fixes of the review - what quitting asks, the keyboard
//! under a card, the field focus after a form, the draft of the editor.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::items_after_statements
)]

mod m7_5c_common;

use iced::widget::text_editor::{Action as EditorAction, Edit, Motion};
use m7_5c_common::*;
use mychess_app::epd_window::{AnnotateConfig, AnnotateMode, RunScope};
use mychess_app::{Key, KeyInput, Modifiers, NamedKey, update};
use mychess_gui::windows::epd::{EPD_EDITOR_ID, EPD_QUIT_BOX_ID, EPD_SECONDS_ID, EPD_STRIP_ID};
use mychess_gui::windows::online_ui::{OnlineConfirm, OnlineUiMessage};
use mychess_gui::windows::tournament_ui::{TournamentConfirm, TournamentUiMessage};

fn key(rig: &mut Rig, key: NamedKey, modifiers: Modifiers) {
    rig.ui(Message::Ui(UiMessage::Key {
        input: KeyInput {
            key: Key::Named(key),
            modifiers,
        },
        text_focused: false,
    }));
}

const NAVIGATION: Modifiers = Modifiers {
    command: true,
    shift: true,
    alt: false,
};

fn first_row(rig: &Rig) -> mychess_app::epd_window::EpdIndex {
    rig.shell.app.epd_rows(rig.tab().id, 0..1)[0].index
}

/// Types `\nce 7` at the end of the editor (a draft, not yet a commit).
fn type_draft(rig: &mut Rig) {
    let first = first_row(rig);
    rig.send(EpdMessage::Select(first));
    rig.ui(Message::Epd(EpdUiMessage::Editor(EditorAction::Move(
        Motion::DocumentEnd,
    ))));
    for c in "\nce 7".chars() {
        let edit = if c == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(c)
        };
        rig.ui(Message::Epd(EpdUiMessage::Editor(EditorAction::Edit(edit))));
    }
    assert!(rig.tab().editor_draft);
}

/// Reports the size of the window once, so the quit has a geometry to
/// record: the width reaches the preferences only when the quit goes on
/// (the first report does not save anything by itself).
fn arm_quit_probe(rig: &mut Rig) {
    rig.ui(Message::Ui(UiMessage::WindowResized(iced::Size::new(
        1280.0, 800.0,
    ))));
    assert!(!quit_went_on(rig));
}

/// Whether the quit of the application was reached (`quit_final` records
/// the geometry before asking the application to quit).
fn quit_went_on(rig: &Rig) -> bool {
    rig.shell.app.prefs.window.width == Some(1280)
}

/// A tournament that is live: it started and waits for its files.
fn start_tournament(rig: &mut Rig) {
    use mychess_app::engines::EngineDraft;
    use mychess_app::tournament::{Participant, TimeControl, TournamentConfig, TournamentMessage};
    for name in ["Alpha", "Beta"] {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(name, "/fake/engine"))
            .expect("engine added");
    }
    let config = TournamentConfig {
        participants: rig
            .shell
            .app
            .engines
            .engines()
            .iter()
            .map(|e| Participant::new(e.id.0, &e.name))
            .collect(),
        time: TimeControl::Depth(1),
        ..TournamentConfig::default()
    };
    drop(update(
        &mut rig.shell.app,
        mychess_app::Message::Tournament(TournamentMessage::Start {
            config: Box::new(config),
            at: std::time::Instant::now(),
        }),
    ));
    assert!(rig.shell.app.tournament_live());
}

fn config() -> AnnotateConfig {
    AnnotateConfig {
        engine: None,
        seconds: 3,
        mode: AnnotateMode::Count,
        scope: RunScope::All,
    }
}

// ---- I1: the answer "quit without saving" holds for one request ----

#[test]
fn the_quit_answer_does_not_stay_armed_when_the_quit_does_not_go_on() {
    let mut rig = Rig::new();
    rig.open("cuatro.epd", &cuatro());
    rig.send(EpdMessage::AddPosition);
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(true)));
    assert!(
        !rig.shell.ui.dialogs.epd.quit_confirmed,
        "the answer was spent with the request"
    );
    // The quit did not go on (the user cancelled the question of the game,
    // say); a new request about an altered file asks again.
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
}

#[test]
fn confirming_the_quit_reaches_the_quit_of_the_application() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    arm_quit_probe(&mut rig);
    rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
    rig.ui(Message::Epd(EpdUiMessage::StripInput("id".into())));
    rig.ui(Message::Epd(EpdUiMessage::StripGo));
    assert!(rig.tab().busy, "the batch is in flight");
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    assert!(rig.tab().busy, "nothing was cancelled yet");
    assert!(!quit_went_on(&rig), "the question comes first");
    rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(true)));
    // The quit of the application was reached; `Quit` drops the jobs.
    assert!(quit_went_on(&rig));
    assert!(!rig.tab().busy);
    assert!(rig.shell.ui.dialogs.epd.form.is_none());
}

// ---- I2: drafts and batches in flight are not lost without asking ----

#[test]
fn quitting_with_an_unconfirmed_draft_asks_first() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    type_draft(&mut rig);
    assert!(!rig.tab().dirty, "only the draft differs");
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
}

#[test]
fn quitting_while_a_batch_edit_runs_asks_first() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
    rig.ui(Message::Epd(EpdUiMessage::StripInput("id".into())));
    rig.ui(Message::Epd(EpdUiMessage::StripGo));
    assert!(rig.tab().busy && !rig.tab().dirty);
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
}

#[test]
fn quitting_with_nothing_to_lose_does_not_ask() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert!(rig.shell.ui.dialogs.epd.form.is_none());
}

// ---- I3: a card owns the keyboard; the keys need the panel ----

#[test]
fn navigation_keys_do_nothing_under_a_card() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = first_row(&rig);
    rig.send(EpdMessage::Select(first));
    // Control: with no card the key moves the selection.
    key(&mut rig, NamedKey::Down, NAVIGATION);
    let moved = rig.tab().selected;
    assert_ne!(moved, Some(first));
    rig.send(EpdMessage::RemovePosition);
    assert!(rig.shell.modal_layer_open());
    key(&mut rig, NamedKey::Down, NAVIGATION);
    key(&mut rig, NamedKey::Home, NAVIGATION);
    assert_eq!(rig.tab().selected, moved, "the card owns the keyboard");
}

#[test]
fn navigation_keys_do_nothing_with_the_panel_closed() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = first_row(&rig);
    rig.send(EpdMessage::Select(first));
    rig.ui(Message::Ui(UiMessage::Action(Action::ToggleEpdPane)));
    assert!(!rig.shell.ui.is_visible(PaneKind::Epd));
    rig.ui(Message::Ui(UiMessage::Action(Action::EpdNext)));
    assert_eq!(rig.tab().selected, Some(first));
}

#[test]
fn a_focused_text_field_blocks_the_four_navigation_actions() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = first_row(&rig);
    rig.send(EpdMessage::Select(first));
    rig.ui(Message::Epd(EpdUiMessage::Focus(EPD_EDITOR_ID, true)));
    for action in [
        Action::EpdFirst,
        Action::EpdPrev,
        Action::EpdNext,
        Action::EpdLast,
    ] {
        rig.ui(Message::Ui(UiMessage::Action(action)));
        assert_eq!(rig.tab().selected, Some(first), "{action:?}");
    }
    rig.ui(Message::Epd(EpdUiMessage::Focus(EPD_EDITOR_ID, false)));
    rig.ui(Message::Ui(UiMessage::Action(Action::EpdLast)));
    let last = rig
        .shell
        .app
        .epd_rows(rig.tab().id, 4..5)
        .first()
        .map(|row| row.index);
    assert_eq!(rig.tab().selected, last, "free again: the last of five");
    assert_eq!(rig.tab().selected_row, Some(4));
}

// ---- I5: a form that closes takes the focus with it ----

#[test]
fn escape_closes_every_form_and_frees_the_field_focus() {
    let forms = [
        EpdForm::Analyze(mychess_gui::windows::epd::AnalyzeDraft::from_prefs(
            &mychess_app::epd_window::EpdPrefs::default(),
        )),
        EpdForm::ConfirmUndo(config()),
        EpdForm::Strip("bm".into()),
        EpdForm::Report(mychess_app::epd_window::EpdFileId(1)),
        EpdForm::ConfirmQuit,
    ];
    for form in forms {
        let mut rig = Rig::new();
        rig.open("cinco.epd", CINCO);
        rig.shell.ui.dialogs.epd.form = Some(form.clone());
        rig.shell.ui.dialogs.epd.focused = Some(EPD_SECONDS_ID);
        assert!(rig.shell.modal_layer_open(), "{form:?}");
        key(&mut rig, NamedKey::Escape, Modifiers::NONE);
        assert!(rig.shell.ui.dialogs.epd.form.is_none(), "{form:?}");
        assert!(rig.shell.ui.dialogs.epd.focused.is_none(), "{form:?}");
        assert!(!rig.shell.modal_layer_open(), "{form:?}");
    }
}

#[test]
fn closing_a_form_by_its_buttons_frees_the_field_focus() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
    rig.shell.ui.dialogs.epd.focused = Some(EPD_STRIP_ID);
    rig.ui(Message::Epd(EpdUiMessage::StripInput("id".into())));
    rig.ui(Message::Epd(EpdUiMessage::StripGo));
    assert!(rig.shell.ui.dialogs.epd.focused.is_none(), "Enter");
    rig.ui(Message::Epd(EpdUiMessage::OpenAnalyze));
    rig.shell.ui.dialogs.epd.focused = Some(EPD_SECONDS_ID);
    rig.ui(Message::Epd(EpdUiMessage::CloseForm));
    assert!(rig.shell.ui.dialogs.epd.focused.is_none(), "Cancel");
}

#[test]
fn a_form_closed_behind_the_back_of_the_handlers_still_frees_the_focus() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.ui(Message::Epd(EpdUiMessage::OpenAnalyze));
    rig.shell.ui.dialogs.epd.focused = Some(EPD_SECONDS_ID);
    // Synchronization sees the form open, then gone.
    rig.ui(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    rig.shell.ui.dialogs.epd.form = None;
    rig.ui(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    assert!(rig.shell.ui.dialogs.epd.focused.is_none());
}

// ---- D3855: leaving the editor writes the draft ----

#[test]
fn losing_the_focus_of_the_editor_writes_the_draft() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    type_draft(&mut rig);
    rig.ui(Message::Epd(EpdUiMessage::Focus(EPD_EDITOR_ID, true)));
    assert!(rig.tab().editor_draft, "focus alone writes nothing");
    rig.ui(Message::Epd(EpdUiMessage::Focus(EPD_EDITOR_ID, false)));
    let tab = rig.tab();
    assert!(!tab.editor_draft && tab.dirty);
    assert!(tab.editor_text.contains("ce 7"));
}

#[test]
fn another_field_losing_the_focus_keeps_the_draft() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    type_draft(&mut rig);
    rig.ui(Message::Epd(EpdUiMessage::Focus(EPD_EDITOR_ID, true)));
    rig.ui(Message::Epd(EpdUiMessage::Focus("epd-filter", false)));
    assert!(rig.tab().editor_draft);
}

// ---- (e): the text form comes back after "no" ----

#[test]
fn answering_no_to_the_quit_question_brings_back_the_open_form() {
    for escape in [false, true] {
        let mut rig = Rig::new();
        rig.open("cuatro.epd", &cuatro());
        rig.send(EpdMessage::AddPosition);
        rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
        rig.ui(Message::Epd(EpdUiMessage::StripInput("ce".into())));
        rig.ui(Message::Ui(UiMessage::CloseRequested));
        assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
        if escape {
            key(&mut rig, NamedKey::Escape, Modifiers::NONE);
        } else {
            rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(false)));
        }
        assert_eq!(
            rig.shell.ui.dialogs.epd.form,
            Some(EpdForm::Strip("ce".into())),
            "escape: {escape}"
        );
    }
}

// ---- the disabled editor and the tabs ----

#[test]
fn an_editor_held_by_a_job_ignores_typing_and_the_save_button_is_off() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = first_row(&rig);
    rig.send(EpdMessage::Select(first));
    rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
    rig.ui(Message::Epd(EpdUiMessage::StripInput("id".into())));
    rig.ui(Message::Epd(EpdUiMessage::StripGo));
    let tab = rig.tab();
    assert!(tab.busy && !tab.editor_enabled);
    let before = rig.shell.ui.dialogs.epd.content.text();
    rig.ui(Message::Epd(EpdUiMessage::Editor(EditorAction::Edit(
        Edit::Insert('x'),
    ))));
    assert_eq!(rig.shell.ui.dialogs.epd.content.text(), before);
    assert!(!rig.tab().editor_draft);
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(id("epd-btn-save")).unwrap();
    assert_eq!(sim.into_messages().count(), 0, "Save is disabled");
}

#[test]
fn the_tab_of_an_altered_file_is_marked_with_a_plain_asterisk() {
    let mut rig = Rig::new();
    rig.open("cuatro.epd", &cuatro());
    rig.send(EpdMessage::AddPosition);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("* cuatro.epd").is_ok());
    // A clean file has no mark.
    let mut clean = Rig::new();
    clean.open("cuatro.epd", &cuatro());
    let mut sim = clean.sim(1400.0, 900.0);
    assert!(sim.find("cuatro.epd").is_ok());
    assert!(sim.find("* cuatro.epd").is_err());
}

// ---- D3866: the question of the EPD comes before the others ----

fn dirty_epd(rig: &mut Rig) {
    rig.open("cuatro.epd", &cuatro());
    rig.send(EpdMessage::AddPosition);
    assert!(rig.tab().dirty);
}

#[test]
fn the_epd_question_comes_before_the_one_of_a_live_tournament() {
    let mut rig = Rig::new();
    dirty_epd(&mut rig);
    start_tournament(&mut rig);
    arm_quit_probe(&mut rig);
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    assert_eq!(rig.shell.ui.dialogs.tournament.confirm, None);
    rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(true)));
    assert_eq!(
        rig.shell.ui.dialogs.tournament.confirm,
        Some(TournamentConfirm::Quit)
    );
    assert!(!quit_went_on(&rig));
    rig.ui(Message::Tournament(TournamentUiMessage::Confirm(true)));
    assert!(quit_went_on(&rig), "the second yes quits");
}

#[test]
fn the_epd_question_comes_before_the_one_of_a_live_online_game() {
    use mychess_app::online::{OnlinePhase, PlayingState};
    let mut rig = Rig::new();
    dirty_epd(&mut rig);
    rig.shell.app.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.shell.app.online.phase.is_live());
    arm_quit_probe(&mut rig);
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    assert_eq!(rig.shell.ui.dialogs.online.confirm, None);
    rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(true)));
    assert_eq!(
        rig.shell.ui.dialogs.online.confirm,
        Some(OnlineConfirm::Quit)
    );
    assert!(!quit_went_on(&rig));
    rig.ui(Message::Online(OnlineUiMessage::Confirm(true)));
    assert!(quit_went_on(&rig), "the second yes quits");
}

#[test]
fn answering_yes_to_the_quit_loses_the_text_form_that_was_open() {
    // Documented (D3869): "yes" spends `form_before_quit`; when the
    // tournament then cancels the quit, the typed opcode is gone.
    let mut rig = Rig::new();
    dirty_epd(&mut rig);
    start_tournament(&mut rig);
    rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
    rig.ui(Message::Epd(EpdUiMessage::StripInput("ce".into())));
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(true)));
    rig.ui(Message::Tournament(TournamentUiMessage::Confirm(false)));
    assert!(rig.shell.ui.dialogs.epd.form.is_none());
}

// ---- D3869: the question shows with the panel hidden; stale layers ----

#[test]
fn the_quit_question_is_drawn_with_the_panel_hidden() {
    let mut rig = Rig::new();
    dirty_epd(&mut rig);
    rig.ui(Message::Ui(UiMessage::Action(Action::ToggleEpdPane)));
    assert!(!rig.shell.ui.is_visible(PaneKind::Epd));
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(
        sim.find(id(EPD_QUIT_BOX_ID)).is_ok(),
        "the card is on screen"
    );
}

#[test]
fn a_report_of_a_file_that_is_gone_is_dropped_and_frees_the_keyboard() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.shell.ui.dialogs.epd.form = Some(EpdForm::Report(mychess_app::epd_window::EpdFileId(99)));
    assert!(
        rig.shell.modal_layer_open(),
        "the form alone holds the keys"
    );
    rig.ui(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    assert!(rig.shell.ui.dialogs.epd.form.is_none());
    assert!(!rig.shell.modal_layer_open());
}
