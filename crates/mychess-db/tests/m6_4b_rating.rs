//! M6-4b: the rating graph of a player (spec 2.4, rules R1 to R4, decision
//! D3375, criterion 14). Expected values come from `sc_name info -ratings`
//! of the Scid and the corrections of the spec.
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
    CancelToken, GameSource, MAX_RATING_PLAYERS, NoProgress, RatingOptions, RatingPlayer,
    RatingSeries, StatsError, rating_histories, rating_history, rating_x,
};
use pretty_assertions::assert_eq;
use support::{all, g, only, open};

fn history(db: &impl GameSource, player: &RatingPlayer, options: RatingOptions) -> RatingSeries {
    rating_history(
        db,
        player,
        options,
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

fn name(n: &str) -> RatingPlayer {
    RatingPlayer::Name(n.to_owned())
}

#[test]
fn the_last_game_of_the_month_gives_the_rating_in_either_colour() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.03.01").elo(2400, 2300),
        g("Bob", "Ann", "1-0").date("2000.03.20").elo(2310, 2410), // last of March, Black
        g("Ann", "Bob", "1-0").date("2000.01.31").elo(2390, 2290), // game 3, January
        g("Ann", "Cy", "1-0").date("2001.12.01").elo(2450, 2000),
    ]);
    let series = history(&db, &name("Ann"), RatingOptions::default());
    // March: game 2 (number order) as Black, 2410; the order is by game
    // number, not by date, but each month has one entry.
    assert_eq!(
        series.points,
        [(2000, 1, 2390), (2000, 3, 2410), (2001, 12, 2450)]
    );
    let bob = history(&db, &name("Bob"), RatingOptions::default());
    assert_eq!(bob.points, [(2000, 1, 2290), (2000, 3, 2310)]);
}

#[test]
fn the_highest_game_number_wins_even_if_its_date_is_earlier() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.03.28").elo(2400, 2300),
        g("Ann", "Bob", "1-0").date("2000.03.02").elo(2380, 2300),
    ]);
    let series = history(&db, &name("Ann"), RatingOptions::default());
    assert_eq!(series.points, [(2000, 3, 2380)]);
}

#[test]
fn a_game_without_rating_does_not_erase_the_month() {
    // Scid with the minimum at 0 lets the rating 0 of game 2 overwrite 2400.
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.05.01").elo(2400, 2300),
        g("Ann", "Bob", "1-0").date("2000.05.20"),
        g("Ann", "Bob", "1-0").date("2000.06.20"),
    ]);
    let series = history(&db, &name("Ann"), RatingOptions::default());
    assert_eq!(series.points, [(2000, 5, 2400)]);
}

#[test]
fn start_year_and_start_elo_drop_points() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("1979.12.31").elo(2000, 2000),
        g("Ann", "Bob", "1-0").date("1980.01.01").elo(2100, 2000),
        g("Ann", "Bob", "1-0").date("1981.01.01").elo(1999, 2000),
        g("Ann", "Bob", "1-0").date("1982.01.01").elo(2000, 2000),
    ]);
    let year = |start_year, start_elo| {
        history(
            &db,
            &name("Ann"),
            RatingOptions {
                start_year,
                start_elo,
                only_filter: false,
            },
        )
        .points
    };
    assert_eq!(
        year(1900, 0),
        [
            (1979, 12, 2000),
            (1980, 1, 2100),
            (1981, 1, 1999),
            (1982, 1, 2000)
        ]
    );
    assert_eq!(
        year(1980, 0),
        [(1980, 1, 2100), (1981, 1, 1999), (1982, 1, 2000)]
    );
    // Rating at least 2000: 1999 is dropped, 2000 stays.
    assert_eq!(
        year(1900, 2000),
        [(1979, 12, 2000), (1980, 1, 2100), (1982, 1, 2000)]
    );
}

#[test]
fn unknown_month_is_january_and_unknown_year_is_dropped() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.??.??").elo(2400, 2300),
        g("Ann", "Bob", "1-0").date("????.??.??").elo(2500, 2300),
        g("Ann", "Bob", "1-0").date("2002.07.??").elo(2450, 2300),
    ]);
    let series = history(&db, &name("Ann"), RatingOptions::default());
    assert_eq!(series.points, [(2000, 1, 2400), (2002, 7, 2450)]);
}

#[test]
fn a_player_without_games_has_an_empty_series() {
    let db = open(&[g("Ann", "Bob", "1-0").elo(2400, 2300)]);
    assert!(history(&db, &name("Zed"), RatingOptions::default()).is_empty());
    assert!(history(&db, &RatingPlayer::Id(9999), RatingOptions::default()).is_empty());
    let empty = open(&[]);
    assert!(history(&empty, &name("Ann"), RatingOptions::default()).is_empty());
}

#[test]
fn a_player_can_be_given_by_name_or_by_id() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.03.01").elo(2400, 2300),
        g("Cy", "Bob", "1-0").date("2000.04.01").elo(2000, 2310),
    ]);
    let bob_id = db.entry(1).unwrap().black_id;
    let by_id = history(&db, &RatingPlayer::Id(bob_id), RatingOptions::default());
    let by_name = history(&db, &name("Bob"), RatingOptions::default());
    assert_eq!(by_id, by_name);
    assert_eq!(by_id.points, [(2000, 3, 2300), (2000, 4, 2310)]);
}

#[test]
fn only_the_games_of_the_filter_when_asked() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.03.01").elo(2400, 2300),
        g("Ann", "Bob", "1-0").date("2000.03.02").elo(2450, 2300),
        g("Ann", "Bob", "1-0").date("2000.04.02").elo(2500, 2300),
    ]);
    let filter = only(&db, &[1, 3]);
    let options = RatingOptions {
        only_filter: true,
        ..RatingOptions::default()
    };
    let with = rating_history(
        &db,
        &name("Ann"),
        options,
        Some(&filter),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(with.points, [(2000, 3, 2400), (2000, 4, 2500)]);
    // Without the option the filter is not looked at.
    let without = rating_history(
        &db,
        &name("Ann"),
        RatingOptions::default(),
        Some(&filter),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(without.points, [(2000, 3, 2450), (2000, 4, 2500)]);
    // A filter of another size is an error when it is used.
    let bad = mychess_db::Filter::new(9);
    assert_eq!(
        rating_history(
            &db,
            &name("Ann"),
            options,
            Some(&bad),
            &mut NoProgress,
            &CancelToken::new()
        )
        .unwrap_err(),
        StatsError::FilterSize {
            filter: 9,
            games: 3
        }
    );
}

#[test]
fn several_players_come_from_one_pass_and_at_most_nine() {
    let db = open(&[
        g("Ann", "Bob", "1-0").date("2000.03.01").elo(2400, 2300),
        g("Bob", "Cy", "1-0").date("2000.04.01").elo(2310, 2100),
    ]);
    let players = [name("Ann"), name("Bob"), name("Cy"), name("Nobody")];
    let all_series = rating_histories(
        &db,
        &players,
        RatingOptions::default(),
        Some(&all(&db)),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(all_series.len(), 4);
    assert_eq!(all_series[0].points, [(2000, 3, 2400)]);
    assert_eq!(all_series[1].points, [(2000, 3, 2300), (2000, 4, 2310)]);
    assert_eq!(all_series[2].points, [(2000, 4, 2100)]);
    assert!(all_series[3].is_empty());

    let many: Vec<RatingPlayer> = (0..12).map(|i| name(&format!("P{i}"))).collect();
    let series = rating_histories(
        &db,
        &many,
        RatingOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(series.len(), MAX_RATING_PLAYERS);
    assert_eq!(MAX_RATING_PLAYERS, 9);
}

#[test]
fn x_is_the_year_plus_hundredths_of_the_month() {
    assert_eq!(rating_x(1999, 1), 1999.0);
    assert_eq!(rating_x(1999, 4), 1999.25);
    assert_eq!(rating_x(1999, 7), 1999.5);
    assert_eq!(rating_x(1999, 10), 1999.75);
    assert_eq!(rating_x(1999, 12), 1999.91);
    let db = open(&[g("Ann", "Bob", "1-0").date("2000.04.01").elo(2400, 2300)]);
    let series = history(&db, &name("Ann"), RatingOptions::default());
    assert_eq!(series.xy(), [(2000.25, 2400)]);
}

#[test]
fn a_cancelled_token_is_an_error_and_blocks_agree_across_threads() {
    let db = open(&[g("Ann", "Bob", "1-0").elo(2400, 2300)]);
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        rating_history(
            &db,
            &name("Ann"),
            RatingOptions::default(),
            None,
            &mut NoProgress,
            &cancel
        )
        .unwrap_err(),
        StatsError::Cancelled
    );
    // 5000 games over many months: the last game of each month must win
    // across the task boundaries (2048 entries) with any number of threads.
    let games: Vec<_> = (0..5000u32)
        .map(|i| {
            g("Ann", "Bob", "1-0")
                .date(&format!("{}.{:02}.10", 1950 + i / 120, 1 + (i / 10) % 12))
                .elo(2000 + (i % 700) as u16, 2000)
        })
        .collect();
    let db = open(&games);
    let run = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| history(&db, &name("Ann"), RatingOptions::default()))
    };
    let one = run(1);
    assert_eq!(one, run(4));
    // Brute force: the last game number of each (year, month).
    let mut expected = std::collections::BTreeMap::new();
    for n in 1..=db.len() {
        let e = db.entry(n).unwrap();
        expected.insert(
            (e.date.year().unwrap(), e.date.month().unwrap()),
            e.white_elo,
        );
    }
    let expected: Vec<_> = expected.into_iter().map(|((y, m), e)| (y, m, e)).collect();
    assert_eq!(one.points, expected);
}
