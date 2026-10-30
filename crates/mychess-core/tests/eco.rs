//! ECO codes (spec m1-2, rule 14; Scid `eco_FromString`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
use mychess_core::{Eco, EcoError};
use proptest::prelude::*;
use rstest::rstest;

fn eco(text: &str) -> Eco {
    text.parse().unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

/// Highest valid Scid code: `E99z4` = 500 * 131.
const MAX_CODE: u16 = 65_500;

#[rstest]
#[case("A00")]
#[case("A01")]
#[case("B91a")]
#[case("E99b2")]
#[case("C10")]
#[case("E99z4")]
#[case("A00a")]
#[case("A00a1")]
#[case("D45z3")]
fn round_trips_canonical_text(#[case] text: &str) {
    assert_eq!(eco(text).to_string(), text);
}

#[rstest]
#[case("C1", "C10")]
#[case("C", "C00")]
#[case("c", "C00")]
#[case("c10", "C10")]
#[case("a41", "A41")]
#[case("b91a", "B91a")]
#[case("e99b2", "E99b2")]
#[case("E", "E00")]
#[case("A0", "A00")]
fn parses_lowercase_and_partial_codes(#[case] input: &str, #[case] canonical: &str) {
    assert_eq!(eco(input), eco(canonical));
    assert_eq!(eco(input).to_string(), canonical);
}

#[rstest]
#[case("A00", 1)]
#[case("A00a", 2)]
#[case("A00a1", 3)]
#[case("A00a4", 6)]
#[case("A00b", 7)]
#[case("A00z4", 131)]
#[case("A01", 132)]
#[case("A10", 1311)]
#[case("B00", 13_101)]
#[case("E00", 52_401)]
#[case("E99", 65_370)]
#[case("E99z4", 65_500)]
fn scid_internal_codes(#[case] text: &str, #[case] code: u16) {
    assert_eq!(eco(text).to_scid_code(), code);
    assert_eq!(Eco::from_scid_code(code), Some(eco(text)));
}

#[test]
fn scid_code_zero_and_overflow_are_not_codes() {
    assert_eq!(Eco::from_scid_code(0), None);
    assert_eq!(Eco::from_scid_code(MAX_CODE + 1), None);
    assert_eq!(Eco::from_scid_code(u16::MAX), None);
    assert!(Eco::from_scid_code(MAX_CODE).is_some());
}

#[test]
fn every_valid_scid_code_round_trips_through_text_and_back() {
    for code in 1..=MAX_CODE {
        let e = Eco::from_scid_code(code).unwrap();
        assert_eq!(e.to_scid_code(), code);
        let text = e.to_string();
        let back: Eco = text
            .parse()
            .unwrap_or_else(|err| panic!("{code} {text}: {err}"));
        assert_eq!(back, e, "code {code} text {text}");
        assert_eq!(back.to_scid_code(), code);
        // Format: letter A-E, two digits, optional a-z, optional 1-4.
        let b = text.as_bytes();
        assert!(matches!(b[0], b'A'..=b'E'), "{text}");
        assert!(b[1].is_ascii_digit() && b[2].is_ascii_digit(), "{text}");
        assert!(matches!(text.len(), 3..=5), "{text}");
    }
}

#[test]
fn text_codes_are_in_the_same_order_as_numeric_codes() {
    let mut prev: Option<Eco> = None;
    for code in 1..=MAX_CODE {
        let e = Eco::from_scid_code(code).unwrap();
        if let Some(p) = prev {
            assert!(p < e);
        }
        prev = Some(e);
    }
}

#[rstest]
#[case("")]
#[case("F00")]
#[case("Z99")]
#[case("f")]
#[case("0")]
#[case("00")]
#[case("1A0")]
#[case(" A00")]
#[case("A00 ")]
#[case("AB")]
#[case("A0x")]
#[case("A5x")]
#[case("A-1")]
#[case("A00A")]
#[case("A00a0")]
#[case("A00a5")]
#[case("A00a12")]
#[case("A00ab")]
#[case("A00 a")]
#[case("A001")]
#[case("A00a1x")]
#[case("B91xyz")]
#[case("Á00")]
#[case("A0é")]
#[case("A00é")]
#[case("\0")]
fn rejects_invalid_text(#[case] text: &str) {
    assert!(
        matches!(text.parse::<Eco>(), Err(EcoError::Syntax { .. })),
        "{text:?}"
    );
}

#[test]
fn basic_and_is_extended() {
    assert_eq!(eco("B91a").basic(), eco("B91"));
    assert_eq!(eco("B91a2").basic(), eco("B91"));
    assert_eq!(eco("B91").basic(), eco("B91"));
    assert!(eco("B91a").is_extended());
    assert!(eco("E99z4").is_extended());
    assert!(!eco("B91").is_extended());
    assert!(!eco("A00").is_extended());
    assert_eq!(eco("A00").basic().to_scid_code(), 1);
    assert_eq!(eco("E99z4").basic().to_string(), "E99");
}

#[test]
fn partial_code_is_not_extended() {
    assert!(!eco("C").is_extended());
    assert!(!eco("C1").is_extended());
}

/// Scid accepts trailing junk after a valid ECO (`eco_FromString` ignores
/// what follows, e.g. `A00a5` is `A00a`); this crate rejects it on purpose.
#[test]
fn observation_trailing_junk_is_rejected_unlike_scid() {
    assert!("A00a5".parse::<Eco>().is_err());
    assert!("B91 extra".parse::<Eco>().is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_never_panics_and_ok_implies_canonical_round_trip(s in any::<String>()) {
        if let Ok(e) = s.parse::<Eco>() {
            prop_assert_eq!(e.to_string().parse::<Eco>().unwrap(), e);
        }
    }

    #[test]
    fn eco_shaped_text_never_panics(s in "[A-Ea-ez0-9 ]{0,7}") {
        let _ = s.parse::<Eco>();
    }

    #[test]
    fn from_scid_code_is_total(code in any::<u16>()) {
        match Eco::from_scid_code(code) {
            Some(e) => {
                prop_assert!((1..=MAX_CODE).contains(&code));
                prop_assert_eq!(e.to_scid_code(), code);
            }
            None => prop_assert!(code == 0 || code > MAX_CODE),
        }
    }
}

// ---------- parse_prefix (Scid-compatible eco_FromString) ----------

#[test]
fn parse_prefix_accepts_a_valid_prefix_and_returns_the_rest() {
    assert_eq!(
        Eco::parse_prefix("B90 Sicilian"),
        Some((eco("B90"), " Sicilian"))
    );
    assert_eq!(Eco::parse_prefix("E99b2"), Some((eco("E99b2"), "")));
    assert_eq!(Eco::parse_prefix("c1"), Some((eco("C10"), "")));
    assert_eq!(Eco::parse_prefix("Z00"), None);
    assert_eq!(Eco::parse_prefix("A0x"), None);
}

proptest! {
    #[test]
    fn parse_prefix_agrees_with_from_str_on_valid_codes(code in 1u16..=MAX_CODE) {
        let e = Eco::from_scid_code(code).unwrap();
        let text = e.to_string();
        prop_assert_eq!(Eco::parse_prefix(&text), Some((e, "")));
        let with_junk = format!("{text} !");
        prop_assert_eq!(Eco::parse_prefix(&with_junk), Some((e, " !")));
    }

    #[test]
    fn parse_prefix_never_panics(s in any::<String>()) {
        let _ = Eco::parse_prefix(&s);
    }
}
