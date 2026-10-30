//! Contract tests of matching, comments, titles and Elo lookup (M6-3a):
//! rules 16 to 36, 60 to 64 of `docs/specs/m6-3-jogadores-torneios-spelling.md`.
//! Synthetic data only.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_3a_support.rs"]
mod support;

use std::fmt::Write as _;
use std::time::Instant;

use mychess_core::PartialDate;
use mychess_db::crosstable::PlayerBioProvider;
use mychess_db::si4::NameKind;
use mychess_db::spelling::elo::{period_position, positions_in_year};
use mychess_db::spelling::{PlayerComment, SpellChecker};
use mychess_db::{Title, TitleProvider};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{date, read};

fn player(text: &str) -> SpellChecker {
    read(&format!("@PLAYER \"\"\n{text}"))
        .checker(NameKind::Player)
        .clone()
}

/// Entries by file index:
/// 0 Kova, 1 Kovac, 2 Kovacs, 3 Kovalev, 4 alias `Kovac` -> Kovalev.
const KOV: &str = "Kova\nKovac\nKovacs\nKovalev\n= Kovac\n";

// ---------------------------------------------------------------------------
// Rules 25 to 29.
// ---------------------------------------------------------------------------

#[test]
fn an_exact_match_of_the_highest_file_index_ends_the_scan() {
    let p = player(KOV);
    // Index 4 (alias, exact) is scanned first and wins over index 1 (exact).
    assert_eq!(p.correct("Kovac"), Some("Kovalev"));
    assert_eq!(p.corrections("Kovac", 10), ["Kovalev"]);
}

#[test]
fn without_an_exact_match_the_lowest_file_index_wins() {
    let p = player(KOV);
    assert_eq!(p.correct("Kov"), Some("Kova"));
    assert_eq!(p.correct("Kovale"), Some("Kovalev"));
    assert_eq!(p.correct("K"), Some("Kova"));
}

#[test]
fn corrections_follow_the_decreasing_file_order_without_repeats() {
    let p = player(KOV);
    assert_eq!(
        p.corrections("Kov", 10),
        ["Kovalev", "Kovacs", "Kovac", "Kova"]
    );
    assert_eq!(p.corrections("Kov", 2), ["Kovalev", "Kovacs"]);
    assert_eq!(p.corrections("Kov", 1), ["Kovalev"]);
    assert_eq!(p.corrections("Kov", 0).len(), 0);
}

#[test]
fn non_consecutive_repeats_are_removed_keeping_the_first() {
    // Raw scan order: Alpha(3), Beta(2), Alpha(1)/(0): the Scid would list
    // Alpha twice; D3305 lists it once.
    let p = player("Zora Alpha\n= Zora Alpha1\nZora Beta\nZora Alpha\n");
    assert_eq!(p.corrections("Zora", 10), ["Zora Alpha", "Zora Beta"]);
    assert_eq!(p.corrections("Zora", 2), ["Zora Alpha", "Zora Beta"]);
}

#[test]
fn an_exact_match_after_other_candidates_returns_only_itself() {
    // Scan order: Kovalev-alias(4, exact for "Kovac") comes before the others,
    // so build a case where prefix matches come first: Ab(0) Abc(1) Abcd(2).
    let p = player("Ab\nAbc\nAbcd\n");
    // "Abc": Abcd (prefix, scanned first), then Abc (exact) -> only Abc.
    assert_eq!(p.corrections("Abc", 10), ["Abc"]);
    assert_eq!(p.correct("Abc"), Some("Abc"));
}

#[test]
fn the_direction_of_the_prefix_is_query_shorter_than_entry() {
    let p = player("Kovac\n");
    assert_eq!(p.correct("Kov"), Some("Kovac"));
    assert_eq!(
        p.correct("Kovacs"),
        None,
        "a longer query does not match a shorter entry"
    );
    assert_eq!(
        p.correct("Kovac "),
        None,
        "without excluded characters the space counts"
    );
}

#[test]
fn case_counts_everywhere() {
    let p = player("Kovac\n");
    for q in ["kovac", "KOVAC", "kov", "K OVAC"] {
        assert_eq!(p.correct(q), None, "{q}");
        assert_eq!(p.corrections(q, 10).len(), 0, "{q}");
        assert_eq!(p.comment_loose(q), None, "{q}");
    }
}

#[test]
fn queries_ignore_the_excluded_characters_of_the_kind() {
    let set = read("@PLAYER \"., -\"\nKovac, Anna-Maria\n@SITE \"()\"\nRio (BRA)\n");
    let p = set.checker(NameKind::Player);
    for q in ["Kovac Anna Maria", "KovacAnna", "K.o.v.a.c", "Kovac,Anna-M"] {
        assert_eq!(p.correct(q), Some("Kovac, Anna-Maria"), "{q}");
    }
    // The player set does not exclude parentheses.
    assert_eq!(
        set.checker(NameKind::Site).correct("Rio BRA"),
        Some("Rio (BRA)")
    );
    assert_eq!(set.checker(NameKind::Site).correct("Rio, BRA"), None);
}

#[rstest]
#[case("")]
#[case(".")]
#[case(".,-")]
#[case("   ")]
fn an_empty_key_matches_nothing(#[case] query: &str) {
    let set = read("@PLAYER \"., -\"\nKovac\n= Kovacs\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.correct(query), None);
    assert_eq!(p.corrections(query, 10).len(), 0);
    assert_eq!(p.comment_loose(query), None);
    assert_eq!(p.bio(query).len(), 0);
    assert!(!p.match_is_exact(query));
}

#[test]
fn comment_exact_wants_the_whole_correct_name_with_the_excluded_characters() {
    let set = read("@PLAYER \"., \"\nKovac, Anna #GM X [1]\n= Kovac A\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.comment_exact("Kovac, Anna"), Some("GM X [1]"));
    assert_eq!(p.comment_exact("KovacAnna"), None, "key, not name");
    assert_eq!(p.comment_exact("Kovac A"), None, "alias");
    assert_eq!(p.comment_exact("Kovac"), None, "prefix");
    assert_eq!(p.comment_loose("Kovac A"), Some("GM X [1]"));
    assert_eq!(p.comment_loose("Kovac"), Some("GM X [1]"));
}

#[test]
fn comment_loose_takes_the_lowest_index_prefix_match_or_the_exact_one() {
    let p = player("Smith #first\nSmithson #second\nSmithy #third\n");
    assert_eq!(p.comment_loose("Smi"), Some("first"));
    assert_eq!(p.comment_loose("Smithso"), Some("second"));
    assert_eq!(p.comment_loose("Smithy"), Some("third"));
    assert_eq!(p.comment_loose("Smith"), Some("first"), "exact");
    assert!(p.match_is_exact("Smith"));
    assert!(!p.match_is_exact("Smi"));
}

// ---------------------------------------------------------------------------
// Rule 32: bio(name).
// ---------------------------------------------------------------------------

#[test]
fn bio_of_a_prefix_match_is_the_one_of_the_lowest_index_entry() {
    let p = player("Aa\n%Bio note of Aa\nAb\n%Bio note of Ab\n");
    assert_eq!(p.bio("A"), ["note of Aa"]);
    assert_eq!(p.bio("Ab"), ["note of Ab"]);
}

#[test]
fn bio_of_an_alias_prefix_match_is_empty_but_the_exact_alias_gets_the_main_notes() {
    let p = player("Main Name\n%Bio main note\n= Alias Name\n");
    assert_eq!(p.bio("Alias Name"), ["main note"], "exact alias");
    assert_eq!(
        p.bio("Alias").len(),
        0,
        "an alias reached by prefix has no notes"
    );
    assert_eq!(p.bio("Main"), ["main note"]);
}

#[test]
fn bio_of_an_exact_alias_whose_main_name_has_no_notes_is_empty() {
    let p = player("Main Name\n= Alias Name\n");
    assert_eq!(p.bio("Alias Name").len(), 0);
}

#[test]
fn bio_when_the_exact_match_is_a_main_name_ignores_notes_scanned_before_it() {
    // "Ab" exact (index 0) is scanned last; the prefix matches scanned before
    // it overwrite the stored notes, but the exact main name wins.
    let p = player("Ab\n%Bio exact notes\nAbc\n%Bio longer notes\n");
    assert_eq!(p.bio("Ab"), ["exact notes"]);
    assert_eq!(p.bio("Abc"), ["longer notes"]);
}

// ---------------------------------------------------------------------------
// Rule 33/34: retrievename and providers (via correct / comment_exact).
// ---------------------------------------------------------------------------

#[rstest]
#[case("GM RUS [2812] 1963.04.13", Some(Title::Gm))]
#[case("gm RUS [2812]", Some(Title::Gm))]
#[case("Gm RUS [2812]", Some(Title::Gm))]
#[case("gM RUS [2812]", Some(Title::Gm))]
#[case("IM POR [2450]", Some(Title::Im))]
#[case("FM POR [2300]", Some(Title::Fm))]
#[case("CM POR [2200]", Some(Title::Cm))]
#[case("WGM POR [2200]", Some(Title::Wgm))]
#[case("WIM POR [2200]", Some(Title::Wim))]
#[case("WFM POR [2200]", Some(Title::Wfm))]
#[case("hgm POR [2200]", Some(Title::Gm))]
#[case("gm+cgm POR [2200]", Some(Title::Gm))]
#[case("im+cim POR [2200]", Some(Title::Im))]
#[case("wim+clm POR [2200]", Some(Title::Wim))]
#[case("cim+im POR [2200]", None)]
#[case("cgm POR [2200]", None)]
#[case("cim POR [2200]", None)]
#[case("csm POR [2200]", None)]
#[case("clg POR [2200]", None)]
#[case("clm POR [2200]", None)]
#[case("wcm POR [2200]", None)]
#[case("comp POR [2200]", None)]
#[case("w POR [2200]", None)]
#[case("- POR [2200]", None)]
#[case("xyz POR [2200]", None)]
#[case("", None)]
#[case("gmx POR [2200]", None)]
#[case("GM", Some(Title::Gm))]
fn title_for_the_header_search(#[case] comment: &str, #[case] expected: Option<Title>) {
    let p = player(&format!("Test, T #{comment}\n"));
    assert_eq!(p.title("Test, T"), expected, "{comment:?}");
    assert_eq!(
        PlayerComment::parse(comment).title(),
        expected,
        "{comment:?}"
    );
}

#[test]
fn the_title_provider_needs_a_correct_name_but_the_bio_provider_is_loose() {
    let p = player("Braga, Carlos Eduardo #IM POR/BRA [2450] 1975.01.02\n= Braga, Carlos\n");
    assert_eq!(p.title("Braga, Carlos Eduardo"), Some(Title::Im));
    assert_eq!(p.title("Braga, Carlos"), None);
    assert_eq!(p.title("Braga"), None);
    assert_eq!(p.title("Nobody"), None);
    let bio = PlayerBioProvider::bio(&p, "Braga, Carlos").unwrap();
    assert_eq!(bio.title, "im");
    assert_eq!(bio.country, "BRA");
    assert_eq!(bio.birth.to_string(), "1975.01.02");
    assert!(PlayerBioProvider::bio(&p, "Nobody").is_none());
}

#[test]
fn the_bio_provider_titles_of_the_tournament_table() {
    // GetTitle of the Scid knows these ten; case is ignored (D3306).
    let p = player(
        "A #GM X [1]\nB #cim X [1]\nC #CGM X [1]\nD #hgm X [1]\nE #wcm X [1]\nF #- X [1]\nG #w X [1]\nH #comp X [1]\n",
    );
    let title = |n: &str| PlayerBioProvider::bio(&p, n).unwrap().title;
    assert_eq!(title("A"), "gm");
    assert_eq!(title("B"), "cim");
    assert_eq!(title("C"), "cgm");
    assert_eq!(title("D"), "hgm");
    for n in ["E", "F", "G", "H"] {
        assert_eq!(title(n), "", "{n}");
    }
}

// ---------------------------------------------------------------------------
// Rules 16 to 24: the comment.
// ---------------------------------------------------------------------------

#[rstest]
#[case("GM RUS [2812] 1963.04.13 i", "GM", &["RUS"], "RUS", 2812, "1963.04.13", "????.??.??")]
#[case("- ENG/NZL [....] 1904.??.??--1979.08.15", "-", &["ENG", "NZL"], "NZL", 0, "1904.??.??", "1979.08.15")]
#[case("- YUG/SCG/SRB [2600] 1960", "-", &["YUG", "SCG", "SRB"], "SRB", 2600, "1960.??.??", "????.??.??")]
#[case("cim URS/RUS [2520*] 1911.06.14--2004.02.29", "cim", &["URS", "RUS"], "RUS", 2520, "1911.06.14", "2004.02.29")]
#[case("- ??? [1500] ????.??.??--????.??.??", "-", &["???"], "???", 1500, "????.??.??", "????.??.??")]
#[case("- FRA [....] ????.??.??--1977.02.??", "-", &["FRA"], "FRA", 0, "????.??.??", "1977.02.??")]
#[case("- FRA [....] ????.??.??--1977", "-", &["FRA"], "FRA", 0, "????.??.??", "1977.??.??")]
#[case("w KAZ [2185] ????", "w", &["KAZ"], "KAZ", 2185, "????.??.??", "????.??.??")]
#[case("- - [1900] 1950", "-", &[], "", 1900, "1950.??.??", "????.??.??")]
#[case("- AB [1900] 1950", "-", &["AB"], "", 1900, "1950.??.??", "????.??.??")]
#[case("- AUS [1490]", "-", &["AUS"], "AUS", 1490, "????.??.??", "????.??.??")]
#[case("", "", &[], "", 0, "????.??.??", "????.??.??")]
#[case("GM", "GM", &[], "", 0, "????.??.??", "????.??.??")]
#[case("GM BRA", "GM", &["BRA"], "BRA", 0, "????.??.??", "????.??.??")]
#[case("GM BRA [", "GM", &["BRA"], "BRA", 0, "????.??.??", "????.??.??")]
#[case("GM BRA ]2000", "GM", &["BRA"], "BRA", 0, "2000.??.??", "????.??.??")]
fn comment_fields(
    #[case] text: &str,
    #[case] titles: &str,
    #[case] countries: &[&str],
    #[case] last_country: &str,
    #[case] peak: u16,
    #[case] born: &str,
    #[case] died: &str,
) {
    let c = PlayerComment::parse(text);
    assert_eq!(c.title_field(), titles, "{text:?}");
    assert_eq!(c.countries(), countries, "{text:?}");
    assert_eq!(c.last_country(), last_country, "{text:?}");
    assert_eq!(c.peak(), peak, "{text:?}");
    assert_eq!(c.born().to_string(), born, "{text:?}");
    assert_eq!(c.died().to_string(), died, "{text:?}");
}

/// Grammar of the spec: `NASCIMENTO [ "--" MORTE ]` with `DATA = ANO [ "." MES [ "." DIA ] ]`.
/// A birth that is only a year, followed by `--`, must not borrow its month
/// and day from the death date (the Scid does, because it looks for the next
/// `.` without stopping at `-`).
#[test]
fn a_year_only_birth_before_a_death_date_keeps_unknown_month_and_day() {
    let c = PlayerComment::parse("- FRA [1800] 1904--1979.08.15");
    assert_eq!(c.born().to_string(), "1904.??.??");
    assert_eq!(c.died().to_string(), "1979.08.15");
    let c = PlayerComment::parse("- FRA [....] ????--1977.02.??");
    assert_eq!(c.born().to_string(), "????.??.??");
}

#[test]
fn the_fifth_field_is_ignored() {
    for flag in ["i", "w", "wi", "xyz", "i w extra"] {
        let text = format!("GM RUS [2812] 1963.04.13 {flag}");
        let c = PlayerComment::parse(&text);
        assert_eq!(c.born().to_string(), "1963.04.13", "{flag}");
        assert_eq!(c.died().to_string(), "????.??.??", "{flag}");
    }
}

#[test]
fn raw_title_and_country_lists_for_the_information_window() {
    let c = PlayerComment::parse("im+cim URS/RUS [2520*] 1911.06.14--2004.02.29");
    assert_eq!(c.titles_raw(), ["im", "cim"]);
    assert_eq!(c.countries(), ["URS", "RUS"]);
    assert_eq!(c.born_field(), "1911.06.14--2004.02.29");
    let c = PlayerComment::parse("- - [....] ????");
    assert_eq!(c.titles_raw().len(), 0);
    assert_eq!(c.countries().len(), 0);
}

#[test]
fn peak_rating_forms() {
    for (text, peak, estimate) in [
        ("GM RUS [2425]", 2425, false),
        ("GM RUS [2520*]", 2520, true),
        ("GM RUS [....]", 0, false),
        ("GM RUS []", 0, false),
        ("GM RUS [*]", 0, false),
        ("GM RUS 2425", 0, false),
        ("GM RUS [0]", 0, false),
        ("GM RUS [x2425]", 0, false),
        ("GM RUS [2425 ] [3000]", 2425, false),
    ] {
        let c = PlayerComment::parse(text);
        assert_eq!((c.peak(), c.peak_is_estimate()), (peak, estimate), "{text}");
    }
}

#[test]
fn a_year_above_2047_is_an_unknown_birth() {
    for text in ["- X [1] 3000.01.01", "- X [1] 9999", "- X [1] 2048.05.05"] {
        let born = PlayerComment::parse(text).born();
        assert!(born.year().is_none(), "{text}: {born}");
    }
    assert_eq!(
        PlayerComment::parse("- X [1] 2047").born().year(),
        Some(2047)
    );
}

#[test]
fn hostile_comments_never_panic() {
    let many_x = "x ".repeat(40_000);
    let many_brackets = "[".repeat(60_000);
    let long_date = format!("- AAA [1] {}", "9".repeat(60_000));
    for text in [
        "\u{0}",
        "é é é [é] é--é",
        "[[[[",
        "]]]]",
        "[99999999999999999999]",
        "- ÉBC [1] 1--",
        "-- -- -- --",
        "GM ABÉ [1]",
        &many_x,
        &many_brackets,
        &long_date,
    ] {
        let c = PlayerComment::parse(text);
        std::hint::black_box((
            c.title(),
            c.titles_raw(),
            c.countries(),
            c.last_country(),
            c.peak(),
            c.born(),
            c.died(),
            c.born_field(),
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn comment_parsing_is_total(text in "\\PC{0,80}") {
        let c = PlayerComment::parse(&text);
        prop_assert!(c.last_country().len() <= 3);
        prop_assert!(c.countries().iter().all(|x| !x.is_empty() && *x != "-"));
    }

    #[test]
    fn well_formed_comments_round_trip_their_fields(
        peak in 0u16..3000, year in 1850u16..2020, month in 1u8..13, day in 1u8..29, cc in "[A-Z]{3}"
    ) {
        let text = format!("GM {cc} [{peak}] {year}.{month:02}.{day:02} i");
        let c = PlayerComment::parse(&text);
        prop_assert_eq!(c.last_country(), cc.as_str());
        prop_assert_eq!(c.peak(), peak);
        prop_assert_eq!(c.born().to_string(), format!("{year}.{month:02}.{day:02}"));
        prop_assert_eq!(c.title(), Some(Title::Gm));
    }

    #[test]
    fn matching_is_total_and_consistent(
        names in proptest::collection::vec("[A-Za-z]{1,6}", 1..20),
        query in "[A-Za-z]{0,6}"
    ) {
        let mut text = String::from("@PLAYER \"\"\n");
        for n in &names { text.push_str(n); text.push('\n'); }
        let p = player(&text.replace("@PLAYER \"\"\n", ""));
        let correct = p.correct(&query);
        let list = p.corrections(&query, 10);
        if query.is_empty() {
            prop_assert!(correct.is_none() && list.is_empty());
        } else {
            let matching = names.iter().filter(|n| n.starts_with(&query)).count();
            prop_assert_eq!(correct.is_some(), matching > 0);
            prop_assert_eq!(!list.is_empty(), matching > 0);
            let exact = names.contains(&query);
            if exact {
                prop_assert_eq!(list.len(), 1);
                prop_assert_eq!(correct, Some(query.as_str()));
            } else if let Some(c) = correct {
                // The lowest file index among the prefix matches.
                let first = names.iter().find(|n| n.starts_with(&query)).unwrap();
                prop_assert_eq!(c, first.as_str());
            }
            let mut seen = std::collections::HashSet::new();
            prop_assert!(list.iter().all(|x| seen.insert(*x)), "no repeats");
        }
    }
}

// ---------------------------------------------------------------------------
// Performance: O(log n + k) with 100 000 names (generous bound).
// ---------------------------------------------------------------------------

#[test]
fn queries_on_a_hundred_thousand_names_are_fast() {
    let mut text = String::from("@PLAYER \"., -_*\"\n");
    for i in 0..100_000u32 {
        writeln!(
            text,
            "Family{i:06}, Given{} #GM BRA [2500] 1980.01.01\n  = Fam{i:06}",
            i % 97
        )
        .unwrap();
    }
    let set = read(&text);
    let p = set.checker(NameKind::Player);
    assert_eq!(p.num_names(), 100_000);
    let started = Instant::now();
    for i in (0..100_000u32).step_by(25) {
        let exact = format!("Family{i:06}, Given{}", i % 97);
        assert_eq!(p.correct(&exact), Some(exact.as_str()));
        // A prefix with a handful of candidates (k = 10).
        let prefix = format!("Family{:05}", i / 10);
        assert!(p.correct(&prefix).is_some());
        assert!(p.corrections(&prefix, 10).len() <= 10);
        assert!(p.comment_exact(&exact).is_some());
        assert_eq!(p.title(&exact), Some(Title::Gm));
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_secs_f32() < 5.0,
        "4000 x 5 queries took {elapsed:?}: not O(log n + k)"
    );
}

// ---------------------------------------------------------------------------
// Rules 61, 62: the period tables and `elo_at`.
// ---------------------------------------------------------------------------

const MONTH_TO_POSITION: [(u16, [usize; 12]); 6] = [
    (2013, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    (2012, [0, 0, 1, 1, 2, 2, 3, 4, 5, 6, 7, 8]),
    (2011, [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]),
    (2009, [0, 0, 0, 1, 1, 1, 2, 2, 3, 3, 4, 4]),
    (2005, [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3]),
    (1990, [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1]),
];

#[test]
fn the_period_table_maps_every_month_of_every_period() {
    for (year, expected) in MONTH_TO_POSITION {
        for (i, want) in expected.into_iter().enumerate() {
            let month = u8::try_from(i + 1).unwrap();
            assert_eq!(period_position(year, month), want, "{year}.{month:02}");
        }
        assert_eq!(period_position(year, 0), 0, "{year}: unknown month");
    }
    // Boundaries between periods.
    for (year, n) in [
        (2025, 12),
        (2013, 12),
        (2012, 9),
        (2011, 6),
        (2010, 6),
        (2009, 5),
        (2008, 4),
        (2001, 4),
        (2000, 2),
        (1970, 2),
    ] {
        assert_eq!(positions_in_year(year), n, "{year}");
    }
}

#[test]
fn elo_at_reads_every_month_of_every_period_end_to_end() {
    for (year, expected) in MONTH_TO_POSITION {
        let values: Vec<String> = (1..=12).map(|i| (2200 + i).to_string()).collect();
        let set = read(&format!("@PLAYER\nX\n%Elo {year}:{}\n", values.join(",")));
        let p = set.checker(NameKind::Player);
        for (i, want) in expected.into_iter().enumerate() {
            let when = date(&format!("{year}.{:02}.15", i + 1));
            let want = u16::try_from(2201 + want).unwrap();
            assert_eq!(
                p.elo_at("X", when, false),
                Some(want),
                "{year}.{:02}",
                i + 1
            );
            assert_eq!(
                p.elo_at("X", when, true),
                Some(want),
                "exact {year}.{:02}",
                i + 1
            );
        }
    }
}

#[test]
fn elo_at_uses_the_table_of_the_year_of_the_game_only() {
    let set = read("@PLAYER\nX\n%Elo 2013:2300,2301 2011:2100\n");
    let p = set.checker(NameKind::Player);
    // No fall back to another year.
    assert_eq!(p.elo_at("X", date("2012.06.01"), false), None);
    assert_eq!(p.elo_at("X", date("2014.01.01"), false), None);
    assert_eq!(p.elo_at("X", date("2011.12.01"), false), Some(2100));
}

#[test]
fn a_zero_period_falls_back_to_the_first_position_of_the_year() {
    let set = read("@PLAYER\nX\n%Elo 2013:2300,?,?,2303\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.elo_at("X", date("2013.02.01"), false), Some(2300));
    assert_eq!(
        p.elo_at("X", date("2013.02.01"), true),
        Some(2300),
        "exact still uses position 0"
    );
    assert_eq!(p.elo_at("X", date("2013.04.01"), false), Some(2303));
}

#[test]
fn unknown_month_sweeps_the_year_only_when_not_exact_and_position_zero_is_empty() {
    let set = read("@PLAYER\nX\n%Elo 2013:?,?,2302\nY\n%Elo 2013:2300,?,2302\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.elo_at("X", date("2013.00.00"), false), Some(2302));
    assert_eq!(p.elo_at("X", date("2013.00.00"), true), None);
    assert_eq!(p.elo_at("X", date("2013.??.??"), false), Some(2302));
    assert_eq!(p.elo_at("Y", date("2013.00.00"), false), Some(2300));
    assert_eq!(p.elo_at("Y", date("2013.00.00"), true), Some(2300));
    // Known month with nothing at all: no sweep.
    assert_eq!(p.elo_at("X", date("2013.05.01"), false), None);
}

#[test]
fn elo_at_year_limits_follow_the_current_year_of_the_checker() {
    let text = "@PLAYER\nX\n%Elo 1969:100 1970:200 2020:300 2021:400 2030:500\n";
    let set = read(text);
    let p = set.checker(NameKind::Player);
    assert_eq!(p.last_year(), 2020);
    assert_eq!(p.elo_at("X", date("1969.01.01"), false), None);
    assert_eq!(p.elo_at("X", date("1970.01.01"), false), Some(200));
    assert_eq!(p.elo_at("X", date("2020.06.01"), false), Some(300));
    assert_eq!(p.elo_at("X", date("2021.06.01"), false), None);
    let later = p.clone().with_last_year(2030);
    assert_eq!(later.elo_at("X", date("2021.06.01"), false), Some(400));
    assert_eq!(later.elo_at("X", date("2030.06.01"), false), Some(500));
    let earlier = p.clone().with_last_year(1995);
    assert_eq!(earlier.elo_at("X", date("2020.06.01"), false), None);
}

#[test]
fn elo_at_needs_the_exact_correct_name() {
    let set = read("@PLAYER\nAlfa, A\n%Elo 2013:2300\n= Alfa\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.elo_at("Alfa, A", date("2013.01.01"), false), Some(2300));
    assert_eq!(p.elo_at("Alfa", date("2013.01.01"), false), None, "alias");
    assert_eq!(p.elo_at("Alf", date("2013.01.01"), false), None, "prefix");
    assert_eq!(p.elo_at("", date("2013.01.01"), false), None);
}

#[test]
fn elo_at_with_an_unknown_year_is_none() {
    let set = read("@PLAYER\nX\n%Elo 2013:2300\n");
    let p = set.checker(NameKind::Player);
    for d in [PartialDate::UNKNOWN, date("????.05.01"), date("0000.00.00")] {
        assert_eq!(p.elo_at("X", d, false), None, "{d}");
    }
}

// ---------------------------------------------------------------------------
// Rule 64: elo_history.
// ---------------------------------------------------------------------------

#[rstest]
#[case(2013, 12, &[(1, "2013.08"), (11, "2013.91")])]
#[case(2012, 9, &[(1, "2012.11"), (4, "2012.44"), (8, "2012.88")])]
#[case(2011, 6, &[(1, "2011.16"), (5, "2011.83")])]
#[case(2009, 5, &[(1, "2009.20"), (4, "2009.80")])]
#[case(2005, 4, &[(1, "2005.25"), (2, "2005.50"), (3, "2005.75")])]
#[case(1999, 2, &[(1, "1999.50")])]
fn history_fractions_use_the_positions_of_the_year(
    #[case] year: u16,
    #[case] n: usize,
    #[case] probes: &[(usize, &str)],
) {
    let values: Vec<String> = (0..n).map(|i| (2200 + i).to_string()).collect();
    let set = read(&format!("@PLAYER\nX\n%Elo {year}:{}\n", values.join(",")));
    let h = set.checker(NameKind::Player).elo_history("X", 1970, 0);
    assert_eq!(h.len(), n);
    assert_eq!(h[0], (f32::from(year), 2200));
    for &(position, text) in probes {
        let want: f32 = text.parse().unwrap();
        assert_eq!(
            h[position],
            (want, u16::try_from(2200 + position).unwrap()),
            "{text}"
        );
    }
}

#[test]
fn history_skips_zero_positions_and_respects_both_limits() {
    let set = read("@PLAYER\nX\n%Elo 2013:2300,?,2500 2005:2000,2100,?,2300\n");
    let p = set.checker(NameKind::Player);
    let all = p.elo_history("X", 1970, 0);
    let elos: Vec<u16> = all.iter().map(|e| e.1).collect();
    assert_eq!(elos, [2000, 2100, 2300, 2300, 2500]);
    assert!(all.windows(2).all(|w| w[0].0 <= w[1].0), "ordered by time");
    assert_eq!(p.elo_history("X", 2006, 0).len(), 2);
    assert_eq!(p.elo_history("X", 1970, 2300).len(), 3);
    assert_eq!(p.elo_history("X", 2013, 2301).len(), 1);
    assert_eq!(p.elo_history("X", 2014, 0).len(), 0);
    assert_eq!(p.elo_history("X", 1970, 9999).len(), 0);
}

#[test]
fn history_goes_through_the_correct_name_and_is_empty_without_elo() {
    let set = read("@PLAYER\nAlfa, A\n%Elo 2013:2300\n= Alfa\nBeta, B\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.elo_history("Alfa", 1970, 0).len(), 1, "alias");
    assert_eq!(
        p.elo_history("Alf", 1970, 0).len(),
        1,
        "prefix, as correct() resolves it"
    );
    assert_eq!(p.elo_history("Beta, B", 1970, 0).len(), 0);
    assert_eq!(p.elo_history("", 1970, 0).len(), 0);
    assert_eq!(p.elo_history("Zzz", 1970, 0).len(), 0);
}
