//! M7-3a: the games of a player with one colour (acceptance criteria 1 and 8
//! of `docs/specs/m7-3-relatorio-de-jogador.md`).
#![allow(clippy::unwrap_used)]

#[path = "common/m7_3a_support.rs"]
mod support;

use mychess_core::Color::{Black, White};
use mychess_db::report::{NameMatch, ReportError, games_of_player};
use mychess_db::{CancelToken, Filter, GameSource};
use support::{fixture, open};

fn games(filter: &Filter) -> Vec<u32> {
    filter.iter_games().collect()
}

fn pick(name: &str, color: mychess_core::Color, matching: NameMatch) -> Vec<u32> {
    let db = fixture();
    games(&games_of_player(&db, name, color, matching, &CancelToken::new()).unwrap())
}

#[test]
fn exact_name_with_white_gives_games_1_4_and_6() {
    assert_eq!(pick("Alfa, A", White, NameMatch::Exact), [1, 4, 6]);
}

#[test]
fn exact_name_with_black_gives_games_2_and_7() {
    assert_eq!(pick("Alfa, A", Black, NameMatch::Exact), [2, 7]);
}

#[test]
fn prefix_joins_the_two_spellings() {
    assert_eq!(pick("Alfa", White, NameMatch::Prefix), [1, 4, 6, 8]);
}

#[test]
fn contains_ignores_the_case_and_matches_inside_the_name() {
    assert_eq!(pick("fa, ", White, NameMatch::Contains), [1, 4, 6, 8]);
    assert_eq!(pick("FA, A", White, NameMatch::Contains), [1, 4, 6]);
    assert_eq!(pick("eta", Black, NameMatch::Contains), [1, 5, 8]);
}

#[test]
fn prefix_does_not_match_in_the_middle() {
    assert_eq!(pick("lfa", White, NameMatch::Prefix), Vec::<u32>::new());
}

#[test]
fn exact_name_differs_by_case() {
    assert_eq!(pick("alfa, a", White, NameMatch::Exact), Vec::<u32>::new());
}

#[test]
fn unknown_name_gives_an_empty_filter_not_an_error() {
    let db = fixture();
    let f = games_of_player(&db, "Zeta, Z", White, NameMatch::Exact, &CancelToken::new()).unwrap();
    assert_eq!(f.len(), db.len());
    assert_eq!(f.count(), 0);
}

#[test]
fn a_player_with_no_game_of_that_colour_gives_an_empty_filter() {
    // "Alfa, Z" only has the game 8, with White.
    assert_eq!(pick("Alfa, Z", Black, NameMatch::Exact), Vec::<u32>::new());
    assert_eq!(pick("Alfa, Z", White, NameMatch::Exact), [8]);
}

#[test]
fn every_selected_game_has_the_value_one() {
    let db = fixture();
    let f = games_of_player(&db, "Alfa, A", White, NameMatch::Exact, &CancelToken::new()).unwrap();
    assert!(f.iter_games().all(|n| f.get(n) == 1 && f.ply(n) == 0));
}

#[test]
fn an_empty_database_gives_an_empty_filter() {
    let db = open("");
    assert_eq!(db.len(), 0);
    let f = games_of_player(&db, "Alfa, A", White, NameMatch::Exact, &CancelToken::new()).unwrap();
    assert_eq!((f.len(), f.count()), (0, 0));
}

#[test]
fn empty_and_unknown_names_are_invalid() {
    let db = fixture();
    for name in ["", "   ", "?", " ? "] {
        let r = games_of_player(&db, name, White, NameMatch::Exact, &CancelToken::new());
        assert!(matches!(r, Err(ReportError::InvalidName)), "{name:?}");
    }
}

#[test]
fn a_cancelled_token_stops_the_selection() {
    let db = fixture();
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = games_of_player(&db, "Alfa, A", White, NameMatch::Exact, &cancel);
    assert!(matches!(r, Err(ReportError::Cancelled)));
}

#[test]
fn the_name_is_trimmed_before_the_match() {
    assert_eq!(pick("  Alfa, A ", White, NameMatch::Exact), [1, 4, 6]);
}
