//! Regression tests of the review of the tactics training in the GUI (spec
//! M8-4, D1949, D1953): the shortcut does not open the dialog over a live
//! session, and the dialog comes back with the reason when the session ends
//! because its database changed.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::path::Path;

use mychess_app::tactics::{
    TacticsConfig, TacticsEffect, TacticsMessage, TacticsNotice, TacticsPhase, find_next, judge,
    open_file,
};
use mychess_app::{Action, AppState, Effect, Message as AppMessage, update};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::tactics_text::notice_text;
use mychess_gui::{Message, Shell, UiMessage};

fn settle(app: &mut AppState, effects: Vec<Effect>) {
    let mut queue: VecDeque<Effect> = effects.into();
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

fn live_shell(dir: &Path) -> Shell {
    let path = dir.join("p.pgn");
    std::fs::write(
        &path,
        "[Event \"One\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n",
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
    shell
}

#[test]
fn the_shortcut_with_a_live_session_does_not_open_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = live_shell(dir.path());
    drop(shell.update(Message::Ui(UiMessage::Action(Action::TacticsTrainer))));
    assert!(!shell.app.tactics_dialog_open());
    assert!(!shell.modal_layer_open());
    assert_eq!(shell.app.tactics_phase(), TacticsPhase::Solving);
}

#[test]
fn the_reason_the_session_ended_has_a_text_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in [TacticsNotice::SourceChanged, TacticsNotice::StoreNewer] {
            let text = notice_text(notice, &i18n);
            assert!(!text.is_empty() && !text.contains("tactics-"), "{text}");
        }
    }
}

#[test]
fn the_search_for_mates_is_on_by_default_and_the_dialog_can_turn_it_off() {
    use mychess_gui::windows::tactics_dialog::{TacticsEdit, TacticsUiMessage};
    let mut shell = Shell::new(None);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Open)));
    let view = shell.app.tactics_view();
    assert!(shell.ui.dialogs.tactics.to_start(&view).scan_mates);
    drop(shell.update(Message::TacticsUi(TacticsUiMessage::Edit(
        TacticsEdit::ScanMates(false),
    ))));
    assert!(!shell.ui.dialogs.tactics.to_start(&view).scan_mates);
}
