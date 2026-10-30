//! M3-8 GUI contract: the "Import PGN file..." item, the file dialogs and
//! the window of the progress and the report (spec
//! `docs/specs/m3-8-importar-arquivo-pgn.md`, D1970-D1989), headless.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

#[path = "m5_2_gui_common/mod.rs"]
mod base;

use std::path::{Path, PathBuf};

use base::{Message, TreeRig, named, sent};
use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{
    AppNotice, Effect, FileImportMessage, LanguagePref, ListMessage, Message as AppMessage,
    NamedKey, PickKind, PickSpec, update,
};
use mychess_gui::host::{HostWork, PickDialog, classify};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::import_file_dialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use tempfile::tempdir;

fn game(n: usize) -> String {
    format!(
        "[Event \"E{n}\"]\n[White \"White {n}\"]\n[Black \"Black {n}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n"
    )
}

fn games(count: usize) -> String {
    (1..=count).map(game).collect()
}

fn pgn(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// "Import PGN file..." through the application of the rig; the dialog is
/// not shown (the answer is given by hand). Returns the effects of the
/// answer, not run.
fn begin(rig: &mut TreeRig, files: &[PathBuf]) -> Vec<Effect> {
    let effects = update(
        &mut rig.rig.shell.app,
        AppMessage::FileImport(FileImportMessage::Open),
    );
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) if spec.kind == PickKind::ImportPgn => Some(spec),
            _ => None,
        })
        .expect("a file dialog");
    update(
        &mut rig.rig.shell.app,
        AppMessage::FilesPicked(spec.request, files.to_vec()),
    )
}

/// The box of the window alone.
fn box_sim(rig: &TreeRig) -> Simulator<'_, Message> {
    let element = import_file_dialog::view(rig.app(), &rig.rig.shell.i18n).expect("window open");
    Simulator::with_size(Settings::default(), Size::new(1000.0, 900.0), element)
}

fn box_shows(rig: &TreeRig, text: &str) -> bool {
    box_sim(rig).find(text).is_ok()
}

/// Runs the job the way the host does and feeds the answer to the shell.
fn finish(rig: &mut TreeRig, effects: Vec<Effect>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for effect in effects {
        let answer = rt.block_on(mychess_app::perform(effect));
        rig.app_msg(answer);
    }
}

fn file_menu(rig: &TreeRig) -> Vec<MenuEntry> {
    let shell = &rig.rig.shell;
    menu_specs(&shell.app, &shell.ui, shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-file")
        .expect("File menu")
        .entries
}

fn import_item(entries: &[MenuEntry]) -> (usize, bool, bool, Option<Message>) {
    entries
        .iter()
        .enumerate()
        .find_map(|(i, e)| match e {
            MenuEntry::Item {
                label: Label::Key("menu-file-import-pgn-file"),
                action,
                shortcut,
                ..
            } => Some((i, action.is_some(), shortcut.is_some(), action.clone())),
            _ => None,
        })
        .expect("the item exists")
}

fn position_of(entries: &[MenuEntry], key: &str) -> usize {
    entries
        .iter()
        .position(|e| matches!(e, MenuEntry::Item { label: Label::Key(k), .. } if *k == key))
        .unwrap_or_else(|| panic!("{key}"))
}

// ---- the menu ----

#[test]
fn d1970_the_item_is_in_the_file_menu_after_append_enabled_and_without_a_key() {
    let dir = tempdir().unwrap();
    for case in ["empty", "pgn", "si4"] {
        let mut rig = TreeRig::new();
        match case {
            "pgn" => rig.open_pgn(dir.path(), "a.pgn", &games(2)),
            "si4" => rig.open_si4(dir.path(), "b", &games(2)),
            _ => {}
        }
        let entries = file_menu(&rig);
        let (at, enabled, shortcut, message) = import_item(&entries);
        assert!(at > position_of(&entries, "menu-file-append"), "{case}");
        assert!(enabled, "{case}: enabled with any base");
        assert!(!shortcut, "{case}: no default key");
        assert!(matches!(
            message,
            Some(Message::App(AppMessage::FileImport(
                FileImportMessage::Open
            )))
        ));
    }
}

#[test]
fn d1970_the_action_exists_and_has_no_default_key() {
    use mychess_app::{Action, Shortcuts};
    assert_eq!(
        Action::from_name("ImportPgnFile"),
        Some(Action::ImportPgnFile)
    );
    assert_eq!(Action::ImportPgnFile.name(), "ImportPgnFile");
    let map = Shortcuts::defaults();
    assert!(map.bindings(Action::ImportPgnFile).is_empty());
}

#[test]
fn d1989_the_item_is_translated_in_both_languages() {
    assert_eq!(
        I18n::new(Language::EnUs).tr("menu-file-import-pgn-file"),
        "Import PGN file..."
    );
    assert_eq!(
        I18n::new(Language::PtBr).tr("menu-file-import-pgn-file"),
        "Importar arquivo PGN..."
    );
}

// ---- the file dialogs ----

fn spec(kind: PickKind, name: Option<&str>) -> PickSpec {
    PickSpec {
        request: mychess_app::RequestId(3),
        kind,
        initial_dir: Some(PathBuf::from("games")),
        file_name: name.map(str::to_owned),
    }
}

#[test]
fn d1971_the_import_dialog_takes_several_pgn_files() {
    for (language, title, filter) in [
        (Language::EnUs, "Import PGN files", "PGN files"),
        (Language::PtBr, "Importar arquivos PGN", "Arquivos PGN"),
    ] {
        let i18n = I18n::new(language);
        let HostWork::Pick(dialog) =
            classify(Effect::PickFile(spec(PickKind::ImportPgn, None)), &i18n)
        else {
            panic!("a file dialog");
        };
        assert!(dialog.is_multiple());
        assert!(!dialog.is_save());
        assert_eq!(dialog.title, title);
        assert_eq!(dialog.filters[0].0, filter);
        for wanted in ["pgn", "gz"] {
            assert!(dialog.filters[0].1.contains(&wanted), "{wanted}");
        }
        assert_eq!(dialog.filters.last().unwrap().1, ["*"]);
    }
    assert_eq!(
        mychess_gui::host::picked_many_message(mychess_app::RequestId(3), vec![PathBuf::from("a")]),
        AppMessage::FilesPicked(mychess_app::RequestId(3), vec![PathBuf::from("a")])
    );
}

#[test]
fn d1974_the_new_base_dialog_saves_a_si4_with_the_suggested_name() {
    for (language, title, filter) in [
        (Language::EnUs, "Name of the new database", "Scid databases"),
        (Language::PtBr, "Nome da nova base", "Bases do Scid"),
    ] {
        let i18n = I18n::new(language);
        let dialog = PickDialog::new(&spec(PickKind::NewBase, Some("Games.si4")), &i18n);
        assert!(dialog.is_save());
        assert!(!dialog.is_multiple());
        assert_eq!(dialog.title, title);
        assert_eq!(dialog.filters[0].0, filter);
        assert!(dialog.filters[0].1.contains(&"si4"));
        assert_eq!(dialog.file_name.as_deref(), Some("Games.si4"));
        assert_eq!(dialog.directory.as_deref(), Some(Path::new("games")));
    }
}

// ---- the window ----

#[test]
fn d1985_the_running_window_is_modal_shows_the_progress_and_stops_with_esc() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(2));
    let a = pgn(dir.path(), "a.pgn", &games(3));
    let b = pgn(dir.path(), "b.pgn", &games(3));
    let effects = begin(&mut rig, &[a, b]);
    assert!(rig.rig.shell.modal_layer_open());
    for text in [
        "Importing PGN files",
        "Database: base.si4",
        "Files (2)",
        "a.pgn",
        "File 1 of 2 - 0 games stored - 0%",
        "Stop",
    ] {
        assert!(box_shows(&rig, text), "{text}");
    }
    assert!(!box_shows(&rig, "Close"), "no Close while it runs");
    // Enter does nothing while it runs; Esc is "Stop".
    rig.key(named(NamedKey::Enter));
    assert!(rig.app().list_job().is_some());
    assert!(!rig.app().list_job().unwrap().1.is_cancelled());
    rig.key(named(NamedKey::Escape));
    assert!(rig.app().list_job().unwrap().1.is_cancelled());
    assert!(rig.rig.shell.modal_layer_open(), "Esc does not close it");
    finish(&mut rig, effects);
    assert!(!rig.rig.shell.modal_layer_open());
}

#[test]
fn d1985_the_stop_button_sends_the_cancel_of_the_job() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(1));
    let a = pgn(dir.path(), "a.pgn", &games(2));
    let b = pgn(dir.path(), "b.pgn", &games(2));
    let _effects = begin(&mut rig, &[a, b]);
    let mut sim = box_sim(&rig);
    sim.click("Stop").unwrap();
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(sent(
        &messages,
        &Message::App(AppMessage::Bases(mychess_app::BasesMessage::List(
            ListMessage::CancelJob
        )))
    ));
}

#[test]
fn d1983_the_report_is_shown_and_enter_and_esc_close_it() {
    let dir = tempdir().unwrap();
    for key in [NamedKey::Enter, NamedKey::Escape] {
        let mut rig = TreeRig::new();
        rig.open_si4(dir.path(), &format!("base{key:?}"), &games(2));
        let a = pgn(dir.path(), "a.pgn", &games(2));
        let b = pgn(dir.path(), "b.pgn", &games(4));
        let effects = begin(&mut rig, &[a, b]);
        finish(&mut rig, effects);
        assert!(rig.rig.shell.modal_layer_open());
        for text in [
            "Games found: 6. Imported: 6. Not imported: 0. Warnings: 0.",
            "Close",
        ] {
            assert!(box_shows(&rig, text), "{text}");
        }
        assert!(!box_shows(&rig, "Stop"));
        rig.key(named(key));
        assert!(!rig.rig.shell.modal_layer_open(), "{key:?} closes");
        assert!(rig.app().file_import_view().is_none());
    }
}

#[test]
fn d1984_a_clean_single_file_closes_the_window_and_the_status_bar_tells() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(2));
    let a = pgn(dir.path(), "a.pgn", &games(3));
    let effects = begin(&mut rig, &[a]);
    finish(&mut rig, effects);
    assert!(!rig.rig.shell.modal_layer_open());
    assert!(rig.shows("3 games imported"));
    assert!(matches!(
        rig.rig.shell.app.notice,
        Some(AppNotice::FileImport(_)) | None
    ));
}

#[test]
fn d1989_the_report_lines_name_the_file_the_game_and_the_line() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(1));
    let mut text = game(1);
    text.push_str(
        "[Event \"Bad\"]\n[White \"X\"]\n[Black \"Y\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Qh8 Nf6 1-0\n\n",
    );
    let bad = pgn(dir.path(), "bad.pgn", &text);
    let missing = dir.path().join("missing.pgn");
    let effects = begin(&mut rig, &[bad, missing]);
    finish(&mut rig, effects);
    let view = rig.app().file_import_view().expect("window");
    let result = view.result.expect("result");
    assert!(!result.report.lines.is_empty());
    let line =
        import_file_dialog::line_text(&rig.rig.shell.i18n, view.files, &result.report.lines[0]);
    assert!(line.starts_with("bad.pgn, game 2, line "), "{line}");
    assert!(box_shows(&rig, &line));
    assert_eq!(
        result.report.file_errors.len(),
        1,
        "the file that cannot be opened"
    );
}

#[test]
fn d1989_the_window_speaks_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_si4(dir.path(), "base", &games(1));
    let a = pgn(dir.path(), "a.pgn", &games(2));
    let b = pgn(dir.path(), "b.pgn", &games(2));
    let effects = begin(&mut rig, &[a, b]);
    for text in ["Importando arquivos PGN", "Banco: base.si4", "Parar"] {
        assert!(box_shows(&rig, text), "{text}");
    }
    finish(&mut rig, effects);
    for text in [
        "Partidas encontradas: 4. Importadas: 4. Não importadas: 0. Avisos: 0.",
        "Fechar",
    ] {
        assert!(box_shows(&rig, text), "{text}");
    }
}

#[test]
fn d1997_the_stop_and_failure_remarks_have_a_plural() {
    use mychess_app::{BaseError, FileImportNotice as N};
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let text = |i18n: &I18n, notice: &N| import_file_dialog::notice_text(i18n, notice);
    assert_eq!(
        text(&en, &N::Interrupted { count: 1 }),
        "Import stopped: one game was stored"
    );
    assert_eq!(
        text(&en, &N::Interrupted { count: 5 }),
        "Import stopped: 5 games were stored"
    );
    assert_eq!(
        text(&pt, &N::Interrupted { count: 1 }),
        "Importação interrompida: uma partida foi gravada"
    );
    let failed = |count| N::Failed {
        count,
        error: BaseError::ReadOnly,
    };
    assert!(text(&en, &failed(1)).contains("after one game"));
    assert!(text(&en, &failed(2)).contains("after 2 games"));
    assert!(text(&pt, &failed(1)).contains("depois de uma partida"));
    assert!(text(&pt, &failed(2)).contains("depois de 2 partidas"));
}
