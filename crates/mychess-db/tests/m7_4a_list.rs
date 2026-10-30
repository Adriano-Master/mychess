//! M7-4a, criteria 5-7 of `docs/specs/m7-4-exportacao.md`: the game list
//! text (`format_game_line`, `export_game_list`) and the CSV export.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assigning_clones)]

#[path = "common/m7_4a_support.rs"]
mod support;

use std::fs;

use mychess_db::export::csv::{CsvColumn, CsvOptions, export_game_csv, format_csv_row};
use mychess_db::export::game_list::{
    DEFAULT_LIST_FORMAT, GameListOptions, export_game_list, format_game_line, format_game_line_with,
};
use mychess_db::si4::{GameFlags, IndexEntry};
use mychess_db::{CancelToken, ExportError, NoProgress};
use support::{build_db, games_from_pgn, six_game_db};

const NONE: [Option<char>; 6] = [None; 6];

#[test]
fn default_format_matches_the_columns_of_the_specification() {
    let db = six_game_db();
    let entry = db.db.entry(1).unwrap();
    let line = format_game_line(&entry, db.db.names(), (1, 1), DEFAULT_LIST_FORMAT, NONE);
    // Independent oracle: std padding counts characters, like the contract.
    let expected = format!(
        "{:>6}: {:<13} {:>4}  {:<13} {:>4}  {:<3}:{:>2} {:>4} {:<11} {:<4}",
        1, "Silva, João", 2500, "Müller", 0, "1-0", 5, 2020, "Rio BRA", "B20"
    );
    assert_eq!(line, expected);
    assert_eq!(
        line.chars().count(),
        6 + 2 + 13 + 1 + 4 + 2 + 13 + 1 + 4 + 2 + 3 + 1 + 2 + 1 + 4 + 1 + 11 + 1 + 4
    );
}

#[test]
fn name_with_accents_is_padded_by_characters_and_long_names_are_cut() {
    let db = six_game_db();
    let entry = db.db.entry(4).unwrap();
    let line = format_game_line(&entry, db.db.names(), (4, 4), "[b13][b4]", NONE);
    assert_eq!(line, "[Ünal Çelik   ][Ünal]");
}

#[test]
fn star_width_gives_estimated_elo_when_the_game_has_none() {
    let db = six_game_db();
    let entry = db.db.entry(4).unwrap();
    // White "Ana" has no Elo in the game.
    let est = |name: &str| (name == "Ana").then_some(2050);
    let line = format_game_line_with(&entry, db.db.names(), (4, 4), "[W*][B*]", NONE, &est);
    assert_eq!(line, "[(2050)][1800 ]");
    let none = format_game_line(&entry, db.db.names(), (4, 4), "[W*]", NONE);
    assert_eq!(none, "[     ]");
    let fixed = format_game_line(&entry, db.db.names(), (4, 4), "[W4]", NONE);
    assert_eq!(fixed, "[   0]");
}

#[test]
fn move_count_is_clipped_to_the_width() {
    let mut entry = IndexEntry::default();
    let db = six_game_db();
    let names = db.db.names();
    entry.num_half_moves = 19; // 10 moves
    assert_eq!(format_game_line(&entry, names, (1, 1), "m1", NONE), "#");
    assert_eq!(format_game_line(&entry, names, (1, 1), "m2", NONE), "10");
    entry.num_half_moves = 9; // 5 moves
    assert_eq!(format_game_line(&entry, names, (1, 1), "m1", NONE), "5");
    entry.num_half_moves = 200; // 100 moves
    assert_eq!(format_game_line(&entry, names, (1, 1), "m2", NONE), "##");
    assert_eq!(format_game_line(&entry, names, (1, 1), "m3", NONE), "100");
}

#[test]
fn country_needs_a_space_and_an_upper_case_letter() {
    let db = six_game_db();
    let names = db.db.names();
    let c = |n| format_game_line(&db.db.entry(n).unwrap(), names, (n, n), "c3", NONE);
    assert_eq!(c(4), "FRA"); // Paris FRA
    assert_eq!(c(5), "ESP");
    assert_eq!(c(6), "?  "); // Lisboa
    assert_eq!(c(3), "?  "); // ?
}

#[test]
fn result_in_one_and_three_columns() {
    let db = six_game_db();
    let line = |n, f| format_game_line(&db.db.entry(n).unwrap(), db.db.names(), (n, n), f, NONE);
    assert_eq!(line(1, "r1"), "1");
    assert_eq!(line(2, "r1"), "=");
    assert_eq!(line(4, "r1"), "0");
    assert_eq!(line(3, "r1"), "*");
    assert_eq!(line(2, "r3|"), "=-=|");
    assert_eq!(line(3, "r3|"), "*  |");
}

#[test]
fn flags_start_delete_and_marks() {
    let mut games: Vec<_> = games_from_pgn(&support::six_pgn());
    let mut flagged = vec![];
    for (i, g) in games.drain(..).enumerate() {
        let flags = if i == 0 {
            GameFlags::DELETE | GameFlags::WHITE_OPENING | GameFlags::CUSTOM1 | GameFlags::USER
        } else {
            GameFlags::empty()
        };
        flagged.push((g, flags));
    }
    let db = build_db(&flagged);
    let entry = |n| db.db.entry(n).unwrap();
    let names = db.db.names();
    let mut custom = NONE;
    custom[0] = Some('F');
    assert_eq!(
        format_game_line(&entry(1), names, (1, 1), "[D][S][M6]", custom),
        "[D][ ][WUF   ]"
    );
    // The delete flag is skipped by M; a custom flag without letter shows its digit.
    assert_eq!(
        format_game_line(&entry(1), names, (1, 1), "[U6]", NONE),
        "[WU1   ]"
    );
    // Game 3 starts from a position.
    assert_eq!(
        format_game_line(&entry(3), names, (3, 3), "[S][D]", NONE),
        "[S][ ]"
    );
}

#[test]
fn counts_final_material_and_stored_line() {
    let db = six_game_db();
    let names = db.db.names();
    let e6 = db.db.entry(6).unwrap();
    assert_eq!(
        format_game_line(&e6, names, (6, 6), "F", NONE),
        "qRBN8:qRBN8"
    );
    let e1 = db.db.entry(1).unwrap();
    assert!(e1.stored_line_code > 0);
    // Width 1 (the default) cuts the text to one character, like `strPad`.
    assert_eq!(format_game_line(&e1, names, (1, 1), "O", NONE), "1");
    let o = format_game_line(&e1, names, (1, 1), "O60", NONE);
    assert!(o.starts_with("1.e4 c5"), "{o}");
    let t = format_game_line(&e1, names, (1, 1), "T", NONE);
    assert_eq!(t, format!("{:>3} {}", e1.stored_line_code, o.trim_end()));
    // A game without stored line.
    let e3 = db.db.entry(3).unwrap();
    assert_eq!(format_game_line(&e3, names, (3, 3), "[O4]", NONE), "[    ]");
}

#[test]
fn unknown_letters_are_ignored_and_other_characters_copied() {
    let db = six_game_db();
    let e = db.db.entry(1).unwrap();
    assert_eq!(
        format_game_line(&e, db.db.names(), (7, 3), "g2 z9 | f2 q", NONE),
        " 7  |  3 "
    );
}

#[test]
fn huge_widths_are_limited_and_never_panic() {
    let db = six_game_db();
    let e = db.db.entry(1).unwrap();
    let line = format_game_line(
        &e,
        db.db.names(),
        (1, 1),
        "w99999999999999999999999 g*0",
        NONE,
    );
    assert_eq!(line.chars().count(), 1000 + 1 + 1);
}

fn dest(db: &support::Db, name: &str) -> std::path::PathBuf {
    support::path_in(&db.dir, name)
}

#[test]
fn export_writes_one_line_per_game_in_the_given_order() {
    let db = six_game_db();
    let out = dest(&db, "list");
    let report = export_game_list(
        &db.db,
        [3u32, 1, 2],
        &out,
        &GameListOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(report.written, 3);
    assert_eq!(report.path, dest(&db, "list.txt"));
    let bytes = fs::read(&report.path).unwrap();
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains('\r'));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("     3:"), "{}", lines[0]);
    assert!(lines[1].starts_with("     1:"));
    assert!(lines[2].starts_with("     2:"));
    assert!(text.ends_with('\n'));
}

#[test]
fn empty_filter_is_nothing_to_export_and_creates_no_file() {
    let db = six_game_db();
    let out = dest(&db, "none.txt");
    let result = export_game_list(
        &db.db,
        std::iter::empty::<u32>(),
        &out,
        &GameListOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(
        matches!(result, Err(ExportError::NothingToExport)),
        "{result:?}"
    );
    assert!(!out.exists());
    assert!(
        !support::file_names(db.dir.path())
            .iter()
            .any(|n| n.contains("tmp"))
    );
}

#[test]
fn format_without_known_letters_is_refused_before_touching_anything() {
    let db = six_game_db();
    let out = dest(&db, "x.txt");
    fs::write(&out, "keep").unwrap();
    for format in ["", "|-|", "z9 k3"] {
        let opts = GameListOptions {
            format: format.to_owned(),
            ..GameListOptions::default()
        };
        let result = export_game_list(
            &db.db,
            [1u32],
            &out,
            &opts,
            &mut NoProgress,
            &CancelToken::new(),
        );
        assert!(
            matches!(result, Err(ExportError::EmptyFormat)),
            "{format:?}"
        );
    }
    assert_eq!(fs::read_to_string(&out).unwrap(), "keep");
}

#[test]
fn append_adds_to_the_end_and_cancel_keeps_the_old_file() {
    let db = six_game_db();
    let out = dest(&db, "a.txt");
    fs::write(&out, "old\n").unwrap();
    let opts = GameListOptions {
        format: "g3".to_owned(),
        append: true,
        ..GameListOptions::default()
    };
    export_game_list(
        &db.db,
        [5u32, 6],
        &out,
        &opts,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&out).unwrap(), "old\n  5\n  6\n");
    let token = CancelToken::new();
    token.cancel();
    let report = export_game_list(&db.db, [1u32], &out, &opts, &mut NoProgress, &token).unwrap();
    assert!(report.interrupted);
    assert_eq!(fs::read_to_string(&out).unwrap(), "old\n  5\n  6\n");
    assert!(
        !support::file_names(db.dir.path())
            .iter()
            .any(|n| n.contains(".tmp"))
    );
}

#[test]
fn unreadable_entry_is_skipped_and_reported() {
    let db = six_game_db();
    let out = dest(&db, "u.txt");
    let report = export_game_list(
        &db.db,
        [1u32, 99, 2],
        &out,
        &GameListOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(report.written, 2);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0, 99);
}

// ---------------------------------------------------------------- CSV

fn tricky_db() -> support::Db {
    let mut g = games_from_pgn("[Event \"E\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n").remove(0);
    g.tags_mut().white = "Doe, \"Jane\"".to_owned();
    g.tags_mut().black = "Two\nLines".to_owned();
    g.tags_mut().event = "Semi;colon".to_owned();
    build_db(&[(g, GameFlags::empty())])
}

#[test]
fn csv_quotes_commas_quotes_and_line_breaks() {
    let db = tricky_db();
    let entry = db.db.entry(1).unwrap();
    let row = format_csv_row(
        &entry,
        db.db.names(),
        (1, 1),
        &[
            CsvColumn::White,
            CsvColumn::Black,
            CsvColumn::Event,
            CsvColumn::Result,
        ],
        ',',
        true,
    );
    assert_eq!(row, "\"Doe, \"\"Jane\"\"\",\"Two\nLines\",Semi;colon,1-0\n");
    let row = format_csv_row(
        &entry,
        db.db.names(),
        (1, 1),
        &[CsvColumn::Event],
        ';',
        true,
    );
    assert_eq!(row, "\"Semi;colon\"\n");
}

#[test]
fn csv_file_has_header_bom_and_separator() {
    let db = tricky_db();
    let out = dest(&db, "g");
    let opts = CsvOptions {
        columns: vec![CsvColumn::Number, CsvColumn::White, CsvColumn::Result],
        header: Some(vec![
            "Nº".to_owned(),
            "Branca, a".to_owned(),
            "Res".to_owned(),
        ]),
        separator: ';',
        bom: true,
        ..CsvOptions::default()
    };
    let report = export_game_csv(
        &db.db,
        [1u32],
        &out,
        &opts,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(report.path, dest(&db, "g.csv"));
    let bytes = fs::read(&report.path).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(
        text,
        "\u{feff}Nº;Branca, a;Res\n1;\"Doe, \"\"Jane\"\"\";1-0\n"
    );
}

#[test]
fn csv_without_header_or_bom_and_option_errors() {
    let db = six_game_db();
    let out = dest(&db, "plain.csv");
    let opts = CsvOptions {
        columns: vec![CsvColumn::Number, CsvColumn::Year],
        ..CsvOptions::default()
    };
    export_game_csv(
        &db.db,
        [1u32, 3],
        &out,
        &opts,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&out).unwrap(), "1,2020\n3,\n");
    let bad = |o: CsvOptions| {
        export_game_csv(
            &db.db,
            [1u32],
            &out,
            &o,
            &mut NoProgress,
            &CancelToken::new(),
        )
    };
    assert!(matches!(
        bad(CsvOptions {
            columns: vec![],
            ..CsvOptions::default()
        }),
        Err(ExportError::InvalidOptions(_))
    ));
    assert!(matches!(
        bad(CsvOptions {
            header: Some(vec![]),
            ..CsvOptions::default()
        }),
        Err(ExportError::InvalidOptions(_))
    ));
    assert!(matches!(
        bad(CsvOptions {
            separator: '\t',
            ..CsvOptions::default()
        }),
        Err(ExportError::InvalidOptions(_))
    ));
    assert!(matches!(
        export_game_csv(
            &db.db,
            std::iter::empty::<u32>(),
            &dest(&db, "z.csv"),
            &CsvOptions::default(),
            &mut NoProgress,
            &CancelToken::new()
        ),
        Err(ExportError::NothingToExport)
    ));
    assert!(!dest(&db, "z.csv").exists());
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

        /// Any format string gives a line without a panic and the same line
        /// twice; width digits are bounded, so the output stays small.
        #[test]
        fn arbitrary_formats_never_panic(format in "[ -~\u{e9}\u{1F600}*0-9]{0,40}", n in 1u32..=6) {
            let db = six_game_db();
            let entry = db.db.entry(n).unwrap();
            let a = format_game_line(&entry, db.db.names(), (n, n), &format, NONE);
            let b = format_game_line(&entry, db.db.names(), (n, n), &format, NONE);
            prop_assert_eq!(&a, &b);
            prop_assert!(a.chars().count() <= 40 * 1100);
        }
    }
}

fn formulas_db() -> support::Db {
    let mut g = games_from_pgn("[Event \"E\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n").remove(0);
    g.tags_mut().white = "=HYPERLINK(\"http://x\",\"y\")".to_owned();
    g.tags_mut().black = "+1".to_owned();
    g.tags_mut().event = "-1".to_owned();
    g.tags_mut().site = "@SUM(A1)".to_owned();
    g.tags_mut().round = "=cmd|' /C calc'!A0".to_owned();
    build_db(&[(g, GameFlags::empty())])
}

#[test]
fn csv_neutralizes_formula_triggers_in_text_columns_by_default() {
    let db = formulas_db();
    let entry = db.db.entry(1).unwrap();
    let columns = [
        CsvColumn::Number,
        CsvColumn::White,
        CsvColumn::Black,
        CsvColumn::Event,
        CsvColumn::Site,
        CsvColumn::Round,
        CsvColumn::Result,
    ];
    assert!(CsvOptions::default().neutralize_formulas);
    let row = format_csv_row(&entry, db.db.names(), (1, 1), &columns, ';', true);
    assert_eq!(
        row,
        "1;\"'=HYPERLINK(\"\"http://x\"\",\"\"y\"\")\";'+1;'-1;'@SUM(A1);'=cmd|' /C calc'!A0;1-0\n"
    );
    // Off: the text is written as it is.
    let raw = format_csv_row(&entry, db.db.names(), (1, 1), &columns, ';', false);
    assert!(raw.contains(";+1;-1;@SUM(A1);=cmd|"), "{raw}");

    // A single trigger character alone is plain text for a spreadsheet (and
    // `-` is a common PGN round): it is not quoted; longer text still is.
    let mut g = games_from_pgn("[Event \"E\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n").remove(0);
    g.tags_mut().white = "-".to_owned();
    g.tags_mut().black = "=".to_owned();
    g.tags_mut().event = "+".to_owned();
    g.tags_mut().site = "@".to_owned();
    g.tags_mut().round = "-".to_owned();
    let lone = build_db(&[(g, GameFlags::empty())]);
    let entry = lone.db.entry(1).unwrap();
    let row = format_csv_row(&entry, lone.db.names(), (1, 1), &columns, ';', true);
    assert_eq!(row, "1;-;=;+;@;-;1-0\n");
}

#[test]
fn csv_file_neutralizes_formulas_and_numbers_stay_numbers() {
    let db = formulas_db();
    let columns = vec![CsvColumn::Number, CsvColumn::Black, CsvColumn::Year];
    for (neutralize, expected) in [(true, "1,'+1,\n"), (false, "1,+1,\n")] {
        let out = dest(&db, if neutralize { "on" } else { "off" });
        let opts = CsvOptions {
            columns: columns.clone(),
            neutralize_formulas: neutralize,
            ..CsvOptions::default()
        };
        let report = export_game_csv(
            &db.db,
            [1u32],
            &out,
            &opts,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&report.path).unwrap(), expected);
    }
}

#[test]
fn append_to_a_crlf_file_uses_crlf() {
    let db = six_game_db();
    let out = dest(&db, "crlf.txt");
    fs::write(&out, "old\r\nline\r\n").unwrap();
    let opts = GameListOptions {
        format: "g3".to_owned(),
        append: true,
        ..GameListOptions::default()
    };
    export_game_list(
        &db.db,
        [5u32, 6],
        &out,
        &opts,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "old\r\nline\r\n  5\r\n  6\r\n"
    );
}
