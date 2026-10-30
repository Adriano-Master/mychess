// Table derived from Scid vs PC 4.27, src/naglatex.h.
// Copyright (c) 2000-2013 Shane Hudson and others; updated by W. van den
// Akker. Scid is distributed under the GNU General Public License (see its
// COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! NAG to LaTeX (`xskak`) translation (decision D3710 of
//! `docs/specs/m7-4-exportacao.md`).
//!
//! Two corrections to the Scid table: NAG 136 is `\timelimit` (Scid has the
//! typo `\timelimt`, which does not compile) and NAG 48/49 are
//! `{$\boxplus$}` (Scid has `{$$\boxplus$$}`, display math inside a line).

use mychess_core::Nag;

/// The LaTeX text for `nag`, or `None` when Scid's table has no translation
/// (an empty entry), when the NAG is above 215, or for the diagram NAG
/// (`D`, `$201`), which the writer turns into a `\chessboard`.
///
/// Entries that are `xskak` macros start with a space, so they can be glued to
/// the move before them.
///
/// ```
/// use mychess_core::Nag;
/// use mychess_pgn::export::latex_nag;
///
/// assert_eq!(latex_nag(Nag::new(1).unwrap()), Some("!"));
/// assert_eq!(latex_nag(Nag::new(11).unwrap()), None);
/// ```
#[allow(clippy::match_same_arms)] // one arm per NAG, 1:1 with `naglatex.h`
pub fn latex_nag(nag: Nag) -> Option<&'static str> {
    Some(match nag.value() {
        1 => "!",
        2 => "?",
        3 => "!!",
        4 => "??",
        5 => "!?",
        6 => "?!",
        7 => "forced",
        8 => " \\xskakcomment{\\onlymove}",
        9 => "worst",
        10 => " \\xskakcomment{\\equal}",
        12 => "{$\\leftrightarrows$}",
        13 => " \\xskakcomment{\\unclear}",
        14 => " \\xskakcomment{\\wbetter}",
        15 => " \\xskakcomment{\\bbetter}",
        16 => " \\xskakcomment{\\wupperhand}",
        17 => " \\xskakcomment{\\bupperhand}",
        18 => " \\xskakcomment{\\wdecisive}",
        19 => " \\xskakcomment{\\bdecisive}",
        22 => " \\xskakcomment{\\zugzwang}",
        23 => " \\xskakcomment{\\zugzwang}",
        26 => " \\xskakcomment{\\moreroom}",
        30 => "{$\\circlearrowleft$}",
        31 => "{$\\circlearrowright$}",
        35 => " \\xskakcomment{\\devadvantage}",
        36 => " \\xskakcomment{\\withinit}",
        40 => " \\xskakcomment{\\withattack}",
        44 => " \\xskakcomment{\\compensation}",
        48 => "{$\\boxplus$}",
        49 => "{$\\boxplus$}",
        50 => " \\xskakcomment{\\centre}",
        51 => " \\xskakcomment{\\centre}",
        58 => " \\xskakcomment{\\kside}",
        62 => " \\xskakcomment{\\qside}",
        132 => " \\xskakcomment{\\counterplay}",
        136 => " \\xskakcomment{\\timelimit}",
        140 => " \\xskakcomment{\\withidea}",
        142 => " \\xskakcomment{\\betteris}",
        144 => " \\xskakcomment{\\various}",
        145 => " \\xskakcomment{\\comment}",
        146 => " \\xskakcomment{\\novelty}",
        147 => " \\xskakcomment{\\weakpt}",
        148 => " \\xskakcomment{\\ending}",
        149 => " \\xskakcomment{\\file}",
        150 => " \\xskakcomment{\\diagonal}",
        151 => " \\xskakcomment{\\bishoppair}",
        153 => " \\xskakcomment{\\opposbishops}",
        154 => " \\xskakcomment{\\samebishops}",
        190 => " \\xskakcomment{\\etc}",
        191 => " \\xskakcomment{\\doublepawns}",
        192 => " \\xskakcomment{\\seppawns}",
        193 => " \\xskakcomment{\\unitedpawns}",
        210 => " \\xskakcomment{\\see}",
        211 => " \\xskakcomment{\\mate}",
        212 => " \\xskakcomment{\\passedpawn}",
        213 => " \\xskakcomment{\\morepawns}",
        214 => " \\xskakcomment{\\with}",
        215 => " \\xskakcomment{\\without}",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nag(n: u16) -> Nag {
        Nag::new(n).unwrap()
    }

    #[test]
    fn plain_symbols() {
        assert_eq!(latex_nag(nag(1)), Some("!"));
        assert_eq!(latex_nag(nag(2)), Some("?"));
        assert_eq!(latex_nag(nag(3)), Some("!!"));
    }

    #[test]
    fn typo_and_display_math_are_corrected() {
        let t = latex_nag(nag(136)).unwrap();
        assert!(t.contains("\\timelimit}") && !t.contains("timelimt"));
        for n in [48, 49] {
            let t = latex_nag(nag(n)).unwrap();
            assert!(t.contains("$\\boxplus$") && !t.contains("$$"));
        }
    }

    #[test]
    fn untranslated_and_out_of_table_are_none() {
        for n in [11, 20, 201, 216, 255] {
            assert_eq!(latex_nag(nag(n)), None, "NAG {n}");
        }
    }
}
