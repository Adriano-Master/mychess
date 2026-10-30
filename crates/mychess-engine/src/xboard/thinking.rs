//! What an XBoard engine writes while it searches: thinking lines (`post`),
//! `stat01:`, `move`, `pong`, and their conversion to domain types.
//!
//! Everything here is tolerant: text that does not fit yields `None` or a
//! truncated variation, never a panic (spec m4-2, rules 1-3 and 9).

use mychess_core::{Color, Move, Position, SanMode};

use crate::protocol::{Bound, Score, SearchId, SearchInfo};
use crate::uci::parse::truncate_utf8;

/// A thinking line split into its columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Thinking {
    pub(crate) ply: u32,
    /// Raw score, from the point of view of the side to move.
    pub(crate) score: i32,
    /// Raw time: centiseconds (whole seconds for some engines).
    pub(crate) time: u64,
    pub(crate) nodes: u64,
    pub(crate) pv_text: String,
}

/// The most plies a line may claim; more is garbage.
const MAX_PLY: u32 = 255;

/// The most bytes kept of the variation text of one line: 256 moves with
/// their numbers fit well within it, and the rest is never read.
const MAX_PV_TEXT_BYTES: usize = 4096;

/// The most moves read from one PV, bounding the work done per line.
const MAX_PV_MOVES: usize = 256;

/// Scores from this value up are mate scores (`100_000 + n`, CECP).
const MATE_BASE: i64 = 100_000;

fn take_digits(text: &str) -> Option<(&str, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    (end > 0).then(|| text.split_at(end))
}

/// Skips at least one whitespace character.
fn skip_space(text: &str) -> Option<&str> {
    let rest = text.trim_start();
    (rest.len() < text.len()).then_some(rest)
}

/// Reads `^\s*(\d+)\S?\s+([+-]?\d+)\s+(\d+)\s+(\d+)\s+(.+)$`: ply, one
/// optional glued character (`9.`, `10+`, `11-`, `12?`), score, time, nodes
/// and the variation. Zero or more than 255 plies, numbers out of range and
/// missing fields yield `None` (the Scid wants six scanned fields,
/// `analysis.tcl:3121`). A leading `+` on the score is accepted
/// (`%+5d`).
pub(crate) fn parse_thinking_line(line: &str) -> Option<Thinking> {
    let (ply_text, rest) = take_digits(line.trim_start())?;
    let ply: u32 = ply_text.parse().ok()?;
    if ply == 0 || ply > MAX_PLY {
        return None;
    }
    let mut chars = rest.chars();
    let rest = match chars.next() {
        Some(c) if !c.is_whitespace() => chars.as_str(),
        _ => rest,
    };
    let rest = skip_space(rest)?;
    let (score_text, rest) = {
        let sign_len = usize::from(rest.starts_with(['-', '+']));
        let (digits, _) = take_digits(&rest[sign_len..])?;
        rest.split_at(sign_len + digits.len())
    };
    let score: i32 = score_text
        .strip_prefix('+')
        .unwrap_or(score_text)
        .parse()
        .ok()?;
    let (time_text, rest) = take_digits(skip_space(rest)?)?;
    let time: u64 = time_text.parse().ok()?;
    let (nodes_text, rest) = take_digits(skip_space(rest)?)?;
    let nodes: u64 = nodes_text.parse().ok()?;
    let pv_text = skip_space(rest)?.trim_end();
    if pv_text.is_empty() {
        return None;
    }
    Some(Thinking {
        ply,
        score,
        time,
        nodes,
        pv_text: truncate_utf8(pv_text, MAX_PV_TEXT_BYTES),
    })
}

/// The statistics of `stat01: time nodes ply mvleft mvtot mvname`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stat01 {
    pub(crate) time: u64,
    pub(crate) nodes: u64,
    pub(crate) ply: u32,
}

/// Reads a `stat01:` line (Scid: `analysis.tcl:3155-3168`). A `ply` above
/// 255 is garbage, as in thinking lines.
pub(crate) fn parse_stat01(line: &str) -> Option<Stat01> {
    let rest = line.trim_start().strip_prefix("stat01:")?;
    let mut fields = rest.split_whitespace();
    let stat = Stat01 {
        time: fields.next()?.parse().ok()?,
        nodes: fields.next()?.parse().ok()?,
        ply: fields.next()?.parse().ok()?,
    };
    (stat.ply <= MAX_PLY).then_some(stat)
}

/// Reads `pong N`.
pub(crate) fn parse_pong(line: &str) -> Option<u64> {
    let rest = line.trim().strip_prefix("pong")?;
    rest.strip_prefix(char::is_whitespace)?.trim().parse().ok()
}

/// The move text of `move X`, `My move is: X`, `My move is X` and the old
/// `N. ... X` form (Phalanx and Comet; Scid `analysis.tcl:3038-3043`).
pub(crate) fn parse_move_line(line: &str) -> Option<&str> {
    let line = line.trim();
    let text = if let Some(rest) = line.strip_prefix("move") {
        rest.strip_prefix(char::is_whitespace)?
    } else if let Some(rest) = line.strip_prefix("My move is") {
        rest.trim_start().strip_prefix(':').unwrap_or(rest)
    } else {
        let (_, rest) = take_digits(line)?;
        rest.strip_prefix('.')?.trim_start().strip_prefix("...")?
    };
    let mv = text.split_whitespace().next()?;
    Some(mv)
}

/// Strips the decoration around a PV token: move numbers glued to the move,
/// leading `(` and trailing `+ : ? ! #`. Returns `None` for tokens that are
/// only a move number (`4.`, `4...`, `...`).
fn clean_token(token: &str) -> Option<&str> {
    let mut text = token.trim_start_matches('(');
    if let Some((_, rest)) = take_digits(text)
        && rest.starts_with('.')
    {
        text = rest.trim_start_matches('.');
    } else if text.starts_with("..") {
        text = text.trim_start_matches('.');
    }
    let text = text.trim_end_matches(['+', ':', '?', '!', '#']);
    (!text.is_empty()).then_some(text)
}

/// Removes `{...}` and `(...)` comments from a PV text (nesting-aware).
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0_u32;
    for c in text.chars() {
        match c {
            '{' | '(' => depth = depth.saturating_add(1),
            '}' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Reads one move, SAN (tolerant) first and then coordinates, and plays it.
fn read_move(position: &Position, text: &str) -> Option<Move> {
    let mv = position
        .parse_san(text, SanMode::Tolerant)
        .ok()
        .map(|parsed| parsed.mv)
        .or_else(|| position.parse_uci(text).ok())?;
    (!mv.is_null()).then_some(mv)
}

/// Reads a single move of an engine (`move X`), SAN or coordinates, and
/// checks that it is legal in `position`.
pub(crate) fn parse_engine_move(position: &Position, text: &str) -> Option<Move> {
    let text = clean_token(text)?;
    read_move(position, text).filter(|m| position.is_legal(*m))
}

/// Reads a variation of SAN or coordinate moves, validated one by one from
/// `position`. Returns the legal prefix and whether a token had to be
/// dropped (spec m4-2, rule 2). At most 256 moves are read.
pub(crate) fn parse_pv(position: &Position, text: &str) -> (Vec<Move>, bool) {
    let text = strip_comments(text);
    let mut current = position.clone();
    let mut moves = Vec::new();
    for token in text.split_whitespace() {
        let Some(token) = clean_token(token) else {
            continue;
        };
        if moves.len() >= MAX_PV_MOVES {
            break;
        }
        match read_move(&current, token) {
            Some(mv) if current.play(mv).is_ok() => moves.push(mv),
            _ => return (moves, true),
        }
    }
    (moves, false)
}

/// How the numbers of an engine are to be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Style {
    /// The engine already reports scores from White's point of view.
    pub(crate) scores_white_pov: bool,
    /// The time column is in whole seconds, not centiseconds.
    pub(crate) whole_seconds: bool,
}

/// A score column: `|score| >= 100_000` is a mate in `|score| - 100_000`
/// moves (at least 1); anything else is centipawns. The interpretation of
/// the mate distance is not defined by the Scid (open question 1 of spec
/// m4-2).
pub(crate) fn convert_score(raw: i32) -> Score {
    let magnitude = i64::from(raw).abs();
    if magnitude >= MATE_BASE {
        let n = i32::try_from((magnitude - MATE_BASE).max(1)).unwrap_or(i32::MAX);
        Score::Mate(if raw < 0 { -n } else { n })
    } else {
        Score::Cp(raw)
    }
}

fn time_ms(raw: u64, style: Style) -> u64 {
    raw.saturating_mul(if style.whole_seconds { 1000 } else { 10 })
}

/// Turns a thinking line into a report of search `id` rooted at `root`.
pub(crate) fn to_search_info(
    thinking: &Thinking,
    id: SearchId,
    root: &Position,
    style: Style,
) -> SearchInfo {
    let mut score = convert_score(thinking.score);
    if root.turn() == Color::Black && !style.scores_white_pov {
        score = score.negated();
    }
    let (pv, pv_truncated) = parse_pv(root, &thinking.pv_text);
    SearchInfo {
        search: id,
        multipv: 1,
        depth: Some(thinking.ply),
        time_ms: Some(time_ms(thinking.time, style)),
        nodes: Some(thinking.nodes),
        score: Some((score, Bound::Exact)),
        pv,
        pv_truncated,
        ..SearchInfo::default()
    }
}

/// Turns `stat01:` into a report without a line.
pub(crate) fn stat_to_info(stat: Stat01, id: SearchId, style: Style) -> SearchInfo {
    SearchInfo {
        search: id,
        multipv: 1,
        depth: Some(stat.ply),
        time_ms: Some(time_ms(stat.time, style)),
        nodes: Some(stat.nodes),
        ..SearchInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thinking(line: &str) -> Thinking {
        parse_thinking_line(line).unwrap_or_else(|| panic!("not a thinking line: {line}"))
    }

    #[test]
    fn stat01_with_an_absurd_ply_is_refused() {
        assert!(parse_stat01("stat01: 100 5000 255 1 2 e4").is_some());
        assert_eq!(parse_stat01("stat01: 100 5000 256 1 2 e4"), None);
        assert_eq!(parse_stat01("stat01: 100 5000 4294967295 1 2 e4"), None);
    }

    #[test]
    fn a_huge_variation_text_is_cut_before_it_is_kept() {
        let line = format!("9 10 100 1000 {}", "Nf3 ".repeat(100_000));
        let t = thinking(&line);
        assert!(t.pv_text.len() <= MAX_PV_TEXT_BYTES);
        assert!(t.pv_text.starts_with("Nf3 Nf3"));
    }

    #[test]
    fn plain_thinking_line() {
        let t = thinking("  9  +34  120  345678  Nf3 Nc6 Bb5");
        assert_eq!(
            t,
            Thinking {
                ply: 9,
                score: 34,
                time: 120,
                nodes: 345_678,
                pv_text: "Nf3 Nc6 Bb5".into()
            }
        );
    }

    #[test]
    fn glued_characters_after_ply() {
        for line in [
            "9. 10 5 100 e4",
            "10+ 10 5 100 e4",
            "11- 10 5 100 e4",
            "12? 10 5 100 e4",
        ] {
            assert!(parse_thinking_line(line).is_some(), "{line}");
        }
        assert_eq!(thinking("12? 10 5 100 e4").ply, 12);
    }

    #[test]
    fn signs_and_bounds() {
        assert_eq!(thinking("3 -250 5 100 e4").score, -250);
        assert_eq!(thinking("3 +250 5 100 e4").score, 250);
        assert_eq!(thinking(" 5.  +34 1 2 e4").ply, 5);
    }

    #[test]
    fn rejects_malformed_lines() {
        for line in [
            "",
            "   ",
            "e4 e5",
            "0 10 5 100 e4",
            "256 10 5 100 e4",
            "3 10 5 100",
            "3 10 5 100   ",
            "3 10 5 e4",
            "3 99999999999 5 100 e4",
            "3 10 99999999999999999999999 100 e4",
            "3 10 5 99999999999999999999999 e4",
            "3.. 10 5 100 e4",
            "3 - 5 100 e4",
            "1. ... e5",
        ] {
            assert_eq!(parse_thinking_line(line), None, "{line}");
        }
    }

    #[test]
    fn stat01_and_pong() {
        assert_eq!(
            parse_stat01("stat01: 120 4500 7 20 35 e2e4"),
            Some(Stat01 {
                time: 120,
                nodes: 4500,
                ply: 7
            })
        );
        assert_eq!(parse_stat01("stat01: 1 2"), None);
        assert_eq!(parse_stat01("stat1: 1 2 3"), None);
        assert_eq!(parse_pong("pong 12"), Some(12));
        assert_eq!(parse_pong("pong"), None);
        assert_eq!(parse_pong("pong x"), None);
        assert_eq!(parse_pong("pongo 1"), None);
    }

    #[test]
    fn move_lines() {
        assert_eq!(parse_move_line("move e2e4"), Some("e2e4"));
        assert_eq!(parse_move_line("My move is: Nf3"), Some("Nf3"));
        assert_eq!(parse_move_line("My move is e7e5"), Some("e7e5"));
        assert_eq!(parse_move_line("1. ... e7e5"), Some("e7e5"));
        assert_eq!(parse_move_line("12. ... Nc6"), Some("Nc6"));
        assert_eq!(parse_move_line("move"), None);
        assert_eq!(parse_move_line("moves e2e4"), None);
        assert_eq!(parse_move_line("1-0 {White mates}"), None);
        assert_eq!(parse_move_line("resign"), None);
    }

    #[test]
    fn pv_san_with_numbers_and_decoration() {
        let start = Position::new();
        let (moves, cut) = parse_pv(&start, "1. e4 e5 2. Nf3 Nc6 3. Bb5+");
        assert_eq!((moves.len(), cut), (5, false));
        let (moves, cut) = parse_pv(&start, "1.e4 e5 2.Nf3 {good} (a comment) Nc6 3...a6");
        assert_eq!(moves.len(), 4, "{moves:?}");
        assert!(cut, "3...a6 is illegal for White");
    }

    #[test]
    fn pv_black_numbers_and_ellipsis() {
        let black =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .unwrap();
        let (moves, cut) = parse_pv(&black, "1... e5 2. Nf3 ... Nc6");
        assert_eq!((moves.len(), cut), (3, false));
    }

    #[test]
    fn pv_coordinates_castling_and_mixed() {
        let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
        let (moves, cut) = parse_pv(&pos, "O-O O-O-O e1g1");
        // The third move is illegal: the king already castled.
        assert_eq!((moves.len(), cut), (2, true));
        let (moves, cut) = parse_pv(&Position::new(), "e2e4 e7e5 Ng1f3");
        assert_eq!((moves.len(), cut), (3, false));
    }

    #[test]
    fn pv_is_truncated_at_the_first_bad_token() {
        let start = Position::new();
        assert_eq!(
            parse_pv(&start, "e4 e5 Qxh7 Nc6"),
            (parse_pv(&start, "e4 e5").0, true)
        );
        assert_eq!(parse_pv(&start, "e4 zzz"), (parse_pv(&start, "e4").0, true));
        assert_eq!(parse_pv(&start, ""), (Vec::new(), false));
        assert_eq!(parse_pv(&start, "4. ... 1-0"), (Vec::new(), true));
        // A null move is not a move of a variation.
        assert_eq!(parse_pv(&start, "--"), (Vec::new(), true));
    }

    #[test]
    fn pv_is_bounded() {
        let start = Position::new();
        let text = "Nf3 Nf6 Ng1 Ng8 ".repeat(500);
        let (moves, cut) = parse_pv(&start, &text);
        assert_eq!(moves.len(), MAX_PV_MOVES);
        assert!(!cut);
    }

    #[test]
    fn engine_move_must_be_legal() {
        let start = Position::new();
        assert!(parse_engine_move(&start, "e2e4").is_some());
        assert!(parse_engine_move(&start, "Nf3").is_some());
        assert!(parse_engine_move(&start, "e7e5").is_none());
        assert!(parse_engine_move(&start, "???").is_none());
    }

    #[test]
    fn scores_and_mates() {
        assert_eq!(convert_score(34), Score::Cp(34));
        assert_eq!(convert_score(-99_999), Score::Cp(-99_999));
        assert_eq!(convert_score(100_003), Score::Mate(3));
        assert_eq!(convert_score(-100_003), Score::Mate(-3));
        assert_eq!(convert_score(100_000), Score::Mate(1));
        assert_eq!(convert_score(i32::MIN), Score::Mate(-2_147_383_648));
    }

    #[test]
    fn info_is_white_point_of_view_and_units_follow_the_style() {
        let black =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .unwrap();
        let t = thinking("6 30 120 5000 e5 Nf3");
        let info = to_search_info(&t, SearchId(3), &black, Style::default());
        assert_eq!(info.score, Some((Score::Cp(-30), Bound::Exact)));
        assert_eq!(info.time_ms, Some(1200));
        assert_eq!(
            (info.depth, info.nodes, info.multipv),
            (Some(6), Some(5000), 1)
        );
        assert_eq!(info.pv.len(), 2);
        let white_pov = Style {
            scores_white_pov: true,
            whole_seconds: true,
        };
        let info = to_search_info(&t, SearchId(3), &black, white_pov);
        assert_eq!(info.score, Some((Score::Cp(30), Bound::Exact)));
        assert_eq!(info.time_ms, Some(120_000));
        let mate = thinking("6 100002 1 1 e5");
        let info = to_search_info(&mate, SearchId(1), &black, Style::default());
        assert_eq!(info.score, Some((Score::Mate(-2), Bound::Exact)));
    }

    #[test]
    fn stat_info_has_no_line() {
        let info = stat_to_info(
            Stat01 {
                time: 7,
                nodes: 9,
                ply: 4,
            },
            SearchId(2),
            Style::default(),
        );
        assert!(info.pv.is_empty() && info.score.is_none());
        assert_eq!(
            (info.time_ms, info.nodes, info.depth),
            (Some(70), Some(9), Some(4))
        );
    }

    proptest::proptest! {
        #[test]
        fn parsers_never_panic(text in "\\PC{0,200}") {
            drop(parse_thinking_line(&text));
            let _ = parse_stat01(&text);
            let _ = parse_pong(&text);
            let _ = parse_move_line(&text);
            drop(parse_pv(&Position::new(), &text));
            drop(crate::xboard::features::parse_feature_line(&text));
            let _ = crate::xboard::quirks::detect([text.as_str()]);
        }

        #[test]
        fn shaped_lines_never_panic(
            ply in 0_u64..400,
            glue in "\\PC{0,2}",
            score in -300_000_i64..300_000,
            time in 0_u64..u64::MAX,
            nodes in 0_u64..u64::MAX,
            pv in "[a-hA-HxNBRQKO0-9=+#!?:.() {}-]{0,60}",
        ) {
            let line = format!("{ply}{glue} {score} {time} {nodes} {pv}");
            if let Some(t) = parse_thinking_line(&line) {
                drop(to_search_info(&t, SearchId(1), &Position::new(), Style::default()));
            }
        }
    }
}
