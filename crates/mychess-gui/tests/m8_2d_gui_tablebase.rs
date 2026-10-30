//! M8-2d (GUI): the tablebase rule of the tournament form (spec D2540,
//! D2541): offered only with a probe, the pieces field, and the texts in both
//! languages.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_app::tournament::{ConfigError, TournamentNotice};
use mychess_app::{AppState, engines::EngineDraft};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::tournament_draft::{DraftEdit, TField, TournamentDraft};
use mychess_gui::windows::tournament_text::notice_text;

fn app(with_probe: bool) -> AppState {
    let mut app = AppState::default();
    for name in ["Alpha", "Beta"] {
        app.engines
            .add(EngineDraft::new(name, "fake-engine"))
            .expect("engine added");
    }
    if with_probe {
        app.prefs.tablebase.local = true;
        app.prefs.tablebase.directories = vec!["tables".to_owned()];
    }
    app
}

#[test]
fn without_a_probe_the_saved_rule_is_switched_off() {
    let mut app = app(false);
    app.prefs.tournament.adjudication.tablebase = true;
    assert!(!app.tournament_tablebase_available());
    let draft = TournamentDraft::new(&app, String::new());
    assert!(!draft.to_config().adjudication.tablebase);
}

#[test]
fn local_on_without_folders_is_no_probe() {
    let mut app = app(false);
    app.prefs.tablebase.local = true;
    app.prefs.tournament.adjudication.tablebase = true;
    let draft = TournamentDraft::new(&app, String::new());
    assert!(!draft.to_config().adjudication.tablebase);
}

#[test]
fn with_a_probe_the_saved_rule_is_kept() {
    let mut app = app(true);
    app.prefs.tournament.adjudication.tablebase = true;
    assert!(app.tournament_tablebase_available());
    let draft = TournamentDraft::new(&app, String::new());
    assert!(draft.to_config().adjudication.tablebase);
}

#[test]
fn turning_the_rule_on_fills_the_pieces_with_five() {
    let app = app(true);
    let mut draft = TournamentDraft::new(&app, String::new());
    draft.edit(DraftEdit::Tablebase(true), &app);
    assert!(draft.to_config().adjudication.tablebase);
    assert_eq!(draft.text(TField::TbPieces), "5");
    assert_eq!(draft.to_config().adjudication.tb_pieces, 5);
    draft.edit(DraftEdit::Tablebase(false), &app);
    assert!(!draft.to_config().adjudication.tablebase);
}

#[test]
fn the_pieces_field_goes_to_the_configuration() {
    let app = app(true);
    let mut draft = TournamentDraft::new(&app, String::new());
    draft.edit(DraftEdit::Tablebase(true), &app);
    for (text, expected) in [("1", 1), ("6", 6), ("7", 7)] {
        draft.edit(DraftEdit::Number(TField::TbPieces, text.to_owned()), &app);
        assert_eq!(draft.to_config().adjudication.tb_pieces, expected);
        assert!(draft.error().is_none(), "{text}: {:?}", draft.error());
    }
}

#[test]
fn pieces_out_of_range_are_an_adjudication_error() {
    let app = app(true);
    let mut draft = TournamentDraft::new(&app, String::new());
    draft.edit(DraftEdit::Tablebase(true), &app);
    for text in ["9", "8", "0"] {
        draft.edit(DraftEdit::Number(TField::TbPieces, text.to_owned()), &app);
        assert_eq!(
            draft.error(),
            Some(&ConfigError::BadAdjudication("tablebase pieces")),
            "{text}"
        );
    }
    draft.edit(DraftEdit::Number(TField::TbPieces, "6".to_owned()), &app);
    assert!(draft.error().is_none());
}

#[test]
fn the_new_texts_exist_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in [
            "tournament-adj-tablebase",
            "tournament-adj-tb-pieces",
            "tournament-unit-pieces",
            "tournament-notice-tablebase-unavailable",
        ] {
            let text = i18n.tr(key);
            assert!(
                !text.is_empty() && text != key,
                "{language:?} {key}: {text}"
            );
        }
        let notice = notice_text(&i18n, &TournamentNotice::TablebaseUnavailable, &[]);
        assert_eq!(notice, i18n.tr("tournament-notice-tablebase-unavailable"));
    }
    let en = I18n::new(Language::EnUs).tr("tournament-adj-tablebase");
    let pt = I18n::new(Language::PtBr).tr("tournament-adj-tablebase");
    assert_ne!(en, pt);
}
