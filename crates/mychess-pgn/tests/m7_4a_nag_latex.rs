//! M7-4a, criterion 3: `latex_nag` against the table of Scid's `naglatex.h`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_core::Nag;
use mychess_pgn::export::latex_nag;

fn nag(n: u16) -> Nag {
    Nag::new(n).unwrap()
}

#[test]
fn first_three_are_the_move_symbols() {
    assert_eq!(latex_nag(nag(1)), Some("!"));
    assert_eq!(latex_nag(nag(2)), Some("?"));
    assert_eq!(latex_nag(nag(3)), Some("!!"));
}

#[test]
fn nag_136_is_spelled_timelimit_and_48_49_use_inline_math() {
    assert!(latex_nag(nag(136)).unwrap().contains("\\timelimit}"));
    for n in [48, 49] {
        let t = latex_nag(nag(n)).unwrap();
        assert!(t.contains("$\\boxplus$"), "{t}");
        assert!(!t.contains("$$"), "{t}");
    }
}

#[test]
fn nags_without_translation_give_none() {
    for n in [11, 21, 100, 201, 209, 216, 255] {
        assert_eq!(latex_nag(nag(n)), None, "NAG {n}");
    }
}

#[test]
fn every_translation_is_balanced_latex() {
    for n in 1..=255u16 {
        if let Some(t) = latex_nag(nag(n)) {
            assert_eq!(
                t.matches('{').count(),
                t.matches('}').count(),
                "NAG {n}: {t}"
            );
        }
    }
}

/// Reads `naglatex.h` of the Scid reference (run with `cargo xtask compat`).
#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_table_matches_naglatex_h_but_for_the_two_corrections() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reference/scid_vs_pc-4.27/src/naglatex.h");
    assert!(
        path.is_file(),
        "Scid vs PC reference not found at {}",
        path.display()
    );
    let bytes = std::fs::read(&path).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let entries: Vec<String> = text
        .lines()
        .filter(|l| l.trim_start().starts_with('"'))
        .map(|l| {
            let start = l.find('"').unwrap() + 1;
            let end = l[start..].rfind("\",").map_or(l.len(), |e| e + start);
            let mut s = l[start..end].replace("\\\\", "\\");
            // Drop the closing quote of the last column when no comma follows.
            if s.ends_with('"') {
                s.pop();
            }
            s
        })
        .collect();
    assert_eq!(entries.len(), 216);
    for (n, scid) in entries.iter().enumerate().skip(1) {
        let ours = latex_nag(nag(u16::try_from(n).unwrap()));
        let expected = match n {
            136 => Some(" \\xskakcomment{\\timelimit}"),
            48 | 49 => Some("{$\\boxplus$}"),
            _ if scid.is_empty() => None,
            _ => Some(scid.as_str()),
        };
        assert_eq!(ours, expected, "NAG {n}");
    }
}
