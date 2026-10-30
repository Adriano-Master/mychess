//! M7-4c (GUI, spec M7-4): the dialog of "Export games" gains the format
//! selector and the options of each format, the question about an existing
//! file and the extra lines of the report; "Export position as image..." has
//! its window with preview, options, file dialog and question; the
//! assets reach the application; every layer is modal.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::assert_is_empty
)]

mod m7_2c_common;

use std::path::Path;

use m7_2c_common::*;
use mychess_app::export_docs::{
    ExportDocsMessage, ExportEdit, ExportFormat, ImageFormat, ImageStepView,
};
use mychess_app::{
    Effect, ExportScope, GamesExportMessage, GamesExportReport, GamesExportResult, PickKind, update,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::export_games_dialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};

fn docs(m: ExportDocsMessage) -> AppMessage {
    AppMessage::ExportDocs(m)
}

fn new_rig() -> Rig {
    let mut rig = Rig::sample();
    // Any update lets the shell give the application its assets.
    rig.msg(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    rig
}

fn open_dialog(rig: &mut Rig) {
    rig.app(GamesExportMessage::Open);
    assert!(rig.shell.app.games_export_view().is_some());
}

/// Runs the effects the way the host does, handing the answers back.
fn run_effects(rig: &mut Rig, effects: Vec<Effect>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut queue = effects;
    let mut guard = 0;
    while let Some(effect) = queue.pop() {
        guard += 1;
        assert!(guard < 200, "the effects never end");
        let answer = rt.block_on(mychess_app::perform(effect));
        queue.extend(update(&mut rig.shell.app, answer));
    }
}

/// "Choose destination" then the answer of the file dialog with `dest`.
fn choose(rig: &mut Rig, dest: &Path) -> PickKind {
    let effects = update(
        &mut rig.shell.app,
        AppMessage::GamesExport(GamesExportMessage::Choose),
    );
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .expect("a file dialog");
    let kind = spec.kind;
    let effects = update(
        &mut rig.shell.app,
        AppMessage::FilePicked(spec.request, Some(dest.to_path_buf())),
    );
    run_effects(rig, effects);
    // Let the shell catch up with the state.
    rig.msg(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    kind
}

fn format_of(rig: &Rig) -> ExportFormat {
    rig.shell.app.export_docs_view().prefs.format
}

// ---- the dialog ----

#[test]
fn the_file_menu_has_the_image_item_and_it_opens_the_image_window() {
    let rig = new_rig();
    let shell = &rig.shell;
    let file = menu_specs(&shell.app, &shell.ui, shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-file")
        .expect("File menu");
    let sent = file
        .entries
        .iter()
        .find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key("menu-file-export-image"),
                action,
                ..
            } => Some(format!("{action:?}")),
            _ => None,
        })
        .expect("the item");
    assert!(sent.contains("OpenImage"), "{sent}");
}

#[test]
fn the_dialog_shows_the_seven_formats_and_the_options_of_the_chosen_one() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    let table: [(ExportFormat, &[&str]); 7] = [
        (ExportFormat::Pgn, &["export-games-box"]),
        (
            ExportFormat::Html,
            &[
                "export-html-comments",
                "export-html-variations",
                "export-html-symbols",
                "export-html-diagram-0",
                "export-html-diagram-200",
            ],
        ),
        (
            ExportFormat::Viewer,
            &[
                "export-viewer-per-file-500",
                "export-viewer-include-pgn",
                "export-viewer-open-after",
            ],
        ),
        (
            ExportFormat::Latex,
            &[
                "export-latex-comments",
                "export-latex-variations",
                "export-latex-flip",
                "export-latex-null-moves",
            ],
        ),
        (
            ExportFormat::Epd,
            &[
                "export-epd-dedupe",
                "export-epd-final-position",
                "export-epd-counters",
                "export-epd-id",
                "export-epd-game-number",
            ],
        ),
        (
            ExportFormat::List,
            &["export-list-format-field", "export-list-default"],
        ),
        (
            ExportFormat::Csv,
            &[
                "export-csv-header",
                "export-csv-sep-comma",
                "export-csv-sep-semicolon",
                "export-csv-bom",
            ],
        ),
    ];
    for (format, ids) in table {
        rig.app(docs(ExportDocsMessage::SetFormat(format)));
        assert_eq!(format_of(&rig), format);
        let mut sim = rig.sim(1400.0, 1400.0);
        for other in [
            ExportFormat::Pgn,
            ExportFormat::Html,
            ExportFormat::Viewer,
            ExportFormat::Latex,
            ExportFormat::Epd,
            ExportFormat::List,
            ExportFormat::Csv,
        ] {
            let name = format!("export-format-{}", other.name());
            assert!(sim.find(id(&name)).is_ok(), "{name}");
        }
        for name in ids {
            if *name == "export-games-box" {
                continue;
            }
            assert!(sim.find(id(name)).is_ok(), "{format:?}: {name}");
        }
    }
}

#[test]
fn a_click_on_a_format_asks_the_application_for_it() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    let sent = debug_of(&click_id(rig.sim(1400.0, 1400.0), "export-format-latex"));
    assert!(
        sent.iter().any(|m| m.contains("SetFormat(Latex)")),
        "{sent:?}"
    );
}

#[test]
fn the_options_are_sent_to_the_application_and_kept() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Html)));
    let sent = debug_of(&click_id(rig.sim(1400.0, 1400.0), "export-html-comments"));
    assert!(
        sent.iter().any(|m| m.contains("HtmlComments(false)")),
        "{sent:?}"
    );
    rig.app(docs(ExportDocsMessage::Edit(ExportEdit::HtmlComments(
        false,
    ))));
    assert!(!rig.shell.app.export_docs_view().prefs.html.comments);
    let sent = debug_of(&click_id(
        rig.sim(1400.0, 1400.0),
        "export-html-diagram-300",
    ));
    assert!(
        sent.iter().any(|m| m.contains("HtmlDiagramPx(300)")),
        "{sent:?}"
    );
}

#[test]
fn the_viewer_cannot_append_and_the_lists_need_a_database_scope() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    rig.app(GamesExportMessage::SetAppend(true));
    assert!(rig.shell.app.games_export_view().unwrap().draft.append);
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Viewer)));
    // The application turns "append" off for the viewer.
    let view = rig.shell.app.games_export_view().unwrap();
    assert!(!view.draft.append);
    rig.app(GamesExportMessage::SetScope(ExportScope::Current));
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Csv)));
    let view = rig.shell.app.games_export_view().unwrap();
    assert_ne!(view.draft.scope, ExportScope::Current);
}

#[test]
fn the_host_gives_the_application_the_piece_drawings() {
    let rig = new_rig();
    assert!(rig.shell.app.export_docs_view().has_assets);
}

// ---- the jobs ----

#[test]
fn the_viewer_is_written_and_the_report_shows_the_files() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    rig.app(GamesExportMessage::SetScope(ExportScope::All));
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Viewer)));
    let dest = rig.dir.path().join("games.html");
    let kind = choose(&mut rig, &dest);
    assert!(
        matches!(
            kind,
            PickKind::ExportDoc {
                format: ExportFormat::Viewer,
                append: false
            }
        ),
        "{kind:?}"
    );
    let text = std::fs::read_to_string(&dest).expect("the volume");
    assert!(text.contains("id=\"mychess-data\""), "{text:.200}");
    assert!(text.contains("<symbol id=\"wK\""));
    assert!(text.contains("id=\"viewer\""));
    // A job that went well closes the window and says so in the status bar.
    let notice = match rig.shell.app.notice.as_ref() {
        Some(AppNotice::GamesExport(n)) => n.clone(),
        other => panic!("{other:?}"),
    };
    match notice {
        mychess_app::GamesExportNotice::ViewerExported { volumes, count, .. } => {
            assert_eq!(volumes, 1);
            assert_eq!(count, 7);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_existing_file_raises_the_question_and_escape_answers_no() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    rig.app(GamesExportMessage::SetScope(ExportScope::All));
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Latex)));
    // A name without extension: the file dialog did not confirm replacing
    // the `.tex` the application adds.
    let dest = rig.dir.path().join("doc");
    choose(&mut rig, &dest);
    let tex = rig.dir.path().join("doc.tex");
    assert!(tex.exists());
    rig.app(GamesExportMessage::Close);
    open_dialog(&mut rig);
    rig.app(GamesExportMessage::SetScope(ExportScope::All));
    std::fs::write(&tex, "old").unwrap();
    choose(&mut rig, &dest);
    assert!(rig.shell.app.export_docs_view().confirm.is_some());
    assert_eq!(std::fs::read_to_string(&tex).unwrap(), "old");
    let mut sim = rig.sim(1400.0, 900.0);
    for name in [
        "export-confirm-box",
        "export-confirm-yes",
        "export-confirm-no",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(
        sim.find("The file doc.tex already exists. Replace it?")
            .is_ok()
    );
    drop(sim);
    // Enter never replaces: it answers no (D4301).
    assert_eq!(
        export_games_dialog::enter_confirm_message(&rig.shell.app),
        Some(AppMessage::ExportDocs(ExportDocsMessage::Confirm(false)))
    );
    // Esc answers no.
    assert!(export_games_dialog::escape_confirm_message(&rig.shell.app).is_some());
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    rig.drain(task);
    assert!(rig.shell.app.export_docs_view().confirm.is_none());
    assert_eq!(std::fs::read_to_string(&tex).unwrap(), "old");
    // The dialog is still there, back at the options.
    assert!(rig.shell.app.games_export_view().is_some());
}

#[test]
fn hundreds_of_existing_volumes_are_summed_up_and_the_buttons_stay() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    rig.app(GamesExportMessage::SetScope(ExportScope::All));
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Viewer)));
    std::fs::write(rig.dir.path().join("doc.html"), "old").unwrap();
    for n in 2..=400 {
        std::fs::write(rig.dir.path().join(format!("doc_{n}.html")), "old").unwrap();
    }
    let dest = rig.dir.path().join("doc");
    choose(&mut rig, &dest);
    let files = rig
        .shell
        .app
        .export_docs_view()
        .confirm
        .map_or(0, |c| c.files.len());
    assert_eq!(files, 400);
    let mut sim = rig.sim(1400.0, 900.0);
    for name in ["export-confirm-yes", "export-confirm-no"] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(
        sim.find(
            "These files already exist: doc.html, doc_2.html, doc_3.html, doc_4.html, \
             doc_5.html and 395 more. Replace them?"
        )
        .is_ok()
    );
}

#[test]
fn the_answer_yes_replaces_the_file() {
    let mut rig = new_rig();
    open_dialog(&mut rig);
    rig.app(GamesExportMessage::SetScope(ExportScope::All));
    rig.app(docs(ExportDocsMessage::SetFormat(ExportFormat::Csv)));
    let dest = rig.dir.path().join("list");
    std::fs::write(rig.dir.path().join("list.csv"), "old").unwrap();
    choose(&mut rig, &dest);
    assert!(rig.shell.app.export_docs_view().confirm.is_some());
    let sent = debug_of(&click_id(rig.sim(1400.0, 900.0), "export-confirm-yes"));
    assert!(sent.iter().any(|m| m.contains("Confirm(true)")), "{sent:?}");
    let effects = update(&mut rig.shell.app, docs(ExportDocsMessage::Confirm(true)));
    run_effects(&mut rig, effects);
    let text = std::fs::read_to_string(rig.dir.path().join("list.csv")).unwrap();
    assert_ne!(text, "old");
    assert!(text.contains("Alfa"), "{text}");
}

#[test]
fn the_report_shows_repeated_not_selected_volumes_and_the_problem() {
    let i18n = I18n::new(Language::EnUs);
    let result = GamesExportResult {
        path: "x.epd".into(),
        report: GamesExportReport {
            written: 3,
            repeated: 2,
            not_selected: 4,
            volumes: 5,
            problem: Some(mychess_app::export_docs::ExportProblem::NoAppendMarker),
            ..GamesExportReport::default()
        },
        error: None,
    };
    let lines = export_games_dialog::summary_lines(&result, &i18n);
    let all = lines.join("\n");
    for expected in [
        "Repeated positions left out: 2",
        "Games with no position to give: 4",
        "Files written: 5",
        "closing line",
    ] {
        assert!(all.contains(expected), "{expected} in {all}");
    }
    let pt = I18n::new(Language::PtBr);
    let all = export_games_dialog::summary_lines(&result, &pt).join("\n");
    assert!(
        all.contains("Posições repetidas deixadas de fora: 2"),
        "{all}"
    );
}

#[test]
fn the_notices_of_the_viewer_the_epd_and_the_image_carry_their_numbers() {
    use mychess_app::GamesExportNotice as N;
    let i18n = I18n::new(Language::EnUs);
    let text = export_games_dialog::notice_text(
        &i18n,
        &N::ViewerExported {
            volumes: 3,
            count: 1200,
            name: "a.html".into(),
        },
    );
    assert!(
        text.contains("3 files") && text.contains("1200") && text.contains("a.html"),
        "{text}"
    );
    let text = export_games_dialog::notice_text(
        &i18n,
        &N::EpdExported {
            count: 9,
            repeated: 2,
            name: "a.epd".into(),
        },
    );
    assert!(text.contains('9') && text.contains("2 repeated"), "{text}");
    let text = export_games_dialog::notice_text(
        &i18n,
        &N::ImageSaved {
            name: "b.png".into(),
        },
    );
    assert!(text.contains("b.png"), "{text}");
}

// ---- the image ----

fn open_image(rig: &mut Rig) {
    rig.app(docs(ExportDocsMessage::OpenImage));
    assert!(rig.shell.app.export_image_view().is_some());
}

#[test]
fn the_image_window_is_a_modal_card_with_preview_and_options() {
    let mut rig = new_rig();
    assert!(!rig.shell.modal_layer_open());
    open_image(&mut rig);
    assert!(rig.shell.modal_layer_open());
    let mut sim = rig.sim(1400.0, 1000.0);
    for name in [
        "export-image-box",
        "export-image-preview",
        "export-image-png",
        "export-image-svg",
        "export-image-size-512",
        "export-image-size-4096",
        "export-image-coordinates",
        "export-image-flip",
        "export-image-last-move",
        "export-image-marks",
        "export-image-transparent",
        "export-image-margin-0",
        "export-image-save",
        "export-image-close",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(sim.find("Export position as image").is_ok());
}

#[test]
fn the_preview_is_drawn_once_per_change_and_follows_the_options() {
    let mut rig = new_rig();
    open_image(&mut rig);
    assert!(rig.shell.ui.dialogs.export_image.preview_ready());
    rig.app(docs(ExportDocsMessage::Edit(ExportEdit::ImageFlip(true))));
    assert!(rig.shell.app.export_image_view().unwrap().flip);
    assert!(rig.shell.ui.dialogs.export_image.preview_ready());
    // The window gone, the preview goes too.
    rig.app(docs(ExportDocsMessage::ImageClose));
    assert!(rig.shell.app.export_image_view().is_none());
    assert!(!rig.shell.ui.dialogs.export_image.preview_ready());
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn clicks_on_the_options_reach_the_application() {
    let mut rig = new_rig();
    open_image(&mut rig);
    for (name, expected) in [
        ("export-image-size-1024", "ImageSize(1024)"),
        ("export-image-svg", "ImageFormat(Svg)"),
        ("export-image-margin-32", "ImageMargin(32)"),
        ("export-image-flip", "ImageFlip(true)"),
        ("export-image-coordinates", "ImageCoordinates(false)"),
    ] {
        let sent = debug_of(&click_id(rig.sim(1400.0, 1000.0), name));
        assert!(
            sent.iter().any(|m| m.contains(expected)),
            "{name}: {sent:?}"
        );
    }
}

#[test]
fn saving_writes_a_png_through_the_file_dialog() {
    let mut rig = new_rig();
    open_image(&mut rig);
    rig.app(docs(ExportDocsMessage::Edit(ExportEdit::ImageSize(256))));
    let effects = update(&mut rig.shell.app, docs(ExportDocsMessage::ImageSave));
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .expect("a file dialog");
    assert!(matches!(
        spec.kind,
        PickKind::ExportImage {
            format: ImageFormat::Png
        }
    ));
    assert!(matches!(
        rig.shell.app.export_image_view().unwrap().step,
        ImageStepView::Picking
    ));
    // While the dialog is open the window says so.
    assert!(
        rig.sim(1400.0, 1000.0)
            .find("Waiting for the file name...")
            .is_ok()
    );
    let dest = rig.dir.path().join("board.png");
    let effects = update(
        &mut rig.shell.app,
        AppMessage::FilePicked(spec.request, Some(dest.clone())),
    );
    run_effects(&mut rig, effects);
    let bytes = std::fs::read(&dest).expect("the image");
    assert_eq!(&bytes[..4], b"\x89PNG");
    // Saved: the window closes.
    assert!(rig.shell.app.export_image_view().is_none());
}

#[test]
fn an_existing_image_raises_the_question_and_the_keys_answer_it() {
    let mut rig = new_rig();
    open_image(&mut rig);
    rig.app(docs(ExportDocsMessage::Edit(ExportEdit::ImageFormat(
        ImageFormat::Svg,
    ))));
    // Without an extension the dialog did not confirm the `.svg` added.
    std::fs::write(rig.dir.path().join("pos.svg"), "old").unwrap();
    let effects = update(&mut rig.shell.app, docs(ExportDocsMessage::ImageSave));
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .expect("a file dialog");
    let effects = update(
        &mut rig.shell.app,
        AppMessage::FilePicked(spec.request, Some(rig.dir.path().join("pos"))),
    );
    run_effects(&mut rig, effects);
    assert!(matches!(
        rig.shell.app.export_image_view().unwrap().step,
        ImageStepView::Confirm(_)
    ));
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("pos.svg")).unwrap(),
        "old"
    );
    let mut sim = rig.sim(1400.0, 1000.0);
    for name in [
        "export-image-confirm-box",
        "export-image-confirm-yes",
        "export-image-confirm-no",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    drop(sim);
    // Enter never replaces: it answers no (D4301).
    assert_eq!(
        mychess_gui::windows::export_image::enter_message(&rig.shell),
        Some(AppMessage::ExportDocs(ExportDocsMessage::Confirm(false)))
    );
    // Esc answers no: the options come back, the file is untouched.
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    rig.drain(task);
    assert!(matches!(
        rig.shell.app.export_image_view().unwrap().step,
        ImageStepView::Options
    ));
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("pos.svg")).unwrap(),
        "old"
    );
    // Esc again closes the window.
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    rig.drain(task);
    assert!(rig.shell.app.export_image_view().is_none());
}

#[test]
fn the_keys_of_the_game_behind_are_blocked_while_the_image_window_is_open() {
    let mut rig = new_rig();
    open_image(&mut rig);
    let before = rig.shell.app.session.cursor();
    drop(rig.key(Key::Named(NamedKey::Left), Modifiers::NONE));
    assert_eq!(rig.shell.app.session.cursor(), before);
}

#[test]
fn the_texts_exist_in_both_languages_and_portuguese_says_variante() {
    use std::collections::BTreeSet;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
    let keys = |lang: &str, file: &str| -> BTreeSet<String> {
        std::fs::read_to_string(dir.join(lang).join(file))
            .unwrap()
            .lines()
            .filter_map(|l| {
                let (k, _) = l.split_once(" = ")?;
                (!k.starts_with('#') && !k.contains(' ')).then(|| k.to_owned())
            })
            .collect()
    };
    for file in ["export.ftl", "dialogs.ftl"] {
        assert_eq!(keys("en-US", file), keys("pt-BR", file), "{file}");
    }
    let pt = std::fs::read_to_string(dir.join("pt-BR/export.ftl")).unwrap();
    assert!(!pt.to_lowercase().contains("variaç"));
    // Every `export-` key the code of the dialogs names exists.
    let en: BTreeSet<String> = keys("en-US", "export.ftl")
        .union(&keys("en-US", "dialogs.ftl"))
        .cloned()
        .collect();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows");
    let ids = [
        "export-games-box",
        "export-confirm-box",
        "export-confirm-no",
        "export-confirm-yes",
        "export-options-scroll",
        "export-list-format-field",
        "export-image-box",
        "export-image-confirm-box",
        "export-image-preview",
        "export-image-png",
        "export-image-svg",
        "export-image-confirm-no",
        "export-image-confirm-yes",
        "export-csv-sep-comma",
        "export-csv-sep-semicolon",
    ];
    for file in [
        "export_games_dialog.rs",
        "export_options.rs",
        "export_image.rs",
    ] {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        for chunk in text.split('"').skip(1).step_by(2) {
            let key_like = chunk.starts_with("export-")
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !chunk.ends_with('-');
            if key_like && !ids.contains(&chunk) {
                assert!(
                    en.contains(chunk),
                    "{file} names {chunk}, which has no text"
                );
            }
        }
    }
}
