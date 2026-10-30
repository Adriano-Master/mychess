//! M7-1a1, criteria 12 and 13: `diff_games`, `share_tags_preview` and
//! `Si4Writer::share_tags` (`docs/specs/m7-1-manutencao.md`).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a1_support.rs"]
mod support;

use mychess_db::maintenance::{
    GameDiff, MAX_DIFF_TOKENS, SharedField, SharedTag, diff_games, share_tags_preview,
};
use mychess_db::si4::Si4Writer;
use pretty_assertions::assert_eq;
use support::{RUY6, build, entries_of, game_of, index_bytes, open, s};

fn compared(
    diff: GameDiff,
) -> (
    Vec<mychess_db::maintenance::DiffToken>,
    Vec<mychess_db::maintenance::DiffToken>,
    Vec<mychess_db::maintenance::DiffToken>,
    Vec<mychess_db::maintenance::DiffToken>,
) {
    match diff {
        GameDiff::Compared {
            header_a,
            header_b,
            moves_a,
            moves_b,
        } => (header_a, header_b, moves_a, moves_b),
        GameDiff::TooLarge { .. } => panic!("unexpectedly too large"),
    }
}

fn marked(tokens: &[mychess_db::maintenance::DiffToken]) -> Vec<&str> {
    tokens
        .iter()
        .filter(|t| t.differs)
        .map(|t| t.text.as_str())
        .collect()
}

fn unmarked(tokens: &[mychess_db::maintenance::DiffToken]) -> Vec<&str> {
    tokens
        .iter()
        .filter(|t| !t.differs)
        .map(|t| t.text.as_str())
        .collect()
}

#[test]
fn identical_games_have_no_mark() {
    let a = game_of(&s(&RUY6).commented().with_variation());
    let diff = diff_games(&a, &a.clone()).unwrap();
    assert!(!diff.has_differences());
    let (ha, hb, ma, mb) = compared(diff);
    assert!(marked(&ha).is_empty() && marked(&hb).is_empty());
    assert!(marked(&ma).is_empty() && marked(&mb).is_empty());
    assert!(!ma.is_empty() && !ha.is_empty());
}

#[test]
fn a_comment_and_a_variation_are_the_only_marks() {
    let a = game_of(&s(&RUY6).commented());
    let b = game_of(&s(&RUY6).with_variation());
    let diff = diff_games(&a, &b).unwrap();
    assert!(diff.has_differences());
    let (ha, hb, ma, mb) = compared(diff);
    assert!(
        marked(&ha).is_empty() && marked(&hb).is_empty(),
        "same headers"
    );
    assert_eq!(marked(&ma), ["{a comment}"]);
    let variation = marked(&mb);
    assert_eq!(variation.first(), Some(&"("));
    assert_eq!(variation.last(), Some(&")"));
    assert!(variation.len() >= 3, "{variation:?}");
    assert_eq!(unmarked(&ma), unmarked(&mb), "the main line is common");
}

#[test]
fn a_header_difference_marks_the_line_on_both_sides() {
    let a = game_of(&s(&RUY6));
    let b = game_of(&s(&RUY6).event("Other"));
    let (ha, hb, ma, mb) = compared(diff_games(&a, &b).unwrap());
    assert_eq!(marked(&ha), ["[Event \"Aberto\"]"]);
    assert_eq!(marked(&hb), ["[Event \"Other\"]"]);
    assert!(marked(&ma).is_empty() && marked(&mb).is_empty());
}

#[test]
fn a_game_over_the_token_limit_is_too_large() {
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let long: Vec<&'static str> = cycle
        .iter()
        .copied()
        .cycle()
        .take(MAX_DIFF_TOKENS + 400)
        .collect();
    let big = game_of(&s(&long));
    let small = game_of(&s(&RUY6));
    for (x, y) in [(&big, &small), (&small, &big)] {
        match diff_games(x, y).unwrap() {
            GameDiff::TooLarge { tokens_a, tokens_b } => {
                assert!(tokens_a.max(tokens_b) > MAX_DIFF_TOKENS);
            }
            GameDiff::Compared { .. } => panic!("expected TooLarge"),
        }
    }
}

fn tag(game: u32, field: SharedField, old: &str, new: &str) -> SharedTag {
    SharedTag {
        game,
        field,
        old: old.into(),
        new: new.into(),
    }
}

#[test]
fn each_of_the_six_cases_is_proposed_and_written() {
    let dir = tempfile::tempdir().unwrap();
    let rich = s(&RUY6).elo(2500, 2400);
    let poor = s(&RUY6).date("2020.??.??").event("?").round("?").elo(0, 0);
    let base = build(dir.path(), &[poor, rich]);
    let db = open(&base);
    let preview = share_tags_preview(&db, 1, 2);
    assert_eq!(
        preview,
        [
            tag(1, SharedField::Date, "2020", "2020.01.10"),
            tag(1, SharedField::Event, "?", "Aberto"),
            tag(1, SharedField::Round, "?", "1"),
            tag(1, SharedField::WhiteElo, "0", "2500"),
            tag(1, SharedField::BlackElo, "0", "2400"),
        ]
    );
    // Same list with the games swapped: the changes go to game 2.
    let swapped = share_tags_preview(&db, 2, 1);
    assert_eq!(swapped.len(), 5);
    assert!(swapped.iter().all(|t| t.game == 1));
    drop(db);

    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let done = writer.share_tags(1, 2).unwrap();
    assert_eq!(done, preview);
    drop(writer);
    let after = index_bytes(&base);
    assert_eq!(
        entries_of(&before)[1],
        entries_of(&after)[1],
        "game 2 untouched"
    );
    assert_eq!(before[..182], after[..182]);

    let mut db = open(&base);
    let g = db.game(1).unwrap();
    assert_eq!(g.tags().date.to_string(), "2020.01.10");
    assert_eq!(g.tags().event, "Aberto");
    assert_eq!(g.tags().round, "1");
    assert_eq!(g.tags().white_rating.unwrap().value.get(), 2500);
    assert_eq!(g.tags().black_rating.unwrap().value.get(), 2400);
    assert!(
        share_tags_preview(&db, 1, 2).is_empty(),
        "nothing left to share"
    );
}

#[test]
fn changes_can_flow_to_both_games() {
    let dir = tempfile::tempdir().unwrap();
    let a = s(&RUY6).event("?").elo(2500, 0);
    let b = s(&RUY6).round("?").elo(0, 2400);
    let base = build(dir.path(), &[a, b]);
    let mut writer = Si4Writer::open(&base).unwrap();
    let done = writer.share_tags(1, 2).unwrap();
    let games: Vec<u32> = done.iter().map(|t| t.game).collect();
    assert!(games.contains(&1) && games.contains(&2), "{done:?}");
    drop(writer);
    let mut db = open(&base);
    for n in [1, 2] {
        let g = db.game(n).unwrap();
        assert_eq!(g.tags().event, "Aberto");
        assert_eq!(g.tags().round, "1");
        assert_eq!(g.tags().white_rating.unwrap().value.get(), 2500);
        assert_eq!(g.tags().black_rating.unwrap().value.get(), 2400);
    }
}

#[test]
fn nothing_to_share_or_bad_numbers_give_an_empty_list_and_no_write() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[s(&RUY6), s(&RUY6)]);
    let before = index_bytes(&base);
    let db = open(&base);
    for (a, b) in [(1, 2), (1, 1), (0, 2), (1, 0), (1, 3), (9, 1)] {
        assert!(share_tags_preview(&db, a, b).is_empty(), "{a},{b}");
    }
    drop(db);
    let mut writer = Si4Writer::open(&base).unwrap();
    for (a, b) in [(1, 2), (2, 2), (0, 1), (1, 7)] {
        assert!(writer.share_tags(a, b).unwrap().is_empty(), "{a},{b}");
    }
    drop(writer);
    assert_eq!(index_bytes(&base), before);
}

#[test]
fn a_date_that_is_not_a_prefix_is_not_shared() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(
        dir.path(),
        &[s(&RUY6).date("2020.??.??"), s(&RUY6).date("2019.05.01")],
    );
    let db = open(&base);
    assert_eq!(share_tags_preview(&db, 1, 2), Vec::<SharedTag>::new());
}

/// Offset of the entry of game `n` (base 1) in the `.si4` file.
fn entry_offset(n: usize) -> usize {
    182 + 47 * (n - 1)
}

#[test]
fn copying_only_the_event_leaves_the_raw_elo_and_the_corrupt_date_alone() {
    let dir = tempfile::tempdir().unwrap();
    let poor = s(&RUY6).event("?").date("????.??.??").elo(2500, 2400);
    let rich = s(&RUY6).date("????.??.??").elo(2500, 2400);
    let base = build(dir.path(), &[poor, rich]);
    // Game 1: raw Elos above 4000 (a lenient parse reads them as 4000) with
    // rating type 2, and a date word with year 0, month 15 and an event-date field (it
    // reads as an unknown date).
    let index = base.with_extension("si4");
    let mut bytes = std::fs::read(&index).unwrap();
    let at = entry_offset(1);
    bytes[at + 29..at + 31].copy_from_slice(&0x2FFFu16.to_be_bytes());
    bytes[at + 31..at + 33].copy_from_slice(&0x2FFEu16.to_be_bytes());
    let corrupt = (0x123u32 << 20) | (15 << 5) | 31;
    bytes[at + 25..at + 29].copy_from_slice(&corrupt.to_be_bytes());
    std::fs::write(&index, &bytes).unwrap();
    let before = index_bytes(&base);

    let mut writer = Si4Writer::open(&base).unwrap();
    let done = writer.share_tags(1, 2).unwrap();
    assert_eq!(done.len(), 1, "{done:?}");
    assert_eq!(done[0].field, SharedField::Event);
    drop(writer);

    let after = index_bytes(&base);
    let (b, a) = (entries_of(&before), entries_of(&after));
    assert_eq!(b[1], a[1], "game 2 untouched");
    let diff: Vec<usize> = (0..47).filter(|&i| b[0][i] != a[0][i]).collect();
    assert!(
        diff.iter().all(|i| matches!(i, 14..=16)),
        "only the event bytes may change, changed: {diff:?}"
    );
    assert!(!diff.is_empty());
    assert_eq!(
        b[0][25..33],
        a[0][25..33],
        "date and Elo words byte for byte"
    );
}
