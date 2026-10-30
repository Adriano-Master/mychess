//! Contract tests of the PGN import (spec M3-3: "Importação" rules 42-50,
//! "`pgnscid`" rule 51, D345-D350, D376-D384 and the acceptance criteria of
//! "Importação"). Derived from the spec and from Scid's `sc_base_import`, not
//! from the implementation.
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
use std::io::{self, Read};

use mychess_core::GameResult;
use mychess_db::si4::{GameFlags, NameKind, Si4Database, Si4Writer};
use mychess_db::{
    CancelToken, Filter, ImportError, ImportNoticeKind, ImportOptions, ImportStop, NoProgress,
    PlayerStats, PlyCountTag, ScidFlagsTag, SkipReason, import_pgn, import_pgn_file,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{THREE, canon, file_names, gzip_stored, import_bytes, import_str, many_games};

fn notice_kinds(r: &mychess_db::ImportReport) -> Vec<ImportNoticeKind> {
    r.notices.iter().map(|n| n.kind.clone()).collect()
}

// ====================================================================
// Basic import
// ====================================================================

#[test]
fn three_games_into_a_new_database_reopen_with_names_and_eco() {
    let imp = import_str(THREE, &ImportOptions::default());
    let r = &imp.report;
    assert_eq!(r.games_read, 3);
    assert_eq!(r.games_added, 3);
    assert_eq!(r.added, 1..4);
    assert!(r.games_skipped.is_empty());
    assert!(!r.interrupted && r.stopped.is_none());
    assert!(!r.has_warnings(), "{r}");
    assert_eq!(r.to_string(), "", "nothing to say about a clean import");

    let mut db = imp.open();
    assert_eq!(db.len(), 3);
    let g1 = db.game(1).unwrap();
    assert_eq!(g1.tags().event, "One");
    assert_eq!(g1.tags().site, "Paris");
    assert_eq!(g1.tags().white, "Alpha");
    assert_eq!(g1.tags().black, "Beta");
    assert_eq!(g1.tags().result, GameResult::White);
    assert_eq!(g1.tags().eco.map(|e| e.to_string()), Some("C44".to_owned()));
    assert_eq!(g1.mainline().count(), 3);
    let g2 = db.game(2).unwrap();
    assert_eq!(g2.tags().white, "Gamma");
    assert_eq!(g2.tags().result, GameResult::Black);
    assert_eq!(g2.tags().eco.map(|e| e.to_string()), Some("D06".to_owned()));
    let g3 = db.game(3).unwrap();
    assert_eq!(g3.tags().result, GameResult::Draw);

    // the name base is shared: "Alpha" is one player, "Delta" too
    let alpha = db.entry(1).unwrap().white_id;
    assert_eq!(db.entry(3).unwrap().white_id, alpha);
    assert_eq!(db.entry(2).unwrap().black_id, db.entry(3).unwrap().black_id);
    assert_eq!(db.names().num_names(NameKind::Player), 4);
    assert_eq!(db.names().num_names(NameKind::Event), 3);
    assert_eq!(db.names().get(NameKind::Player, alpha), Some(&b"Alpha"[..]));
}

#[test]
fn import_leaves_only_the_three_database_files() {
    let imp = import_str(THREE, &ImportOptions::default());
    let names = file_names(imp.dir.path());
    assert_eq!(names, ["db.sg4", "db.si4", "db.sn4"]);
}

#[test]
fn games_added_to_the_filter_by_the_caller_make_it_cover_the_database() {
    let imp = import_str(THREE, &ImportOptions::default());
    let mut filter = Filter::new(0);
    for _ in 0..imp.report.games_added {
        filter.append(1);
    }
    let db = imp.open();
    assert_eq!(filter.len(), db.len());
    assert_eq!(filter.count(), 3);
    assert!((1..=3).all(|g| filter.get(g) == 1));
}

#[test]
fn importing_into_a_database_with_games_continues_the_numbering() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let first = import_pgn(
        many_games(2).as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(first.added, 1..3);
    let second = import_pgn(
        THREE.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(second.added, 3..6);
    assert_eq!(second.games_added, 3);
    drop(writer);
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 5);
    assert_eq!(db.game(3).unwrap().tags().event, "One");
    assert_eq!(db.game(5).unwrap().tags().event, "Three");
    // the games already there are untouched
    assert_eq!(db.game(1).unwrap().tags().event, "E0");
}

#[test]
fn empty_import_reports_an_empty_range() {
    let imp = import_str("", &ImportOptions::default());
    assert_eq!(imp.report.games_read, 0);
    assert_eq!(imp.report.games_added, 0);
    assert!(imp.report.added.is_empty());
    assert!(!imp.report.has_warnings());
    assert_eq!(imp.open().len(), 0);
}

#[rstest]
#[case::whitespace("  \n\n\t\n")]
#[case::text_only("this is not a pgn file\nat all\n")]
#[case::bom_only("\u{feff}")]
fn input_without_games_imports_nothing_and_does_not_fail(#[case] text: &str) {
    let imp = import_str(text, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 0);
    assert_eq!(imp.open().len(), 0);
}

#[test]
fn same_input_gives_identical_database_files_in_file_order() {
    // rayon prepares in parallel; the result must not depend on scheduling
    let pgn = many_games(450);
    let a = import_str(&pgn, &ImportOptions::default());
    let b = import_str(&pgn, &ImportOptions::default());
    for ext in ["si4", "sg4"] {
        let fa = std::fs::read(a.base.with_extension(ext)).unwrap();
        let fb = std::fs::read(b.base.with_extension(ext)).unwrap();
        assert_eq!(fa, fb, ".{ext} differs between two identical imports");
    }
    let mut db = a.open();
    for i in 0..450u32 {
        assert_eq!(db.game(i + 1).unwrap().tags().event, format!("E{i}"));
    }
}

// ====================================================================
// Warnings (rule 48)
// ====================================================================

#[test]
fn illegal_move_in_game_two_keeps_all_three_and_warns_with_game_and_line() {
    let pgn = THREE.replace("1. d4 d5 2. c4 0-1", "1. d4 d5 2. Ke5 0-1");
    let imp = import_str(&pgn, &ImportOptions::default());
    let r = &imp.report;
    assert_eq!(r.games_read, 3);
    assert_eq!(r.games_added, 3);
    assert!(r.has_warnings());
    let text = r.to_string();
    let first = text.lines().next().unwrap();
    // "(game 2, line L) message", L inside game 2 of the file
    let rest = first.strip_prefix("(game 2, line ").unwrap_or_else(|| {
        panic!("first message must be for game 2: {text}");
    });
    let (line, message) = rest.split_once(") ").unwrap();
    let line: usize = line.parse().unwrap();
    let start_of_game_2 = pgn
        .lines()
        .position(|l| l.starts_with("[Event \"Two\""))
        .unwrap()
        + 1;
    let start_of_game_3 = pgn
        .lines()
        .position(|l| l.starts_with("[Event \"Three\""))
        .unwrap()
        + 1;
    assert!(
        (start_of_game_2..start_of_game_3).contains(&line),
        "line {line} is not inside game 2 ({start_of_game_2}..{start_of_game_3})"
    );
    assert!(!message.is_empty());
    assert!(
        text.lines().all(|l| l.starts_with("(game 2, ")),
        "only game 2 has a problem: {text}"
    );

    // rule 45: the game is saved with what was read before the error
    let mut db = imp.open();
    assert_eq!(db.len(), 3);
    let g2 = db.game(2).unwrap();
    assert_eq!(g2.mainline().count(), 2, "1.d4 d5 were kept");
    // the neighbours are intact
    assert_eq!(db.game(3).unwrap().mainline().count(), 2);
    assert_eq!(db.game(1).unwrap().mainline().count(), 3);
}

#[test]
fn every_warning_line_has_the_scid_prefix_in_game_order() {
    let mut pgn = String::new();
    for i in 1..=4 {
        write!(pgn, "[Event \"E{i}\"]\n\n1. e4 e5 2. Ke5 *\n\n").unwrap();
    }
    let imp = import_str(&pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 4);
    let text = imp.report.to_string();
    let games: Vec<u32> = text
        .lines()
        .map(|l| {
            let inner = l.strip_prefix("(game ").unwrap_or_else(|| panic!("{l}"));
            assert!(inner.contains(", line "), "{l}");
            inner.split(',').next().unwrap().parse().unwrap()
        })
        .collect();
    assert_eq!(games, [1, 2, 3, 4]);
}

#[test]
fn text_between_games_is_informative_and_not_a_warning() {
    let pgn =
        "preface text\n\n[Event \"x\"]\n\n1. e4 *\n\nmiddle text\n\n[Event \"y\"]\n\n1. d4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 2);
    assert!(!imp.report.has_warnings(), "{}", imp.report);
    assert_eq!(imp.report.to_string(), "");
}

#[test]
fn max_messages_caps_the_report_and_counts_what_was_dropped() {
    let mut pgn = String::new();
    for i in 1..=10 {
        write!(pgn, "[Event \"E{i}\"]\n\n1. e4 e5 2. Ke5 *\n\n").unwrap();
    }
    let mut options = ImportOptions::default();
    options.max_messages = 3;
    let imp = import_str(&pgn, &options);
    assert_eq!(
        imp.report.games_added, 10,
        "dropping messages never drops games"
    );
    assert!(imp.report.diagnostics.len() + imp.report.notices.len() <= 3);
    assert!(imp.report.messages_dropped >= 7);
    assert!(imp.report.has_warnings());
    assert!(imp.report.to_string().contains("further messages"));
}

// ====================================================================
// Cancelling (rules 16, 43, 47; D348)
// ====================================================================

#[test]
fn cancel_after_the_first_hundred_keeps_them_and_the_database_opens() {
    let pgn = many_games(250);
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let token = CancelToken::new();
    let seen = token.clone();
    let mut sink = move |_: u64, _: u64| seen.cancel();
    let r = import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut sink,
        &token,
    )
    .unwrap();
    drop(writer);
    assert!(r.interrupted);
    assert_eq!(r.games_added, 100);
    assert_eq!(r.added, 1..101);
    assert!(r.stopped.is_none(), "cancelling is not a stop (D348)");
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 100);
    // the first hundred of the file, in order, all decodable
    for i in 0..100u32 {
        assert_eq!(db.game(i + 1).unwrap().tags().event, format!("E{i}"));
    }
}

#[test]
fn cancel_before_starting_imports_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let token = CancelToken::new();
    token.cancel();
    let r = import_pgn(
        THREE.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &token,
    )
    .unwrap();
    drop(writer);
    assert!(r.interrupted);
    assert_eq!(r.games_added, 0);
    assert!(r.added.is_empty());
    assert_eq!(Si4Database::open(&base).unwrap().len(), 0);
}

#[test]
fn a_run_that_is_not_cancelled_is_not_marked_interrupted_even_with_exactly_one_chunk() {
    let imp = import_str(&many_games(100), &ImportOptions::default());
    assert!(!imp.report.interrupted);
    assert_eq!(imp.report.games_added, 100);
}

// ====================================================================
// Limits (D347)
// ====================================================================

#[test]
fn tag_value_over_255_bytes_is_cut_on_a_utf8_boundary() {
    // 'é' is 2 bytes: 255 falls in the middle of a character
    let long = "é".repeat(200);
    let pgn = format!("[Event \"x\"]\n[Note \"{long}\"]\n\n1. e4 *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 1);
    assert!(
        notice_kinds(&imp.report).contains(&ImportNoticeKind::TagValueTruncated {
            name: "Note".to_owned()
        })
    );
    assert!(imp.report.has_warnings());
    assert!(imp.report.to_string().starts_with("(game 1, line "));
    let mut db = imp.open();
    let game = db.game(1).unwrap();
    let value = game.tags().get_extra("Note").unwrap();
    assert!(value.len() <= 255, "len {}", value.len());
    assert!(value.len() >= 253, "too much was cut: {}", value.len());
    assert!(long.starts_with(value), "must be a prefix of the original");
    assert_eq!(
        value.chars().count(),
        value.len() / 2,
        "only whole characters"
    );
}

#[test]
fn tag_value_of_exactly_255_bytes_is_not_touched() {
    let exact = "v".repeat(255);
    let pgn = format!("[Event \"x\"]\n[Note \"{exact}\"]\n\n1. e4 *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    assert!(imp.report.notices.is_empty(), "{:?}", imp.report.notices);
    let mut db = imp.open();
    assert_eq!(
        db.game(1).unwrap().tags().get_extra("Note"),
        Some(exact.as_str())
    );
}

#[test]
fn tag_value_with_a_four_byte_character_at_the_cut() {
    // 3 ASCII bytes + 4-byte emoji repeated: 255 = 3 + 63*4: ends inside the 64th emoji? 3+4k
    let long = format!("abc{}", "\u{1F600}".repeat(100));
    let pgn = format!("[Event \"x\"]\n[Note \"{long}\"]\n\n1. e4 *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    let mut db = imp.open();
    let game = db.game(1).unwrap();
    let value = game.tags().get_extra("Note").unwrap();
    assert!(value.len() <= 255);
    assert_eq!(
        value.len(),
        3 + 4 * 63,
        "the incomplete emoji is dropped entirely"
    );
}

#[test]
fn more_than_40_extra_tags_keep_the_first_40() {
    let mut pgn = String::from("[Event \"x\"]\n");
    for i in 0..45 {
        writeln!(pgn, "[X{i:02} \"v{i}\"]").unwrap();
    }
    pgn.push_str("\n1. e4 *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 1);
    assert_eq!(
        notice_kinds(&imp.report),
        [ImportNoticeKind::TooManyExtraTags { dropped: 5 }]
    );
    let mut db = imp.open();
    let game = db.game(1).unwrap();
    let extras = game.tags().extras();
    assert_eq!(extras.len(), 40);
    for (i, (name, _)) in extras.iter().enumerate() {
        assert_eq!(name, &format!("X{i:02}"), "order is kept");
    }
    assert_eq!(game.tags().get_extra("X40"), None);
}

#[test]
fn exactly_40_extra_tags_are_all_kept_without_a_notice() {
    let mut pgn = String::from("[Event \"x\"]\n");
    for i in 0..40 {
        writeln!(pgn, "[X{i:02} \"v\"]").unwrap();
    }
    pgn.push_str("\n1. e4 *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    assert!(imp.report.notices.is_empty(), "{:?}", imp.report.notices);
    assert_eq!(imp.open().game(1).unwrap().tags().extras().len(), 40);
}

#[test]
fn tag_with_an_empty_name_is_dropped_with_a_notice() {
    let pgn = "[Event \"x\"]\n[ \"orphan\"]\n[Keep \"yes\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 1);
    let mut db = imp.open();
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().get_extra("Keep"), Some("yes"));
    assert!(
        game.tags().extras().iter().all(|(n, _)| !n.is_empty()),
        "no tag with an empty name may be stored"
    );
    assert!(imp.report.has_warnings(), "dropping a tag must be reported");
}

#[rstest]
#[case::white("White")]
#[case::black("Black")]
#[case::event("Event")]
#[case::site("Site")]
#[case::round("Round")]
fn name_over_255_bytes_is_truncated_with_a_notice(#[case] tag: &str) {
    let long = "n".repeat(300);
    let field = |t: &str, d: &str| {
        if t == tag { long.clone() } else { d.to_owned() }
    };
    let pgn = format!(
        "[Event \"{}\"]\n[Site \"{}\"]\n[White \"{}\"]\n[Black \"{}\"]\n[Round \"{}\"]\n\n1. e4 *\n",
        field("Event", "e"),
        field("Site", "s"),
        field("White", "w"),
        field("Black", "b"),
        field("Round", "1"),
    );
    let imp = import_str(&pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 1, "{}", imp.report);
    assert!(
        notice_kinds(&imp.report).contains(&ImportNoticeKind::NameTruncated),
        "{:?}",
        imp.report.notices
    );
    let mut db = imp.open();
    let game = db.game(1).unwrap();
    let t = game.tags();
    let got = match tag {
        "White" => &t.white,
        "Black" => &t.black,
        "Event" => &t.event,
        "Site" => &t.site,
        _ => &t.round,
    };
    assert_eq!(got.len(), 255);
    assert!(long.starts_with(got.as_str()));
}

#[test]
fn name_of_exactly_255_bytes_is_not_a_notice() {
    let exact = "n".repeat(255);
    let pgn = format!("[Event \"{exact}\"]\n[White \"{exact}\"]\n\n1. e4 *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    assert!(imp.report.notices.is_empty());
    assert_eq!(imp.open().game(1).unwrap().tags().white, exact);
}

#[test]
fn record_over_131071_bytes_is_skipped_and_the_import_goes_on() {
    let big = "x".repeat(200_000);
    let pgn = format!(
        "[Event \"a\"]\n\n1. e4 {{{big}}} *\n\n[Event \"b\"]\n\n1. d4 *\n\n[Event \"c\"]\n\n1. c4 *\n"
    );
    let imp = import_str(&pgn, &ImportOptions::default());
    let r = &imp.report;
    assert_eq!(r.games_read, 3);
    assert_eq!(r.games_added, 2);
    assert_eq!(r.added, 1..3, "numbers of stored games stay contiguous");
    assert_eq!(r.games_skipped.len(), 1);
    let (number, reason) = &r.games_skipped[0];
    assert_eq!(*number, 1, "number of the game in the file");
    match reason {
        SkipReason::RecordTooLarge { size, limit } => {
            assert_eq!(*limit, 131_071);
            assert!(size > limit);
        }
        other => panic!("unexpected reason {other:?}"),
    }
    assert!(r.has_warnings());
    assert!(r.stopped.is_none());
    let mut db = imp.open();
    assert_eq!(db.len(), 2);
    assert_eq!(db.game(1).unwrap().tags().event, "b");
    assert_eq!(db.game(2).unwrap().tags().event, "c");
}

#[test]
fn record_just_under_the_limit_is_stored() {
    // the comment alone is below the limit, with room for the header
    let almost = "y".repeat(131_000);
    let pgn = format!("[Event \"a\"]\n\n1. e4 {{{almost}}} *\n");
    let imp = import_str(&pgn, &ImportOptions::default());
    assert!(imp.report.games_skipped.is_empty(), "{}", imp.report);
    assert_eq!(imp.report.games_added, 1);
}

#[test]
fn game_that_cannot_be_encoded_is_skipped_not_fatal() {
    // a NUL in a name cannot go in the name file
    let pgn = "[Event \"a\"]\n[White \"x\u{0}y\"]\n\n1. e4 *\n\n[Event \"b\"]\n\n1. d4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_read, 2);
    // either the NUL was cleaned by the reader (both stored) or the game was
    // skipped; in no case may the import fail or lose the second game
    let mut db = imp.open();
    assert_eq!(db.len(), imp.report.games_added);
    assert_eq!(
        db.game(db.len()).unwrap().tags().event,
        "b",
        "the game after the problem is stored"
    );
}

#[test]
fn round_name_limit_stops_the_import_and_keeps_what_fits() {
    let limit = 262_143usize;
    let total = limit + 7;
    let mut pgn = String::with_capacity(total * 32);
    for i in 0..total {
        write!(pgn, "[Round \"r{i}\"]\n\n1. e4 *\n\n").unwrap();
    }
    let imp = import_str(&pgn, &ImportOptions::default());
    let r = &imp.report;
    match &r.stopped {
        Some(ImportStop::NameLimit { kind, limit: l }) => {
            assert_eq!(*kind, NameKind::Round);
            assert_eq!(*l as usize, limit);
        }
        other => panic!("expected NameLimit, got {other:?}"),
    }
    assert!(r.has_warnings());
    assert!(r.to_string().contains("import stopped"));
    assert!(r.games_added as usize <= limit);
    assert!(r.games_added as usize >= limit - 1, "got {}", r.games_added);
    assert!(!r.interrupted);
    let db = imp.open();
    assert_eq!(db.len(), r.games_added);
    assert!(db.names().num_names(NameKind::Round) as usize <= limit);
}

// ====================================================================
// ScidFlags (rule 50, D350)
// ====================================================================

#[test]
fn scid_flags_d_marks_the_game_deleted_and_creates_no_extra_tag() {
    let pgn = "[Event \"x\"]\n[ScidFlags \"D\"]\n[Other \"o\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert!(!imp.report.has_warnings(), "{}", imp.report);
    let mut db = imp.open();
    assert!(db.entry(1).unwrap().flags.contains(GameFlags::DELETE));
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().get_extra("ScidFlags"), None);
    assert_eq!(game.tags().extras().len(), 1, "only [Other]");
    let raw = db.raw_game(1).unwrap();
    assert!(raw.extra_tags.iter().all(|(n, _)| n != b"ScidFlags"));
}

#[test]
fn scid_flags_with_every_letter_sets_the_19_editable_flags() {
    let pgn = "[Event \"x\"]\n[ScidFlags \"DWBMENPTKQ!?U123456\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert!(!imp.report.has_warnings(), "{}", imp.report);
    let db = imp.open();
    let flags = db.entry(1).unwrap().flags;
    assert!(flags.contains(GameFlags::EDITABLE), "{flags:?}");
    assert_eq!(flags.letters(), "DWBMENPTKQ!?U123456");
    assert!(!flags.contains(GameFlags::START), "standard start position");
}

#[test]
fn scid_flags_letters_are_case_insensitive() {
    let pgn = "[Event \"x\"]\n[ScidFlags \"dw\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert!(imp.report.notices.is_empty());
    let flags = imp.open().entry(1).unwrap().flags;
    assert!(flags.contains(GameFlags::DELETE | GameFlags::WHITE_OPENING));
}

#[test]
fn unknown_flag_letter_is_a_notice_and_does_not_turn_on_start() {
    // Scid maps an unknown letter to START by accident (rule 50); D350 does not
    let pgn = "[Event \"x\"]\n[ScidFlags \"DzW\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert_eq!(
        notice_kinds(&imp.report),
        [ImportNoticeKind::UnknownFlagChar('z')]
    );
    assert!(imp.report.to_string().starts_with("(game 1, line "));
    let flags = imp.open().entry(1).unwrap().flags;
    assert!(flags.contains(GameFlags::DELETE | GameFlags::WHITE_OPENING));
    assert!(!flags.contains(GameFlags::START));
}

#[test]
fn empty_scid_flags_tag_is_consumed_without_notice() {
    let pgn = "[Event \"x\"]\n[ScidFlags \"\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert!(imp.report.notices.is_empty());
    let mut db = imp.open();
    assert_eq!(db.game(1).unwrap().tags().extras().len(), 0);
}

#[test]
fn scid_flags_can_be_ignored() {
    let mut options = ImportOptions::default();
    options.scid_flags = ScidFlagsTag::Ignore;
    let pgn = "[Event \"x\"]\n[ScidFlags \"D\"]\n\n1. e4 *\n";
    let imp = import_str(pgn, &options);
    let mut db = imp.open();
    assert!(!db.entry(1).unwrap().flags.contains(GameFlags::DELETE));
    assert_eq!(
        db.game(1).unwrap().tags().extras().len(),
        0,
        "tag dropped too"
    );
}

#[test]
fn scid_flags_do_not_leak_into_the_next_game() {
    let pgn = "[Event \"a\"]\n[ScidFlags \"D\"]\n\n1. e4 *\n\n[Event \"b\"]\n\n1. d4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    let db = imp.open();
    assert!(db.entry(1).unwrap().flags.contains(GameFlags::DELETE));
    assert!(!db.entry(2).unwrap().flags.contains(GameFlags::DELETE));
}

// ====================================================================
// Text outside the games and PlyCount (rules 44, 51; D349)
// ====================================================================

const WITH_JUNK: &str =
    "hello there\n\n[Event \"x\"]\n\n1. e4 *\n\nbetween games\n\n[Event \"y\"]\n\n1. d4 *\n";

#[test]
fn text_outside_the_games_is_ignored_by_default() {
    let imp = import_str(WITH_JUNK, &ImportOptions::default());
    let mut db = imp.open();
    assert_eq!(db.len(), 2);
    assert_eq!(db.game(1).unwrap().initial_comment(), None);
    assert_eq!(db.game(2).unwrap().initial_comment(), None);
}

#[test]
fn cli_profile_keeps_text_outside_the_games_as_initial_comment_of_the_next_game() {
    let imp = import_str(WITH_JUNK, &ImportOptions::cli());
    let mut db = imp.open();
    assert_eq!(db.len(), 2);
    assert_eq!(db.game(1).unwrap().initial_comment(), Some("hello there"));
    assert_eq!(db.game(2).unwrap().initial_comment(), Some("between games"));
}

#[test]
fn text_after_the_last_game_is_never_a_comment() {
    let pgn = "[Event \"x\"]\n\n1. e4 *\n\ntrailing words\n";
    let imp = import_str(pgn, &ImportOptions::cli());
    let mut db = imp.open();
    assert_eq!(db.len(), 1);
    let game = db.game(1).unwrap();
    let all = canon(&game);
    assert!(!all.contains("trailing"), "{all}");
}

#[test]
fn cli_profile_has_the_pgnscid_defaults() {
    let cli = ImportOptions::cli();
    assert_eq!(cli.ply_count, PlyCountTag::Ignore);
    assert_eq!(cli.pre_game_text, mychess_pgn::read::PreGameText::AsComment);
    let open_base = ImportOptions::default();
    assert_eq!(open_base.ply_count, PlyCountTag::Keep);
    assert_eq!(
        open_base.pre_game_text,
        mychess_pgn::read::PreGameText::Ignore
    );
    assert_eq!(open_base.scid_flags, ScidFlagsTag::Apply);
}

#[test]
fn ply_count_is_kept_as_an_extra_tag_in_the_open_database_profile_only() {
    let pgn = "[Event \"x\"]\n[PlyCount \"3\"]\n\n1. e4 e5 2. Nf3 *\n";
    let kept = import_str(pgn, &ImportOptions::default());
    assert_eq!(
        kept.open().game(1).unwrap().tags().get_extra("PlyCount"),
        Some("3")
    );
    let dropped = import_str(pgn, &ImportOptions::cli());
    assert_eq!(
        dropped.open().game(1).unwrap().tags().get_extra("PlyCount"),
        None
    );
}

// ====================================================================
// Encodings and line ends
// ====================================================================

#[test]
fn latin1_input_is_converted_to_utf8() {
    // "Müller" and a comment with 'é' in ISO 8859-1
    let mut bytes = b"[Event \"x\"]\n[White \"M".to_vec();
    bytes.push(0xFC);
    bytes.extend_from_slice(b"ller\"]\n\n1. e4 {caf");
    bytes.push(0xE9);
    bytes.extend_from_slice(b"} *\n");
    let imp = import_bytes(&bytes, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 1);
    let mut db = imp.open();
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().white, "Müller");
    let first = game.mainline().next().unwrap();
    assert_eq!(game.comment(first).unwrap(), Some("café"));
    assert!(!imp.report.has_warnings(), "{}", imp.report);
}

#[test]
fn utf8_with_bom_imports_the_same_as_without() {
    let plain = "[Event \"x\"]\n[White \"Müller\"]\n\n1. e4 *\n";
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(plain.as_bytes());
    let a = import_str(plain, &ImportOptions::default());
    let b = import_bytes(&with_bom, &ImportOptions::default());
    assert_eq!(b.report.games_added, 1);
    let mut da = a.open();
    let mut db = b.open();
    assert_eq!(db.game(1).unwrap().tags().white, "Müller");
    assert_eq!(canon(&da.game(1).unwrap()), canon(&db.game(1).unwrap()));
}

#[rstest]
#[case::lf("\n")]
#[case::crlf("\r\n")]
#[case::cr("\r")]
fn line_terminators_give_the_same_games(#[case] eol: &str) {
    let pgn = THREE.replace('\n', eol);
    let imp = import_str(&pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 3);
    assert!(!imp.report.has_warnings(), "{}", imp.report);
    let reference = import_str(THREE, &ImportOptions::default());
    let mut a = imp.open();
    let mut b = reference.open();
    for n in 1..=3 {
        assert_eq!(
            canon(&a.game(n).unwrap()),
            canon(&b.game(n).unwrap()),
            "game {n}"
        );
    }
}

#[test]
fn crlf_changes_only_the_line_numbers_of_the_warnings_not_their_count() {
    let lf = THREE.replace("2. c4 0-1", "2. Ke5 0-1");
    let crlf = lf.replace('\n', "\r\n");
    let a = import_str(&lf, &ImportOptions::default());
    let b = import_str(&crlf, &ImportOptions::default());
    assert_eq!(
        a.report.to_string(),
        b.report.to_string(),
        "CRLF must count as one line end"
    );
}

// ====================================================================
// Errors and read-only
// ====================================================================

/// A reader that serves `data` and then fails.
struct FailingReader {
    data: Vec<u8>,
    pos: usize,
}

impl Read for FailingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.data.len() {
            return Err(io::Error::other("disk on fire"));
        }
        let n = buf.len().min(self.data.len() - self.pos).min(64);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[test]
fn read_error_in_the_middle_keeps_the_games_read_before_it() {
    let pgn = many_games(150);
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let reader = FailingReader {
        data: pgn.into_bytes(),
        pos: 0,
    };
    let r = import_pgn(
        reader,
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("a read failure is reported, not returned as Err (D379)");
    drop(writer);
    match &r.stopped {
        Some(ImportStop::Read { message }) => assert!(!message.is_empty()),
        other => panic!("expected ImportStop::Read, got {other:?}"),
    }
    assert!(r.has_warnings());
    assert!(r.to_string().contains("read error"));
    assert!(!r.interrupted);
    // game 150 is still being read when the error comes; 149 are complete
    assert!(r.games_added >= 149, "got {}", r.games_added);
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), r.games_added);
}

#[test]
fn read_error_before_any_game_stores_nothing_and_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let reader = FailingReader {
        data: Vec::new(),
        pos: 0,
    };
    let r = import_pgn(
        reader,
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    drop(writer);
    assert!(matches!(r.stopped, Some(ImportStop::Read { .. })));
    assert_eq!(r.games_added, 0);
    assert_eq!(Si4Database::open(&base).unwrap().len(), 0);
}

#[test]
fn importing_into_a_type_3_database_is_refused_with_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("ro");
    let mut writer = Si4Writer::create(&base, 3).unwrap();
    let result = import_pgn(
        THREE.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    );
    assert!(matches!(result, Err(ImportError::ReadOnly)));
    drop(writer);
    assert_eq!(
        Si4Database::open(&base).unwrap().len(),
        0,
        "nothing written"
    );
}

#[test]
fn importing_from_a_missing_file_is_an_open_error_with_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut writer = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
    let missing = dir.path().join("missing.pgn");
    let err = import_pgn_file(
        &missing,
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    match err {
        ImportError::Open { path, .. } => assert_eq!(path, missing),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn import_from_a_file_matches_import_from_memory() {
    let dir = tempfile::tempdir().unwrap();
    let pgn_path = dir.path().join("in.pgn");
    std::fs::write(&pgn_path, THREE).unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let r = import_pgn_file(
        &pgn_path,
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    drop(writer);
    assert_eq!(r.games_added, 3);
    let mem = import_str(THREE, &ImportOptions::default());
    for ext in ["si4", "sg4"] {
        assert_eq!(
            std::fs::read(base.with_extension(ext)).unwrap(),
            std::fs::read(mem.base.with_extension(ext)).unwrap(),
            ".{ext}"
        );
    }
}

// ====================================================================
// Progress (rule 43, D376)
// ====================================================================

#[test]
fn progress_is_in_bytes_and_ends_at_the_input_size() {
    // games big enough that the reader cannot swallow the file in one gulp
    let filler = "z".repeat(16_000);
    let mut pgn = String::new();
    for i in 0..250 {
        write!(pgn, "[Event \"E{i}\"]\n\n1. e4 {{{filler}}} e5 *\n\n").unwrap();
    }
    let size = pgn.len() as u64;
    let dir = tempfile::tempdir().unwrap();
    let mut writer = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
    let mut options = ImportOptions::default();
    options.total_bytes = Some(size);
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &options,
        None,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(
        seen.len() >= 3,
        "one report per 100 games plus the end: {seen:?}"
    );
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {seen:?}"
    );
    assert!(seen.iter().all(|(d, t)| *t == size && *d <= size));
    assert_eq!(seen.last().copied(), Some((size, size)));
    assert!(
        seen[0].0 > 0 && seen[0].0 < size,
        "first report is partial: {seen:?}"
    );
}

#[test]
fn progress_total_is_zero_when_the_size_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let mut writer = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    import_pgn(
        THREE.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|(_, t)| *t == 0));
}

#[test]
fn file_import_uses_the_file_size_as_total() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in.pgn");
    std::fs::write(&path, many_games(250)).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    let mut writer = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
    let mut last = (0, 0);
    let mut sink = |d: u64, t: u64| last = (d, t);
    import_pgn_file(
        &path,
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(last, (size, size));
}

// ====================================================================
// PlayerStats (D332, D373, D376)
// ====================================================================

#[test]
fn player_stats_keep_the_highest_elo_of_each_player_over_the_import() {
    let pgn = "[Event \"1\"]\n[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"2500\"]\n[BlackElo \"2300\"]\n\n1. e4 *\n\n\
               [Event \"2\"]\n[White \"A\"]\n[Black \"B\"]\n[WhiteElo \"2600\"]\n\n1. d4 *\n\n\
               [Event \"3\"]\n[White \"B\"]\n[Black \"A\"]\n[WhiteElo \"2400\"]\n[BlackElo \"2450\"]\n\n1. c4 *\n\n\
               [Event \"4\"]\n[White \"C\"]\n[Black \"A\"]\n\n1. Nf3 *\n";
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let mut stats = PlayerStats::new();
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        Some(&mut stats),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    let id = |name: &[u8]| writer.names().find(NameKind::Player, name).unwrap();
    assert_eq!(stats.max_elo(id(b"A")), 2600);
    assert_eq!(stats.max_elo(id(b"B")), 2400);
    assert_eq!(stats.max_elo(id(b"C")), 0, "no Elo anywhere");
    // the same numbers a fresh scan of the reopened database finds
    let ids = [id(b"A"), id(b"B"), id(b"C")];
    drop(writer);
    let db = Si4Database::open(&base).unwrap();
    let fresh = PlayerStats::from_database(&db);
    for id in ids {
        assert_eq!(stats.max_elo(id), fresh.max_elo(id), "player {id}");
    }
}

#[test]
fn player_stats_are_not_changed_by_games_that_were_skipped() {
    let big = "x".repeat(200_000);
    let pgn = format!(
        "[Event \"a\"]\n[White \"Skipped\"]\n[WhiteElo \"3000\"]\n\n1. e4 {{{big}}} *\n\n\
         [Event \"b\"]\n[White \"Kept\"]\n[WhiteElo \"2000\"]\n\n1. d4 *\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let mut writer = Si4Writer::create(&dir.path().join("db"), 0).unwrap();
    let mut stats = PlayerStats::new();
    let r = import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        Some(&mut stats),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(r.games_added, 1);
    let kept = writer.names().find(NameKind::Player, b"Kept").unwrap();
    assert_eq!(stats.max_elo(kept), 2000);
    assert!(
        writer.names().find(NameKind::Player, b"Skipped").is_none(),
        "a skipped game must not add its names"
    );
}

// ====================================================================
// Unsupported variants (D382)
// ====================================================================

#[test]
fn unsupported_variant_game_is_skipped_instead_of_stored_empty() {
    let pgn = "[Event \"v\"]\n[Variant \"Crazyhouse\"]\n\n1. e4 e5 2. Nf3 *\n\n[Event \"n\"]\n\n1. d4 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert_eq!(imp.report.games_read, 2);
    // whichever way the reader classifies the variant, no empty ghost game
    let mut db = imp.open();
    for n in 1..=db.len() {
        let g = db.game(n).unwrap();
        if g.tags().event == "v" {
            assert!(
                g.mainline().count() > 0,
                "variant game stored without its moves"
            );
        }
    }
    if imp
        .report
        .games_skipped
        .iter()
        .any(|(_, r)| *r == SkipReason::UnsupportedVariant)
    {
        assert_eq!(imp.report.games_added, 1);
        assert_eq!(db.game(1).unwrap().tags().event, "n");
    }
}

// ====================================================================
// Canonical content survives (comments, variations, NAGs)
// ====================================================================

#[test]
fn comments_variations_and_nags_survive_the_import() {
    let pgn = "[Event \"x\"]\n[Result \"*\"]\n\n{start} 1. e4 $1 {best} e5 (1... c5 2. Nf3 $14) 2. Nf3 Nc6 *\n";
    let imp = import_str(pgn, &ImportOptions::default());
    assert!(!imp.report.has_warnings(), "{}", imp.report);
    let mut db = imp.open();
    let text = canon(&db.game(1).unwrap());
    for needle in [
        "{start}",
        "$1",
        "{best}",
        "(1... c5 2. Nf3 $14)",
        "2. Nf3 Nc6",
    ] {
        assert!(text.contains(needle), "{needle:?} missing in\n{text}");
    }
}

#[test]
fn game_from_a_fen_start_keeps_its_position() {
    let pgn = "[Event \"x\"]\n[SetUp \"1\"]\n[FEN \"7k/5Q2/8/8/8/8/8/1K6 b - - 0 1\"]\n\n*\n";
    let imp = import_str(pgn, &ImportOptions::default());
    let mut db = imp.open();
    assert_eq!(imp.report.games_added, 1);
    let game = db.game(1).unwrap();
    assert_eq!(game.mainline().count(), 0);
    assert!(canon(&game).contains("7k/5Q2/8/8/8/8/8/1K6 b - - 0 1"));
    assert!(db.entry(1).unwrap().flags.contains(GameFlags::START));
}

// ====================================================================
// .pgn.gz (rule 43, D353): detected by content, progress in compressed bytes
// ====================================================================

#[test]
fn gz_file_is_detected_by_content_not_by_extension() {
    let dir = tempfile::tempdir().unwrap();
    // deliberately named .pgn
    let path = dir.path().join("looks_plain.pgn");
    let gz = gzip_stored(THREE.as_bytes());
    std::fs::write(&path, &gz).unwrap();
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    let mut last = (0, 0);
    let mut sink = |d: u64, t: u64| last = (d, t);
    let r = import_pgn_file(
        &path,
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    drop(writer);
    assert_eq!(r.games_added, 3, "{r}");
    // total and progress are compressed bytes, as BytesUsed() in the Scid
    let size = gz.len() as u64;
    assert_eq!(last, (size, size));
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.game(2).unwrap().tags().event, "Two");
}

#[test]
fn gz_and_plain_imports_give_identical_database_files() {
    let plain = import_str(THREE, &ImportOptions::default());
    let gz = import_bytes(&gzip_stored(THREE.as_bytes()), &ImportOptions::default());
    for ext in ["si4", "sg4"] {
        assert_eq!(
            std::fs::read(plain.base.with_extension(ext)).unwrap(),
            std::fs::read(gz.base.with_extension(ext)).unwrap(),
            ".{ext}"
        );
    }
}

#[test]
fn concatenated_gzip_members_are_read_in_sequence() {
    let mut data = gzip_stored(&THREE.as_bytes()[..THREE.find("[Event \"Two\"").unwrap()]);
    data.extend_from_slice(&gzip_stored(
        &THREE.as_bytes()[THREE.find("[Event \"Two\"").unwrap()..],
    ));
    let imp = import_bytes(&data, &ImportOptions::default());
    assert_eq!(imp.report.games_added, 3, "{}", imp.report);
}

#[test]
fn truncated_gzip_stops_with_a_read_error_and_keeps_the_complete_games() {
    let filler = "q".repeat(2000);
    let mut pgn = String::new();
    for i in 0..60 {
        write!(pgn, "[Event \"E{i}\"]\n\n1. e4 {{{filler}}} e5 *\n\n").unwrap();
    }
    let mut gz = gzip_stored(pgn.as_bytes());
    gz.truncate(gz.len() / 2);
    let imp = import_bytes(&gz, &ImportOptions::default());
    let r = &imp.report;
    assert!(
        matches!(r.stopped, Some(ImportStop::Read { .. })),
        "{:?}",
        r.stopped
    );
    assert!(
        r.games_added > 0 && r.games_added < 60,
        "got {}",
        r.games_added
    );
    assert_eq!(imp.open().len(), r.games_added);
}
