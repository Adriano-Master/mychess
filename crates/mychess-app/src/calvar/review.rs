//! What an exercise keeps in the history so that it can be reviewed later
//! without an engine: the lines typed, the evaluations, the analysis of the
//! engine and the parts of the score (spec M8-6d, D2367-D2372).
//!
//! The record holds text and small integers only (moves in UCI, fractions in
//! thousandths), so it stays `Eq` and round-trips through TOML exactly. The
//! SAN of the lines and of the good moves is made again from the start
//! position when the record is turned back into a [`Report`]. Reading is
//! tolerant: a record that no longer fits its position is dropped, the
//! entry stays in the history without a review.

use mychess_core::{Move, Position};
use toml::{Table, Value};

use super::compare::{GoodMove, LineReport, Report, ScoreParts};
use super::eval::Eval;
use super::line::{Assessment, MAX_LINE_PLIES, MAX_LINES, san_line};

/// Plies of the engine line kept for each good move.
pub const MAX_RECORD_PV: usize = 12;
/// Good moves kept.
pub const MAX_RECORD_GOOD: usize = 16;
/// Longest text of a list of moves read from the file.
const MAX_MOVES_TEXT: usize = 1_000;

/// A good move of the start position, as saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoodRecord {
    /// The move, in UCI.
    pub mv: String,
    /// Its evaluation, White's point of view.
    pub eval: Eval,
    /// The depth reached.
    pub depth: u32,
    /// The line of the engine, in UCI, starting with the move.
    pub pv: Vec<String>,
    /// The user listed it.
    pub found: bool,
    /// It was missed and was crucial.
    pub crucial: bool,
}

/// A line of the user, as saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRecord {
    /// The moves, in UCI.
    pub moves: Vec<String>,
    /// The evaluation of the user.
    pub assessment: Assessment,
    /// The engine's evaluation after the first move.
    pub first: Option<Eval>,
    /// The engine's evaluation at the end of the line.
    pub end: Option<Eval>,
    /// The class of the end according to the engine.
    pub engine: Option<Assessment>,
    /// The first move is a good move.
    pub good_candidate: bool,
    /// Plies that follow the engine's play.
    pub agree_plies: u8,
    /// Accuracy, in thousandths.
    pub accuracy: u16,
    /// The line did not drop the evaluation.
    pub sound: bool,
    /// Credit for the depth, in thousandths.
    pub depth_credit: u16,
    /// Quality, in thousandths.
    pub quality: u16,
}

/// The whole result of an exercise, as saved (D2367).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRecord {
    /// The good moves, best first.
    pub good: Vec<GoodRecord>,
    /// The lines of the user, in the order given.
    pub lines: Vec<LineRecord>,
    /// Candidates part of the score, in thousandths.
    pub candidates: u16,
    /// Accuracy part, in thousandths.
    pub accuracy: u16,
    /// Quality part, in thousandths.
    pub quality: u16,
    /// Points taken away, in tenths of a point.
    pub penalty: u16,
}

fn thousandths(x: f32) -> u16 {
    let scaled = (x.clamp(0.0, 1.0) * 1000.0).round();
    // `scaled` is a whole number from 0 to 1000 (NaN clamps to 0 below).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = if scaled.is_nan() { 0 } else { scaled as u16 };
    n
}

fn fraction(thousandths: u16) -> f32 {
    f32::from(thousandths.min(1000)) / 1000.0
}

impl ReviewRecord {
    /// Keeps what `report` says about the exercise that started at `root`.
    pub fn from_report(root: &Position, report: &Report) -> ReviewRecord {
        let good = report
            .good
            .iter()
            .take(MAX_RECORD_GOOD)
            .map(|g| GoodRecord {
                mv: root.uci(g.mv),
                eval: g.eval,
                depth: g.depth,
                pv: uci_line(root, &g.pv, MAX_RECORD_PV),
                found: g.found,
                crucial: g.crucial,
            })
            .collect();
        let lines = report
            .lines
            .iter()
            .take(MAX_LINES)
            .map(|l| LineRecord {
                moves: uci_line(root, &l.moves, MAX_LINE_PLIES),
                assessment: l.assessment,
                first: l.first,
                end: l.end,
                engine: l.engine_assessment,
                good_candidate: l.good_candidate,
                agree_plies: u8::try_from(l.agree_plies).unwrap_or(u8::MAX),
                accuracy: thousandths(l.accuracy),
                sound: l.sound,
                depth_credit: thousandths(l.depth_credit),
                quality: thousandths(l.quality),
            })
            .collect();
        ReviewRecord {
            good,
            lines,
            candidates: thousandths(report.parts.candidates),
            accuracy: thousandths(report.parts.accuracy),
            quality: thousandths(report.parts.quality),
            penalty: thousandths(report.parts.penalty / 100.0),
        }
    }

    /// Turns the record back into a report of the exercise that started at
    /// `root`. `None` when a move does not fit the position (the file was
    /// edited, or belongs to another position).
    pub fn to_report(&self, root: &Position, score: u8) -> Option<Report> {
        let mut good = Vec::with_capacity(self.good.len());
        for g in &self.good {
            let mv = root.parse_uci(&g.mv).ok()?;
            let mover = root.turn();
            good.push(GoodMove {
                mv,
                san: root.san(mv).ok()?,
                eval: g.eval,
                mover_cp: g.eval.for_mover(mover),
                depth: g.depth,
                pv: parse_line(root, &g.pv),
                found: g.found,
                crucial: g.crucial,
            });
        }
        let mut lines = Vec::with_capacity(self.lines.len());
        for l in &self.lines {
            let moves = parse_all(root, &l.moves)?;
            lines.push(LineReport {
                san: san_line(root, &moves),
                moves,
                assessment: l.assessment,
                first: l.first,
                end: l.end,
                engine_assessment: l.engine,
                good_candidate: l.good_candidate,
                agree_plies: usize::from(l.agree_plies),
                accuracy: fraction(l.accuracy),
                sound: l.sound,
                depth_credit: fraction(l.depth_credit),
                quality: fraction(l.quality),
            });
        }
        let found = u8::try_from(good.iter().filter(|g| g.found).count()).unwrap_or(u8::MAX);
        let good_total = u8::try_from(good.len()).unwrap_or(u8::MAX);
        Some(Report {
            good,
            lines,
            found,
            good_total,
            score,
            parts: ScoreParts {
                candidates: fraction(self.candidates),
                accuracy: fraction(self.accuracy),
                quality: fraction(self.quality),
                penalty: f32::from(self.penalty.min(1000)) / 10.0,
            },
        })
    }

    // ---- TOML ----

    /// The record as a TOML table.
    pub fn to_table(&self) -> Table {
        let good: Vec<Value> = self
            .good
            .iter()
            .map(|g| {
                let mut t = Table::new();
                t.insert("mv".into(), Value::String(g.mv.clone()));
                put_eval(&mut t, "eval", Some(g.eval));
                t.insert("depth".into(), int(u64::from(g.depth)));
                t.insert("pv".into(), Value::String(g.pv.join(" ")));
                t.insert("found".into(), Value::Boolean(g.found));
                t.insert("crucial".into(), Value::Boolean(g.crucial));
                Value::Table(t)
            })
            .collect();
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                let mut t = Table::new();
                t.insert("moves".into(), Value::String(l.moves.join(" ")));
                t.insert(
                    "assessment".into(),
                    Value::String(l.assessment.symbol().into()),
                );
                put_eval(&mut t, "first", l.first);
                put_eval(&mut t, "end", l.end);
                if let Some(engine) = l.engine {
                    t.insert("engine".into(), Value::String(engine.symbol().into()));
                }
                t.insert("good_candidate".into(), Value::Boolean(l.good_candidate));
                t.insert("agree".into(), int(u64::from(l.agree_plies)));
                t.insert("accuracy".into(), int(u64::from(l.accuracy)));
                t.insert("sound".into(), Value::Boolean(l.sound));
                t.insert("depth_credit".into(), int(u64::from(l.depth_credit)));
                t.insert("quality".into(), int(u64::from(l.quality)));
                Value::Table(t)
            })
            .collect();
        let mut t = Table::new();
        t.insert("candidates".into(), int(u64::from(self.candidates)));
        t.insert("accuracy".into(), int(u64::from(self.accuracy)));
        t.insert("quality".into(), int(u64::from(self.quality)));
        t.insert("penalty".into(), int(u64::from(self.penalty)));
        if !good.is_empty() {
            t.insert("good".into(), Value::Array(good));
        }
        if !lines.is_empty() {
            t.insert("lines".into(), Value::Array(lines));
        }
        t
    }

    /// Reads a record from a TOML table; `None` when it is not usable.
    pub fn from_table(table: &Table) -> Option<ReviewRecord> {
        let mut good = Vec::new();
        if let Some(Value::Array(items)) = table.get("good") {
            for item in items.iter().take(MAX_RECORD_GOOD) {
                good.push(parse_good(item.as_table()?)?);
            }
        }
        let mut lines = Vec::new();
        if let Some(Value::Array(items)) = table.get("lines") {
            for item in items.iter().take(MAX_LINES) {
                lines.push(parse_line_record(item.as_table()?)?);
            }
        }
        if lines.is_empty() {
            return None;
        }
        Some(ReviewRecord {
            good,
            lines,
            candidates: get_pm(table, "candidates"),
            accuracy: get_pm(table, "accuracy"),
            quality: get_pm(table, "quality"),
            penalty: get_pm(table, "penalty"),
        })
    }
}

fn int(n: u64) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn put_eval(table: &mut Table, key: &str, eval: Option<Eval>) {
    let Some(eval) = eval else {
        return;
    };
    table.insert(format!("{key}_cp"), Value::Integer(i64::from(eval.cp)));
    if let Some(mate) = eval.mate_in {
        table.insert(format!("{key}_mate"), Value::Integer(i64::from(mate)));
    }
}

fn get_i32(table: &Table, key: &str) -> Option<i32> {
    table
        .get(key)
        .and_then(Value::as_integer)
        .and_then(|n| i32::try_from(n).ok())
}

fn get_eval(table: &Table, key: &str) -> Option<Eval> {
    let cp = get_i32(table, &format!("{key}_cp"))?;
    Some(Eval {
        cp,
        mate_in: get_i32(table, &format!("{key}_mate")),
    })
}

fn get_pm(table: &Table, key: &str) -> u16 {
    table
        .get(key)
        .and_then(Value::as_integer)
        .and_then(|n| u16::try_from(n).ok())
        .map_or(0, |n| n.min(1000))
}

fn get_bool(table: &Table, key: &str) -> bool {
    table.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn get_words(table: &Table, key: &str, cap: usize) -> Option<Vec<String>> {
    let text = table.get(key)?.as_str()?;
    if text.chars().count() > MAX_MOVES_TEXT {
        return None;
    }
    Some(
        text.split_whitespace()
            .take(cap)
            .map(str::to_owned)
            .collect(),
    )
}

fn parse_good(t: &Table) -> Option<GoodRecord> {
    let mv = t.get("mv")?.as_str()?;
    if mv.chars().count() > 8 {
        return None;
    }
    Some(GoodRecord {
        mv: mv.to_owned(),
        eval: get_eval(t, "eval")?,
        depth: t
            .get("depth")
            .and_then(Value::as_integer)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        pv: get_words(t, "pv", MAX_RECORD_PV).unwrap_or_default(),
        found: get_bool(t, "found"),
        crucial: get_bool(t, "crucial"),
    })
}

fn parse_line_record(t: &Table) -> Option<LineRecord> {
    let moves = get_words(t, "moves", MAX_LINE_PLIES)?;
    if moves.is_empty() {
        return None;
    }
    Some(LineRecord {
        moves,
        assessment: Assessment::from_symbol(t.get("assessment")?.as_str()?)?,
        first: get_eval(t, "first"),
        end: get_eval(t, "end"),
        engine: t
            .get("engine")
            .and_then(Value::as_str)
            .and_then(Assessment::from_symbol),
        good_candidate: get_bool(t, "good_candidate"),
        agree_plies: t
            .get("agree")
            .and_then(Value::as_integer)
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(0),
        accuracy: get_pm(t, "accuracy"),
        sound: get_bool(t, "sound"),
        depth_credit: get_pm(t, "depth_credit"),
        quality: get_pm(t, "quality"),
    })
}

/// The moves in UCI, each in the position the previous ones lead to.
fn uci_line(start: &Position, moves: &[Move], cap: usize) -> Vec<String> {
    let mut pos = start.clone();
    let mut out = Vec::new();
    for mv in moves.iter().take(cap) {
        out.push(pos.uci(*mv));
        match pos.after(*mv) {
            Ok(next) => pos = next,
            Err(_) => break,
        }
    }
    out
}

/// The moves of `words` that fit, one after the other; the first that does
/// not ends the list.
fn parse_line(start: &Position, words: &[String]) -> Vec<Move> {
    let mut pos = start.clone();
    let mut out = Vec::new();
    for word in words {
        let Ok(mv) = pos.parse_uci(word) else {
            break;
        };
        let Ok(next) = pos.after(mv) else {
            break;
        };
        out.push(mv);
        pos = next;
    }
    out
}

/// All the moves of `words`, or `None` when one does not fit.
fn parse_all(start: &Position, words: &[String]) -> Option<Vec<Move>> {
    let moves = parse_line(start, words);
    (moves.len() == words.len()).then_some(moves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calvar::compare::compare;
    use crate::calvar::eval::{LineProbes, Probe, RootLine};
    use crate::calvar::line::UserLine;

    fn report() -> (Position, Report) {
        let root = Position::new();
        let e4 = root.parse_uci("e2e4").expect("e4");
        let after_e4 = root.after(e4).expect("after");
        let e5 = after_e4.parse_uci("e7e5").expect("e5");
        let d4 = root.parse_uci("d2d4").expect("d4");
        let root_lines = vec![
            RootLine {
                multipv: 1,
                depth: 14,
                eval: Eval::centipawns(30),
                pv: vec![e4, e5],
            },
            RootLine {
                multipv: 2,
                depth: 14,
                eval: Eval::centipawns(25),
                pv: vec![d4],
            },
        ];
        let user = vec![UserLine {
            moves: vec![e4, e5],
            assessment: Assessment::WhiteSlightly,
        }];
        let probes = vec![LineProbes {
            first: Some(Probe {
                eval: Eval::centipawns(28),
                depth: 12,
                pv: vec![e5],
            }),
            end: Some(Probe {
                eval: Eval::from_score(mychess_engine::Score::Mate(3)),
                depth: 12,
                pv: Vec::new(),
            }),
        }];
        let report = compare(&root, &root_lines, &user, &probes);
        (root, report)
    }

    #[test]
    fn a_record_turns_back_into_the_same_report() {
        let (root, report) = report();
        let record = ReviewRecord::from_report(&root, &report);
        let back = record.to_report(&root, report.score).expect("report");
        assert_eq!(back, report);
    }

    #[test]
    fn a_record_survives_toml() {
        let (root, report) = report();
        let record = ReviewRecord::from_report(&root, &report);
        let table = record.to_table();
        let text = toml::to_string(&table).expect("render");
        let parsed: Table = text.parse().expect("parse");
        assert_eq!(ReviewRecord::from_table(&parsed), Some(record));
    }

    #[test]
    fn a_move_that_does_not_fit_the_position_drops_the_report() {
        let (root, report) = report();
        let mut record = ReviewRecord::from_report(&root, &report);
        record.lines[0].moves = vec!["e2e5".to_owned()];
        assert_eq!(record.to_report(&root, 50), None);
        let other = Position::from_fen("4k3/8/8/8/8/8/8/4K2R w - - 0 1").expect("fen");
        let record = ReviewRecord::from_report(&root, &report);
        assert_eq!(record.to_report(&other, 50), None);
    }

    #[test]
    fn a_table_without_lines_or_with_a_bad_line_is_not_a_record() {
        let empty: Table = "candidates = 500".parse().expect("parse");
        assert_eq!(ReviewRecord::from_table(&empty), None);
        let bad: Table = "[[lines]]\nmoves = \"e2e4\"\nassessment = \"??\"\n"
            .parse()
            .expect("parse");
        assert_eq!(ReviewRecord::from_table(&bad), None);
    }

    #[test]
    fn long_lists_are_cut_to_their_caps() {
        let (root, report) = report();
        let mut record = ReviewRecord::from_report(&root, &report);
        let line = record.lines[0].clone();
        record.lines = vec![line; MAX_LINES + 5];
        let parsed = ReviewRecord::from_table(&record.to_table()).expect("record");
        assert_eq!(parsed.lines.len(), MAX_LINES);
    }

    #[test]
    fn an_extreme_score_in_the_file_does_not_panic() {
        let (root, report) = report();
        let mut record = ReviewRecord::from_report(&root, &report);
        let wild = Eval {
            cp: i32::MIN,
            mate_in: None,
        };
        for line in &mut record.lines {
            line.first = Some(wild);
            line.end = Some(wild);
        }
        let text = toml::to_string(&record.to_table()).expect("render");
        let parsed: Table = text.parse().expect("parse");
        let record = ReviewRecord::from_table(&parsed).expect("record");
        assert!(record.lines.iter().all(|l| l.end == Some(wild)));
        // Turning it back into a report must not overflow either.
        drop(record.to_report(&root, 50));
    }
}
