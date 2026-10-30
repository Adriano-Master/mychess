//! M7-4b contract: the `[export]` table of the preferences (spec
//! `docs/specs/m7-4-exportacao.md`, "Preferências", D3715, D3729,
//! criterion 15): round trip, wrong values back to the default of their
//! own field, templates written only when edited, and the neighbours of the
//! table untouched.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4b_common;

use m7_4b_common::*;
use mychess_app::{CsvSeparator, TemplateKind};

fn custom() -> ExportPrefs {
    let mut p = ExportPrefs {
        format: ExportFormat::Viewer,
        ..ExportPrefs::default()
    };
    p.html.comments = false;
    p.html.symbols = true;
    p.html.diagram_px = 320;
    p.viewer.per_file = 77;
    p.viewer.include_pgn = false;
    p.viewer.open_after = true;
    p.latex.flip = true;
    p.latex.convert_null_moves = false;
    p.epd.dedupe = false;
    p.epd.final_position = true;
    p.epd.hmvc_fmvn = true;
    p.epd.id = true;
    p.epd.game_number = true;
    p.list_format = String::from("g4 w10 r1");
    p.csv.header = false;
    p.csv.separator = CsvSeparator::Semicolon;
    p.csv.bom = true;
    p.image.format = ImageFormat::Svg;
    p.image.size_px = 1024;
    p.image.coordinates = false;
    p.image.last_move = false;
    p.image.marks = false;
    p.image.transparent = true;
    p.image.margin_px = 24;
    p.html_start = String::from("<html><!-- my head -->\n<body>\n");
    p.latex_end = String::from("\\end{document} % mine\n");
    p
}

#[test]
fn the_default_goes_there_and_back() {
    let prefs = Preferences::default();
    let text = render_preferences(&prefs);
    assert!(text.contains("[export]"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.export_docs, ExportPrefs::default());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn every_option_goes_there_and_back() {
    let mut prefs = Preferences::default();
    *prefs.export_docs = custom();
    let text = render_preferences(&prefs);
    for table in [
        "[export.html]",
        "[export.viewer]",
        "[export.latex]",
        "[export.epd]",
        "[export.csv]",
        "[export.image]",
    ] {
        assert!(text.contains(table), "{table} in\n{text}");
    }
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.export_docs, custom());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn the_templates_are_written_only_when_they_differ_from_the_default() {
    let plain = render_preferences(&Preferences::default());
    for key in ["html_start", "html_end", "latex_start", "latex_end"] {
        assert!(!plain.contains(key), "{key} written for the default");
    }
    let mut prefs = Preferences::default();
    *prefs.export_docs = custom();
    let text = render_preferences(&prefs);
    assert!(text.contains("html_start") && text.contains("latex_end"));
    assert!(!text.contains("html_end") && !text.contains("latex_start"));
    // Restoring the default removes it from the file again.
    let mut restored = (*prefs.export_docs).clone();
    restored.html_start = ExportPrefs::default().html_start;
    *prefs.export_docs = restored;
    assert!(!render_preferences(&prefs).contains("html_start"));
}

#[test]
fn wrong_values_go_back_to_the_default_of_their_own_field() {
    let loaded = load_preferences(
        "[export]\nformat = \"docx\"\nlist_format = 7\nhtml_start = false\n\
         [export.html]\ndiagram_px = 99999\ncomments = \"yes\"\nsymbols = true\n\
         [export.viewer]\nper_file = 0\ninclude_pgn = false\n\
         [export.image]\nsize_px = 10\nmargin_px = -3\nformat = \"gif\"\ntransparent = true\n\
         [export.csv]\nseparator = \"tab\"\nbom = true\n\
         [export.epd]\ndedupe = 1\nid = true\n",
    );
    let p = &loaded.prefs.export_docs;
    let d = ExportPrefs::default();
    assert_eq!(p.format, d.format);
    assert_eq!(p.list_format, d.list_format);
    assert_eq!(p.html_start, d.html_start);
    assert_eq!(p.html.diagram_px, d.html.diagram_px);
    assert_eq!(p.html.comments, d.html.comments);
    assert_eq!(p.viewer.per_file, d.viewer.per_file);
    assert_eq!(p.image.size_px, d.image.size_px);
    assert_eq!(p.image.margin_px, d.image.margin_px);
    assert_eq!(p.image.format, d.image.format);
    assert_eq!(p.csv.separator, d.csv.separator);
    assert_eq!(p.epd.dedupe, d.epd.dedupe);
    // The good values next to the bad ones are kept.
    assert!(p.html.symbols);
    assert!(!p.viewer.include_pgn);
    assert!(p.image.transparent);
    assert!(p.csv.bom);
    assert!(p.epd.id);
    assert!(loaded.issues.len() >= 10, "{:?}", loaded.issues);
}

#[test]
fn a_table_of_the_wrong_type_is_ignored_without_losing_the_rest() {
    let loaded = load_preferences("[export]\nhtml = 3\nviewer = \"x\"\nformat = \"latex\"\n");
    assert_eq!(loaded.prefs.export_docs.format, ExportFormat::Latex);
    assert_eq!(loaded.prefs.export_docs.html, ExportPrefs::default().html);
    assert_ne!(loaded.issues.len(), 0);
    let loaded = load_preferences("export = 5\n");
    assert_eq!(*loaded.prefs.export_docs, ExportPrefs::default());
    assert_ne!(loaded.issues.len(), 0);
}

#[test]
fn an_oversized_template_or_list_format_goes_back_to_the_default() {
    let huge = "x".repeat(70_000);
    let loaded = load_preferences(&format!(
        "[export]\nlatex_start = \"{huge}\"\nlist_format = \"{}\"\n",
        "w".repeat(300)
    ));
    let d = ExportPrefs::default();
    assert_eq!(loaded.prefs.export_docs.latex_start, d.latex_start);
    assert_eq!(loaded.prefs.export_docs.list_format, d.list_format);
}

#[test]
fn a_change_is_sanitized_and_does_not_touch_the_pgn_export_table() {
    let mut rig = Rig::new();
    rig.state.prefs.pgn_export.width = 111;
    let mut bad = custom();
    bad.image.size_px = 5;
    bad.viewer.per_file = 9_999_999;
    rig.raw(PrefsMessage::Set(PrefChange::ExportDocs(Box::new(bad))));
    let prefs = &rig.state.prefs;
    assert_eq!(
        prefs.export_docs.image.size_px,
        ExportPrefs::default().image.size_px
    );
    assert_eq!(
        prefs.export_docs.viewer.per_file,
        ExportPrefs::default().viewer.per_file
    );
    assert_eq!(prefs.export_docs.format, ExportFormat::Viewer);
    // Both tables live side by side in the file.
    let loaded = load_preferences(&render_preferences(prefs));
    assert_eq!(loaded.prefs.pgn_export.width, 111);
    assert_eq!(loaded.prefs.export_docs.format, ExportFormat::Viewer);
}

#[test]
fn templates_can_be_edited_and_restored_through_the_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    rig.edit(ExportEdit::Template(
        TemplateKind::HtmlStart,
        "<p>mine".to_owned(),
    ));
    rig.edit(ExportEdit::Template(
        TemplateKind::LatexEnd,
        "\\bye".to_owned(),
    ));
    assert_eq!(rig.state.prefs.export_docs.html_start, "<p>mine");
    rig.edit(ExportEdit::ResetTemplate(TemplateKind::HtmlStart));
    assert_eq!(
        rig.state.prefs.export_docs.html_start,
        ExportPrefs::default().html_start
    );
    assert_eq!(rig.state.prefs.export_docs.latex_end, "\\bye");
    rig.edit(ExportEdit::ListFormat("w10".to_owned()));
    rig.edit(ExportEdit::ResetListFormat);
    assert_eq!(
        rig.state.prefs.export_docs.list_format,
        mychess_app::DEFAULT_LIST_FORMAT
    );
}

#[test]
fn the_edited_html_template_reaches_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    rig.edit(ExportEdit::Template(
        TemplateKind::HtmlStart,
        "<!DOCTYPE html><html><body class=\"mine\">\n".to_owned(),
    ));
    rig.edit(ExportEdit::Template(
        TemplateKind::HtmlEnd,
        "<!-- the end -->\n</body></html>\n".to_owned(),
    ));
    let (effects, _) = rig.gx_choose(&dir.path().join("t.html"));
    rig.gx_run(effects);
    let text = text_of(&dir.path().join("t.html"));
    assert!(
        text.starts_with("<!DOCTYPE html><html><body class=\"mine\">"),
        "{text}"
    );
    assert!(text.contains("<!-- the end -->"));
}

#[test]
fn a_choice_made_while_the_file_is_loading_is_saved_only_after_the_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::write(
        &path,
        "[export]\nformat = \"csv\"\n[export.csv]\nbom = true\n",
    )
    .unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let mut rig = Rig::new();
    rig.state = mychess_app::AppState::with_preferences_path(Some(path.clone()));
    rig.state.open_pgn_in_slots = true;
    let startup = rig.raw(Message::Startup);
    let load = startup
        .into_iter()
        .find(|e| matches!(e, Effect::LoadPreferences { .. }))
        .expect("the load");
    // The file is still being read: a base is opened and the format chosen.
    rig.open_si4(dir.path(), "b", &numbered(2));
    rig.gx(GamesExportMessage::Open);
    let effects = rig.raw(ExportDocsMessage::SetFormat(ExportFormat::Latex));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::SavePreferences { .. })),
        "nothing is written before the load: {effects:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    // The load ends: the choice wins over the file, the rest of the file
    // stays, and the change is marked to reach the disk.
    let loaded = rig.perform(load);
    rig.raw(loaded);
    assert_eq!(rig.state.prefs.export_docs.format, ExportFormat::Latex);
    assert!(
        rig.state.prefs.export_docs.csv.bom,
        "the file's value survived"
    );
    assert!(rig.state.prefs_dirty, "the change must still be saved");
    // Quitting writes it, with both the choice and the rest of the file.
    let effects = rig.raw(Message::Quit);
    let save = effects
        .into_iter()
        .find(|e| matches!(e, Effect::SavePreferences { .. }))
        .expect("a save when quitting");
    let answer = rig.perform(save);
    rig.raw(answer);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("format = \"latex\""), "{text}");
    assert!(text.contains("bom = true"), "{text}");
}
