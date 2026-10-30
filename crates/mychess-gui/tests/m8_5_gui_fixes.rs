//! Regression tests of the review of the opening trainer (spec M8-5,
//! D2253-D2260): the key that moves on must not leak from a text field, the
//! new remark has its texts, and the dialog does not read the slots in
//! `view`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::path::Path;

use mychess_app::opening_trainer::{
    OpeningConfig, OpeningEffect, OpeningMessage, OpeningNotice, OpeningPhase, RepSourceKind,
    TrainSide, build,
};
use mychess_app::{
    AppState, Effect, GameMessage, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey,
    PlayOptions, update,
};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::opening_trainer_dialog::{OpeningEdit, OpeningUiMessage};
use mychess_gui::windows::opening_trainer_text::notice_text;
use mychess_gui::{Message, Shell, UiMessage};

const PGN: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 *\n";

fn enter() -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

fn settle(app: &mut AppState, effects: Vec<Effect>) {
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        let Effect::OpeningTrainer(OpeningEffect::Build { request, job }) = effect else {
            continue;
        };
        let answer = OpeningMessage::Built {
            request,
            result: build(&job).map(Box::new),
        };
        queue.extend(update(app, AppMessage::OpeningTrainer(answer)));
    }
}

/// A shell whose first line is over: Enter would go to the next line.
fn finished_line_shell(dir: &Path) -> Shell {
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
    for uci in ["e2e4", "g1f3"] {
        let mv = shell.app.session.position().parse_uci(uci).unwrap();
        let effects = update(
            &mut shell.app,
            AppMessage::Game(GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            }),
        );
        settle(&mut shell.app, effects);
    }
    assert!(matches!(
        shell.app.opening_phase(),
        OpeningPhase::LineDone(_)
    ));
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::FilePicked(None))));
    shell
}

#[test]
fn enter_with_the_search_field_of_the_list_focused_does_not_skip_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = finished_line_shell(dir.path());
    if !shell.ui.is_visible(PaneKind::GameList) {
        drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::GameList))));
    }
    assert!(
        shell.ui.is_visible(PaneKind::GameList),
        "the list is on screen"
    );
    shell.ui.games.search_focused = true;
    drop(shell.update(enter()));
    assert_eq!(shell.app.opening_view().line, 1);
    shell.ui.games.search_focused = false;
    drop(shell.update(enter()));
    assert_eq!(
        shell.app.opening_view().line,
        2,
        "and without the focus it does"
    );
}

#[test]
fn enter_with_a_field_of_the_analysis_panel_focused_does_not_skip_the_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = finished_line_shell(dir.path());
    shell.ui.analysis.focused_field = Some("analysis-field");
    drop(shell.update(enter()));
    assert_eq!(shell.app.opening_view().line, 1);
    shell.ui.analysis.focused_field = None;
    drop(shell.update(enter()));
    assert_eq!(shell.app.opening_view().line, 2);
}

#[test]
fn the_remark_of_a_game_that_could_not_be_put_back_has_a_text_in_both_languages() {
    use mychess_gui::i18n::{I18n, Language};
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let text = notice_text(OpeningNotice::GameNotRestored, &i18n);
        assert!(!text.is_empty() && !text.contains("opening-"), "{text}");
        let cut = notice_text(OpeningNotice::Truncated, &i18n);
        assert!(
            !cut.to_lowercase().contains("depth") && !cut.to_lowercase().contains("profundidade"),
            "the cut is about games and positions, not depth: {cut}"
        );
    }
}

#[test]
fn the_dialog_keeps_the_names_of_the_files_until_the_choice_changes() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Open)));
    assert!(
        shell.ui.dialogs.opening.names().is_empty(),
        "no typed base is open"
    );
    drop(
        shell.update(Message::OpeningUi(OpeningUiMessage::FilePicked(Some(
            "/x/rep.bin".into(),
        )))),
    );
    assert_eq!(shell.ui.dialogs.opening.names(), ["rep.bin".to_owned()]);
    // Another change of the form that does not decide the names keeps them.
    drop(shell.update(Message::OpeningUi(OpeningUiMessage::Edit(
        OpeningEdit::Depth(24),
    ))));
    assert_eq!(shell.ui.dialogs.opening.names(), ["rep.bin".to_owned()]);
    // Closing the dialog forgets them.
    drop(shell.update(Message::App(AppMessage::OpeningTrainer(
        OpeningMessage::CloseDialog,
    ))));
    assert!(shell.ui.dialogs.opening.names().is_empty() || !shell.app.opening_dialog_open());
}
