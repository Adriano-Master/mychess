// Derived from Scid vs PC 4.27, tcl/search/material.tcl (`::search::material::
// CommonEndings` and `CommonPatterns` menus). Copyright (c) 1999-2003 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The "Common Endings" and "Common Patterns" menus of the material search
//! window as data (decision D3224 of `docs/specs/m6-1-buscas-avancadas.md`).
//! The interface only lists them; their names come from the translation files
//! under the `key` of each preset.

use mychess_core::{Color, Role};

use super::material::{MaterialSearch, Pattern, SideRanges};

/// A common ending: it replaces the ranges of the search (the window does
/// "Zero" first, so what is not listed is 0 to 0), the "ignore colors" box
/// and the patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndingPreset {
    /// A stable name, such as `ending-rooks`, for the translation key.
    pub key: &'static str,
    /// White's ranges.
    pub white: SideRanges,
    /// Black's ranges.
    pub black: SideRanges,
    /// The value of the "ignore colors" box.
    pub ignore_colors: bool,
    /// The patterns.
    pub patterns: Vec<Pattern>,
}

impl EndingPreset {
    /// Applies the ending to `search` (the other options are kept).
    pub fn apply(&self, search: &mut MaterialSearch) {
        search.white = self.white;
        search.black = self.black;
        search.ignore_colors = self.ignore_colors;
        search.patterns.clone_from(&self.patterns);
    }
}

/// How a common pattern changes a piece count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountAdjust {
    /// Raise the minimum to at least this (the maximum is kept, then made at
    /// least the minimum).
    AtLeast(u8),
    /// Set the minimum and the maximum.
    Range(u8, u8),
}

/// A common pattern: some adjustments of the counts and the patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternPreset {
    /// A stable name, such as `pattern-maroczy`, for the translation key.
    pub key: &'static str,
    /// The adjustments of the counts, by colour and piece.
    pub counts: Vec<(Color, Role, CountAdjust)>,
    /// The patterns (they replace the ones of the search).
    pub patterns: Vec<Pattern>,
}

impl PatternPreset {
    /// Applies the preset to `search`: the counts start from what the search
    /// already has (the window does not do "Zero" here).
    pub fn apply(&self, search: &mut MaterialSearch) {
        for &(color, role, adjust) in &self.counts {
            let side = match color {
                Color::White => &mut search.white,
                Color::Black => &mut search.black,
            };
            if let Some(range) = side.get_mut(role) {
                match adjust {
                    CountAdjust::AtLeast(n) => range.min = range.min.max(n),
                    CountAdjust::Range(lo, hi) => {
                        range.min = lo;
                        range.max = hi;
                    }
                }
            }
        }
        search.patterns.clone_from(&self.patterns);
        *search = search.normalized();
    }
}

/// `"wp"` as colour and role.
fn piece(code: &str) -> Option<(Color, Role)> {
    let mut chars = code.chars();
    let color = match chars.next()? {
        'w' => Color::White,
        'b' => Color::Black,
        _ => return None,
    };
    let role = Role::from_char(chars.next()?)?;
    Some((color, role))
}

/// The six ranges `q r b n m p` are addressed here by the Scid names
/// `wq bq ... wm bm`; `m` is the minor pieces.
fn set_range(white: &mut SideRanges, black: &mut SideRanges, code: &str, is_min: bool, n: u8) {
    let Some((color, kind)) = code.chars().next().zip(code.chars().nth(1)) else {
        return;
    };
    let side = if color == 'w' { white } else { black };
    let range = match kind {
        'q' => &mut side.queen,
        'r' => &mut side.rook,
        'b' => &mut side.bishop,
        'n' => &mut side.knight,
        'm' => &mut side.minor,
        'p' => &mut side.pawn,
        _ => return,
    };
    if is_min {
        range.min = n;
    } else {
        range.max = n;
    }
}

fn pattern(text: &str) -> Pattern {
    let mut words = text.split_whitespace();
    Pattern::from_scid(
        words.next().unwrap_or("?"),
        words.next().unwrap_or("?"),
        words.next().unwrap_or("?"),
        words.next().unwrap_or("Yes"),
    )
    .unwrap_or_default()
}

fn ending(
    key: &'static str,
    mins: &[(&str, u8)],
    maxs: &[(&str, u8)],
    ignore_colors: bool,
    patterns: &[&str],
) -> EndingPreset {
    let mut white = SideRanges::ZERO;
    let mut black = SideRanges::ZERO;
    for &(code, n) in mins {
        set_range(&mut white, &mut black, code, true, n);
    }
    for &(code, n) in maxs {
        set_range(&mut white, &mut black, code, false, n);
    }
    EndingPreset {
        key,
        white,
        black,
        ignore_colors,
        patterns: patterns.iter().map(|p| pattern(p)).collect(),
    }
}

/// The 13 common endings, in the order of the menu of Scid.
// A table of data: its length is the number of rows.
#[allow(clippy::too_many_lines)]
pub fn material_presets() -> Vec<EndingPreset> {
    vec![
        ending(
            "ending-pawns",
            &[("wp", 1), ("bp", 1)],
            &[("wp", 8), ("bp", 8)],
            false,
            &[],
        ),
        ending(
            "ending-rook-vs-pawns",
            &[("wr", 1), ("bp", 1)],
            &[("wr", 1), ("bp", 8)],
            false,
            &[],
        ),
        ending(
            "ending-rook-pawn-vs-rook",
            &[("wr", 1), ("br", 1), ("wp", 1)],
            &[("wr", 1), ("br", 1), ("wp", 1)],
            false,
            &[],
        ),
        ending(
            "ending-rook-pawns-vs-rook",
            &[("wr", 1), ("br", 1), ("wp", 1)],
            &[("wr", 1), ("br", 1), ("wp", 8)],
            false,
            &[],
        ),
        ending(
            "ending-rooks",
            &[("wr", 1), ("br", 1)],
            &[("wr", 1), ("br", 1), ("wp", 8), ("bp", 8)],
            false,
            &[],
        ),
        ending(
            "ending-rooks-passed-a",
            &[("wr", 1), ("br", 1), ("wp", 1)],
            &[("wr", 1), ("br", 1), ("wp", 8), ("bp", 8)],
            true,
            &["wp a ? Yes", "bp a ? No", "bp b ? No"],
        ),
        ending(
            "ending-rooks-double",
            &[("wr", 2), ("br", 2)],
            &[("wr", 2), ("br", 2), ("wp", 8), ("bp", 8)],
            false,
            &[],
        ),
        ending(
            "ending-bishops",
            &[("wb", 1), ("bb", 1), ("wm", 1), ("bm", 1)],
            &[
                ("wb", 1),
                ("bb", 1),
                ("wm", 1),
                ("bm", 1),
                ("wp", 8),
                ("bp", 8),
            ],
            false,
            &[],
        ),
        ending(
            "ending-bishop-vs-knight",
            &[("wb", 1), ("bn", 1), ("wm", 1), ("bm", 1)],
            &[
                ("wb", 1),
                ("bn", 1),
                ("wm", 1),
                ("bm", 1),
                ("wp", 8),
                ("bp", 8),
            ],
            false,
            &[],
        ),
        ending(
            "ending-knights",
            &[("wn", 1), ("bn", 1), ("wm", 1), ("bm", 1)],
            &[
                ("wn", 1),
                ("bn", 1),
                ("wm", 1),
                ("bm", 1),
                ("wp", 8),
                ("bp", 8),
            ],
            false,
            &[],
        ),
        ending(
            "ending-queens",
            &[("wq", 1), ("bq", 1)],
            &[("wq", 1), ("bq", 1), ("wp", 8), ("bp", 8)],
            false,
            &[],
        ),
        ending(
            "ending-queen-pawn-vs-queen",
            &[("wq", 1), ("bq", 1), ("wp", 1)],
            &[("wq", 1), ("bq", 1), ("wp", 1)],
            false,
            &[],
        ),
        ending(
            "ending-bishop-pair-vs-knight-pair",
            &[("wb", 2), ("bn", 2), ("wm", 2), ("bm", 2)],
            &[
                ("wq", 1),
                ("bq", 1),
                ("wr", 2),
                ("br", 2),
                ("wb", 2),
                ("bn", 2),
                ("wm", 2),
                ("bm", 2),
                ("wp", 8),
                ("bp", 8),
            ],
            false,
            &[],
        ),
    ]
}

fn pattern_preset(
    key: &'static str,
    counts: &[(&str, CountAdjust)],
    patterns: &[&str],
) -> PatternPreset {
    PatternPreset {
        key,
        counts: counts
            .iter()
            .filter_map(|&(code, adjust)| piece(code).map(|(c, r)| (c, r, adjust)))
            .collect(),
        patterns: patterns.iter().map(|p| pattern(p)).collect(),
    }
}

/// The 14 common patterns, in the order of the menu of Scid.
// A table of data: its length is the number of rows.
#[allow(clippy::too_many_lines)]
pub fn pattern_presets() -> Vec<PatternPreset> {
    use CountAdjust::{AtLeast, Range};
    vec![
        pattern_preset(
            "pattern-white-iqp",
            &[("wp", AtLeast(1))],
            &["wp d ? Yes", "wp c ? No", "wp e ? No"],
        ),
        pattern_preset(
            "pattern-white-iqp-break-e6",
            &[("wp", AtLeast(1)), ("bp", AtLeast(1))],
            &[
                "wp d 5 Yes",
                "wp c ? No",
                "wp e ? No",
                "wp d 4 No",
                "bp e 6 Yes",
                "bp c ? No",
                "bp d ? No",
            ],
        ),
        pattern_preset(
            "pattern-white-iqp-break-c6",
            &[("wp", AtLeast(1)), ("bp", AtLeast(1))],
            &[
                "wp d 5 Yes",
                "wp c ? No",
                "wp e ? No",
                "wp d 4 No",
                "bp c 6 Yes",
                "bp e ? No",
                "bp d ? No",
            ],
        ),
        pattern_preset(
            "pattern-black-iqp",
            &[("bp", AtLeast(1))],
            &["bp d ? Yes", "bp c ? No", "bp e ? No"],
        ),
        pattern_preset(
            "pattern-white-black-iqp",
            &[("wp", AtLeast(1)), ("bp", AtLeast(1))],
            &[
                "wp d ? Yes",
                "wp c ? No",
                "wp e ? No",
                "bp d ? Yes",
                "bp c ? No",
                "bp e ? No",
            ],
        ),
        pattern_preset(
            "pattern-couple-c3-d4",
            &[("wp", Range(4, 6)), ("bp", Range(4, 6))],
            &[
                "wp c 3 Yes",
                "wp d 4 Yes",
                "wp b ? No",
                "wp e ? No",
                "bp c ? No",
                "bp d ? No",
            ],
        ),
        pattern_preset(
            "pattern-hanging-c5-d5",
            &[("bp", Range(4, 6)), ("wp", Range(4, 6))],
            &[
                "bp c 5 Yes",
                "bp d 5 Yes",
                "bp b ? No",
                "bp e ? No",
                "wp c ? No",
                "wp d ? No",
            ],
        ),
        pattern_preset(
            "pattern-maroczy",
            &[("bp", Range(1, 7)), ("wp", Range(2, 7))],
            &[
                "wp c 4 Yes",
                "wp e 4 Yes",
                "bp d ? Yes",
                "wp d ? No",
                "bp c ? No",
                "bp d 5 No",
            ],
        ),
        pattern_preset(
            "pattern-rook-sac-c3",
            &[("br", Range(2, 2)), ("wr", Range(2, 2))],
            &["br c 3 Yes", "wp b 2 Yes"],
        ),
        pattern_preset("pattern-kc1-kg8", &[], &["wk c 1 Yes", "bk g 8 Yes"]),
        pattern_preset("pattern-kg1-kc8", &[], &["wk g 1 Yes", "bk c 8 Yes"]),
        pattern_preset(
            "pattern-light-fian",
            &[("wb", AtLeast(1)), ("bb", AtLeast(1))],
            &["wb g 2 Yes", "bb b 7 Yes"],
        ),
        pattern_preset(
            "pattern-dark-fian",
            &[("wb", AtLeast(1)), ("bb", AtLeast(1))],
            &["wb b 2 Yes", "bb g 7 Yes"],
        ),
        pattern_preset(
            "pattern-four-fian",
            &[("wb", AtLeast(2)), ("bb", AtLeast(2))],
            &["wb b 2 Yes", "wb g 2 Yes", "bb b 7 Yes", "bb g 7 Yes"],
        ),
    ]
}
