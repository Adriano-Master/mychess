//! M6-4a: which games form the tournament of a reference game (spec 2.1,
//! D3356 to D3358). Expected values come from `isCrosstableGame` and
//! `sc_game_crosstable` of the Scid and the decisions of the spec, not from
//! the implementation.
#![allow(clippy::unwrap_used, clippy::too_many_lines, clippy::assert_is_empty)]

#[path = "common/m6_4a_support.rs"]
mod support;

use mychess_core::PartialDate;
use mychess_db::{
    CROSSTABLE_MAX_GAMES, CancelToken, CrosstableError, NoProgress, TournamentReference,
    load_games, load_games_cancellable, select_tournament,
};
use pretty_assertions::assert_eq;
use support::{Game, game, open};

fn select(games: &[Game], reference_game: u32, deleted: bool) -> Vec<u32> {
    let db = open(games);
    let reference = TournamentReference::from_game(&db, reference_game).unwrap();
    select_tournament(
        &db,
        &reference,
        deleted,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
    .games
}

#[test]
fn takes_games_with_the_same_event_and_site_within_three_months() {
    let games = [
        game("A", "B", "1-0").date("2020.05.15"),
        game("A", "C", "1-0").date("2020.02.15"),
        game("B", "C", "1-0").date("2020.08.15"),
        game("A", "D", "1-0").date("2020.02.14"),
        game("B", "D", "1-0").date("2020.08.16"),
        game("C", "D", "1-0").date("2020.05.15").event("Other"),
        game("C", "D", "1-0").date("2020.05.15").site("Elsewhere"),
    ];
    assert_eq!(select(&games, 1, false), [1, 2, 3]);
}

#[test]
fn event_date_pulls_in_games_far_in_time() {
    let games = [
        game("A", "B", "1-0")
            .date("2020.05.15")
            .event_date("2020.05.01"),
        game("A", "C", "1-0")
            .date("2021.01.01")
            .event_date("2020.05.01"),
        game("A", "D", "1-0")
            .date("2021.01.01")
            .event_date("2020.05.02"),
        game("B", "C", "1-0").date("2021.01.01"),
    ];
    assert_eq!(select(&games, 1, false), [1, 2]);
}

#[test]
fn a_reference_with_only_a_year_takes_the_whole_year_including_december() {
    let games = [
        game("A", "B", "1-0").date("2020.??.??"),
        game("A", "C", "1-0").date("2020.12.20"),
        game("B", "C", "1-0").date("2020.01.01"),
        game("B", "D", "1-0").date("2020.??.??"),
        game("C", "D", "1-0").date("2019.12.31"),
        game("C", "E", "1-0").date("2021.01.01"),
    ];
    assert_eq!(select(&games, 1, false), [1, 2, 3, 4]);
}

#[test]
fn a_game_with_unknown_month_and_day_matches_by_year() {
    let games = [
        game("A", "B", "1-0").date("2020.05.15"),
        game("A", "C", "1-0").date("2020.??.??"),
        game("A", "D", "1-0").date("2019.??.??"),
    ];
    assert_eq!(select(&games, 1, false), [1, 2]);
}

#[test]
fn games_of_one_player_against_themselves_are_ignored() {
    let games = [game("A", "B", "1-0"), game("A", "A", "1-0")];
    assert_eq!(select(&games, 1, false), [1]);
}

#[test]
fn deleted_games_need_the_option() {
    let games = [game("A", "B", "1-0"), game("A", "C", "1-0").deleted()];
    assert_eq!(select(&games, 1, false), [1]);
    assert_eq!(select(&games, 1, true), [1, 2]);
}

#[test]
fn names_are_compared_exactly_with_case() {
    let games = [
        game("A", "B", "1-0").event("Open"),
        game("A", "C", "1-0").event("open"),
        game("A", "D", "1-0").event("Open "),
    ];
    assert_eq!(select(&games, 1, false), [1]);
}

#[test]
fn the_reference_may_be_any_game_of_the_tournament() {
    let games = [
        game("A", "B", "1-0").date("2020.05.01"),
        game("A", "C", "1-0").date("2020.07.30"),
        game("B", "C", "1-0").date("2020.10.28"),
    ];
    // From game 2 (July 30) the window reaches April 30 to October 30.
    assert_eq!(select(&games, 2, false), [1, 2, 3]);
    // From game 1 it ends on August 1.
    assert_eq!(select(&games, 1, false), [1, 2]);
}

#[test]
fn an_event_that_is_not_in_the_database_selects_nothing() {
    let db = open(&[game("A", "B", "1-0")]);
    let reference = TournamentReference {
        event: "Nowhere".into(),
        site: "Rio".into(),
        event_raw: Vec::new(),
        site_raw: Vec::new(),
        date: PartialDate::UNKNOWN,
        event_date: PartialDate::UNKNOWN,
    };
    let selection =
        select_tournament(&db, &reference, false, &mut NoProgress, &CancelToken::new()).unwrap();
    assert!(selection.games.is_empty());
}

#[test]
fn reference_from_a_missing_game_is_an_error() {
    let db = open(&[game("A", "B", "1-0")]);
    assert!(matches!(
        TournamentReference::from_game(&db, 0),
        Err(CrosstableError::InvalidGame(0))
    ));
    assert!(matches!(
        TournamentReference::from_game(&db, 2),
        Err(CrosstableError::InvalidGame(2))
    ));
}

#[test]
fn reference_carries_event_site_and_dates() {
    let db = open(&[game("A", "B", "1-0").event_date("2020.04.01")]);
    let r = TournamentReference::from_game(&db, 1).unwrap();
    assert_eq!(
        (
            r.event.as_str(),
            r.site.as_str(),
            r.date.to_string(),
            r.event_date.to_string()
        ),
        ("Club", "Rio", "2020.05.01".into(), "2020.04.01".into())
    );
}

#[test]
fn a_cancelled_selection_is_an_error() {
    // The token is looked at every 5000 games, so the database needs more.
    let games: Vec<Game> = (0..5100).map(|_| game("A", "B", "1/2-1/2")).collect();
    let db = open(&games);
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = select_tournament(&db, &reference, false, &mut NoProgress, &cancel);
    assert!(matches!(result, Err(CrosstableError::Cancelled)));
    cancel.reset();
    let all = select_tournament(&db, &reference, false, &mut NoProgress, &cancel).unwrap();
    assert_eq!(all.games.len(), 5100);
}

#[test]
fn progress_ends_at_the_total() {
    let db = open(&[game("A", "B", "1-0"), game("A", "C", "1-0")]);
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let mut last = (0, 0);
    let mut sink = |done, total| last = (done, total);
    select_tournament(&db, &reference, false, &mut sink, &CancelToken::new()).unwrap();
    assert_eq!(last, (2, 2));
}

#[test]
fn loaded_games_carry_names_rounds_and_elo_in_game_order() {
    let games = [
        game("Ana", "Bia", "1-0").round("1.2").elo(2500, 2400),
        game("Bia", "Ana", "1/2-1/2").round("2"),
        game("Ana", "Cid", "*").round("?"),
    ];
    let db = open(&games);
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let selection =
        select_tournament(&db, &reference, false, &mut NoProgress, &CancelToken::new()).unwrap();
    let data = load_games(&db, &selection);
    assert_eq!(data.games.len(), 3);
    assert_eq!(
        data.games
            .iter()
            .map(|g| (g.number, g.round, g.sub_round))
            .collect::<Vec<_>>(),
        [(1, 1, true), (2, 2, false), (3, 0, false)]
    );
    assert_eq!(
        (data.games[0].white_elo, data.games[0].black_elo),
        (2500, 2400)
    );
    let mut names: Vec<&str> = data.names.values().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, ["Ana", "Bia", "Cid"]);
}

#[test]
fn names_are_compared_by_their_stored_bytes() {
    let db = open(&[game("A", "B", "1-0").event("Pérez Open")]);
    let mut reference = TournamentReference::from_game(&db, 1).unwrap();
    assert_eq!(reference.event_raw, "Pérez Open".as_bytes());
    let all = |r: &TournamentReference| {
        select_tournament(&db, r, false, &mut NoProgress, &CancelToken::new())
            .unwrap()
            .games
    };
    assert_eq!(all(&reference), [1]);
    // The same text as Windows-1252 bytes is a different name for the Scid.
    reference.event_raw = b"P\xe9rez Open".to_vec();
    assert_eq!(all(&reference), Vec::<u32>::new());
    // A reference made by hand, without raw bytes, uses the UTF-8 of the text.
    reference.event_raw.clear();
    assert_eq!(all(&reference), [1]);
}

#[test]
fn the_selection_stops_at_the_game_limit() {
    let mut text = String::new();
    for _ in 0..=CROSSTABLE_MAX_GAMES {
        text.push_str(
            "[Event \"E\"]\n[Site \"S\"]\n[Date \"2020.05.01\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1/2-1/2\"]\n\n1/2-1/2\n\n",
        );
    }
    let db = mychess_db::PgnDatabase::from_reader(
        text.as_bytes(),
        std::path::Path::new("big.pgn"),
        &mychess_db::OpenOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
    .0;
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let selection =
        select_tournament(&db, &reference, false, &mut NoProgress, &CancelToken::new()).unwrap();
    assert!(selection.truncated);
    assert_eq!(selection.games.len(), CROSSTABLE_MAX_GAMES);
    assert!(load_games(&db, &selection).truncated);
}

#[test]
fn a_selection_within_the_limit_is_not_truncated() {
    let db = open(&[game("A", "B", "1-0"), game("A", "C", "1-0")]);
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let selection =
        select_tournament(&db, &reference, false, &mut NoProgress, &CancelToken::new()).unwrap();
    assert!(!selection.truncated);
    assert_eq!(selection.unreadable, 0);
    let data = load_games(&db, &selection);
    assert!(!data.truncated);
    assert_eq!(data.unreadable, 0);
}

#[test]
fn unreadable_entries_are_counted_and_carried_to_the_games() {
    let db = open(&[game("A", "B", "1-0")]);
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let mut selection =
        select_tournament(&db, &reference, false, &mut NoProgress, &CancelToken::new()).unwrap();
    selection.unreadable = 2;
    // Game 99 does not exist: reading its entry fails.
    selection.games.push(99);
    let data = load_games(&db, &selection);
    assert_eq!(data.games.len(), 1);
    assert_eq!(data.unreadable, 3);
}

#[test]
fn loading_games_can_be_cancelled() {
    let db = open(&[game("A", "B", "1-0")]);
    let reference = TournamentReference::from_game(&db, 1).unwrap();
    let mut selection =
        select_tournament(&db, &reference, false, &mut NoProgress, &CancelToken::new()).unwrap();
    selection.games = vec![1; 6000];
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = load_games_cancellable(&db, &selection, &cancel);
    assert!(matches!(result, Err(CrosstableError::Cancelled)));
    cancel.reset();
    assert_eq!(
        load_games_cancellable(&db, &selection, &cancel)
            .unwrap()
            .games
            .len(),
        6000
    );
}
