//! The game chooser and collections (spec M2-2, D124-D126, rules 24, 29).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::assigning_clones,
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    let_underscore_drop
)]

mod m2_2_common;

use std::sync::Arc;

use m2_2_common::*;
use mychess_app::{
    ChooserMessage as C, GameChooser, PgnCollection, PgnLoad, PgnSource, ScanLimits, scan_pgn,
};
use mychess_pgn::read::ReadOptions;
use rstest::rstest;

fn game_text(
    event: &str,
    site: &str,
    round: &str,
    white: &str,
    black: &str,
    date: &str,
    result: &str,
) -> String {
    format!(
        "[Event \"{event}\"]\n[Site \"{site}\"]\n[Date \"{date}\"]\n[Round \"{round}\"]\n[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n[WhiteElo \"2700\"]\n[ECO \"B90\"]\n\n1. e4 c5 {result}\n\n"
    )
}

fn collection() -> Arc<PgnCollection> {
    let text = [
        game_text(
            "Tata Steel",
            "Wijk aan Zee",
            "1",
            "Carlsen, M.",
            "Caruana, F.",
            "2024.01.14",
            "1-0",
        ),
        game_text(
            "Candidates",
            "Toronto",
            "7.2",
            "\u{c9}cole, Jos\u{e9}",
            "Nepo",
            "2023.04.09",
            "1/2-1/2",
        ),
        game_text(
            "Olympiad",
            "Budapest",
            "3",
            "Anand, V.",
            "Gukesh, D.",
            "2022.08.01",
            "0-1",
        ),
    ]
    .concat();
    let (load, _) = scan_pgn(
        PgnSource::Text(text),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )
    .unwrap();
    match load {
        PgnLoad::Many(c) => c,
        other => panic!("{other:?}"),
    }
}

#[test]
fn new_chooser_shows_everything_and_highlights_the_first_game() {
    let c = GameChooser::new(collection());
    assert_eq!(c.visible, vec![0, 1, 2]);
    assert_eq!(c.selected, Some(0));
    assert!(c.filter.is_empty());
}

#[test]
fn headers_expose_rows_for_the_list() {
    let c = collection();
    let h = c.header(0).unwrap();
    assert_eq!(
        (h.white.as_str(), h.black.as_str()),
        ("Carlsen, M.", "Caruana, F.")
    );
    assert_eq!(h.white_elo, Some(2700));
    assert_eq!(h.black_elo, None);
    assert_eq!(h.event, "Tata Steel");
    assert_eq!(h.round, "1");
    assert_eq!(h.date.year(), Some(2024));
    assert!(h.eco.is_some());
    assert!(c.header(3).is_none());
}

#[rstest]
#[case("carlsen", vec![0])]
#[case("CARLSEN", vec![0])]
#[case("ca", vec![0, 1])] // "Carlsen" and the "Candidates" event
#[case("toronto", vec![1])]
#[case("tata", vec![0])]
#[case("7.2", vec![1])]
#[case("2022", vec![2])]
#[case("1-0", vec![0])]
#[case("0-1", vec![2])]
#[case("1/2", vec![1])]
#[case("\u{e9}cole", vec![1])]
#[case("\u{c9}COLE", vec![1])]
#[case("nepo", vec![1])]
#[case("", vec![0, 1, 2])]
#[case("   ", vec![0, 1, 2])]
#[case("zzzz", vec![])]
fn d125_filter_is_a_case_insensitive_substring_search(
    #[case] query: &str,
    #[case] expected: Vec<usize>,
) {
    let mut c = GameChooser::new(collection());
    let mut h = Harness::new();
    h.state.chooser = Some(c.clone());
    h.send(C::Filter(query.to_owned()));
    c = h.state.chooser.clone().unwrap();
    assert_eq!(c.visible, expected, "query {query:?}");
    assert_eq!(c.filter, query);
}

#[test]
fn d125_filter_does_not_search_elo_eco_or_month() {
    let c = collection();
    assert!(c.filter("2700").is_empty(), "Elo is not searched");
    assert!(c.filter("b90").is_empty(), "ECO is not searched");
    assert!(c.filter("2024.01").len() <= 1);
    assert!(c.filter("14").is_empty(), "day/month are not searched");
}

#[test]
fn d125_unicode_case_folding_handles_the_final_sigma() {
    let text = game_text(
        "E",
        "S",
        "1",
        "\u{39f}\u{394}\u{3a5}\u{3a3}\u{3a3}\u{395}\u{391}\u{3a3}",
        "B",
        "2024.01.01",
        "*",
    );
    let (load, _) = scan_pgn(
        PgnSource::Text(format!("{text}{text}")),
        &ScanLimits::default(),
        &ReadOptions::default(),
    )
    .unwrap();
    let PgnLoad::Many(c) = load else { panic!() };
    // Lower case with a final sigma is "\u{3bf}\u{3b4}\u{3c5}\u{3c3}\u{3c3}\u{3b5}\u{3b1}\u{3c2}"; the user types a plain sigma.
    assert_eq!(
        c.filter("\u{3bf}\u{3b4}\u{3c5}\u{3c3}\u{3c3}\u{3b5}\u{3b1}\u{3c3}")
            .len(),
        2
    );
}

#[test]
fn filter_keeps_the_selection_when_still_visible_and_otherwise_moves_it() {
    let mut h = Harness::new();
    h.state.chooser = Some(GameChooser::new(collection()));
    h.send(C::Select(2));
    h.send(C::Filter("anand".to_owned()));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(2));
    h.send(C::Filter("carlsen".to_owned()));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(0));
    h.send(C::Filter("nomatch".to_owned()));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, None);
    // Confirm with nothing highlighted does nothing.
    let eff = h.raw(C::Confirm);
    assert!(eff.is_empty());
}

#[test]
fn select_ignores_hidden_and_out_of_range_rows() {
    let mut h = Harness::new();
    h.state.chooser = Some(GameChooser::new(collection()));
    h.send(C::Filter("carlsen".to_owned()));
    h.send(C::Select(2));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(0));
    h.send(C::Select(999));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(0));
}

#[test]
fn move_stays_within_the_visible_rows() {
    let mut h = Harness::new();
    h.state.chooser = Some(GameChooser::new(collection()));
    h.send(C::Move(1));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(1));
    h.send(C::Move(100));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(2));
    h.send(C::Move(-100));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(0));
    h.send(C::Move(isize::MIN));
    h.send(C::Move(isize::MAX));
    assert_eq!(h.state.chooser.as_ref().unwrap().selected, Some(2));
}

#[test]
fn confirm_opens_the_highlighted_game_and_open_a_given_one() {
    let mut h = Harness::new();
    h.state.chooser = Some(GameChooser::new(collection()));
    h.send(C::Move(1));
    let eff = h.raw(C::Confirm);
    assert!(matches!(
        eff.as_slice(),
        [Effect::ParseCollectionGame { index: 1, .. }]
    ));
    h.send(C::Confirm);
    assert_eq!(h.tags().white, "\u{c9}cole, Jos\u{e9}");
    assert!(h.state.chooser.is_none(), "choosing closes the chooser");
    assert_eq!(h.plies(), 2);
}

#[test]
fn escape_cancels_without_touching_the_game() {
    let mut h = Harness::new();
    h.play("e2e4");
    h.state.chooser = Some(GameChooser::new(collection()));
    let eff = h.send(C::Cancel);
    assert!(eff.is_empty());
    assert!(h.state.chooser.is_none());
    assert_eq!(h.plies(), 1);
}

#[test]
fn open_with_an_out_of_range_index_is_ignored() {
    let mut h = Harness::new();
    h.state.chooser = Some(GameChooser::new(collection()));
    assert!(h.raw(C::Open(3)).is_empty());
    assert!(h.raw(C::Open(usize::MAX)).is_empty());
    assert!(h.state.chooser.is_some());
}

#[test]
fn chooser_messages_without_a_chooser_do_nothing() {
    let mut h = Harness::new();
    for m in [
        C::Confirm,
        C::Open(0),
        C::Move(1),
        C::Select(0),
        C::Filter("x".into()),
    ] {
        assert!(h.raw(m).is_empty());
    }
    assert!(h.state.chooser.is_none());
}

#[test]
fn stale_collection_game_answer_is_dropped() {
    let mut h = Harness::new();
    h.state.chooser = Some(GameChooser::new(collection()));
    let first = h.raw(C::Open(0));
    let second = h.raw(C::Open(2));
    let a = h.perform(first.into_iter().next().unwrap());
    let eff = h.raw(a);
    assert!(eff.is_empty());
    assert_eq!(h.plies(), 0, "answer of the superseded request ignored");
    let b = h.perform(second.into_iter().next().unwrap());
    h.raw(b);
    assert_eq!(h.tags().white, "Anand, V.");
}

#[test]
fn opening_a_chooser_teaches_the_name_book_the_players_of_the_collection() {
    let mut h = Harness::new();
    let c = collection();
    let dir = tempfile::tempdir().unwrap();
    let text: String = (0..3)
        .map(|i| String::from_utf8(c.raw_bytes(i).unwrap().to_vec()).unwrap())
        .collect();
    let file = write_file(dir.path(), "all.pgn", text.as_bytes());
    h.send(Command::Open);
    h.pick(Some(&file));
    let found = h
        .state
        .names
        .matches(mychess_app::NameKind::Player, "car", 9);
    let names: Vec<_> = found.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"Carlsen, M."), "{names:?}");
    assert!(names.contains(&"Caruana, F."), "{names:?}");
}

#[test]
fn parse_of_a_collection_game_matches_a_direct_parse() {
    let c = collection();
    let h = Harness::new();
    let eff = h.perform(Effect::ParseCollectionGame {
        request: RequestId(1),
        collection: c.clone(),
        index: 2,
    });
    let Message::CollectionGameParsed { result, .. } = eff else {
        panic!()
    };
    let loaded = result.unwrap();
    let direct = c.parse_game(2, &ReadOptions::default()).unwrap();
    assert!(loaded.game.same_content(&direct.game));
    // Out-of-range index is an error, not a panic.
    let eff = h.perform(Effect::ParseCollectionGame {
        request: RequestId(2),
        collection: c,
        index: 99,
    });
    let Message::CollectionGameParsed { result, .. } = eff else {
        panic!()
    };
    assert!(result.is_err());
}
