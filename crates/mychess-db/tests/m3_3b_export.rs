//! Contract tests of the PGN export (spec M3-3: "Exportação" rules 22-26 and
//! 55-60, D356-D360, D385-D387 and the acceptance criteria of
//! "Exportação"). Derived from the spec and from Scid's `sc_base_export`.
#![allow(
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names
)]

#[path = "common/m3_3b_support.rs"]
mod support;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mychess_core::{Game, Nag};
use mychess_db::si4::{GameFlags, Si4Database};
use mychess_db::{
    CancelToken, ExportError, ExportItem, ExportOptions, ExportSkipReason, Filter, ImportOptions,
    NoProgress, export_pgn, with_pgn_extension,
};
use mychess_pgn::write::{Encoding, LineWidth, NagOutput, NullMoves};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{THREE, break_record, canon, file_names, import_str};

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn stored_all(db: &mut Si4Database<std::fs::File>) -> Vec<ExportItem> {
    (1..=db.len()).map(|n| ExportItem::stored(db, n)).collect()
}

fn run(items: Vec<ExportItem>, dest: &Path, options: &ExportOptions) -> mychess_db::ExportReport {
    export_pgn(items, dest, options, &mut NoProgress, &CancelToken::new()).unwrap()
}

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.pgn");
    (dir, out)
}

fn read_text(path: &Path) -> String {
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

fn game_with(white: &str, comment: &str) -> Game {
    let mut g = Game::new();
    white.clone_into(&mut g.tags_mut().white);
    let root = g.root();
    let mv = g.position_at(root).unwrap().parse_uci("e2e4").unwrap();
    let id = g.add_move(root, mv).unwrap();
    if !comment.is_empty() {
        g.set_comment(id, comment).unwrap();
    }
    g
}

// ====================================================================
// Current game and stored games
// ====================================================================

#[test]
fn current_game_exports_the_in_memory_version_and_ends_with_a_blank_line() {
    let (_dir, out) = tmp();
    // an edit that was never saved to any database
    let game = game_with("Unsaved Player", "fresh edit");
    let r = run(
        vec![ExportItem::Game(Box::new(game))],
        &out,
        &ExportOptions::scid_dialog(),
    );
    assert_eq!(r.written, 1);
    assert!(r.skipped.is_empty() && !r.interrupted);
    assert_eq!(r.unrepresentable, 0);
    let text = read_text(&out);
    assert!(text.contains("[White \"Unsaved Player\"]"), "{text}");
    assert!(text.contains("{fresh edit}"), "{text}");
    assert!(
        text.ends_with("\n\n"),
        "a blank line follows the game: {text:?}"
    );
    assert!(!text.ends_with("\n\n\n"), "only one blank line: {text:?}");
}

#[test]
fn several_games_of_a_database_are_written_in_the_order_given() {
    let imp = import_str(THREE, &ImportOptions::default());
    let mut db = imp.open();
    let (_dir, out) = tmp();
    let r = run(stored_all(&mut db), &out, &ExportOptions::default());
    assert_eq!(r.written, 3);
    let text = read_text(&out);
    let pos: Vec<usize> = ["\"One\"", "\"Two\"", "\"Three\""]
        .iter()
        .map(|e| text.find(e).unwrap_or_else(|| panic!("{e} missing")))
        .collect();
    assert!(pos[0] < pos[1] && pos[1] < pos[2], "{text}");
    assert_eq!(text.matches("[Event ").count(), 3);
}

#[test]
fn filter_export_writes_only_the_games_in_the_filter_in_database_order() {
    let imp = import_str(THREE, &ImportOptions::default());
    let mut db = imp.open();
    let mut filter = Filter::new(db.len());
    filter.set(2, 0);
    let items: Vec<ExportItem> = filter
        .iter_games()
        .map(|n| ExportItem::stored(&mut db, n))
        .collect();
    let (_dir, out) = tmp();
    let r = run(items, &out, &ExportOptions::default());
    assert_eq!(r.written, 2);
    let text = read_text(&out);
    assert!(text.find("\"One\"").unwrap() < text.find("\"Three\"").unwrap());
    assert!(!text.contains("\"Two\""));
}

#[test]
fn empty_item_list_makes_an_empty_file_with_only_the_texts() {
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.start_text = "START".into();
    o.end_text = "END".into();
    let r = run(Vec::new(), &out, &o);
    assert_eq!(r.written, 0);
    assert_eq!(std::fs::read(&out).unwrap(), b"STARTEND");
}

#[test]
fn unreadable_game_is_skipped_with_its_number_and_the_rest_is_written() {
    let imp = import_str(THREE, &ImportOptions::default());
    break_record(&imp.base, 2);
    let mut db = imp.open();
    let (_dir, out) = tmp();
    let r = run(stored_all(&mut db), &out, &ExportOptions::default());
    assert_eq!(r.written, 2);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].0, 2);
    assert!(matches!(r.skipped[0].1, ExportSkipReason::Unreadable(_)));
    assert!(!r.interrupted);
    let text = read_text(&out);
    assert!(text.contains("\"One\"") && text.contains("\"Three\""));
    assert!(!text.contains("\"Two\""));
}

#[test]
fn every_game_unreadable_still_creates_a_valid_empty_file() {
    let imp = import_str(THREE, &ImportOptions::default());
    for n in 1..=3 {
        break_record(&imp.base, n);
    }
    let mut db = imp.open();
    let (_dir, out) = tmp();
    let r = run(stored_all(&mut db), &out, &ExportOptions::default());
    assert_eq!(r.written, 0);
    assert_eq!(r.skipped.len(), 3);
    assert_eq!(std::fs::read(&out).unwrap(), b"");
}

// ====================================================================
// Dialog defaults (rule 23, D357, D386)
// ====================================================================

#[test]
fn scid_dialog_presets_the_defaults_of_the_export_dialog() {
    let o = ExportOptions::scid_dialog();
    assert!(!o.append);
    assert_eq!(o.start_text, "");
    assert_eq!(o.end_text, "");
    // D357: UTF-8 without BOM, not Latin-1
    assert_eq!(o.encoding, Encoding::Utf8);
    let p = &o.pgn;
    assert!(p.comments, "comments = yes");
    assert!(p.variations, "variations = yes");
    assert!(
        p.move_number_space,
        "space after the move number = yes (1. e4)"
    );
    assert!(!p.indent_comments, "indent comments = no");
    assert!(p.indent_variations, "indent variations = yes");
    assert_eq!(
        p.width,
        LineWidth::Columns(75),
        "line breaks = yes (75 columns)"
    );
    assert!(!p.column, "column style = no");
    assert_eq!(p.nags, NagOutput::Numeric, "symbols instead of $n = no");
    assert!(!p.strip_marks, "strip mark codes = no");
    assert!(!p.scid_flags, "keep ScidFlags = no");
    assert_eq!(
        p.null_moves,
        NullMoves::ConvertToComment,
        "null moves -> comment = yes"
    );
}

#[test]
fn dialog_output_has_numeric_nags_spaced_numbers_and_indented_variations() {
    let pgn = "[Event \"x\"]\n\n1. e4 $1 e5 {middle} (1... c5 2. Nf3) 2. Nf3 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    let mut db = imp.open();
    let (_dir, out) = tmp();
    run(stored_all(&mut db), &out, &ExportOptions::scid_dialog());
    let text = read_text(&out);
    assert!(text.contains("1. e4 $1"), "{text}");
    assert!(!text.contains("1.e4"), "{text}");
    assert!(text.contains("{middle}"), "{text}");
    // indented variation: on its own line, starting with spaces and "("
    let var_line = text.lines().find(|l| l.contains("(1... c5")).unwrap();
    assert!(
        var_line.starts_with(' '),
        "variation must be indented: {var_line:?}"
    );
}

#[test]
fn dialog_output_wraps_movetext_at_75_columns() {
    let mut pgn = String::from("[Event \"x\"]\n\n");
    let moves = ["Nf3", "Nf6", "Ng1", "Ng8"];
    for i in 0..60 {
        if i % 2 == 0 {
            write!(pgn, "{}. ", i / 2 + 1).unwrap();
        }
        pgn.push_str(moves[i % 4]);
        pgn.push(' ');
    }
    pgn.push('*');
    let imp = import_str(&pgn, &ImportOptions::default());
    let mut db = imp.open();
    let (_dir, out) = tmp();
    run(stored_all(&mut db), &out, &ExportOptions::scid_dialog());
    let text = read_text(&out);
    let movetext: Vec<&str> = text
        .lines()
        .skip_while(|l| l.starts_with('[') || l.is_empty())
        .collect();
    assert!(movetext.len() > 2, "long game must wrap: {text}");
    assert!(
        movetext.iter().all(|l| l.chars().count() <= 75),
        "a line is longer than 75 columns:\n{text}"
    );
}

#[test]
fn dialog_does_not_write_scid_flags_even_when_the_game_has_them() {
    let mut g = game_with("w", "");
    g.tags_mut().set_extra("ScidFlags", "DW").unwrap();
    let (_dir, out) = tmp();
    run(
        vec![ExportItem::Game(Box::new(g.clone()))],
        &out,
        &ExportOptions::scid_dialog(),
    );
    assert!(!read_text(&out).contains("ScidFlags"));
    // and writes them when asked
    let (_dir2, out2) = tmp();
    let mut o = ExportOptions::scid_dialog();
    o.pgn = o.pgn.with_scid_flags(true);
    run(vec![ExportItem::Game(Box::new(g))], &out2, &o);
    assert!(read_text(&out2).contains("[ScidFlags \"DW\"]"));
}

#[test]
fn dialog_converts_a_null_move_line_into_a_comment() {
    let pgn = "[Event \"x\"]\n\n1. e4 e5 2. -- d5 3. d4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    let mut db = imp.open();
    let (_dir, out) = tmp();
    run(stored_all(&mut db), &out, &ExportOptions::scid_dialog());
    let text = read_text(&out);
    // the result stays readable PGN: the null move is inside a comment
    let start = text.find("--").unwrap();
    let open = text[..start]
        .rfind('{')
        .expect("a comment must open before --");
    assert!(
        text[open..start].find('}').is_none(),
        "the null move must be inside braces: {text}"
    );
    // strict default keeps the null move as a move
    let (_d2, out2) = tmp();
    run(stored_all(&mut db), &out2, &ExportOptions::default());
    let strict = read_text(&out2);
    assert!(strict.contains("2. -- d5"), "{strict}");
}

#[test]
fn braces_inside_comments_become_parentheses() {
    let mut g = game_with("w", "");
    let first = g.mainline().next().unwrap();
    g.set_comment(first, "a {nested} b").unwrap();
    let (_dir, out) = tmp();
    run(
        vec![ExportItem::Game(Box::new(g))],
        &out,
        &ExportOptions::scid_dialog(),
    );
    let text = read_text(&out);
    assert!(text.contains("{a (nested) b}"), "{text}");
}

// ====================================================================
// start / end text, cancelling
// ====================================================================

#[test]
fn start_and_end_text_are_written_verbatim_around_the_games() {
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.start_text = "% header\n".into();
    o.end_text = "% footer".into();
    let items = vec![ExportItem::Game(Box::new(game_with("w", "")))];
    let r = run(items, &out, &o);
    assert_eq!(r.written, 1);
    let text = read_text(&out);
    assert!(text.starts_with("% header\n[Event "), "{text}");
    assert!(text.ends_with("\n\n% footer"), "{text:?}");
}

#[test]
fn cancelling_keeps_the_games_written_and_still_writes_the_end_text() {
    let (dir, out) = tmp();
    let token = CancelToken::new();
    let t2 = token.clone();
    let items = (0..10).map(move |i| {
        if i == 2 {
            t2.cancel();
        }
        ExportItem::Game(Box::new(game_with(&format!("P{i}"), "")))
    });
    let mut o = ExportOptions::default();
    o.start_text = "S\n".into();
    o.end_text = "THE END\n".into();
    let r = export_pgn(items, &out, &o, &mut NoProgress, &token).unwrap();
    assert!(r.interrupted);
    assert!(r.written >= 1 && r.written < 10, "written {}", r.written);
    let text = read_text(&out);
    assert!(text.starts_with("S\n"));
    assert!(text.ends_with("THE END\n"), "{text:?}");
    assert_eq!(text.matches("[Event ").count() as u64, r.written);
    // the last game is whole: its blank line is there before the end text
    assert!(text.contains("\n\nTHE END\n"));
    assert_eq!(file_names(dir.path()), ["out.pgn"], "no temporary left");
}

#[test]
fn cancelling_before_the_first_game_still_creates_the_file_with_the_texts() {
    let (dir, out) = tmp();
    let token = CancelToken::new();
    token.cancel();
    let mut o = ExportOptions::default();
    o.start_text = "A".into();
    o.end_text = "Z".into();
    let r = export_pgn(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &out,
        &o,
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(r.interrupted);
    assert_eq!(r.written, 0);
    assert_eq!(std::fs::read(&out).unwrap(), b"AZ");
    assert_eq!(file_names(dir.path()), ["out.pgn"]);
}

#[test]
fn the_items_after_a_cancel_are_not_consumed_more_than_needed() {
    // the iterator is lazy (D385): after the cancel no further item is pulled
    let (_dir, out) = tmp();
    let token = CancelToken::new();
    let t2 = token.clone();
    let mut pulled = 0u32;
    let items = std::iter::from_fn(|| {
        pulled += 1;
        if pulled == 3 {
            t2.cancel();
        }
        (pulled <= 1000).then(|| ExportItem::Game(Box::new(game_with("w", ""))))
    });
    let r = export_pgn(
        items,
        &out,
        &ExportOptions::default(),
        &mut NoProgress,
        &token,
    )
    .unwrap();
    assert!(r.interrupted);
    assert!(pulled <= 5, "pulled {pulled} items after the cancel");
}

#[test]
fn progress_counts_games_and_ends_at_the_total() {
    let (_dir, out) = tmp();
    let items: Vec<ExportItem> = (0..250)
        .map(|i| ExportItem::Game(Box::new(game_with(&format!("P{i}"), ""))))
        .collect();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    let r = export_pgn(
        items,
        &out,
        &ExportOptions::default(),
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(r.written, 250);
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert_eq!(seen.last().copied(), Some((250, 250)));
    assert!(seen.iter().all(|(d, t)| d <= t));
}

// ====================================================================
// Append (rule 57, D358, D387)
// ====================================================================

#[test]
fn append_to_a_bom_file_stays_utf8_and_does_not_repeat_the_start_text() {
    let (_dir, out) = tmp();
    let mut first = BOM.to_vec();
    first.extend_from_slice("[Event \"Old\"]\n\n1. e4 *\n\n".as_bytes());
    std::fs::write(&out, &first).unwrap();
    let mut o = ExportOptions::default();
    o.append = true;
    o.start_text = "START-TEXT".into();
    o.end_text = "END-TEXT".into();
    // even though the options say Latin-1, the file decides
    o.encoding = Encoding::Latin1;
    let r = run(
        vec![ExportItem::Game(Box::new(game_with("Zoë ♟", "")))],
        &out,
        &o,
    );
    assert_eq!(r.written, 1);
    assert_eq!(r.unrepresentable, 0, "UTF-8 holds everything");
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(&BOM));
    assert_eq!(
        bytes.windows(3).filter(|w| *w == BOM).count(),
        1,
        "the BOM is not written again"
    );
    assert!(bytes.starts_with(&first), "existing content is untouched");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("Zoë ♟"));
    assert!(
        !text.contains("START-TEXT"),
        "start text is for new files only"
    );
    assert!(text.ends_with("END-TEXT"));
}

#[test]
fn append_to_a_latin1_file_writes_latin1() {
    let (_dir, out) = tmp();
    // not valid UTF-8: 0xE9 alone
    let mut existing = b"[Event \"caf".to_vec();
    existing.push(0xE9);
    existing.extend_from_slice(b"\"]\n\n1. e4 *\n\n");
    std::fs::write(&out, &existing).unwrap();
    let mut o = ExportOptions::default();
    o.append = true;
    o.encoding = Encoding::Utf8;
    let r = run(
        vec![ExportItem::Game(Box::new(game_with(
            "Zo\u{e9}\u{20ac}",
            "",
        )))],
        &out,
        &o,
    );
    assert_eq!(r.unrepresentable, 1, "the euro sign has no Latin-1 code");
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(&existing));
    let tail = &bytes[existing.len()..];
    assert!(
        tail.windows(4).any(|w| w == [b'Z', b'o', 0xE9, b'?']),
        "{tail:?}"
    );
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "must still be Latin-1"
    );
}

#[test]
fn append_to_a_plain_valid_utf8_file_without_bom_writes_utf8_without_bom() {
    let (_dir, out) = tmp();
    let existing = "[Event \"Zoë\"]\n\n1. e4 *\n\n";
    std::fs::write(&out, existing).unwrap();
    let mut o = ExportOptions::default();
    o.append = true;
    o.encoding = Encoding::Latin1;
    run(
        vec![ExportItem::Game(Box::new(game_with("Ωmega", "")))],
        &out,
        &o,
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(!bytes.starts_with(&BOM));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with(existing));
    assert!(text.contains("Ωmega"));
}

#[test]
fn append_detects_utf8_even_when_a_character_straddles_the_read_block() {
    // 64 KiB blocks (D387): put a 2-byte character across the 65536 boundary
    let (_dir, out) = tmp();
    let mut existing = String::from("[Event \"x\"]\n\n1. e4 {");
    while existing.len() < 65_535 {
        existing.push('a');
    }
    existing.push('é'); // bytes 65535..65537
    existing.push_str("} *\n\n");
    assert!(existing.is_char_boundary(65_535));
    std::fs::write(&out, &existing).unwrap();
    let mut o = ExportOptions::default();
    o.append = true;
    o.encoding = Encoding::Latin1;
    run(
        vec![ExportItem::Game(Box::new(game_with("Ωmega", "")))],
        &out,
        &o,
    );
    let text = String::from_utf8(std::fs::read(&out).unwrap()).expect("still valid UTF-8");
    assert!(text.contains("Ωmega"));
}

#[test]
fn append_to_a_missing_file_is_an_error_and_creates_nothing() {
    let (dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.append = true;
    let err = export_pgn(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &out,
        &o,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, ExportError::Io { .. }), "{err:?}");
    assert_eq!(file_names(dir.path()), Vec::<String>::new());
}

#[test]
fn append_to_an_empty_file_acts_like_a_new_file() {
    let (_dir, out) = tmp();
    std::fs::write(&out, b"").unwrap();
    let mut o = ExportOptions::default();
    o.append = true;
    o.encoding = Encoding::Utf8Bom;
    o.start_text = "START\n".into();
    run(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &out,
        &o,
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        bytes.starts_with(&BOM),
        "empty file: the configured BOM applies"
    );
    assert_eq!(&bytes[3..9], b"START\n");
}

#[test]
fn append_twice_accumulates_games_in_order() {
    let (_dir, out) = tmp();
    run(
        vec![ExportItem::Game(Box::new(game_with("first", "")))],
        &out,
        &ExportOptions::default(),
    );
    let mut o = ExportOptions::default();
    o.append = true;
    run(
        vec![ExportItem::Game(Box::new(game_with("second", "")))],
        &out,
        &o,
    );
    let text = read_text(&out);
    assert!(text.find("first").unwrap() < text.find("second").unwrap());
    assert_eq!(text.matches("[Event ").count(), 2);
}

#[test]
fn append_leaves_no_temporary_file() {
    let (dir, out) = tmp();
    std::fs::write(&out, "x\n").unwrap();
    let mut o = ExportOptions::default();
    o.append = true;
    run(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &out,
        &o,
    );
    assert_eq!(file_names(dir.path()), ["out.pgn"]);
}

// ====================================================================
// Encodings (D357, D387)
// ====================================================================

#[test]
fn utf8_is_the_default_and_has_no_bom() {
    let (_dir, out) = tmp();
    let r = run(
        vec![ExportItem::Game(Box::new(game_with("Zoë", "")))],
        &out,
        &ExportOptions::scid_dialog(),
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(!bytes.starts_with(&BOM));
    assert!(bytes.windows(2).any(|w| w == [0xC3, 0xAB]));
    assert_eq!(r.unrepresentable, 0);
}

#[test]
fn utf8_with_bom_starts_with_one_bom_before_the_start_text() {
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.encoding = Encoding::Utf8Bom;
    o.start_text = "T".into();
    run(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &out,
        &o,
    );
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..4], [0xEF, 0xBB, 0xBF, b'T']);
    assert_eq!(bytes.windows(3).filter(|w| *w == BOM).count(), 1);
}

#[test]
fn latin1_writes_one_byte_per_character_and_counts_the_unrepresentable_ones() {
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.encoding = Encoding::Latin1;
    o.start_text = "start \u{263a}\n".into(); // 1 unrepresentable
    let mut g = game_with("Ωmega", "♟♟ caf\u{e9}"); // 1 + 2
    g.tags_mut().event = "\u{1F600}".into(); // 1 (one character, 4 UTF-8 bytes)
    let r = run(vec![ExportItem::Game(Box::new(g))], &out, &o);
    assert_eq!(r.unrepresentable, 5);
    let bytes = std::fs::read(&out).unwrap();
    assert!(!bytes.starts_with(&BOM));
    assert!(bytes.contains(&0xE9), "e-acute must be the single byte E9");
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    assert!(text.contains("[White \"?mega\"]"), "{text}");
    assert!(text.contains("[Event \"?\"]"), "{text}");
    assert!(text.contains("{?? caf\u{e9}}"), "{text}");
    assert!(text.starts_with("start ?\n"));
}

#[test]
fn latin1_without_unrepresentable_characters_reports_zero() {
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.encoding = Encoding::Latin1;
    let r = run(
        vec![ExportItem::Game(Box::new(game_with("Müller", "ça va")))],
        &out,
        &o,
    );
    assert_eq!(r.unrepresentable, 0);
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.contains(&0xFC) && bytes.contains(&0xE7));
}

// ====================================================================
// File names and atomic writing (D358, D359, D387)
// ====================================================================

#[rstest]
#[case("x", "x.pgn")]
#[case("x.pgn", "x.pgn")]
#[case("X.PGN", "X.PGN")]
#[case("x.Pgn", "x.Pgn")]
#[case("a.b", "a.b.pgn")]
#[case("games.pgn.bak", "games.pgn.bak.pgn")]
#[case("pgn", "pgn.pgn")]
#[case("jogos de xadrez", "jogos de xadrez.pgn")]
#[case("partidá", "partidá.pgn")]
fn extension_is_added_unless_the_name_already_ends_in_pgn_in_any_case(
    #[case] name: &str,
    #[case] expected: &str,
) {
    assert_eq!(with_pgn_extension(Path::new(name)), PathBuf::from(expected));
    // with a directory, which is built with join (no separators in the test)
    let dir = Path::new("some").join("dir");
    assert_eq!(
        with_pgn_extension(&dir.join(name)),
        dir.join(expected),
        "directory part must be kept"
    );
}

#[test]
fn a_directory_named_dot_pgn_does_not_count_as_the_extension() {
    let p = Path::new("folder.pgn").join("games");
    assert_eq!(
        with_pgn_extension(&p),
        Path::new("folder.pgn").join("games.pgn")
    );
}

#[test]
fn export_to_upper_case_pgn_name_does_not_double_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("X.PGN");
    let r = run(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &dest,
        &ExportOptions::default(),
    );
    assert_eq!(r.path, dest);
    assert_eq!(file_names(dir.path()), ["X.PGN"]);
}

#[test]
fn export_to_a_name_without_extension_adds_pgn_and_reports_the_final_path() {
    let dir = tempfile::tempdir().unwrap();
    let r = run(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &dir.path().join("games"),
        &ExportOptions::default(),
    );
    assert_eq!(r.path, dir.path().join("games.pgn"));
    assert_eq!(file_names(dir.path()), ["games.pgn"]);
}

#[test]
fn success_leaves_no_temporary_and_replaces_an_existing_file_entirely() {
    let (dir, out) = tmp();
    std::fs::write(&out, "OLD CONTENT THAT MUST DISAPPEAR\n").unwrap();
    run(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &out,
        &ExportOptions::default(),
    );
    assert_eq!(file_names(dir.path()), ["out.pgn"]);
    assert!(!read_text(&out).contains("OLD CONTENT"));
}

#[test]
fn destination_without_a_file_name_is_refused() {
    let err = export_pgn(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        Path::new(""),
        &ExportOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, ExportError::NoFileName { .. }), "{err:?}");
}

#[test]
fn destination_in_a_missing_directory_is_an_io_error_that_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("no_such_dir").join("out.pgn");
    let err = export_pgn(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &dest,
        &ExportOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, ExportError::Io { .. }), "{err:?}");
    assert_eq!(file_names(dir.path()), Vec::<String>::new());
}

#[test]
fn destination_that_is_a_directory_fails_and_leaves_it_alone() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("target.pgn");
    std::fs::create_dir(&dest).unwrap();
    let result = export_pgn(
        vec![ExportItem::Game(Box::new(game_with("w", "")))],
        &dest,
        &ExportOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(result, Err(ExportError::Io { .. })), "{result:?}");
    assert!(dest.is_dir());
    assert_eq!(file_names(dir.path()), ["target.pgn"], "temporary removed");
}

// ====================================================================
// Round trip export -> import (acceptance criteria)
// ====================================================================

const RICH: &str = "[Event \"Rich\"]\n[Site \"Zürich\"]\n[Date \"2020.05.??\"]\n[Round \"3.1\"]\n\
    [White \"Müller, Jörg\"]\n[Black \"Åsa\"]\n[Result \"1-0\"]\n[WhiteElo \"2650\"]\n\
    [BlackElo \"2410\"]\n[ECO \"C84\"]\n[Annotator \"tester\"]\n\n\
    {opening} 1. e4 $1 e5 (1... c5 $14 2. Nf3 {sicilian} (2. c3) 2... d6) 2. Nf3 {develop} Nc6 \
    3. Bb5 a6 $2 4. Ba4 Nf6 5. O-O 1-0\n\n\
    [Event \"Fen\"]\n[SetUp \"1\"]\n[FEN \"8/P6k/8/8/8/8/8/K7 w - - 0 1\"]\n[Result \"*\"]\n\n\
    1. a8=N+ Kg6 2. Nb6 *\n\n\
    [Event \"Empty\"]\n[Result \"*\"]\n\n*\n";

fn roundtrip(options: &ExportOptions) {
    let first = import_str(RICH, &ImportOptions::default());
    let mut db1 = first.open();
    let (_dir, out) = tmp();
    let r = run(stored_all(&mut db1), &out, options);
    assert_eq!(r.written, 3);
    let second = support::import_bytes(&std::fs::read(&out).unwrap(), &ImportOptions::default());
    assert!(!second.report.has_warnings(), "{}", second.report);
    assert_eq!(second.report.games_added, 3);
    let mut db2 = second.open();
    for n in 1..=3 {
        let a = db1.game(n).unwrap();
        let b = db2.game(n).unwrap();
        assert!(
            a.same_content(&b),
            "game {n}\n{}\n----\n{}",
            canon(&a),
            canon(&b)
        );
    }
}

#[test]
fn export_then_import_gives_the_same_games_with_default_options() {
    roundtrip(&ExportOptions::default());
}

#[test]
fn export_then_import_gives_the_same_games_with_the_dialog_options() {
    roundtrip(&ExportOptions::scid_dialog());
}

#[test]
fn export_then_import_gives_the_same_games_in_latin1_and_utf8_bom() {
    for enc in [Encoding::Latin1, Encoding::Utf8Bom] {
        let mut o = ExportOptions::default();
        o.encoding = enc;
        roundtrip(&o);
    }
}

#[test]
fn export_then_import_gives_the_same_games_without_line_wrapping_and_with_symbols() {
    let mut o = ExportOptions::scid_dialog();
    o.pgn = o.pgn.with_nags(NagOutput::Symbolic);
    roundtrip(&o);
    let mut o = ExportOptions::default();
    o.pgn = o.pgn.clone();
    roundtrip(&o);
}

#[test]
fn scid_flags_round_trip_through_export_and_import() {
    // one game per flag letter plus one with all of them
    let letters = mychess_db::si4::FLAG_LETTERS;
    let mut pgn = String::new();
    for (i, c) in letters.chars().enumerate() {
        write!(pgn, "[Event \"F{i}\"]\n[ScidFlags \"{c}\"]\n\n1. e4 *\n\n").unwrap();
    }
    write!(
        pgn,
        "[Event \"All\"]\n[ScidFlags \"{letters}\"]\n\n1. d4 *\n\n"
    )
    .unwrap();
    pgn.push_str("[Event \"None\"]\n\n1. c4 *\n\n");
    let first = import_str(&pgn, &ImportOptions::default());
    assert!(!first.report.has_warnings(), "{}", first.report);
    let mut db1 = first.open();
    let total = db1.len();
    assert_eq!(total as usize, letters.chars().count() + 2);

    // The caller (the app) carries the index flags into the game as the
    // ScidFlags tag; the writer prints it with scid_flags = true.
    let items: Vec<ExportItem> = (1..=total)
        .map(|n| {
            let flags = db1.entry(n).unwrap().flags;
            let mut game = db1.game(n).unwrap();
            let letters = flags.letters();
            if !letters.is_empty() {
                game.tags_mut().set_extra("ScidFlags", &letters).unwrap();
            }
            ExportItem::Game(Box::new(game))
        })
        .collect();
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.pgn = o.pgn.with_scid_flags(true);
    run(items, &out, &o);
    let text = read_text(&out);
    assert_eq!(text.matches("[ScidFlags ").count() as u32, total - 1);

    let second = support::import_bytes(text.as_bytes(), &ImportOptions::default());
    let db2 = second.open();
    assert_eq!(db2.len(), total);
    for n in 1..=total {
        assert_eq!(
            db1.entry(n).unwrap().flags,
            db2.entry(n).unwrap().flags,
            "flags of game {n}"
        );
    }
    // 'D' in particular
    assert!(db2.entry(1).unwrap().flags.contains(GameFlags::DELETE));
}

#[test]
fn exporting_stored_games_with_scid_flags_on_writes_the_flags_of_the_index() {
    // Acceptance criterion: "Reimportar a saída com -scidFlags reproduz os 22
    // flags de cada partida". The natural call is ExportItem::stored with
    // scid_flags = true; the flags live in the index entry, not in the Game.
    let pgn = "[Event \"x\"]\n[ScidFlags \"DWT\"]\n\n1. e4 *\n";
    let first = import_str(pgn, &ImportOptions::default());
    let mut db1 = first.open();
    let (_dir, out) = tmp();
    let mut o = ExportOptions::default();
    o.pgn = o.pgn.with_scid_flags(true);
    run(stored_all(&mut db1), &out, &o);
    let second = support::import_bytes(&std::fs::read(&out).unwrap(), &ImportOptions::default());
    assert_eq!(
        second.open().entry(1).unwrap().flags.letters(),
        "DWT",
        "flags lost: the exported PGN was\n{}",
        read_text(&out)
    );
}

#[test]
fn nags_comments_and_variations_survive_export_and_import() {
    let mut g = Game::new();
    let root = g.root();
    let m = g.position_at(root).unwrap().parse_uci("e2e4").unwrap();
    let id = g.add_move(root, m).unwrap();
    g.add_nag(id, Nag::new(1).unwrap()).unwrap();
    g.set_comment(id, "best").unwrap();
    let alt = g.position_at(root).unwrap().parse_uci("d2d4").unwrap();
    g.add_move(root, alt).unwrap();
    let (_dir, out) = tmp();
    run(
        vec![ExportItem::Game(Box::new(g.clone()))],
        &out,
        &ExportOptions::scid_dialog(),
    );
    let back = support::import_bytes(&std::fs::read(&out).unwrap(), &ImportOptions::default());
    let mut db = back.open();
    let got = db.game(1).unwrap();
    assert_eq!(canon(&got), canon(&g));
}
