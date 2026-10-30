//! M7-1a2, criterion 16: `list_extra_tags`, `filter_by_tag` and
//! `Si4Writer::edit_tags` (`docs/specs/m7-1-manutencao.md`, decision D3521,
//! rules 34 to 37).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use mychess_core::Game;
use mychess_db::si4::{FailPoint, Si4Error, Si4Writer, TagOp};
use mychess_db::{
    CancelToken, Filter, MaintScope, NoProgress, filter_by_tag, games_in_scope, list_extra_tags,
};
use proptest::prelude::*;
use support::{build_games, entries_of, open, plain, record, snapshot};

fn tagged(tags: &[(&str, &str)]) -> Game {
    let mut game = plain();
    for (name, value) in tags {
        game.tags_mut().set_extra(name, value).unwrap();
    }
    game
}

fn add(name: &str, value: &str) -> TagOp {
    TagOp::Add {
        name: name.into(),
        value: value.into(),
    }
}

fn strip(name: &str) -> TagOp {
    TagOp::Strip { name: name.into() }
}

fn run(base: &std::path::Path, games: &[u32], op: &TagOp) -> mychess_db::si4::TagReport {
    let mut writer = Si4Writer::open(base).unwrap();
    writer
        .edit_tags(games, op, &CancelToken::new(), &mut NoProgress)
        .unwrap()
}

fn listing(base: &std::path::Path) -> Vec<(String, u32)> {
    let mut db = open(base);
    list_extra_tags(
        &mut db,
        MaintScope::All,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
    .tags
    .into_iter()
    .map(|t| (t.name, t.games))
    .collect()
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(
        dir.path(),
        &[
            tagged(&[("Annotator", "Zed")]),
            tagged(&[("Annotator", "Yan"), ("PlyCount", "4")]),
            plain(),
            tagged(&[("Annotator", "Zed")]),
        ],
    );
    (dir, base)
}

#[test]
fn lists_tags_with_counts_per_game() {
    let (_dir, base) = fixture();
    assert_eq!(
        listing(&base),
        vec![("Annotator".to_owned(), 3), ("PlyCount".to_owned(), 1)]
    );
}

#[test]
fn the_list_is_ordered_ignoring_case_then_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(
        dir.path(),
        &[tagged(&[
            ("beta", "1"),
            ("Alpha", "1"),
            ("alpha", "2"),
            ("Zulu", "1"),
        ])],
    );
    let names: Vec<String> = listing(&base).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Alpha", "alpha", "beta", "Zulu"]);
}

#[test]
fn an_empty_database_lists_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[]);
    assert_eq!(listing(&base), vec![]);
    let mut db = open(&base);
    let found = filter_by_tag(
        &mut db,
        MaintScope::All,
        "Annotator",
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!((found.matched, found.filter.len()), (0, 0));
}

#[test]
fn listing_respects_the_scope_and_reports_progress() {
    let (_dir, base) = fixture();
    let mut db = open(&base);
    let mut filter = Filter::new(4);
    filter.fill(0);
    filter.set(2, 1);
    let mut seen = Vec::new();
    let mut sink = |d: u64, t: u64| seen.push((d, t));
    let found = list_extra_tags(
        &mut db,
        MaintScope::Filter(&filter),
        &mut sink,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(found.examined, 1);
    assert_eq!(found.tags.len(), 2);
    assert_eq!(seen.last(), Some(&(1, 1)));
    // A filter of the wrong size is refused.
    let wrong = Filter::new(3);
    assert!(
        list_extra_tags(
            &mut db,
            MaintScope::Filter(&wrong),
            &mut NoProgress,
            &CancelToken::new()
        )
        .is_err()
    );
    // A cancelled run says so.
    let token = CancelToken::new();
    token.cancel();
    let cut = list_extra_tags(&mut db, MaintScope::All, &mut NoProgress, &token).unwrap();
    assert!(cut.interrupted || cut.examined == 4);
}

#[test]
fn filtering_by_a_tag_keeps_only_games_of_the_scope_that_have_it() {
    let (_dir, base) = fixture();
    let mut db = open(&base);
    let found = filter_by_tag(
        &mut db,
        MaintScope::All,
        "Annotator",
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(found.filter.iter_games().collect::<Vec<_>>(), [1, 2, 4]);
    assert_eq!(found.matched, 3);

    let mut scope = Filter::new(4);
    scope.fill(0);
    scope.set(2, 1);
    scope.set(3, 1);
    let found = filter_by_tag(
        &mut db,
        MaintScope::Filter(&scope),
        "Annotator",
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(found.filter.iter_games().collect::<Vec<_>>(), [2]);
    // A tag nobody has, and one with different case.
    for tag in ["Nope", "annotator"] {
        let found = filter_by_tag(
            &mut db,
            MaintScope::All,
            tag,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(found.matched, 0, "{tag}");
    }
}

#[test]
fn strip_removes_the_tag_and_leaves_other_games_byte_identical() {
    let (_dir, base) = fixture();
    let before = snapshot(&base);
    let report = run(&base, &[1, 2, 3, 4], &strip("Annotator"));
    assert_eq!(report.changed, 3);
    assert_eq!(report.unchanged, 1);
    let after = snapshot(&base);
    let (b, a) = (entries_of(&before[0]), entries_of(&after[0]));
    assert_eq!(b[2], a[2], "game 3 had no tag: entry untouched");
    let mut db = open(&base);
    assert_eq!(listing(&base), vec![("PlyCount".to_owned(), 1)]);
    // The rest of game 2 survives: its other tag and its moves.
    let game = db.game(2).unwrap();
    assert_eq!(game.tags().get_extra("PlyCount"), Some("4"));
    assert_eq!(game.tags().get_extra("Annotator"), None);
    assert_eq!(game.mainline().count(), 4);
    // Header and names untouched.
    assert_eq!(before[0][..182], after[0][..182]);
    assert_eq!(before[1], after[1]);
    // A game that was not rewritten keeps its record bytes.
    let old_record_3 = {
        let dir = tempfile::tempdir().unwrap();
        let again = build_games(dir.path(), &[plain()]);
        record(&mut open(&again), 1)
    };
    assert_eq!(record(&mut db, 3), old_record_3);
}

#[test]
fn stripping_an_absent_tag_or_in_other_games_changes_nothing() {
    let (_dir, base) = fixture();
    let before = snapshot(&base);
    assert_eq!(run(&base, &[1, 2, 3, 4], &strip("Nope")).changed, 0);
    assert_eq!(run(&base, &[3], &strip("Annotator")).changed, 0);
    // Different case is a different tag.
    assert_eq!(run(&base, &[1, 2, 3, 4], &strip("annotator")).changed, 0);
    assert_eq!(snapshot(&base), before);
}

#[test]
fn add_sets_the_tag_only_in_the_listed_games_and_replaces_an_existing_one() {
    let (_dir, base) = fixture();
    let report = run(&base, &[1, 3], &add("Source", "unit"));
    assert_eq!(report.changed, 2);
    let mut db = open(&base);
    for (n, expected) in [(1, Some("unit")), (2, None), (3, Some("unit")), (4, None)] {
        assert_eq!(
            db.game(n).unwrap().tags().get_extra("Source"),
            expected,
            "game {n}"
        );
    }
    drop(db);
    // The same value again: nothing to do. A new value: replaced in place,
    // keeping the order of the tags of the game.
    assert_eq!(run(&base, &[1, 3], &add("Source", "unit")).changed, 0);
    assert_eq!(run(&base, &[1], &add("Annotator", "New")).changed, 1);
    let mut db = open(&base);
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().get_extra("Annotator"), Some("New"));
    let names: Vec<&str> = game
        .tags()
        .extras()
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(names, ["Annotator", "Source"]);
}

#[test]
fn invalid_names_and_values_are_refused_without_writing() {
    let (_dir, base) = fixture();
    let before = snapshot(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let long = "x".repeat(241);
    let cases = [
        add("a b", "v"),
        add("ECO", "v"),
        add("eco", "v"),
        add("", "v"),
        add("é", "v"),
        add("Fen", "v"),
        add("Okay", &long),
        add("Okay", "line\nbreak"),
        add(&"n".repeat(241), "v"),
        strip(""),
    ];
    for op in &cases {
        let error = writer
            .edit_tags(&[1, 2], op, &CancelToken::new(), &mut NoProgress)
            .unwrap_err();
        assert!(matches!(error, Si4Error::InvalidTag { .. }), "{op:?}");
    }
    drop(writer);
    assert_eq!(snapshot(&base), before);
    // The largest valid value is accepted.
    assert_eq!(run(&base, &[1], &add("Okay", &"x".repeat(240))).changed, 1);
}

#[test]
fn a_game_with_forty_tags_cannot_get_a_new_one_but_may_replace() {
    let dir = tempfile::tempdir().unwrap();
    let names: Vec<String> = (0..40).map(|i| format!("T{i}")).collect();
    let pairs: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "v")).collect();
    let base = build_games(dir.path(), &[tagged(&pairs), plain()]);
    let report = run(&base, &[1, 2], &add("Extra", "x"));
    assert_eq!((report.changed, report.skipped), (1, 1));
    let mut db = open(&base);
    assert_eq!(db.game(1).unwrap().tags().get_extra("Extra"), None);
    assert_eq!(db.game(2).unwrap().tags().get_extra("Extra"), Some("x"));
    drop(db);
    let report = run(&base, &[1], &add("T7", "other"));
    assert_eq!((report.changed, report.skipped), (1, 0));
}

#[test]
fn a_record_that_would_pass_the_limit_is_skipped_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let mut big = plain();
    let first = big.mainline().next().unwrap();
    big.set_comment(first, &"x".repeat(130_950)).unwrap();
    let base = build_games(dir.path(), &[big, plain()]);
    let before = snapshot(&base);
    let report = run(&base, &[1, 2], &add("Source", &"y".repeat(240)));
    assert_eq!((report.changed, report.skipped), (1, 1));
    let after = snapshot(&base);
    assert_eq!(entries_of(&before[0])[0], entries_of(&after[0])[0]);
}

#[test]
fn cancelling_and_failing_leave_the_database_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<Game> = (0..600).map(|_| tagged(&[("Annotator", "a")])).collect();
    let base = build_games(dir.path(), &games);
    let before = snapshot(&base);
    let all: Vec<u32> = (1..=600).collect();

    let token = CancelToken::new();
    let trigger = token.clone();
    let mut calls = 0;
    let mut sink = move |_: u64, _: u64| {
        calls += 1;
        if calls == 2 {
            trigger.cancel();
        }
    };
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .edit_tags(&all, &strip("Annotator"), &token, &mut sink)
        .unwrap();
    assert!(report.interrupted);
    assert_eq!(report.changed, 0);
    drop(writer);
    assert_eq!(snapshot(&base), before);

    for label in ["rewrite:appended", "rewrite:entries"] {
        let mut writer = Si4Writer::open(&base).unwrap();
        writer.set_fail_point(FailPoint::at_label(label));
        let error = writer
            .edit_tags(
                &all,
                &strip("Annotator"),
                &CancelToken::new(),
                &mut NoProgress,
            )
            .unwrap_err();
        assert!(matches!(error, Si4Error::InjectedFailure { .. }));
        drop(writer);
        assert_eq!(snapshot(&base), before, "{label}");
    }
}

#[test]
fn games_in_scope_lists_the_numbers_in_order() {
    assert_eq!(games_in_scope(MaintScope::All, 3).unwrap(), [1, 2, 3]);
    assert_eq!(
        games_in_scope(MaintScope::All, 0).unwrap(),
        Vec::<u32>::new()
    );
    let mut filter = Filter::new(5);
    filter.fill(0);
    filter.set(4, 1);
    filter.set(2, 1);
    assert_eq!(
        games_in_scope(MaintScope::Filter(&filter), 5).unwrap(),
        [2, 4]
    );
    assert!(games_in_scope(MaintScope::Filter(&filter), 6).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// After a strip the tag is not listed any more, every other tag keeps its
    /// count, and a second strip is a no-op.
    #[test]
    fn strip_removes_exactly_one_tag(
        sets in prop::collection::vec(prop::collection::vec(0usize..4, 0..4), 0..8),
        victim in 0usize..4,
    ) {
        const NAMES: [&str; 4] = ["Alpha", "Beta", "Gamma", "alpha"];
        let dir = tempfile::tempdir().unwrap();
        let games: Vec<Game> = sets
            .iter()
            .map(|s| {
                let mut seen = std::collections::BTreeSet::new();
                let pairs: Vec<(&str, &str)> = s
                    .iter()
                    .filter(|i| seen.insert(**i))
                    .map(|i| (NAMES[*i], "v"))
                    .collect();
                tagged(&pairs)
            })
            .collect();
        let base = build_games(dir.path(), &games);
        let before = listing(&base);
        let count = u32::try_from(games.len()).unwrap();
        let all: Vec<u32> = (1..=count).collect();
        let report = run(&base, &all, &strip(NAMES[victim]));
        let after = listing(&base);
        prop_assert!(after.iter().all(|(n, _)| n != NAMES[victim]));
        let expected: Vec<_> = before.iter().filter(|(n, _)| n != NAMES[victim]).cloned().collect();
        prop_assert_eq!(&after, &expected);
        let removed = before.iter().find(|(n, _)| n == NAMES[victim]).map_or(0, |(_, c)| *c);
        prop_assert_eq!(report.changed, removed);
        let snap = snapshot(&base);
        prop_assert_eq!(run(&base, &all, &strip(NAMES[victim])).changed, 0);
        prop_assert_eq!(snapshot(&base), snap);
    }
}
