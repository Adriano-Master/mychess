// Scid: src/optable.cpp `OpTable::GuessNumRows` 747-750, `IsRowMergable`,
// `MergeRow`, `SelectTableLines`, `MakeRows` 780-902, `SortTableLines`
// 915-990 and `HasNotes` 1314-1360. Copyright (c) 2001-2003 Shane Hudson.
// Scid is distributed under the GNU General Public License (see its
// COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The theory table: order of the lines, fusion into rows and footnotes
//! (decisions D3610 to D3613). The algorithm is that of `MakeRows`; the
//! order of the ties is fixed and documented here (D3611).

use std::cmp::Ordering;

use super::model::{COLUMNS, MAX_LINE_MOVES, Note, NoteEntry, ReportLine, TableRow};

/// Fewest rows of the table (Scid: `OPTABLE_MIN_ROWS`).
pub const MIN_ROWS: u8 = 5;
/// Most rows of the table (Scid: `OPTABLE_MAX_ROWS`).
pub const MAX_ROWS: u8 = 20;

/// Whether the more frequent move comes first (the spec, rule 6.4). The C
/// code of the Scid sorts with `count_i - count_j > 0 → swap`, which reads as
/// the *less* frequent first; the spec and its acceptance criteria say the
/// opposite. The direction is this one constant (and the average rating of
/// two single lines follows it).
const MOST_FREQUENT_FIRST: bool = true;

/// The table built from the lines chosen for it.
#[derive(Debug, Default)]
pub(crate) struct Built {
    /// The chosen lines in table order (Scid: `Line[0..NumTableLines]`).
    pub order: Vec<usize>,
    /// The rows.
    pub rows: Vec<TableRow>,
    /// The footnotes.
    pub notes: Vec<Note>,
    /// For each line index (of `lines`) the note that lists it, 0 if none.
    pub note_of: Vec<u32>,
}

/// Integer square root.
fn isqrt(value: usize) -> usize {
    let mut x = 0usize;
    while (x + 1).saturating_mul(x + 1) <= value {
        x += 1;
    }
    x
}

/// The automatic number of rows: `isqrt(3 * n / 4) + 3` within 5 and 20.
pub(crate) fn guess_rows(table_lines: usize) -> usize {
    (isqrt(table_lines.saturating_mul(3) / 4) + 3)
        .clamp(usize::from(MIN_ROWS), usize::from(MAX_ROWS))
}

struct Group {
    start: usize,
    len: usize,
}

/// The order of two groups of lines that differ in the move at one depth.
fn compare_groups(
    lines: &[ReportLine],
    idx: &[usize],
    depth: usize,
    a: &Group,
    b: &Group,
) -> Ordering {
    let text = |g: &Group| idx.get(g.start).map_or("", |&l| lines[l].move_at(depth));
    let (ta, tb) = (text(a), text(b));
    // Lines with no more moves come first.
    match (ta.is_empty(), tb.is_empty()) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    let mut ord = a.len.cmp(&b.len);
    if ord == Ordering::Equal && a.len == 1 {
        let elo = |g: &Group| idx.get(g.start).map_or(0, |&l| lines[l].avg_elo);
        ord = elo(a).cmp(&elo(b));
    }
    if MOST_FREQUENT_FIRST {
        ord = ord.reverse();
    }
    ord.then_with(|| ta.as_bytes().cmp(tb.as_bytes()))
}

/// Orders `idx` recursively by the move at `depth` (Scid: `SortTableLines`).
/// On entry the lines are in game-number order, so equal lines stay in it.
fn sort_lines(lines: &[ReportLine], idx: &mut [usize], depth: usize) {
    if idx.len() < 2 || depth >= MAX_LINE_MOVES {
        return;
    }
    // Stable: lines with the same move become adjacent, in game order.
    idx.sort_by(|&a, &b| lines[a].move_at(depth).cmp(lines[b].move_at(depth)));
    let mut groups: Vec<Group> = Vec::new();
    for (i, &l) in idx.iter().enumerate() {
        match groups.last_mut() {
            Some(g)
                if idx.get(g.start).is_some_and(|&first| {
                    lines[first].move_at(depth) == lines[l].move_at(depth)
                }) =>
            {
                g.len += 1;
            }
            _ => groups.push(Group { start: i, len: 1 }),
        }
    }
    let snapshot = idx.to_vec();
    groups.sort_by(|a, b| compare_groups(lines, &snapshot, depth, a, b));
    let mut at = 0;
    for g in &groups {
        let end = at + g.len;
        if let (Some(dst), Some(src)) =
            (idx.get_mut(at..end), snapshot.get(g.start..g.start + g.len))
        {
            dst.copy_from_slice(src);
        }
        if let Some(part) = idx.get_mut(at..end) {
            sort_lines(lines, part, depth + 1);
        }
        at = end;
    }
}

struct RowState {
    head: usize,
    chain: Vec<usize>,
    count: u32,
    points: u32,
}

fn merge_row(rows: &mut Vec<RowState>, r: usize) {
    if r + 1 >= rows.len() {
        return;
    }
    let sub = rows.remove(r);
    if let Some(next) = rows.get_mut(r) {
        // Scid `Insert`: the sub-line's own notes, the sub-line, then the
        // notes the target already had.
        let mut chain = sub.chain;
        chain.push(sub.head);
        chain.append(&mut next.chain);
        next.chain = chain;
        next.count = next.count.saturating_add(sub.count);
        next.points = next.points.saturating_add(sub.points);
    }
}

fn row_common(lines: &[ReportLine], rows: &[RowState], a: usize, b: usize) -> usize {
    match (rows.get(a), rows.get(b)) {
        (Some(x), Some(y)) => lines[x.head].common_length(&lines[y.head]),
        _ => 0,
    }
}

/// Builds the table from the lines in `chosen` (indices in `lines`), with
/// about `target_rows` rows.
pub(crate) fn build(lines: &[ReportLine], chosen: &[usize], target_rows: usize) -> Built {
    let mut built = Built {
        note_of: vec![0; lines.len()],
        ..Built::default()
    };
    if chosen.is_empty() {
        return built;
    }
    let mut order = chosen.to_vec();
    order.sort_by_key(|&l| lines[l].game);
    sort_lines(lines, &mut order, 0);

    let mut rows: Vec<RowState> = order
        .iter()
        .map(|&l| RowState {
            head: l,
            chain: Vec::new(),
            count: 1,
            points: lines[l].white_points(),
        })
        .collect();
    let target = target_rows.clamp(usize::from(MIN_ROWS), usize::from(MAX_ROWS));

    // Rows that share all their columns, or that are a prefix of the next.
    let mut i = 0;
    while i + 1 < rows.len() {
        let common = row_common(lines, &rows, i, i + 1);
        let len = lines[rows[i].head].moves.len();
        if common >= 2 * COLUMNS || common == len {
            merge_row(&mut rows, i);
        } else {
            i += 1;
        }
    }
    // Then the cheapest merges until the target is reached.
    while rows.len() > target && rows.len() >= 2 {
        let mut best_cost = rows[0].count.saturating_add(rows[1].count);
        let mut best_row = 0;
        for r in 1..rows.len() - 1 {
            if row_common(lines, &rows, r, r + 1) > row_common(lines, &rows, r, r - 1) {
                let cost = rows[r].count.saturating_add(rows[r + 1].count);
                if cost < best_cost {
                    best_cost = cost;
                    best_row = r;
                }
            }
        }
        merge_row(&mut rows, best_row);
    }

    let mut note_move: Vec<usize> = vec![0; lines.len()];
    for row in &rows {
        for &sub in &row.chain {
            note_move[sub] = lines[sub].common_length(&lines[row.head]);
        }
    }
    let notes = number_notes(lines, &rows, &mut note_move, &mut built.note_of);
    built.notes = collect_notes(&order, &notes, &built.note_of, &note_move);
    built.rows = rows
        .into_iter()
        .map(|r| TableRow {
            line: r.head,
            merged: r.chain,
            count: r.count,
            points: r.points,
        })
        .collect();
    built.order = order;
    built
}

/// Where a note was created: `(row, cell)`, in note order.
type Anchors = Vec<(usize, usize)>;

/// Walks the cells row by row and numbers the notes (Scid: `HasNotes`).
fn number_notes(
    lines: &[ReportLine],
    rows: &[RowState],
    note_move: &mut [usize],
    note_of: &mut [u32],
) -> Anchors {
    let mut anchors: Anchors = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let head_len = lines[row.head].moves.len();
        for j in 0..2 * COLUMNS {
            let last =
                (head_len > 0 && j == head_len - 1) || (j == 2 * COLUMNS - 1 && j < head_len);
            let mut seen = false;
            let mut number = u32::try_from(anchors.len()).unwrap_or(u32::MAX);
            let mark = |anchors: &mut Anchors, seen: &mut bool, number: &mut u32| {
                if !*seen {
                    *seen = true;
                    anchors.push((r, j));
                    *number = u32::try_from(anchors.len()).unwrap_or(u32::MAX);
                }
            };
            if last {
                mark(&mut anchors, &mut seen, &mut number);
                note_move[row.head] = j + 1;
                note_of[row.head] = number;
            }
            for &sub in &row.chain {
                if note_move[sub] == j {
                    mark(&mut anchors, &mut seen, &mut number);
                    note_of[sub] = number;
                }
                if note_of[sub] == 0 && last {
                    note_of[sub] = number;
                    note_move[sub] = j + 1;
                }
            }
        }
    }
    anchors
}

fn collect_notes(
    order: &[usize],
    anchors: &Anchors,
    note_of: &[u32],
    note_move: &[usize],
) -> Vec<Note> {
    anchors
        .iter()
        .enumerate()
        .map(|(i, &(row, cell))| {
            let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let entries = order
                .iter()
                .rev()
                .filter(|&&l| note_of[l] == number)
                .map(|&l| NoteEntry {
                    line: l,
                    start: note_move[l],
                })
                .collect();
            Note {
                number,
                row,
                cell,
                entries,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mychess_core::{GameResult, PartialDate};

    use super::*;
    use crate::report::model::{EndTheme, NUM_THEMES};

    fn line(game: u32, elo: u16, result: GameResult, moves: &[&str]) -> ReportLine {
        ReportLine {
            game,
            white_id: 0,
            black_id: 0,
            white: String::new(),
            black: String::new(),
            white_elo: 0,
            black_elo: 0,
            avg_elo: elo,
            result,
            half_moves: 40,
            date: PartialDate::UNKNOWN,
            site: String::new(),
            eco: None,
            start_ply: 0,
            moves: moves.iter().map(|m| (*m).to_owned()).collect(),
            short_game: false,
            themes: [0; NUM_THEMES],
            end_theme: EndTheme::Pawns,
            move_order: 0,
            note: 0,
        }
    }

    const W: GameResult = GameResult::White;

    #[test]
    fn guess_rows_follows_the_formula_and_limits() {
        assert_eq!(guess_rows(0), 5);
        assert_eq!(guess_rows(5), 5);
        assert_eq!(guess_rows(30), 7); // isqrt(22) + 3
        assert_eq!(guess_rows(500), 20); // isqrt(375) + 3 = 22, clamped
        assert_eq!(guess_rows(1_000_000), 20);
    }

    #[test]
    fn most_frequent_move_first_then_rating_then_alphabet() {
        let lines = vec![
            line(1, 2000, W, &["e4"]),
            line(2, 2400, W, &["d4"]),
            line(3, 2100, W, &["c4"]),
            line(4, 2000, W, &["e4"]),
            line(5, 2400, W, &["b4"]),
        ];
        let built = build(&lines, &[0, 1, 2, 3, 4], 20);
        let moves: Vec<&str> = built.order.iter().map(|&l| lines[l].move_at(0)).collect();
        // e4 twice first; the singles by rating (2400 b4/d4 then 2100),
        // ties alphabetical.
        assert_eq!(moves, ["e4", "e4", "b4", "d4", "c4"]);
    }

    #[test]
    fn short_lines_come_first_and_merge_into_the_next() {
        let lines = vec![
            line(1, 2000, W, &["e4", "e5", "Nf3"]),
            line(2, 2000, GameResult::Black, &["e4", "e5"]),
        ];
        let built = build(&lines, &[0, 1], 5);
        // Line 2 (prefix) merges into line 1.
        assert_eq!(built.rows.len(), 1);
        let row = &built.rows[0];
        assert_eq!((row.line, row.merged.clone(), row.count), (0, vec![1], 2));
        assert_eq!(row.points, 2);
        assert_eq!(row.score_percent(), 50);
        // The main line has the last-move note, the prefix is listed in it.
        assert_eq!(built.notes.len(), 1);
        assert_eq!(built.notes[0].cell, 2);
        assert_eq!(built.note_of, vec![1, 1]);
    }

    #[test]
    fn rows_merge_down_to_the_target_and_every_line_appears_once() {
        // 30 distinct lines with a common start.
        let names = [
            "a3", "b3", "c3", "d3", "e3", "f3", "g3", "h3", "a4", "b4", "c4", "d4", "e4", "f4",
            "g4", "h4", "Na3", "Nc3", "Nf3", "Nh3", "Bb2", "Bc2", "Qd2", "Qe2", "Rb1", "Kd1",
            "Ke2", "Kf2", "Kg2", "Kh2",
        ];
        let lines: Vec<ReportLine> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                line(
                    u32::try_from(i + 1).unwrap(),
                    2000 + u16::try_from(i).unwrap(),
                    W,
                    &["e4", "e5", n, "Nc6"],
                )
            })
            .collect();
        let chosen: Vec<usize> = (0..lines.len()).collect();
        let built = build(&lines, &chosen, 8);
        assert_eq!(built.rows.len(), 8);
        let mut seen: Vec<usize> = built
            .rows
            .iter()
            .flat_map(|r| std::iter::once(r.line).chain(r.merged.iter().copied()))
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, chosen, "each line is a row head or merged once");
        assert_eq!(built.rows.iter().map(|r| r.count).sum::<u32>(), 30);
        // Every merged line is in exactly one note.
        let in_notes: usize = built.notes.iter().map(|n| n.entries.len()).sum();
        assert_eq!(in_notes, built.note_of.iter().filter(|&&n| n > 0).count());
        assert!(built.rows.iter().all(|r| built.note_of[r.line] > 0));
    }

    #[test]
    fn long_lines_note_their_extra_moves_at_the_last_column() {
        let m: Vec<String> = (0..18).map(|i| format!("m{i}")).collect();
        let refs: Vec<&str> = m.iter().map(String::as_str).collect();
        let lines = vec![line(1, 2000, W, &refs)];
        let built = build(&lines, &[0], 5);
        assert_eq!(built.notes.len(), 1);
        assert_eq!(built.notes[0].cell, 15);
        assert_eq!(
            built.notes[0].entries,
            vec![NoteEntry { line: 0, start: 16 }]
        );
    }

    #[test]
    fn empty_input_gives_an_empty_table() {
        let built = build(&[], &[], 5);
        assert!(built.rows.is_empty() && built.notes.is_empty() && built.order.is_empty());
    }
}
