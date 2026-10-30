//! M8-5c (GUI): the menu entry and shortcut of the opening trainer, its
//! dialog and keys, the panel under the board and the Fluent files
//! (D2238-D2243).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use mychess_app::opening_trainer::{
    OpeningConfig, OpeningEffect, OpeningMessage, OpeningNotice, OpeningPhase, RepSourceKind,
    TrainSide, build,
};
use mychess_app::{
    Action, AppState, Effect, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers,
    NamedKey, PlayOptions, update,
};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::opening_trainer_dialog::{OPENING_BOX_ID, OpeningEdit, OpeningUiMessage};
use mychess_gui::windows::opening_trainer_panel::OPENING_BAR_ID;
use mychess_gui::windows::opening_trainer_text::{candidates_text, notice_text, report_text};
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

const PGN: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 *\n";

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
        .join("opening_trainer.ftl");
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
    assert!(en.len() > 70, "the file was read: {}", en.len());
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
            !lower.contains("variação") && !lower.contains("variacao"),
            "{key}: {text}"
        );
    }
}

#[test]
fn every_notice_has_a_text_in_both_languages() {
    use mychess_gui::i18n::{I18n, Language};
    let notices = [
        OpeningNotice::Busy,
        OpeningNotice::NoRepertoire,
        OpeningNotice::NoSource,
        OpeningNotice::SourceUnreadable,
        OpeningNotice::SourceNeedsUpgrade,
        OpeningNotice::SourceCorrupt,
        OpeningNotice::SourceUnfinished,
        OpeningNotice::BookUnreadable,
        OpeningNotice::MaskUnreadable,
        OpeningNotice::EmptyRepertoire,
        OpeningNotice::OneSideEmpty,
        OpeningNotice::Truncated,
        OpeningNotice::Locked,
        OpeningNotice::TimedOut,
        OpeningNotice::NoHint,
        OpeningNotice::Dubious,
        OpeningNotice::OutOfRepertoire,
        OpeningNotice::StoreNotSaved,
        OpeningNotice::StoreUnreadable,
        OpeningNotice::StoreNewer,
        OpeningNotice::SourceChanged,
    ];
    let known = ftl("en-US");
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in notices {
            assert!(known.contains_key(notice.message_key()), "{notice:?}");
            let text = notice_text(notice, &i18n);
            assert!(
                !text.is_empty() && !text.contains("opening-"),
                "{notice:?}: {text}"
            );
        }
    }
}

#[test]
fn the_report_and_the_candidates_are_translated_with_their_numbers() {
    use mychess_app::opening_trainer::{Candidate, ReportView};
    use mychess_gui::i18n::{I18n, Language};
    let report = ReportView {
        positions: 12,
        never: 5,
        played: 7,
        due: 3,
        learned: 2,
        good: 9,
        dubious: 1,
        out: 4,
    };
    let candidates = [
        Candidate {
            san: "e4".to_owned(),
            percent: 60,
            dubious: false,
        },
        Candidate {
            san: "f4".to_owned(),
            percent: 10,
            dubious: true,
        },
    ];
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let text = report_text(&report, &i18n);
        for n in ["12", "5", "7", "3", "2", "9", "1", "4"] {
            assert!(text.contains(n), "{text}");
        }
        let moves = candidates_text(&candidates, &i18n).unwrap();
        assert!(
            moves.contains("e4 60%") && moves.contains("f4?! 10%"),
            "{moves}"
        );
    }
    assert!(candidates_text(&[], &I18n::new(Language::EnUs)).is_none());
}

fn menu_entry(shell: &Shell) -> (Option<mychess_app::Binding>, bool) {
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    for spec in specs.iter().filter(|s| s.label_key == "menu-play") {
        for entry in &spec.entries {
            if let MenuEntry::Item {
                label: Label::Key("menu-play-opening-trainer"),
                shortcut,
                action,
                ..
            } = entry
            {
                return (*shortcut, action.is_some());
            }
        }
    }
    panic!("the Play menu has no opening trainer entry");
}

#[test]
fn the_menu_offers_the_trainer_with_a_conflict_free_command_shortcut() {
    let shell = Shell::new(None);
    let (shortcut, enabled) = menu_entry(&shell);
    assert!(enabled);
    let binding = shortcut.expect("a default shortcut");
    assert_eq!(binding.to_string(), "Command+Shift+F6");
    assert!(binding.modifiers.command, "uses Command, not a fixed Ctrl");
    let mut owners = 0;
    for action in Action::all().chain([Action::OpeningTrainer, Action::TacticsTrainer]) {
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
    drop(shell.update(Message::Ui(UiMessage::Action(Action::OpeningTrainer))));
    assert!(shell.app.opening_dialog_open());
    assert!(shell.modal_layer_open());
    drop(shell.update(key(NamedKey::Escape)));
    assert!(!shell.app.opening_dialog_open());
    assert!(!shell.modal_layer_open());
}

#[test]
fn enter_without_a_repertoire_keeps_the_dialog_open_and_says_why() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Open)));
    drop(shell.update(key(NamedKey::Enter)));
    assert!(shell.app.opening_dialog_open());
    assert_eq!(
        shell.app.opening_view().notice,
        Some(OpeningNotice::NoRepertoire)
    );
}

#[test]
fn the_dialog_is_drawn_over_the_window() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Open)));
    let element = mychess_gui::windows::main_window::view(&shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    assert!(sim.find(iced::widget::Id::new(OPENING_BOX_ID)).is_ok());
    assert!(sim.find(iced::widget::Id::new("board-pane")).is_ok());
}

/// Runs the effects of the trainer the way `perform` would, but in line.
fn settle(app: &mut AppState, effects: Vec<Effect>) {
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        let Effect::OpeningTrainer(effect) = effect else {
            continue;
        };
        let OpeningEffect::Build { request, job } = effect else {
            continue;
        };
        let answer = OpeningMessage::Built {
            request,
            result: build(&job).map(Box::new),
        };
        queue.extend(update(app, AppMessage::OpeningTrainer(answer)));
    }
}

fn started_shell(dir: &Path) -> Shell {
    let path = dir.join("rep.pgn");
    std::fs::write(&path, PGN).unwrap();
    let mut shell = Shell::new(None);
    let config = OpeningConfig {
        source: RepSourceKind::File,
        file: Some(path),
        side: TrainSide::White,
        ..OpeningConfig::default()
    };
    let effects = update(
        &mut shell.app,
        AppMessage::OpeningTrainer(OpeningMessage::Start {
            config: Box::new(config),
            seed: 1,
            today: 20_000,
        }),
    );
    settle(&mut shell.app, effects);
    assert_eq!(shell.app.opening_phase(), OpeningPhase::UserTurn);
    // Any message of the shell brings the visual state up to date.
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::FilePicked(None))));
    shell
}

fn play(shell: &mut Shell, uci: &str) {
    let mv = shell.app.session.position().parse_uci(uci).unwrap();
    let effects = update(
        &mut shell.app,
        AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }),
    );
    settle(&mut shell.app, effects);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::FilePicked(None))));
}

fn bar_visible(shell: &Shell) -> bool {
    let element = mychess_gui::windows::main_window::view(shell);
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    );
    sim.find(iced::widget::Id::new(OPENING_BAR_ID)).is_ok()
}

#[test]
fn the_panel_appears_under_the_board_with_a_session_and_goes_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    assert!(bar_visible(&shell));
    drop(shell.update(Message::App(AppMessage::OpeningTrainer(
        OpeningMessage::Close,
    ))));
    assert_eq!(shell.app.opening_phase(), OpeningPhase::Closed);
    assert!(!bar_visible(&shell));
}

#[test]
fn the_second_hint_draws_an_arrow_on_the_board_and_the_first_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    assert_eq!(shell.ui.dialogs.opening.arrows().len(), 0);
    drop(shell.update(Message::App(AppMessage::OpeningTrainer(
        OpeningMessage::Hint,
    ))));
    assert_eq!(
        shell.ui.dialogs.opening.arrows().len(),
        0,
        "level one is text"
    );
    drop(shell.update(Message::App(AppMessage::OpeningTrainer(
        OpeningMessage::Hint,
    ))));
    let arrows = shell.ui.dialogs.opening.arrows();
    assert_eq!(arrows.len(), 1);
    assert_eq!(arrows[0].from.to_string(), "e2");
    assert_eq!(arrows[0].to.to_string(), "e4");
}

#[test]
fn enter_goes_to_the_next_line_only_when_the_line_is_over() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    drop(shell.update(key(NamedKey::Enter)));
    assert_eq!(
        shell.app.opening_view().line,
        1,
        "while it is your move Enter does nothing"
    );
    play(&mut shell, "e2e4");
    play(&mut shell, "g1f3");
    assert!(matches!(
        shell.app.opening_phase(),
        OpeningPhase::LineDone(_)
    ));
    drop(shell.update(key(NamedKey::Enter)));
    assert_eq!(shell.app.opening_view().line, 2);
    assert_eq!(shell.app.opening_phase(), OpeningPhase::UserTurn);
}

#[test]
fn enter_behind_the_about_box_does_not_leak() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    play(&mut shell, "e2e4");
    play(&mut shell, "g1f3");
    drop(shell.update(Message::Ui(UiMessage::Action(Action::Help))));
    assert!(shell.modal_layer_open());
    drop(shell.update(key(NamedKey::Enter)));
    assert_eq!(shell.app.opening_view().line, 1);
}

#[test]
fn enter_in_a_text_field_does_not_skip_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = started_shell(dir.path());
    play(&mut shell, "e2e4");
    play(&mut shell, "g1f3");
    drop(shell.update(Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::NONE,
        },
        text_focused: true,
        typed: None,
    })));
    assert_eq!(shell.app.opening_view().line, 1);
}

#[test]
fn the_dialog_form_edits_a_draft_and_the_start_message_carries_the_clock() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Open)));
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Edit(
        OpeningEdit::Depth(24),
    ))));
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Edit(
        OpeningEdit::Side(TrainSide::Both),
    ))));
    let view = shell.app.opening_view();
    let start = shell.ui.dialogs.opening.to_start(&view);
    assert_eq!((start.max_depth, start.side), (24, TrainSide::Both));
    // The application keeps its own configuration until a start.
    assert_eq!(view.config.max_depth, OpeningConfig::default().max_depth);
    let AppMessage::OpeningTrainer(OpeningMessage::Start { config, today, .. }) =
        shell.ui.dialogs.opening.start_message(&view)
    else {
        panic!("a start message");
    };
    assert_eq!(config.max_depth, 24);
    assert!(today > 19_000, "days since 1970: {today}");
}

#[test]
fn a_file_picked_in_the_dialog_selects_the_file_source() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Open)));
    drop(
        shell.update(Message::OpeningUi(OpeningUiMessage::FilePicked(Some(
            "/x/rep.bin".into(),
        )))),
    );
    let view = shell.app.opening_view();
    let start = shell.ui.dialogs.opening.to_start(&view);
    assert_eq!(start.source, RepSourceKind::File);
    assert_eq!(start.file.as_deref(), Some(Path::new("/x/rep.bin")));
}

#[test]
fn the_reset_needs_two_clicks() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Open)));
    assert!(!shell.ui.dialogs.opening.reset_armed());
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Reset)));
    assert!(shell.ui.dialogs.opening.reset_armed());
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Reset)));
    assert!(!shell.ui.dialogs.opening.reset_armed());
}
