//! The operations that "Paste analysis" and the analysis of the positions
//! write into a record (spec M7-5, regra 9, D3809; Scid `pasteAnalysis`,
//! `epd.tcl` 714-761).

use mychess_core::{Color, Move, Position};
use mychess_engine::Score;
use mychess_pgn::epd::{EpdOperation, EpdRecord};

/// The opcodes the analysis writes, and removes before writing.
pub const ANALYSIS_OPCODES: [&str; 5] = ["acd", "acn", "ce", "dm", "pv"];

/// What one search tells about a position.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineLine {
    /// The score from White's point of view.
    pub score: Score,
    /// The depth reached.
    pub depth: u32,
    /// The nodes searched.
    pub nodes: u64,
    /// The best variation, legal from the position.
    pub pv: Vec<Move>,
}

/// `ce` and `dm` for `score` with `turn` to move (rule 9).
///
/// `ce` is in centipawns from the point of view of the side to move. A mate
/// is `dm` full moves (negative when the side to move is mated) and `ce =
/// ±(32767 - (2*|dm| - 1))` with the sign of `dm`; the Scid loses the sign of
/// the mate (`epd.tcl` 747-751), the MyChess keeps it.
pub fn ce_and_dm(score: Score, turn: Color) -> (i32, i32) {
    match score {
        Score::Cp(cp) => (
            if turn == Color::White {
                cp
            } else {
                cp.saturating_neg()
            },
            0,
        ),
        Score::Mate(n) => {
            let dm = if turn == Color::White {
                n
            } else {
                n.saturating_neg()
            };
            if dm == 0 {
                // The side to move is mated already.
                return (-32_767, 0);
            }
            let distance = dm.unsigned_abs().min(16_383);
            let magnitude = 32_767_i32 - (2 * i32::try_from(distance).unwrap_or(0) - 1);
            (if dm < 0 { -magnitude } else { magnitude }, dm)
        }
    }
}

/// The variation in SAN without move numbers, cut at the first move that is
/// not legal (it cannot happen with a validated line).
pub fn pv_text(position: &Position, pv: &[Move]) -> String {
    let mut pos = position.clone();
    let mut out: Vec<String> = Vec::with_capacity(pv.len());
    for &mv in pv {
        let Ok(san) = pos.san(mv) else { break };
        out.push(san);
        match pos.after(mv) {
            Ok(next) => pos = next,
            Err(_) => break,
        }
    }
    out.join(" ")
}

/// The operations `acd acn ce dm pv`, in that order (`pv` only when the line
/// has moves).
pub fn analysis_operations(position: &Position, line: &EngineLine) -> Vec<EpdOperation> {
    let (ce, dm) = ce_and_dm(line.score, position.turn());
    let mut ops = vec![
        EpdOperation::new("acd", line.depth.to_string()),
        EpdOperation::new("acn", line.nodes.to_string()),
        EpdOperation::new("ce", ce.to_string()),
        EpdOperation::new("dm", dm.to_string()),
    ];
    let pv = pv_text(position, &line.pv);
    if !pv.is_empty() {
        ops.push(EpdOperation::new("pv", pv));
    }
    ops
}

/// Removes the old analysis operations of `record` and appends `ops` (and
/// nothing else changes). Returns whether the record differs afterwards.
pub(crate) fn put_analysis(record: &mut EpdRecord, ops: Vec<EpdOperation>) -> bool {
    let before = record.operations().to_vec();
    for opcode in ANALYSIS_OPCODES {
        record.remove_opcode(opcode);
    }
    record.operations_mut().extend(ops);
    record.operations() != before.as_slice()
}

/// Replaces (or appends) the operation `op` in `record`, removing every
/// operation of the same opcode (any case) first.
pub(crate) fn put_operation(record: &mut EpdRecord, op: EpdOperation) -> bool {
    let before = record.operations().to_vec();
    record.remove_opcode(&op.opcode);
    record.operations_mut().push(op);
    record.operations() != before.as_slice()
}

/// The name of an engine as an opcode: `[A-Za-z0-9_]`, starting with a
/// letter, at most 14 characters; otherwise `engine` (rule 20).
pub fn engine_opcode(name: &str) -> String {
    let mapped: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mapped = mapped.trim_matches('_');
    let mut chars = mapped.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    if !starts_with_letter {
        return "engine".to_owned();
    }
    mapped.chars().take(14).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(sans: &[&str]) -> Position {
        let mut p = Position::new();
        for s in sans {
            let m = p
                .parse_san(s, mychess_core::SanMode::Tolerant)
                .expect("legal")
                .mv;
            p = p.after(m).expect("after");
        }
        p
    }

    #[test]
    fn centipawns_follow_the_side_to_move() {
        assert_eq!(ce_and_dm(Score::Cp(25), Color::White), (25, 0));
        assert_eq!(ce_and_dm(Score::Cp(25), Color::Black), (-25, 0));
    }

    #[test]
    fn mate_for_the_mover_and_against_keep_their_sign() {
        assert_eq!(ce_and_dm(Score::Mate(3), Color::White), (32_762, 3));
        assert_eq!(ce_and_dm(Score::Mate(-3), Color::Black), (32_762, 3));
        assert_eq!(ce_and_dm(Score::Mate(-2), Color::White), (-32_764, -2));
        assert_eq!(ce_and_dm(Score::Mate(2), Color::Black), (-32_764, -2));
        assert_eq!(ce_and_dm(Score::Mate(1), Color::White), (32_766, 1));
    }

    #[test]
    fn huge_mate_distances_do_not_overflow() {
        let (ce, dm) = ce_and_dm(Score::Mate(i32::MAX), Color::White);
        assert_eq!(dm, i32::MAX);
        assert!(ce > 0);
        let (ce, _) = ce_and_dm(Score::Mate(i32::MIN), Color::White);
        assert!(ce < 0);
    }

    #[test]
    fn the_line_becomes_five_operations() {
        let line = EngineLine {
            score: Score::Cp(25),
            depth: 20,
            nodes: 1_234_567,
            pv: vec![
                Position::new()
                    .parse_san("e4", mychess_core::SanMode::Tolerant)
                    .expect("e4")
                    .mv,
            ],
        };
        let ops = analysis_operations(&Position::new(), &line);
        let text: Vec<String> = ops
            .iter()
            .map(|o| format!("{} {}", o.opcode, o.operands))
            .collect();
        assert_eq!(text, ["acd 20", "acn 1234567", "ce 25", "dm 0", "pv e4"]);
    }

    #[test]
    fn the_variation_is_san_without_numbers() {
        let start = Position::new();
        let e4 = start
            .parse_san("e4", mychess_core::SanMode::Tolerant)
            .expect("e4")
            .mv;
        let p1 = start.after(e4).expect("after");
        let e5 = p1
            .parse_san("e5", mychess_core::SanMode::Tolerant)
            .expect("e5")
            .mv;
        assert_eq!(pv_text(&start, &[e4, e5]), "e4 e5");
        // Black to move: the same line for the position after 1.e4.
        assert_eq!(pv_text(&after(&["e4"]), &[e5]), "e5");
    }

    #[test]
    fn old_analysis_is_removed_and_the_rest_stays() {
        let mut rec = EpdRecord::with_operations(
            Position::new(),
            vec![
                EpdOperation::new("id", "\"x\""),
                EpdOperation::new("ce", "99"),
                EpdOperation::new("pv", "d4"),
                EpdOperation::new("bm", "e4"),
            ],
        );
        let changed = put_analysis(&mut rec, vec![EpdOperation::new("ce", "25")]);
        assert!(changed);
        let codes: Vec<&str> = rec.operations().iter().map(|o| o.opcode.as_str()).collect();
        assert_eq!(codes, ["id", "bm", "ce"]);
    }

    #[test]
    fn writing_the_same_analysis_twice_changes_nothing_the_second_time() {
        let mut rec = EpdRecord::new(Position::new());
        let ops = vec![EpdOperation::new("ce", "25")];
        assert!(put_analysis(&mut rec, ops.clone()));
        assert!(!put_analysis(&mut rec, ops));
    }

    #[test]
    fn engine_names_become_opcodes() {
        assert_eq!(engine_opcode("Stockfish 17"), "Stockfish_17");
        assert_eq!(engine_opcode("Lc0"), "Lc0");
        assert_eq!(engine_opcode("1stEngine"), "engine");
        assert_eq!(engine_opcode("  "), "engine");
        assert_eq!(engine_opcode("A very long engine name"), "A_very_long_en");
        assert_eq!(engine_opcode("é"), "engine");
    }
}
