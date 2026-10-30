//! Contract tests of the NDJSON framer (spec m8-3, D1309; acceptance criterion
//! "`LineFramer`"). Derived from the spec, not from the implementation.

#![allow(clippy::assert_is_empty)]
use mychess_online::{
    Error,
    ndjson::{Line, LineFramer, MAX_LINE_LEN},
};
use proptest::prelude::*;

fn text(s: &str) -> Line {
    Line::Text(s.to_owned())
}

/// Feeds `wire` cut at the given sizes (cycled) and returns every line plus
/// the flushed tail.
fn feed(wire: &[u8], cuts: &[usize]) -> Vec<Line> {
    let mut framer = LineFramer::new();
    let mut out = Vec::new();
    let mut pos = 0;
    let mut i = 0;
    while pos < wire.len() {
        let n = cuts[i % cuts.len()].max(1).min(wire.len() - pos);
        out.extend(framer.push(&wire[pos..pos + n]).expect("no overflow"));
        pos += n;
        i += 1;
    }
    out.extend(framer.finish_stream());
    out
}

#[test]
fn max_line_len_is_one_mebibyte() {
    assert_eq!(MAX_LINE_LEN, 1024 * 1024);
}

#[test]
fn every_two_chunk_split_of_a_realistic_wire_gives_the_same_lines() {
    let wire = b"{\"type\":\"gameFull\",\"id\":\"abcd1234\"}\r\n\n{\"type\":\"gameState\"}\r\n{\"x\":\"\xc3\xa9\xe2\x82\xac\"}\n";
    let expected = feed(wire, &[wire.len()]);
    assert_eq!(
        expected,
        [
            text("{\"type\":\"gameFull\",\"id\":\"abcd1234\"}"),
            Line::KeepAlive,
            text("{\"type\":\"gameState\"}"),
            text("{\"x\":\"\u{e9}\u{20ac}\"}"),
        ]
    );
    for cut in 0..=wire.len() {
        let mut f = LineFramer::new();
        let mut got = f.push(&wire[..cut]).expect("first");
        got.extend(f.push(&wire[cut..]).expect("second"));
        assert_eq!(got, expected, "cut at byte {cut}");
        assert_eq!(f.pending(), 0);
    }
}

#[test]
fn byte_by_byte_delivery_gives_the_same_lines() {
    let wire = "{\"a\":1}\r\n\r\n{\"b\":\"caf\u{e9}\"}\n\n".as_bytes();
    assert_eq!(feed(wire, &[1]), feed(wire, &[wire.len()]));
}

#[test]
fn crlf_split_after_the_carriage_return_is_one_terminator() {
    let mut f = LineFramer::new();
    assert!(f.push(b"{\"a\":1}\r").expect("push").is_empty());
    assert_eq!(f.pending(), 8);
    assert_eq!(f.push(b"\n").expect("push"), [text("{\"a\":1}")]);
    // A split CRLF that is the whole line is a keep-alive.
    assert!(f.push(b"\r").expect("push").is_empty());
    assert_eq!(f.push(b"\n").expect("push"), [Line::KeepAlive]);
}

#[test]
fn only_the_last_carriage_return_is_stripped_and_inner_ones_stay() {
    let mut f = LineFramer::new();
    let got = f.push(b"a\rb\n").expect("push");
    assert_eq!(got, [text("a\rb")]);
    let got = f.push(b"a\r\r\n").expect("push");
    assert_eq!(got, [text("a\r")]);
}

#[test]
fn empty_lines_are_keepalives_in_any_terminator_style() {
    let mut f = LineFramer::new();
    let got = f.push(b"\n\r\n\n").expect("push");
    assert_eq!(got, [Line::KeepAlive, Line::KeepAlive, Line::KeepAlive]);
    // A bare "\r" line (CR then LF) is also a keep-alive.
    assert_eq!(f.push(b"\r\n").expect("push"), [Line::KeepAlive]);
}

#[test]
fn an_empty_chunk_yields_nothing_and_changes_nothing() {
    let mut f = LineFramer::new();
    assert!(f.push(b"").expect("push").is_empty());
    assert!(f.push(b"ab").expect("push").is_empty());
    assert!(f.push(b"").expect("push").is_empty());
    assert_eq!(f.pending(), 2);
    assert_eq!(f.push(b"c\n").expect("push"), [text("abc")]);
}

#[test]
fn a_multibyte_character_split_between_chunks_is_not_invalid() {
    // U+20AC is E2 82 AC; cut after each of its bytes.
    for cut in 1..3 {
        let bytes = "\u{20ac}\n".as_bytes();
        let mut f = LineFramer::new();
        assert!(f.push(&bytes[..cut]).expect("push").is_empty());
        assert_eq!(f.push(&bytes[cut..]).expect("push"), [text("\u{20ac}")]);
    }
}

#[test]
fn invalid_utf8_becomes_invalid_and_the_next_line_is_unaffected() {
    let mut f = LineFramer::new();
    let got = f
        .push(b"{\"a\":\"\xff\xfe\"}\n{\"ok\":true}\n")
        .expect("push");
    assert_eq!(got.len(), 2);
    match &got[0] {
        Line::Invalid(s) => assert!(s.contains("{\"a\":"), "lossy text keeps the valid part"),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(got[1], text("{\"ok\":true}"));
}

#[test]
fn a_truncated_multibyte_sequence_at_end_of_line_is_invalid() {
    let mut f = LineFramer::new();
    let got = f.push(b"abc\xe2\x82\n").expect("push");
    assert!(matches!(got.as_slice(), [Line::Invalid(_)]), "{got:?}");
}

#[test]
fn a_line_of_exactly_the_limit_is_accepted_and_one_more_byte_is_not() {
    let mut f = LineFramer::new();
    let mut wire = vec![b'a'; MAX_LINE_LEN];
    wire.push(b'\n');
    let got = f.push(&wire).expect("exactly the limit");
    assert_eq!(got.len(), 1);

    let mut g = LineFramer::new();
    let mut wire = vec![b'a'; MAX_LINE_LEN + 1];
    wire.push(b'\n');
    assert_eq!(g.push(&wire), Err(Error::LineTooLong));
}

#[test]
fn an_oversized_line_is_rejected_even_without_a_newline_and_across_chunks() {
    let mut f = LineFramer::new();
    let piece = vec![b'x'; MAX_LINE_LEN / 2];
    assert!(f.push(&piece).expect("half").is_empty());
    assert!(f.push(&piece).expect("full").is_empty());
    assert_eq!(f.push(b"y"), Err(Error::LineTooLong));
}

#[test]
fn many_short_lines_in_one_big_chunk_are_not_an_overflow() {
    // The limit is per line, not per chunk.
    let mut f = LineFramer::new();
    let wire: Vec<u8> = b"0123456789\n".repeat(MAX_LINE_LEN / 10);
    assert!(wire.len() > MAX_LINE_LEN);
    let got = f.push(&wire).expect("short lines");
    assert_eq!(got.len(), MAX_LINE_LEN / 10);
}

#[test]
fn a_line_of_exactly_the_limit_with_crlf_is_not_above_the_limit() {
    // The spec says "linha acima de 1 MiB": the CR belongs to the terminator.
    let mut f = LineFramer::new();
    let mut wire = vec![b'a'; MAX_LINE_LEN];
    wire.extend_from_slice(b"\r\n");
    let got = f.push(&wire);
    assert!(
        got.as_ref().is_ok_and(|l| l.len() == 1),
        "a CRLF line with 1 MiB of content must be accepted, got {:?}",
        got.map(|l| l.len())
    );
}

#[test]
fn custom_limit_applies_per_line() {
    let mut f = LineFramer::with_max_len(4);
    assert_eq!(f.push(b"abcd\nefgh\n").expect("ok").len(), 2);
    assert_eq!(f.push(b"abcde\n"), Err(Error::LineTooLong));
}

#[test]
fn the_unterminated_tail_is_flushed_once_and_empty_means_nothing() {
    let mut f = LineFramer::new();
    assert_eq!(f.finish_stream(), None);
    assert_eq!(f.push(b"{}\n{\"cut\"").expect("push").len(), 1);
    assert_eq!(f.finish_stream(), Some(text("{\"cut\"")));
    assert_eq!(f.finish_stream(), None);
    assert_eq!(f.pending(), 0);
}

proptest! {
    #[test]
    fn any_fragmentation_of_the_same_bytes_gives_the_same_lines(
        wire in proptest::collection::vec(
            prop_oneof![
                4 => proptest::char::range(' ', '~').prop_map(|c| c as u8),
                2 => Just(b'\n'),
                2 => Just(b'\r'),
                1 => Just(0xC3u8),
                1 => Just(0xA9u8),
                1 => Just(0xFFu8),
            ],
            0..300,
        ),
        cuts in proptest::collection::vec(1usize..40, 1..30),
    ) {
        let whole = feed(&wire, &[wire.len().max(1)]);
        let pieces = feed(&wire, &cuts);
        prop_assert_eq!(pieces, whole);
    }

    #[test]
    fn unicode_lines_survive_any_cut_including_inside_a_codepoint(
        lines in proptest::collection::vec("[^\\n\\r]{0,30}", 0..8),
        cuts in proptest::collection::vec(1usize..7, 1..20),
    ) {
        let mut wire = Vec::new();
        for l in &lines {
            wire.extend_from_slice(l.as_bytes());
            wire.extend_from_slice(b"\r\n");
        }
        let got = feed(&wire, &cuts);
        let want: Vec<Line> = lines
            .iter()
            .map(|l| if l.is_empty() { Line::KeepAlive } else { Line::Text(l.clone()) })
            .collect();
        prop_assert_eq!(got, want);
    }
}
