//! Partial dates (spec m1-2, rule 13; Scid `date.cpp`, `strTrimDate`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
use mychess_core::{DateError, MAX_YEAR, PartialDate};
use proptest::prelude::*;
use rstest::rstest;

fn date(y: Option<u16>, m: Option<u8>, d: Option<u8>) -> PartialDate {
    PartialDate::new(y, m, d).unwrap()
}

// ---------- construction and Display ----------

#[test]
fn unknown_date_prints_question_marks() {
    assert_eq!(PartialDate::UNKNOWN.to_string(), "????.??.??");
    assert_eq!(PartialDate::default(), PartialDate::UNKNOWN);
}

#[rstest]
#[case(None, None, None, "????.??.??")]
#[case(Some(2024), None, None, "2024.??.??")]
#[case(Some(2024), Some(5), None, "2024.05.??")]
#[case(Some(2024), Some(5), Some(17), "2024.05.17")]
#[case(Some(5), Some(1), Some(1), "0005.01.01")]
#[case(Some(2047), Some(12), Some(31), "2047.12.31")]
#[case(None, Some(5), Some(17), "????.05.17")]
#[case(None, None, Some(9), "????.??.09")]
fn displays_as_pgn_date(
    #[case] y: Option<u16>,
    #[case] m: Option<u8>,
    #[case] d: Option<u8>,
    #[case] text: &str,
) {
    assert_eq!(date(y, m, d).to_string(), text);
}

#[test]
fn zero_parts_mean_unknown() {
    let d = date(Some(0), Some(0), Some(0));
    assert_eq!(d, PartialDate::UNKNOWN);
    assert_eq!(d.year(), None);
}

#[test]
fn accessors_return_the_parts() {
    let d = date(Some(1999), Some(12), Some(31));
    assert_eq!(
        (d.year(), d.month(), d.day()),
        (Some(1999), Some(12), Some(31))
    );
}

#[test]
fn new_rejects_out_of_range_parts() {
    assert_eq!(MAX_YEAR, 2047);
    assert_eq!(
        PartialDate::new(Some(2048), None, None),
        Err(DateError::Year(2048))
    );
    assert_eq!(
        PartialDate::new(None, Some(13), None),
        Err(DateError::Month(13))
    );
    assert_eq!(
        PartialDate::new(None, None, Some(32)),
        Err(DateError::Day(32))
    );
    assert!(PartialDate::new(Some(2047), Some(12), Some(31)).is_ok());
}

#[test]
fn day_is_not_checked_against_month() {
    assert!(PartialDate::new(Some(2023), Some(2), Some(31)).is_ok());
    assert_eq!(
        date(Some(2023), Some(2), Some(30)).to_string(),
        "2023.02.30"
    );
}

// ---------- strict FromStr round-trip ----------

#[rstest]
#[case("????.??.??")]
#[case("2024.??.??")]
#[case("2024.05.??")]
#[case("2024.05.17")]
#[case("????.05.17")]
#[case("0001.01.01")]
fn from_str_round_trips_canonical_text(#[case] text: &str) {
    let d: PartialDate = text.parse().unwrap();
    assert_eq!(d.to_string(), text);
}

#[rstest]
#[case("2048.01.01")]
#[case("2024.13.01")]
#[case("2024.05.32")]
#[case("2024.0?.17")]
#[case("20??.05.17")]
#[case("abcd.05.17")]
#[case("2024-05-17")]
#[case("2024.05.17.01")]
#[case("2024.05.17.")]
#[case(" 2024.05.17")]
#[case("2024.05.17 ")]
#[case("-1.05.17")]
#[case("２０２４.05.17")]
fn from_str_rejects_invalid_text(#[case] text: &str) {
    assert!(
        matches!(text.parse::<PartialDate>(), Err(DateError::Syntax { .. })),
        "{text:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn scid_u32_round_trips(
        y in proptest::option::of(0u16..=2047),
        m in proptest::option::of(0u8..=12),
        d in proptest::option::of(0u8..=31),
    ) {
        let original = PartialDate::new(y, m, d).unwrap();
        prop_assert_eq!(PartialDate::from_scid_u32(original.to_scid_u32()), Some(original));
    }

    #[test]
    fn from_scid_u32_never_panics_and_re_encodes(v in any::<u32>()) {
        if let Some(d) = PartialDate::from_scid_u32(v) {
            prop_assert_eq!(d.to_scid_u32(), v);
        }
    }

    #[test]
    fn display_then_from_str_is_identity(
        y in proptest::option::of(0u16..=2047),
        m in proptest::option::of(0u8..=12),
        d in proptest::option::of(0u8..=31),
    ) {
        let original = PartialDate::new(y, m, d).unwrap();
        let text = original.to_string();
        prop_assert_eq!(text.parse::<PartialDate>().unwrap(), original);
        prop_assert_eq!(PartialDate::parse_lenient(&text), original);
        prop_assert!(PartialDate::is_valid_str(&text));
        prop_assert_eq!(text.len(), 10);
    }

    #[test]
    fn parse_lenient_never_panics_and_yields_a_displayable_date(s in any::<String>()) {
        let d = PartialDate::parse_lenient(&s);
        prop_assert_eq!(d.to_string().len(), 10);
        prop_assert!(d.year().is_none_or(|y| y <= MAX_YEAR));
        prop_assert!(d.month().is_none_or(|m| m <= 12));
        prop_assert!(d.day().is_none_or(|x| x <= 31));
    }

    #[test]
    fn date_like_soup_never_panics(s in "[0-9?.a-z -]{0,20}") {
        let valid = PartialDate::is_valid_str(&s);
        prop_assert_eq!(s.parse::<PartialDate>().is_ok(), valid);
        let _ = PartialDate::parse_lenient(&s);
    }

    #[test]
    fn valid_text_parses_leniently_to_the_same_as_strictly(
        y in "[0-9?]{0,4}", m in "[0-9?]{0,2}", d in "[0-9?]{0,2}",
    ) {
        let text = format!("{y}.{m}.{d}");
        if PartialDate::is_valid_str(&text) {
            prop_assert_eq!(text.parse::<PartialDate>().unwrap(), PartialDate::parse_lenient(&text));
        }
    }

    #[test]
    fn short_is_a_prefix_of_display(
        y in proptest::option::of(0u16..=2047),
        m in proptest::option::of(0u8..=12),
        d in proptest::option::of(0u8..=31),
    ) {
        let dt = PartialDate::new(y, m, d).unwrap();
        prop_assert!(dt.to_string().starts_with(&dt.short()));
    }
}

// ---------- parse_lenient (date_EncodeFromString) ----------

#[rstest]
#[case("2024.05.17", Some(2024), Some(5), Some(17))]
#[case("2024.5.7", Some(2024), Some(5), Some(7))]
#[case("2024.05.??", Some(2024), Some(5), None)]
#[case("2024.??.17", Some(2024), None, Some(17))]
#[case("????.??.??", None, None, None)]
#[case("?", None, None, None)]
#[case("2024", Some(2024), None, None)]
#[case("2024.05", Some(2024), Some(5), None)]
#[case("", None, None, None)]
#[case("2024.13.05", Some(2024), None, None)]
#[case("2024.05.32", Some(2024), Some(5), None)]
#[case("2024.99.99", Some(2024), None, None)]
#[case("2047.12.31", Some(2047), Some(12), Some(31))]
#[case("2024.05.17.99", Some(2024), Some(5), Some(17))]
#[case("garbage", None, None, None)]
#[case("0000.00.00", None, None, None)]
fn parse_lenient_follows_date_encode_from_string(
    #[case] text: &str,
    #[case] y: Option<u16>,
    #[case] m: Option<u8>,
    #[case] d: Option<u8>,
) {
    let got = PartialDate::parse_lenient(text);
    assert_eq!((got.year(), got.month(), got.day()), (y, m, d), "{text:?}");
}

#[test]
fn parse_lenient_with_year_above_2047_and_nothing_else_is_unknown() {
    assert_eq!(PartialDate::parse_lenient("2048"), PartialDate::UNKNOWN);
    assert_eq!(
        PartialDate::parse_lenient("9999.13.01"),
        PartialDate::UNKNOWN
    );
    assert_eq!(
        PartialDate::parse_lenient("99999999999999999999"),
        PartialDate::UNKNOWN
    );
}

/// Scid `date_EncodeFromString` (`date.cpp:61-90`) only zeroes the year when
/// it is above 2047 and keeps decoding month and day, so `2048.05.17` is
/// `????.05.17`. The implementation returns a fully unknown date.
#[test]
fn parse_lenient_year_above_2047_keeps_month_and_day_like_scid() {
    let got = PartialDate::parse_lenient("2048.05.17");
    assert_eq!(
        (got.year(), got.month(), got.day()),
        (None, Some(5), Some(17))
    );
}

// ---------- is_valid_str (date_ValidString) ----------

#[rstest]
#[case("2024.05.17")]
#[case("2024.05")]
#[case("2024")]
#[case("????.??.??")]
#[case("????.??")]
#[case("????")]
#[case("?")]
#[case("")]
#[case("2024.??.17")]
#[case("????.05.??")]
#[case("2047.12.31")]
#[case("0.0.0")]
#[case("2024.02.30")]
#[case("2024.05.")]
#[case("2024..")]
#[case("00002024.05.17")]
#[case("?????.?.?")]
fn is_valid_str_accepts(#[case] text: &str) {
    assert!(PartialDate::is_valid_str(text), "{text:?}");
}

#[rstest]
#[case("2024.0?.17")]
#[case("20?4.05.17")]
#[case("?024")]
#[case("2024.05.1?")]
#[case("abcd")]
#[case("2024.ab.17")]
#[case("2024.05.1a")]
#[case("2048")]
#[case("2048.05.17")]
#[case("2024.13")]
#[case("2024.13.01")]
#[case("2024.05.32")]
#[case("2024.05.17.01")]
#[case("2024.05.17.")]
#[case("2024-05-17")]
#[case(" 2024")]
#[case("2024 ")]
#[case("-5")]
#[case("+5")]
#[case("2024/05/17")]
#[case("2024.05.17\n")]
#[case("é")]
fn is_valid_str_rejects(#[case] text: &str) {
    assert!(!PartialDate::is_valid_str(text), "{text:?}");
}

// ---------- ordering ----------

#[test]
fn unknown_sorts_before_any_known_date() {
    let known = date(Some(1), None, None);
    assert!(PartialDate::UNKNOWN < known);
    assert!(PartialDate::UNKNOWN < date(None, Some(1), Some(1)));
}

#[test]
fn unknown_month_and_day_sort_before_known_ones_within_a_year() {
    let y = date(Some(2024), None, None);
    let ym = date(Some(2024), Some(1), None);
    let ymd = date(Some(2024), Some(1), Some(1));
    assert!(y < ym && ym < ymd);
    assert!(date(Some(2023), Some(12), Some(31)) < y);
    assert!(ymd < date(Some(2024), Some(2), None));
}

#[test]
fn sorting_matches_scid_packed_encoding() {
    // Scid packs year<<9 | month<<5 | day, with 0 meaning unknown.
    let key = |d: PartialDate| {
        (u32::from(d.year().unwrap_or(0)) << 9)
            | (u32::from(d.month().unwrap_or(0)) << 5)
            | u32::from(d.day().unwrap_or(0))
    };
    let mut dates = vec![
        date(Some(2024), Some(5), Some(17)),
        PartialDate::UNKNOWN,
        date(Some(2024), None, None),
        date(Some(1990), Some(1), Some(1)),
        date(None, Some(6), Some(6)),
        date(Some(2024), Some(5), None),
    ];
    let mut by_key = dates.clone();
    dates.sort();
    by_key.sort_by_key(|d| key(*d));
    assert_eq!(dates, by_key);
}

// ---------- short (strTrimDate) ----------

#[rstest]
#[case(None, None, None, "????")]
#[case(Some(2024), None, None, "2024")]
#[case(Some(2024), Some(5), None, "2024.05")]
#[case(Some(2024), Some(5), Some(17), "2024.05.17")]
#[case(Some(2024), None, Some(17), "2024.??.17")]
#[case(None, Some(5), None, "????.05")]
#[case(None, None, Some(9), "????.??.09")]
fn short_trims_unknown_trailing_parts(
    #[case] y: Option<u16>,
    #[case] m: Option<u8>,
    #[case] d: Option<u8>,
    #[case] expected: &str,
) {
    assert_eq!(date(y, m, d).short(), expected);
}
