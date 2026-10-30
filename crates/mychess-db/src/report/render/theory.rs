//! The blocks of the theory table and of its notes (Scid: `PrintText`,
//! `PrintNotes`, `OpLine::PrintNote`).

use std::collections::HashMap;
use std::fmt::Write as _;

use mychess_core::Color;

use super::blocks::{game_summary, stem_line};
use super::{Block, Join, NoteBlock, NoteText, StemView, TheoryBlock, TheoryCell, TheoryRow};
use crate::report::labels::{ReportLabels, fill};
use crate::report::model::{COLUMNS, ReportCore, ReportLine, TableRow};
use crate::report::options::ReportOptions;
use crate::report::stats::oriented;

/// A move as it is printed in a cell of the table: without `+` and `x`
/// (Scid: `OPTABLE_Compact`).
fn compact(san: &str) -> String {
    san.chars().filter(|c| *c != '+' && *c != 'x').collect()
}

/// The text of the notes of one game (`OpLine::PrintNote` without the
/// summary): the first move shown, with its number, and the rest.
fn note_moves(line: &ReportLine, first_number: usize, start: usize) -> (String, String) {
    let (mut white, mut number) = (true, first_number);
    let (mut first, mut rest) = (String::new(), String::new());
    for i in 0..line.moves.len() {
        if i >= start {
            let mv = line.move_at(i);
            if i == start {
                first = format!("{number}{}{mv}", if white { "." } else { "..." });
            } else if white {
                let _ = write!(rest, " {number}.{mv}");
            } else {
                rest.push(' ');
                rest.push_str(mv);
            }
        }
        if white {
            white = false;
        } else {
            white = true;
            number += 1;
        }
    }
    (first, rest)
}

fn notes(core: &ReportCore, first_number: usize) -> Vec<NoteBlock> {
    core.notes
        .iter()
        .map(|note| {
            let mut entries = Vec::new();
            let mut prev: Option<&ReportLine> = None;
            for e in &note.entries {
                let Some(line) = core.lines.get(e.line) else {
                    continue;
                };
                let mut start = e.start;
                let mut join = Join::First;
                if let Some(p) = prev {
                    start = line.common_length(p);
                    join = if start <= e.start && p.moves.len() > start {
                        Join::Paragraph
                    } else {
                        Join::Next
                    };
                }
                let (first_move, other_moves) = note_moves(line, first_number, start);
                entries.push(NoteText {
                    join,
                    first_move,
                    other_moves,
                    summary: game_summary(line, false),
                });
                prev = Some(line);
            }
            NoteBlock {
                number: note.number,
                entries,
            }
        })
        .collect()
}

/// Appends the theory table and its notes. `in_report` is false for the
/// table alone, where the comment of the limit is not shown. `view` is the
/// colour whose score the table shows (White for the opening report, the
/// colour of the player in the player report).
pub(super) fn push(
    blocks: &mut Vec<Block>,
    core: &ReportCore,
    line: &StemView<'_>,
    view: Color,
    opts: &ReportOptions,
    labels: &ReportLabels,
    in_report: bool,
) {
    if core.rows.is_empty() {
        if !in_report {
            blocks.push(Block::Paragraph(labels.empty.clone()));
        }
        return;
    }
    let first_number = line.first_move_number();
    let mut stem = stem_line(line).join(" ");
    if let Some(ex) = line.excluded {
        let dots = if line.plies().is_multiple_of(2) {
            "."
        } else {
            "..."
        };
        let _ = write!(stem, " (X {first_number}{dots}{ex})");
    }
    let theory = oriented(&core.theory, view);
    let pts = theory.white.saturating_mul(2).saturating_add(theory.draws);
    let mut points = (pts / 2).to_string();
    if pts % 2 == 1 {
        points.push(labels.decimal);
        points.push('5');
    }
    let heading = format!(
        "+{} ={} -{} ({points}/{}: {}%)",
        theory.white,
        theory.draws,
        theory.black,
        core.theory_games(),
        (crate::report::stats::score_tenths(&theory) + 5) / 10
    );
    let anchors: HashMap<(usize, usize), u32> = core
        .notes
        .iter()
        .map(|n| ((n.row, n.cell), n.number))
        .collect();
    let rows = core
        .rows
        .iter()
        .enumerate()
        .filter_map(|(r, row)| {
            let head = core.lines.get(row.line)?;
            let same = match r.checked_sub(1).and_then(|p| core.rows.get(p)) {
                Some(prev) => core
                    .lines
                    .get(prev.line)
                    .map_or(0, |p| head.common_length(p)),
                None => 0,
            };
            let cells = (0..2 * COLUMNS)
                .map(|j| TheoryCell {
                    text: if j < same {
                        "...".to_owned()
                    } else {
                        compact(head.move_at(j))
                    },
                    note: anchors.get(&(r, j)).copied().unwrap_or(0),
                })
                .collect();
            let score = match view {
                Color::White => row.score_percent(),
                // The points of Black are what White left: 2 per game.
                Color::Black => TableRow {
                    points: row.count.saturating_mul(2).saturating_sub(row.points),
                    ..row.clone()
                }
                .score_percent(),
            };
            Some(TheoryRow {
                number: r + 1,
                cells,
                count: row.count,
                score,
            })
        })
        .collect();
    let comment = if in_report && core.theory_games() > opts.max_table_games {
        fill(&labels.table_comment, opts.max_table_games, None)
    } else {
        String::new()
    };
    blocks.push(Block::Theory(Box::new(TheoryBlock {
        stem,
        heading,
        first_number,
        rows,
        comment,
    })));
    let n = notes(core, first_number);
    if !n.is_empty() {
        blocks.push(Block::Notes(n));
    }
}
