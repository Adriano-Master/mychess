//! Pure text formatting of the analysis lines (spec M4-4, D439, D441).
//!
//! Everything here is a function of its arguments, so the tables of the spec
//! are tested one by one. The Scid keeps the same code in `analysis.tcl`
//! (`scoreToMate`, `addMoveNumbers`, `formatAnalysisScore`,
//! `calculateNodes`).

use std::fmt::Write as _;

use mychess_core::{Color, Move, Position};
use mychess_engine::{Bound, Score, Wdl};

use crate::prefs::AnalysisPrefs;

/// The score the Scid gives to a checkmate or a stalemate position that has
/// no engine to ask (`getScoreNoMoves`, `analysis.tcl` 2682-2696), in
/// centipawns: 327.0 pawns.
pub const NO_MOVES_MATE_CP: i32 = 32_700;

/// Whether a move number is written `12.e4` or `12. e4` (the notation
/// preference `move_number_space`, the Scid's `moveNumberSpaces`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberStyle {
    /// `12.e4`, `12...e5`.
    #[default]
    Tight,
    /// `12. e4`, `12. ... e5` (the Scid writes the space before the dots
    /// too).
    Spaced,
}

impl From<bool> for NumberStyle {
    fn from(spaced: bool) -> NumberStyle {
        if spaced {
            NumberStyle::Spaced
        } else {
            NumberStyle::Tight
        }
    }
}

/// `+0.35`, `-1.20`, with the sign always written.
fn centipawns_text(cp: i32) -> String {
    let abs = i64::from(cp).abs();
    let sign = if cp < 0 { '-' } else { '+' };
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// The score as the lines show it (D439): centipawns as `+0.35`, a mate as
/// `+M3` (White mates) or `-M3` (Black mates); an exact bound gets no mark,
/// a lower bound is prefixed with `≥` and an upper bound with `≤`.
///
/// The Scid shows a mate without its sign (`M3`); the sign is a deliberate
/// improvement, since the score is always from White's point of view here.
pub fn format_score(score: Score, bound: Bound) -> String {
    let body = match score {
        Score::Cp(cp) => centipawns_text(cp),
        Score::Mate(0) => "M0".to_owned(),
        Score::Mate(n) => {
            let sign = if n < 0 { '-' } else { '+' };
            format!("{sign}M{}", i64::from(n).abs())
        }
    };
    match bound {
        Bound::Exact => body,
        Bound::Lower => format!("\u{2265}{body}"),
        Bound::Upper => format!("\u{2264}{body}"),
    }
}

/// The score as the comments the analysis adds write it (D441): `+0.35` or
/// `M3` for a mate of White (`-M3` for one of Black).
pub fn comment_score(score: Score) -> String {
    match score {
        Score::Cp(cp) => centipawns_text(cp),
        Score::Mate(n) if n < 0 => format!("-M{}", i64::from(n).abs()),
        Score::Mate(n) => format!("M{}", i64::from(n).abs()),
    }
}

/// The win/draw/loss expectation as whole percentages that add up to 100
/// (`uci.tcl` 345-371): the per mille values are truncated to percent and
/// the largest one takes what is missing.
pub fn wdl_percent(wdl: Wdl) -> (u32, u32, u32) {
    let win = u32::from(wdl.win) / 10;
    let draw = u32::from(wdl.draw) / 10;
    let loss = u32::from(wdl.loss) / 10;
    let largest = win.max(draw).max(loss);
    if win == largest {
        (100u32.saturating_sub(draw + loss), draw, loss)
    } else if draw == largest {
        (win, 100u32.saturating_sub(win + loss), loss)
    } else {
        (win, draw, 100u32.saturating_sub(win + draw))
    }
}

/// ` W: 45% D: 30% B: 25%` (the leading space is part of the text).
pub fn format_wdl(wdl: Wdl) -> String {
    let (w, d, b) = wdl_percent(wdl);
    format!(" W: {w}% D: {d}% B: {b}%")
}

/// The comment of an engine line (D441): `[name] depth:score`, followed by
/// the win/draw/loss expectation when the engine sends one. The name is left
/// out unless [`AnalysisPrefs::name_in_comments`] is on.
pub fn format_comment(
    prefs: &AnalysisPrefs,
    name: &str,
    depth: u32,
    score: Score,
    wdl: Option<Wdl>,
) -> String {
    let mut text = String::new();
    if prefs.name_in_comments {
        text.push('[');
        // A hostile engine can name itself with a megabyte.
        text.extend(name.chars().take(MAX_COMMENT_NAME_CHARS));
        text.push_str("] ");
    }
    let _ = write!(text, "{depth}:{}", comment_score(score));
    if let Some(wdl) = wdl {
        text.push_str(&format_wdl(wdl));
    }
    text
}

/// The longest engine name copied into a comment, in characters.
const MAX_COMMENT_NAME_CHARS: usize = 64;

/// A node count in thousands, dropping the last three digits (the Scid's
/// `calculateNodes`): 999 is 0, 123456 is 123.
pub fn short_nodes(nodes: u64) -> u64 {
    nodes / 1000
}

/// Nodes per second from a node count and a time in milliseconds; `0` when
/// the time is zero (the Scid's `updateAnalysisText`).
pub fn nodes_per_second(nodes: u64, time_ms: u64) -> u64 {
    if time_ms == 0 {
        return 0;
    }
    let nps = u128::from(nodes) * 1000 / u128::from(time_ms);
    u64::try_from(nps).unwrap_or(u64::MAX)
}

/// A percentage from a per mille value, rounded (`hashfull`, `cpuload`).
pub fn permille_percent(permille: u16) -> u16 {
    u16::try_from((u32::from(permille) + 5) / 10).unwrap_or(u16::MAX)
}

/// The moves of `pv` in SAN with move numbers, starting from `position`
/// (`addMoveNumbers`, `analysis.tcl` 3623-3658): `12... e5 13.Nf3 Nc6`.
/// `max_plies` cuts the line (`0`: no limit). The text stops at the first
/// move that is not playable from `position` (a stale line), so it never
/// panics.
pub fn format_pv_limited(
    position: &Position,
    pv: &[Move],
    numbers: NumberStyle,
    max_plies: usize,
) -> String {
    let spc = match numbers {
        NumberStyle::Tight => "",
        NumberStyle::Spaced => " ",
    };
    let take = if max_plies == 0 { pv.len() } else { max_plies };
    let mut current = position.clone();
    let mut number = current.fullmove_number();
    let mut out = String::new();
    let mut black_first = current.turn() == Color::Black;
    for &mv in pv.iter().take(take) {
        let Ok(san) = current.san(mv) else {
            break;
        };
        if !out.is_empty() {
            out.push(' ');
        }
        if current.turn() == Color::White {
            let _ = write!(out, "{number}.{spc}{san}");
        } else if black_first {
            // The Scid writes `12.... e5` (four dots, from `$n.$spc...`);
            // this is the usual `12... e5`.
            let dots = if spc.is_empty() { "..." } else { ". ..." };
            let _ = write!(out, "{number}{dots} {san}");
        } else {
            out.push_str(&san);
        }
        black_first = false;
        if current.turn() == Color::Black {
            number = number.saturating_add(1);
        }
        if current.play(mv).is_err() {
            break;
        }
    }
    out
}

/// [`format_pv_limited`] without a limit.
pub fn format_pv(position: &Position, pv: &[Move], numbers: NumberStyle) -> String {
    format_pv_limited(position, pv, numbers, 0)
}

/// `1234K (987 kn/s)`-style numbers of the extra information line: the node
/// count in thousands and the speed in thousands of nodes per second.
pub fn format_nodes(nodes: u64, time_ms: u64) -> String {
    format!(
        "{}K ({} kn/s)",
        short_nodes(nodes),
        short_nodes(nodes_per_second(nodes, time_ms))
    )
}

/// A time as `1.23` seconds (two decimals), from milliseconds.
pub fn format_seconds(time_ms: u64) -> String {
    format!("{}.{:02}", time_ms / 1000, (time_ms % 1000) / 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fen(text: &str) -> Position {
        Position::from_fen(text).expect("fen")
    }

    #[test]
    fn centipawn_scores_always_carry_their_sign() {
        let exact = |s| format_score(s, Bound::Exact);
        assert_eq!(exact(Score::Cp(35)), "+0.35");
        assert_eq!(exact(Score::Cp(-120)), "-1.20");
        assert_eq!(exact(Score::Cp(0)), "+0.00");
        assert_eq!(exact(Score::Cp(-5)), "-0.05");
        assert_eq!(exact(Score::Cp(32_700)), "+327.00");
        assert_eq!(exact(Score::Cp(i32::MIN)), "-21474836.48");
    }

    #[test]
    fn mates_are_signed_and_bounds_get_a_prefix() {
        assert_eq!(format_score(Score::Mate(3), Bound::Exact), "+M3");
        assert_eq!(format_score(Score::Mate(-3), Bound::Exact), "-M3");
        assert_eq!(format_score(Score::Mate(0), Bound::Exact), "M0");
        assert_eq!(format_score(Score::Cp(35), Bound::Lower), "\u{2265}+0.35");
        assert_eq!(format_score(Score::Cp(-35), Bound::Upper), "\u{2264}-0.35");
        assert_eq!(
            format_score(Score::Mate(i32::MIN), Bound::Exact),
            "-M2147483648"
        );
    }

    #[test]
    fn the_comment_score_keeps_the_scid_shape() {
        assert_eq!(comment_score(Score::Cp(35)), "+0.35");
        assert_eq!(comment_score(Score::Mate(3)), "M3");
        assert_eq!(comment_score(Score::Mate(-3)), "-M3");
    }

    #[test]
    fn wdl_percentages_add_up_to_one_hundred() {
        assert_eq!(
            wdl_percent(Wdl {
                win: 450,
                draw: 300,
                loss: 250
            }),
            (45, 30, 25)
        );
        // 333/333/334 truncate to 33 each: the first of the largest (win)
        // takes what is missing.
        assert_eq!(
            wdl_percent(Wdl {
                win: 333,
                draw: 333,
                loss: 334
            }),
            (34, 33, 33)
        );
        assert_eq!(
            wdl_percent(Wdl {
                win: 1000,
                draw: 0,
                loss: 0
            }),
            (100, 0, 0)
        );
    }

    #[test]
    fn the_comment_has_name_depth_score_and_wdl() {
        let mut prefs = AnalysisPrefs {
            name_in_comments: true,
            ..AnalysisPrefs::default()
        };
        assert_eq!(
            format_comment(&prefs, "Stockfish", 20, Score::Cp(35), None),
            "[Stockfish] 20:+0.35"
        );
        let wdl = Wdl {
            win: 450,
            draw: 300,
            loss: 250,
        };
        assert_eq!(
            format_comment(&prefs, "SF", 18, Score::Mate(3), Some(wdl)),
            "[SF] 18:M3 W: 45% D: 30% B: 25%"
        );
        prefs.name_in_comments = false;
        assert_eq!(
            format_comment(&prefs, "SF", 18, Score::Cp(-100), None),
            "18:-1.00"
        );
    }

    #[test]
    fn node_counts_drop_three_digits() {
        assert_eq!(short_nodes(0), 0);
        assert_eq!(short_nodes(999), 0);
        assert_eq!(short_nodes(1000), 1);
        assert_eq!(short_nodes(123_456), 123);
        assert_eq!(short_nodes(u64::MAX), u64::MAX / 1000);
        assert_eq!(nodes_per_second(5000, 0), 0);
        assert_eq!(nodes_per_second(5000, 500), 10_000);
        assert_eq!(nodes_per_second(u64::MAX, 1), u64::MAX);
        assert_eq!(format_nodes(1_234_000, 1000), "1234K (1234 kn/s)");
        assert_eq!(format_seconds(1234), "1.23");
        assert_eq!(permille_percent(125), 13);
        assert_eq!(permille_percent(1000), 100);
    }

    #[test]
    fn white_starting_numbers_every_white_move() {
        let pos = Position::new();
        let pv: Vec<Move> = {
            let mut p = pos.clone();
            ["e2e4", "e7e5", "g1f3", "b8c6"]
                .iter()
                .map(|u| {
                    let m = p.parse_uci(u).expect("uci");
                    p.play(m).expect("play");
                    m
                })
                .collect()
        };
        assert_eq!(
            format_pv(&pos, &pv, NumberStyle::Tight),
            "1.e4 e5 2.Nf3 Nc6"
        );
        assert_eq!(
            format_pv(&pos, &pv, NumberStyle::Spaced),
            "1. e4 e5 2. Nf3 Nc6"
        );
        assert_eq!(
            format_pv_limited(&pos, &pv, NumberStyle::Tight, 3),
            "1.e4 e5 2.Nf3"
        );
    }

    #[test]
    fn black_starting_writes_the_dots_first() {
        let pos = fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
        let mut p = pos.clone();
        let mut pv = Vec::new();
        for u in ["e7e5", "g1f3", "b8c6"] {
            let m = p.parse_uci(u).expect("uci");
            p.play(m).expect("play");
            pv.push(m);
        }
        assert_eq!(
            format_pv(&pos, &pv, NumberStyle::Tight),
            "1... e5 2.Nf3 Nc6"
        );
        assert_eq!(
            format_pv(&pos, &pv, NumberStyle::Spaced),
            "1. ... e5 2. Nf3 Nc6"
        );
        assert_eq!(format_pv(&pos, &pv[..1], NumberStyle::Tight), "1... e5");
        assert_eq!(format_pv(&pos, &[], NumberStyle::Tight), "");
    }

    #[test]
    fn castling_promotion_and_check_use_san() {
        let pos = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 7");
        let castle = pos.parse_uci("e1g1").expect("castle");
        let after = pos.after(castle).expect("after");
        let queen_side = after.parse_uci("e8c8").expect("castle");
        assert_eq!(
            format_pv(&pos, &[castle, queen_side], NumberStyle::Tight),
            "7.O-O O-O-O"
        );
        let pos = fen("r3k2r/1P6/8/8/8/8/8/R3K2R w KQkq - 0 7");
        let promote = pos.parse_uci("b7a8q").expect("promotion");
        assert_eq!(format_pv(&pos, &[promote], NumberStyle::Tight), "7.bxa8=Q+");
    }

    #[test]
    fn a_stale_pv_stops_at_its_first_unplayable_move() {
        let pos = Position::new();
        let e4 = pos.parse_uci("e2e4").expect("uci");
        // The same move twice: the second one is not playable.
        assert_eq!(format_pv(&pos, &[e4, e4], NumberStyle::Tight), "1.e4");
    }

    #[test]
    fn a_long_pv_is_cut_by_the_limit() {
        // Knights shuffling: 256 plies of legal moves.
        let pos = Position::new();
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        let mut p = pos.clone();
        let mut pv = Vec::new();
        for i in 0..256 {
            let m = p.parse_uci(cycle[i % 4]).expect("uci");
            p.play(m).expect("play");
            pv.push(m);
        }
        let full = format_pv(&pos, &pv, NumberStyle::Tight);
        assert!(full.ends_with("128.Ng1 Ng8"), "{full}");
        let cut = format_pv_limited(&pos, &pv, NumberStyle::Tight, 8);
        assert_eq!(cut, "1.Nf3 Nf6 2.Ng1 Ng8 3.Nf3 Nf6 4.Ng1 Ng8");
    }
}
