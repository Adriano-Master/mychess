//! M7-4a, criterion 8 of `docs/specs/m7-4-exportacao.md`: EPD export of the
//! positions of a filter.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assigning_clones)]

#[path = "common/m7_4a_support.rs"]
mod support;

use std::fs;

use mychess_core::Position;
use mychess_db::export::epd::{EpdExportOptions, EpdItem, export_epd};
use mychess_db::si4::GameFlags;
use mychess_db::{CancelToken, ExportError, ExportItem, NoProgress};
use mychess_pgn::epd::{EpdBook, EpdOptions};
use support::{build_db, games_from_pgn, six_game_db};

const AFTER_E4_C5: &str = "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -";
const KPK: &str = "4k3/8/8/8/8/8/4P3/4K3 w - -";

fn run(
    db: &mut support::Db,
    plies: &[(u32, Option<u32>)],
    name: &str,
    opts: &EpdExportOptions,
) -> (
    Result<mychess_db::export::epd::EpdExportReport, ExportError>,
    std::path::PathBuf,
) {
    let items: Vec<EpdItem> = plies
        .iter()
        .map(|(n, ply)| EpdItem::new(ExportItem::stored(&mut db.db, *n), *ply))
        .collect();
    let dest = support::path_in(&db.dir, name);
    let r = export_epd(items, &dest, opts, &mut NoProgress, &CancelToken::new());
    (r, dest)
}

#[test]
fn filter_with_ply_gives_four_field_fens_and_drops_repeats() {
    let mut db = six_game_db();
    let plies = [(1, Some(2)), (2, Some(2)), (3, None), (6, None)];
    let (report, dest) = run(&mut db, &plies, "pos", &EpdExportOptions::default());
    let report = report.unwrap();
    assert_eq!(report.path, dest.with_extension("epd"));
    assert_eq!(report.written, 2);
    assert_eq!(report.repeated, 1);
    assert_eq!(report.not_selected, 1);
    let text = fs::read_to_string(&report.path).unwrap();
    assert_eq!(text, format!("{AFTER_E4_C5}\n{KPK}\n"));
}

#[test]
fn without_dedupe_repeated_positions_are_written() {
    let mut db = six_game_db();
    let opts = EpdExportOptions {
        dedupe: false,
        ..EpdExportOptions::default()
    };
    let (report, _) = run(&mut db, &[(1, Some(2)), (2, Some(2))], "r", &opts);
    let report = report.unwrap();
    assert_eq!((report.written, report.repeated), (2, 0));
    let text = fs::read_to_string(&report.path).unwrap();
    assert_eq!(text, format!("{AFTER_E4_C5}\n{AFTER_E4_C5}\n"));
}

#[test]
fn ply_zero_counts_as_no_ply_and_ply_past_the_end_is_the_final_position() {
    let mut db = six_game_db();
    let (r, _) = run(&mut db, &[(6, Some(0))], "z", &EpdExportOptions::default());
    assert!(matches!(r, Err(ExportError::NothingToExport)));
    let (r, _) = run(
        &mut db,
        &[(6, Some(500))],
        "far",
        &EpdExportOptions::default(),
    );
    let text = fs::read_to_string(r.unwrap().path).unwrap();
    // After 1.f3 e5 2.g4 Qh4#.
    assert_eq!(
        text,
        "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq -\n"
    );
}

#[test]
fn final_position_option_uses_the_end_of_every_game() {
    let mut db = six_game_db();
    let opts = EpdExportOptions {
        final_position: true,
        ..EpdExportOptions::default()
    };
    let (r, _) = run(&mut db, &[(5, None), (3, None)], "fin", &opts);
    let r = r.unwrap();
    assert_eq!(r.written, 2);
    let text = fs::read_to_string(r.path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "rnbqkbnr/ppp2ppp/4p3/3p4/2PP4/8/PP2PPPP/RNBQKBNR w KQkq -"
    );
    assert_eq!(lines[1], "8/3k4/8/4P3/8/8/8/4K3 b - -");
}

#[test]
fn clocks_id_and_game_number_are_added_on_request() {
    let mut db = six_game_db();
    let opts = EpdExportOptions {
        hmvc_fmvn: true,
        id: true,
        game_number: true,
        ..EpdExportOptions::default()
    };
    let (r, _) = run(&mut db, &[(1, Some(2))], "ops", &opts);
    let text = fs::read_to_string(r.unwrap().path).unwrap();
    assert_eq!(
        text,
        format!(
            "{AFTER_E4_C5} id \"Silva, João-Müller, Aberto 2020\"; c0 \"1\"; hmvc 0; fmvn 2;\n"
        )
    );
}

#[test]
fn id_operand_cannot_break_the_line() {
    let mut g = games_from_pgn("[Event \"E\"]\n[Result \"*\"]\n\n1. e4 e5 *\n").remove(0);
    g.tags_mut().white = "A\";bm Qh5;\nB".to_owned();
    let mut db = build_db(&[(g, GameFlags::empty())]);
    let opts = EpdExportOptions {
        id: true,
        ..EpdExportOptions::default()
    };
    let (r, _) = run(&mut db, &[(1, Some(1))], "inj", &opts);
    let text = fs::read_to_string(r.unwrap().path).unwrap();
    assert_eq!(text.lines().count(), 1);
    // One operation only: nothing was injected.
    let book = EpdBook::read(text.as_bytes(), EpdOptions::standard())
        .unwrap()
        .book;
    let rec = book.iter().next().unwrap();
    assert_eq!(rec.operations().len(), 1, "{text}");
    assert!(rec.operation("bm").is_none());
}

#[test]
fn en_passant_square_is_written_as_the_epd_writer_does_and_reads_back() {
    let g = games_from_pgn("[Event \"E\"]\n[Result \"*\"]\n\n1. e4 a6 2. e5 d5 *\n").remove(0);
    let mut db = build_db(&[(g, GameFlags::empty())]);
    let (r, _) = run(&mut db, &[(1, Some(4))], "ep", &EpdExportOptions::default());
    let report = r.unwrap();
    let text = fs::read_to_string(&report.path).unwrap();
    assert_eq!(
        text,
        "rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6\n"
    );
    let book = EpdBook::read_file(&report.path, EpdOptions::standard())
        .unwrap()
        .book;
    assert_eq!(book.len(), 1);
    let pos = Position::from_fen("rnbqkbnr/1pp1pppp/p7/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3")
        .unwrap();
    assert!(book.contains(&pos));
}

#[test]
fn file_read_back_by_epd_book_reproduces_the_positions() {
    let mut db = six_game_db();
    let (r, _) = run(
        &mut db,
        &[(1, Some(2)), (3, None), (5, Some(3))],
        "rb",
        &EpdExportOptions::default(),
    );
    let report = r.unwrap();
    assert_eq!(report.written, 3);
    let book = EpdBook::read_file(&report.path, EpdOptions::standard())
        .unwrap()
        .book;
    assert_eq!(book.len(), 3);
    assert!(book.contains(&Position::from_fen(&format!("{AFTER_E4_C5} 0 2")).unwrap()));
    assert!(book.contains(&Position::from_fen(&format!("{KPK} 0 1")).unwrap()));
}

#[test]
fn nothing_selected_is_nothing_to_export_and_leaves_no_file() {
    let mut db = six_game_db();
    let (r, dest) = run(
        &mut db,
        &[(1, None), (6, None)],
        "none",
        &EpdExportOptions::default(),
    );
    assert!(matches!(r, Err(ExportError::NothingToExport)));
    assert!(!dest.with_extension("epd").exists());
    assert!(
        !support::file_names(db.dir.path())
            .iter()
            .any(|n| n.contains("none"))
    );
    // Same for an empty iterator.
    let r = export_epd(
        Vec::<EpdItem>::new(),
        &support::path_in(&db.dir, "empty"),
        &EpdExportOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(r, Err(ExportError::NothingToExport)));
}

#[test]
fn append_keeps_the_old_lines_and_cancel_leaves_the_file_alone() {
    let mut db = six_game_db();
    let dest = support::path_in(&db.dir, "keep.epd");
    fs::write(&dest, "old line\n").unwrap();
    let opts = EpdExportOptions {
        append: true,
        ..EpdExportOptions::default()
    };
    let (r, _) = run(&mut db, &[(1, Some(2))], "keep", &opts);
    r.unwrap();
    assert_eq!(
        fs::read_to_string(&dest).unwrap(),
        format!("old line\n{AFTER_E4_C5}\n")
    );
    let token = CancelToken::new();
    token.cancel();
    let items = vec![EpdItem::new(ExportItem::stored(&mut db.db, 1), Some(2))];
    let report = export_epd(items, &dest, &opts, &mut NoProgress, &token).unwrap();
    assert!(report.interrupted);
    assert_eq!(
        fs::read_to_string(&dest).unwrap(),
        format!("old line\n{AFTER_E4_C5}\n")
    );
    assert!(
        !support::file_names(db.dir.path())
            .iter()
            .any(|n| n.contains(".tmp"))
    );
}

#[test]
fn unreadable_items_are_skipped_and_reported() {
    let mut db = six_game_db();
    let (r, _) = run(
        &mut db,
        &[(1, Some(2)), (77, Some(1))],
        "bad",
        &EpdExportOptions::default(),
    );
    let r = r.unwrap();
    assert_eq!(r.written, 1);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].0, 77);
}
