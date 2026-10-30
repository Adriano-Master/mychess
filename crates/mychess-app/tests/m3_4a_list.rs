//! Contract tests of the rows of the game list (spec M3-4, D509, D521,
//! columns table; Scid `IndexEntry::PrintGameInfo`, `glistFields`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use m3_4a_common::*;
use mychess_app::GameRow;
use rstest::rstest;

const FULL: &str = "[Event \"Wijk Open\"]\n[Site \"Wijk aan Zee NED\"]\n[Date \"2024.05.17\"]\n\
    [EventDate \"2024.05.01\"]\n[Round \"4.2\"]\n[White \"Alpha\"]\n[Black \"Beta\"]\n\
    [Result \"1-0\"]\n[WhiteElo \"2700\"]\n[BlackElo \"2650\"]\n[ECO \"C44\"]\n\n\
    1. e4 {nice} e5 (1... c5) 2. Nf3 $1 Nc6 3. Bb5 1-0\n\n";

/// FEN start, partial date, no ECO, ends with a lone rook against nothing.
const SECOND: &str = "[Event \"?\"]\n[Site \"Paris\"]\n[Date \"2024.??.??\"]\n[Round \"-\"]\n\
    [White \"Gamma\"]\n[Black \"Delta\"]\n[Result \"*\"]\n[SetUp \"1\"]\n\
    [FEN \"4k3/8/8/8/8/8/4P3/4K2R w K - 0 1\"]\n[ScidFlags \"DT3\"]\n\n1. Rh7 Kd8 *\n\n";

fn pgn_two() -> String {
    format!("{FULL}{SECOND}")
}

fn rows_pgn(dir: &std::path::Path, text: &str) -> (Rig, Vec<GameRow>) {
    let mut rig = Rig::new();
    rig.open(&write(dir, "list.pgn", text));
    let rows = rig.state.bases.list_page(sel(1), 1, 1000);
    (rig, rows)
}

fn rows_si4(dir: &std::path::Path, text: &str) -> (Rig, Vec<GameRow>) {
    let mut rig = Rig::new();
    rig.open(&make_si4(dir, "list", text));
    let rows = rig.state.bases.list_page(sel(1), 1, 1000);
    (rig, rows)
}

#[test]
fn the_first_row_has_the_22_columns_of_the_scid_for_an_si4() {
    let dir = tempfile::tempdir().unwrap();
    let (_rig, rows) = rows_si4(dir.path(), &pgn_two());
    let r = &rows[0];
    assert_eq!(r.number, 1);
    assert_eq!((r.white.as_str(), r.black.as_str()), ("Alpha", "Beta"));
    assert_eq!(r.result, mychess_core::GameResult::White);
    assert_eq!(r.length, 3, "(5 half moves + 1) / 2");
    assert_eq!(r.date, "2024.05.17");
    assert_eq!(r.event, "Wijk Open");
    assert_eq!((r.white_elo, r.black_elo), (2700, 2650));
    assert_eq!(r.round, "4.2");
    assert_eq!(r.site, "Wijk aan Zee NED");
    assert!(!r.deleted);
    assert_eq!((r.variations, r.comments, r.annos), (1, 1, 1));
    assert_eq!(r.eco, "C44");
    assert_eq!(r.opening, "", "Opening is empty in slice 4a");
    assert_eq!(r.flags, "");
    assert!(!r.start);
    assert_eq!(r.country, "NED");
    assert_eq!(r.event_date, "2024.05.01");
    assert_eq!(r.end_material, "qRBN8:qRBN8");
    assert!(!r.unreadable);
}

#[test]
fn the_second_row_covers_start_flags_partial_date_and_unknown_country() {
    let dir = tempfile::tempdir().unwrap();
    let (_rig, rows) = rows_si4(dir.path(), &pgn_two());
    let r = &rows[1];
    assert_eq!(r.number, 2);
    assert_eq!(r.result, mychess_core::GameResult::Unknown);
    assert_eq!(r.length, 1, "2 half moves");
    assert_eq!(r.date, "2024");
    assert!(r.start, "starts from a FEN");
    assert!(r.deleted, "ScidFlags D");
    assert_eq!(r.flags, "T3", "D is not repeated in the Flags column");
    assert_eq!(r.country, "?");
    assert_eq!(r.eco, "");
    assert_eq!(
        r.end_material, "r1:",
        "a lone rook and a pawn against a bare king"
    );
    assert_eq!(r.event_date, "????", "Scid: strTrimDate of an unknown date");
}

#[test]
fn a_pgn_gives_the_same_rows_as_the_si4_made_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_a, pgn) = rows_pgn(dir.path(), &pgn_two());
    let (_b, si4) = rows_si4(dir.path(), &pgn_two());
    assert_eq!(pgn.len(), 2);
    for (p, s) in pgn.iter().zip(&si4) {
        let (mut p, mut s) = (p.clone(), s.clone());
        // Maintenance flags exist only in SI4 (a PGN has no index flags).
        p.flags = String::new();
        s.flags = String::new();
        p.deleted = false;
        s.deleted = false;
        assert_eq!(p, s);
    }
}

#[test]
fn only_the_games_of_the_filter_are_listed_in_database_order() {
    let dir = tempfile::tempdir().unwrap();
    let (rig, _) = rows_pgn(dir.path(), &pgn_two());
    // `first` is a position in the filter, from 1.
    let rows = rig.state.bases.list_page(sel(1), 2, 5);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].number, 2);
}

#[test]
fn first_zero_past_the_end_or_count_zero_give_no_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (rig, _) = rows_pgn(dir.path(), &pgn_two());
    let b = &rig.state.bases;
    assert!(b.list_page(sel(1), 0, 10).is_empty(), "first is 1-based");
    assert!(b.list_page(sel(1), 3, 10).is_empty());
    assert!(b.list_page(sel(1), u32::MAX, 10).is_empty());
    assert!(b.list_page(sel(1), 1, 0).is_empty());
    assert!(b.list_page(sel(5), 1, 10).is_empty(), "slot not open");
    assert_eq!(
        b.list_page(sel(1), 1, u32::MAX).len(),
        2,
        "a huge count is capped"
    );
}

#[test]
fn an_empty_filter_means_an_empty_list() {
    let rig = Rig::new();
    assert!(
        rig.state
            .bases
            .list_page(SlotId::CLIPBASE, 1, 10)
            .is_empty()
    );
    assert_eq!(
        mychess_app::list_page(
            &*rig
                .state
                .bases
                .slot(SlotId::CLIPBASE)
                .unwrap()
                .source
                .lock(),
            &mychess_db::Filter::new(0),
            1,
            10
        )
        .len(),
        0
    );
}

#[rstest]
#[case("", 0)]
#[case("1. e4 *", 1)]
#[case("1. e4 e5 *", 1)]
#[case("1. e4 e5 2. Nf3 *", 2)]
#[case("1. e4 e5 2. Nf3 Nc6 *", 2)]
#[case("1. e4 e5 2. Nf3 Nc6 3. Bb5 *", 3)]
fn length_is_half_moves_plus_one_over_two(#[case] moves: &str, #[case] expected: u16) {
    let dir = tempfile::tempdir().unwrap();
    let pgn = format!("[White \"W\"]\n[Black \"B\"]\n\n{moves}\n");
    for rows in [rows_pgn(dir.path(), &pgn).1, rows_si4(dir.path(), &pgn).1] {
        assert_eq!(rows[0].length, expected, "{moves:?}");
    }
}

#[rstest]
#[case("Wijk aan Zee NED", "NED")]
#[case("Paris", "?")]
#[case("", "?")]
#[case("NED", "?")]
#[case(" NED", "NED")]
#[case("Berlin GER", "GER")]
#[case("Berlin ger", "?")]
#[case("Moscow USSR", "?")]
#[case("Abc DE", "?")]
#[case("Rome ITA ", "?")]
fn country_is_the_last_three_letters_after_a_space(#[case] site: &str, #[case] expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let pgn = format!("[Site \"{site}\"]\n[White \"W\"]\n[Black \"B\"]\n\n1. e4 *\n");
    for rows in [rows_pgn(dir.path(), &pgn).1, rows_si4(dir.path(), &pgn).1] {
        assert_eq!(rows[0].country, expected, "{site:?}");
    }
}

#[rstest]
#[case("2024.05.17", "2024.05.17")]
#[case("2024.05.??", "2024.05")]
#[case("2024.??.??", "2024")]
#[case("2024.??.17", "2024.??.17")]
#[case("????.??.??", "????")]
#[case("????.05.??", "????.05")]
fn dates_drop_unknown_trailing_parts(#[case] date: &str, #[case] expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let pgn = format!(
        "[Date \"{date}\"]\n[EventDate \"{date}\"]\n[White \"W\"]\n[Black \"B\"]\n\n1. e4 *\n"
    );
    for rows in [rows_pgn(dir.path(), &pgn).1, rows_si4(dir.path(), &pgn).1] {
        assert_eq!(rows[0].date, expected, "{date}");
        // Scid cannot store an event date whose year is unknown (the year
        // is a 3-bit offset from the game year): only the year survives.
        let event = if date.starts_with("????") {
            "????"
        } else {
            expected
        };
        assert_eq!(rows[0].event_date, event, "{date}");
    }
}

#[rstest]
#[case("1-0", mychess_core::GameResult::White)]
#[case("0-1", mychess_core::GameResult::Black)]
#[case("1/2-1/2", mychess_core::GameResult::Draw)]
#[case("*", mychess_core::GameResult::Unknown)]
fn the_result_column_follows_the_game(
    #[case] tag: &str,
    #[case] expected: mychess_core::GameResult,
) {
    let dir = tempfile::tempdir().unwrap();
    let pgn = format!("[White \"W\"]\n[Black \"B\"]\n[Result \"{tag}\"]\n\n1. e4 {tag}\n");
    for rows in [rows_pgn(dir.path(), &pgn).1, rows_si4(dir.path(), &pgn).1] {
        assert_eq!(rows[0].result, expected);
    }
}

#[test]
fn the_number_column_is_the_database_number_not_the_position_in_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let (rig, _) = rows_si4(dir.path(), &pgn_two());
    let rows = rig.state.bases.list_page(sel(1), 2, 1);
    assert_eq!(rows[0].number, 2);
}

#[test]
fn a_page_of_a_large_pgn_returns_only_the_requested_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut pgn = String::new();
    for i in 0..300 {
        pgn.push_str(&format!(
            "[Event \"E{i}\"]\n[White \"W{i}\"]\n[Black \"B\"]\n\n1. e4 *\n\n"
        ));
    }
    let (rig, rows) = rows_pgn(dir.path(), &pgn);
    assert_eq!(rows.len(), 300);
    let page = rig.state.bases.list_page(sel(1), 101, 25);
    assert_eq!(page.len(), 25);
    assert_eq!(page[0].number, 101);
    assert_eq!(page[0].white, "W100");
    assert_eq!(page[24].number, 125);
    let tail = rig.state.bases.list_page(sel(1), 295, 25);
    assert_eq!(
        tail.iter().map(|r| r.number).collect::<Vec<_>>(),
        [295, 296, 297, 298, 299, 300]
    );
}

#[test]
fn the_clipbase_rows_have_the_same_columns() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "full.pgn", FULL));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    let from_pgn = rig.state.bases.list_page(sel(1), 1, 1);
    let from_clip = rig.state.bases.list_page(SlotId::CLIPBASE, 1, 1);
    let (mut p, mut c) = (from_pgn[0].clone(), from_clip[0].clone());
    // ECO is not derived by the Clipbase encoder from the tag alone; every
    // other column must agree.
    p.eco = String::new();
    c.eco = String::new();
    assert_eq!(p, c);
}
