//! M7-4a, criterion 4 and rules A.1-A.4 of `docs/specs/m7-4-exportacao.md`:
//! the HTML and LaTeX file exports (atomic write, append, cancel).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assigning_clones)]

#[path = "common/m7_4a_support.rs"]
mod support;

use std::fs;

use mychess_db::export::document::{
    HtmlExportOptions, LatexExportOptions, export_html, export_latex,
};
use mychess_db::si4::Si4Error;
use mychess_db::{CancelToken, ExportError, ExportItem, ExportSkipReason, NoProgress};
use mychess_pgn::export::{HtmlOptions, NoDiagrams};
use support::games_from_pgn;

fn game_items(pgn: &str) -> Vec<ExportItem> {
    games_from_pgn(pgn)
        .into_iter()
        .map(|g| ExportItem::Game(Box::new(g)))
        .collect()
}

const TWO: &str = "[Event \"One\"]\n[White \"W1\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n[Event \"Two\"]\n[White \"W2\"]\n[Result \"*\"]\n\n1. d4 *\n";
const ONE_MORE: &str = "[Event \"Three\"]\n[White \"W3\"]\n[Result \"0-1\"]\n\n1. c4 0-1\n";

fn html(
    items: Vec<ExportItem>,
    dest: &std::path::Path,
    opts: &HtmlExportOptions,
) -> Result<mychess_db::ExportReport, ExportError> {
    export_html(
        items,
        dest,
        opts,
        &mut NoDiagrams,
        &mut NoProgress,
        &CancelToken::new(),
    )
}

fn latex(
    items: Vec<ExportItem>,
    dest: &std::path::Path,
    opts: &LatexExportOptions,
) -> Result<mychess_db::ExportReport, ExportError> {
    export_latex(items, dest, opts, &mut NoProgress, &CancelToken::new())
}

fn tmp_count(dir: &tempfile::TempDir) -> usize {
    support::file_names(dir.path())
        .iter()
        .filter(|n| n.contains(".tmp"))
        .count()
}

#[test]
fn html_file_is_start_games_end_and_gets_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    let report = html(
        game_items(TWO),
        &dir.path().join("out"),
        &HtmlExportOptions::default(),
    )
    .unwrap();
    assert_eq!(report.path, dir.path().join("out.html"));
    assert_eq!(report.written, 2);
    let text = fs::read_to_string(&report.path).unwrap();
    assert!(text.starts_with("<!DOCTYPE html>"));
    assert_eq!(text.matches("<article").count(), 2);
    assert!(text.ends_with("</body>\n</html>\n"));
    assert_eq!(tmp_count(&dir), 0);
}

#[test]
fn zero_games_still_write_a_valid_empty_document() {
    let dir = tempfile::tempdir().unwrap();
    let report = html(
        vec![],
        &dir.path().join("e.HTML"),
        &HtmlExportOptions::default(),
    )
    .unwrap();
    assert_eq!(report.path, dir.path().join("e.HTML"));
    assert_eq!(report.written, 0);
    let text = fs::read_to_string(&report.path).unwrap();
    assert!(text.contains("<body>") && text.ends_with("</html>\n"));
}

#[test]
fn append_inserts_before_the_last_marker_and_keeps_everything_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.html");
    html(game_items(TWO), &dest, &HtmlExportOptions::default()).unwrap();
    let before = fs::read_to_string(&dest).unwrap();
    let marker = before.rfind("</body>").unwrap();
    let opts = HtmlExportOptions {
        append: true,
        ..HtmlExportOptions::default()
    };
    let report = html(game_items(ONE_MORE), &dest, &opts).unwrap();
    assert_eq!(report.written, 1);
    let after = fs::read_to_string(&dest).unwrap();
    assert!(
        after.as_bytes().starts_with(&before.as_bytes()[..marker]),
        "old part preserved byte for byte"
    );
    assert_eq!(after.matches("<article").count(), 3);
    assert_eq!(after.matches("</body>").count(), 1);
    assert!(after.ends_with("</body>\n</html>\n"));
    let pos_new = after.find("W3").unwrap();
    assert!(pos_new > marker - 1 && pos_new < after.rfind("</body>").unwrap());
    assert_eq!(tmp_count(&dir), 0);
}

#[test]
fn append_accepts_a_marker_in_other_case_with_leading_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("c.html");
    fs::write(&dest, "<html><body>\n<p>old</p>\n   </BODY>\n</html>\n").unwrap();
    let opts = HtmlExportOptions {
        append: true,
        html: HtmlOptions {
            end: "</body></html>\n".to_owned(),
            ..HtmlOptions::default()
        },
        ..HtmlExportOptions::default()
    };
    html(game_items(ONE_MORE), &dest, &opts).unwrap();
    let text = fs::read_to_string(&dest).unwrap();
    assert!(
        text.starts_with("<html><body>\n<p>old</p>\n<article"),
        "{text}"
    );
    assert!(text.ends_with("</body></html>\n"));
}

#[test]
fn append_without_a_marker_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("n.html");
    fs::write(&dest, "<html>no end here\n").unwrap();
    let opts = HtmlExportOptions {
        append: true,
        ..HtmlExportOptions::default()
    };
    let err = html(game_items(ONE_MORE), &dest, &opts).unwrap_err();
    assert!(matches!(err, ExportError::NoAppendMarker { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(&dest).unwrap(), "<html>no end here\n");
    assert_eq!(support::file_names(dir.path()), vec!["n.html"]);
}

#[test]
fn append_to_a_missing_or_non_utf8_file_is_an_error_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let opts = HtmlExportOptions {
        append: true,
        ..HtmlExportOptions::default()
    };
    let missing = html(
        game_items(ONE_MORE),
        &dir.path().join("missing.html"),
        &opts,
    );
    assert!(matches!(missing, Err(ExportError::Io { .. })));
    let dest = dir.path().join("l1.html");
    fs::write(&dest, b"caf\xE9\n</body>\n").unwrap();
    let r = html(game_items(ONE_MORE), &dest, &opts);
    assert!(matches!(r, Err(ExportError::NotUtf8 { .. })), "{r:?}");
    assert_eq!(fs::read(&dest).unwrap(), b"caf\xE9\n</body>\n");
    assert_eq!(tmp_count(&dir), 0);
}

#[test]
fn latex_file_and_append_use_end_document() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("g");
    let report = latex(game_items(TWO), &dest, &LatexExportOptions::default()).unwrap();
    assert_eq!(report.path, dir.path().join("g.tex"));
    let first = fs::read_to_string(&report.path).unwrap();
    assert!(first.starts_with("\\documentclass{scrreprt}"));
    assert!(first.ends_with("\\end{document}\n"));
    let cut = first.rfind("\\end{document}").unwrap();
    let opts = LatexExportOptions {
        append: true,
        ..LatexExportOptions::default()
    };
    latex(game_items(ONE_MORE), &report.path, &opts).unwrap();
    let after = fs::read_to_string(&report.path).unwrap();
    assert!(after.as_bytes().starts_with(&first.as_bytes()[..cut]));
    assert_eq!(after.matches("\\twocolumn[").count(), 3);
    assert_eq!(after.matches("\\end{document}").count(), 1);
    // Lower-case marker with leading spaces.
    fs::write(&report.path, "x\n  \\END{DOCUMENT}\n").unwrap();
    latex(game_items(ONE_MORE), &report.path, &opts).unwrap();
    assert!(
        fs::read_to_string(&report.path)
            .unwrap()
            .starts_with("x\n\\twocolumn[")
    );
    // No marker.
    fs::write(&report.path, "nothing\n").unwrap();
    assert!(matches!(
        latex(game_items(ONE_MORE), &report.path, &opts),
        Err(ExportError::NoAppendMarker { .. })
    ));
    assert_eq!(fs::read_to_string(&report.path).unwrap(), "nothing\n");
}

#[test]
fn unreadable_games_are_skipped_counted_and_do_not_stop_the_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut items = game_items(TWO);
    items.insert(
        1,
        ExportItem::Stored {
            number: 9,
            game: Err(Si4Error::InvalidGameNumber { number: 9, len: 2 }),
        },
    );
    let report = html(items, &dir.path().join("s"), &HtmlExportOptions::default()).unwrap();
    assert_eq!(report.written, 2);
    assert!(matches!(
        report.skipped.as_slice(),
        [(9, ExportSkipReason::Unreadable(_))]
    ));
}

#[test]
fn cancel_discards_the_new_file_and_keeps_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("c.html");
    fs::write(&dest, "old").unwrap();
    let token = CancelToken::new();
    let seen = token.clone();
    let games = games_from_pgn(TWO);
    let items = games.into_iter().enumerate().map(move |(i, g)| {
        if i == 1 {
            seen.cancel();
        }
        ExportItem::Game(Box::new(g))
    });
    let report = export_html(
        items,
        &dest,
        &HtmlExportOptions::default(),
        &mut NoDiagrams,
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
    assert_eq!(tmp_count(&dir), 0);
}

#[test]
fn cancel_can_keep_the_partial_document_closed_normally() {
    let dir = tempfile::tempdir().unwrap();
    let token = CancelToken::new();
    let seen = token.clone();
    let games = games_from_pgn(TWO);
    let items = games.into_iter().enumerate().map(move |(i, g)| {
        if i == 1 {
            seen.cancel();
        }
        ExportItem::Game(Box::new(g))
    });
    let opts = HtmlExportOptions {
        discard_on_cancel: false,
        ..HtmlExportOptions::default()
    };
    let report = export_html(
        items,
        &dir.path().join("k"),
        &opts,
        &mut NoDiagrams,
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(report.interrupted);
    assert_eq!(report.written, 2); // the item that sets the flag is still rendered
    let text = fs::read_to_string(&report.path).unwrap();
    assert!(
        text.ends_with("</html>\n"),
        "the partial file is a closed document"
    );
}

#[test]
fn destination_without_a_file_name_is_an_error() {
    let r = html(
        vec![],
        std::path::Path::new(""),
        &HtmlExportOptions::default(),
    );
    assert!(r.is_err());
}

#[test]
fn append_to_a_crlf_file_writes_crlf_and_keeps_the_bom() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("crlf.html");
    let old = "\u{feff}<html><body>\r\n<p>old</p>\r\n</body>\r\n</html>\r\n";
    fs::write(&dest, old).unwrap();
    let opts = HtmlExportOptions {
        append: true,
        ..HtmlExportOptions::default()
    };
    html(game_items(ONE_MORE), &dest, &opts).unwrap();
    let text = fs::read_to_string(&dest).unwrap();
    assert!(text.starts_with("\u{feff}<html><body>\r\n<p>old</p>\r\n<article"));
    assert!(text.contains("W3"));
    let bare_lf = text.replace("\r\n", "").contains('\n');
    assert!(!bare_lf, "mixed line endings: {text:?}");
}

#[test]
fn append_finds_the_marker_on_the_first_line_after_a_bom() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("bom.html");
    fs::write(&dest, "\u{feff}</body>\n</html>\n").unwrap();
    let opts = HtmlExportOptions {
        append: true,
        ..HtmlExportOptions::default()
    };
    html(game_items(ONE_MORE), &dest, &opts).unwrap();
    let text = fs::read_to_string(&dest).unwrap();
    assert!(text.starts_with("\u{feff}<article"), "{text:?}");
    assert_eq!(text.matches('\u{feff}').count(), 1);
}
