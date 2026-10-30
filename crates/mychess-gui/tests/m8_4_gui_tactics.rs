//! M8-4c (GUI): the menu entry and shortcut of the tactics training, its
//! dialog and keys, the panel under the board and the Fluent files (D1937-
//! D1942).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mychess_app::tactics::{
    TacticsConfig, TacticsEffect, TacticsMessage, TacticsNotice, TacticsPhase, find_next, judge,
    open_file,
};
use mychess_app::{
    Action, AppState, Effect, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, update,
};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::tactics_dialog::{TACTICS_BOX_ID, TacticsUiMessage};
use mychess_gui::windows::tactics_panel::TACTICS_BAR_ID;
use mychess_gui::windows::tactics_text::notice_text;
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";

fn key(named: NamedKey) -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(named),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

fn ftl(language: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("tactics.ftl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .collect()
}

fn variables(text: &str) -> BTreeSet<String> {
    text.split("{ $")
        .skip(1)
        .filter_map(|rest| rest.split([' ', '}']).next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn both_languages_have_the_same_keys_with_the_same_variables() {
    let (en, pt) = (ftl("en-US"), ftl("pt-BR"));
    assert!(en.len() > 60, "the file was read: {}", en.len());
    assert_eq!(
        en.keys().collect::<Vec<_>>(),
        pt.keys().collect::<Vec<_>>(),
        "the keys differ"
    );
    for (key, text) in &en {
        assert_eq!(variables(text), variables(&pt[key]), "variables of {key}");
    }
}

#[test]
fn portuguese_says_variante_never_variacao() {
    for (key, text) in ftl("pt-BR") {
        let lower = text.to_lowercase();
        assert!(
            !lower.contains("variação")
                && !lower.contains("variacao")
                && !lower.contains("variações"),
            "{key}: {text}"
        );
    }
}

#[test]
fn every_notice_and_status_has_a_text_in_both_languages() {
    use mychess_gui::i18n::{I18n, Language};
    let notices = [
        TacticsNotice::Busy,
        TacticsNotice::UnsavedGame,
        TacticsNotice::NoSource,
        TacticsNotice::SourceUnreadable,
        TacticsNotice::NoGames,
        TacticsNotice::Locked,
        TacticsNotice::TimedOut,
        TacticsNotice::Inconclusive,
        TacticsNotice::NoHint,
        TacticsNotice::StoreNotSaved,
        TacticsNotice::StoreUnreadable,
        TacticsNotice::StoreNewer,
        TacticsNotice::SourceChanged,
    ];
    let known = ftl("en-US");
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in notices {
            assert!(known.contains_key(notice.message_key()), "{notice:?}");
            let text = notice_text(notice, &i18n);
            assert!(
                !text.is_empty() && !text.contains("tactics-"),
                "{notice:?}: {text}"
            );
        }
    }
}

fn menu_entry(shell: &Shell) -> (Option<mychess_app::Binding>, bool) {
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    for spec in specs.iter().filter(|s| s.label_key == "menu-play") {
        for entry in &spec.entries {
            if let MenuEntry::Item {
                label: Label::Key("menu-play-tactics"),
                shortcut,
                action,
                ..
            } = entry
            {
                return (*shortcut, action.is_some());
            }
        }
    }
    panic!("the Play menu has no tactics entry");
}

#[test]
fn the_menu_offers_the_training_with_a_conflict_free_command_shortcut() {
    let shell = Shell::new(None);
    let (shortcut, enabled) = menu_entry(&shell);
    assert!(enabled);
    let binding = shortcut.expect("a default shortcut");
    assert_eq!(binding.to_string(), "Command+Shift+X");
    assert!(binding.modifiers.command, "uses Command, not a fixed Ctrl");
    let mut owners = 0;
    for action in Action::all().chain([Action::TacticsTrainer, Action::PlayEngine]) {
        if shell.app.shortcuts.bindings(action).contains(&binding) {
            owners += 1;
        }
    }
    assert_eq!(owners, 1);
}

#[test]
fn the_action_opens_the_dialog_as_a_modal_layer_and_escape_closes_it() {
    let mut shell = Shell::new(None);
    assert!(!shell.modal_layer_open());
    drop(shell.update(Message::Ui(UiMessage::Action(Action::TacticsTrainer))));
    assert!(shell.app.tactics_dialog_open());
    assert!(shell.modal_layer_open());
    drop(shell.update(key(NamedKey::Escape)));
    assert!(!shell.app.tactics_dialog_open());
    assert!(!shell.modal_layer_open());
}

#[test]
fn enter_without_a_file_keeps_the_dialog_open_and_says_why() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Open)));
    drop(shell.update(key(NamedKey::Enter)));
    assert!(shell.app.tactics_dialog_open());
    assert_eq!(
        shell.app.tactics_view().notice,
        Some(TacticsNotice::NoSource)
    );
}

#[test]
fn the_dialog_is_drawn_over_the_window() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Open)));
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(TACTICS_BOX_ID)).is_ok());
    assert!(sim.find(iced::widget::Id::new("board-pane")).is_ok());
}

/// Runs the effects of the training the way `perform` would, but in line.
fn settle(app: &mut AppState, effects: Vec<Effect>) {
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        let Effect::Tactics(effect) = effect else {
            continue;
        };
        let answer = match effect {
            TacticsEffect::OpenFile { request, path } => TacticsMessage::Opened {
                request,
                result: open_file(&path).map(Box::new),
            },
            TacticsEffect::Find {
                request,
                source,
                query,
            } => TacticsMessage::Found {
                request,
                outcome: Box::new(find_next(&source, &query)),
            },
            TacticsEffect::Judge { request, job } => TacticsMessage::Judged {
                request,
                verdict: Box::new(judge(&job)),
            },
            _ => continue,
        };
        queue.extend(update(app, AppMessage::Tactics(answer)));
    }
}

fn started_shell(dir: &Path) -> Shell {
    let path = dir.join("p.pgn");
    std::fs::write(
        &path,
        format!("[Event \"One\"]\n[SetUp \"1\"]\n[FEN \"{MATE1}\"]\n\n*\n"),
    )
    .unwrap();
    let mut shell = Shell::new(None);
    let config = TacticsConfig {
        file: Some(path),
        ..TacticsConfig::default()
    };
    let effects = update(
        &mut shell.app,
        AppMessage::Tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        }),
    );
    settle(&mut shell.app, effects);
    assert_eq!(shell.app.tactics_phase(), TacticsPhase::Solving);
    // Any message of the shell brings the visual state up to date.
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::FilePicked(None))));
    shell
}

#[test]
fn the_panel_appears_under_the_board_with_a_session_and_goes_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(TACTICS_BAR_ID)).is_ok());
    drop(sim);
    drop(shell.update(Message::App(AppMessage::Tactics(TacticsMessage::Close))));
    assert_eq!(shell.app.tactics_phase(), TacticsPhase::Closed);
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(TACTICS_BAR_ID)).is_err());
}

#[test]
fn the_second_hint_draws_an_arrow_on_the_board_and_the_first_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    assert_eq!(shell.ui.dialogs.tactics.arrows().len(), 0);
    drop(shell.update(Message::App(AppMessage::Tactics(TacticsMessage::Hint))));
    assert_eq!(
        shell.ui.dialogs.tactics.arrows().len(),
        0,
        "level one is text"
    );
    drop(shell.update(Message::App(AppMessage::Tactics(TacticsMessage::Hint))));
    let arrows = shell.ui.dialogs.tactics.arrows();
    assert_eq!(arrows.len(), 1);
    assert_eq!(arrows[0].from.to_string(), "a1");
    assert_eq!(arrows[0].to.to_string(), "a8");
}

#[test]
fn the_dialog_form_edits_a_draft_and_enter_starts_with_it() {
    use mychess_gui::windows::tactics_dialog::TacticsEdit;
    let mut shell = Shell::new(None);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Open)));
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Edit(
        TacticsEdit::MateMax(5),
    ))));
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Edit(
        TacticsEdit::Attempts(9),
    ))));
    let view = shell.app.tactics_view();
    let start = shell.ui.dialogs.tactics.to_start(&view);
    assert_eq!((start.mate_max, start.max_attempts), (5, 9));
    // The application keeps its own configuration until a start.
    assert_eq!(view.config.mate_max, 3);
}
