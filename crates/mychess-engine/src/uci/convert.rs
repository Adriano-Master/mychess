//! From parsed engine text to domain types: moves validated against the
//! searched position and scores turned to White's point of view.

use mychess_core::{Color, Move, Position};

use crate::protocol::{Bound, MATE_CENTIPAWNS, Score, SearchId, SearchInfo, Wdl};
use crate::uci::parse::{RawBestMove, RawBound, RawInfo, RawScoreKind};

/// Plays `words` from `root` and returns the legal prefix and whether a word
/// had to be dropped (illegal, unreadable or a null move).
pub(crate) fn validate_pv(root: &Position, words: &[String]) -> (Vec<Move>, bool) {
    let mut position = root.clone();
    let mut moves = Vec::with_capacity(words.len());
    for word in words {
        match position.parse_uci(word) {
            Ok(m) if !m.is_null() && position.play(m).is_ok() => moves.push(m),
            _ => return (moves, true),
        }
    }
    (moves, false)
}

/// Converts an `info` line of search `id`, whose engine-side evaluation is
/// from the point of view of the side to move in `root`.
pub(crate) fn to_search_info(raw: RawInfo, id: SearchId, root: &Position) -> SearchInfo {
    let black = root.turn() == Color::Black;
    let score = raw.score.map(|(kind, bound)| {
        let (score, bound) = match kind {
            // `mate 0`: the side to move is already checkmated. White's
            // point of view cannot carry that in `Mate(0)`, so it becomes
            // the extreme centipawn value (the Scid shows 327.50 too).
            RawScoreKind::Mate(0) => (Score::Cp(-MATE_CENTIPAWNS), bound_of(bound)),
            RawScoreKind::Mate(n) => (Score::Mate(n), bound_of(bound)),
            RawScoreKind::Cp(cp) => (Score::Cp(cp), bound_of(bound)),
        };
        if black {
            (score.negated(), bound.flipped())
        } else {
            (score, bound)
        }
    });
    let (pv, pv_truncated) = validate_pv(root, &raw.pv);
    SearchInfo {
        search: id,
        multipv: raw.multipv.unwrap_or(1),
        depth: raw.depth,
        seldepth: raw.seldepth,
        time_ms: raw.time_ms,
        nodes: raw.nodes,
        nps: raw.nps,
        hashfull_permille: raw.hashfull,
        cpuload_permille: raw.cpuload,
        tbhits: raw.tbhits,
        score,
        wdl: raw.wdl.and_then(|(w, d, l)| normalize_wdl(w, d, l, black)),
        pv,
        pv_truncated,
        currmove: raw
            .currmove
            .as_deref()
            .and_then(|m| root.parse_uci(m).ok())
            .filter(|m| !m.is_null()),
        currmovenumber: raw.currmovenumber,
        string: raw.string,
    }
}

fn bound_of(raw: RawBound) -> Bound {
    match raw {
        RawBound::Exact => Bound::Exact,
        RawBound::Lower => Bound::Lower,
        RawBound::Upper => Bound::Upper,
    }
}

/// White's win/draw/loss from the side to move's, adjusted to add up to 1000
/// (the remainder goes to the largest value; an excess scales all down).
fn normalize_wdl(w: u32, d: u32, l: u32, black: bool) -> Option<Wdl> {
    let (win, loss) = if black { (l, w) } else { (w, l) };
    let sum = u64::from(win) + u64::from(d) + u64::from(loss);
    if sum == 0 {
        return None;
    }
    let scale = |v: u32| -> u16 {
        let scaled = if sum > 1000 {
            u64::from(v) * 1000 / sum
        } else {
            u64::from(v)
        };
        u16::try_from(scaled).unwrap_or(1000)
    };
    let mut values = [scale(win), scale(d), scale(loss)];
    let total: u16 = values.iter().sum();
    let remainder = 1000_u16.saturating_sub(total);
    // The first largest wins ties, so the result is deterministic.
    let mut largest = 0;
    for (i, v) in values.iter().enumerate() {
        if *v > values[largest] {
            largest = i;
        }
    }
    values[largest] += remainder;
    Some(Wdl {
        win: values[0],
        draw: values[1],
        loss: values[2],
    })
}

/// A `bestmove` checked against the searched position.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConvertedBestMove {
    pub(crate) mv: Option<Move>,
    pub(crate) ponder: Option<Move>,
    /// Set when the move text was not a legal move.
    pub(crate) warning: Option<String>,
}

/// Validates `bestmove` against `root`. An illegal move becomes "no move"
/// with a warning; an illegal `ponder` move is dropped.
pub(crate) fn to_best_move(raw: &RawBestMove, root: &Position) -> ConvertedBestMove {
    let mut warning = None;
    let mv = raw
        .mv
        .as_deref()
        .and_then(|text| match root.parse_uci(text) {
            Ok(m) if !m.is_null() && root.is_legal(m) => Some(m),
            _ => {
                warning = Some(format!(
                    "illegal bestmove `{}` in {}",
                    crate::uci::parse::truncate_utf8(text, 32),
                    root.fen()
                ));
                None
            }
        });
    let ponder = mv.zip(raw.ponder.as_deref()).and_then(|(m, text)| {
        let after = root.after(m).ok()?;
        after.parse_uci(text).ok().filter(|p| !p.is_null())
    });
    ConvertedBestMove {
        mv,
        ponder,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uci::parse::{UciLine, parse_line};

    fn info(root: &Position, line: &str) -> SearchInfo {
        let UciLine::Info(raw) = parse_line(line) else {
            panic!()
        };
        to_search_info(raw, SearchId(1), root)
    }

    #[test]
    fn score_is_white_point_of_view() {
        let black =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .unwrap();
        let i = info(
            &black,
            "info depth 3 score cp 30 lowerbound wdl 100 300 600 pv e7e5",
        );
        assert_eq!(i.score, Some((Score::Cp(-30), Bound::Upper)));
        assert_eq!(
            i.wdl,
            Some(Wdl {
                win: 600,
                draw: 300,
                loss: 100
            })
        );
        let white = Position::new();
        let i = info(&white, "info score mate 2 pv e2e4");
        assert_eq!(i.score, Some((Score::Mate(2), Bound::Exact)));
        let m = info(&black, "info score mate 2");
        assert_eq!(m.score, Some((Score::Mate(-2), Bound::Exact)));
    }

    #[test]
    fn mate_zero_keeps_its_sign() {
        let white = Position::new();
        assert_eq!(
            info(&white, "info score mate 0").score,
            Some((Score::Cp(-MATE_CENTIPAWNS), Bound::Exact))
        );
    }

    #[test]
    fn pv_is_cut_at_the_first_illegal_move() {
        let i = info(&Position::new(), "info depth 1 pv e2e4 e7e5 e5e3 g1f3");
        assert_eq!(i.pv.len(), 2);
        assert!(i.pv_truncated);
        let ok = info(&Position::new(), "info depth 1 pv e2e4 e7e5");
        assert!(!ok.pv_truncated);
    }

    #[test]
    fn wdl_is_normalized_to_one_thousand() {
        let w = Position::new();
        let i = info(&w, "info wdl 400 400 100");
        assert_eq!(
            i.wdl,
            Some(Wdl {
                win: 500,
                draw: 400,
                loss: 100
            })
        );
        let i = info(&w, "info wdl 1000 1000 1000");
        let wdl = i.wdl.unwrap();
        assert_eq!(wdl.win + wdl.draw + wdl.loss, 1000);
        assert_eq!(info(&w, "info wdl 0 0 0").wdl, None);
    }

    #[test]
    fn bestmove_is_validated() {
        let root = Position::new();
        let ok = to_best_move(
            &RawBestMove {
                missing: false,
                mv: Some("e2e4".into()),
                ponder: Some("e7e5".into()),
            },
            &root,
        );
        assert!(ok.mv.is_some() && ok.ponder.is_some() && ok.warning.is_none());
        let bad = to_best_move(
            &RawBestMove {
                missing: false,
                mv: Some("e2e5".into()),
                ponder: Some("e7e5".into()),
            },
            &root,
        );
        assert!(bad.mv.is_none() && bad.ponder.is_none() && bad.warning.is_some());
        let none = to_best_move(
            &RawBestMove {
                missing: false,
                mv: None,
                ponder: None,
            },
            &root,
        );
        assert!(none.mv.is_none() && none.warning.is_none());
        let bad_ponder = to_best_move(
            &RawBestMove {
                missing: false,
                mv: Some("e2e4".into()),
                ponder: Some("e2e4".into()),
            },
            &root,
        );
        assert!(bad_ponder.mv.is_some() && bad_ponder.ponder.is_none());
    }
}
