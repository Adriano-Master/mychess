//! M8-2b (GUI): the tournament between engines on the real window: the
//! entry points, the form, the panel, the boxes and the confirmation before
//! quitting (spec `docs/specs/m8-2-torneios-entre-engines.md`, "M8-2b",
//! D1360-D1379). Headless: no engine process is started, the answers of the
//! engines are messages built here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;

use std::time::Instant;

use common::Rig;
use iced::Size;
use mychess_app::engines::{EngineDraft, EngineFailure, FailureKind};
use mychess_app::tournament::{
    Adjudication, Loaded, Participant, Phase, PrepareFailure, TimeControl, TournamentConfig,
    TournamentMessage,
};
use mychess_app::{Action, Effect, Message as AppMessage};
use mychess_core::PartialDate;
use mychess_gui::i18n::Language;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::tournament_ui::{
    DecisionChoice, TournamentConfirm, TournamentTab, TournamentUiMessage,
};
use mychess_gui::{Message, UiMessage};

const SIZE: Size = Size::new(1400.0, 900.0);

fn rig_with_engines() -> Rig {
    let mut rig = Rig::new();
    for name in ["Alpha", "Beta"] {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(name, "/fake/engine"))
            .expect("engine added");
    }
    rig
}

fn active(rig: &Rig) -> Option<ActiveDialog> {
    ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs)
}

fn config(rig: &Rig) -> TournamentConfig {
    TournamentConfig {
        participants: rig
            .shell
            .app
            .engines
            .engines()
            .iter()
            .map(|e| Participant::new(e.id.0, &e.name))
            .collect(),
        time: TimeControl::Depth(1),
        adjudication: Adjudication {
            max_plies: Some(6),
            ..Adjudication::default()
        },
        ..TournamentConfig::default()
    }
}

/// Starts a tournament in the application without running any effect: the
/// files go to `dir`, and the engines are never launched. Returns the
/// effects that follow the start (the preparation of the first game).
fn start(rig: &mut Rig, dir: &std::path::Path) -> Vec<Effect> {
    let mut config = config(rig);
    config.failures.max_failures = 1;
    config.failures.max_retries = 0;
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Tournament(TournamentMessage::Start {
            config: Box::new(config),
            at: Instant::now(),
        }),
    );
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentLoad { request, .. } => Some(*request),
            _ => None,
        })
        .expect("the start asks for the files");
    let probed = effects.iter().any(|e| matches!(e, Effect::TournamentProbe));
    let loaded = Loaded {
        book: None,
        book_unreadable: false,
        suite: None,
        slug: "test-cup".to_owned(),
        dir: dir.to_path_buf(),
        today: PartialDate::new(Some(2026), Some(10), Some(24)).expect("date"),
        seed: 1,
        tablebase: None,
    };
    let mut effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Tournament(TournamentMessage::Loaded {
            request,
            result: Box::new(Ok(loaded)),
            at: Instant::now(),
        }),
    );
    rig.sync();
    if probed {
        // The unknown machine is read first (D2501): the answer releases the
        // load that waits for it.
        effects = mychess_app::update(
            &mut rig.shell.app,
            AppMessage::Tournament(TournamentMessage::Resources(
                mychess_app::tournament::SystemResources {
                    cores: 4,
                    ram_available_mb: Some(15_000),
                },
            )),
        );
        rig.sync();
    }
    effects
}

fn epoch_of(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentPrepare { epoch, .. } => Some(*epoch),
            _ => None,
        })
        .expect("the first game is being prepared")
}

fn running(rig: &mut Rig) -> (tempfile::TempDir, u64) {
    let dir = tempfile::tempdir().expect("tempdir");
    let effects = start(rig, dir.path());
    let epoch = epoch_of(&effects);
    (dir, epoch)
}

fn fail_first_game(rig: &mut Rig, epoch: u64) {
    let failure = EngineFailure {
        kind: FailureKind::CannotStart,
        exit_code: None,
        stderr_tail: Vec::new(),
    };
    mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Tournament(TournamentMessage::Prepared {
            epoch,
            result: Err(PrepareFailure {
                participant: 0,
                failure,
            }),
            at: Instant::now(),
        }),
    );
    rig.sync();
}

fn click(rig: &Rig, label: &str) -> Vec<Message> {
    let mut sim = rig.sim(SIZE);
    sim.click(label)
        .unwrap_or_else(|e| panic!("no button {label:?}: {e:?}"));
    sim.into_messages().collect()
}

fn has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

fn is_ui(messages: &[Message], wanted: &TournamentUiMessage) -> bool {
    messages
        .iter()
        .any(|m| matches!(m, Message::Tournament(t) if t == wanted))
}

// ---- the entry points ----

#[test]
fn the_shortcut_action_opens_the_form_and_escape_closes_it() {
    let mut rig = rig_with_engines();
    rig.msg(Message::Ui(UiMessage::Action(Action::TournamentEngines)));
    assert_eq!(active(&rig), Some(ActiveDialog::TournamentForm));
    assert!(rig.shell.modal_layer_open());
    assert!(has(&rig, "Tournament between engines"));
    rig.press(common::named(mychess_app::NamedKey::Escape));
    assert_eq!(active(&rig), None);
}

#[test]
fn the_play_menu_offers_the_tournament_with_its_shortcut() {
    use mychess_gui::ViewOptions;
    use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
    let rig = rig_with_engines();
    let specs = menu_specs(&rig.shell.app, &rig.shell.ui, ViewOptions::default());
    let tools = specs
        .iter()
        .find(|s| s.label_key == "menu-play")
        .expect("play menu");
    let entry = tools.entries.iter().find_map(|e| match e {
        MenuEntry::Item {
            label: Label::Key("menu-play-tournament"),
            shortcut,
            action,
            ..
        } => Some((*shortcut, action.is_some())),
        _ => None,
    });
    let (shortcut, enabled) = entry.expect("the entry exists");
    assert!(enabled);
    assert!(shortcut.is_some(), "Command+Shift+R is bound");
}

// ---- the form ----

#[test]
fn the_form_starts_with_two_engines_and_enter_submits_it() {
    let mut rig = rig_with_engines();
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    let draft = rig
        .shell
        .ui
        .dialogs
        .tournament
        .draft
        .as_ref()
        .expect("form");
    assert_eq!(draft.participants().len(), 2, "two engines come filled in");
    assert!(draft.error().is_none());
    let messages = click(&rig, "Start");
    assert!(is_ui(&messages, &TournamentUiMessage::Submit));
    // Enter does the same through the keyboard.
    rig.press(common::named(mychess_app::NamedKey::Enter));
    assert!(rig.shell.ui.dialogs.tournament.draft.is_none(), "submitted");
}

#[test]
fn a_form_with_one_participant_says_why_it_cannot_start() {
    let mut rig = Rig::new();
    rig.shell
        .app
        .engines
        .add(EngineDraft::new("Alpha", "/fake/engine"))
        .expect("engine added");
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    assert!(has(&rig, "At least two participants are needed."));
    // The disabled button sends nothing.
    let mut sim = rig.sim(SIZE);
    sim.click("Start").expect("the button is there");
    assert!(sim.into_messages().next().is_none());
    rig.press(common::named(mychess_app::NamedKey::Enter));
    assert!(rig.shell.ui.dialogs.tournament.draft.is_some());
}

#[test]
fn the_form_speaks_portuguese() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    for name in ["Alpha", "Beta"] {
        rig.shell
            .app
            .engines
            .add(EngineDraft::new(name, "/fake/engine"))
            .expect("engine added");
    }
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    assert!(has(&rig, "Torneio entre engines"));
    assert!(has(&rig, "Iniciar"));
    assert_eq!(rig.shell.i18n.language(), Language::PtBr);
}

// ---- the panel ----

#[test]
fn the_panel_without_a_tournament_offers_to_start_or_resume() {
    let mut rig = rig_with_engines();
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Tournament)));
    assert!(rig.shell.ui.is_open(PaneKind::Tournament));
    let messages = click(&rig, "New tournament...");
    assert!(is_ui(&messages, &TournamentUiMessage::Open));
}

#[test]
fn a_new_tournament_opens_its_panel_by_itself_on_the_live_tab() {
    let mut rig = rig_with_engines();
    assert!(!rig.shell.ui.is_open(PaneKind::Tournament));
    let (_dir, _epoch) = running(&mut rig);
    assert!(rig.shell.app.tournament_live());
    assert!(rig.shell.ui.is_open(PaneKind::Tournament));
    assert_eq!(rig.shell.ui.dialogs.tournament.tab, TournamentTab::Live);
    for label in ["Pause", "Stop after this game", "End", "Live", "Pairings"] {
        assert!(has(&rig, label), "{label}");
    }
}

#[test]
fn the_five_tabs_show_their_content() {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    for (tab, probe) in [
        (TournamentTab::Standings, "Pts"),
        (TournamentTab::Cross, "Export"),
        (TournamentTab::Pairings, "Pending"),
        (TournamentTab::Log, "The tournament started."),
        (TournamentTab::Live, "Pause"),
    ] {
        rig.msg(Message::Tournament(TournamentUiMessage::Tab(tab)));
        assert!(has(&rig, probe), "{tab:?} shows {probe:?}");
    }
}

#[test]
fn ending_the_tournament_asks_first() {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    let messages = click(&rig, "End");
    assert!(is_ui(
        &messages,
        &TournamentUiMessage::Request(TournamentConfirm::Abort)
    ));
    rig.feed(messages);
    assert_eq!(active(&rig), Some(ActiveDialog::TournamentConfirm));
    // "No" leaves everything as it was.
    let no = click(&rig, "No");
    rig.feed(no);
    assert_eq!(active(&rig), None);
    assert!(rig.shell.app.tournament_live());
    // "Yes" sends the abort.
    rig.msg(Message::Tournament(TournamentUiMessage::Request(
        TournamentConfirm::Abort,
    )));
    let yes = click(&rig, "Yes");
    assert!(
        yes.iter()
            .any(|m| matches!(m, Message::Tournament(TournamentUiMessage::Confirm(true)))),
        "{yes:?}"
    );
    let out = rig
        .shell
        .ui
        .dialogs
        .tournament
        .update(TournamentUiMessage::Confirm(true), &rig.shell.app);
    assert!(matches!(
        out.messages.as_slice(),
        [AppMessage::Tournament(TournamentMessage::Abort(_))]
    ));
}

#[test]
fn the_status_bar_shows_the_progress() {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    let bar = mychess_gui::windows::tournament_panel::status_progress(&rig.shell)
        .expect("a tournament is on");
    assert!(bar.text.ends_with(": 0 of 2 games"), "{}", bar.text);
    assert!(has(&rig, &bar.text), "the bar is drawn: {:?}", bar.text);
}

// ---- the question about a participant ----

#[test]
fn a_participant_that_fails_too_often_raises_the_decision_box() {
    let mut rig = rig_with_engines();
    let (_dir, epoch) = running(&mut rig);
    fail_first_game(&mut rig, epoch);
    assert!(matches!(
        rig.shell
            .app
            .tournament()
            .map(mychess_app::tournament::Tournament::phase),
        Some(Phase::Decision(0))
    ));
    assert_eq!(active(&rig), Some(ActiveDialog::TournamentDecision));
    for label in ["Try again", "Withdraw it", "Replace it", "Later"] {
        assert!(has(&rig, label), "{label}");
    }
    assert!(!has(&rig, "Skip"), "the box has no way to skip");
    // Replace needs an engine chosen: until then it sends nothing.
    let mut sim = rig.sim(SIZE);
    sim.click("Replace it").expect("there");
    assert!(sim.into_messages().next().is_none());
    // Retry answers the tournament.
    let messages = click(&rig, "Try again");
    assert!(is_ui(
        &messages,
        &TournamentUiMessage::Decide(DecisionChoice::Retry)
    ));
}

#[test]
fn the_decision_box_can_wait_and_comes_back_from_the_toolbar() {
    let mut rig = rig_with_engines();
    let (_dir, epoch) = running(&mut rig);
    fail_first_game(&mut rig, epoch);
    rig.press(common::named(mychess_app::NamedKey::Escape));
    assert_eq!(
        active(&rig),
        None,
        "Esc closes the box, the tournament waits"
    );
    assert!(rig.shell.app.tournament_live());
    let messages = click(&rig, "Answer...");
    rig.feed(messages);
    assert_eq!(active(&rig), Some(ActiveDialog::TournamentDecision));
}

#[test]
fn withdrawing_sends_the_decision_to_the_application() {
    let mut rig = rig_with_engines();
    let (_dir, epoch) = running(&mut rig);
    fail_first_game(&mut rig, epoch);
    let out = rig.shell.ui.dialogs.tournament.update(
        TournamentUiMessage::Decide(DecisionChoice::Withdraw),
        &rig.shell.app,
    );
    assert!(matches!(
        out.messages.as_slice(),
        [AppMessage::Tournament(TournamentMessage::Decide(
            mychess_app::tournament::Decision::Withdraw,
            _
        ))]
    ));
}

// ---- leaving the program ----

#[test]
fn quitting_with_a_tournament_on_asks_first() {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    rig.msg(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(active(&rig), Some(ActiveDialog::TournamentConfirm));
    assert_eq!(
        rig.shell.ui.dialogs.tournament.confirm,
        Some(TournamentConfirm::Quit)
    );
    // Esc is "No": the program stays.
    rig.press(common::named(mychess_app::NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(rig.shell.app.tournament_live());
}

/// The tasks the shell returns for a key press in the "Quit" box.
fn quit_box_key_units(key: mychess_app::NamedKey) -> usize {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    rig.msg(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(active(&rig), Some(ActiveDialog::TournamentConfirm));
    let task = rig.shell.update(Message::Dialog(
        mychess_gui::windows::dialog_ui::DialogMessage::Key {
            input: common::named(key),
            text_focused: false,
            typed: None,
        },
    ));
    assert_eq!(active(&rig), None, "the box closes");
    task.units()
}

#[test]
fn enter_in_the_quit_box_quits_and_escape_does_not() {
    assert_eq!(quit_box_key_units(mychess_app::NamedKey::Escape), 0);
    assert!(
        quit_box_key_units(mychess_app::NamedKey::Enter) > 0,
        "Enter is Yes: the shell has to ask the application to quit"
    );
}

#[test]
fn a_quit_question_does_not_outlive_the_tournament() {
    let mut rig = rig_with_engines();
    rig.shell.ui.dialogs.tournament.confirm = Some(TournamentConfirm::Quit);
    rig.sync();
    assert_eq!(rig.shell.ui.dialogs.tournament.confirm, None);
}

#[test]
fn resuming_from_the_form_drops_the_form() {
    let mut rig = rig_with_engines();
    rig.msg(Message::Tournament(TournamentUiMessage::Open));
    assert!(rig.shell.ui.dialogs.tournament.draft.is_some());
    rig.msg(Message::Tournament(TournamentUiMessage::ResumePick(
        std::path::PathBuf::from("missing.toml"),
    )));
    assert!(rig.shell.ui.dialogs.tournament.draft.is_none());
}

#[test]
fn the_pairings_window_follows_the_height_of_the_list() {
    let ui = mychess_gui::windows::tournament_ui::TournamentUi::default();
    let (_, small) = ui.window_for(1000, 200.0);
    let (_, large) = ui.window_for(1000, 1500.0);
    assert!(
        large > small,
        "{small} rows for 200 px, {large} for 1500 px"
    );
    assert!(large * 26 >= 1500, "the tall list must be filled");
}

#[test]
fn quitting_without_a_tournament_does_not_ask() {
    let mut rig = rig_with_engines();
    rig.msg(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(active(&rig), None);
}

// ---- the clocks ----

#[test]
fn the_tick_runs_only_with_the_live_tab_in_sight() {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    let ui = &rig.shell.ui.dialogs.tournament;
    assert!(ui.wants_ticks(&rig.shell.app, true));
    assert!(!ui.wants_ticks(&rig.shell.app, false), "panel behind a tab");
    rig.msg(Message::Tournament(TournamentUiMessage::Tab(
        TournamentTab::Log,
    )));
    let ui = &rig.shell.ui.dialogs.tournament;
    assert!(!ui.wants_ticks(&rig.shell.app, true), "another tab");
}

#[test]
fn closing_the_panel_does_not_end_the_tournament() {
    let mut rig = rig_with_engines();
    let (_dir, _epoch) = running(&mut rig);
    rig.msg(Message::Ui(UiMessage::ClosePane(PaneKind::Tournament)));
    assert!(!rig.shell.ui.is_open(PaneKind::Tournament));
    assert!(rig.shell.app.tournament_live());
}
