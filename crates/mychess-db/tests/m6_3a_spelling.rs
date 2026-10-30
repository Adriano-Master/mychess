//! Contract tests of the reading and the queries of the spelling file (M6-3a,
//! criteria 1 to 14 of `docs/specs/m6-3-jogadores-torneios-spelling.md`).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_3a_support.rs"]
mod support;

use std::fmt::Write as _;
use std::io::Write as _;

use mychess_core::PartialDate;
use mychess_db::si4::NameKind;
use mychess_db::spelling::{PlayerComment, ReadOptions, SpellingError, SpellingSet};
use mychess_db::{CancelToken, NoProgress, Title, TitleProvider};
use pretty_assertions::assert_eq;
use support::{EXAMPLE, date, example, opts, read};

#[test]
fn example_counts_and_elo_flag() {
    let set = example(2020);
    let r = set.report();
    assert_eq!((r.players, r.sites, r.events, r.rounds), (3, 1, 1, 1));
    assert_eq!(r.skipped_lines, 0);
    assert!(set.checker(NameKind::Player).has_elo_data());
    assert!(!set.checker(NameKind::Site).has_elo_data());
    assert!(!set.checker(NameKind::Event).has_elo_data());
    assert!(!set.checker(NameKind::Round).has_elo_data());
    assert_eq!(set.checker(NameKind::Player).num_aliases(), 4);
}

#[test]
fn correct_follows_the_scan_order_of_the_scid() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    assert_eq!(p.correct("Almeida, B"), Some("Almeida, Bruno"));
    assert_eq!(p.correct("Almeida, Bea"), Some("Almeida, Beatriz Costa"));
    assert_eq!(p.correct("Almeida, B C"), Some("Almeida, Beatriz Costa"));
    assert_eq!(p.correct("almeida, b"), None, "case counts");
    assert_eq!(p.correct(".,-"), None);
    assert_eq!(p.correct(""), None);
    assert_eq!(p.correct("Zzz"), None);
    // Without an exact match the lowest file index wins.
    assert_eq!(p.correct("Almeida"), Some("Almeida, Beatriz Costa"));
}

#[test]
fn corrections_are_ordered_deduplicated_and_exact_wins() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    assert_eq!(
        p.corrections("Almeida", 10),
        ["Almeida, Bruno", "Almeida, Beatriz Costa"]
    );
    assert_eq!(p.corrections("Almeida, B", 10), ["Almeida, Bruno"]);
    assert_eq!(p.corrections("Almeida", 1), ["Almeida, Bruno"]);
    assert_eq!(p.corrections("Almeida, Be", 10), ["Almeida, Beatriz Costa"]);
    assert_eq!(p.corrections("Almeida", 0).len(), 0);
    assert_eq!(p.corrections("", 10).len(), 0);
}

#[test]
fn comments_and_bio() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    assert_eq!(
        p.comment_exact("Braga, Carlos Eduardo"),
        Some("IM POR/BRA [2450] 1975.01.02")
    );
    assert_eq!(p.comment_exact("Braga, Carlos"), None);
    assert_eq!(
        p.comment_loose("Braga, Carlos"),
        Some("IM POR/BRA [2450] 1975.01.02")
    );
    assert_eq!(p.comment_loose("Nobody"), None);
    let notes = ["FIDEID 9000001", "Campeã brasileira em 2015"];
    assert_eq!(p.bio("Almeida, Beatriz Costa"), notes);
    assert_eq!(p.bio("Almeida, Beatriz"), notes);
    assert_eq!(p.bio("Braga, Carlos Eduardo").len(), 0);
    assert!(p.match_is_exact("Almeida, Beatriz"));
    assert!(!p.match_is_exact("Almeida, Bea"));
}

#[test]
fn replacement_rules() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    assert_eq!(
        p.prefix_fix("GM Braga, Carlos").as_deref(),
        Some("Braga, Carlos")
    );
    assert_eq!(
        p.suffix_fix("Braga, Carlos ").as_deref(),
        Some("Braga, Carlos")
    );
    assert_eq!(p.prefix_fix("Braga"), None);
    let site = set.checker(NameKind::Site);
    assert_eq!(
        site.suffix_fix("Sao Paulo (Brazil)").as_deref(),
        Some("Sao Paulo BRA")
    );
    let event = set.checker(NameKind::Event);
    assert_eq!(
        event.infix_fix("Brasil-ch 2020").as_deref(),
        Some("Brasil Ch 2020")
    );
    assert_eq!(event.infix_fix("Open"), None);
}

#[test]
fn the_last_rule_defined_is_tried_first() {
    let set = read("@PLAYER\n%Prefix \"A\" \"1\"\n%Prefix \"AB\" \"2\"\n");
    let p = set.checker(NameKind::Player);
    assert_eq!(p.prefix_fix("ABC").as_deref(), Some("2C"));
    assert_eq!(p.prefix_fix("AXC").as_deref(), Some("1XC"));
    let set = read("@PLAYER\n%Prefix \"AB\" \"2\"\n%Prefix \"A\" \"1\"\n");
    assert_eq!(
        set.checker(NameKind::Player).prefix_fix("ABC").as_deref(),
        Some("1BC")
    );
}

#[test]
fn grammar_details() {
    // '#' inside %Bio ends the note; "%Bio" alone is ignored.
    let set = read("@PLAYER\nX, Y\n%Bio Rating #1 em 1990\n%Bio\n%Bio ok\n");
    assert_eq!(set.checker(NameKind::Player).bio("X, Y"), ["Rating", "ok"]);
    // Abbreviated and unknown sections.
    let set = read("@pl \"x\"\nA\n@FOO\nB\n@S\nC\n");
    let r = set.report();
    assert_eq!((r.players, r.sites), (1, 1));
    // A line before any section, and an alias before any name, are ignored.
    let set = read("Orphan\n@PLAYER\n= alias\nName\n");
    assert_eq!(set.report().players, 1);
    assert_eq!(set.checker(NameKind::Player).correct("alias"), None);
    // Old biography lines.
    assert_eq!(read("@PLAYER\n> old bio\nName\n").report().players, 1);
}

#[test]
fn line_endings_bom_and_missing_final_newline() {
    let lf = read("@PLAYER\nA\nB").report().players;
    assert_eq!(lf, 2, "the last line has no newline");
    assert_eq!(read("@PLAYER\r\nA\r\nB\r\n").report().players, 2);
    assert_eq!(read("@PLAYER\rA\rB\r").report().players, 2);
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(b"@PLAYER\nA\n");
    let set = SpellingSet::read_bytes(&bytes, &opts(2020)).unwrap();
    assert_eq!(set.report().players, 1, "the BOM does not hide the section");
}

#[test]
fn windows_1252_and_utf8() {
    let latin = SpellingSet::read_bytes(b"@PLAYER\nJos\xE9 Pereira\n", &opts(2020)).unwrap();
    assert_eq!(
        latin.checker(NameKind::Player).correct("Jos\u{e9} Pereira"),
        Some("Jos\u{e9} Pereira")
    );
    let utf8 = read("@PLAYER\nJosé Pereira\n");
    assert_eq!(
        utf8.checker(NameKind::Player).correct("José"),
        Some("José Pereira")
    );
}

#[test]
fn gzip_reads_like_text() {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(EXAMPLE).unwrap();
    let gz = enc.finish().unwrap();
    let from_bytes = SpellingSet::read_bytes(&gz, &opts(2020)).unwrap();
    assert_eq!(from_bytes.report().players, 3);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spelling.ssp.GZ");
    std::fs::write(&path, &gz).unwrap();
    let from_file =
        SpellingSet::read(&path, &opts(2020), &CancelToken::new(), &mut NoProgress).unwrap();
    assert_eq!(from_file.report(), from_bytes.report());
    // A damaged stream is an error.
    let broken = &gz[..gz.len() / 2];
    assert!(matches!(
        SpellingSet::read_bytes(broken, &opts(2020)),
        Err(SpellingError::Gzip(_))
    ));
}

#[test]
fn unterminated_quote_fails_and_other_errors() {
    let err = SpellingSet::read_bytes(b"@PLAYER \"abc\nName\n", &opts(2020)).unwrap_err();
    assert!(matches!(err, SpellingError::UnclosedQuote { line: 1 }));
    // An unknown section with an open quote is not an error.
    assert!(SpellingSet::read_bytes(b"@FOO \"abc\n", &opts(2020)).is_ok());
    let dir = tempfile::tempdir().unwrap();
    for path in [dir.path().join("missing.ssp"), dir.path().to_path_buf()] {
        assert!(
            SpellingSet::read(&path, &opts(2020), &CancelToken::new(), &mut NoProgress).is_err()
        );
    }
    let small = ReadOptions {
        max_bytes: 4,
        ..opts(2020)
    };
    assert!(matches!(
        SpellingSet::read_bytes(EXAMPLE, &small),
        Err(SpellingError::TooLarge { limit: 4 })
    ));
    let cancelled = CancelToken::new();
    cancelled.cancel();
    let many = "@PLAYER\nA\n".repeat(10_000);
    let path = dir.path().join("big.ssp");
    std::fs::write(&path, many).unwrap();
    assert!(matches!(
        SpellingSet::read(&path, &opts(2020), &cancelled, &mut NoProgress),
        Err(SpellingError::Cancelled)
    ));
}

#[test]
fn long_lines_are_skipped_and_counted() {
    let mut text = String::from("@PLAYER\nA\n");
    text.push_str(&"x".repeat(70 * 1024));
    text.push_str("\nB\n");
    let set = read(&text);
    assert_eq!(set.report().skipped_lines, 1);
    assert_eq!(set.report().players, 2);
    // Exactly at the limit still counts.
    let text = format!("@PLAYER\n{}\n", "y".repeat(64 * 1024));
    assert_eq!(read(&text).report().players, 1);
}

#[test]
fn bio_notes_are_limited() {
    let mut text = String::from("@PLAYER\nX\n");
    for i in 0..1005 {
        writeln!(text, "%Bio note {i}").unwrap();
    }
    let set = read(&text);
    assert_eq!(set.checker(NameKind::Player).bio("X").len(), 1000);
    assert_eq!(set.report().bio_dropped, 5);
}

#[test]
fn player_comment_examples() {
    let cases: [(&str, &str, u16, bool, &str, &str); 7] = [
        (
            "GM RUS [2812] 1963.04.13 i",
            "RUS",
            2812,
            false,
            "1963.04.13",
            "????.??.??",
        ),
        (
            "- BAN [1648] 2015",
            "BAN",
            1648,
            false,
            "2015.??.??",
            "????.??.??",
        ),
        (
            "- ENG/NZL [....] 1904.??.??--1979.08.15",
            "NZL",
            0,
            false,
            "1904.??.??",
            "1979.08.15",
        ),
        (
            "cim URS/RUS [2520*] 1911.06.14--2004.02.29",
            "RUS",
            2520,
            true,
            "1911.06.14",
            "2004.02.29",
        ),
        (
            "- FRA [....] ????",
            "FRA",
            0,
            false,
            "????.??.??",
            "????.??.??",
        ),
        (
            "- AUS [1490]",
            "AUS",
            1490,
            false,
            "????.??.??",
            "????.??.??",
        ),
        (
            "w KAZ [2185] ????",
            "KAZ",
            2185,
            false,
            "????.??.??",
            "????.??.??",
        ),
    ];
    for (text, country, peak, estimate, born, died) in cases {
        let c = PlayerComment::parse(text);
        assert_eq!(c.last_country(), country, "{text}");
        assert_eq!((c.peak(), c.peak_is_estimate()), (peak, estimate), "{text}");
        assert_eq!(c.born().to_string(), born, "{text}");
        assert_eq!(c.died().to_string(), died, "{text}");
    }
}

#[test]
fn title_provider() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    assert_eq!(p.title("Braga, Carlos Eduardo"), Some(Title::Im));
    assert_eq!(p.title("Almeida, Beatriz Costa"), Some(Title::Wgm));
    assert_eq!(p.title("Almeida, Bruno"), None);
    assert_eq!(p.title("Braga, Carlos"), None, "alias");
    let titles = read(
        "@PLAYER\nA #GM X [1]\nB #gm X [1]\nC #Gm X [1]\nD #hgm X [1]\nE #cim X [1]\nF #csm X [1]\nG #w X [1]\nH #- X [1]\n",
    );
    let t = titles.checker(NameKind::Player);
    for name in ["A", "B", "C", "D"] {
        assert_eq!(t.title(name), Some(Title::Gm), "{name}");
    }
    for name in ["E", "F", "G", "H"] {
        assert_eq!(t.title(name), None, "{name}");
    }
}

#[test]
fn elo_by_period() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    let name = "Almeida, Beatriz Costa";
    let at = |d: &str, exact: bool| p.elo_at(name, date(d), exact);
    assert_eq!(at("2013.03.10", false), Some(2305));
    assert_eq!(at("2013.12.31", false), Some(2350));
    assert_eq!(at("2013.00.00", false), Some(2300));
    assert_eq!(at("2001.02.01", false), Some(2200));
    assert_eq!(at("2001.12.01", false), Some(2230));
    assert_eq!(at("2002.06.01", false), None);
    assert_eq!(at("1969.06.01", false), None);
    assert_eq!(at("2021.06.01", false), None);
    assert_eq!(at("????.??.??", false), None);
    assert_eq!(
        p.elo_at("Almeida, Beatriz", date("2013.03.10"), false),
        None
    );
    assert_eq!(
        p.elo_at("Braga, Carlos Eduardo", date("2013.03.10"), false),
        None
    );
}

#[test]
fn elo_period_tables() {
    let values = |n: u16| -> String {
        (1..=n)
            .map(|i| (2200 + i).to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let text = format!(
        "@PLAYER\nX\n%Elo 2012:{} 2010:{} 2009:{} 2004:{} 1999:{}\n",
        values(9),
        values(6),
        values(5),
        values(4),
        values(2)
    );
    let set = read(&text);
    let p = set.checker(NameKind::Player);
    let at = |d: &str| p.elo_at("X", date(d), false);
    assert_eq!(at("2012.05.01"), Some(2203));
    assert_eq!(at("2012.09.01"), Some(2206));
    assert_eq!(at("2012.12.01"), Some(2209));
    assert_eq!(at("2010.04.01"), Some(2202));
    assert_eq!(at("2010.12.01"), Some(2206));
    assert_eq!(at("2009.07.01"), Some(2203));
    assert_eq!(at("2009.11.01"), Some(2205));
    assert_eq!(at("2004.10.01"), Some(2204));
    assert_eq!(at("1999.06.01"), Some(2201));
    assert_eq!(at("1999.07.01"), Some(2202));
}

#[test]
fn elo_fallbacks() {
    let set = read(
        "@PLAYER\nX\n%Elo 2005:?,2250,?,?\nY\n%Elo 2005:?,?,2300,?\nZ\n%Elo 2005:2200,?,?,?\n",
    );
    let p = set.checker(NameKind::Player);
    assert_eq!(
        p.elo_at("X", date("2005.01.15"), false),
        None,
        "known month: no sweep"
    );
    assert_eq!(p.elo_at("X", date("2005.00.00"), false), Some(2250));
    assert_eq!(p.elo_at("X", date("2005.00.00"), true), None);
    assert_eq!(p.elo_at("Y", date("2005.04.01"), false), None);
    assert_eq!(p.elo_at("Z", date("2005.10.01"), false), Some(2200));
}

#[test]
fn elo_line_quirks() {
    let set = read(
        "@PLAYER\nA\n%Elo 2005:2200,x,2300\nB\n%Elo 1969:2000 2013:2300,2301,2302,2303,2304,2305,2306,2307,2308,2309,2310,2311,2312\nC\n%Elo 2013:2300 2014:2310\nD\n%Elo 2013:2300\n%Elo 2013:?,2301\n",
    );
    let p = set.checker(NameKind::Player);
    assert_eq!(p.elo_at("A", date("2005.00.00"), false), Some(2200));
    assert_eq!(p.elo_at("A", date("2005.04.01"), false), Some(2200));
    assert_eq!(p.elo_table("A").unwrap().year(2005).unwrap()[2], 0);
    assert!(p.elo_table("B").unwrap().year(1969).is_none());
    assert_eq!(p.elo_table("B").unwrap().year(2013).unwrap()[11], 2311);
    assert_eq!(p.elo_table("C").unwrap().years().count(), 2);
    // A second line only overwrites the positions it defines.
    let d = p.elo_table("D").unwrap();
    assert_eq!(d.year(2013).unwrap()[..2], [0, 2301]);
}

#[test]
fn elo_history_pairs() {
    let set = example(2020);
    let p = set.checker(NameKind::Player);
    let all = p.elo_history("Almeida, Beatriz Costa", 2000, 2200);
    assert_eq!(all.len(), 16);
    assert_eq!(all[0], (2001.00_f32, 2200));
    assert_eq!(all[1], (2001.25_f32, 2210));
    assert_eq!(all[3], (2001.75_f32, 2230));
    assert_eq!(all[4], (2013.00_f32, 2300));
    assert_eq!(all[5], (2013.08_f32, 2301));
    assert_eq!(all[6], (2013.16_f32, 2305));
    assert_eq!(all[15], (2013.91_f32, 2350));
    assert_eq!(p.elo_history("Almeida, Beatriz Costa", 2010, 0).len(), 12);
    let high = p.elo_history("Almeida, Beatriz Costa", 2000, 2320);
    assert_eq!(high.len(), 7);
    assert_eq!(high[0], (2013.41_f32, 2320));
    assert_eq!(p.elo_history("Nobody", 2000, 0).len(), 0);
    // A correct name found through an alias.
    assert_eq!(p.elo_history("Almeida, Beatriz", 2010, 0).len(), 12);
}

#[test]
fn queries_with_arbitrary_text_never_panic() {
    let set = example(2020);
    for text in [
        "",
        " ",
        "é",
        "日本語",
        "\u{0}",
        "Almeida, 🙂",
        "%Bio",
        "((((",
        "\n",
    ] {
        for kind in NameKind::ALL {
            let c = set.checker(kind);
            std::hint::black_box(c.correct(text));
            std::hint::black_box(c.corrections(text, 3));
            std::hint::black_box(c.comment_loose(text));
            std::hint::black_box(c.comment_exact(text));
            std::hint::black_box(c.bio(text));
            std::hint::black_box(c.prefix_fix(text));
            std::hint::black_box(c.suffix_fix(text));
            std::hint::black_box(c.infix_fix(text));
            std::hint::black_box(c.elo_at(text, PartialDate::UNKNOWN, false));
            std::hint::black_box(c.elo_history(text, 0, 0));
        }
    }
}

mod robustness {
    use super::*;
    use proptest::prelude::*;

    fn line() -> impl Strategy<Value = String> {
        prop_oneof![
            "@(PLAYER|player|P|SITE|EVENT|ROUND|FOO)( \"[., -_*]{0,4}\")?",
            "%(Prefix|Suffix|Infix) \"[A-Za-z ]{0,5}\" \"[A-Za-z ]{0,5}\"",
            "%Bio [ -~]{0,20}",
            "%Elo ([0-9]{1,4}:([0-9]{1,4}|\\?)(,([0-9]{1,4}|\\?)){0,13} ?){0,3}",
            "= ?[A-Za-z, ]{0,12}",
            "[A-Za-z, ]{0,12}( #[ -~]{0,30})?",
            "[ -~]{0,30}",
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn grammar_lines_never_panic_and_are_deterministic(lines in proptest::collection::vec(line(), 0..40)) {
            let text = lines.join("\n");
            let a = SpellingSet::read_bytes(text.as_bytes(), &opts(2020));
            let b = SpellingSet::read_bytes(text.as_bytes(), &opts(2020));
            prop_assert_eq!(a.is_ok(), b.is_ok());
            if let (Ok(a), Ok(b)) = (a, b) {
                prop_assert_eq!(a.report(), b.report());
                for kind in NameKind::ALL {
                    let (ca, cb) = (a.checker(kind), b.checker(kind));
                    for name in ca.correct_names().take(5) {
                        prop_assert_eq!(ca.correct(name), cb.correct(name));
                        std::hint::black_box(ca.elo_history(name, 1970, 0));
                    }
                }
            }
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
            if let Err(e) = SpellingSet::read_bytes(&bytes, &opts(2020)) {
                prop_assert!(!e.to_string().is_empty());
            }
        }

        #[test]
        fn every_prefix_of_the_example_reads(cut in 0usize..EXAMPLE.len()) {
            if let Err(e) = SpellingSet::read_bytes(&EXAMPLE[..cut], &opts(2020)) {
                prop_assert!(!e.to_string().is_empty());
            }
        }

        #[test]
        fn correct_names_are_found_by_themselves(names in proptest::collection::vec("[A-Z][a-z]{1,6}, [A-Z][a-z]{1,6}", 1..20)) {
            let mut text = String::from("@PLAYER \"., \"\n");
            for n in &names { text.push_str(n); text.push('\n'); }
            let set = SpellingSet::read_bytes(text.as_bytes(), &opts(2020)).unwrap();
            let p = set.checker(NameKind::Player);
            for n in &names {
                // The last of equal names wins, and an exact match is always found.
                prop_assert_eq!(p.correct(n), Some(n.as_str()));
                prop_assert!(p.comment_exact(n).is_some());
            }
        }
    }
}

/// Compatibility with the real file of the Scid (not in the repository).
/// Run with `MYCHESS_TEST_SSP=/path/spelling.ssp cargo test -p mychess-db
/// --release -- --ignored compat_`.
#[test]
#[ignore = "requires MYCHESS_TEST_SSP pointing to the spelling.ssp of Scid vs PC 4.27"]
fn compat_real_spelling_file() {
    let Some(path) = std::env::var_os("MYCHESS_TEST_SSP") else {
        eprintln!("MYCHESS_TEST_SSP is not set");
        return;
    };
    let started = std::time::Instant::now();
    let set = SpellingSet::read(
        std::path::Path::new(&path),
        &ReadOptions::default(),
        &CancelToken::new(),
        &mut NoProgress,
    )
    .unwrap();
    let elapsed = started.elapsed();
    let r = set.report();
    assert!((474_800..474_900).contains(&r.players), "{}", r.players);
    assert_eq!((r.sites, r.events, r.rounds), (804, 11, 20));
    assert!(!set.checker(NameKind::Player).has_elo_data());
    let p = set.checker(NameKind::Player);
    assert!(
        p.comment_exact("Kasparov, Garry")
            .unwrap()
            .starts_with("GM RUS [2812] 1963.04.13")
    );
    assert_eq!(p.correct("Kasparov, Garry K"), Some("Kasparov, Garry"));
    if !cfg!(debug_assertions) {
        assert!(elapsed.as_secs_f32() < 3.0, "{elapsed:?}");
    }
}

/// Speed on a synthetic file of 60 MB (twice the size of the real
/// one, which is about 30 MB). Run with `cargo test -p mychess-db --release --test m6_3a_spelling
/// -- --ignored perf_`.
#[test]
#[ignore = "timing test: run in release mode"]
fn perf_synthetic_large_file() {
    let mut text = String::from("@PLAYER \"., -_*\"\n");
    for i in 0..475_000u32 {
        writeln!(
            text,
            "Family{i:06}, Given Name{} #GM RUS [2812] 1963.04.13 i\n  %Bio FIDEID {}\n  %Bio Some long note about a chess player number {i}",
            i % 97,
            4_000_000 + i
        )
        .unwrap();
        if i % 11 == 0 {
            writeln!(text, "  = Family{i:06}, G").unwrap();
        }
    }
    eprintln!("synthetic size: {} MB", text.len() >> 20);
    let started = std::time::Instant::now();
    let set = SpellingSet::read_bytes(text.as_bytes(), &opts(2020)).unwrap();
    let read = started.elapsed();
    assert_eq!(set.report().players, 475_000);
    let started = std::time::Instant::now();
    for i in (0..475_000u32).step_by(1000) {
        let name = format!("Family{i:06}, Given Name{}", i % 97);
        assert_eq!(
            set.checker(NameKind::Player).correct(&name),
            Some(name.as_str())
        );
    }
    eprintln!("read {read:?}, 475 queries {:?}", started.elapsed());
    if !cfg!(debug_assertions) {
        assert!(read.as_secs_f32() < 3.0, "{read:?}");
    }
}

#[test]
fn default_read_limit_is_256_mib() {
    // D3440: the limit only stops absurd allocations.
    assert_eq!(mychess_db::spelling::MAX_FILE_BYTES, 256 * 1024 * 1024);
    assert_eq!(
        ReadOptions::default().max_bytes,
        mychess_db::spelling::MAX_FILE_BYTES
    );
}
