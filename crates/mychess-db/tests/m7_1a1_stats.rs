//! M7-1a1, criteria 1 to 3: `database_stats`, `name_usage` and
//! `compaction_stats` (`docs/specs/m7-1-manutencao.md`).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a1_support.rs"]
mod support;

use mychess_db::maintenance::{compaction_stats, database_stats, name_usage};
use mychess_db::si4::write::plan_append;
use mychess_db::si4::{FlagOp, GameFlags, NameBase, NameBuilder, NameChange, NameKind, Si4Writer};
use mychess_db::{CancelToken, NoProgress};
use proptest::prelude::*;
use support::{S, build, open, s, six_games};

fn stats_of(games: &[S]) -> mychess_db::DatabaseStats {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), games);
    database_stats(&open(&base), &mut NoProgress, &CancelToken::new())
}

#[test]
fn six_game_database_matches_the_specification() {
    let stats = stats_of(&six_games());
    assert_eq!(stats.games, 6);
    assert_eq!(stats.flag_count(GameFlags::DELETE), 0);
    assert_eq!(stats.results, [2, 2, 1, 1]);
    assert_eq!(stats.years, Some((2020, 2020, 2020)));
    // 2500 + 2400 + 2400 + 2510 + 2520 + 2410 over six values.
    assert_eq!(stats.ratings, Some((2400, 2520, 2456)));
    assert_eq!(stats.percent(3), 50);
    assert!(!stats.interrupted);
}

#[test]
fn empty_database_has_no_ranges_and_zero_percent() {
    let stats = stats_of(&[]);
    assert_eq!(stats.games, 0);
    assert_eq!((stats.years, stats.ratings), (None, None));
    assert_eq!(stats.percent(5), 0);
    assert_eq!(stats.results, [0; 4]);
}

#[test]
fn single_game_database_has_equal_min_max_and_mean() {
    let stats = stats_of(&[s(&["e2e4"]).elo(2000, 0).date("1999.03.04")]);
    assert_eq!(stats.years, Some((1999, 1999, 1999)));
    assert_eq!(stats.ratings, Some((2000, 2000, 2000)));
}

#[test]
fn games_without_a_date_do_not_enter_the_year_range() {
    let stats = stats_of(&[
        s(&["e2e4"]).date("????.??.??"),
        s(&["e2e4"]).date("2000.??.??"),
        s(&["e2e4"]).date("2010.??.??"),
    ]);
    assert_eq!(stats.years, Some((2000, 2010, 2005)));
}

#[test]
fn deleted_and_custom_flags_are_counted_per_bit() {
    let stats = stats_of(&[
        s(&["e2e4"]).flags(GameFlags::DELETE | GameFlags::CUSTOM6),
        s(&["e2e4"]).flags(GameFlags::CUSTOM6 | GameFlags::TACTICS),
        s(&["e2e4"]),
    ]);
    assert_eq!(stats.flag_count(GameFlags::DELETE), 1);
    assert_eq!(stats.flag_count(GameFlags::CUSTOM6), 2);
    assert_eq!(stats.flag_count(GameFlags::TACTICS), 1);
    assert_eq!(stats.flag_count(GameFlags::CUSTOM1), 0);
    assert_eq!(stats.flag_count(GameFlags::DELETE | GameFlags::TACTICS), 0);
}

#[test]
fn a_player_name_nobody_uses_is_counted_as_unused() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    assert_eq!(name_usage(&open(&base))[0].unused, 0);
    // Renaming every "Delta, D" to an existing name leaves the old one unused.
    let mut writer = Si4Writer::open(&base).unwrap();
    writer
        .apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Delta, D", "Alfa, A")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    drop(writer);
    let usage = name_usage(&open(&base));
    assert_eq!(usage[0].total, 4);
    assert_eq!(usage[0].unused, 1);
    let stats = database_stats(&open(&base), &mut NoProgress, &CancelToken::new());
    assert_eq!(stats.names, usage);
}

#[test]
fn unused_names_come_from_the_index_not_from_the_stored_frequencies() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let mut writer = Si4Writer::open(&base).unwrap();
    writer
        .apply_name_changes(
            NameKind::Player,
            &[NameChange::new("Delta, D", "Alfa, A")],
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    drop(writer);
    let honest = name_usage(&open(&base));
    assert_eq!(honest[0].unused, 1);

    // Make every stored frequency lie: each name claims 7 more uses, so the
    // unused name looks used in the `.sn4`.
    let path = base.with_extension("sn4");
    let parsed = NameBase::parse(&std::fs::read(&path).unwrap()).unwrap();
    let mut names = NameBuilder::from_name_base(&parsed);
    for kind in NameKind::ALL {
        for id in 0..names.num_names(kind) {
            names.inc_frequency(kind, id, 7);
        }
    }
    std::fs::write(&path, names.to_bytes()).unwrap();
    let db = open(&base);
    assert!(
        (0..db.names().num_names(NameKind::Player)).all(|id| db
            .names()
            .frequency(NameKind::Player, id)
            .unwrap_or(0)
            >= 7),
        "the fixture must really carry the wrong frequencies"
    );
    assert_eq!(name_usage(&db), honest);
    let stats = database_stats(&db, &mut NoProgress, &CancelToken::new());
    assert_eq!(stats.names, honest);
}

#[test]
fn compaction_stats_follow_the_block_rule_after_a_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games()[..5]);
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    drop(writer);
    let db = open(&base);
    let stats = compaction_stats(&db);
    assert_eq!(
        (stats.games, stats.kept_games, stats.deleted_games),
        (5, 4, 1)
    );
    // The oracle replays the layout with `plan_append`.
    let mut end = 0u64;
    for n in [1, 3, 4, 5] {
        let length = db.entry(n).unwrap().length;
        let (offset, _) = plan_append(end, length as usize);
        end = offset + u64::from(length);
    }
    assert_eq!(stats.compacted_bytes, end);
    assert_eq!(stats.current_bytes, db.games_file_len());
    assert!(!stats.is_compact());
}

#[test]
fn a_database_without_deletions_or_gaps_is_already_compact() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let stats = compaction_stats(&open(&base));
    assert_eq!(stats.deleted_games, 0);
    assert_eq!(stats.current_bytes, stats.compacted_bytes);
    assert!(stats.is_compact());
}

#[test]
fn cancelling_returns_a_flagged_partial_result() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &six_games());
    let cancel = CancelToken::new();
    cancel.cancel();
    let stats = database_stats(&open(&base), &mut NoProgress, &cancel);
    // The first tick comes after 4096 entries or 100 ms: a tiny database may
    // finish before it sees the token, and then it is complete.
    assert!(stats.interrupted || stats.games == 6);
    assert!(stats.results.iter().sum::<u32>() <= 6);
}

#[test]
#[ignore = "builds 100,000 games: run with --ignored"]
fn hundred_thousand_games_are_counted_in_one_pass() {
    let dir = tempfile::tempdir().unwrap();
    let game = s(&["e2e4"]);
    let games: Vec<S> = (0..100_000).map(|_| game.clone()).collect();
    let base = build(dir.path(), &games);
    let mut last = 0;
    let mut sink = |done: u64, total: u64| {
        assert!(done >= last && done <= total, "progress {done}/{total}");
        last = done;
    };
    let stats = database_stats(&open(&base), &mut sink, &CancelToken::new());
    assert_eq!(stats.games, 100_000);
    assert_eq!(stats.results[0], 100_000);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn counters_add_up_to_the_total(
        specs in proptest::collection::vec((0usize..4, 0u16..3000, any::<bool>(), 0usize..3), 0..14)
    ) {
        let results = [
            mychess_core::GameResult::White,
            mychess_core::GameResult::Black,
            mychess_core::GameResult::Draw,
            mychess_core::GameResult::Unknown,
        ];
        let games: Vec<S> = specs
            .iter()
            .map(|&(r, elo, deleted, w)| {
                let g = s(&["e2e4"])
                    .result(results[r])
                    .elo(elo, 0)
                    .players(["Alfa, A", "Beta, B", "Gama, G"][w], "Delta, D");
                if deleted { g.flags(GameFlags::DELETE) } else { g }
            })
            .collect();
        let stats = stats_of(&games);
        prop_assert_eq!(stats.games as usize, games.len());
        prop_assert_eq!(stats.results.iter().sum::<u32>() as usize, games.len());
        prop_assert_eq!(
            stats.flag_count(GameFlags::DELETE) as usize,
            specs.iter().filter(|s| s.2).count()
        );
        let compaction_ok = stats.compacted_bytes <= stats.games_file_bytes;
        prop_assert!(compaction_ok);
        for usage in stats.names {
            prop_assert!(usage.unused <= usage.total);
        }
    }
}

#[test]
fn cancelling_from_the_progress_sink_flags_a_partial_count() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<S> = (0..9000).map(|_| s(&["e2e4"])).collect();
    let base = build(dir.path(), &games);
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut sink = |_: u64, _: u64| token.cancel();
    let stats = database_stats(&open(&base), &mut sink, &cancel);
    assert!(stats.interrupted);
    assert_eq!(stats.games, 9000);
    let counted = stats.results.iter().sum::<u32>();
    assert!(counted > 0 && counted < 9000, "{counted}");
}
