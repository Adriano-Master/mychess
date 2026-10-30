//! The arrows of the engine on the main board (spec M4-5, D456; Scid
//! `analysis.tcl` `updateAnalysisBoard`, `boardVisualizeMoves`).

use mychess_core::{Color, Square};

use super::prefs::{ArrowMode, ArrowPrefs, SequenceSide};
use crate::analysis::AnalysisView;
use crate::report::{ReportData, ReviewStep};
use crate::state::AppState;

/// What an arrow stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowKind {
    /// The first move of line 1 (drawn in the highlight colour of the
    /// engine).
    Best,
    /// The first move of another MultiPV line (grey).
    Other,
    /// A move of the main line in the sequence mode (thin); the value is the
    /// ply counted from the position (0 is the move to play now).
    Sequence(u8),
    /// The best move of the engine at a moment of the guided review (spec
    /// M4-11, D1664): green, with an outline, drawn by the review and not by
    /// a running engine.
    ReviewBest,
}

/// An arrow to draw on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineArrow {
    /// Where the piece leaves (the king's square for castling).
    pub from: Square,
    /// Where it lands (the king's square for castling).
    pub to: Square,
    /// The MultiPV line the move belongs to, from 1.
    pub rank: u8,
    /// What it stands for.
    pub kind: ArrowKind,
    /// The side that makes the move (colours the sequence arrows).
    pub side: Color,
    /// The number written on the arrow, if any.
    pub label: Option<u8>,
}

/// The arrows of the lines of `view` for the position they are about, or
/// nothing when the position is unknown. Moves that are not on the board
/// (the null move) are left out. In the sequence mode the submode filters by
/// the **absolute** colour (White, Black or both), unlike the Scid, which
/// filters by the parity of the ply relative to the side to move (D828); a
/// null move is skipped without taking a number (the Scid counts it, D828).
pub fn engine_arrows(view: &AnalysisView, prefs: ArrowPrefs) -> Vec<EngineArrow> {
    let Some(position) = view.position() else {
        return Vec::new();
    };
    let turn = position.turn();
    match prefs.mode {
        ArrowMode::Arrows => {
            let mut lines: Vec<_> = view
                .lines
                .iter()
                .filter_map(|l| l.pv.first().map(|mv| (l.multipv, *mv)))
                .collect();
            lines.sort_by_key(|(rank, _)| *rank);
            // The Scid numbers the lines only when there are more than two
            // (`movecount != 2`).
            let numbered = prefs.number_lines && lines.len() > 2;
            let mut out: Vec<EngineArrow> = Vec::with_capacity(lines.len());
            for (rank, mv) in lines {
                let (Some(from), Some(to)) = (mv.from(), mv.to()) else {
                    continue;
                };
                if out.iter().any(|a| a.from == from && a.to == to) {
                    continue;
                }
                out.push(EngineArrow {
                    from,
                    to,
                    rank,
                    kind: if rank == 1 {
                        ArrowKind::Best
                    } else {
                        ArrowKind::Other
                    },
                    side: turn,
                    label: (numbered && rank > 1).then_some(rank),
                });
            }
            out
        }
        ArrowMode::Sequence => {
            let Some(line) = view
                .lines
                .iter()
                .find(|l| l.multipv == 1)
                .or(view.lines.first())
            else {
                return Vec::new();
            };
            let mut out = Vec::new();
            let mut number: u8 = 0;
            // Every move of the continuation is played on a copy: the
            // sequence stops at the first one that is not legal (D827).
            let mut scratch = position.clone();
            for (ply, mv) in line.pv.iter().enumerate() {
                if scratch.play(*mv).is_err() {
                    break;
                }
                let side = if ply % 2 == 0 { turn } else { !turn };
                let wanted = match prefs.sequence_side {
                    SequenceSide::Both => true,
                    SequenceSide::White => side == Color::White,
                    SequenceSide::Black => side == Color::Black,
                };
                let (Some(from), Some(to)) = (mv.from(), mv.to()) else {
                    continue;
                };
                if !wanted {
                    continue;
                }
                number = number.saturating_add(1);
                if number > prefs.sequence_len {
                    break;
                }
                out.push(EngineArrow {
                    from,
                    to,
                    rank: line.multipv,
                    kind: ArrowKind::Sequence(u8::try_from(ply).unwrap_or(u8::MAX)),
                    side,
                    label: Some(number),
                });
            }
            out
        }
    }
}

/// The arrows of the guided review for the position on the board (spec
/// M4-11, D1665, D1667): the best move of the moment on show, when the
/// cursor is on the position before it and nothing is walking.
///
/// - the verdict (looking at the position) shows it only with the
///   `report.review_arrow` preference on;
/// - "try again" hides it (it would give the answer away);
/// - after the attempt was right, or the best move was asked for, it shows
///   whatever the preference says;
/// - a report without a best move for the moment draws nothing.
pub fn review_arrows(state: &AppState) -> Vec<EngineArrow> {
    let Some(review) = state.report.review.as_ref() else {
        return Vec::new();
    };
    if state.report.walk.is_some() {
        return Vec::new();
    }
    let shown = match review.step {
        ReviewStep::Trying => false,
        ReviewStep::Looking => state.prefs.report.review_arrow,
        ReviewStep::Solved | ReviewStep::Revealed => true,
    };
    if !shown {
        return Vec::new();
    }
    let (Some(moment), Some(data)) = (review.current(), state.report_data()) else {
        return Vec::new();
    };
    let game = state.session.game();
    if ReportData::node_before(game, moment.ply) != Some(state.session.cursor()) {
        return Vec::new();
    }
    let Some(judged) = moment
        .ply
        .checked_sub(1)
        .and_then(|i| data.report.moves.get(i))
    else {
        return Vec::new();
    };
    let Some(best) = judged.best else {
        return Vec::new();
    };
    let (Some(from), Some(to)) = (best.from(), best.to()) else {
        return Vec::new();
    };
    vec![EngineArrow {
        from,
        to,
        rank: 1,
        kind: ArrowKind::ReviewBest,
        side: judged.mover,
        label: None,
    }]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mychess_core::Position;
    use mychess_engine::{Bound, Score, SearchId, SearchInfo};

    use super::*;
    use crate::analysis::{AnalysisView, FormatOptions};
    use crate::engines::{AnalysisTarget, RunId};
    use crate::prefs::AnalysisPrefs;

    /// A view of the start position with these lines (UCI, played from it).
    fn view(lines: &[&[&str]]) -> AnalysisView {
        let mut v = AnalysisView::new(RunId(1), 5, &AnalysisPrefs::default());
        let target = Arc::new(AnalysisTarget {
            start: Position::new(),
            moves: Vec::new(),
        });
        v.begin_search(SearchId(1), Some(target));
        for (i, ucis) in lines.iter().enumerate() {
            let mut pos = Position::new();
            let pv = ucis
                .iter()
                .map(|u| {
                    let m = pos.parse_uci(u).expect("uci");
                    pos.play(m).expect("play");
                    m
                })
                .collect();
            v.apply_info(
                SearchInfo {
                    search: SearchId(1),
                    multipv: u8::try_from(i + 1).expect("small"),
                    depth: Some(8),
                    score: Some((Score::Cp(10), Bound::Exact)),
                    pv,
                    ..SearchInfo::default()
                },
                FormatOptions::default(),
            );
        }
        v
    }

    fn squares(arrows: &[EngineArrow]) -> Vec<(String, String)> {
        arrows
            .iter()
            .map(|a| (a.from.to_string(), a.to.to_string()))
            .collect()
    }

    #[test]
    fn one_line_gives_one_best_arrow_without_number() {
        let v = view(&[&["e2e4", "e7e5"]]);
        let a = engine_arrows(&v, ArrowPrefs::default());
        assert_eq!(squares(&a), [("e2".into(), "e4".into())]);
        assert_eq!(
            (a[0].kind, a[0].label, a[0].side),
            (ArrowKind::Best, None, Color::White)
        );
    }

    #[test]
    fn two_lines_are_not_numbered_three_are() {
        let two = view(&[&["e2e4"], &["d2d4"]]);
        let a = engine_arrows(&two, ArrowPrefs::default());
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|x| x.label.is_none()));
        assert_eq!(a[1].kind, ArrowKind::Other);
        let five = view(&[&["e2e4"], &["d2d4"], &["g1f3"], &["c2c4"], &["b1c3"]]);
        let a = engine_arrows(&five, ArrowPrefs::default());
        let labels: Vec<_> = a.iter().map(|x| x.label).collect();
        assert_eq!(labels, [None, Some(2), Some(3), Some(4), Some(5)]);
        let off = ArrowPrefs {
            number_lines: false,
            ..ArrowPrefs::default()
        };
        assert!(engine_arrows(&five, off).iter().all(|x| x.label.is_none()));
    }

    #[test]
    fn duplicate_first_moves_collapse_into_one_arrow() {
        let mut v = view(&[&["e2e4"]]);
        let dup = v.lines[0].clone();
        v.lines.push(crate::analysis::PvLine { multipv: 2, ..dup });
        assert_eq!(engine_arrows(&v, ArrowPrefs::default()).len(), 1);
    }

    #[test]
    fn an_empty_pv_or_no_position_gives_nothing() {
        let mut v = view(&[&["e2e4"], &["d2d4"], &["g1f3"]]);
        v.lines[1].pv.clear();
        let a = engine_arrows(&v, ArrowPrefs::default());
        assert_eq!(a.iter().map(|x| x.rank).collect::<Vec<_>>(), [1, 3]);
        // 2 lines with moves: no numbers (the Scid counts the moves).
        assert!(a.iter().all(|x| x.label.is_none()));
        let fresh = AnalysisView::new(RunId(1), 1, &AnalysisPrefs::default());
        assert!(engine_arrows(&fresh, ArrowPrefs::default()).is_empty());
    }

    #[test]
    fn sequence_numbers_the_first_moves_and_colours_by_side() {
        let v = view(&[&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]]);
        let prefs = ArrowPrefs {
            mode: ArrowMode::Sequence,
            ..ArrowPrefs::default()
        };
        let a = engine_arrows(&v, prefs);
        assert_eq!(a.len(), 3);
        assert_eq!(
            a.iter().map(|x| x.label).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3)]
        );
        assert_eq!(a[1].side, Color::Black);
        assert_eq!(a[2].kind, ArrowKind::Sequence(2));
    }

    #[test]
    fn sequence_submodes_filter_by_side_and_count_only_shown_moves() {
        let v = view(&[&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]]);
        let mut prefs = ArrowPrefs {
            mode: ArrowMode::Sequence,
            sequence_len: 2,
            sequence_side: SequenceSide::White,
            ..ArrowPrefs::default()
        };
        let a = engine_arrows(&v, prefs);
        assert_eq!(
            squares(&a),
            [("e2".into(), "e4".into()), ("g1".into(), "f3".into())]
        );
        prefs.sequence_side = SequenceSide::Black;
        let a = engine_arrows(&v, prefs);
        assert_eq!(
            squares(&a),
            [("e7".into(), "e5".into()), ("b8".into(), "c6".into())]
        );
        assert!(a.iter().all(|x| x.side == Color::Black));
        prefs.sequence_len = 8;
        prefs.sequence_side = SequenceSide::Both;
        assert_eq!(engine_arrows(&v, prefs).len(), 5);
    }

    #[test]
    fn sequence_submode_is_an_absolute_colour_with_black_to_move() {
        // Decision D828 (deviation from the Scid): "White" means the white
        // pieces whoever is to move.
        let first = Position::new().parse_uci("e2e4").expect("uci");
        // A view about the position after 1.e4, Black to move.
        let target = Arc::new(AnalysisTarget {
            start: Position::new(),
            moves: vec![first],
        });
        let mut b = AnalysisView::new(RunId(1), 5, &AnalysisPrefs::default());
        b.begin_search(SearchId(2), Some(target));
        let mut p1 = Position::new();
        p1.play(first).expect("play");
        let mut p2 = p1.clone();
        let ucis = ["e7e5", "g1f3", "b8c6"];
        let moves: Vec<_> = ucis
            .iter()
            .map(|u| {
                let m = p2.parse_uci(u).expect("uci");
                p2.play(m).expect("play");
                m
            })
            .collect();
        b.apply_info(
            SearchInfo {
                search: SearchId(2),
                multipv: 1,
                depth: Some(8),
                score: Some((Score::Cp(10), Bound::Exact)),
                pv: moves,
                ..SearchInfo::default()
            },
            FormatOptions::default(),
        );
        let mut prefs = ArrowPrefs {
            mode: ArrowMode::Sequence,
            sequence_len: 8,
            sequence_side: SequenceSide::White,
            ..ArrowPrefs::default()
        };
        let a = engine_arrows(&b, prefs);
        assert_eq!(squares(&a), [("g1".into(), "f3".into())]);
        assert_eq!(a[0].label, Some(1));
        prefs.sequence_side = SequenceSide::Black;
        let a = engine_arrows(&b, prefs);
        assert_eq!(
            squares(&a),
            [("e7".into(), "e5".into()), ("b8".into(), "c6".into())]
        );
        assert!(a.iter().all(|x| x.side == Color::Black));
    }

    #[test]
    fn sequence_stops_at_the_first_illegal_move() {
        let mut v = view(&[&["e2e4", "e7e5", "g1f3"]]);
        // Replace the third move by one that is not legal there.
        let bogus = Position::new().parse_uci("e2e4").expect("uci");
        v.lines[0].pv[2] = bogus;
        let prefs = ArrowPrefs {
            mode: ArrowMode::Sequence,
            sequence_len: 8,
            ..ArrowPrefs::default()
        };
        assert_eq!(engine_arrows(&v, prefs).len(), 2);
    }
}
