//! M6-4b: the histograms of the filter graphs (spec 2.3, rules G1 to G9, the
//! boundary corrections of D3372 and criterion 13). Expected values are
//! worked out from `sc_filter freq` and `graphs.tcl` and from the decisions
//! of the spec, not from the implementation.
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

use mychess_core::PartialDate;
use mychess_db::{
    CancelToken, Filter, FilterGraphConfig, GameSource, GraphKind, Histogram, NoProgress,
    StatsError, filter_histogram,
};
use pretty_assertions::assert_eq;
use support::{G, all, g, only, open};

fn config() -> FilterGraphConfig {
    FilterGraphConfig::new(2012)
}

fn hist(
    db: &impl GameSource,
    filter: &Filter,
    kind: GraphKind,
    config: &FilterGraphConfig,
) -> Histogram {
    filter_histogram(
        db,
        filter,
        kind,
        config,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

/// `(label, filtered, all)` of every bin.
fn rows(h: &Histogram) -> Vec<(String, u32, u32)> {
    h.bins
        .iter()
        .map(|b| (b.label.clone(), b.filtered, b.all))
        .collect()
}

fn find<'a>(rows: &'a [(String, u32, u32)], label: &str) -> &'a (String, u32, u32) {
    rows.iter().find(|r| r.0 == label).unwrap()
}

#[test]
fn decades_are_eleven_bins_and_the_edges_are_exact() {
    let dates = [
        "1919.12.31",
        "1920.01.01",
        "1999.12.31",
        "2000.01.01",
        "2009.12.31",
        "2010.01.01",
        "2020.??.??",
        "2035.05.05",
        "????.??.??",
        "1900.??.??",
    ];
    let games: Vec<G> = dates.iter().map(|d| g("A", "B", "1-0").date(d)).collect();
    let db = open(&games);
    let h = hist(&db, &all(&db), GraphKind::Decade, &config());
    let r = rows(&h);
    let labels: Vec<&str> = r.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(
        labels,
        [
            "-1919", "20-29", "30-39", "40-49", "50-59", "60-69", "70-79", "80-89", "90-99",
            "00-09", "2010+"
        ]
    );
    // -1919: 1919.12.31, unknown date (year 0, rule G1) and 1900.
    assert_eq!(find(&r, "-1919").1, 3);
    assert_eq!(find(&r, "20-29").1, 1); // 1920.01.01
    assert_eq!(find(&r, "90-99").1, 1); // 1999.12.31
    assert_eq!(find(&r, "00-09").1, 2); // 2000.01.01 and 2009.12.31
    // Open last decade (D3372.1): 2010, 2020.??.?? and 2035.
    assert_eq!(find(&r, "2010+").1, 3);
    assert_eq!(h.bins.iter().map(|b| b.filtered).sum::<u32>(), 10);
}

#[test]
fn year_bins_follow_min_max_and_step() {
    let games = [
        g("A", "B", "1-0").date("1994.12.31"),
        g("A", "B", "1-0").date("1995.01.01"),
        g("A", "B", "1-0").date("1996.07.07"),
        g("A", "B", "1-0").date("1997.??.??"),
        g("A", "B", "1-0").date("1998.12.31"),
        g("A", "B", "1-0").date("1999.01.01"),
        g("A", "B", "1-0").date("2000.01.01"),
    ];
    let db = open(&games);
    let mut config = config();
    config.year_min = 1995;
    config.year_max = 1999;
    config.year_step = 2;
    let h = hist(&db, &all(&db), GraphKind::Year, &config);
    // [1995,1996], [1997,1998], [1999,2000]; labels i + step/2.
    assert_eq!(
        rows(&h),
        [
            ("1996".to_owned(), 2, 2),
            ("1998".to_owned(), 2, 2),
            ("2000".to_owned(), 2, 2),
        ]
    );
    config.year_step = 1;
    config.year_max = 1996;
    let h = hist(&db, &all(&db), GraphKind::Year, &config);
    assert_eq!(
        rows(&h),
        [("1995".to_owned(), 1, 1), ("1996".to_owned(), 1, 1)]
    );
}

#[test]
fn a_filter_subset_counts_apart_from_the_whole_base() {
    let games = [
        g("A", "B", "1-0").date("2000.01.01"),
        g("A", "B", "1-0").date("2000.02.02"),
        g("A", "B", "1-0").date("2001.01.01"),
        g("A", "B", "1-0").date("2001.01.02"),
    ];
    let db = open(&games);
    let mut config = config();
    config.year_min = 2000;
    config.year_max = 2001;
    let h = hist(&db, &only(&db, &[1, 3, 4]), GraphKind::Year, &config);
    assert_eq!(
        rows(&h),
        [("2000".to_owned(), 1, 2), ("2001".to_owned(), 2, 2)]
    );
    assert_eq!((h.filter_total, h.all_total), (3, 4));
    // Relative: per mille of the bin; mean = filter * 1000 / base = 750.
    let relative = h.relative();
    assert_eq!(relative.bins[0].value, 500.0);
    assert_eq!(relative.bins[1].value, 999.9);
    assert_eq!(relative.mean, Some(750.0));
    // Absolute: points 1 and 2, mean (1 + 2) / 2 = 1 in integer division.
    let absolute = h.absolute();
    assert_eq!(absolute.bins[1].value, 2.0);
    assert_eq!(absolute.max, 2.0);
    assert_eq!(absolute.mean, Some(1.0));
    assert_eq!(absolute.total, 3);
}

#[test]
fn rating_bins_without_estimate_are_inclusive_at_both_ends() {
    // The lower rating decides: 2399 and 2400 are in different bins and
    // 2199 is in the bin that ends there (D3372.2; Scid loses it).
    let games = [
        g("A", "B", "1-0").elo(2399, 2800),
        g("A", "B", "1-0").elo(2400, 2800),
        g("A", "B", "1-0").elo(2199, 2500),
        g("A", "B", "1-0").elo(2200, 2500),
        g("A", "B", "1-0").elo(2100, 2500),
        g("A", "B", "1-0").elo(2099, 2500),
        g("A", "B", "1-0").elo(0, 2500),
    ];
    let db = open(&games);
    let mut config = config();
    config.estimate = false;
    config.elo_min = 2100;
    config.elo_max = 2400;
    let r = rows(&hist(&db, &all(&db), GraphKind::Elo, &config));
    assert_eq!(
        r,
        [
            ("2100-2199".to_owned(), 2, 2), // 2100 and 2199
            ("2200-2299".to_owned(), 1, 1), // 2200
            ("2300-2399".to_owned(), 1, 1), // 2399
            ("2400-2499".to_owned(), 1, 1), // 2400
        ]
    );
}

#[test]
fn rating_with_estimate_uses_the_mean_and_caps_a_lone_rating_at_2200() {
    let games = [
        g("A", "B", "1-0").elo(2000, 2200), // mean 2100
        g("A", "B", "1-0").elo(2000, 2199), // sum 4199: mean 2099.5, below 4200
        g("A", "B", "1-0").elo(2250, 2149), // sum 4399: top of 2100-2199
        g("A", "B", "1-0").elo(2250, 2150), // sum 4400: 2200-2299
        g("A", "B", "1-0").elo(2600, 0),    // lone: 2600 + 2200 = 4800: 2400-2499
        g("A", "B", "1-0").elo(0, 2150),    // lone: 2150 + 2150 = 4300: 2100-2199
        g("A", "B", "1-0").elo(0, 0),       // sum 0: in no bin here
    ];
    let db = open(&games);
    let mut config = config();
    config.elo_min = 2100;
    config.elo_max = 2400;
    let r = rows(&hist(&db, &all(&db), GraphKind::Elo, &config));
    assert_eq!(
        r,
        [
            ("2100-2199".to_owned(), 3, 3),
            ("2200-2299".to_owned(), 1, 1),
            ("2300-2399".to_owned(), 0, 0),
            ("2400-2499".to_owned(), 1, 1),
        ]
    );
    // A bin that contains 0 takes the games without any rating (G2).
    config.elo_min = 0;
    config.elo_max = 0;
    let h = hist(&db, &all(&db), GraphKind::Elo, &config);
    assert_eq!(rows(&h), [("0-1799".to_owned(), 1, 1)]);
}

#[test]
fn the_first_rating_bin_covers_up_to_1799() {
    let games = [
        g("A", "B", "1-0").elo(0, 0),
        g("A", "B", "1-0").elo(1500, 1600),
        g("A", "B", "1-0").elo(1799, 1900),
        g("A", "B", "1-0").elo(1800, 1850),
    ];
    let db = open(&games);
    let mut config = config();
    config.estimate = false;
    config.elo_min = 0;
    config.elo_max = 1800;
    let r = rows(&hist(&db, &all(&db), GraphKind::Elo, &config));
    assert_eq!(
        r,
        [("0-1799".to_owned(), 3, 3), ("1800-1899".to_owned(), 1, 1)]
    );
}

#[test]
fn move_bins_use_half_moves_two_i_minus_one_to_two_i() {
    // 1, 2, 3 half-moves: the first two are move 1, the third is move 2.
    let games = [
        g("A", "B", "1-0").plies(1),
        g("A", "B", "1-0").plies(2),
        g("A", "B", "1-0").plies(3),
        g("A", "B", "1-0").plies(4),
        g("A", "B", "1-0").plies(5),
    ];
    let db = open(&games);
    let mut config = config();
    config.moves_min = 1;
    config.moves_max = 3;
    config.moves_step = 1;
    let r = rows(&hist(&db, &all(&db), GraphKind::Moves, &config));
    assert_eq!(
        r,
        [
            (String::new(), 2, 2),
            (String::new(), 2, 2),
            (String::new(), 1, 1),
        ]
    );
    // Step 2: [1, 4] and [5, 8] half-moves.
    config.moves_step = 2;
    config.moves_max = 3;
    let r = rows(&hist(&db, &all(&db), GraphKind::Moves, &config));
    assert_eq!(r, [(String::new(), 4, 4), (String::new(), 1, 1)]);
    // Labels only on multiples of 5.
    config.moves_min = 4;
    config.moves_max = 6;
    config.moves_step = 1;
    let labels: Vec<_> = hist(&db, &all(&db), GraphKind::Moves, &config)
        .bins
        .into_iter()
        .map(|b| b.label)
        .collect();
    assert_eq!(labels, ["", "5", ""]);
}

#[test]
fn an_empty_database_has_zero_points_and_no_mean() {
    let db = open(&[]);
    let h = hist(&db, &Filter::new(0), GraphKind::Year, &config());
    assert!(h.bins.iter().all(|b| b.filtered == 0 && b.all == 0));
    let relative = h.relative();
    assert_eq!(relative.mean, None);
    assert!(relative.bins.iter().all(|p| p.value == 0.0));
    assert_eq!(h.absolute().mean, None);
}

#[test]
fn the_one_pass_histogram_equals_counting_bin_by_bin() {
    // Games with all sorts of dates, ratings and lengths, 5000 of them.
    let games: Vec<G> = (0..5000u32)
        .map(|i| {
            let mut game = g("A", "B", "1-0")
                .date(&format!("{}.{:02}.15", 1900 + (i * 7) % 130, 1 + i % 12))
                .plies(1 + (i as usize * 3) % 20);
            if i % 5 != 0 {
                game = game.elo(1500 + (i * 13 % 1500) as u16, 1500 + (i * 29 % 1500) as u16);
            }
            game
        })
        .collect();
    let db = open(&games);
    let mut filter = Filter::new(db.len());
    for n in (1..=db.len()).filter(|n| n % 4 != 1) {
        filter.set(n, 1);
    }
    filter.set(7, 0);
    let entries: Vec<_> = (1..=db.len()).map(|n| (n, db.entry(n).unwrap())).collect();
    let mut config = config();
    config.year_min = 1950;
    config.year_max = 2012;
    config.moves_max = 12;

    // Decade, by brute force over the same ranges.
    let count = |keep: &dyn Fn(&mychess_db::si4::IndexEntry) -> bool| -> (u32, u32) {
        let mut filtered = 0;
        let mut total = 0;
        for (n, e) in &entries {
            if keep(e) {
                total += 1;
                filtered += u32::from(filter.contains(*n));
            }
        }
        (filtered, total)
    };
    let year = |e: &mychess_db::si4::IndexEntry| u32::from(e.date.year().unwrap_or(0));
    let h = hist(&db, &filter, GraphKind::Decade, &config);
    for (i, bin) in h.bins.iter().enumerate() {
        let (lo, hi): (u32, u32) = match i {
            0 => (0, 1919),
            10 => (2010, u32::MAX),
            _ => (1920 + 10 * (i as u32 - 1), 1929 + 10 * (i as u32 - 1)),
        };
        let expected = count(&|e| (lo..=hi).contains(&year(e)));
        assert_eq!((bin.filtered, bin.all), expected, "decade {}", bin.label);
    }
    let h = hist(&db, &filter, GraphKind::Year, &config);
    for (i, bin) in h.bins.iter().enumerate() {
        let y = 1950 + i as u32;
        assert_eq!(
            (bin.filtered, bin.all),
            count(&|e| year(e) == y),
            "year {y}"
        );
    }
    let h = hist(&db, &filter, GraphKind::Moves, &config);
    for (i, bin) in h.bins.iter().enumerate() {
        let mv = 5 + i as u32;
        let expected = count(&|e| {
            let half = u32::from(e.num_half_moves);
            half == 2 * mv - 1 || half == 2 * mv
        });
        assert_eq!((bin.filtered, bin.all), expected, "move {mv}");
    }
    let h = hist(&db, &filter, GraphKind::Elo, &config);
    for (i, bin) in h.bins.iter().enumerate() {
        let lo = 2100 + 100 * i as u32;
        let expected = count(&|e| {
            let (w, b) = (u32::from(e.white_elo), u32::from(e.black_elo));
            let mut sum = w + b;
            if w == 0 && b != 0 {
                sum += b.min(2200);
            } else if b == 0 && w != 0 {
                sum += w.min(2200);
            }
            (2 * lo..=2 * (lo + 99) + 1).contains(&sum)
        });
        assert_eq!((bin.filtered, bin.all), expected, "elo {lo}");
    }

    // One thread and many threads give the same histograms.
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                [
                    GraphKind::Decade,
                    GraphKind::Year,
                    GraphKind::Elo,
                    GraphKind::Moves,
                ]
                .map(|kind| hist(&db, &filter, kind, &config))
            })
    };
    assert_eq!(run(1), run(4));
}

#[test]
fn a_filter_of_another_size_and_a_cancelled_token_are_errors() {
    let db = open(&[g("A", "B", "1-0")]);
    assert_eq!(
        filter_histogram(
            &db,
            &Filter::new(3),
            GraphKind::Year,
            &config(),
            &mut NoProgress,
            &CancelToken::new()
        )
        .unwrap_err(),
        StatsError::FilterSize {
            filter: 3,
            games: 1
        }
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        filter_histogram(
            &db,
            &all(&db),
            GraphKind::Year,
            &config(),
            &mut NoProgress,
            &cancel
        )
        .unwrap_err(),
        StatsError::Cancelled
    );
}

#[test]
fn partial_dates_count_in_the_year() {
    let games = [g("A", "B", "1-0").date("2020.??.??")];
    let db = open(&games);
    let entry = db.entry(1).unwrap();
    assert_eq!(entry.date, PartialDate::parse_lenient("2020.??.??"));
    let mut config = config();
    config.year_min = 2020;
    config.year_max = 2020;
    let h = hist(&db, &all(&db), GraphKind::Year, &config);
    assert_eq!(rows(&h), [("2020".to_owned(), 1, 1)]);
}
