//! The NAG field of the comment editor (D179, rules 26-27):
//! `parse_nag_field` and `nag_field_text`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::assigning_clones,
    clippy::manual_assert_eq,
    clippy::format_collect,
    clippy::unchecked_time_subtraction,
    clippy::field_reassign_with_default,
    clippy::cast_possible_truncation,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value
)]

mod m2_4_common;

use m2_4_common::*;
use proptest::prelude::*;
use rstest::rstest;

// The 16 buttons of the palette, in the order of comment.tcl:99-120.
const PALETTE: [(&str, u16); 16] = [
    ("!", 1),
    ("?", 2),
    ("!!", 3),
    ("??", 4),
    ("!?", 5),
    ("?!", 6),
    ("+-", 18),
    ("-+", 19),
    ("+/-", 16),
    ("-/+", 17),
    ("+=", 14),
    ("=+", 15),
    ("=", 10),
    ("~", 13),
    ("N", 146),
    ("D", 201),
];

#[test]
fn every_palette_symbol_is_accepted_and_maps_to_its_number() {
    for (symbol, number) in PALETTE {
        assert_eq!(parse_nag_field(symbol).unwrap(), [nag(number)], "{symbol}");
    }
}

#[test]
fn spec_example_mixes_symbols_dollar_and_plain_numbers() {
    assert_eq!(
        parse_nag_field("! !? +- $18 18 N").unwrap(),
        [nag(1), nag(5), nag(18), nag(146)],
        "duplicates ($18 then 18) are dropped, order is kept"
    );
}

#[rstest]
#[case("")]
#[case("   ")]
#[case("\t\n  \r\n")]
#[case("\u{a0}")]
fn blank_field_means_no_nags(#[case] text: &str) {
    assert_eq!(parse_nag_field(text).unwrap(), []);
}

#[rstest]
#[case("$0")]
#[case("0")]
#[case("$256")]
#[case("256")]
#[case("$99999999999999999999999")]
#[case("$")]
#[case("$-1")]
#[case("-1")]
#[case("$1x")]
#[case("1.5")]
#[case("abc")]
#[case("xyz")]
#[case("x")]
#[case("!!x")]
#[case("!x")]
#[case("+-x")]
#[case("D1")]
#[case("n")]
#[case("\u{0}")]
#[case("\u{1f600}")]
#[case("$１")] // full-width digit
fn invalid_token_is_an_error_carrying_the_token(#[case] token: &str) {
    let err = parse_nag_field(&format!("! {token} ?")).unwrap_err();
    assert_eq!(err.token, token.chars().take(32).collect::<String>());
    // The error does not depend on what comes before or after.
    assert!(parse_nag_field(token).is_err());
}

#[test]
fn first_bad_token_is_reported() {
    assert_eq!(parse_nag_field("! bad worse").unwrap_err().token, "bad");
}

#[test]
fn bounds_of_the_nag_range() {
    assert_eq!(parse_nag_field("$1").unwrap(), [nag(1)]);
    assert_eq!(parse_nag_field("$255").unwrap(), [nag(255)]);
    assert_eq!(parse_nag_field("001").unwrap(), [nag(1)]);
}

#[test]
fn long_token_is_clipped_in_the_error() {
    let long = "q".repeat(5000);
    let err = parse_nag_field(&long).unwrap_err();
    assert_eq!(err.token.chars().count(), 32);
}

#[test]
fn clipping_respects_character_boundaries() {
    let long = "é".repeat(100);
    assert_eq!(
        parse_nag_field(&long).unwrap_err().token.chars().count(),
        32
    );
}

#[test]
fn one_mebibyte_of_text_does_not_panic_or_hang() {
    let many = "1 ".repeat(512 * 1024);
    assert_eq!(parse_nag_field(&many).unwrap(), [nag(1)]);
    let giant = "!".repeat(1 << 20);
    drop(parse_nag_field(&giant));
    let mixed: String = (1..=255)
        .map(|n| format!("${n} "))
        .collect::<String>()
        .repeat(2000);
    assert_eq!(parse_nag_field(&mixed).unwrap().len(), 255);
}

#[test]
fn field_text_of_no_nags_is_empty() {
    assert_eq!(nag_field_text(&[]), "");
}

#[test]
fn field_text_uses_symbols_when_they_read_back_and_dollar_otherwise() {
    let text = nag_field_text(&[nag(1), nag(18), nag(255), nag(146)]);
    let tokens: Vec<&str> = text.split(' ').collect();
    assert_eq!(tokens[0], "!");
    assert_eq!(tokens[1], "+-");
    assert_eq!(tokens[2], "$255");
    assert_eq!(
        parse_nag_field(&text).unwrap(),
        [nag(1), nag(18), nag(255), nag(146)]
    );
}

#[test]
fn every_nag_round_trips_alone() {
    for n in 1..=255u16 {
        let text = nag_field_text(&[nag(n)]);
        assert_eq!(
            parse_nag_field(&text).unwrap(),
            [nag(n)],
            "NAG {n} written as {text:?}"
        );
    }
}

#[test]
fn all_nags_together_round_trip_in_order() {
    let all: Vec<Nag> = (1..=255).map(nag).collect();
    let text = nag_field_text(&all);
    assert_eq!(parse_nag_field(&text).unwrap(), all);
}

#[test]
fn field_text_separates_with_single_spaces_without_edge_blanks() {
    let text = nag_field_text(&[nag(1), nag(2), nag(3)]);
    assert!(!text.starts_with(' ') && !text.ends_with(' ') && !text.contains("  "));
}

proptest! {
    #[test]
    fn round_trip_of_any_distinct_list(numbers in prop::collection::vec(1u16..=255, 0..40)) {
        let mut seen = Vec::new();
        for n in numbers { if !seen.contains(&n) { seen.push(n); } }
        let nags: Vec<Nag> = seen.iter().copied().map(nag).collect();
        prop_assert_eq!(parse_nag_field(&nag_field_text(&nags)).unwrap(), nags);
    }

    #[test]
    fn arbitrary_text_never_panics_and_ok_results_have_no_duplicates(text in "\\PC{0,200}") {
        if let Ok(nags) = parse_nag_field(&text) {
            for (i, a) in nags.iter().enumerate() {
                prop_assert!(!nags[i + 1..].contains(a));
            }
        }
    }

    #[test]
    fn parse_of_text_of_parse_is_stable(text in "[ !?+/=~$0-9ND-]{0,60}") {
        if let Ok(nags) = parse_nag_field(&text) {
            let again = parse_nag_field(&nag_field_text(&nags)).unwrap();
            prop_assert_eq!(again, nags);
        }
    }
}

// ---- the exclusive groups of D180 on the session (palette toggle) ----

fn session_at_e4() -> GameSession {
    let mut g = Game::new();
    main_line(&mut g, "e4");
    let mut s = GameSession::new(g);
    s.update(GameMessage::Navigate(Nav::GameEnd));
    s
}

fn nags_of(s: &GameSession) -> Vec<Nag> {
    s.game().nags(s.cursor()).unwrap().to_vec()
}

#[test]
fn toggle_on_a_present_nag_removes_it_and_other_of_the_group_swaps() {
    let mut s = session_at_e4();
    s.update(GameMessage::ToggleNag(nag(1)));
    assert_eq!(nags_of(&s), [nag(1)]);
    s.update(GameMessage::ToggleNag(nag(2)));
    assert_eq!(nags_of(&s), [nag(2)], "? swaps ! (same group)");
    s.update(GameMessage::ToggleNag(nag(2)));
    assert!(nags_of(&s).is_empty());
}

#[test]
fn evaluation_group_covers_ten_thirteen_and_fourteen_to_nineteen() {
    for member in [10u16, 13, 14, 15, 16, 17, 18, 19] {
        for other in [10u16, 13, 14, 15, 16, 17, 18, 19] {
            if member == other {
                continue;
            }
            let mut s = session_at_e4();
            s.update(GameMessage::AddNag(nag(member)));
            s.update(GameMessage::AddNag(nag(other)));
            assert_eq!(nags_of(&s), [nag(other)], "{member} then {other}");
        }
    }
}

#[test]
fn nags_of_different_groups_coexist() {
    let mut s = session_at_e4();
    for n in [1, 18, 146, 201] {
        s.update(GameMessage::AddNag(nag(n)));
    }
    assert_eq!(nags_of(&s), [nag(1), nag(18), nag(146), nag(201)]);
}

#[test]
fn the_free_text_field_does_not_enforce_groups() {
    let mut s = session_at_e4();
    let nags = parse_nag_field("! ?").unwrap();
    s.update(GameMessage::SetNags(nags.clone()));
    assert_eq!(nags_of(&s), nags, "D180: the text field is free");
}

#[test]
fn nag_buttons_at_the_root_are_refused() {
    let mut s = session_at_e4();
    s.update(GameMessage::Navigate(Nav::Start));
    s.update(GameMessage::ToggleNag(nag(1)));
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
    assert!(!s.is_dirty());
    s.update(GameMessage::SetNags(vec![nag(1)]));
    assert_eq!(s.notice(), Some(&Notice::NoMoveHere));
}
