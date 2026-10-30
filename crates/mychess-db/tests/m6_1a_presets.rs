//! Data tests of the common endings and common patterns (spec
//! `docs/specs/m6-1-buscas-avancadas.md`, items 15 and 17 of "Comportamento
//! observável", D3224). The expected values are typed here from the tables of
//! the spec, not computed.
#![allow(
    clippy::assert_is_empty,
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    unused_qualifications,
    clippy::field_reassign_with_default,
    clippy::unwrap_used,
    clippy::too_many_lines
)]

use mychess_db::{
    MaterialSearch, Pattern, PieceRange, SideRanges, material_presets, pattern_presets,
};
use pretty_assertions::assert_eq;

/// `[queen, rook, bishop, knight, minor, pawn]` as ranges.
fn side(min: [u8; 6], max: [u8; 6]) -> SideRanges {
    let r = |i: usize| PieceRange::new(min[i], max[i]);
    SideRanges {
        queen: r(0),
        rook: r(1),
        bishop: r(2),
        knight: r(3),
        minor: r(4),
        pawn: r(5),
    }
}

fn pat(text: &str) -> Pattern {
    let w: Vec<&str> = text.split(' ').collect();
    Pattern::from_scid(w[0], w[1], w[2], w[3]).unwrap()
}

struct Ending {
    key: &'static str,
    white: ([u8; 6], [u8; 6]),
    black: ([u8; 6], [u8; 6]),
    flip: bool,
    patterns: &'static [&'static str],
}

#[test]
fn the_thirteen_common_endings_have_the_counts_of_the_table() {
    #[rustfmt::skip]
    let table = [
        Ending { key: "ending-pawns", white: ([0,0,0,0,0,1], [0,0,0,0,0,8]), black: ([0,0,0,0,0,1], [0,0,0,0,0,8]), flip: false, patterns: &[] },
        Ending { key: "ending-rook-vs-pawns", white: ([0,1,0,0,0,0], [0,1,0,0,0,0]), black: ([0,0,0,0,0,1], [0,0,0,0,0,8]), flip: false, patterns: &[] },
        Ending { key: "ending-rook-pawn-vs-rook", white: ([0,1,0,0,0,1], [0,1,0,0,0,1]), black: ([0,1,0,0,0,0], [0,1,0,0,0,0]), flip: false, patterns: &[] },
        Ending { key: "ending-rook-pawns-vs-rook", white: ([0,1,0,0,0,1], [0,1,0,0,0,8]), black: ([0,1,0,0,0,0], [0,1,0,0,0,0]), flip: false, patterns: &[] },
        Ending { key: "ending-rooks", white: ([0,1,0,0,0,0], [0,1,0,0,0,8]), black: ([0,1,0,0,0,0], [0,1,0,0,0,8]), flip: false, patterns: &[] },
        Ending { key: "ending-rooks-passed-a", white: ([0,1,0,0,0,1], [0,1,0,0,0,8]), black: ([0,1,0,0,0,0], [0,1,0,0,0,8]), flip: true, patterns: &["wp a ? Yes", "bp a ? No", "bp b ? No"] },
        Ending { key: "ending-rooks-double", white: ([0,2,0,0,0,0], [0,2,0,0,0,8]), black: ([0,2,0,0,0,0], [0,2,0,0,0,8]), flip: false, patterns: &[] },
        Ending { key: "ending-bishops", white: ([0,0,1,0,1,0], [0,0,1,0,1,8]), black: ([0,0,1,0,1,0], [0,0,1,0,1,8]), flip: false, patterns: &[] },
        Ending { key: "ending-bishop-vs-knight", white: ([0,0,1,0,1,0], [0,0,1,0,1,8]), black: ([0,0,0,1,1,0], [0,0,0,1,1,8]), flip: false, patterns: &[] },
        Ending { key: "ending-knights", white: ([0,0,0,1,1,0], [0,0,0,1,1,8]), black: ([0,0,0,1,1,0], [0,0,0,1,1,8]), flip: false, patterns: &[] },
        Ending { key: "ending-queens", white: ([1,0,0,0,0,0], [1,0,0,0,0,8]), black: ([1,0,0,0,0,0], [1,0,0,0,0,8]), flip: false, patterns: &[] },
        Ending { key: "ending-queen-pawn-vs-queen", white: ([1,0,0,0,0,1], [1,0,0,0,0,1]), black: ([1,0,0,0,0,0], [1,0,0,0,0,0]), flip: false, patterns: &[] },
        Ending { key: "ending-bishop-pair-vs-knight-pair", white: ([0,0,2,0,2,0], [1,2,2,0,2,8]), black: ([0,0,0,2,2,0], [1,2,0,2,2,8]), flip: false, patterns: &[] },
    ];
    let presets = material_presets();
    assert_eq!(presets.len(), table.len());
    for (preset, row) in presets.iter().zip(&table) {
        assert_eq!(preset.key, row.key);
        assert_eq!(
            preset.white,
            side(row.white.0, row.white.1),
            "{} white",
            row.key
        );
        assert_eq!(
            preset.black,
            side(row.black.0, row.black.1),
            "{} black",
            row.key
        );
        assert_eq!(preset.ignore_colors, row.flip, "{} flip", row.key);
        let expected: Vec<Pattern> = row.patterns.iter().map(|p| pat(p)).collect();
        assert_eq!(preset.patterns, expected, "{} patterns", row.key);
    }
}

#[test]
fn applying_an_ending_keeps_the_other_options() {
    let preset = &material_presets()[5];
    let mut q = MaterialSearch::default();
    q.moves = (3, 30);
    q.end_only = true;
    preset.apply(&mut q);
    assert_eq!(q.moves, (3, 30));
    assert!(q.end_only);
    assert!(q.ignore_colors);
    assert_eq!(q.patterns.len(), 3);
    assert_eq!(q.white.rook, PieceRange::new(1, 1));
}

struct Preset {
    key: &'static str,
    /// Expected ranges of the white and black pawns after applying to the
    /// default search (0 to 8 pawns), and of other pieces if listed.
    wp: (u8, u8),
    bp: (u8, u8),
    others: &'static [(&'static str, (u8, u8))],
    patterns: &'static [&'static str],
}

#[test]
fn the_fourteen_common_patterns_have_the_patterns_and_counts_of_the_table() {
    #[rustfmt::skip]
    let table = [
        Preset { key: "pattern-white-iqp", wp: (1, 8), bp: (0, 8), others: &[], patterns: &["wp d ? Yes", "wp c ? No", "wp e ? No"] },
        Preset { key: "pattern-white-iqp-break-e6", wp: (1, 8), bp: (1, 8), others: &[], patterns: &["wp d 5 Yes", "wp c ? No", "wp e ? No", "wp d 4 No", "bp e 6 Yes", "bp c ? No", "bp d ? No"] },
        Preset { key: "pattern-white-iqp-break-c6", wp: (1, 8), bp: (1, 8), others: &[], patterns: &["wp d 5 Yes", "wp c ? No", "wp e ? No", "wp d 4 No", "bp c 6 Yes", "bp e ? No", "bp d ? No"] },
        Preset { key: "pattern-black-iqp", wp: (0, 8), bp: (1, 8), others: &[], patterns: &["bp d ? Yes", "bp c ? No", "bp e ? No"] },
        Preset { key: "pattern-white-black-iqp", wp: (1, 8), bp: (1, 8), others: &[], patterns: &["wp d ? Yes", "wp c ? No", "wp e ? No", "bp d ? Yes", "bp c ? No", "bp e ? No"] },
        Preset { key: "pattern-couple-c3-d4", wp: (4, 6), bp: (4, 6), others: &[], patterns: &["wp c 3 Yes", "wp d 4 Yes", "wp b ? No", "wp e ? No", "bp c ? No", "bp d ? No"] },
        Preset { key: "pattern-hanging-c5-d5", wp: (4, 6), bp: (4, 6), others: &[], patterns: &["bp c 5 Yes", "bp d 5 Yes", "bp b ? No", "bp e ? No", "wp c ? No", "wp d ? No"] },
        Preset { key: "pattern-maroczy", wp: (2, 7), bp: (1, 7), others: &[], patterns: &["wp c 4 Yes", "wp e 4 Yes", "bp d ? Yes", "wp d ? No", "bp c ? No", "bp d 5 No"] },
        Preset { key: "pattern-rook-sac-c3", wp: (0, 8), bp: (0, 8), others: &[("wr", (2, 2)), ("br", (2, 2))], patterns: &["br c 3 Yes", "wp b 2 Yes"] },
        Preset { key: "pattern-kc1-kg8", wp: (0, 8), bp: (0, 8), others: &[], patterns: &["wk c 1 Yes", "bk g 8 Yes"] },
        Preset { key: "pattern-kg1-kc8", wp: (0, 8), bp: (0, 8), others: &[], patterns: &["wk g 1 Yes", "bk c 8 Yes"] },
        Preset { key: "pattern-light-fian", wp: (0, 8), bp: (0, 8), others: &[("wb", (1, 2)), ("bb", (1, 2))], patterns: &["wb g 2 Yes", "bb b 7 Yes"] },
        Preset { key: "pattern-dark-fian", wp: (0, 8), bp: (0, 8), others: &[("wb", (1, 2)), ("bb", (1, 2))], patterns: &["wb b 2 Yes", "bb g 7 Yes"] },
        Preset { key: "pattern-four-fian", wp: (0, 8), bp: (0, 8), others: &[("wb", (2, 2)), ("bb", (2, 2))], patterns: &["wb b 2 Yes", "wb g 2 Yes", "bb b 7 Yes", "bb g 7 Yes"] },
    ];
    let presets = pattern_presets();
    assert_eq!(presets.len(), table.len());
    for (preset, row) in presets.iter().zip(&table) {
        assert_eq!(preset.key, row.key);
        let mut q = MaterialSearch::default();
        preset.apply(&mut q);
        assert_eq!(
            (q.white.pawn.min, q.white.pawn.max),
            row.wp,
            "{} wp",
            row.key
        );
        assert_eq!(
            (q.black.pawn.min, q.black.pawn.max),
            row.bp,
            "{} bp",
            row.key
        );
        for (code, want) in row.others {
            let s = if code.starts_with('w') {
                &q.white
            } else {
                &q.black
            };
            let got = match &code[1..] {
                "r" => s.rook,
                "b" => s.bishop,
                _ => unreachable!(),
            };
            assert_eq!((got.min, got.max), *want, "{} {code}", row.key);
        }
        let expected: Vec<Pattern> = row.patterns.iter().map(|p| pat(p)).collect();
        assert_eq!(q.patterns, expected, "{} patterns", row.key);
    }
}

#[test]
fn a_pattern_only_raises_a_minimum_that_is_lower() {
    // The window does not "Zero" before a common pattern (spec, item 17).
    let preset = pattern_presets().remove(0);
    let mut q = MaterialSearch::default();
    q.white.pawn = PieceRange::new(3, 5);
    preset.apply(&mut q);
    assert_eq!(q.white.pawn, PieceRange::new(3, 5));
}
