//! M6-4b: the lines of the statistics window (spec 2.2, rules S1 to S5,
//! criterion 12). Expected values come from `sc_filter stats` and
//! `::windows::stats::Refresh` of the Scid, worked out by hand.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cloned_ref_to_slice_refs,
    clippy::needless_pass_by_value
)]

#[path = "common/m6_4bc_support.rs"]
mod support;

use mychess_db::{
    CancelToken, Filter, FilterStatsRow, GameSource, NoProgress, StatsConfig, StatsError,
    StatsQuery, filter_stats, filter_stats_rows, filter_stats_table,
};
use pretty_assertions::assert_eq;
use support::{all, g, only, open};

fn stats(db: &impl GameSource, filter: &Filter, query: StatsQuery) -> FilterStatsRow {
    filter_stats(db, filter, query, &mut NoProgress, &CancelToken::new()).unwrap()
}

#[test]
fn all_counts_results_and_the_score_treats_a_star_as_half_a_point() {
    // 3 wins of White, 2 draws, 1 win of Black, 2 unfinished.
    let mut games = Vec::new();
    for _ in 0..3 {
        games.push(g("A", "B", "1-0"));
    }
    for _ in 0..2 {
        games.push(g("A", "B", "1/2-1/2"));
    }
    games.push(g("A", "B", "0-1"));
    for _ in 0..2 {
        games.push(g("A", "B", "*"));
    }
    let db = open(&games);
    let row = stats(&db, &all(&db), StatsQuery::All);
    assert_eq!(
        row,
        FilterStatsRow {
            total: 8,
            white: 3,
            draw: 2,
            black: 1,
            none: 2,
            skipped_entries: 0
        }
    );
    // (2*3 + 2 + 2) * 500 / 8 = 625 tenths = 62.5 %.
    assert_eq!(row.score_tenths(), 625);
}

#[test]
fn an_empty_filter_gives_zero_and_zero_percent() {
    let db = open(&[g("A", "B", "1-0")]);
    let row = stats(&db, &only(&db, &[]), StatsQuery::All);
    assert_eq!(row, FilterStatsRow::default());
    assert_eq!(row.score_tenths(), 0);
}

#[test]
fn only_the_games_in_the_filter_count() {
    let db = open(&[
        g("A", "B", "1-0"),
        g("A", "B", "0-1"),
        g("A", "B", "1/2-1/2"),
    ]);
    let row = stats(&db, &only(&db, &[2, 3]), StatsQuery::All);
    assert_eq!((row.total, row.white, row.draw, row.black), (2, 0, 1, 1));
}

#[test]
fn both_rated_needs_both_ratings_at_or_above_the_threshold() {
    let db = open(&[
        g("A", "B", "1-0").elo(2400, 2400),
        g("A", "B", "1-0").elo(2400, 2399),
        g("A", "B", "1-0").elo(2399, 2500),
        g("A", "B", "0-1").elo(2400, 0),
        g("A", "B", "0-1").elo(0, 0),
        g("A", "B", "1/2-1/2").elo(2650, 2700),
    ]);
    let filter = all(&db);
    assert_eq!(stats(&db, &filter, StatsQuery::BothRated(2400)).total, 2);
    assert_eq!(stats(&db, &filter, StatsQuery::BothRated(2600)).total, 1);
    // Rating 0 (none) never passes for a threshold above 0.
    assert_eq!(stats(&db, &filter, StatsQuery::BothRated(1)).total, 4);
    assert_eq!(stats(&db, &filter, StatsQuery::BothRated(0)).total, 6);
}

#[test]
fn year_intervals_are_closed_below_and_open_above() {
    let db = open(&[
        g("A", "B", "1-0").date("1999.12.31"),
        g("A", "B", "1-0").date("2000.01.01"),
        g("A", "B", "0-1").date("2004.12.31"),
        g("A", "B", "0-1").date("2005.01.01"),
        g("A", "B", "0-1").date("2005.??.??"),
        g("A", "B", "1/2-1/2").date("????.??.??"),
        g("A", "B", "1/2-1/2").date("2040.06.01"),
    ]);
    let filter = all(&db);
    let years = |from, to| stats(&db, &filter, StatsQuery::Years { from, to }).total;
    assert_eq!(years(2000, Some(2005)), 2);
    assert_eq!(years(2005, None), 3); // 2005, 2005.??.?? and 2040
    assert_eq!(years(2000, None), 5);
    // A game without a year only enters when the interval starts at 0.
    assert_eq!(years(0, Some(2000)), 2);
    let since = |from| stats(&db, &filter, StatsQuery::Since(from)).total;
    assert_eq!(since(2000), 5);
    assert_eq!(since(2005), 3);
    assert_eq!(since(0), 7);
    assert_eq!(since(1900), 6);
}

#[test]
fn several_queries_come_from_one_pass_and_match_the_single_ones() {
    let db = open(&[
        g("A", "B", "1-0").elo(2500, 2500).date("2011.01.01"),
        g("A", "B", "0-1").elo(2100, 2500).date("2001.01.01"),
        g("A", "B", "1/2-1/2").date("1995.01.01"),
    ]);
    let filter = all(&db);
    let queries = [
        StatsQuery::All,
        StatsQuery::BothRated(2400),
        StatsQuery::Since(2000),
    ];
    let rows =
        filter_stats_rows(&db, &filter, &queries, &mut NoProgress, &CancelToken::new()).unwrap();
    for (query, row) in queries.iter().zip(&rows) {
        assert_eq!(*row, stats(&db, &filter, *query), "{query:?}");
    }
    assert_eq!(rows.iter().map(|r| r.total).collect::<Vec<_>>(), [3, 1, 2]);
}

#[test]
fn the_table_has_the_default_lines_of_scid() {
    let db = open(&[g("A", "B", "1-0").elo(2650, 2650).date("2012.05.05")]);
    let config = StatsConfig::new(2012);
    let table = filter_stats_table(
        &db,
        &all(&db),
        &config,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    let queries: Vec<StatsQuery> = table.iter().map(|l| l.query).collect();
    assert_eq!(queries, config.queries());
    // all + 4 thresholds + 5 intervals (2000, 2005, 2010, 2011, 2012) + 5 since.
    assert_eq!(queries.len(), 1 + 4 + 5 + 5);
    assert_eq!(table[0].row.total, 1);
    // The game is in every "both rated" line and in the 2012 lines.
    assert!(table[1..5].iter().all(|l| l.row.total == 1));
    let interval_2012 = table
        .iter()
        .find(|l| {
            l.query
                == StatsQuery::Years {
                    from: 2012,
                    to: None,
                }
        })
        .unwrap();
    assert_eq!(interval_2012.row.white, 1);
    let interval_2010 = table
        .iter()
        .find(|l| {
            l.query
                == StatsQuery::Years {
                    from: 2010,
                    to: Some(2011),
                }
        })
        .unwrap();
    assert_eq!(interval_2010.row.total, 0);
}

#[test]
fn a_filter_of_another_size_is_an_error_and_a_cancelled_token_stops() {
    let db = open(&[g("A", "B", "1-0")]);
    let bad = Filter::new(5);
    assert_eq!(
        filter_stats(
            &db,
            &bad,
            StatsQuery::All,
            &mut NoProgress,
            &CancelToken::new()
        ),
        Err(StatsError::FilterSize {
            filter: 5,
            games: 1
        })
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        filter_stats(&db, &all(&db), StatsQuery::All, &mut NoProgress, &cancel),
        Err(StatsError::Cancelled)
    );
}

#[test]
fn the_result_is_the_same_with_one_thread_and_with_many() {
    // 5000 games: more than two tasks of 2048 entries.
    let results = ["1-0", "0-1", "1/2-1/2", "*"];
    let games: Vec<_> = (0..5000u32)
        .map(|i| {
            g("A", "B", results[(i % 4) as usize])
                .elo(1800 + (i % 900) as u16, 2000 + (i % 700) as u16)
                .date(&format!("{}.01.01", 1990 + i % 30))
        })
        .collect();
    let db = open(&games);
    let mut filter = Filter::new(db.len());
    for n in (1..=db.len()).filter(|n| n % 3 != 0) {
        filter.set(n, 1);
    }
    let config = StatsConfig::new(2019);
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                filter_stats_table(&db, &filter, &config, &mut NoProgress, &CancelToken::new())
                    .unwrap()
            })
    };
    let one = run(1);
    assert_eq!(one, run(4));
    assert_eq!(one[0].row.total, filter.count());
}
