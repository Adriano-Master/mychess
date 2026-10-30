//! M6-4b: statistics over a damaged index report the entries they skipped
//! (D3443). The damage is the type-4 bits of the result nibble of the entry,
//! as in `m3_3a_gaps.rs`.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

#[path = "common/m3_3a_support.rs"]
mod support;

use mychess_core::GameResult;
use mychess_db::{
    CancelToken, Filter, FilterGraphConfig, GameSource, GraphKind, NoProgress, RatingOptions,
    RatingPlayer, StatsQuery, filter_histogram, filter_stats, filter_stats_rows, rating_history,
};
use pretty_assertions::assert_eq;
use support::{B, break_entry, build};

fn damaged() -> support::Fixture {
    let fx = build(&[
        B::new("A", "B")
            .result(GameResult::White)
            .date("2000.01.01")
            .elo(2500, 2400)
            .done(),
        B::new("A", "B")
            .result(GameResult::Draw)
            .date("2001.01.01")
            .elo(2510, 2400)
            .done(),
        B::new("A", "B")
            .result(GameResult::Black)
            .date("2002.01.01")
            .elo(2520, 2400)
            .done(),
        B::new("A", "B")
            .result(GameResult::White)
            .date("2003.01.01")
            .elo(2530, 2400)
            .done(),
    ]);
    break_entry(&fx, 2);
    break_entry(&fx, 3);
    fx
}

fn full(db: &impl GameSource) -> Filter {
    let mut f = Filter::new(db.len());
    f.fill(1);
    f
}

#[test]
fn stats_rows_report_the_unreadable_entries_they_skipped() {
    let fx = damaged();
    let db = fx.open();
    let row = filter_stats(
        &db,
        &full(&db),
        StatsQuery::All,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!((row.total, row.white, row.skipped_entries), (2, 2, 2));
    let rows = filter_stats_rows(
        &db,
        &full(&db),
        &[StatsQuery::All, StatsQuery::Since(2003)],
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert!(rows.iter().all(|r| r.skipped_entries == 2), "{rows:?}");
}

#[test]
fn a_healthy_database_skips_nothing() {
    let fx = build(&[B::new("A", "B").done()]);
    let db = fx.open();
    let row = filter_stats(
        &db,
        &full(&db),
        StatsQuery::All,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!((row.total, row.skipped_entries), (1, 0));
}

#[test]
fn the_histogram_and_the_rating_series_report_the_skipped_entries() {
    let fx = damaged();
    let db = fx.open();
    let config = FilterGraphConfig::new(2026);
    let h = filter_histogram(
        &db,
        &full(&db),
        GraphKind::Decade,
        &config,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(h.unreadable, 2);
    let s = rating_history(
        &db,
        &RatingPlayer::Name("A".into()),
        RatingOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(s.skipped, 2);
    assert_eq!(s.points.len(), 2);
}
