//! M8-6 fixes after the review (GUI, spec D2350-D2363): the example in the
//! field of moves is a line the program accepts, in both languages, and the
//! draft of the dialog survives the late reading of `calvar.toml`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use mychess_app::calvar::{CalvarMessage, CalvarStore, LoadedStore, PositionSource, read_moves};
use mychess_app::{Message as AppMessage, update};
use mychess_core::Position;
use mychess_gui::windows::calvar_dialog::{CalvarEdit, CalvarUiMessage};
use mychess_gui::{Message, Shell};

fn ftl(language: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("calvar.ftl");
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .collect()
}

#[test]
fn the_example_of_the_field_of_moves_is_accepted_by_the_program_in_both_languages() {
    for language in ["en-US", "pt-BR"] {
        let texts = ftl(language);
        let placeholder = &texts["calvar-entry-placeholder"];
        let example = placeholder.rsplit_once(": ").expect("an example").1;
        let read = read_moves(&Position::new(), example);
        assert_eq!(read.rejected, None, "{language}: {example}");
        assert!(read.moves.len() >= 3, "{language}: {example}");
    }
}

#[test]
fn the_tip_of_the_field_says_the_piece_letters_are_in_english() {
    for language in ["en-US", "pt-BR"] {
        let tip = &ftl(language)["calvar-entry-tip"];
        assert!(tip.contains("N, B, R, Q, K"), "{language}: {tip}");
    }
    assert!(ftl("pt-BR")["calvar-entry-tip"].contains("inglês"));
}

#[test]
fn what_was_saved_appears_under_the_changes_made_before_it_arrived() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    // The user edits before the file is read.
    drop(
        shell.update(Message::CalvarUi(CalvarUiMessage::Edit(CalvarEdit::Lines(
            5,
        )))),
    );
    let mut store = CalvarStore::default();
    store.config.multipv = 8;
    store.config.lines_required = 2;
    store.config.follow_board = true;
    drop(update(
        &mut shell.app,
        AppMessage::Calvar(CalvarMessage::StoreLoaded(Box::new(LoadedStore {
            store,
            ..LoadedStore::default()
        }))),
    ));
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Cancel)));
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    // A new session of the dialog: the draft is gone, the saved values show.
    let view = shell.app.calvar_view();
    let shown = shell.ui.dialogs.calvar.to_start(&view);
    assert_eq!(shown.multipv, 8);
    assert_eq!(shown.lines_required, 2);
    assert!(shown.follow_board);
}

#[test]
fn a_change_made_before_the_file_arrived_wins_over_the_saved_value() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    drop(
        shell.update(Message::CalvarUi(CalvarUiMessage::Edit(CalvarEdit::Lines(
            5,
        )))),
    );
    let mut store = CalvarStore::default();
    store.config.multipv = 8;
    store.config.lines_required = 2;
    drop(update(
        &mut shell.app,
        AppMessage::Calvar(CalvarMessage::StoreLoaded(Box::new(LoadedStore {
            store,
            ..LoadedStore::default()
        }))),
    ));
    let view = shell.app.calvar_view();
    let shown = shell.ui.dialogs.calvar.to_start(&view);
    assert_eq!(shown.lines_required, 5, "the user's change");
    assert_eq!(shown.multipv, 8, "the saved value the user did not touch");
}

#[test]
fn typing_a_fen_keeps_only_the_last_text_and_the_order_of_the_other_changes() {
    let mut shell = Shell::new(None);
    drop(shell.update(Message::CalvarUi(CalvarUiMessage::Open)));
    for edit in [
        CalvarEdit::Source(PositionSource::Fen),
        CalvarEdit::Fen("a".into()),
        CalvarEdit::Fen("ab".into()),
        CalvarEdit::Fen("abc".into()),
        CalvarEdit::MultiPv(4),
    ] {
        drop(shell.update(Message::CalvarUi(CalvarUiMessage::Edit(edit))));
    }
    let view = shell.app.calvar_view();
    let shown = shell.ui.dialogs.calvar.to_start(&view);
    assert_eq!(shown.fen, "abc");
    assert_eq!(shown.source, PositionSource::Fen);
    assert_eq!(shown.multipv, 4);
}
