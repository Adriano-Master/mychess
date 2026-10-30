//! Numeric annotation glyphs (spec m1-2, rules 9-10, decisions D8, D10, D13).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
use std::path::PathBuf;

use mychess_core::{Nag, NagError};
use proptest::prelude::*;
use rstest::rstest;

fn value_of(text: &str) -> Option<u8> {
    Nag::parse(text).map(Nag::value)
}

// ---------- rule 9: every symbol of game_parseNag ----------

#[rstest]
#[case("!", 1)]
#[case("?", 2)]
#[case("!!", 3)]
#[case("??", 4)]
#[case("!?", 5)]
#[case("?!", 6)]
#[case("[]", 8)]
#[case("=", 10)]
#[case("~", 13)]
#[case("+=", 14)]
#[case("+/=", 14)]
#[case("=+", 15)]
#[case("=/+", 15)]
#[case("+/-", 16)]
#[case("-/+", 17)]
#[case("+-", 18)]
#[case("-+", 19)]
#[case("+--", 20)]
#[case("--+", 21)]
#[case("ZZ", 22)]
#[case("zz", 23)]
#[case("+>", 40)]
#[case("->", 41)]
#[case("=/&", 44)]
#[case("~=", 44)]
#[case("[+]", 48)]
#[case("[++]", 50)]
#[case(">", 54)]
#[case(">>", 56)]
#[case(">>>", 58)]
#[case("<", 60)]
#[case("<<", 62)]
#[case("<<<", 64)]
#[case("<+>", 130)]
#[case("<->", 131)]
#[case("<++>", 132)]
#[case("<-->", 133)]
#[case("<+++>", 134)]
#[case("<--->", 135)]
#[case("/\\", 140)]
#[case("(_)", 142)]
#[case("R", 144)]
#[case("RR", 145)]
#[case("N", 146)]
#[case("x", 147)]
#[case("x...", 147)]
#[case("_|_", 148)]
#[case("<=>", 149)]
#[case("/", 150)]
#[case("BB", 151)]
#[case("Bb", 153)]
#[case("||", 190)]
#[case("o-o", 192)]
#[case("oo", 193)]
#[case("D", 201)]
#[case("--", 210)]
#[case("o^", 212)]
#[case("|_", 214)]
#[case("_|", 215)]
fn parses_every_scid_symbol(#[case] text: &str, #[case] expected: u8) {
    assert_eq!(value_of(text), Some(expected), "symbol {text:?}");
}

#[rstest]
#[case("$1", 1)]
#[case("$12", 12)]
#[case("$255", 255)]
#[case("1", 1)]
#[case("12", 12)]
#[case("255", 255)]
#[case("$007", 7)]
fn parses_numeric_forms(#[case] text: &str, #[case] expected: u8) {
    assert_eq!(value_of(text), Some(expected));
}

#[rstest]
#[case("")]
#[case("$")]
#[case("$0")]
#[case("0")]
#[case("$256")]
#[case("256")]
#[case("$300")]
#[case("300")]
#[case("$511")]
#[case("$65536")]
#[case("$99999999999999999999999999")]
#[case("$-1")]
#[case("$ 5")]
#[case("abc")]
#[case("!x")]
#[case("?x")]
#[case("+")]
#[case("-")]
#[case("+-x")]
#[case("--x")]
#[case("<<<<")]
#[case("<+++")]
#[case("Z")]
#[case("z")]
#[case("ZX")]
#[case("RX")]
#[case("NN")]
#[case("Dx")]
#[case("B")]
#[case("BX")]
#[case("[")]
#[case("[+")]
#[case("(")]
#[case("(_")]
#[case("_")]
#[case("_|x")]
#[case("|")]
#[case("o")]
#[case("O")]
#[case("é")]
#[case("!é")]
#[case(" !")]
#[case("\0")]
fn rejects_what_game_parse_nag_does_not_know(#[case] text: &str) {
    assert_eq!(Nag::parse(text), None, "text {text:?}");
}

#[test]
fn numeric_parse_is_exact_over_a_wide_range() {
    // D10: no truncation modulo 256.
    for n in 0u32..=100_000 {
        let expected = u8::try_from(n).ok().filter(|v| *v != 0);
        for text in [format!("${n}"), n.to_string()] {
            assert_eq!(value_of(&text), expected, "text {text:?}");
        }
    }
}

// ---------- D10 / constructor ----------

#[test]
fn new_rejects_zero() {
    assert_eq!(Nag::new(0), Err(NagError::Zero));
}

#[rstest]
#[case(256)]
#[case(300)]
#[case(1000)]
#[case(u16::MAX)]
fn new_rejects_values_above_255(#[case] n: u16) {
    assert_eq!(Nag::new(n), Err(NagError::OutOfRange(u32::from(n))));
}

#[test]
fn new_accepts_the_whole_valid_range_and_value_round_trips() {
    for n in 1u16..=255 {
        let nag = Nag::new(n).unwrap();
        assert_eq!(u16::from(nag.value()), n);
        assert_eq!(nag.to_string(), format!("${n}"));
        assert_eq!(Nag::parse(&nag.to_string()), Some(nag));
    }
}

#[test]
fn nags_are_ordered_by_number() {
    assert!(Nag::new(1).unwrap() < Nag::new(2).unwrap());
    assert!(Nag::new(255).unwrap() > Nag::new(254).unwrap());
}

// ---------- D13: ascii / glyph ----------

/// NAGs that Scid reads from an ASCII symbol (rule 9).
const WITH_ASCII: [u8; 56 - 5] = [
    1, 2, 3, 4, 5, 6, 8, 10, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 40, 41, 44, 48, 50, 54,
    56, 58, 60, 62, 64, 130, 131, 132, 133, 134, 135, 140, 142, 144, 145, 146, 147, 148, 149, 150,
    151, 153, 190, 192, 193, 201,
];

#[test]
fn ascii_symbol_exists_for_every_nag_scid_can_read() {
    let mut expected: Vec<u8> = WITH_ASCII.to_vec();
    expected.extend([210, 212, 214, 215]);
    let actual: Vec<u8> = (1u16..=255)
        .map(|n| Nag::new(n).unwrap())
        .filter(|n| n.ascii().is_some())
        .map(Nag::value)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn ascii_symbol_parses_back_to_the_same_nag() {
    for n in 1u16..=255 {
        let nag = Nag::new(n).unwrap();
        if let Some(sym) = nag.ascii() {
            assert_eq!(Nag::parse(sym), Some(nag), "symbol {sym:?} of {nag}");
        }
    }
}

#[rstest]
#[case(1, "!")]
#[case(2, "?")]
#[case(3, "!!")]
#[case(4, "??")]
#[case(5, "!?")]
#[case(6, "?!")]
#[case(10, "=")]
#[case(18, "+-")]
#[case(19, "-+")]
#[case(146, "N")]
#[case(201, "D")]
fn ascii_gives_the_traditional_symbol(#[case] n: u16, #[case] symbol: &str) {
    assert_eq!(Nag::new(n).unwrap().ascii(), Some(symbol));
}

#[rstest]
#[case(7)]
#[case(9)]
#[case(11)]
#[case(66)]
#[case(211)]
#[case(255)]
fn ascii_is_none_for_nags_without_a_scid_symbol(#[case] n: u16) {
    assert_eq!(Nag::new(n).unwrap().ascii(), None);
}

/// Hand-copied entries of `evalNagsRegular` (`nagtext.h`).
#[rstest]
#[case(1, "!")]
#[case(2, "?")]
#[case(3, "!!")]
#[case(4, "??")]
#[case(5, "!?")]
#[case(6, "?!")]
#[case(7, "forced")]
#[case(8, "□")]
#[case(9, "worst")]
#[case(10, "=")]
#[case(11, "=, quiet")]
#[case(12, "↹")]
#[case(13, "~")]
#[case(14, "+=")]
#[case(18, "+-")]
#[case(22, "⊙")]
#[case(23, "⊙")]
#[case(36, "↑")]
#[case(44, "=/&")]
#[case(48, "⊞")]
#[case(58, "⋙")]
#[case(66, "$66")]
#[case(92, "↗")]
#[case(100, "$100")]
#[case(130, "⇄")]
#[case(135, "⇆⇆⇆")]
#[case(140, "△")]
#[case(141, "$141")]
#[case(142, "⌓")]
#[case(146, "N")]
#[case(147, "×")]
#[case(150, "⇗")]
#[case(151, "◫")]
#[case(190, "ǁ")]
#[case(201, "D")]
#[case(210, "—")]
#[case(211, "#")]
#[case(212, "⚨")]
#[case(213, "$213")]
#[case(214, "⌊")]
#[case(215, "⌋")]
fn glyph_matches_chosen_entries_of_the_scid_table(#[case] n: u16, #[case] glyph: &str) {
    assert_eq!(Nag::new(n).unwrap().glyph(), glyph);
}

#[test]
fn glyph_falls_back_to_dollar_form_beyond_the_scid_table() {
    for n in 216u16..=255 {
        assert_eq!(Nag::new(n).unwrap().glyph(), format!("${n}"));
    }
}

#[test]
fn glyph_is_never_empty() {
    for n in 1u16..=255 {
        assert!(!Nag::new(n).unwrap().glyph().is_empty(), "NAG {n}");
    }
}

fn scid_reference() -> PathBuf {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

/// The strings of `evalNagsRegular`, in order.
fn scid_nag_table() -> Vec<String> {
    let path = scid_reference().join("src").join("nagtext.h");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines = text.lines().skip_while(|l| !l.contains("evalNagsRegular"));
    lines.next();
    let mut out = Vec::new();
    for line in lines {
        let t = line.trim_start();
        if t.starts_with('}') {
            break;
        }
        if let Some(rest) = t.strip_prefix('"') {
            let end = rest.find('"').expect("closing quote");
            out.push(rest[..end].to_owned());
        }
    }
    out
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_all_216_glyphs_match_scid_nagtext() {
    let table = scid_nag_table();
    assert_eq!(table.len(), 216, "parsed table size");
    assert_eq!(table[0], "");
    for (n, expected) in table.iter().enumerate().skip(1) {
        let nag = Nag::new(u16::try_from(n).unwrap()).unwrap();
        assert_eq!(nag.glyph(), expected.as_str(), "glyph of ${n}");
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_every_ascii_symbol_that_equals_a_scid_glyph_is_consistent() {
    // Where Scid's own glyph is plain ASCII and parses, it must agree with the
    // NAG's number.
    for (n, glyph) in scid_nag_table().iter().enumerate().skip(1) {
        if let Some(parsed) = Nag::parse(glyph)
            && glyph.is_ascii()
            && !glyph.starts_with('$')
            && !matches!(n, 22 | 23 | 44 | 45)
        {
            assert_eq!(usize::from(parsed.value()), n, "glyph {glyph:?}");
        }
    }
}

// ---------- observations (conscious divergence from "strict" parsing) ----------

#[test]
fn observation_parse_reads_by_prefix_like_scid() {
    // Same as game_parseNag: `!!x` is `!!`, anything starting with `x` is $147.
    assert_eq!(value_of("!!x"), Some(3));
    assert_eq!(value_of("!!!"), Some(3));
    assert_eq!(value_of("xyz"), Some(147));
    assert_eq!(value_of("o-ox"), Some(192));
    assert_eq!(value_of("12abc"), Some(12));
}

// ---------- hostile input ----------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn parse_never_panics_on_arbitrary_text(s in any::<String>()) {
        let _ = Nag::parse(&s);
    }

    #[test]
    fn parse_never_panics_on_symbol_soup(s in "[-!?+=/\\\\<>\\[\\]()_|~xoRNBDzZ$0-9 .&^]{0,12}") {
        if let Some(n) = Nag::parse(&s) {
            prop_assert!((1..=255).contains(&n.value()));
        }
    }

    #[test]
    fn display_then_parse_is_identity(n in 1u16..=255) {
        let nag = Nag::new(n).unwrap();
        prop_assert_eq!(Nag::parse(&nag.to_string()), Some(nag));
    }
}
