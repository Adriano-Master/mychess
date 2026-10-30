// Derived from Scid vs PC 4.27, src/crosstab.cpp (`PrintTable`, `PrintPlayer`,
// `PrintPerformance`, `PrintAllPlayAll`, `PrintSwiss`, `PrintKnockout`,
// `PrintKnockoutLaTeX`) and src/tkscid.cpp (`sc_game_crosstable`, the header).
// Copyright (c) 2000-2004 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Text, HTML and LaTeX output (spec 2.1 "Saída", rule T30, decision D3363).
//!
//! The three outputs share one printer, as in the Scid, driven by a [`Style`]
//! with the fragments each format puts around rows and cells. The plain
//! text is the Scid's byte for byte (for names in ASCII); HTML and LaTeX are
//! complete documents (see the module `html` and `latex`).

mod html;
mod latex;
mod text;

use std::fmt::Write as _;

use mychess_core::Color;

use super::{Clash, Crosstable, Notice, PlayerRow, TableMode};

pub use html::render_html;
pub use latex::render_latex;
pub use text::render_text;

/// The words of the exported files. The default is the English of the Scid
/// (D3364); the interface may pass translations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableLabels {
    /// Header of the name column (`Player`).
    pub player: String,
    /// Header of the Elo column (`Rtng`).
    pub rating: String,
    /// Header of the title column (`Ti`).
    pub title: String,
    /// Header of the age column (`Age`).
    pub age: String,
    /// Header of the country column (`Nat`).
    pub nation: String,
    /// Header of the score column (`Score`).
    pub score: String,
    /// Header of the tie-break column (`Tie`, printed in parentheses).
    pub tie: String,
    /// Header of the performance column (`Perf`).
    pub performance: String,
    /// Header of the rating change column (`Chg`).
    pub change: String,
    /// Header of the tallies column in HTML (`+/-/=`).
    pub tallies: String,
    /// `Average Rating`.
    pub average_rating: String,
    /// `Category`.
    pub category: String,
    /// The word in `3 games: +2 -0 =1`.
    pub games: String,
}

impl Default for CrosstableLabels {
    fn default() -> CrosstableLabels {
        let s = String::from;
        CrosstableLabels {
            player: s("Player"),
            rating: s("Rtng"),
            title: s("Ti"),
            age: s("Age"),
            nation: s("Nat"),
            score: s("Score"),
            tie: s("Tie"),
            performance: s("Perf"),
            change: s("Chg"),
            tallies: s("+/-/="),
            average_rating: s("Average Rating"),
            category: s("Category"),
            games: s("games"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Html,
    Latex,
}

/// The fragments of one output format (the `StartTable`... of the Scid).
struct Style {
    kind: Kind,
    start_table: &'static str,
    end_table: &'static str,
    start_row: &'static str,
    end_row: &'static str,
    newline: &'static str,
    /// The line break of the heading lines (event, site, dates).
    heading_break: &'static str,
    blank_row_line: &'static str,
    start_col: &'static str,
    end_col: &'static str,
    start_right: &'static str,
    end_right: &'static str,
    start_bold: &'static str,
    end_bold: &'static str,
}

fn digit(n: usize) -> char {
    char::from_digit(u32::try_from(n % 10).unwrap_or(0), 10).unwrap_or('0')
}

/// `name` padded on the right to `width` characters (`%-*s`).
fn pad(name: &str, width: usize) -> String {
    let len = name.chars().count();
    let mut s = name.to_owned();
    s.extend(std::iter::repeat_n(' ', width.saturating_sub(len)));
    s
}

/// `name` cut or padded to exactly `width` characters (`strPad`).
fn fit(name: &str, width: usize) -> String {
    let cut: String = name.chars().take(width).collect();
    pad(&cut, width)
}

struct Printer<'a> {
    table: &'a Crosstable,
    labels: &'a CrosstableLabels,
    style: &'static Style,
    out: String,
}

/// Prints everything below the heading: the table and the line of totals.
fn table_body(table: &Crosstable, labels: &CrosstableLabels, style: &'static Style) -> String {
    let mut p = Printer {
        table,
        labels,
        style,
        out: String::new(),
    };
    match table.mode {
        TableMode::AllPlayAll => p.all_play_all(),
        TableMode::Swiss => p.swiss(),
        TableMode::Knockout => {
            if style.kind == Kind::Latex {
                p.knockout_latex();
            } else {
                p.knockout();
            }
        }
    }
    p.totals();
    p.out
}

/// The notices, the event, the site, the dates and the average rating.
fn heading(table: &Crosstable, labels: &CrosstableLabels, style: &'static Style) -> String {
    let p = Printer {
        table,
        labels,
        style,
        out: String::new(),
    };
    let mut out = String::new();
    for notice in &table.notices {
        out.push_str(&p.notice(*notice));
    }
    let _ = write!(
        out,
        "{}{}{}, {}",
        p.esc(&table.event),
        style.heading_break,
        p.esc(&table.site),
        table.first_date.short()
    );
    if table.last_date != table.first_date {
        let _ = write!(out, " - {}", table.last_date.short());
    }
    out.push_str(style.heading_break);
    if table.average_elo > 0 && table.options.show_ratings {
        let _ = write!(
            out,
            "{}: {}",
            p.esc(&labels.average_rating),
            table.average_elo
        );
        if table.category > 0 && table.mode == TableMode::AllPlayAll {
            let _ = write!(out, "  ({} {})", p.esc(&labels.category), table.category);
        }
        out.push_str(if style.kind == Kind::Latex {
            "\n\n"
        } else {
            style.heading_break
        });
    }
    out
}

impl Printer<'_> {
    fn esc(&self, text: &str) -> String {
        match self.style.kind {
            Kind::Plain => text.to_owned(),
            Kind::Html => html::escape(text),
            Kind::Latex => latex::escape(text),
        }
    }

    fn notice(&self, notice: Notice) -> String {
        let text = self.esc(&notice.english());
        match self.style.kind {
            Kind::Plain => format!("{text}\n\n"),
            Kind::Html => format!("<p><strong>{text}</strong></p>\n"),
            Kind::Latex => format!("\\textbf{{{text}}}\n\n"),
        }
    }

    fn is(&self, kind: Kind) -> bool {
        self.style.kind == kind
    }

    fn dp(&self) -> char {
        self.table.options.decimal_point
    }

    /// `%2u%c%c`: points in half points as `1.5`.
    fn points(&self, half: u32) -> String {
        format!(
            "{:2}{}{}",
            half / 2,
            self.dp(),
            if half & 1 == 1 { '5' } else { '0' }
        )
    }

    fn dashes(&mut self) {
        if self.is(Kind::Plain) {
            self.out
                .extend(std::iter::repeat_n('-', self.table.line_width));
            self.out.push_str(self.style.newline);
        }
    }

    fn push_cell(&mut self, start: &str, text: &str, end: &str) {
        self.out.push_str(start);
        self.out.push_str(text);
        self.out.push_str(end);
    }

    /// `PrintPlayer`: name, Elo, title, age and country cells.
    fn player(&mut self, row: &PlayerRow) {
        let s = self.style;
        let cols = self.table.columns;
        let name = self.esc(&pad(&row.name, self.table.longest_name));
        self.push_cell(s.start_col, &format!("{name} "), s.end_col);
        if self.is(Kind::Latex) {
            self.out.push(' ');
        }
        if cols.ratings {
            let text = if row.elo > 0 {
                format!("{:4} ", row.elo)
            } else if self.is(Kind::Html) {
                "  -  ".to_owned()
            } else {
                "     ".to_owned()
            };
            self.push_cell(s.start_right, &text, s.end_right);
        }
        if cols.titles {
            let text = if self.is(Kind::Html) && row.title.is_empty() {
                " -  ".to_owned()
            } else {
                format!("{:>3} ", self.esc(&row.title))
            };
            self.push_cell(s.start_col, &text, s.end_col);
        }
        if cols.ages {
            let text = match row.age {
                Some(age) => format!("{age:3} "),
                None if self.is(Kind::Html) => " -  ".to_owned(),
                None => "    ".to_owned(),
            };
            self.push_cell(s.start_col, &text, s.end_col);
        }
        if cols.countries {
            let text = if self.is(Kind::Html) && row.country.is_empty() {
                " -  ".to_owned()
            } else {
                format!("{:<3} ", self.esc(&row.country))
            };
            self.push_cell(s.start_col, &text, s.end_col);
        }
    }

    /// `PrintPerformance`.
    fn performance(&mut self, row: &PlayerRow) {
        let s = self.style;
        if !self.table.columns.ratings {
            return;
        }
        if row.rated_games == 0 {
            if self.table.columns.tallies {
                let blank = if self.is(Kind::Html) {
                    "     -     "
                } else {
                    "           "
                };
                self.push_cell(s.start_right, blank, s.end_right);
            }
            return;
        }
        let latex = self.is(Kind::Latex);
        let Some(perf) = row.performance else {
            // No figure to show: the empty Perf cell keeps the tallies in
            // the column of Chg.
            if latex && self.table.columns.tallies {
                self.push_cell(s.start_right, "           ", s.end_right);
            }
            return;
        };
        let text = match perf.change {
            Some(change) if latex => format!("{:4} & {change:+3}", perf.rating),
            Some(change) => format!("{:4} {change:+3}", perf.rating),
            None => format!("{:4}    ", perf.rating),
        };
        self.out.push_str("   ");
        self.out.push_str(s.start_right);
        self.out.push_str(&text);
        if !latex {
            self.out.push_str(s.end_right);
        } else if perf.change.is_none() && self.table.columns.tallies {
            // No Chg cell was written: close Perf so the tallies stay in the
            // Chg column (D3363).
            self.out.push_str(s.end_right);
        }
    }

    /// The tallies cell. HTML closes it (D3363); LaTeX has the column of the
    /// last `r` and no trailing `&`.
    fn tallies(&mut self, row: &PlayerRow) {
        if !self.table.columns.tallies {
            return;
        }
        let end = if self.is(Kind::Html) {
            self.style.end_col
        } else {
            ""
        };
        let text = format!("  (+{} -{} ={})", row.won, row.lost, row.drawn);
        self.push_cell(self.style.start_col, &text, end);
    }

    /// The `Score` cell (and, in LaTeX, the games cell).
    fn score_cells(&mut self, row: &PlayerRow) {
        let s = self.style;
        let points = self.points(row.score_half);
        if self.is(Kind::Latex) {
            self.push_cell(s.start_right, &format!(" {points} "), s.end_right);
            self.push_cell(s.start_right, &format!(" {:2}  ", row.games), s.end_right);
        } else if self.table.options.three_points {
            self.push_cell(s.start_right, &format!(" {points}  "), s.end_right);
        } else {
            let text = format!(" {points} / {:2}  ", row.games);
            self.push_cell(s.start_right, &text, s.end_right);
        }
    }

    /// The heading of the `Score` column and what follows it.
    fn score_header(&mut self) {
        let s = self.style;
        let word = self.esc(&self.labels.score);
        if self.is(Kind::Latex) {
            let _ = write!(self.out, " \\multicolumn{{2}}{{c}}{{\\bfseries {word}}} & ");
        } else if self.table.options.three_points {
            let _ = write!(self.out, "  {}{word}{} ", s.start_bold, s.end_bold);
        } else {
            let _ = write!(self.out, "   {} {word} {}   ", s.start_bold, s.end_bold);
        }
    }

    /// The first heading cells: blank/`Player`, `Rtng`, `Ti`, `Age`, `Nat`.
    fn leading_header(&mut self) {
        let s = self.style;
        let t = self.table;
        match s.kind {
            Kind::Html => {
                let _ = write!(
                    self.out,
                    "<th></th> <th>{}</th> ",
                    self.esc(&self.labels.player)
                );
            }
            Kind::Latex => {
                let _ = write!(
                    self.out,
                    "  & \\bfseries {} & ",
                    self.esc(&self.labels.player)
                );
            }
            Kind::Plain => {
                self.out.extend(std::iter::repeat_n(
                    ' ',
                    t.longest_name + 2 + t.number_width,
                ));
            }
        }
        let cols = t.columns;
        let l = self.labels;
        for (shown, text) in [
            (cols.ratings, format!(" {}", l.rating)),
            (cols.titles, format!("  {}", l.title)),
            (cols.ages, format!(" {}", l.age)),
            (cols.countries, format!(" {}", l.nation)),
        ] {
            if shown {
                let text = self.esc(&text);
                self.push_cell(s.start_bold, &text, s.end_bold);
            }
        }
    }

    fn perf_header(&mut self) {
        let s = self.style;
        let t = self.table;
        if t.columns.ratings {
            let (perf, chg) = (
                self.esc(&self.labels.performance),
                self.esc(&self.labels.change),
            );
            if self.is(Kind::Latex) {
                let _ = write!(self.out, "   \\bfseries {perf} & \\bfseries {chg}");
            } else {
                let _ = write!(self.out, "   {}{perf} {chg}{}", s.start_bold, s.end_bold);
            }
        }
        if t.columns.tallies && self.is(Kind::Html) {
            let _ = write!(
                self.out,
                "   {}{}{}",
                s.start_bold,
                self.esc(&self.labels.tallies),
                s.end_bold
            );
        }
    }

    fn blank_between_groups(&mut self, position: usize, previous: &mut u32, score: u32) {
        let o = &self.table.options;
        if o.separate_score_groups
            && o.sort == super::CrosstableSort::Score
            && position > 0
            && score != *previous
        {
            self.out.push_str(self.style.blank_row_line);
        }
        *previous = score;
    }

    fn row_start(&mut self, position: usize) {
        let s = self.style;
        let text = format!("{:>w$}: ", position + 1, w = self.table.number_width);
        self.out.push_str(s.start_row);
        self.push_cell(s.start_right, &text, s.end_right);
        if self.is(Kind::Latex) && self.table.mode == TableMode::AllPlayAll {
            self.out.push(' ');
        }
    }

    /// `PrintAllPlayAll`.
    #[allow(clippy::too_many_lines)] // one function per Scid printer
    fn all_play_all(&mut self) {
        let s = self.style;
        let t = self.table;
        let limit = t.rows.len();
        let mc = t.max_clashes as usize;
        self.out.push_str(s.start_table);
        if self.is(Kind::Latex) {
            let cols = t.columns;
            self.out.push_str("\\begin{longtable}{rl");
            for (on, c) in [
                (cols.ratings, "c"),
                (cols.titles, "r"),
                (cols.ages, "r"),
                (cols.countries, "l"),
            ] {
                if on {
                    self.out.push_str(c);
                }
            }
            self.out.push_str("r@{/}r");
            if cols.tiebreaks {
                self.out.push('r');
            }
            // A two-player table has a single result column.
            let columns = if limit == 2 { 1 } else { limit };
            for i in 0..columns {
                self.out.push('c');
                if i + 1 < columns {
                    self.out.push_str("@{ }");
                }
            }
            if cols.ratings {
                self.out.push('r');
            }
            self.out.push_str("r}\n");
        }
        self.out.push_str(s.start_row);
        if self.is(Kind::Latex) {
            self.out.push_str("\\toprule\n");
        }
        self.leading_header();
        self.score_header();
        if t.columns.tiebreaks {
            let text = format!(" ({}) ", self.esc(&self.labels.tie));
            self.push_cell(s.start_bold, &text, s.end_bold);
        }
        for (player, row) in t.rows.iter().enumerate() {
            let mut name = fit(&row.name, mc);
            if t.options.numbered_columns {
                name = numbered(player + 1, mc);
            }
            if limit == 2 {
                name = fit("12345678901234567890123456789012345678901234567890", mc);
            }
            let name = self.esc(&name);
            if self.is(Kind::Latex) {
                let _ = write!(self.out, " \\ttfamily {name} &");
            } else {
                let _ = write!(self.out, " {}{name}{}", s.start_bold, s.end_bold);
            }
            if limit == 2 {
                break;
            }
        }
        self.perf_header();
        self.out.push_str(s.end_row);
        self.out.push_str(s.newline);
        if self.is(Kind::Latex) {
            self.out.push_str("\\midrule\n\\endhead\n");
        } else {
            self.dashes();
        }

        let mut previous = 0;
        for (player, row) in t.rows.iter().enumerate() {
            self.blank_between_groups(player, &mut previous, row.score_half);
            self.row_start(player);
            self.player(row);
            self.score_cells(row);
            if t.columns.tiebreaks {
                let text = format!(
                    "{:3}{}{:02} ",
                    row.tiebreak / 4,
                    self.dp(),
                    (row.tiebreak % 4) * 25
                );
                self.push_cell(s.start_right, &text, s.end_right);
            }
            let mut bullet = false;
            for column in 0..limit {
                if limit == 2 && column == player {
                    continue;
                }
                self.out.push(' ');
                self.out.push_str(s.start_right);
                let clashes: &[Clash] = row.versus.get(column).map_or(&[], Vec::as_slice);
                for count in 0..mc {
                    match clashes.get(count) {
                        Some(c) if c.outcome.result_char() == '=' => self.out.push('½'),
                        Some(c) => self.out.push(c.outcome.result_char()),
                        None if self.is(Kind::Latex) => {
                            if column == player && !bullet {
                                self.out.push_str("$\\bullet$");
                                bullet = true;
                            }
                        }
                        None => self.out.push('.'),
                    }
                }
                self.out.push_str(s.end_col);
            }
            self.performance(row);
            self.tallies(row);
            self.out.push_str(s.end_row);
            self.out.push_str(s.newline);
        }
        if self.is(Kind::Latex) {
            self.out.push_str("\\bottomrule\n");
        } else {
            self.dashes();
        }
        self.out.push_str(s.end_table);
    }

    /// `PrintSwiss`.
    #[allow(clippy::too_many_lines)] // one function per Scid printer
    fn swiss(&mut self) {
        let s = self.style;
        let t = self.table;
        let colors = t.options.swiss_colors;
        let width = t.number_width;
        self.out.push_str(s.start_table);
        if self.is(Kind::Latex) {
            let cols = t.columns;
            self.out.push_str("\\begin{longtable}{rl");
            for (on, c) in [
                (cols.ratings, "r"),
                (cols.titles, "r"),
                (cols.ages, "r"),
                (cols.countries, "l"),
            ] {
                if on {
                    self.out.push_str(c);
                }
            }
            self.out.push_str("r@{ / }r");
            if cols.tiebreaks {
                self.out.push('r');
            }
            for i in 0..t.max_round {
                self.out.push('r');
                if i + 1 < t.max_round {
                    self.out.push_str("@{ }");
                }
            }
            if cols.ratings {
                self.out.push('r');
            }
            self.out.push_str("r}\n");
        }
        self.out.push_str(s.start_row);
        if self.is(Kind::Latex) {
            self.out.push_str("\\toprule\n");
        }
        self.leading_header();
        self.score_header();
        if t.columns.tiebreaks {
            let text = format!("({})", self.esc(&self.labels.tie));
            self.push_cell(s.start_bold, &text, s.end_bold);
        }
        for round in 1..=t.max_round {
            if self.is(Kind::Latex) {
                let _ = write!(
                    self.out,
                    " \\multicolumn{{1}}{{c}}{{\\bfseries {round}}} & "
                );
            } else {
                let text = format!(" {}{round:>width$} ", if colors { " " } else { "" });
                self.push_cell(s.start_bold, &text, s.end_bold);
            }
        }
        self.perf_header();
        self.out.push_str(s.end_row);
        self.out.push_str(s.newline);
        if self.is(Kind::Latex) {
            self.out.push_str("\\midrule\n\\endhead\n");
        } else {
            self.dashes();
        }

        let mut previous = 0;
        for (player, row) in t.rows.iter().enumerate() {
            self.blank_between_groups(player, &mut previous, row.score_half);
            self.row_start(player);
            self.player(row);
            self.score_cells(row);
            if t.columns.tiebreaks {
                let text = format!(
                    "{:2}{}{} ",
                    row.tiebreak / 2,
                    self.dp(),
                    if row.tiebreak & 1 == 1 { '5' } else { '0' }
                );
                self.push_cell(s.start_right, &text, s.end_right);
            }
            for cell in &row.by_round {
                self.out.push(' ');
                self.out.push_str(s.start_right);
                match cell {
                    None => {
                        self.out.push_str("...");
                        if width == 3 {
                            self.out.push('.');
                        }
                        if colors {
                            self.out.push('.');
                        }
                    }
                    Some(c) => {
                        let number = c.opponent + 1;
                        let mark = c.outcome.swiss_char();
                        if colors {
                            let color = if c.color == Color::White { 'w' } else { 'b' };
                            let _ = write!(self.out, "{number:>width$}{color}{mark}");
                        } else {
                            let _ = write!(self.out, "{number:>width$}{mark}");
                        }
                    }
                }
                self.out.push_str(s.end_col);
            }
            self.performance(row);
            self.tallies(row);
            self.out.push_str(s.end_row);
            self.out.push_str(s.newline);
        }
        if self.is(Kind::Latex) {
            self.out.push_str("\\bottomrule\n");
        } else {
            self.dashes();
        }
        self.out.push_str(s.end_table);
    }

    /// The results of a knockout match: one character per game, padded.
    fn knockout_results(&mut self, games: &[Clash]) {
        for count in 0..self.table.max_clashes as usize {
            match games.get(count) {
                Some(c) => self.out.push(c.outcome.result_char()),
                None => self.out.push(' '),
            }
        }
    }

    /// `PrintKnockout` for text and HTML.
    fn knockout(&mut self) {
        let s = self.style;
        let t = self.table;
        for block in &t.knockout {
            let _ = write!(self.out, "{}:{}", block.round, s.newline);
            self.out.push_str(s.start_table);
            self.dashes();
            for m in &block.matches {
                let (Some(first), Some(second)) = (t.rows.get(m.first), t.rows.get(m.second))
                else {
                    continue;
                };
                self.out.push_str(s.start_row);
                self.player(first);
                self.out.push_str(s.start_col);
                self.out.push_str("  ");
                self.knockout_results(&m.games);
                self.out.push(' ');
                self.out.push_str(s.end_col);
                let text = format!("{} / {:2}   ", self.points(m.score_half), m.games.len());
                self.push_cell(s.start_right, &text, s.end_right);
                self.player(second);
                self.out.push_str(s.end_row);
                self.out.push_str(s.newline);
            }
            self.dashes();
            self.out.push_str(s.end_table);
            self.out.push_str(s.newline);
        }
    }

    /// `PrintKnockoutLaTeX`.
    fn knockout_latex(&mut self) {
        let s = self.style;
        let t = self.table;
        let cols = t.columns;
        self.out.push_str("\\begin{longtable}[l]{@{}l");
        let extras = [
            (cols.ratings, "r"),
            (cols.titles, "r"),
            (cols.ages, "r"),
            (cols.countries, "l"),
        ];
        for (on, c) in extras {
            if on {
                self.out.push_str(c);
            }
        }
        self.out.push_str("lr@{/}rl");
        for (on, c) in extras {
            if on {
                self.out.push_str(c);
            }
        }
        self.out.push_str("r}\n");
        for block in &t.knockout {
            if block.round == 1 {
                let _ = writeln!(self.out, "{}:\\\\", block.round);
            } else {
                let _ = write!(self.out, "\\\\\n{}:\\\\\n", block.round);
            }
            for m in &block.matches {
                let (Some(first), Some(second)) = (t.rows.get(m.first), t.rows.get(m.second))
                else {
                    continue;
                };
                self.out.push_str(s.start_row);
                self.player(first);
                self.out.push_str(s.start_col);
                self.out.push(' ');
                self.knockout_results(&m.games);
                self.out.push(' ');
                self.out.push_str(s.end_col);
                let points = format!(" {} ", self.points(m.score_half));
                self.push_cell(s.start_right, &points, s.end_right);
                let games = format!(" {:2}  ", m.games.len());
                self.push_cell(s.start_right, &games, s.end_right);
                self.player(second);
                self.out.push_str(s.end_row);
                self.out.push_str(s.newline);
            }
        }
        self.out.push_str(s.end_table);
        self.out.push_str(s.newline);
    }

    /// The line `3 games: +2 -0 =1`, with `*K` when some have no result.
    fn totals(&mut self) {
        let t = self.table;
        if t.game_count > 1 {
            let games = self.esc(&self.labels.games);
            let _ = write!(
                self.out,
                "{} {games}: +{} -{} ={}",
                t.game_count, t.results.white, t.results.black, t.results.draws
            );
            if t.results.unknown > 0 {
                let _ = write!(self.out, " *{}", t.results.unknown);
            }
            self.out.push_str(self.style.newline);
        }
    }
}

/// The header of a numbered column: the number right-aligned in `width`.
fn numbered(number: usize, width: usize) -> String {
    let mut chars = vec![' '; width];
    let place = |chars: &mut Vec<char>, back: usize, value: usize| {
        if let Some(slot) = width.checked_sub(back).and_then(|i| chars.get_mut(i)) {
            *slot = digit(value);
        }
    };
    place(&mut chars, 1, number);
    if width >= 2 && number >= 10 {
        place(&mut chars, 2, number / 10);
    }
    if width >= 3 && number >= 100 {
        place(&mut chars, 3, number / 100);
    }
    chars.into_iter().collect()
}

/// Exposes [`numbered`] to the unit tests.
#[cfg(test)]
pub(super) fn render_numbered_for_tests(number: usize, width: usize) -> String {
    numbered(number, width)
}
