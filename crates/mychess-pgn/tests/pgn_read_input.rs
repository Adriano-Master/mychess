//! Contract tests of the PGN reader input layer: encodings, BOM, gzip, line
//! breaks, streaming through hostile `Read` implementations, the chunker.
//!
//! Derived from `docs/specs/m1-3-pgn.md` (D18, D19, D30, reading rules 1-8,
//! acceptance criteria) and Scid's `pgnparse.cpp`/`mfile.cpp`; only the public
//! API is used.

#![allow(
    clippy::many_single_char_names,
    clippy::manual_is_multiple_of,
    clippy::format_push_string,
    let_underscore_drop,
    clippy::assertions_on_result_states,
    clippy::cast_precision_loss,
    clippy::bool_to_int_with_if,
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::struct_field_names,
    clippy::collapsible_if,
    clippy::single_match_else,
    clippy::doc_markdown,
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    clippy::field_reassign_with_default,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::manual_assert,
    clippy::ignored_unit_patterns,
    clippy::needless_borrows_for_generic_args,
    clippy::unreadable_literal,
    clippy::explicit_iter_loop,
    clippy::uninlined_format_args,
    clippy::if_not_else,
    clippy::redundant_closure_for_method_calls,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::cognitive_complexity,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::bool_assert_comparison,
    clippy::wildcard_imports,
    clippy::assert_is_empty,
    clippy::assigning_clones,
    clippy::double_comparisons,
    clippy::io_other_error,
    clippy::needless_borrow,
    clippy::op_ref,
    clippy::repeat_once,
    clippy::suspicious_map,
    clippy::doc_lazy_continuation,
    clippy::module_name_repetitions
)]

mod common;

use std::io::{self, Read, Write};

use common::*;
use flate2::Compression;
use flate2::write::GzEncoder;
use mychess_pgn::read::{
    DiagnosticKind as K, GameChunker, ParsedGame, PgnError, PgnReader, RawGame, ReadOptions,
    Termination,
};
use pretty_assertions::assert_eq;
use rstest::rstest;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn gz(bytes: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).unwrap();
    enc.finish().unwrap()
}

fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect()
}

const SAMPLE: &str = "[Event \"Torneio\"]\n[White \"Anand\"]\n[Black \"Carlsen\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 {bom test} Nc6 1-0\n";

// ===========================================================================
// BOM and UTF-8
// ===========================================================================

#[test]
fn utf8_bom_is_consumed_and_gives_the_same_game() {
    let mut bytes = BOM.to_vec();
    bytes.extend_from_slice(SAMPLE.as_bytes());
    let with = read_bytes(&bytes, &ReadOptions::default());
    let without = read_all(SAMPLE);
    assert_eq!(with.len(), 1);
    assert_eq!(with[0].game.tags(), without[0].game.tags());
    assert_eq!(dump(&with[0].game), dump(&without[0].game));
    assert!(with[0].diagnostics.is_empty(), "{:?}", with[0].diagnostics);
    assert_eq!(with[0].first_line, 1);
}

#[test]
fn bom_before_a_header_does_not_make_the_first_tag_a_bad_tag() {
    let mut bytes = BOM.to_vec();
    bytes.extend_from_slice(b"[White \"A\"]\n\n*\n");
    let g = read_bytes(&bytes, &ReadOptions::default()).remove(0);
    assert_eq!(g.game.tags().white, "A");
    assert!(!has(&g, |k| *k == K::BadTag || *k == K::JunkBetweenGames));
}

#[test]
fn bom_alone_or_empty_input_has_no_games_and_no_error() {
    for bytes in [&b""[..], &BOM[..], b"\n\n\n", b"   \r\n  "] {
        let mut r = PgnReader::new(bytes, ReadOptions::default());
        assert!(r.next().is_none(), "{bytes:?}");
        assert!(r.next().is_none(), "fused after the end: {bytes:?}");
    }
}

#[test]
fn utf8_text_in_tags_and_comments_is_preserved() {
    let text = "[White \"Rubinstein, Akiba \u{2014} \u{4e2d}\u{6587}\"]\n[Event \"\u{1f600} Caf\u{e9} \u{438}\u{433}\u{440}\u{430}\"]\n\n1. e4 {Pr\u{e9}ludes \u{65e5}\u{672c}\u{8a9e} \u{265f}} e5 *\n";
    let g = one(text);
    assert_eq!(
        g.game.tags().white,
        "Rubinstein, Akiba \u{2014} \u{4e2d}\u{6587}"
    );
    assert_eq!(
        g.game.tags().event,
        "\u{1f600} Caf\u{e9} \u{438}\u{433}\u{440}\u{430}"
    );
    assert_eq!(
        g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
        Some("Pr\u{e9}ludes \u{65e5}\u{672c}\u{8a9e} \u{265f}")
    );
    assert!(!has(&g, |k| *k == K::EncodingFallback));
}

#[test]
fn cyrillic_in_utf8_is_read_exactly() {
    let g = one(
        "[White \"\u{41a}\u{430}\u{440}\u{43f}\u{43e}\u{432}, \u{410}\"]\n[Black \"\u{41a}\u{430}\u{441}\u{43f}\u{430}\u{440}\u{43e}\u{432}\"]\n\n1. e4 {\u{43e}\u{442}\u{43b}\u{438}\u{447}\u{43d}\u{44b}\u{439} \u{445}\u{43e}\u{434}} *\n",
    );
    assert_eq!(
        g.game.tags().white,
        "\u{41a}\u{430}\u{440}\u{43f}\u{43e}\u{432}, \u{410}"
    );
    assert_eq!(
        g.game.tags().black,
        "\u{41a}\u{430}\u{441}\u{43f}\u{430}\u{440}\u{43e}\u{432}"
    );
}

// ===========================================================================
// Latin-1 / windows-1252 / mixed
// ===========================================================================

#[test]
fn latin1_bytes_are_decoded_in_tags_and_comments() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"[White \"M");
    bytes.push(0xFC); // u-umlaut
    bytes.extend_from_slice(b"ller, J\"]\n[Black \"Stra");
    bytes.push(0xDF); // sharp s
    bytes.extend_from_slice(b"er\"]\n[Site \"Z");
    bytes.push(0xFC);
    bytes.extend_from_slice(b"rich\"]\n\n1. e4 {caf");
    bytes.push(0xE9);
    bytes.extend_from_slice(b"} e5 *\n");
    let g = read_bytes(&bytes, &ReadOptions::default()).remove(0);
    assert_eq!(g.game.tags().white, "M\u{fc}ller, J");
    assert_eq!(g.game.tags().black, "Stra\u{df}er");
    assert_eq!(g.game.tags().site, "Z\u{fc}rich");
    assert_eq!(
        g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
        Some("caf\u{e9}")
    );
}

#[test]
fn windows_1252_punctuation_and_euro_are_decoded() {
    // 0x80 euro, 0x92 right single quote, 0x96 en dash, 0x93/0x94 curly quotes
    let mut b = b"[Event \"Prize ".to_vec();
    b.extend_from_slice(&[0x80]);
    b.extend_from_slice(b"500 ");
    b.extend_from_slice(&[0x93]);
    b.extend_from_slice(b"Open");
    b.extend_from_slice(&[0x94, 0x20, 0x96]);
    b.extend_from_slice(b" Kasparov");
    b.extend_from_slice(&[0x92]);
    b.extend_from_slice(b"s\"]\n\n*\n");
    let g = read_bytes(&b, &ReadOptions::default()).remove(0);
    assert_eq!(
        g.game.tags().event,
        "Prize \u{20ac}500 \u{201c}Open\u{201d} \u{2013} Kasparov\u{2019}s"
    );
}

#[test]
fn pound_sign_stays_a_pound_sign_and_is_not_a_chessbase_figure() {
    for text in ["Prize \u{a3}10", "Fee \u{a2}5", "\u{a3}"] {
        let mut b = b"[Event \"".to_vec();
        b.extend(latin1(text));
        b.extend_from_slice(b"\"]\n\n1. e4 {");
        b.extend(latin1(text));
        b.extend_from_slice(b"} *\n");
        let g = read_bytes(&b, &ReadOptions::default()).remove(0);
        assert_eq!(g.game.tags().event, text);
        assert_eq!(
            g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
            Some(text)
        );
    }
}

#[test]
fn move_text_is_never_run_through_the_decoder() {
    let mut b = b"[White \"J".to_vec();
    b.push(0xE9);
    b.extend_from_slice(b"\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *\n");
    let g = read_bytes(&b, &ReadOptions::default()).remove(0);
    assert_eq!(main_sans(&g.game), ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
}

#[test]
fn mixed_file_keeps_utf8_games_intact_next_to_latin1_games() {
    let mut b = Vec::new();
    b.extend_from_slice(
        "[White \"Caf\u{e9} \u{4e2d}\"]\n\n1. e4 {\u{fc}ber \u{20ac}} *\n\n".as_bytes(),
    );
    b.extend_from_slice(b"[White \"M");
    b.push(0xFC);
    b.extend_from_slice(b"ller\"]\n\n1. d4 {gr");
    b.push(0xFC);
    b.extend_from_slice(b"n} *\n\n");
    b.extend_from_slice("[White \"Ren\u{e9}e\"]\n\n1. c4 *\n".as_bytes());
    let games = read_bytes(&b, &ReadOptions::default());
    assert_eq!(games.len(), 3);
    assert_eq!(games[0].game.tags().white, "Caf\u{e9} \u{4e2d}");
    assert_eq!(
        games[0]
            .game
            .comment(games[0].game.mainline().next().unwrap())
            .unwrap(),
        Some("\u{fc}ber \u{20ac}")
    );
    assert_eq!(games[1].game.tags().white, "M\u{fc}ller");
    assert_eq!(
        games[1]
            .game
            .comment(games[1].game.mainline().next().unwrap())
            .unwrap(),
        Some("gr\u{fc}n")
    );
    assert_eq!(games[2].game.tags().white, "Ren\u{e9}e");
}

#[test]
fn undecodable_garbage_never_panics_and_yields_replacement_or_fallback_text() {
    let mut b = b"[White \"".to_vec();
    b.extend_from_slice(&[0xFF, 0xFE, 0xC3, 0x28, 0xA0, 0xA1]);
    b.extend_from_slice(b"\"]\n\n1. e4 {");
    b.extend_from_slice(&[0xC3, 0x28, 0xF0, 0x28, 0x8C, 0xBC]);
    b.extend_from_slice(b"} *\n");
    let g = read_bytes(&b, &ReadOptions::default()).remove(0);
    assert!(g.game.tags().white.chars().count() >= 3);
    assert_eq!(g.game.mainline().count(), 1);
}

#[test]
fn fallback_encoding_defaults_to_windows_1252() {
    let r = PgnReader::new(&b"[White \"A\"]\n\n*\n"[..], ReadOptions::default());
    assert_eq!(r.fallback_encoding().name(), "windows-1252");
}

#[test]
fn encoding_fallback_is_reported_once_as_info() {
    let mut b = Vec::new();
    for i in 0..3 {
        b.extend_from_slice(format!("[Event \"{i}\"]\n[White \"M").as_bytes());
        b.push(0xFC);
        b.extend_from_slice(b"ller\"]\n\n1. e4 *\n\n");
    }
    let games = read_bytes(&b, &ReadOptions::default());
    let infos: Vec<_> = games
        .iter()
        .flat_map(|g| g.diagnostics.iter())
        .filter(|d| d.kind == K::EncodingFallback)
        .collect();
    assert_eq!(infos.len(), 1, "only on the first affected game");
    assert_eq!(infos[0].severity, mychess_pgn::read::Severity::Info);
    assert_eq!(infos[0].game, 1);
}

#[test]
fn cyrillic_windows_1251_file_is_detected_with_enough_evidence() {
    // "Каспаров, Гарри" ... in cp1251; chardetng sees 250+ non-ASCII bytes.
    fn cp1251(s: &str) -> Vec<u8> {
        s.chars()
            .map(|c| match c {
                '\u{410}'..='\u{44f}' => (c as u32 - 0x410 + 0xC0) as u8,
                c => c as u8,
            })
            .collect()
    }
    let phrase = "\u{41a}\u{430}\u{441}\u{43f}\u{430}\u{440}\u{43e}\u{432} \u{43f}\u{43e}\u{431}\u{435}\u{434}\u{438}\u{43b} \u{43a}\u{440}\u{430}\u{441}\u{438}\u{432}\u{43e} \u{438} \u{443}\u{432}\u{435}\u{440}\u{435}\u{43d}\u{43d}\u{43e}";
    let mut b = Vec::new();
    for i in 0..6 {
        b.extend_from_slice(b"[Event \"");
        b.extend(cp1251(phrase));
        b.extend_from_slice(format!("\"]\n[Round \"{i}\"]\n\n1. e4 {{").as_bytes());
        b.extend(cp1251(phrase));
        b.extend_from_slice(b"} *\n\n");
    }
    let games = read_bytes(&b, &ReadOptions::default());
    assert_eq!(games.len(), 6);
    assert_eq!(
        games[5].game.tags().event,
        phrase,
        "windows-1251 is guessed from the accumulated evidence"
    );
}

// ===========================================================================
// Line breaks, NBSP
// ===========================================================================

#[rstest]
#[case("\n")]
#[case("\r\n")]
#[case("\r")]
fn all_line_break_styles_read_the_same_three_games(#[case] eol: &str) {
    let lf = "[Event \"A\"]\n[White \"W\"]\n\n1. e4 e5\n2. Nf3 {a\nb} Nc6 1-0\n\n[Event \"B\"]\n\n1. d4 d5 *\n\n\n[Event \"C\"]\n\n*\n";
    let ref_games = read_all(lf);
    let games = read_all(&lf.replace('\n', eol));
    assert_eq!(games.len(), 3);
    for (a, b) in ref_games.iter().zip(&games) {
        assert_eq!(dump(&a.game), dump(&b.game));
        assert_eq!(a.game.tags(), b.game.tags());
        assert_eq!(a.first_line, b.first_line);
        assert_eq!(a.number, b.number);
        assert_eq!(a.termination, b.termination);
    }
}

#[test]
fn tag_values_never_contain_carriage_returns() {
    for eol in ["\r\n", "\r"] {
        let text = format!("[White \"A\"]{eol}[Black \"B\"]{eol}{eol}1. e4 {{x{eol}y}} *{eol}");
        let g = one(&text);
        assert!(!g.game.tags().white.contains('\r') && !g.game.tags().black.contains('\r'));
        assert_eq!(
            g.game.comment(g.game.mainline().next().unwrap()).unwrap(),
            Some("x y")
        );
    }
}

#[test]
fn utf8_nbsp_between_tokens_counts_as_a_space() {
    let g = mt("1.\u{a0}e4\u{a0}e5\u{a0}2.\u{a0}Nf3 *");
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    assert!(g.diagnostics.is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn latin1_nbsp_byte_between_tokens_counts_as_a_space() {
    let mut b = b"[White \"J".to_vec();
    b.push(0xE9);
    b.extend_from_slice(b"\"]\n\n1. e4");
    b.push(0xA0);
    b.extend_from_slice(b"e5");
    b.push(0xA0);
    b.extend_from_slice(b"2. Nf3 *\n");
    let g = read_bytes(&b, &ReadOptions::default()).remove(0);
    assert_eq!(dump(&g.game), "e4 e5 Nf3");
    assert!(problems(&g).is_empty(), "{:?}", g.diagnostics);
}

#[test]
fn nbsp_at_the_start_of_a_header_line_does_not_hide_the_tag() {
    for bytes in [
        &b"\xc2\xa0[White \"A\"]\n\n1. e4 *\n"[..],
        b"\xa0[White \"A\"]\n\n1. e4 *\n",
    ] {
        let games = read_bytes(bytes, &ReadOptions::default());
        assert_eq!(games.len(), 1, "{bytes:?}");
        assert_eq!(games[0].game.tags().white, "A");
    }
}

#[test]
fn trailing_spaces_and_tabs_after_tags_and_blank_lines_with_spaces() {
    let g = one("[White \"A\"]   \t\n   \n\t\n[Black \"B\"] \n \n1. e4 *\n");
    assert_eq!(
        (g.game.tags().white.as_str(), g.game.tags().black.as_str()),
        ("A", "B")
    );
    assert_eq!(g.game.mainline().count(), 1);
}

#[test]
fn nul_and_control_bytes_in_movetext_are_tolerated() {
    let mut b = b"[White \"A\"]\n\n1. e4 ".to_vec();
    b.extend_from_slice(&[0, 1, 2, 7, 0x1b, 0x7f]);
    b.extend_from_slice(b" e5 *\n");
    let g = read_bytes(&b, &ReadOptions::default()).remove(0);
    assert_eq!(g.game.mainline().count(), 2, "{:?}", g.diagnostics);
}

// ===========================================================================
// Gzip
// ===========================================================================

fn three_games() -> String {
    let mut s = String::new();
    for i in 1..=3 {
        s.push_str(&format!(
            "[Event \"G{i}\"]\n[White \"W{i}\"]\n\n1. e4 e5 2. Nf3 Nc6 {{c{i}}} 1-0\n\n"
        ));
    }
    s
}

#[test]
fn gzip_stream_is_detected_by_magic_bytes() {
    let plain = three_games();
    let games = read_bytes(&gz(plain.as_bytes()), &ReadOptions::default());
    let reference = read_all(&plain);
    assert_eq!(games.len(), 3);
    for (a, b) in games.iter().zip(&reference) {
        assert_eq!(dump(&a.game), dump(&b.game));
        assert_eq!(a.game.tags(), b.game.tags());
        assert_eq!(a.first_line, b.first_line);
        assert_eq!(
            a.byte_range, b.byte_range,
            "offsets are those of the decompressed stream"
        );
    }
}

#[test]
fn multi_member_gzip_is_read_to_the_end() {
    let games = three_games();
    let (a, b) = games.split_at(games.find("[Event \"G3\"]").unwrap());
    let mut bytes = gz(a.as_bytes());
    bytes.extend(gz(b.as_bytes()));
    let got = read_bytes(&bytes, &ReadOptions::default());
    assert_eq!(got.len(), 3);
    assert_eq!(got[2].game.tags().event, "G3");
}

#[test]
fn gzip_with_utf8_bom_inside() {
    let mut inner = BOM.to_vec();
    inner.extend_from_slice(SAMPLE.as_bytes());
    let games = read_bytes(&gz(&inner), &ReadOptions::default());
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].game.tags().white, "Anand");
}

#[test]
fn truncated_gzip_returns_the_complete_games_then_a_gzip_error() {
    let mut plain = String::new();
    // incompressible-ish content so the truncation lands mid-stream
    let mut rng = Lcg(7);
    for i in 0..400 {
        plain.push_str(&format!(
            "[Event \"G{i}\"]\n[White \"P{}\"]\n\n1. e4 e5 2. Nf3 {{{}}} Nc6 1-0\n\n",
            rng.next(),
            rng.next()
        ));
    }
    let full = gz(plain.as_bytes());
    let cut = &full[..full.len() * 2 / 3];
    let results: Vec<_> = PgnReader::new(cut, ReadOptions::default()).collect();
    let oks = results.iter().take_while(|r| r.is_ok()).count();
    assert!(oks > 50 && oks < 400, "{oks}");
    assert!(
        results.len() == oks + 1,
        "exactly one error then the iterator ends (got {} items)",
        results.len()
    );
    match results.last().unwrap() {
        Err(PgnError::Gzip { offset, .. }) => assert!(*offset > 0),
        other => panic!("expected PgnError::Gzip, got {other:?}"),
    }
    for (i, r) in results.iter().take(oks).enumerate() {
        let g = r.as_ref().unwrap();
        assert_eq!(g.number, i as u64 + 1);
        assert_eq!(g.game.tags().event, format!("G{i}"));
    }
}

#[test]
fn gzip_header_followed_by_garbage_is_an_error_not_a_panic() {
    let mut bytes = vec![0x1F, 0x8B, 0x08, 0x00, 0, 0, 0, 0, 0, 0xFF];
    bytes.extend_from_slice(&[0xAB; 64]);
    let results: Vec<_> = PgnReader::new(&bytes[..], ReadOptions::default()).collect();
    assert!(
        matches!(
            results.last(),
            Some(Err(PgnError::Gzip { .. } | PgnError::Io { .. }))
        ),
        "{results:?}"
    );
}

#[test]
fn gzip_truncated_in_the_middle_of_the_header_is_an_error() {
    let full = gz(three_games().as_bytes());
    let results: Vec<_> = PgnReader::new(&full[..6], ReadOptions::default()).collect();
    assert!(results.iter().all(Result::is_err) || results.is_empty());
}

#[test]
fn open_reads_plain_gz_and_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("a.pgn");
    std::fs::write(&plain, three_games()).unwrap();
    let gzp = dir.path().join("b.pgn.gz");
    std::fs::write(&gzp, gz(three_games().as_bytes())).unwrap();
    // a gzip file without the .gz extension is still recognised by its content
    let disguised = dir.path().join("c.pgn");
    std::fs::write(&disguised, gz(three_games().as_bytes())).unwrap();
    for p in [&plain, &gzp, &disguised] {
        let n = PgnReader::open(p, ReadOptions::default())
            .unwrap()
            .map(|r| r.unwrap())
            .count();
        assert_eq!(n, 3, "{}", p.display());
    }
    match PgnReader::open(&dir.path().join("missing.pgn"), ReadOptions::default()) {
        Err(PgnError::Open { path, .. }) => assert!(path.ends_with("missing.pgn")),
        other => panic!("expected Open error, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn open_reports_progress_in_decompressed_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.pgn.gz");
    let plain = three_games();
    std::fs::write(&p, gz(plain.as_bytes())).unwrap();
    let mut r = PgnReader::open(&p, ReadOptions::default()).unwrap();
    assert_eq!(r.bytes_consumed(), 0);
    let mut last = 0;
    while let Some(g) = r.next() {
        g.unwrap();
        assert!(r.bytes_consumed() >= last);
        last = r.bytes_consumed();
    }
    assert!(last > 0 && last <= plain.len() as u64);
}

#[test]
fn progress_reaches_the_stream_length_when_the_iterator_is_exhausted() {
    for text in [
        three_games(),
        "[Event \"A\"]\n\n1. e4 1-0\n".to_owned(),
        "[Event \"A\"]\n\n1. e4 1-0\n\n\n  \n".to_owned(),
    ] {
        let mut r = PgnReader::new(text.as_bytes(), ReadOptions::default());
        while r.next().is_some() {}
        assert_eq!(r.bytes_consumed(), text.len() as u64, "{text:?}");
    }
}

// ===========================================================================
// Hostile `Read` implementations
// ===========================================================================

/// Returns one byte per `read` call.
struct OneByte<'a>(&'a [u8]);

impl Read for OneByte<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.0[0];
        self.0 = &self.0[1..];
        Ok(1)
    }
}

/// Fails with `Interrupted` before every successful read.
struct Interrupting<R> {
    inner: R,
    toggle: bool,
}

impl<R: Read> Read for Interrupting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.toggle = !self.toggle;
        if self.toggle {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        self.inner.read(buf)
    }
}

/// Serves `data`, then fails with an I/O error.
struct FailsAfter<'a> {
    data: &'a [u8],
}

impl Read for FailsAfter<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.data.is_empty() {
            return Err(io::Error::new(io::ErrorKind::Other, "disk on fire"));
        }
        let n = self.data.len().min(buf.len()).min(7);
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

fn rich_pgn() -> Vec<u8> {
    let mut s = String::new();
    s.push('\u{feff}');
    s.push_str("junk before\n\n");
    s.push_str("[Event \"\u{e9}\u{4e2d}\"]\r\n[White \"A (2500)\"]\r\n\r\n1. e4 {multi\r\nline [%clk 0:01:00]\r\n} e5 (1... c5 2. Nf3) 2. Nf3 $14 Nc6 1-0\r\n\r\n");
    s.push_str(
        "[Event \"two\"]\n\n1. d4 d5 2. xyz Nf6 *\n\n[Event \"three\"]\n[Result \"0-1\"]\n\n",
    );
    s.into_bytes()
}

fn summary(games: &[ParsedGame]) -> Vec<(String, String, u64, u64, usize, u64, u64)> {
    games
        .iter()
        .map(|g| {
            (
                g.game.tags().event.clone(),
                dump(&g.game),
                g.number,
                g.first_line,
                g.diagnostics.len(),
                g.byte_range.start,
                g.byte_range.end,
            )
        })
        .collect()
}

#[test]
fn reading_one_byte_per_call_gives_the_same_games_and_diagnostics() {
    let bytes = rich_pgn();
    let whole = read_bytes(&bytes, &ReadOptions::default());
    let slow: Vec<_> = PgnReader::new(OneByte(&bytes), ReadOptions::default())
        .map(Result::unwrap)
        .collect();
    assert_eq!(summary(&slow), summary(&whole));
    for (a, b) in slow.iter().zip(&whole) {
        assert_eq!(a.diagnostics, b.diagnostics);
        assert_eq!(a.game.tags(), b.game.tags());
    }
}

#[test]
fn interrupted_reads_are_retried_transparently() {
    let bytes = rich_pgn();
    let whole = read_bytes(&bytes, &ReadOptions::default());
    let r = Interrupting {
        inner: OneByte(&bytes),
        toggle: false,
    };
    let got: Vec<_> = PgnReader::new(r, ReadOptions::default())
        .map(Result::unwrap)
        .collect();
    assert_eq!(summary(&got), summary(&whole));
}

#[test]
fn every_split_point_of_the_input_reads_the_same() {
    // Cuts a read in two at every offset: catches state lost across buffer refills.
    struct Split<'a> {
        data: &'a [u8],
        at: usize,
        first: bool,
    }
    impl Read for Split<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let limit = if self.first {
                self.at.min(self.data.len())
            } else {
                self.data.len()
            };
            let n = limit.min(buf.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            if self.first && n == limit {
                self.first = false;
            }
            Ok(n)
        }
    }
    let bytes = rich_pgn();
    let whole = summary(&read_bytes(&bytes, &ReadOptions::default()));
    for at in 0..bytes.len() {
        let r = Split {
            data: &bytes,
            at,
            first: true,
        };
        let got: Vec<_> = PgnReader::new(r, ReadOptions::default())
            .map(Result::unwrap)
            .collect();
        assert_eq!(summary(&got), whole, "split at {at}");
    }
}

#[test]
fn io_error_in_the_middle_yields_the_finished_games_then_one_error() {
    let data = three_games();
    let upto = data.find("[Event \"G3\"]").unwrap() + 15; // inside game 3's header
    let results: Vec<_> = PgnReader::new(
        FailsAfter {
            data: &data.as_bytes()[..upto],
        },
        ReadOptions::default(),
    )
    .collect();
    let oks = results.iter().filter(|r| r.is_ok()).count();
    // Games 1 and 2 are complete; the partial game 3 is emitted before the error.
    assert!(oks == 2 || oks == 3, "{results:?}");
    assert_eq!(results.len(), oks + 1, "one error, last: {results:?}");
    assert!(
        matches!(results.last(), Some(Err(PgnError::Io { offset, .. })) if *offset as usize <= upto),
        "{results:?}"
    );
    for (i, r) in results.iter().take(oks).enumerate() {
        assert_eq!(r.as_ref().unwrap().number, i as u64 + 1);
    }
}

#[test]
fn io_error_before_any_data() {
    let mut r = PgnReader::new(FailsAfter { data: b"" }, ReadOptions::default());
    assert!(matches!(r.next(), Some(Err(PgnError::Io { .. }))));
    assert!(r.next().is_none());
}

#[test]
fn error_display_and_source_are_useful() {
    let mut r = PgnReader::new(FailsAfter { data: b"" }, ReadOptions::default());
    let e = r.next().unwrap().unwrap_err();
    let shown = e.to_string();
    assert!(shown.contains("offset"), "{shown}");
    assert!(std::error::Error::source(&e).is_some());
}

/// Produces an endless stream of games and counts what was pulled.
struct Endless {
    produced: std::rc::Rc<std::cell::Cell<u64>>,
    pending: Vec<u8>,
    pos: usize,
    n: u64,
    limit: u64,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.pending.len() {
            if self.n >= self.limit {
                return Ok(0);
            }
            self.n += 1;
            self.pending = format!(
                "[Event \"Game {}\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 {{a comment}} 1-0\n\n",
                self.n
            )
            .into_bytes();
            self.pos = 0;
        }
        let n = buf.len().min(self.pending.len() - self.pos);
        buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
        self.pos += n;
        self.produced.set(self.produced.get() + n as u64);
        Ok(n)
    }
}

#[test]
fn reading_is_lazy_and_bounded_by_the_buffer_not_the_file() {
    let produced = std::rc::Rc::new(std::cell::Cell::new(0));
    let src = Endless {
        produced: produced.clone(),
        pending: Vec::new(),
        pos: 0,
        n: 0,
        limit: 2_000_000,
    };
    let mut r = PgnReader::new(src, ReadOptions::default());
    for i in 1..=100 {
        let g = r.next().unwrap().unwrap();
        assert_eq!(g.number, i);
    }
    // 2M games would be ~350 MB; after 100 games only a couple of read buffers may be pulled.
    assert!(
        produced.get() < 8 * 1024 * 1024,
        "pulled {} bytes for 100 games",
        produced.get()
    );
}

#[test]
#[ignore = "slow: streams ~200 MB of synthetic PGN; run with --ignored"]
fn streaming_two_hundred_megabytes_of_synthetic_pgn() {
    let produced = std::rc::Rc::new(std::cell::Cell::new(0));
    let src = Endless {
        produced: produced.clone(),
        pending: Vec::new(),
        pos: 0,
        n: 0,
        limit: 600_000,
    };
    let mut count = 0u64;
    for g in PgnReader::new(src, ReadOptions::default()) {
        let g = g.unwrap();
        count += 1;
        assert!(g.diagnostics.is_empty());
        assert_eq!(g.number, count);
    }
    assert_eq!(count, 600_000);
    assert!(produced.get() > 150 * 1024 * 1024);
}

// ===========================================================================
// Chunker / RawGame
// ===========================================================================

#[test]
fn chunker_cuts_the_same_games_as_the_reader_and_parse_is_pure() {
    let bytes = rich_pgn();
    let raws: Vec<RawGame> = GameChunker::new(&bytes[..], 64 << 20)
        .map(Result::unwrap)
        .collect();
    let opts = ReadOptions::default();
    let via_raw: Vec<_> = raws.iter().map(|r| r.parse(&opts)).collect();
    let via_reader = read_bytes(&bytes, &opts);
    assert_eq!(summary(&via_raw), summary(&via_reader));
    for (raw, g) in raws.iter().zip(&via_reader) {
        assert_eq!(raw.number(), g.number);
        assert_eq!(raw.start_line(), g.first_line);
        assert_eq!(raw.offset(), g.byte_range.start);
        assert!(
            raw.bytes().starts_with(b"[Event"),
            "{:?}",
            String::from_utf8_lossy(&raw.bytes()[..10.min(raw.bytes().len())])
        );
        // parsing twice gives the same answer
        assert_eq!(summary(&[raw.parse(&opts)]), summary(&[raw.parse(&opts)]));
    }
}

#[test]
fn raw_games_can_be_parsed_in_parallel_and_keep_their_order() {
    fn assert_send<T: Send>() {}
    assert_send::<RawGame>();
    assert_send::<ParsedGame>();
    let text = three_games().repeat(1) + &"[Event \"Z\"]\n\n1. c4 *\n";
    let raws: Vec<RawGame> = GameChunker::new(text.as_bytes(), 64 << 20)
        .map(Result::unwrap)
        .collect();
    let opts = ReadOptions::default();
    let parsed: Vec<_> = std::thread::scope(|s| {
        let hs: Vec<_> = raws.iter().map(|r| s.spawn(|| r.parse(&opts))).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        parsed
            .iter()
            .map(|g| g.game.tags().event.clone())
            .collect::<Vec<_>>(),
        ["G1", "G2", "G3", "Z"]
    );
    assert_eq!(
        parsed.iter().map(|g| g.number).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn scan_tags_returns_raw_pairs_without_reading_moves() {
    let text = "[Event \"E\"]\n[White \"  Doe,J  \"]\n[Weird \"v\"]\n\n1. e4 xyz e5 *\n";
    let raw = GameChunker::new(text.as_bytes(), 64 << 20)
        .next()
        .unwrap()
        .unwrap();
    let tags = raw.scan_tags();
    assert_eq!(
        tags,
        vec![
            ("Event".to_owned(), "E".to_owned()),
            ("White".to_owned(), "  Doe,J  ".to_owned()),
            ("Weird".to_owned(), "v".to_owned()),
        ]
    );
}

#[test]
fn chunker_on_empty_and_headerless_input() {
    assert!(GameChunker::new(&b""[..], 1 << 20).next().is_none());
    assert!(
        GameChunker::new(&b"hello\nworld\n"[..], 1 << 20)
            .next()
            .is_none()
    );
}

#[test]
fn chunker_progress_never_decreases_and_ends_at_the_input_length() {
    let bytes = rich_pgn();
    let mut c = GameChunker::new(&bytes[..], 64 << 20);
    let mut last = 0;
    while let Some(r) = c.next() {
        r.unwrap();
        assert!(c.bytes_consumed() >= last);
        last = c.bytes_consumed();
    }
    assert_eq!(c.bytes_consumed(), bytes.len() as u64);
}

#[test]
fn oversize_game_between_normal_games_resynchronises_on_event_line() {
    let big = "w ".repeat(50_000);
    let text = format!(
        "[Event \"One\"]\n\n1. e4 *\n\n[Event \"Big\"]\n\n1. d4 {{{big}}} d5 *\n[Round \"x\"]\n\n[Event \"Three\"]\n\n1. c4 *\n"
    );
    let o = ReadOptions {
        max_game_bytes: 10_000,
        ..ReadOptions::default()
    };
    let games = read_bytes(text.as_bytes(), &o);
    assert_eq!(
        games
            .iter()
            .map(|g| g.game.tags().event.clone())
            .collect::<Vec<_>>(),
        ["One", "Big", "Three"]
    );
    assert_eq!(games[1].termination, Termination::Oversize);
    assert_eq!(games[2].number, 3);
    assert_eq!(games[2].game.mainline().count(), 1);
    assert!(games[1].byte_range.end <= games[2].byte_range.start);
}

#[test]
fn single_huge_line_without_newlines_is_handled() {
    let mut s = String::from("[Event \"x\"]\n\n");
    for i in 0..30_000 {
        s.push_str(&format!("{}. Nf3 Nf6 {}. Ng1 Ng8 ", 2 * i + 1, 2 * i + 2));
    }
    s.push_str("1/2-1/2");
    let g = one(&s);
    assert_eq!(g.game.mainline().count(), 120_000);
    assert_eq!(g.termination, Termination::ResultToken);
}
