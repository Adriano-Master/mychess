//! Export of the table of standings and of the crosstable (spec M8-2,
//! D1287): CSV, monospaced text (as the Crosstable window of the Scid),
//! Markdown and HTML.
//!
//! Pure functions from the model to text. CSV uses UTF-8, `,`, a point as
//! decimal separator and `\n` line ends. The headers are English by default;
//! the front-end passes translated [`ExportLabels`].

use std::fmt::Write;

use super::rating::{RatingRow, Reliability, elo_text, los_text, margin_text};
use super::standings::{GameScore, StandingRow, points_text};
use super::timecheck::{TimeRow, TimeVerdict, duration_text};
use crate::files::sanitize_file_name;

/// The format of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Comma separated values.
    Csv,
    /// Monospaced text.
    Text,
    /// A Markdown table.
    Markdown,
    /// An HTML table.
    Html,
}

impl ExportFormat {
    /// The extension of the file.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Text => "txt",
            ExportFormat::Markdown => "md",
            ExportFormat::Html => "html",
        }
    }
}

/// Which table to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportTable {
    /// The table of standings.
    Standings,
    /// The crosstable.
    Cross,
    /// The table of Elo and LOS (D2513).
    Ratings,
    /// The report of time (D2516).
    Time,
}

/// The column headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportLabels {
    /// Position.
    pub rank: String,
    /// Participant.
    pub name: String,
    /// Games played.
    pub played: String,
    /// Wins.
    pub wins: String,
    /// Draws.
    pub draws: String,
    /// Losses.
    pub losses: String,
    /// Points.
    pub points: String,
    /// Percentage.
    pub percent: String,
    /// Sonneborn-Berger.
    pub sonneborn_berger: String,
    /// Marks a participant that left.
    pub withdrawn: String,
    /// Elo relative to the opponents.
    pub elo: String,
    /// Margin of error of the Elo (95%).
    pub margin: String,
    /// Likelihood of superiority, per cent.
    pub los: String,
    /// Rating performance.
    pub performance: String,
    /// How much to trust the estimate.
    pub sample: String,
    /// The labels of [`Reliability`], from `Empty` to `Fair`.
    pub reliability: [String; 4],
    /// Timed moves.
    pub moves: String,
    /// Mean response time.
    pub mean: String,
    /// Slowest move.
    pub slowest: String,
    /// Mean excess over the time of a move (overhead).
    pub overhead: String,
    /// Moves above the limit.
    pub overruns: String,
    /// Games lost on time.
    pub forfeits: String,
    /// Share of the clock used, per cent.
    pub clock_used: String,
    /// The verdict.
    pub verdict: String,
    /// The labels of [`TimeVerdict`], from `NoData` to `Forfeits`.
    pub verdicts: [String; 4],
}

impl Default for ExportLabels {
    fn default() -> ExportLabels {
        ExportLabels {
            rank: "Rank".to_owned(),
            name: "Name".to_owned(),
            played: "Played".to_owned(),
            wins: "Wins".to_owned(),
            draws: "Draws".to_owned(),
            losses: "Losses".to_owned(),
            points: "Points".to_owned(),
            percent: "%".to_owned(),
            sonneborn_berger: "SB".to_owned(),
            withdrawn: "withdrawn".to_owned(),
            elo: "Elo".to_owned(),
            margin: "Margin".to_owned(),
            los: "LOS".to_owned(),
            performance: "Performance".to_owned(),
            sample: "Sample".to_owned(),
            reliability: [
                "none".to_owned(),
                "very uncertain".to_owned(),
                "low".to_owned(),
                "fair".to_owned(),
            ],
            moves: "Moves".to_owned(),
            mean: "Mean".to_owned(),
            slowest: "Slowest".to_owned(),
            overhead: "Overhead".to_owned(),
            overruns: "Overruns".to_owned(),
            forfeits: "Forfeits".to_owned(),
            clock_used: "Clock used %".to_owned(),
            verdict: "Verdict".to_owned(),
            verdicts: [
                "no data".to_owned(),
                "respects".to_owned(),
                "overruns".to_owned(),
                "forfeits".to_owned(),
            ],
        }
    }
}

/// The crosstable as `cells[i][j]`.
pub type CrossCells = [Vec<Vec<(u32, GameScore)>>];

/// A table of text: a header row and the data rows.
struct Grid {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    /// Columns written flush left (the names); the others flush right.
    left: Vec<usize>,
    /// Columns that hold only numbers or `-` made by us (never a name): CSV
    /// does not guard them, so `-191` stays a number (D1313, D2513).
    numeric: Vec<usize>,
}

fn sonneborn_text(quarter_points: u64) -> String {
    format!("{}.{:02}", quarter_points / 4, quarter_points % 4 * 25)
}

fn name_of(names: &[String], i: usize) -> String {
    names.get(i).cloned().unwrap_or_default()
}

fn standings_grid(names: &[String], rows: &[StandingRow], labels: &ExportLabels) -> Grid {
    Grid {
        header: vec![
            labels.rank.clone(),
            labels.name.clone(),
            labels.played.clone(),
            labels.wins.clone(),
            labels.draws.clone(),
            labels.losses.clone(),
            labels.points.clone(),
            labels.percent.clone(),
            labels.sonneborn_berger.clone(),
        ],
        rows: rows
            .iter()
            .map(|r| {
                let mut name = name_of(names, r.participant);
                if r.withdrawn {
                    name = format!("{name} ({})", labels.withdrawn);
                }
                vec![
                    r.rank.to_string(),
                    name,
                    r.played.to_string(),
                    r.wins.to_string(),
                    r.draws.to_string(),
                    r.losses.to_string(),
                    r.points_text(),
                    format!("{:.1}", r.percent()),
                    sonneborn_text(r.sonneborn_berger),
                ]
            })
            .collect(),
        left: vec![1],
        numeric: Vec::new(),
    }
}

fn ratings_grid(names: &[String], rows: &[RatingRow], labels: &ExportLabels) -> Grid {
    Grid {
        header: vec![
            labels.rank.clone(),
            labels.name.clone(),
            labels.played.clone(),
            labels.wins.clone(),
            labels.draws.clone(),
            labels.losses.clone(),
            labels.percent.clone(),
            labels.elo.clone(),
            labels.margin.clone(),
            labels.los.clone(),
            labels.performance.clone(),
            labels.sample.clone(),
        ],
        rows: rows
            .iter()
            .map(|r| {
                let mut name = name_of(names, r.participant);
                if r.withdrawn {
                    name = format!("{name} ({})", labels.withdrawn);
                }
                let e = &r.estimate;
                let reliability = match e.reliability {
                    Reliability::Empty => 0,
                    Reliability::VeryUncertain => 1,
                    Reliability::Low => 2,
                    Reliability::Fair => 3,
                };
                vec![
                    r.rank.to_string(),
                    name,
                    e.record.games().to_string(),
                    e.record.wins.to_string(),
                    e.record.draws.to_string(),
                    e.record.losses.to_string(),
                    e.score
                        .map_or_else(|| "-".to_owned(), |s| format!("{:.1}", s * 100.0)),
                    elo_text(e.elo),
                    margin_text(e.margin()),
                    los_text(e.los),
                    r.performance
                        .map_or_else(|| "-".to_owned(), |p| format!("{p:.0}")),
                    labels
                        .reliability
                        .get(reliability)
                        .cloned()
                        .unwrap_or_default(),
                ]
            })
            .collect(),
        left: vec![1, 11],
        numeric: (0..=10).filter(|c| *c != 1).collect(),
    }
}

fn time_grid(names: &[String], rows: &[TimeRow], labels: &ExportLabels) -> Grid {
    Grid {
        header: vec![
            labels.name.clone(),
            labels.moves.clone(),
            labels.mean.clone(),
            labels.slowest.clone(),
            labels.overhead.clone(),
            labels.overruns.clone(),
            labels.forfeits.clone(),
            labels.clock_used.clone(),
            labels.verdict.clone(),
        ],
        rows: rows
            .iter()
            .map(|r| {
                let verdict = match r.verdict {
                    TimeVerdict::NoData => 0,
                    TimeVerdict::Respects => 1,
                    TimeVerdict::Overruns => 2,
                    TimeVerdict::Forfeits => 3,
                };
                vec![
                    name_of(names, r.participant),
                    r.stats.moves.to_string(),
                    duration_text(r.mean_ms),
                    duration_text((r.stats.moves > 0).then_some(r.stats.max_ms)),
                    duration_text(r.overhead_ms),
                    r.stats.overruns.to_string(),
                    r.stats.forfeits.to_string(),
                    r.clock_used_percent
                        .map_or_else(|| "-".to_owned(), |p| format!("{p:.1}")),
                    labels.verdicts.get(verdict).cloned().unwrap_or_default(),
                ]
            })
            .collect(),
        left: vec![0, 8],
        numeric: (1..=7).collect(),
    }
}

fn cross_grid(
    names: &[String],
    cells: &CrossCells,
    rows: &[StandingRow],
    labels: &ExportLabels,
) -> Grid {
    let mut header = vec![labels.rank.clone(), labels.name.clone()];
    header.extend((1..=names.len()).map(|i| i.to_string()));
    header.push(labels.points.clone());
    let data = rows
        .iter()
        .map(|r| {
            let mut line = vec![r.rank.to_string(), name_of(names, r.participant)];
            for j in 0..names.len() {
                let text = if j == r.participant {
                    "x".to_owned()
                } else {
                    cells
                        .get(r.participant)
                        .and_then(|row| row.get(j))
                        .map(|games| {
                            games
                                .iter()
                                .map(|(_, s)| s.symbol())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default()
                };
                line.push(text);
            }
            line.push(points_text(r.half_points));
            line
        })
        .collect();
    Grid {
        header,
        rows: data,
        left: vec![1],
        numeric: Vec::new(),
    }
}

/// A CSV field. A text that a spreadsheet would read as a formula (it
/// starts with `=`, `+`, `-`, `@`, a tab or a carriage return) gets a `'` in
/// front, as OWASP advises, before the quotes are applied (D1313). The names
/// of the participants are the only free text, and they come from engines.
fn csv_field(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    csv_quoted(&guarded)
}

/// The quotes of a CSV field, without the guard of [`csv_field`].
fn csv_quoted(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Escapes what Markdown (and the HTML it may contain) reads as markup in a
/// table cell: the backslash first, then `|`, `<`, `*`, `_`, `[` and the
/// backtick (D1313). Line breaks become spaces.
fn markdown_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '|' | '<' | '*' | '_' | '[' | '`' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

fn render(grid: &Grid, format: ExportFormat) -> String {
    match format {
        ExportFormat::Csv => {
            let line = |cells: &[String], plain: bool| {
                cells
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        if plain && grid.numeric.contains(&i) {
                            csv_quoted(c)
                        } else {
                            csv_field(c)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let mut out = line(&grid.header, false);
            out.push('\n');
            for row in &grid.rows {
                out.push_str(&line(row, true));
                out.push('\n');
            }
            out
        }
        ExportFormat::Text => render_text(grid),
        ExportFormat::Markdown => {
            let line = |cells: &[String]| {
                format!(
                    "| {} |\n",
                    cells
                        .iter()
                        .map(|c| markdown_escape(c))
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            };
            let mut out = line(&grid.header);
            let rule: Vec<String> = (0..grid.header.len())
                .map(|i| {
                    if grid.left.contains(&i) {
                        ":---".to_owned()
                    } else {
                        "---:".to_owned()
                    }
                })
                .collect();
            writeln!(out, "| {} |", rule.join(" | ")).ok();
            for row in &grid.rows {
                out.push_str(&line(row));
            }
            out
        }
        ExportFormat::Html => {
            let mut out = String::from("<table>\n<thead>\n<tr>");
            for cell in &grid.header {
                write!(out, "<th>{}</th>", html_escape(cell)).ok();
            }
            out.push_str("</tr>\n</thead>\n<tbody>\n");
            for row in &grid.rows {
                out.push_str("<tr>");
                for cell in row {
                    write!(out, "<td>{}</td>", html_escape(cell)).ok();
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</tbody>\n</table>\n");
            out
        }
    }
}

/// The table as monospaced text. Columns are aligned by counting characters
/// (`chars().count()`), which is exact for the Latin names engines have; wide
/// characters (CJK, emoji) and combining marks take another width on screen
/// and misalign their row. Control characters become spaces so a name cannot
/// break the table (D1316).
fn render_text(grid: &Grid) -> String {
    let flat = |text: &str| -> String {
        text.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let grid = &Grid {
        header: grid.header.iter().map(|c| flat(c)).collect(),
        rows: grid
            .rows
            .iter()
            .map(|row| row.iter().map(|c| flat(c)).collect())
            .collect(),
        left: grid.left.clone(),
        numeric: grid.numeric.clone(),
    };
    let columns = grid.header.len();
    let widths: Vec<usize> = (0..columns)
        .map(|i| {
            std::iter::once(&grid.header)
                .chain(grid.rows.iter())
                .map(|row| row.get(i).map_or(0, |c| c.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: &[String]| {
        let mut out = String::new();
        for (i, width) in widths.iter().enumerate() {
            let cell = cells.get(i).map_or("", String::as_str);
            let pad = width.saturating_sub(cell.chars().count());
            if i > 0 {
                out.push(' ');
            }
            if grid.left.contains(&i) {
                out.push_str(cell);
                out.push_str(&" ".repeat(pad));
            } else {
                out.push_str(&" ".repeat(pad));
                out.push_str(cell);
            }
        }
        out.trim_end().to_owned()
    };
    let mut out = line(&grid.header);
    out.push('\n');
    let rule: usize = widths.iter().sum::<usize>() + widths.len().saturating_sub(1);
    out.push_str(&"-".repeat(rule));
    out.push('\n');
    for row in &grid.rows {
        out.push_str(&line(row));
        out.push('\n');
    }
    out
}

/// The table of standings as text.
pub fn export_standings(
    names: &[String],
    rows: &[StandingRow],
    labels: &ExportLabels,
    format: ExportFormat,
) -> String {
    render(&standings_grid(names, rows, labels), format)
}

/// The crosstable as text, in the order of `rows`.
pub fn export_cross(
    names: &[String],
    cells: &CrossCells,
    rows: &[StandingRow],
    labels: &ExportLabels,
    format: ExportFormat,
) -> String {
    render(&cross_grid(names, cells, rows, labels), format)
}

/// The table of Elo and LOS as text (D2513).
pub fn export_ratings(
    names: &[String],
    rows: &[RatingRow],
    labels: &ExportLabels,
    format: ExportFormat,
) -> String {
    render(&ratings_grid(names, rows, labels), format)
}

/// The report of time as text (D2516).
pub fn export_time(
    names: &[String],
    rows: &[TimeRow],
    labels: &ExportLabels,
    format: ExportFormat,
) -> String {
    render(&time_grid(names, rows, labels), format)
}

/// A safe file name for an export (D1287): the name of the tournament made
/// safe for the three platforms, the table and the extension.
pub fn export_file_name(slug: &str, table: ExportTable, format: ExportFormat) -> String {
    let kind = match table {
        ExportTable::Standings => "standings",
        ExportTable::Cross => "crosstable",
        ExportTable::Ratings => "ratings",
        ExportTable::Time => "time",
    };
    format!("{}-{kind}.{}", sanitize_file_name(slug), format.extension())
}

#[cfg(test)]
mod tests {
    use mychess_core::GameResult;

    use super::*;
    use crate::tournament::standings::{Finished, cross_table, standings};

    type Sample = (
        Vec<String>,
        Vec<StandingRow>,
        Vec<Vec<Vec<(u32, GameScore)>>>,
    );

    fn sample() -> Sample {
        let games = [
            Finished {
                number: 1,
                white: 0,
                black: 1,
                result: GameResult::White,
            },
            Finished {
                number: 2,
                white: 1,
                black: 0,
                result: GameResult::Draw,
            },
        ];
        let names = vec!["Stock, fish".to_owned(), "Lc0 \"net\"".to_owned()];
        let rows = standings(2, &[], &games);
        let cells = cross_table(2, &games);
        (names, rows, cells)
    }

    #[test]
    fn csv_has_a_point_decimal_quotes_and_unix_line_ends() {
        let (names, rows, _) = sample();
        let csv = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Csv);
        let mut lines = csv.lines();
        assert_eq!(
            lines.next(),
            Some("Rank,Name,Played,Wins,Draws,Losses,Points,%,SB")
        );
        assert_eq!(
            lines.next(),
            Some("1,\"Stock, fish\",2,1,1,0,1.5,75.0,0.75"),
            "{csv}"
        );
        assert_eq!(
            lines.next(),
            Some("2,\"Lc0 \"\"net\"\"\",2,0,1,1,0.5,25.0,0.75")
        );
        assert!(!csv.contains('\r'));
        assert!(csv.ends_with('\n'));
    }

    #[test]
    fn text_aligns_columns_by_characters() {
        let (names, rows, _) = sample();
        let text = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Text);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[1].chars().all(|c| c == '-'));
        let widths: Vec<usize> = lines[2..].iter().map(|l| l.chars().count()).collect();
        assert_eq!(widths[0], widths[1], "rows align:\n{text}");
        assert!(lines[2].starts_with("   1 Stock, fish"), "{text}");
    }

    #[test]
    fn markdown_escapes_pipes_and_aligns_names_left() {
        let (mut names, rows, _) = sample();
        names[0] = "A|B".to_owned();
        let md = export_standings(
            &names,
            &rows,
            &ExportLabels::default(),
            ExportFormat::Markdown,
        );
        let lines: Vec<&str> = md.lines().collect();
        assert_eq!(
            lines[1],
            "| ---: | :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
        );
        assert!(lines[2].contains("A\\|B"), "{md}");
    }

    #[test]
    fn html_escapes_markup() {
        let (mut names, rows, _) = sample();
        names[0] = "<b>&\"'".to_owned();
        let html = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Html);
        assert!(html.starts_with("<table>"));
        assert!(
            html.contains("<td>&lt;b&gt;&amp;&quot;&#39;</td>"),
            "{html}"
        );
        assert!(!html.contains("<b>"));
        assert!(html.trim_end().ends_with("</table>"));
    }

    #[test]
    fn the_crosstable_lists_the_scores_of_each_pair() {
        let (names, rows, cells) = sample();
        let csv = export_cross(
            &names,
            &cells,
            &rows,
            &ExportLabels::default(),
            ExportFormat::Csv,
        );
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Rank,Name,1,2,Points");
        assert_eq!(lines[1], "1,\"Stock, fish\",x,1 \u{bd},1.5");
        assert_eq!(lines[2], "2,\"Lc0 \"\"net\"\"\",0 \u{bd},x,0.5");
    }

    #[test]
    fn withdrawn_participants_are_marked() {
        let (names, _, _) = sample();
        let games = [Finished {
            number: 1,
            white: 0,
            black: 1,
            result: GameResult::Draw,
        }];
        let rows = standings(2, &[false, true], &games);
        let csv = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Csv);
        assert!(csv.contains("(withdrawn)"), "{csv}");
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(
            export_file_name("My Cup: 1", ExportTable::Standings, ExportFormat::Csv),
            "My Cup_ 1-standings.csv"
        );
        assert_eq!(
            export_file_name("CON", ExportTable::Cross, ExportFormat::Html),
            "_CON-crosstable.html"
        );
        assert_eq!(ExportFormat::Markdown.extension(), "md");
    }

    #[test]
    fn an_empty_table_still_has_a_header() {
        let csv = export_standings(&[], &[], &ExportLabels::default(), ExportFormat::Csv);
        assert_eq!(csv.lines().count(), 1);
    }

    #[test]
    fn csv_guards_fields_a_spreadsheet_would_run_as_formulas() {
        // Regression (D1313): a name such as `=HYPERLINK(...)` came from an engine.
        for (field, expected) in [
            ("=1+1", "'=1+1"),
            ("+cmd", "'+cmd"),
            ("-2", "'-2"),
            ("@SUM(A1)", "'@SUM(A1)"),
            ("\tx", "'\tx"),
            ("\rx", "\"'\rx\""),
            ("=a,b", "\"'=a,b\""),
            ("safe", "safe"),
            ("a=b", "a=b"),
        ] {
            assert_eq!(csv_field(field), expected, "{field:?}");
        }
        let names = vec!["=cmd|' /C calc'!A0".to_owned(), "Beta".to_owned()];
        let (_, rows, _) = sample();
        let csv = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Csv);
        assert!(csv.contains(",'=cmd|' /C calc'!A0,"), "{csv}");
    }

    #[test]
    fn markdown_escapes_every_markup_character() {
        assert_eq!(
            markdown_escape("a\\b|c<d*e_f[g`h\ni"),
            "a\\\\b\\|c\\<d\\*e\\_f\\[g\\`h i"
        );
        let names = vec!["<b>x</b>".to_owned(), "*Bold* [l](u)".to_owned()];
        let (_, rows, _) = sample();
        let md = export_standings(
            &names,
            &rows,
            &ExportLabels::default(),
            ExportFormat::Markdown,
        );
        assert!(md.contains("\\<b>x\\</b>"), "{md}");
        assert!(md.contains("\\*Bold\\*"), "{md}");
    }

    #[test]
    fn text_flattens_control_characters_of_a_name() {
        let names = vec!["a\nb".to_owned(), "c".to_owned()];
        let (_, rows, _) = sample();
        let text = export_standings(&names, &rows, &ExportLabels::default(), ExportFormat::Text);
        assert_eq!(text.lines().count(), 4, "{text}");
    }
}
