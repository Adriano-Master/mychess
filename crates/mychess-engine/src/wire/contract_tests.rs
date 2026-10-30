//! Contract tests of the line framing (spec m4-1, D411): separation on `\n`,
//! `\r` removed, lossy UTF-8, NUL removed, 1 MiB cap.

use proptest::prelude::*;

use super::*;

fn split_all(chunks: &[&[u8]]) -> Vec<(String, bool)> {
    let mut s = LineSplitter::default();
    let mut out = Vec::new();
    for c in chunks {
        s.feed(c, &mut out);
    }
    out.extend(s.finish());
    out
}

#[test]
fn crlf_cr_and_lf_endings() {
    let lines = split_all(&[b"a\r\nb\nc\r\r\nd"]);
    let texts: Vec<_> = lines.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(texts, ["a", "b", "c", "d"]);
}

#[test]
fn empty_input_and_blank_lines() {
    assert_eq!(split_all(&[b""]).len(), 0);
    let lines = split_all(&[b"\n\r\n\n"]);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|(t, f)| t.is_empty() && !f));
}

#[test]
fn nul_and_invalid_utf8_are_tolerated() {
    let lines = split_all(&[b"a\0b\xff\xfec\n"]);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].0.starts_with("ab"));
    assert!(lines[0].0.ends_with('c'));
    assert!(!lines[0].0.contains('\0'));
}

#[test]
fn a_line_of_exactly_the_limit_is_not_truncated_and_one_more_byte_is() {
    let exact = vec![b'a'; MAX_LINE_BYTES];
    let lines = split_all(&[&exact, b"\n"]);
    assert_eq!((lines[0].0.len(), lines[0].1), (MAX_LINE_BYTES, false));
    let over = vec![b'a'; MAX_LINE_BYTES + 1];
    let lines = split_all(&[&over, b"\nnext\n"]);
    assert_eq!((lines[0].0.len(), lines[0].1), (MAX_LINE_BYTES, true));
    assert_eq!(lines[1], ("next".to_owned(), false));
}

#[test]
fn a_giant_line_split_in_many_chunks_is_cut_the_same_way() {
    let chunk = vec![b'z'; 65_536];
    let mut chunks: Vec<&[u8]> = vec![&chunk; 64]; // 4 MiB
    chunks.push(b"\nok\n");
    let lines = split_all(&chunks);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].1 && lines[0].0.len() == MAX_LINE_BYTES);
    assert_eq!(lines[1].0, "ok");
}

proptest! {
    #[test]
    fn splitting_does_not_depend_on_the_chunking(
        data in proptest::collection::vec(any::<u8>(), 0..3000),
        cuts in proptest::collection::vec(0_usize..3000, 0..10),
    ) {
        let whole = split_all(&[&data]);
        let mut points: Vec<usize> = cuts.into_iter().map(|c| c % (data.len() + 1)).collect();
        points.sort_unstable();
        let mut pieces: Vec<&[u8]> = Vec::new();
        let mut at = 0;
        for p in points {
            pieces.push(&data[at..p]);
            at = p;
        }
        pieces.push(&data[at..]);
        prop_assert_eq!(split_all(&pieces), whole);
    }

    #[test]
    fn every_line_is_bounded_and_free_of_nul_and_trailing_cr(
        data in proptest::collection::vec(any::<u8>(), 0..5000),
    ) {
        for (text, _) in split_all(&[&data]) {
            prop_assert!(text.len() <= 3 * MAX_LINE_BYTES);
            prop_assert!(!text.contains('\0'));
            prop_assert!(!text.ends_with('\r'));
            prop_assert!(!text.contains('\n'));
        }
    }
}
