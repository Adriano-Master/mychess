//! M3-9 GUI contract: the "Export games..." item, the window of the options,
//! the progress and the report (spec `docs/specs/m3-9-exportar-partidas.md`,
//! D2000-D2004, D2017), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_collect
)]

#[path = "m5_2_gui_common/mod.rs"]
mod base;

use std::path::{Path, PathBuf};

use base::{Message, TreeRig, named, sent};
use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{
    Action, AppNotice, Effect, ExportSkipCause, GamesExportMessage, GamesExportNotice,
    GamesExportReport, GamesExportResult, LanguagePref, ListMessage, Message as AppMessage,
    NamedKey, PickKind, Shortcuts, update,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::icon::Icon;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::export_games_dialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use tempfile::tempdir;

fn games(count: usize) -> String {
    (1..=count)
        .map(|n| {
            format!(
                "[Event \"E{n}\"]\n[White \"White {n}\"]\n[Black \"Black {n}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n"
            )
        })
        .collect()
}

fn app_update(rig: &mut TreeRig, message: GamesExportMessage) -> Vec<Effect> {
    update(&mut rig.rig.shell.app, AppMessage::GamesExport(message))
}

/// The box of the window alone.
fn box_sim(rig: &TreeRig) -> Simulator<'_, Message> {
    let element = export_games_dialog::view(rig.app(), &rig.rig.shell.i18n).expect("window open");
    Simulator::with_size(Settings::default(), Size::new(1000.0, 900.0), element)
}

fn box_shows(rig: &TreeRig, text: &str) -> bool {
    box_sim(rig).find(text).is_ok()
}

/// Chooses `dest` in the dialog by hand; returns the effects of the answer.
fn choose(rig: &mut TreeRig, dest: &Path) -> Vec<Effect> {
    let effects = app_update(rig, GamesExportMessage::Choose);
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .expect("a file dialog");
    assert_eq!(spec.kind, PickKind::SavePgn);
    update(
        &mut rig.rig.shell.app,
        AppMessage::FilePicked(spec.request, Some(dest.to_path_buf())),
    )
}

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

fn position_of(entries: &[MenuEntry], key: &str) -> usize {
    entries
        .iter()
        .position(|e| matches!(e, MenuEntry::Item { label: Label::Key(k), .. } if *k == key))
        .unwrap_or_else(|| panic!("{key}"))
}

// ---- the menu ----

#[test]
fn d2000_the_item_is_in_the_file_menu_enabled_with_an_icon_and_without_a_key() {
    let dir = tempdir().unwrap();
    for case in ["empty", "pgn", "si4"] {
        let mut rig = TreeRig::new();
        match case {
            "pgn" => rig.open_pgn(dir.path(), "a.pgn", &games(2)),
            "si4" => rig.open_si4(dir.path(), "b", &games(2)),
            _ => {}
        }
        let entries = file_menu(&rig);
        let at = position_of(&entries, "menu-file-export-games");
        assert!(at > position_of(&entries, "menu-file-append"), "{case}");
        assert!(
            at > position_of(&entries, "menu-file-import-pgn-file"),
            "{case}"
        );
        match &entries[at] {
            MenuEntry::Item {
                action,
                shortcut,
                icon,
                ..
            } => {
                assert!(shortcut.is_none(), "{case}: no default key");
                assert_eq!(*icon, Some(Icon::Upload), "{case}");
                assert!(matches!(
                    action,
                    Some(Message::App(AppMessage::GamesExport(
                        GamesExportMessage::Open
                    )))
                ));
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn d2000_the_action_exists_and_has_no_default_key() {
    assert_eq!(Action::from_name("ExportGames"), Some(Action::ExportGames));
    assert_eq!(Action::ExportGames.name(), "ExportGames");
    assert!(
        Shortcuts::defaults()
            .bindings(Action::ExportGames)
            .is_empty()
    );
}

#[test]
fn d2000_the_icon_is_the_embedded_lucide_upload() {
    assert_eq!(Icon::Upload.file_stem(), "upload");
    assert!(
        std::str::from_utf8(Icon::Upload.bytes())
            .unwrap()
            .contains("<svg")
    );
}

#[test]
fn d2000_the_item_and_the_action_are_translated_in_both_languages() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    assert_eq!(en.tr("menu-file-export-games"), "Export games...");
    assert_eq!(pt.tr("menu-file-export-games"), "Exportar partidas...");
    assert_eq!(en.tr("action-export-games"), "Export games");
    assert_eq!(pt.tr("action-export-games"), "Exportar partidas");
    assert_eq!(pt.tr("games-job-export"), "Exportando partidas");
    assert!(!pt.tr("export-games-marks").contains("variação"));
    assert!(pt.tr("export-games-variations").contains("variantes"));
}

// ---- the options ----

#[test]
fn d2001_the_options_window_is_modal_and_lists_the_choices() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(5));
    app_update(&mut rig, GamesExportMessage::Open);
    assert!(rig.rig.shell.modal_layer_open());
    assert_eq!(
        ActiveDialog::of(rig.app(), &rig.rig.shell.ui.dialogs),
        Some(ActiveDialog::ExportGames)
    );
    for text in [
        "Export games",
        "The selected games (0)",
        "Include comments",
        "Include variations",
        "Only games with the Tactics (T) flag",
        "Append to the end of the file",
        "Choose destination...",
        "Cancel",
    ] {
        assert!(box_shows(&rig, text), "{text}");
    }
}

#[test]
fn d2001_the_choices_have_their_counts_in_both_languages() {
    use fluent_bundle::FluentArgs;
    let mut args = FluentArgs::new();
    args.set("count", 5_i64);
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    assert_eq!(
        en.tr_args("export-games-scope-all", &args),
        "All the games of the database (5)"
    );
    assert_eq!(
        en.tr_args("export-games-scope-filter", &args),
        "The games of the filter (5)"
    );
    assert_eq!(
        pt.tr_args("export-games-scope-selection", &args),
        "As partidas selecionadas (5)"
    );
    assert_eq!(en.tr("export-games-scope-current"), "The current game");
    assert_eq!(pt.tr("export-games-scope-current"), "A partida atual");
}

#[test]
fn d2001_the_window_speaks_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_si4(dir.path(), "base", &games(2));
    app_update(&mut rig, GamesExportMessage::Open);
    for text in [
        "Exportar partidas",
        "Incluir variantes",
        "Somente partidas com a bandeira Tática (T)",
        "Escolher destino...",
        "Cancelar",
    ] {
        assert!(box_shows(&rig, text), "{text}");
    }
}

#[test]
fn d2001_the_buttons_send_their_messages() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(2));
    app_update(&mut rig, GamesExportMessage::Open);
    let mut sim = box_sim(&rig);
    sim.click("Choose destination...").unwrap();
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(sent(
        &messages,
        &Message::App(AppMessage::GamesExport(GamesExportMessage::Choose))
    ));
    let mut sim = box_sim(&rig);
    sim.click("Cancel").unwrap();
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(sent(
        &messages,
        &Message::App(AppMessage::GamesExport(GamesExportMessage::Close))
    ));
}

#[test]
fn d2017_esc_closes_the_options_and_enter_asks_for_the_file() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(2));
    app_update(&mut rig, GamesExportMessage::Open);
    // Enter: checked on the dialog layer, without running the host dialog.
    let out = rig.rig.shell.ui.dialogs.key(
        ActiveDialog::ExportGames,
        &named(NamedKey::Enter),
        false,
        &rig.rig.shell.app,
    );
    assert_eq!(
        out.messages,
        vec![AppMessage::GamesExport(GamesExportMessage::Choose)]
    );
    rig.key(named(NamedKey::Escape));
    assert!(!rig.rig.shell.modal_layer_open());
    assert!(rig.app().games_export_view().is_none());
}

// ---- the job ----

#[test]
fn d2003_the_running_window_shows_the_progress_stops_with_esc_and_ignores_enter() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(3));
    app_update(&mut rig, GamesExportMessage::Open);
    let dest = dir.path().join("out.pgn");
    let effects = choose(&mut rig, &dest);
    assert!(rig.rig.shell.modal_layer_open());
    for text in ["File: out.pgn", "0 of 0 games - 0%", "Stop"] {
        assert!(box_shows(&rig, text), "{text}");
    }
    assert!(!box_shows(&rig, "Close"));
    rig.key(named(NamedKey::Enter));
    assert!(!rig.app().list_job().unwrap().1.is_cancelled());
    rig.key(named(NamedKey::Escape));
    assert!(rig.app().list_job().unwrap().1.is_cancelled());
    assert!(rig.rig.shell.modal_layer_open(), "Esc does not close it");
    finish(&mut rig, effects);
    assert!(!rig.rig.shell.modal_layer_open());
    assert!(!dest.exists(), "stopped: nothing written");
    assert!(rig.shows("Export stopped: the file was not written"));
}

#[test]
fn d2003_the_stop_button_sends_the_cancel_of_the_job() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(3));
    app_update(&mut rig, GamesExportMessage::Open);
    let _effects = choose(&mut rig, &dir.path().join("o.pgn"));
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
fn d2004_a_clean_export_closes_the_window_and_the_status_bar_tells() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(3));
    app_update(&mut rig, GamesExportMessage::Open);
    let dest = dir.path().join("out.pgn");
    let effects = choose(&mut rig, &dest);
    finish(&mut rig, effects);
    assert!(!rig.rig.shell.modal_layer_open());
    assert!(rig.shows("3 games exported to out.pgn"));
    // The shell may already have taken the remark for the status bar.
    assert!(matches!(
        rig.rig.shell.app.notice,
        Some(AppNotice::GamesExport(GamesExportNotice::Exported {
            count: 3,
            ..
        })) | None
    ));
    assert_eq!(
        std::fs::read_to_string(&dest)
            .unwrap()
            .matches("[Event ")
            .count(),
        3
    );
}

#[test]
fn d2004_a_refusal_stays_as_a_report_and_enter_and_esc_close_it() {
    let dir = tempdir().unwrap();
    for key in [NamedKey::Enter, NamedKey::Escape] {
        let mut rig = TreeRig::new();
        let pgn = dir.path().join(format!("src{key:?}.pgn"));
        std::fs::write(&pgn, games(2)).unwrap();
        rig.open_pgn(dir.path(), &format!("src{key:?}.pgn"), &games(2));
        app_update(&mut rig, GamesExportMessage::Open);
        let effects = choose(&mut rig, &pgn);
        finish(&mut rig, effects);
        assert!(rig.rig.shell.modal_layer_open());
        for text in [
            "The export failed: The destination is the file of an open database: choose another name",
            "Close",
        ] {
            assert!(box_shows(&rig, text), "{text}");
        }
        assert!(!box_shows(&rig, "Stop"));
        rig.key(named(key));
        assert!(!rig.rig.shell.modal_layer_open(), "{key:?} closes");
    }
}

#[test]
fn d2004_the_report_lines_of_skipped_games() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let result = GamesExportResult {
        path: PathBuf::from("x.pgn"),
        report: GamesExportReport {
            written: 4,
            excluded: 2,
            skipped: 1,
            lines: vec![(7, ExportSkipCause::Unreadable)],
            interrupted: false,
            appended: true,
            ..GamesExportReport::default()
        },
        error: None,
    };
    let lines = export_games_dialog::summary_lines(&result, &en);
    assert_eq!(
        lines[0],
        "Games exported: 4. Left out for lacking the T flag: 2. Not exported: 1."
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(
        export_games_dialog::skip_line(&en, 7, ExportSkipCause::Unreadable),
        "Game 7: The game could not be read from the database"
    );
    assert_eq!(
        export_games_dialog::skip_line(&pt, 7, ExportSkipCause::Unwritable),
        "Partida 7: A partida não pôde ser escrita como PGN"
    );
}

#[test]
fn d2004_the_remarks_have_a_plural_in_both_languages() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let text = |i18n: &I18n, n: &GamesExportNotice| export_games_dialog::notice_text(i18n, n);
    let one = GamesExportNotice::Exported {
        count: 1,
        name: "a.pgn".into(),
    };
    let many = GamesExportNotice::Exported {
        count: 20,
        name: "a.pgn".into(),
    };
    assert_eq!(text(&en, &one), "One game exported to a.pgn");
    assert_eq!(text(&en, &many), "20 games exported to a.pgn");
    assert_eq!(text(&pt, &one), "Uma partida exportada para a.pgn");
    assert_eq!(text(&pt, &many), "20 partidas exportadas para a.pgn");
    let appended = |count| GamesExportNotice::StoppedAppended { count };
    assert!(text(&en, &appended(1)).contains("one game"));
    assert!(text(&pt, &appended(5)).contains("5 partidas"));
    assert!(text(&en, &GamesExportNotice::NothingToExport).contains("nothing was written"));
}

#[test]
fn d2017_esc_while_the_file_dialog_is_open_closes_the_window() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "base", &games(2));
    app_update(&mut rig, GamesExportMessage::Open);
    app_update(&mut rig, GamesExportMessage::Choose);
    assert!(box_shows(&rig, "Waiting for the file name..."));
    rig.key(named(NamedKey::Escape));
    assert!(rig.app().games_export_view().is_none());
}

#[test]
fn d2005_the_job_has_a_name_in_the_toolbar_of_the_list() {
    let en = I18n::new(Language::EnUs);
    assert_eq!(en.tr("games-job-export"), "Exporting games");
}
