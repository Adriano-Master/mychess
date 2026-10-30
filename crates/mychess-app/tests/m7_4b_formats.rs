//! M7-4b contract: each format of "Export games..." produces the right file
//! and the right notice (spec `docs/specs/m7-4-exportacao.md`, sections 2-7,
//! criteria 14 and 15, rules A.1-A.4, E, F).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_4b_common;

use m7_4b_common::*;
use mychess_app::CsvSeparator;
use tempfile::tempdir;

fn rig_with(dir: &std::path::Path, text: &str) -> Rig {
    let mut rig = Rig::new();
    rig.open_si4(dir, "mybase", text);
    rig.set_assets();
    rig
}

fn ids_of(text: &str) -> Vec<String> {
    text.match_indices(" id=\"")
        .map(|(i, _)| text[i + 5..].split('"').next().unwrap().to_owned())
        .collect()
}

#[test]
fn html_writes_a_document_with_escapes_and_one_diagram_per_game() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let dest = dir.path().join("out");
    rig.export_as(ExportFormat::Html, &dest);
    let text = text_of(&dir.path().join("out.html"));
    assert!(text.starts_with("<!DOCTYPE html>"), "{}", &text[..40]);
    assert_eq!(text.matches("<article").count(), 2);
    assert!(text.contains("W1 &amp; Co"), "names are escaped");
    assert!(text.contains("Ev &amp; &lt;1&gt;"));
    assert!(!text.contains("<i>"), "comment text is escaped");
    assert!(text.contains("note &lt;i&gt; &amp; more"));
    assert!(!text.contains("[%clk"), "marks are always removed");
    assert_eq!(text.matches("<svg ").count(), 2, "one diagram per game");
    assert!(text.matches("<title>").count() >= 2);
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Exported {
            count: 2,
            name: "out.html".to_owned()
        })
    );
    assert!(rig.state.games_export_view().is_none(), "the window closed");
    assert!(!has_temp(dir.path()));
}

#[test]
fn html_diagrams_of_one_page_never_share_an_id() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.export_as(ExportFormat::Html, &dir.path().join("two.html"));
    let text = text_of(&dir.path().join("two.html"));
    let ids = ids_of(&text);
    assert!(ids.len() > 12, "symbols of both diagrams: {}", ids.len());
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "duplicate ids in one document");
    assert!(ids.iter().any(|id| id.starts_with("dg1-")));
    assert!(ids.iter().any(|id| id.starts_with("dg2-")));
}

#[test]
fn html_options_change_the_output() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    rig.edit(ExportEdit::HtmlSymbols(true));
    rig.edit(ExportEdit::HtmlComments(false));
    rig.edit(ExportEdit::HtmlVariations(false));
    rig.edit(ExportEdit::HtmlDiagramPx(0));
    assert_eq!(rig.state.prefs.export_docs.html.diagram_px, 0);
    let (effects, _) = rig.gx_choose(&dir.path().join("o.html"));
    rig.gx_run(effects);
    let text = text_of(&dir.path().join("o.html"));
    assert!(text.contains('!'), "symbolic NAG");
    assert!(!text.contains("$1"));
    assert!(!text.contains("class=\"c\""), "no comments");
    assert!(!text.contains("class=\"var"), "no variations");
    assert!(!text.contains("<svg "), "no diagrams");
}

#[test]
fn latex_writes_the_document_for_xskak() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &format!("{}{}", rich_pgn(), fen_game()));
    rig.export_as(ExportFormat::Latex, &dir.path().join("games"));
    let text = text_of(&dir.path().join("games.tex"));
    assert!(text.contains("\\begin{document}") && text.trim_end().ends_with("\\end{document}"));
    assert!(text.contains("\\mainline{"));
    assert!(text.contains("W1 \\& Co"), "ampersand escaped");
    assert!(text.contains("A\\_B \\$ \\% \\#"), "specials escaped");
    assert!(text.contains("setfen="), "the game from a position");
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Exported {
            count: 3,
            name: "games.tex".to_owned()
        })
    );
}

#[test]
fn epd_without_a_ply_and_with_standard_starts_exports_nothing_and_creates_no_file() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let dest = dir.path().join("pos.epd");
    rig.export_as(ExportFormat::Epd, &dest);
    assert!(!dest.exists(), "rule 21: no file");
    assert_eq!(docs_notice(&rig), Some(GamesExportNotice::NothingToExport));
    assert!(!has_temp(dir.path()));
}

#[test]
fn epd_final_positions_are_written_once_and_repeats_are_counted() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Epd);
    rig.edit(ExportEdit::EpdFinalPosition(true));
    rig.edit(ExportEdit::EpdGameNumber(true));
    let (effects, kind) = rig.gx_choose(&dir.path().join("final"));
    assert_eq!(
        kind,
        PickKind::ExportDoc {
            format: ExportFormat::Epd,
            append: false
        }
    );
    rig.gx_run(effects);
    let text = text_of(&dir.path().join("final.epd"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "both games end in the same position: {text}"
    );
    // Four FEN fields, then the operation of the first game.
    assert!(
        lines[0].starts_with("r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq -"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("c0 \"1\""), "{}", lines[0]);
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::EpdExported {
            count: 1,
            repeated: 1,
            name: "final.epd".to_owned()
        })
    );
}

#[test]
fn epd_uses_the_ply_of_the_filter() {
    use mychess_app::{AdvancedKind, AdvancedSearchMessage};
    let dir = tempdir().unwrap();
    let third = "[Event \"E\"]\n[Site \"S\"]\n[Date \"2020.01.05\"]\n[Round \"1\"]\n\
                 [White \"Wd\"]\n[Black \"Bd\"]\n[Result \"*\"]\n\n1. d4 d5 *\n\n";
    let mut rig = rig_with(dir.path(), &format!("{}{third}", rich_pgn()));
    // The position search leaves games 1 and 2 in the filter with ply 2
    // (after 1.e4 e5) and drops game 3.
    rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::SelectTab(
        AdvancedKind::Position,
    )));
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::SetFen(
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2".into(),
    )));
    rig.send(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert_eq!(rig.state.bases.current_slot().unwrap().filtered(), 2);
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Epd);
    rig.gx(GamesExportMessage::SetScope(ExportScope::Filter));
    let (effects, _) = rig.gx_choose(&dir.path().join("ply.epd"));
    rig.gx_run(effects);
    let text = text_of(&dir.path().join("ply.epd"));
    assert_eq!(
        text.lines().count(),
        1,
        "the two games share the position: {text}"
    );
    assert!(
        text.starts_with("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -"),
        "{text}"
    );
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::EpdExported {
            count: 1,
            repeated: 1,
            name: "ply.epd".to_owned()
        })
    );
}

#[test]
fn the_game_list_has_one_line_per_game_and_utf8_text() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.export_as(ExportFormat::List, &dir.path().join("list"));
    let text = text_of(&dir.path().join("list.txt"));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("     1: W1 & Co"), "{:?}", lines[0]);
    assert!(lines[1].starts_with("     2: W2 & Co"), "{:?}", lines[1]);
    assert!(!text.contains('\r'));
}

#[test]
fn a_game_list_format_without_a_known_field_is_reported_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::List);
    rig.edit(ExportEdit::ListFormat("!!! 123".to_owned()));
    let (effects, _) = rig.gx_choose(&dir.path().join("none.txt"));
    rig.gx_run(effects);
    assert!(!dir.path().join("none.txt").exists());
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Problem(ExportProblem::EmptyFormat))
    );
    // The window stays so that the format can be fixed.
    assert!(rig.state.games_export_view().is_some());
}

#[test]
fn csv_has_the_visible_columns_a_translated_header_and_quoted_fields() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.state.set_export_csv_headers(vec![
        (mychess_db::CsvColumn::Number, "Número".to_owned()),
        (mychess_db::CsvColumn::White, "Brancas".to_owned()),
    ]);
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Csv);
    rig.edit(ExportEdit::CsvSeparator(CsvSeparator::Semicolon));
    rig.edit(ExportEdit::CsvBom(true));
    let (effects, _) = rig.gx_choose(&dir.path().join("sheet"));
    rig.gx_run(effects);
    let text = text_of(&dir.path().join("sheet.csv"));
    assert!(text.starts_with('\u{feff}'), "BOM");
    let lines: Vec<&str> = text.trim_start_matches('\u{feff}').lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    // The default columns of the list, in order: Number White Black Result
    // Length Date Event WhiteElo BlackElo Eco.
    assert!(
        lines[0].starts_with("Número;Brancas;export-csv-col-black;"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("1;W1 & Co;B1 <b>;1-0;"),
        "{}",
        lines[1]
    );
    assert!(
        lines[1].contains("\"Ev & <1> \"\"q\"\"\""),
        "RFC 4180 quoting: {}",
        lines[1]
    );
}

#[test]
fn lists_need_a_database_game_so_the_current_game_scope_is_refused() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Csv);
    rig.gx(GamesExportMessage::SetScope(ExportScope::Current));
    let view = rig.state.games_export_view().unwrap();
    assert_ne!(view.draft.scope, ExportScope::Current);
    // Even if the scope was already the current game when the format changed.
    rig.format(ExportFormat::Pgn);
    rig.gx(GamesExportMessage::SetScope(ExportScope::Current));
    rig.format(ExportFormat::List);
    assert_ne!(
        rig.state.games_export_view().unwrap().draft.scope,
        ExportScope::Current
    );
}

#[test]
fn the_viewer_writes_volumes_the_pgn_and_a_comment_with_the_piece_credit() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    rig.edit(ExportEdit::ViewerPerFile(1));
    let (effects, _) = rig.gx_choose(&dir.path().join("view"));
    rig.gx_run(effects);
    let one = text_of(&dir.path().join("view.html"));
    let two = text_of(&dir.path().join("view_2.html"));
    assert!(one.contains("id=\"mychess-data\"") && two.contains("id=\"mychess-data\""));
    assert!(one.contains("<!-- Pieces: cburnett by Colin M.L. Burnett"));
    assert!(
        one.contains("<symbol id=\"wK\""),
        "the pieces are in the page"
    );
    assert!(one.contains("view_2.html"), "link to the next volume");
    assert_eq!(whites_in(&text_of(&dir.path().join("view.pgn"))).len(), 2);
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::ViewerExported {
            volumes: 2,
            count: 2,
            name: "view.html".to_owned()
        })
    );
    assert!(!dir.path().join("view_3.html").exists());
}

#[test]
fn the_viewer_without_the_pgn_and_without_assets() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "mybase", &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    let (effects, _) = rig.gx_choose(&dir.path().join("v.html"));
    rig.gx_run(effects);
    assert!(!dir.path().join("v.html").exists());
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Problem(ExportProblem::AssetsMissing))
    );
    // With assets and without the PGN.
    rig.set_assets();
    rig.edit(ExportEdit::ViewerIncludePgn(false));
    let (effects, _) = rig.gx_choose(&dir.path().join("v.html"));
    rig.gx_run(effects);
    assert!(dir.path().join("v.html").exists());
    assert!(!dir.path().join("v.pgn").exists());
}

#[test]
fn names_are_sanitized_and_the_extension_is_added_in_any_case() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.export_as(ExportFormat::Csv, &dir.path().join("we:ird?name"));
    assert!(dir.path().join("we_ird_name.csv").exists());
    rig.export_as(ExportFormat::Csv, &dir.path().join("Upper.CSV"));
    assert!(dir.path().join("Upper.csv").exists());
    assert!(!dir.path().join("Upper.CSV.csv").exists());
    rig.export_as(ExportFormat::Latex, &dir.path().join("NUL"));
    assert!(dir.path().join("_NUL.tex").exists());
    rig.export_as(ExportFormat::Html, &dir.path().join("a.HTM"));
    assert!(dir.path().join("a.html").exists());
}

#[test]
fn the_dialog_proposes_a_name_with_the_extension_of_the_format() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    for (format, name) in [
        (ExportFormat::Html, "mybase.html"),
        (ExportFormat::Viewer, "mybase.html"),
        (ExportFormat::Latex, "mybase.tex"),
        (ExportFormat::Epd, "mybase.epd"),
        (ExportFormat::List, "mybase.txt"),
        (ExportFormat::Csv, "mybase.csv"),
        (ExportFormat::Pgn, "mybase.pgn"),
    ] {
        rig.gx(GamesExportMessage::Open);
        rig.format(format);
        let effects = rig.raw(GamesExportMessage::Choose);
        let spec = effects
            .iter()
            .find_map(|e| match e {
                Effect::PickFile(spec) => Some(spec.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(spec.file_name.as_deref(), Some(name), "{format:?}");
        rig.raw(Message::FilePicked(spec.request, None));
        rig.gx(GamesExportMessage::Close);
    }
}

#[test]
fn only_the_viewer_cannot_append() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetAppend(true));
    rig.format(ExportFormat::Latex);
    let (_, kind) = rig.gx_choose_cancel();
    assert_eq!(
        kind,
        PickKind::ExportDoc {
            format: ExportFormat::Latex,
            append: true
        }
    );
    rig.format(ExportFormat::Viewer);
    assert!(!rig.state.games_export_view().unwrap().draft.append);
    let (_, kind) = rig.gx_choose_cancel();
    assert_eq!(
        kind,
        PickKind::ExportDoc {
            format: ExportFormat::Viewer,
            append: false
        }
    );
}

#[test]
fn a_file_with_unreadable_games_ends_in_the_all_skipped_notice() {
    let dir = tempdir().unwrap();
    let base = make_si4(dir.path(), "broken", &numbered(3));
    // Wipe the game data: the index stays, every game fails to decode.
    let data = base.with_extension("sg4");
    let len = std::fs::metadata(&data).unwrap().len();
    std::fs::write(&data, vec![0u8; usize::try_from(len).unwrap()]).unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    rig.send(BasesMessage::Open(base));
    assert_eq!(
        rig.state
            .bases
            .current_slot()
            .map(mychess_app::BaseSlot::total),
        Some(3),
        "{:?}",
        rig.state.notice
    );
    for (format, name) in [
        (ExportFormat::Html, "h.html"),
        (ExportFormat::Latex, "l.tex"),
        (ExportFormat::Viewer, "v.html"),
    ] {
        // A file already there stays as it is (the notice promises it).
        if format != ExportFormat::Viewer {
            std::fs::write(dir.path().join(name), "precious").unwrap();
        }
        rig.export_as(format, &dir.path().join(name));
        if format != ExportFormat::Viewer {
            assert_eq!(text_of(&dir.path().join(name)), "precious", "{format:?}");
        }
        assert!(
            std::fs::read_dir(dir.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mychess-")),
            "no part file left"
        );
        assert_eq!(
            docs_notice(&rig),
            Some(GamesExportNotice::AllSkipped { skipped: 3 }),
            "{format:?}"
        );
        // The report stays on screen with the count of skipped games.
        let view = rig.state.games_export_view().expect("the report window");
        let GamesExportStepView::Done(result) = view.step else {
            panic!("not the report: {:?}", view.step);
        };
        assert_eq!(
            (result.report.written, result.report.skipped),
            (0, 3),
            "{format:?}"
        );
        assert_eq!(
            result.report.lines.len(),
            if format == ExportFormat::Viewer { 1 } else { 3 }
        );
        // Nothing was created for a job that exported no game.
        if format == ExportFormat::Viewer {
            assert!(!dir.path().join("v.html").exists());
        }
        rig.gx(GamesExportMessage::Close);
    }
}

#[test]
fn an_empty_selection_shows_nothing_to_export_for_the_viewer() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    // The "only tactics" option leaves every game out.
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    rig.gx(GamesExportMessage::SetOnlyTactics(true));
    let (effects, _) = rig.gx_choose(&dir.path().join("none.html"));
    rig.gx_run(effects);
    assert_eq!(docs_notice(&rig), Some(GamesExportNotice::NothingToExport));
    assert!(!dir.path().join("none.html").exists());
    assert!(!dir.path().join("none.pgn").exists());
}

impl Rig {
    /// "Choose destination..." and a cancelled dialog; returns the kind.
    fn gx_choose_cancel(&mut self) -> (Vec<Effect>, PickKind) {
        let effects = self.raw(GamesExportMessage::Choose);
        let spec = effects
            .iter()
            .find_map(|e| match e {
                Effect::PickFile(spec) => Some(spec.clone()),
                _ => None,
            })
            .expect("a file dialog");
        let kind = spec.kind;
        let out = self.raw(Message::FilePicked(spec.request, None));
        (out, kind)
    }
}
