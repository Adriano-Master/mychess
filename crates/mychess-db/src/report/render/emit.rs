//! Writes the blocks of a document in text, HTML or LaTeX.

use std::fmt::Write as _;

use mychess_core::GameResult;

use super::{Align, Block, Fmt, GameItem, Grid, Join, NoteBlock, Summary, TheoryBlock, TheoryRow};
use crate::report::labels::ReportLabels;

const HRULE: &str =
    "-------------------------------------------------------------------------------";
const TITLE_RULE: &str = "--------------------------------------------------------------";
const CELL_WIDTH: usize = 9;

/// Escapes text for `fmt`. Control characters (`NUL` included) never reach
/// the output.
pub(super) fn esc(fmt: Fmt, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            out.push(' ');
            continue;
        }
        match (fmt, c) {
            (Fmt::Html, '&') => out.push_str("&amp;"),
            (Fmt::Html, '<') => out.push_str("&lt;"),
            (Fmt::Html, '>') => out.push_str("&gt;"),
            (Fmt::Html, '"') => out.push_str("&quot;"),
            (Fmt::Html, '\'') => out.push_str("&#39;"),
            (Fmt::Latex, '&') => out.push_str("\\&"),
            (Fmt::Latex, '%') => out.push_str("\\%"),
            (Fmt::Latex, '$') => out.push_str("\\$"),
            (Fmt::Latex, '#') => out.push_str("\\#"),
            (Fmt::Latex, '_') => out.push_str("\\_"),
            (Fmt::Latex, '{') => out.push_str("\\{"),
            (Fmt::Latex, '}') => out.push_str("\\}"),
            (Fmt::Latex, '~') => out.push_str("\\textasciitilde{}"),
            (Fmt::Latex, '^') => out.push_str("\\textasciicircum{}"),
            (Fmt::Latex, '\\') => out.push_str("\\textbackslash{}"),
            (Fmt::Latex, '\u{df}') => out.push_str("{\\ss}"),
            (Fmt::Text | Fmt::Html | Fmt::Latex, c) => out.push(c),
        }
    }
    out
}

fn width(text: &str) -> usize {
    text.chars().count()
}

fn pad_left(text: &str, to: usize) -> String {
    let mut s = text.to_owned();
    for _ in width(text)..to {
        s.push(' ');
    }
    s
}

fn pad_right(text: &str, to: usize) -> String {
    let mut s = String::new();
    for _ in width(text)..to {
        s.push(' ');
    }
    s.push_str(text);
    s
}

const HTML_START: &str = "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>";
const HTML_STYLE: &str = "</title>\n<style type=\"text/css\">\nh1, h2, h3 { color: #990000 }\n.player { color: darkblue }\n.elo { color: green; font-style: italic }\nsup { color: red }\n</style>\n</head>\n<body bgcolor=\"#ffffff\">\n";

const LATEX_PREAMBLE: &str = "\\documentclass[a4paper,10pt]{article}\n\\usepackage[utf8]{inputenc}\n\\usepackage[T1]{fontenc}\n\\usepackage{xcolor}\n\\usepackage{tabularx}\n\\usepackage{xskak}\n\\newcommand{\\win}{1--0}\n\\newcommand{\\draw}{\\ensuremath{\\frac12}--\\ensuremath{\\frac12}}\n\\newcommand{\\loss}{0--1}\n\\providecommand{\\p}{\\textit{p}}\n\\begin{document}\n";

/// Writes the document.
pub(super) fn document(fmt: Fmt, blocks: &[Block], labels: &ReportLabels) -> String {
    let mut out = String::new();
    let title = blocks
        .iter()
        .find_map(|b| match b {
            Block::Title { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("");
    match fmt {
        Fmt::Text => {}
        Fmt::Html => {
            out.push_str(HTML_START);
            out.push_str(&esc(fmt, title));
            out.push_str(HTML_STYLE);
        }
        Fmt::Latex => out.push_str(LATEX_PREAMBLE),
    }
    for block in blocks {
        write_block(fmt, block, labels, &mut out);
    }
    match fmt {
        Fmt::Text => {
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        Fmt::Html => out.push_str("</body>\n</html>\n"),
        Fmt::Latex => out.push_str("\n\\end{document}\n"),
    }
    out
}

fn write_block(fmt: Fmt, block: &Block, labels: &ReportLabels, out: &mut String) {
    match block {
        Block::Title { text, line } => title(fmt, text, line, out),
        Block::Board { fen } => board(fmt, fen, out),
        Block::Header(rows) => header(fmt, rows, out),
        Block::Section { number, text } => section(fmt, *number, text, out),
        Block::Subsection {
            section,
            number,
            text,
        } => subsection(fmt, *section, *number, text, out),
        Block::Paragraph(text) => match fmt {
            Fmt::Text => {
                let _ = writeln!(out, "{}", esc(fmt, text));
            }
            Fmt::Html => {
                let _ = writeln!(out, "<p>{}</p>", esc(fmt, text));
            }
            Fmt::Latex => {
                let _ = writeln!(out, "{}\\par", esc(fmt, text));
            }
        },
        Block::Grid(grid) => grid_block(fmt, grid, out),
        Block::Games(items) => games(fmt, items, out),
        Block::Theory(t) => theory(fmt, t, labels, out),
        Block::Notes(notes) => write_notes(fmt, notes, out),
    }
}

fn title(fmt: Fmt, text: &str, line: &[String], out: &mut String) {
    match fmt {
        Fmt::Text => {
            // The Scid upper-cases the title; moves keep their case.
            let shown = if line.is_empty() {
                esc(fmt, text).to_uppercase()
            } else {
                esc(fmt, text)
            };
            let _ = write!(
                out,
                "{TITLE_RULE}\n{}{shown}\n{TITLE_RULE}\n\n",
                " ".repeat(24)
            );
        }
        Fmt::Html => {
            let _ = write!(out, "<h2><center>{}</center></h2>\n\n", esc(fmt, text));
        }
        Fmt::Latex => {
            out.push_str("\\newchessgame%\n");
            if line.is_empty() {
                let _ = write!(
                    out,
                    "\\begin{{center}}{{\\LARGE \\color{{blue}}{}}}\\end{{center}}\n\n",
                    esc(fmt, text)
                );
            } else {
                // The argument of \hidemoves is read by xskak: no escapes,
                // no check marks (they would need escapes themselves).
                let moves: String = line
                    .iter()
                    .flat_map(|m| m.chars().chain(std::iter::once(' ')))
                    .filter(|c| !matches!(c, '#' | '+' | '%' | '&' | '{' | '}' | '\\' | '$'))
                    .collect();
                let _ = write!(
                    out,
                    "\\hidemoves{{{}}}%\n\\begin{{center}}{{\\LARGE \\color{{blue}}\\printchessgame}}\\end{{center}}\n\n",
                    moves.trim()
                );
            }
        }
    }
}

fn board(fmt: Fmt, fen: &str, out: &mut String) {
    match fmt {
        Fmt::Text => {}
        Fmt::Html => {
            let _ = writeln!(out, "<p><code>{}</code></p>", esc(fmt, fen));
        }
        Fmt::Latex => {
            let fen: String = fen
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | ' ' | '-'))
                .collect();
            let _ = writeln!(
                out,
                "\\begin{{center}}\\chessboard[setfen={{{fen}}}]\\end{{center}}"
            );
        }
    }
}

fn header(fmt: Fmt, rows: &[(String, String)], out: &mut String) {
    match fmt {
        Fmt::Text => {
            out.push('\n');
            for (label, value) in rows {
                let _ = writeln!(out, "{}: {}", esc(fmt, label), esc(fmt, value));
            }
        }
        Fmt::Html => {
            out.push_str("<p>\n");
            for (label, value) in rows {
                let _ = writeln!(out, "{}: {}<br>", esc(fmt, label), esc(fmt, value));
            }
            out.push_str("</p>\n");
        }
        Fmt::Latex => {
            out.push_str("\\begin{tabular}{@{}ll@{}}\n");
            for (label, value) in rows {
                let _ = writeln!(out, "{}: & {} \\\\", esc(fmt, label), esc(fmt, value));
            }
            out.push_str("\\end{tabular}\n");
        }
    }
}

fn section(fmt: Fmt, number: u32, text: &str, out: &mut String) {
    match fmt {
        Fmt::Text => {
            let line = format!("{number}. {}", esc(fmt, text).to_uppercase());
            let rule = "-".repeat(width(&line).min(53));
            let _ = write!(out, "\n\n{line}\n{rule}\n");
        }
        Fmt::Html => {
            let _ = write!(out, "\n<h2>{number}. {}</h2>\n", esc(fmt, text));
        }
        Fmt::Latex => {
            let _ = write!(out, "\n\n\\section{{{}}}\n", esc(fmt, text));
        }
    }
}

fn subsection(fmt: Fmt, section: u32, number: u32, text: &str, out: &mut String) {
    match fmt {
        Fmt::Text => {
            let _ = write!(out, "\n{section}.{number}  {}\n\n", esc(fmt, text));
        }
        Fmt::Html => {
            let _ = write!(out, "\n<h3>{section}.{number} {}</h3>\n\n", esc(fmt, text));
        }
        Fmt::Latex => {
            let _ = write!(out, "\n\\subsection{{{}}}\n\n", esc(fmt, text));
        }
    }
}

fn grid_block(fmt: Fmt, grid: &Grid, out: &mut String) {
    let columns = grid.aligns.len();
    match fmt {
        Fmt::Text => {
            let mut widths = vec![0usize; columns];
            for row in grid.header.iter().chain(&grid.rows) {
                for (w, cell) in widths.iter_mut().zip(row) {
                    *w = (*w).max(width(cell));
                }
            }
            let line = |row: &[String], out: &mut String| {
                let mut cells = Vec::with_capacity(columns);
                for (i, (&w, &align)) in widths.iter().zip(&grid.aligns).enumerate() {
                    let text = esc(Fmt::Text, row.get(i).map_or("", String::as_str));
                    cells.push(match align {
                        Align::Left => pad_left(&text, w),
                        Align::Right => pad_right(&text, w),
                    });
                }
                out.push(' ');
                out.push_str(cells.join("  ").trim_end());
                out.push('\n');
            };
            for row in &grid.header {
                line(row, out);
            }
            for row in &grid.rows {
                line(row, out);
            }
        }
        Fmt::Html => {
            out.push_str("<table border=0 cellspacing=0 cellpadding=4>\n");
            for row in &grid.header {
                out.push_str("<tr>");
                for cell in row {
                    let _ = write!(out, "<th>{}</th>", esc(fmt, cell));
                }
                out.push_str("</tr>\n");
            }
            for row in &grid.rows {
                out.push_str("<tr>");
                for (i, cell) in row.iter().enumerate() {
                    let align = if grid.aligns.get(i) == Some(&Align::Right) {
                        " align=\"right\""
                    } else {
                        ""
                    };
                    let _ = write!(out, "<td{align}>{}</td>", esc(fmt, cell));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</table>\n");
        }
        Fmt::Latex => {
            let spec: String = grid
                .aligns
                .iter()
                .map(|a| if *a == Align::Right { 'r' } else { 'l' })
                .collect();
            let _ = writeln!(out, "\\begin{{tabular}}{{{spec}}}");
            let write_row = |row: &[String], bold: bool, out: &mut String| {
                let cells: Vec<String> = (0..columns)
                    .map(|i| {
                        let t = esc(fmt, row.get(i).map_or("", String::as_str));
                        if bold && !t.is_empty() {
                            format!("\\textbf{{{t}}}")
                        } else {
                            t
                        }
                    })
                    .collect();
                let _ = writeln!(out, "{} \\\\", cells.join(" & "));
            };
            for row in &grid.header {
                write_row(row, true, out);
            }
            if !grid.header.is_empty() {
                out.push_str("\\hline\n");
            }
            for row in &grid.rows {
                write_row(row, false, out);
            }
            out.push_str("\\end{tabular}\n");
        }
    }
}

/// `OpLine::PrintSummary`.
pub(super) fn summary(fmt: Fmt, s: &Summary, full_date: bool) -> String {
    let result = match (fmt, s.result) {
        (Fmt::Latex, GameResult::White) => "{\\win}",
        (Fmt::Latex, GameResult::Black) => "{\\loss}",
        (Fmt::Latex, GameResult::Draw) => "{\\draw}",
        (_, GameResult::White) => "1-0",
        (_, GameResult::Black) => "0-1",
        (_, GameResult::Draw) => "=-=",
        _ => "*",
    };
    let (pre_name, post_name) = match fmt {
        Fmt::Html => ("<span class=\"player\">", "</span>"),
        _ => ("", ""),
    };
    let (pre_elo, post_elo, sep) = match fmt {
        Fmt::Text => (" ", "", " - "),
        Fmt::Html => (" <span class=\"elo\">", "</span>", " -- "),
        Fmt::Latex => (" \\emph{", "}", " -- "),
    };
    let mut out = String::from(result);
    if s.show_moves {
        let _ = write!(out, "({})", s.moves);
    }
    let _ = write!(out, " {pre_name}{}{post_name}", esc(fmt, &s.white));
    if s.white_elo > 0 {
        let _ = write!(out, "{pre_elo}{}{post_elo}", s.white_elo);
    }
    let _ = write!(out, "{sep}{pre_name}{}{post_name}", esc(fmt, &s.black));
    if s.black_elo > 0 {
        let _ = write!(out, "{pre_elo}{}{post_elo}", s.black_elo);
    }
    let when = if full_date { &s.date } else { &s.year };
    let _ = write!(out, ", {} {}", esc(fmt, &s.site), esc(fmt, when));
    out
}

fn games(fmt: Fmt, items: &[GameItem], out: &mut String) {
    match fmt {
        Fmt::Text => {
            for g in items {
                let note = if g.note > 0 {
                    format!(" [{}]", g.note)
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    " {:>2}:  {}{note}",
                    g.rank,
                    summary(fmt, &g.summary, g.full_date)
                );
            }
        }
        Fmt::Html => {
            out.push_str("<p>\n");
            for g in items {
                let note = if g.note > 0 {
                    format!(" [<a href=\"#note{n}\">{n}</a>]", n = g.note)
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    "{}:  {}{note}<br>",
                    g.rank,
                    summary(fmt, &g.summary, g.full_date)
                );
            }
            out.push_str("</p>\n");
        }
        Fmt::Latex => {
            out.push_str("\\begin{tabularx}{0.95\\textwidth}{rX}\n");
            for g in items {
                let note = if g.note > 0 {
                    format!(" $^{{{}}}$", g.note)
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    "\\textbf{{{}:}} & {}{note} \\\\",
                    g.rank,
                    summary(fmt, &g.summary, g.full_date)
                );
            }
            out.push_str("\\end{tabularx}\n");
        }
    }
}

fn cell_text(fmt: Fmt, c: &super::TheoryCell) -> String {
    let mut text = esc(fmt, &c.text);
    if c.note > 0 {
        match fmt {
            Fmt::Text => {
                let _ = write!(text, "[{}]", c.note);
            }
            Fmt::Html => {
                let _ = write!(text, "<sup><a href=\"#note{n}\">{n}</a></sup>", n = c.note);
            }
            Fmt::Latex => {
                let _ = write!(text, "$^{{{}}}$", c.note);
            }
        }
    }
    text
}

fn theory(fmt: Fmt, t: &TheoryBlock, labels: &ReportLabels, out: &mut String) {
    let heading = if t.stem.is_empty() {
        t.heading.clone()
    } else {
        format!("{}: {}", t.stem, t.heading)
    };
    match fmt {
        Fmt::Text => {
            let _ = writeln!(out, "{HRULE}\n {heading}\n{HRULE}");
            out.push_str("  ");
            for i in 0..t.rows.first().map_or(8, |r| r.cells.len() / 2) {
                let _ = write!(out, " {:>3}     ", t.first_number + i);
            }
            let _ = writeln!(out, "\n{HRULE}");
            for row in &t.rows {
                text_row(row, out);
            }
            let _ = writeln!(out, "{HRULE}");
            if !t.comment.is_empty() {
                let _ = writeln!(out, "{}", t.comment);
            }
            out.push('\n');
        }
        Fmt::Html => {
            let _ = writeln!(out, "<center>{}", esc(fmt, &heading));
            out.push_str("<table border=0 cellspacing=0 cellpadding=4>\n<tr><th></th>");
            for i in 0..t.rows.first().map_or(8, |r| r.cells.len() / 2) {
                let _ = write!(out, "<th align=\"left\">{}</th>", t.first_number + i);
            }
            out.push_str("</tr>\n");
            for row in &t.rows {
                let _ = writeln!(out, "<tr><th>{}</th>", row.number);
                for pair in row.cells.chunks(2) {
                    let joined: Vec<String> = pair.iter().map(|c| cell_text(fmt, c)).collect();
                    let _ = writeln!(out, " <td> {} </td>", joined.join(" <br> "));
                }
                let _ = writeln!(out, " <td>{}: {}% </td> </tr>", row.count, row.score);
            }
            out.push_str("</table>\n");
            if !t.comment.is_empty() {
                let _ = writeln!(out, "{}", esc(fmt, &t.comment));
            }
            out.push_str("</center>\n");
        }
        Fmt::Latex => {
            let _ = labels;
            out.push_str("\\renewcommand{\\arraystretch}{1.15}\n\\begin{center}\n");
            out.push_str("\\begin{tabularx}{0.97\\textwidth}{r*{8}{X}r@{: }l}\n\\hline\n");
            let _ = writeln!(
                out,
                "\\multicolumn{{11}}{{p{{13cm}}}}{{\\textbf{{{}}}: \\mbox{{{}}}}} \\\\\n\\hline",
                esc(fmt, &t.stem),
                esc(fmt, &t.heading)
            );
            for i in 0..8 {
                let _ = write!(out, " & {}", t.first_number + i);
            }
            out.push_str(" & \\multicolumn{2}{c}{} \\\\\n");
            for row in &t.rows {
                let _ = write!(out, "\\textbf{{{}}}", row.number);
                let mut prev_empty = true;
                for (j, c) in row.cells.iter().enumerate() {
                    if j % 2 == 0 {
                        out.push_str(" & ");
                    } else if !prev_empty {
                        out.push_str(" \\newline ");
                    }
                    prev_empty = c.text.is_empty();
                    out.push_str(&cell_text(fmt, c));
                }
                let _ = writeln!(out, " & {} & {}\\% \\\\", row.count, row.score);
            }
            out.push_str("\\hline\n");
            if !t.comment.is_empty() {
                let _ = writeln!(
                    out,
                    "\\multicolumn{{11}}{{r}}{{\\em {}}}",
                    esc(fmt, &t.comment)
                );
            }
            out.push_str("\\end{tabularx}\n\\end{center}\n");
        }
    }
}

fn text_row(row: &TheoryRow, out: &mut String) {
    let mut white = format!("{:>2}  ", row.number);
    let mut black = "    ".to_owned();
    for (j, c) in row.cells.iter().enumerate() {
        let text = cell_text(Fmt::Text, c);
        let cell = pad_left(&text, CELL_WIDTH);
        if j % 2 == 0 {
            white.push_str(&cell);
        } else {
            black.push_str(&cell);
        }
    }
    let _ = write!(white, "{:>2}:", row.count);
    let _ = write!(black, "{:>2}%", row.score);
    let _ = write!(out, "{white}\n{black}\n\n");
}

fn write_notes(fmt: Fmt, notes: &[NoteBlock], out: &mut String) {
    match fmt {
        Fmt::Html => out.push_str("\n\n<ol>\n"),
        Fmt::Text | Fmt::Latex => out.push('\n'),
    }
    for note in notes {
        match fmt {
            Fmt::Text => {
                let _ = write!(out, "[*{}*]  ", note.number);
            }
            Fmt::Html => {
                let _ = write!(out, "<li><a name=\"note{}\"></a> ", note.number);
            }
            Fmt::Latex => {
                let _ = write!(out, "$^{{{}}}$", note.number);
            }
        }
        for e in &note.entries {
            match (e.join, fmt) {
                (Join::First, _) => {}
                (Join::Paragraph, Fmt::Text) => out.push_str(".\n     "),
                (Join::Paragraph, Fmt::Html) => out.push_str(".<br>\n"),
                (Join::Paragraph, Fmt::Latex) => out.push_str(".\n\n"),
                (Join::Next, Fmt::Text) => out.push_str(";\n        "),
                (Join::Next, _) => out.push_str(";\n"),
            }
            let (pre, post) = match fmt {
                Fmt::Text => ("", ""),
                Fmt::Html => ("<b>", "</b>"),
                Fmt::Latex => ("\\textbf{", "}"),
            };
            if !e.first_move.is_empty() {
                let _ = write!(
                    out,
                    "{pre}{}{post}{}",
                    esc(fmt, &e.first_move),
                    esc(fmt, &e.other_moves)
                );
            }
            out.push(' ');
            out.push_str(&summary(fmt, &e.summary, false));
        }
        match fmt {
            Fmt::Text | Fmt::Latex => out.push_str(".\n\n"),
            Fmt::Html => out.push_str(".</li>\n"),
        }
    }
    if fmt == Fmt::Html {
        out.push_str("</ol>\n");
    }
}
