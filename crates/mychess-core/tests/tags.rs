//! Header tags, ratings and results (spec m1-2, rules 11, 12, 15, 16; D11, D15).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
use mychess_core::{
    Eco, Elo, GameResult, MAX_ELO, PartialDate, Rating, RatingType, TagError, Tags,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;

fn pairs(t: &Tags) -> Vec<(String, String)> {
    t.pgn_pairs()
}

fn p(k: &str, v: &str) -> (String, String) {
    (k.to_owned(), v.to_owned())
}

fn keys(t: &Tags) -> Vec<String> {
    t.pgn_pairs().into_iter().map(|(k, _)| k).collect()
}

fn rating(value: u16, kind: RatingType) -> Rating {
    Rating::new(Elo::new(value).unwrap(), kind)
}

fn date(text: &str) -> PartialDate {
    text.parse().unwrap()
}

// ---------- rule 11: defaults ----------

#[test]
fn new_tags_have_scid_defaults() {
    let t = Tags::new();
    for field in [&t.event, &t.site, &t.white, &t.black, &t.round] {
        assert_eq!(field, "?");
    }
    assert_eq!(t.date, PartialDate::UNKNOWN);
    assert_eq!(t.event_date, PartialDate::UNKNOWN);
    assert_eq!(t.result, GameResult::Unknown);
    assert_eq!(t.white_rating, None);
    assert_eq!(t.black_rating, None);
    assert_eq!(t.eco, None);
    assert!(t.extras().is_empty());
    assert_eq!(t, Tags::default());
}

// ---------- rule 12: order of pgn_pairs ----------

#[test]
fn default_tags_write_only_the_seven_tag_roster_in_order() {
    assert_eq!(
        pairs(&Tags::new()),
        vec![
            p("Event", "?"),
            p("Site", "?"),
            p("Date", "????.??.??"),
            p("Round", "?"),
            p("White", "?"),
            p("Black", "?"),
            p("Result", "*"),
        ]
    );
}

#[test]
fn seven_tag_roster_uses_the_field_values() {
    let mut t = Tags::new();
    t.event = "Candidates".into();
    t.site = "Toronto CAN".into();
    t.date = date("2024.04.03");
    t.round = "4.1".into();
    t.white = "Gukesh, D".into();
    t.black = "Nakamura, H".into();
    t.result = GameResult::Draw;
    assert_eq!(
        pairs(&t),
        vec![
            p("Event", "Candidates"),
            p("Site", "Toronto CAN"),
            p("Date", "2024.04.03"),
            p("Round", "4.1"),
            p("White", "Gukesh, D"),
            p("Black", "Nakamura, H"),
            p("Result", "1/2-1/2"),
        ]
    );
}

#[test]
fn full_header_has_ratings_then_eco_then_event_date_then_extras() {
    let mut t = Tags::new();
    t.white_rating = Some(rating(2800, RatingType::Elo));
    t.black_rating = Some(rating(2750, RatingType::Elo));
    t.eco = Some("B91a".parse::<Eco>().unwrap());
    t.event_date = date("2024.04.01");
    t.set_extra("Annotator", "me").unwrap();
    t.set_extra("PlyCount", "81").unwrap();
    assert_eq!(
        keys(&t),
        [
            "Event",
            "Site",
            "Date",
            "Round",
            "White",
            "Black",
            "Result",
            "WhiteElo",
            "BlackElo",
            "ECO",
            "EventDate",
            "Annotator",
            "PlyCount"
        ]
    );
    let all = pairs(&t);
    assert_eq!(all[7], p("WhiteElo", "2800"));
    assert_eq!(all[8], p("BlackElo", "2750"));
    assert_eq!(all[9], p("ECO", "B91a"));
    assert_eq!(all[10], p("EventDate", "2024.04.01"));
    assert_eq!(all[11], p("Annotator", "me"));
}

#[test]
fn only_black_rating_is_written_when_white_has_none() {
    let mut t = Tags::new();
    t.black_rating = Some(rating(1900, RatingType::Uscf));
    assert_eq!(pairs(&t)[7], p("BlackUSCF", "1900"));
    assert_eq!(pairs(&t).len(), 8);
}

#[test]
fn rating_of_zero_is_not_written() {
    let mut t = Tags::new();
    t.white_rating = Some(rating(0, RatingType::Elo));
    t.black_rating = Some(rating(0, RatingType::Dwz));
    assert_eq!(pairs(&t), pairs(&Tags::new()));
}

#[test]
fn eco_is_written_in_extended_form_and_basic_form() {
    let mut t = Tags::new();
    t.eco = Some("E99b2".parse().unwrap());
    assert_eq!(pairs(&t)[7], p("ECO", "E99b2"));
    t.eco = Some("A00".parse().unwrap());
    assert_eq!(pairs(&t)[7], p("ECO", "A00"));
}

#[test]
fn unknown_event_date_is_omitted_and_partial_one_is_written() {
    let mut t = Tags::new();
    assert!(!keys(&t).contains(&"EventDate".to_owned()));
    t.event_date = date("2024.??.??");
    assert_eq!(pairs(&t)[7], p("EventDate", "2024.??.??"));
}

#[test]
fn fen_is_never_written_by_tags() {
    let t = Tags::new();
    assert!(!keys(&t).iter().any(|k| k == "FEN" || k == "SetUp"));
}

#[rstest]
#[case(RatingType::Elo, "Elo")]
#[case(RatingType::Rating, "Rating")]
#[case(RatingType::Rapid, "Rapid")]
#[case(RatingType::Iccf, "ICCF")]
#[case(RatingType::Uscf, "USCF")]
#[case(RatingType::Dwz, "DWZ")]
#[case(RatingType::Ecf, "ECF")]
fn white_and_black_tag_names_follow_the_rating_type(#[case] kind: RatingType, #[case] name: &str) {
    assert_eq!(kind.name(), name);
    assert_eq!(kind.to_string(), name);
    assert_eq!(RatingType::from_name(name), Some(kind));
    let mut t = Tags::new();
    t.white_rating = Some(rating(2100, kind));
    t.black_rating = Some(rating(1234, kind));
    let all = pairs(&t);
    assert_eq!(all[7], p(&format!("White{name}"), "2100"));
    assert_eq!(all[8], p(&format!("Black{name}"), "1234"));
}

#[test]
fn white_and_black_can_have_different_rating_kinds() {
    let mut t = Tags::new();
    t.white_rating = Some(rating(2100, RatingType::Dwz));
    t.black_rating = Some(rating(2000, RatingType::Ecf));
    let all = pairs(&t);
    assert_eq!(
        (all[7].0.as_str(), all[8].0.as_str()),
        ("WhiteDWZ", "BlackECF")
    );
}

#[test]
fn rating_types_are_seven_distinct_names_in_scid_order() {
    let names: Vec<&str> = RatingType::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(
        names,
        ["Elo", "Rating", "Rapid", "ICCF", "USCF", "DWZ", "ECF"]
    );
    assert_eq!(RatingType::default(), RatingType::Elo);
}

#[rstest]
#[case("elo")]
#[case("ELO")]
#[case("Iccf")]
#[case("")]
#[case("Foo")]
#[case("Elo ")]
#[case("WhiteElo")]
fn rating_type_from_name_is_exact(#[case] name: &str) {
    assert_eq!(RatingType::from_name(name), None);
}

// ---------- D15: Elo ----------

#[test]
fn elo_accepts_zero_to_4000() {
    assert_eq!(MAX_ELO, 4000);
    assert_eq!(Elo::new(0).unwrap().get(), 0);
    assert_eq!(Elo::new(2882).unwrap().get(), 2882);
    assert_eq!(Elo::new(4000).unwrap().get(), 4000);
    assert_eq!(Elo::new(4000).unwrap().to_string(), "4000");
}

#[rstest]
#[case(4001)]
#[case(4096)]
#[case(9999)]
#[case(u16::MAX)]
fn elo_above_4000_is_an_error(#[case] v: u16) {
    assert_eq!(Elo::new(v), Err(TagError::Elo(v)));
}

#[test]
fn elo_orders_numerically() {
    assert!(Elo::new(999).unwrap() < Elo::new(1000).unwrap());
}

// ---------- rule 16 / D11: extra tags ----------

#[test]
fn extra_tags_keep_insertion_order() {
    let mut t = Tags::new();
    t.set_extra("Zeta", "1").unwrap();
    t.set_extra("Alpha", "2").unwrap();
    t.set_extra("Mid", "3").unwrap();
    let names: Vec<&str> = t.extras().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, ["Zeta", "Alpha", "Mid"]);
    assert_eq!(t.get_extra("Alpha"), Some("2"));
    assert_eq!(t.get_extra("Nope"), None);
}

#[test]
fn duplicate_extra_name_replaces_value_in_place() {
    let mut t = Tags::new();
    t.set_extra("A", "1").unwrap();
    t.set_extra("B", "2").unwrap();
    t.set_extra("A", "3").unwrap();
    assert_eq!(t.extras(), [p("A", "3"), p("B", "2")]);
    assert_eq!(t.extras().len(), 2);
}

#[test]
fn extra_names_are_case_sensitive() {
    let mut t = Tags::new();
    t.set_extra("Annotator", "x").unwrap();
    t.set_extra("annotator", "y").unwrap();
    assert_eq!(t.extras().len(), 2);
    assert_eq!(t.get_extra("Annotator"), Some("x"));
    assert_eq!(t.get_extra("annotator"), Some("y"));
}

#[test]
fn remove_extra_returns_the_value_and_keeps_the_rest_in_order() {
    let mut t = Tags::new();
    t.set_extra("A", "1").unwrap();
    t.set_extra("B", "2").unwrap();
    t.set_extra("C", "3").unwrap();
    assert_eq!(t.remove_extra("B"), Some("2".to_owned()));
    assert_eq!(t.remove_extra("B"), None);
    assert_eq!(t.extras(), [p("A", "1"), p("C", "3")]);
    assert_eq!(t.remove_extra("zzz"), None);
}

#[test]
fn extra_values_may_be_empty_or_contain_quotes_and_unicode() {
    let mut t = Tags::new();
    t.set_extra("Empty", "").unwrap();
    t.set_extra("Quote", "say \"hi\"").unwrap();
    t.set_extra("Uni", "Zürich ♞").unwrap();
    assert_eq!(t.get_extra("Empty"), Some(""));
    assert_eq!(t.get_extra("Quote"), Some("say \"hi\""));
    assert_eq!(t.get_extra("Uni"), Some("Zürich ♞"));
}

#[test]
fn non_ascii_extra_name_is_rejected() {
    let mut t = Tags::new();
    assert!(matches!(
        t.set_extra("Évènement", "x"),
        Err(TagError::InvalidName { .. })
    ));
}

#[test]
fn no_limit_of_40_extras_or_255_bytes_in_the_core() {
    let mut t = Tags::new();
    for i in 0..100 {
        t.set_extra(&format!("Tag{i}"), &"v".repeat(300)).unwrap();
    }
    assert_eq!(t.extras().len(), 100);
    assert_eq!(t.get_extra("Tag99").map(str::len), Some(300));
}

#[rstest]
#[case("")]
#[case("a\"b")]
#[case("\"")]
#[case("a\nb")]
#[case("a\rb")]
#[case("\n")]
#[case("a b")]
#[case("a[b")]
#[case("a]b")]
#[case("a\0b")]
fn invalid_extra_names_are_rejected(#[case] name: &str) {
    let mut t = Tags::new();
    assert!(
        matches!(t.set_extra(name, "v"), Err(TagError::InvalidName { .. })),
        "{name:?}"
    );
    assert!(t.extras().is_empty());
}

#[rstest]
#[case("Event")]
#[case("Site")]
#[case("Date")]
#[case("Round")]
#[case("White")]
#[case("Black")]
#[case("Result")]
#[case("ECO")]
#[case("EventDate")]
#[case("FEN")]
fn names_with_dedicated_fields_are_reserved(#[case] name: &str) {
    let mut t = Tags::new();
    assert!(
        matches!(t.set_extra(name, "v"), Err(TagError::Reserved { .. })),
        "{name}"
    );
    assert!(t.extras().is_empty());
}

#[rstest]
#[case("Annotator")]
#[case("Opening")]
#[case("Variation")]
#[case("PlyCount")]
#[case("SetUp")]
#[case("WhiteCountry")]
#[case("WhiteTitle")]
#[case("BlackNA")]
#[case("Eco")]
#[case("event")]
#[case("WhiteElo2")]
#[case("Whiteelo")]
#[case("WhiteElo")]
#[case("BlackElo")]
#[case("WhiteRating")]
#[case("BlackRapid")]
#[case("WhiteICCF")]
#[case("BlackUSCF")]
#[case("WhiteDWZ")]
#[case("BlackECF")]
fn other_names_are_accepted_as_extras(#[case] name: &str) {
    let mut t = Tags::new();
    t.set_extra(name, "v").unwrap();
    assert_eq!(t.get_extra(name), Some("v"));
}

#[test]
fn a_rejected_extra_leaves_existing_extras_untouched() {
    let mut t = Tags::new();
    t.set_extra("A", "1").unwrap();
    let before = t.clone();
    assert!(t.set_extra("Event", "x").is_err());
    assert!(t.set_extra("", "x").is_err());
    assert_eq!(t, before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn set_extra_never_panics_and_get_returns_the_last_value(
        name in any::<String>(), v1 in any::<String>(), v2 in any::<String>(),
    ) {
        let mut t = Tags::new();
        if t.set_extra(&name, &v1).is_ok() {
            t.set_extra(&name, &v2).unwrap();
            prop_assert_eq!(t.get_extra(&name), Some(v2.as_str()));
            prop_assert_eq!(t.extras().len(), 1);
            prop_assert!(!name.is_empty() && !name.contains(['"', '\n', '\r']));
        }
        let _ = t.pgn_pairs();
    }

    #[test]
    fn pgn_pairs_always_starts_with_the_roster_and_ends_with_extras(
        names in proptest::collection::vec("[A-Za-z]{1,8}", 0..6),
        w in proptest::option::of(0u16..=4000),
        b in proptest::option::of(0u16..=4000),
    ) {
        let mut t = Tags::new();
        t.white_rating = w.map(|v| rating(v, RatingType::Elo));
        t.black_rating = b.map(|v| rating(v, RatingType::Rapid));
        for n in &names {
            let _ = t.set_extra(n, "x");
        }
        let all = t.pgn_pairs();
        let head: Vec<&str> = all.iter().take(7).map(|(k, _)| k.as_str()).collect();
        prop_assert_eq!(head, ["Event", "Site", "Date", "Round", "White", "Black", "Result"]);
        let tail: Vec<(String, String)> = all[all.len() - t.extras().len()..].to_vec();
        prop_assert_eq!(tail.as_slice(), t.extras());
    }
}

// ---------- rule 15: GameResult ----------

#[test]
fn game_result_strings() {
    assert_eq!(GameResult::Unknown.as_str(), "*");
    assert_eq!(GameResult::White.as_str(), "1-0");
    assert_eq!(GameResult::Black.as_str(), "0-1");
    assert_eq!(GameResult::Draw.as_str(), "1/2-1/2");
    assert_eq!(GameResult::Draw.to_string(), "1/2-1/2");
    assert_eq!(GameResult::default(), GameResult::Unknown);
}

#[rstest]
#[case("1-0", GameResult::White)]
#[case("0-1", GameResult::Black)]
#[case("1/2-1/2", GameResult::Draw)]
#[case("*", GameResult::Unknown)]
#[case("1:0", GameResult::White)]
#[case("0:1", GameResult::Black)]
#[case("1/2", GameResult::Draw)]
#[case("1/2:1/2", GameResult::Draw)]
#[case("1-0 (resigned)", GameResult::White)]
#[case("0-1 on time", GameResult::Black)]
#[case("1/2 draw agreed", GameResult::Draw)]
#[case("  1-0", GameResult::White)]
#[case("0-1\n", GameResult::Black)]
#[case("\t1/2-1/2 ", GameResult::Draw)]
#[case("=-=", GameResult::Unknown)]
#[case("", GameResult::Unknown)]
#[case("   ", GameResult::Unknown)]
#[case("2-0", GameResult::Unknown)]
#[case("01", GameResult::Unknown)]
#[case("10", GameResult::Unknown)]
#[case("1 - 0", GameResult::Unknown)]
#[case("1/ 2", GameResult::Unknown)]
#[case("½-½", GameResult::Unknown)]
#[case("1–0", GameResult::Unknown)]
#[case("draw", GameResult::Unknown)]
#[case("?", GameResult::Unknown)]
#[case("x1-0", GameResult::Unknown)]
fn from_str_lenient_reads_prefixes(#[case] text: &str, #[case] expected: GameResult) {
    assert_eq!(GameResult::from_str_lenient(text), expected, "{text:?}");
}

#[test]
fn game_result_round_trips_through_its_string() {
    for r in [
        GameResult::Unknown,
        GameResult::White,
        GameResult::Black,
        GameResult::Draw,
    ] {
        assert_eq!(GameResult::from_str_lenient(r.as_str()), r);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn from_str_lenient_never_panics(s in any::<String>()) {
        let _ = GameResult::from_str_lenient(&s);
    }

    #[test]
    fn from_str_lenient_ignores_anything_after_the_prefix(
        r in prop::sample::select(vec![GameResult::White, GameResult::Black, GameResult::Draw]),
        tail in "[ -~]{0,10}",
    ) {
        let text = format!("{}{}", r.as_str(), tail);
        prop_assert_eq!(GameResult::from_str_lenient(&text), r);
    }
}
