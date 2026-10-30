//! The view-model of the opening report window (spec M7-2c, D3621-D3623).
//!
//! The window never reads the report while it draws: [`Model::build`] turns
//! the finished, shared report into text cells with the click each one
//! carries, once per report and language (the shell keeps the result). The
//! figures and the order of the lines are those of the documents
//! (`mychess_db::report`); only the shape is different: widgets instead of
//! text. Arithmetic is the report's own (integers, the roundings of the
//! Scid), nothing is recomputed here.

use std::fmt::Write as _;
use std::sync::Arc;

use mychess_app::opening_report::data::{
    BestKind, COLUMNS, EndTheme, NUM_THEMES, OpeningReport, RANGES, ReportCore, ReportLine,
    ReportOptions, StatsQuery, THEME_PLIES,
};
use mychess_app::opening_report::{ReportLabels, ReportSelector};
use mychess_core::{Color, GameResult};

/// Pixels a character of the tables takes (13 px Noto Sans, a safe bound).
const CHAR_WIDTH: f32 = 7.4;
/// Space around the text of a cell.
const CELL_PAD: f32 = 14.0;

/// What a click on a cell does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The games of the selection become the filter (D3622).
    Select(ReportSelector),
    /// Loads the game at the position of the report.
    Load(u32),
    /// Goes to a footnote of the theory table.
    Note(u32),
}

/// A cell of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The text.
    pub text: String,
    /// Aligned to the right (numbers).
    pub right: bool,
    /// The click, if the cell is a link.
    pub target: Option<Target>,
}

impl Cell {
    fn left(text: impl Into<String>) -> Cell {
        Cell {
            text: text.into(),
            right: false,
            target: None,
        }
    }

    fn right(text: impl Into<String>) -> Cell {
        Cell {
            text: text.into(),
            right: true,
            target: None,
        }
    }

    fn link(mut self, target: Target) -> Cell {
        self.target = Some(target);
        self
    }
}

/// A table of cells with the width of each column.
#[derive(Debug, Clone, PartialEq)]
pub struct GridModel {
    /// The header lines.
    pub header: Vec<Vec<Cell>>,
    /// The body lines.
    pub rows: Vec<Vec<Cell>>,
    /// The width of each column in logical pixels.
    pub widths: Vec<f32>,
}

impl GridModel {
    fn new(header: Vec<Vec<Cell>>, rows: Vec<Vec<Cell>>) -> GridModel {
        let columns = header.iter().chain(&rows).map(Vec::len).max().unwrap_or(0);
        let widths = (0..columns)
            .map(|c| {
                let chars = header
                    .iter()
                    .chain(&rows)
                    .filter_map(|row| row.get(c))
                    .map(|cell| cell.text.chars().count())
                    .max()
                    .unwrap_or(0);
                // Text lengths are tiny: the conversion is exact.
                #[allow(clippy::cast_precision_loss)]
                let width = chars as f32 * CHAR_WIDTH + CELL_PAD;
                width
            })
            .collect();
        GridModel {
            header,
            rows,
            widths,
        }
    }

    /// Every text of the table, for the search.
    fn texts(&self) -> impl Iterator<Item = &str> {
        self.header
            .iter()
            .chain(&self.rows)
            .flatten()
            .map(|c| c.text.as_str())
    }
}

/// A game of a list (oldest, newest, best, shortest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRow {
    /// The game number in the database.
    pub game: u32,
    /// `3: 1-0(35) Alfa 2500 - Beta 2400, Rio 2020.01.10`.
    pub text: String,
}

/// A row of the theory table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheoryRow {
    /// The row number, from 1.
    pub number: usize,
    /// The 16 cells (White, Black, ...) with the footnote of each (0: none).
    pub cells: Vec<(String, u32)>,
    /// Games in the row.
    pub count: u32,
    /// Score of White in per cent.
    pub score: u32,
    /// The line to the position and the moves of the row, for "Copy line".
    pub line: Arc<str>,
}

/// The theory table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theory {
    /// The line to the position (with the excluded move).
    pub stem: String,
    /// `+3 =1 -1 (3.5/5: 70%)`.
    pub heading: String,
    /// The number of the move of the first column.
    pub first_number: usize,
    /// Whether Black is to move at the position (the first cell is the last
    /// move of the line).
    pub black_first: bool,
    /// The rows.
    pub rows: Vec<TheoryRow>,
    /// The comment of the limit of games.
    pub comment: String,
}

/// A footnote of the theory table: its lines of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteModel {
    /// The number.
    pub number: u32,
    /// The paragraphs.
    pub lines: Vec<String>,
}

/// A piece of the body.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// `1. Statistics and History`.
    Section(String),
    /// `1.1 Statistics`.
    Sub(String),
    /// A line of text.
    Para(String),
    /// A table.
    Grid(GridModel),
    /// A list of games.
    Games(Vec<GameRow>),
    /// The theory table.
    Theory(Box<Theory>),
    /// The footnotes.
    Notes(Vec<NoteModel>),
}

/// The header of the window: the line, the facts and the warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The line to the position, or the title.
    pub title: String,
    /// `label: value` and the click of the value.
    pub rows: Vec<(String, String, Option<Target>)>,
    /// The report was interrupted or games were skipped.
    pub warnings: Vec<String>,
}

/// Everything the window shows of a report.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The header.
    pub header: Header,
    /// The body.
    pub blocks: Vec<Block>,
}

/// The rows of a model that hold a word, found once per change of the
/// search text (never while drawing).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hits {
    /// Per block, whether each of its rows matches: one entry for a
    /// section, a sub-title or a paragraph, one per row of a table or a
    /// list, one per footnote. Empty without a search.
    blocks: Vec<Vec<bool>>,
    count: usize,
}

impl Hits {
    /// Whether row `row` of block `block` matches.
    pub fn hit(&self, block: usize, row: usize) -> bool {
        self.blocks
            .get(block)
            .and_then(|b| b.get(row))
            .copied()
            .unwrap_or(false)
    }

    /// How many rows match, as the search field counts them.
    pub fn count(&self) -> usize {
        self.count
    }
}

impl Model {
    /// Finds the rows that hold `needle`, ignoring case.
    pub fn search(&self, needle: &str) -> Hits {
        let needle = needle.to_lowercase();
        if needle.is_empty() {
            return Hits::default();
        }
        let hit = |t: &str| t.to_lowercase().contains(&needle);
        let blocks: Vec<Vec<bool>> = self
            .blocks
            .iter()
            .map(|block| match block {
                Block::Section(t) | Block::Sub(t) | Block::Para(t) => vec![hit(t)],
                Block::Grid(g) => g
                    .rows
                    .iter()
                    .map(|r| r.iter().any(|c| hit(&c.text)))
                    .collect(),
                Block::Games(g) => g.iter().map(|r| hit(&r.text)).collect(),
                Block::Theory(t) => t
                    .rows
                    .iter()
                    .map(|r| r.cells.iter().any(|c| hit(&c.0)))
                    .collect(),
                Block::Notes(notes) => notes
                    .iter()
                    .map(|n| n.lines.iter().any(|l| hit(l)))
                    .collect(),
            })
            .collect();
        let count = blocks.iter().flatten().filter(|h| **h).count();
        Hits { blocks, count }
    }

    /// How many rows hold `needle` (a one-off; the window keeps [`Hits`]).
    pub fn count_matches(&self, needle: &str) -> usize {
        self.search(needle).count()
    }
}

fn fill(template: &str, n: impl std::fmt::Display, max: Option<u32>) -> String {
    let mut text = template.replace("{n}", &n.to_string());
    if let Some(max) = max {
        text = text.replace("{max}", &max.to_string());
    }
    text
}

fn cap(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn thousands(n: u32, sep: &str) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(sep);
        }
        out.push(c);
    }
    out
}

fn tenths(value: u32, decimal: char) -> String {
    format!("{}{decimal}{}%", value / 10, value % 10)
}

/// The line to the position, numbered: `1.e4 c5 2.Nf3`.
pub fn stem_line(report: &OpeningReport) -> Vec<String> {
    let mut out = Vec::new();
    if report.stem_omitted > 0 {
        out.push("...".to_owned());
    }
    for (i, san) in report.stem.iter().enumerate() {
        let ply = report.stem_omitted + i;
        let number = ply / 2 + 1;
        if ply.is_multiple_of(2) {
            out.push(format!("{number}.{san}"));
        } else if i == 0 {
            out.push(format!("{number}...{san}"));
        } else {
            out.push(san.clone());
        }
    }
    out
}

fn result_mark(result: GameResult) -> &'static str {
    match result {
        GameResult::White => "1-0",
        GameResult::Black => "0-1",
        GameResult::Draw => "=",
        _ => "*",
    }
}

fn surname(name: &str) -> &str {
    name.split(',').next().unwrap_or("")
}

fn elo_text(elo: u16) -> String {
    if elo == 0 {
        String::new()
    } else {
        format!(" {elo}")
    }
}

/// What `OpLine::PrintSummary` writes about a game.
fn summary(line: &ReportLine, show_moves: bool, date: &str) -> String {
    let moves = if show_moves {
        format!("({})", line.full_moves())
    } else {
        String::new()
    };
    format!(
        "{}{} {}{} - {}{}, {} {}",
        result_mark(line.result),
        moves,
        surname(&line.white),
        elo_text(line.white_elo),
        surname(&line.black),
        elo_text(line.black_elo),
        line.site,
        date
    )
}

fn games_block(core: &ReportCore, kind: BestKind, count: u8, full_date: bool) -> Block {
    let rows = core
        .best(kind, usize::from(count))
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let date = if full_date {
                l.date.short()
            } else {
                l.date
                    .year()
                    .map_or_else(|| "?".to_owned(), |y| y.to_string())
            };
            GameRow {
                game: l.game,
                text: format!("{}: {}", i + 1, summary(l, true, &date)),
            }
        })
        .collect();
    Block::Games(rows)
}

struct Builder<'a> {
    blocks: Vec<Block>,
    labels: &'a ReportLabels,
    section: u32,
    sub: u32,
}

impl Builder<'_> {
    fn section(&mut self, text: &str) {
        self.section += 1;
        self.sub = 0;
        self.blocks
            .push(Block::Section(format!("{}. {text}", self.section)));
    }

    fn sub(&mut self, text: &str) {
        self.sub += 1;
        self.blocks
            .push(Block::Sub(format!("{}.{} {text}", self.section, self.sub)));
    }

    fn para(&mut self, text: String) {
        self.blocks.push(Block::Para(text));
    }

    fn grid(&mut self, header: Vec<Vec<Cell>>, rows: Vec<Vec<Cell>>) {
        self.blocks.push(Block::Grid(GridModel::new(header, rows)));
    }
}

impl Model {
    /// The model of `report`, with the words of `labels` and the generator
    /// `generator` (`MyChess 0.1.0`). `part_table` is false for a window
    /// that hides the theory table.
    pub fn build(
        report: &OpeningReport,
        opts: &ReportOptions,
        labels: &ReportLabels,
        database_name: &str,
        generator: &str,
    ) -> Model {
        let opts = opts.normalized();
        let core = &report.core;
        let mut b = Builder {
            blocks: Vec::new(),
            labels,
            section: 0,
            sub: 0,
        };
        let header = header(report, labels, database_name, generator);
        if core.report_games() > 0 {
            if opts.has_stats_section() {
                stats_section(&mut b, report, &opts);
            }
            if opts.has_ratings_section() {
                ratings_section(&mut b, core, &opts);
            }
            if opts.has_trends_section() {
                trends_section(&mut b, core, &opts);
            }
            if opts.has_moves_section() {
                moves_section(&mut b, report, &opts);
            }
            if opts.max_table_games > 0 {
                let title = b.labels.theory_table.clone();
                b.section(&title);
                theory_blocks(&mut b, report, &opts);
            }
        }
        Model {
            header,
            blocks: b.blocks,
        }
    }
}

fn header(
    report: &OpeningReport,
    l: &ReportLabels,
    database_name: &str,
    generator: &str,
) -> Header {
    let core = &report.core;
    let line = stem_line(report);
    let title = if line.is_empty() {
        l.title.clone()
    } else {
        line.join(" ")
    };
    let sep = &l.thousands;
    let mut rows = vec![
        (
            l.database.clone(),
            format!(
                "{database_name} ({} {})",
                thousands(core.database.games, sep),
                l.games
            ),
            None,
        ),
        (
            l.report.clone(),
            format!(
                "{} ({} {})",
                line.join(" "),
                thousands(core.report_games(), sep),
                l.games
            ),
            (core.report_games() > 0).then_some(Target::Select(ReportSelector::All)),
        ),
    ];
    if let Some(eco) = &report.eco {
        rows.push((l.eco.clone(), eco.clone(), None));
    }
    rows.push((l.generated.clone(), generator.to_owned(), None));
    let mut warnings = Vec::new();
    if core.interrupted {
        warnings.push(l.partial.clone());
    }
    if core.skipped > 0 {
        warnings.push(fill(&l.skipped, core.skipped, None));
    }
    Header {
        title,
        rows,
        warnings,
    }
}

fn stats_section(b: &mut Builder<'_>, report: &OpeningReport, opts: &ReportOptions) {
    let l = b.labels;
    let core = &report.core;
    b.section(&l.stats_hist);
    if core.theory_games() > opts.max_games {
        b.para(fill(&l.table_comment, opts.max_games, None));
    }
    if opts
        .sections
        .contains(mychess_app::opening_report::data::SectionSet::STATS)
    {
        b.sub(&l.stats);
        let header = vec![vec![
            Cell::left(""),
            Cell::right(cap(&l.games)),
            Cell::right("1-0"),
            Cell::right("=-="),
            Cell::right("0-1"),
            Cell::right(l.score.clone()),
        ]];
        let mut rows = Vec::new();
        for line in &core.stats {
            let label = match line.query {
                StatsQuery::All => l.stat_all.clone(),
                StatsQuery::BothRated(elo) => format!("{} {elo}+", l.stat_both),
                StatsQuery::Since(year) => format!("{} {year}.01.01", l.stat_since),
                StatsQuery::Years { .. } => continue,
            };
            let r = &line.row;
            let mut first = Cell::left(label);
            if matches!(line.query, StatsQuery::All) {
                first = first.link(Target::Select(ReportSelector::All));
            }
            rows.push(vec![
                first,
                Cell::right(r.total.to_string()),
                Cell::right(r.white.to_string()),
                Cell::right(r.draw.to_string()),
                Cell::right(r.black.to_string()),
                Cell::right(tenths(r.score_tenths(), l.decimal)),
            ]);
        }
        b.grid(header, rows);
    }
    if opts.oldest > 0 && !core.lines.is_empty() {
        b.sub(&l.oldest);
        b.blocks
            .push(games_block(core, BestKind::Oldest, opts.oldest, true));
    }
    if opts.newest > 0 && !core.lines.is_empty() {
        b.sub(&l.newest);
        b.blocks
            .push(games_block(core, BestKind::Newest, opts.newest, true));
    }
    if opts
        .sections
        .contains(mychess_app::opening_report::data::SectionSet::POPULAR)
    {
        b.sub(&l.popular);
        popularity(b, core);
    }
    if opts.has_frequent_players() {
        for (on, color, name) in [
            (opts.most_frequent_white, Color::White, &l.white),
            (opts.most_frequent_black, Color::Black, &l.black),
        ] {
            if on {
                b.sub(&format!("{} ({name})", l.most_frequent));
                players(b, core, color, usize::from(opts.most_frequent));
            }
        }
    }
}

fn popularity(b: &mut Builder<'_>, core: &ReportCore) {
    let l = b.labels;
    let p = &core.popularity;
    let mut header = vec![Cell::left(l.year.clone())];
    let mut row = vec![Cell::left(cap(&fill(&l.every, "X", None)))];
    for (from, to) in RANGES {
        header.push(Cell::right(format!("{}-{:02}", from, to % 100)));
    }
    for r in &p.ranges {
        row.push(Cell::right(
            r.map_or_else(|| l.none.clone(), |n| n.to_string()),
        ));
    }
    b.grid(vec![header], vec![row]);
    let mut rows = vec![vec![
        Cell::left(l.freq_all.clone()),
        Cell::left(fill(&l.every, p.ratio_all, None)),
        Cell::left(""),
    ]];
    for t in &p.recent {
        let label = match t.years {
            10 => &l.freq_10,
            5 => &l.freq_5,
            _ => &l.freq_1,
        };
        let change = match t.delta.cmp(&0) {
            std::cmp::Ordering::Greater => fill(&l.up, t.delta, None),
            std::cmp::Ordering::Less => fill(&l.down, t.delta.unsigned_abs(), None),
            std::cmp::Ordering::Equal => l.same.clone(),
        };
        rows.push(vec![
            Cell::left(label.clone()),
            Cell::left(fill(&l.every, t.ratio, None)),
            Cell::left(format!("({change})")),
        ]);
    }
    b.grid(Vec::new(), rows);
}

fn players(b: &mut Builder<'_>, core: &ReportCore, color: Color, count: usize) {
    let rows = core
        .top_players(color, count)
        .into_iter()
        .map(|p| {
            let years = match (p.first_year, p.last_year) {
                (_, 0) => String::new(),
                (a, z) if a == z => a.to_string(),
                (a, z) => format!("{a}-{z}"),
            };
            let mut name = p.name;
            if !p.notes.is_empty() {
                let list: Vec<String> = p.notes.iter().map(u32::to_string).collect();
                let _ = write!(
                    name,
                    " [{}{}]",
                    list.join(","),
                    if p.more_notes { ",..." } else { "" }
                );
            }
            vec![
                Cell::right(format!("{}:", p.rank)),
                Cell::right(p.games.to_string()),
                Cell::right(years),
                Cell::right(format!("{}%", p.score_percent)),
                Cell::right(if p.max_elo == 0 {
                    String::new()
                } else {
                    p.max_elo.to_string()
                }),
                Cell::left(name),
            ]
        })
        .collect();
    b.grid(Vec::new(), rows);
}

fn ratings_section(b: &mut Builder<'_>, core: &ReportCore, opts: &ReportOptions) {
    let l = b.labels;
    b.section(&l.ratings_perf);
    if opts
        .sections
        .contains(mychess_app::opening_report::data::SectionSet::AVG_PERF)
    {
        b.sub(&l.avg_perf);
        let white = core.elo_report(Color::White);
        let black = core.elo_report(Color::Black);
        b.para(format!(
            "{}: {} ({} {}); {}: {} ({}% vs {})",
            l.white_rating,
            white.average,
            white.games,
            l.games,
            l.white_perf,
            black.opponent_performance,
            black.opponent_percent,
            white.average
        ));
        b.para(format!(
            "{}: {} ({} {}); {}: {} ({}% vs {})",
            l.black_rating,
            black.average,
            black.games,
            l.games,
            l.black_perf,
            white.opponent_performance,
            white.opponent_percent,
            black.average
        ));
    }
    if opts.high_rating > 0 {
        b.sub(&l.high_rating);
        b.blocks.push(games_block(
            core,
            BestKind::HighRating,
            opts.high_rating,
            false,
        ));
    }
}

fn trends_section(b: &mut Builder<'_>, core: &ReportCore, opts: &ReportOptions) {
    let l = b.labels;
    b.section(&l.trends);
    if opts
        .sections
        .contains(mychess_app::opening_report::data::SectionSet::RESULTS)
    {
        b.sub(&l.results);
        let results = [GameResult::White, GameResult::Draw, GameResult::Black];
        let row = |label: &str, score: u32, lengths: [u32; 3], freqs: [u32; 3]| {
            let mut cells = vec![Cell::left(label), Cell::right(tenths(score, l.decimal))];
            cells.extend(lengths.iter().map(|n| Cell::right(n.to_string())));
            cells.extend(freqs.iter().map(|&f| Cell::right(tenths(f, l.decimal))));
            cells
        };
        let report = row(
            &l.report_games,
            core.score_tenths(),
            results.map(|r| core.avg_length(r)),
            results.map(|r| core.freq_tenths(r)),
        );
        let all = row(
            &l.all_games,
            core.all_score_tenths(),
            results.map(|r| core.all_avg_length(r)),
            results.map(|r| core.all_freq_tenths(r)),
        );
        let marks = || ["1-0", "=-=", "0-1"].map(Cell::right);
        let mut head1 = vec![
            Cell::left(""),
            Cell::left(""),
            Cell::right(l.length.clone()),
            Cell::left(""),
            Cell::left(""),
        ];
        head1.extend([
            Cell::right(l.frequency.clone()),
            Cell::left(""),
            Cell::left(""),
        ]);
        let mut head2 = vec![Cell::left(""), Cell::right(cap(&l.score))];
        head2.extend(marks());
        head2.extend(marks());
        b.grid(vec![head1, head2], vec![report, all]);
    }
    if opts.shortest > 0 {
        for (on, color, name) in [
            (opts.shortest_white, Color::White, &l.white),
            (opts.shortest_black, Color::Black, &l.black),
        ] {
            if on {
                b.sub(&format!("{} ({name})", l.shortest));
                b.blocks.push(games_block(
                    core,
                    BestKind::Shortest(color),
                    opts.shortest,
                    false,
                ));
            }
        }
    }
}

fn moves_section(b: &mut Builder<'_>, report: &OpeningReport, opts: &ReportOptions) {
    use mychess_app::opening_report::data::SectionSet;
    let l = b.labels;
    let core = &report.core;
    b.section(&l.moves_themes);
    if opts.move_orders > 0 {
        b.sub(&l.move_orders);
        let total = u32::try_from(core.move_orders.len()).unwrap_or(u32::MAX);
        let shown = u32::from(opts.move_orders);
        let text = if total == 1 {
            l.move_orders_one.clone()
        } else if total <= shown {
            fill(&l.move_orders_all, total, None)
        } else {
            fill(&l.move_orders_many, total, Some(shown))
        };
        b.para(text);
        let rows = core
            .move_orders
            .iter()
            .take(usize::from(opts.move_orders))
            .enumerate()
            .map(|(i, o)| {
                vec![
                    Cell::right(format!("{}:", i + 1)),
                    Cell::left(o.moves.clone())
                        .link(Target::Select(ReportSelector::MoveOrder(o.id))),
                    Cell::right(format!("({})", o.count)),
                ]
            })
            .collect();
        b.grid(Vec::new(), rows);
    }
    if opts.sections.contains(SectionSet::MOVES_FROM) {
        b.sub(&l.moves_from);
        tree_grid(b, report);
    }
    if opts.sections.contains(SectionSet::THEMES) {
        b.sub(&l.themes);
        b.para(fill(&l.theme_description, THEME_PLIES / 2, None));
        let half = NUM_THEMES.div_ceil(2);
        let rows = (0..half)
            .map(|i| {
                let cells = |t: usize| -> [Cell; 2] {
                    match l.theme_names.get(t) {
                        Some(name) => [
                            Cell::left(name.clone()).link(Target::Select(ReportSelector::Theme(t))),
                            Cell::right(format!("{}%", core.theme_percent(t))),
                        ],
                        None => [Cell::left(""), Cell::right("")],
                    }
                };
                let [a, pa] = cells(i);
                let [c, pc] = cells(i + half);
                vec![a, pa, c, pc]
            })
            .collect();
        b.grid(Vec::new(), rows);
    }
    if opts.sections.contains(SectionSet::ENDGAMES) {
        b.sub(&l.endgames);
        b.para(format!("{}:", l.end_class));
        let mut header = vec![Cell::left("")];
        header.extend(EndTheme::ALL.iter().enumerate().map(|(i, t)| {
            Cell::right(t.short_name()).link(Target::Select(ReportSelector::Endgame(i)))
        }));
        let row = |label: &str, all: bool| {
            let mut cells = vec![Cell::left(label)];
            cells.extend(
                (0..EndTheme::ALL.len())
                    .map(|i| Cell::right(format!("{}%", core.endgame_percent(i, all)))),
            );
            cells
        };
        b.grid(
            vec![header],
            vec![row(&l.report_games, false), row(&l.all_games, true)],
        );
    }
}

fn tree_grid(b: &mut Builder<'_>, report: &OpeningReport) {
    let l = b.labels;
    let header = vec![vec![
        Cell::left(""),
        Cell::left(l.tree_move.clone()),
        Cell::right(l.tree_frequency.clone()),
        Cell::right("%"),
        Cell::right(l.score.clone()),
        Cell::right(format!("% {}", l.tree_draws)),
        Cell::right(l.tree_elo.clone()),
        Cell::right(l.tree_perf.clone()),
        Cell::right(l.tree_year.clone()),
        Cell::left(l.eco.clone()),
    ]];
    let opt = |v: Option<u32>| v.map_or_else(String::new, |n| n.to_string());
    let mut rows = Vec::new();
    let total_games = report.tree.total.as_ref().map_or(0, |t| t.total);
    for (i, r) in report.tree.rows.iter().enumerate() {
        let (whole, tenth) = r.share_tenths(total_games);
        let (sp, st) = r.score_tenths();
        rows.push(vec![
            Cell::right((i + 1).to_string()),
            Cell::left(r.san.clone()),
            Cell::right(r.total.to_string()),
            Cell::right(format!("{whole}{}{tenth}%", l.decimal)),
            Cell::right(format!("{sp}{}{st}%", l.decimal)),
            Cell::right(format!("{}%", r.draw_percent())),
            Cell::right(opt(r.avg_elo)),
            Cell::right(opt(r.perf)),
            Cell::right(opt(r.avg_year)),
            Cell::left(r.eco.map_or_else(String::new, |e| e.to_string())),
        ]);
    }
    if let Some(t) = &report.tree.total {
        let (sp, st) = t.score_tenths();
        rows.push(vec![
            Cell::left(""),
            Cell::left(l.tree_total.clone()),
            Cell::right(t.total.to_string()),
            Cell::right(format!("100{}0%", l.decimal)),
            Cell::right(format!("{sp}{}{st}%", l.decimal)),
            Cell::right(format!("{}%", t.draw_percent())),
            Cell::right(opt(t.avg_elo)),
            Cell::right(opt(t.perf)),
            Cell::right(opt(t.avg_year)),
            Cell::left(""),
        ]);
    }
    b.grid(header, rows);
}

/// A move as it is printed in a cell of the table: without `+` and `x`
/// (Scid: `OPTABLE_Compact`).
fn compact(san: &str) -> String {
    san.chars().filter(|c| *c != '+' && *c != 'x').collect()
}

/// The moves of `line` from half move `start`, numbered from `first_number`
/// (`OpLine::PrintNote`).
fn note_moves(line: &ReportLine, first_number: usize, start: usize) -> String {
    let (mut white, mut number) = (true, first_number);
    let mut out = String::new();
    for i in 0..line.moves.len() {
        if i >= start {
            let mv = line.move_at(i);
            if i == start {
                let _ = write!(out, "{number}{}{mv}", if white { "." } else { "..." });
            } else if white {
                let _ = write!(out, " {number}.{mv}");
            } else {
                out.push(' ');
                out.push_str(mv);
            }
        }
        if white {
            white = false;
        } else {
            white = true;
            number += 1;
        }
    }
    out
}

/// The line to the position followed by the moves of `line` as the row
/// shows them (with Black to move the first cell repeats the last move of
/// the line, which is not appended twice).
fn copy_line(report: &OpeningReport, line: &ReportLine, black_first: bool) -> String {
    let mut out = stem_line(report).join(" ");
    let first_number = report.first_move_number();
    let (mut white, mut number) = (!black_first, first_number);
    let skip = usize::from(black_first);
    for (i, mv) in line.moves.iter().enumerate() {
        if i < skip {
            continue;
        }
        if mv.is_empty() {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        if white {
            let _ = write!(out, "{number}.{mv}");
        } else {
            // A first Black move after White's move needs no number.
            out.push_str(mv);
        }
        if white {
            white = false;
        } else {
            white = true;
            number += 1;
        }
    }
    out
}

fn theory_blocks(b: &mut Builder<'_>, report: &OpeningReport, opts: &ReportOptions) {
    let l = b.labels;
    let core = &report.core;
    if core.rows.is_empty() {
        b.para(l.empty.clone());
        return;
    }
    let first_number = report.first_move_number();
    let black_first = report.position.turn() == Color::Black;
    let mut stem = stem_line(report).join(" ");
    if let Some(ex) = &report.excluded {
        let dots = if report.stem_plies().is_multiple_of(2) {
            "."
        } else {
            "..."
        };
        let _ = write!(stem, " (X {first_number}{dots}{ex})");
    }
    let pts = core.theory_points2();
    let mut points = (pts / 2).to_string();
    if pts % 2 == 1 {
        points.push(l.decimal);
        points.push('5');
    }
    let heading = format!(
        "+{} ={} -{} ({points}/{}: {}%)",
        core.theory.white,
        core.theory.draws,
        core.theory.black,
        core.theory_games(),
        (core.theory_score_tenths() + 5) / 10
    );
    let anchors: std::collections::HashMap<(usize, usize), u32> = core
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
                .map(|j| {
                    (
                        if j < same {
                            "...".to_owned()
                        } else {
                            compact(head.move_at(j))
                        },
                        anchors.get(&(r, j)).copied().unwrap_or(0),
                    )
                })
                .collect();
            Some(TheoryRow {
                number: r + 1,
                cells,
                count: row.count,
                score: row.score_percent(),
                line: Arc::from(copy_line(report, head, black_first)),
            })
        })
        .collect();
    let comment = if core.theory_games() > opts.max_table_games {
        fill(&l.table_comment, opts.max_table_games, None)
    } else {
        String::new()
    };
    b.blocks.push(Block::Theory(Box::new(Theory {
        stem,
        heading,
        first_number,
        black_first,
        rows,
        comment,
    })));
    let notes = notes(report, first_number);
    if !notes.is_empty() {
        b.blocks.push(Block::Notes(notes));
    }
}

fn notes(report: &OpeningReport, first_number: usize) -> Vec<NoteModel> {
    let core = &report.core;
    core.notes
        .iter()
        .map(|note| {
            let mut lines: Vec<String> = Vec::new();
            let mut prev: Option<&ReportLine> = None;
            for e in &note.entries {
                let Some(line) = core.lines.get(e.line) else {
                    continue;
                };
                let mut start = e.start;
                let mut paragraph = true;
                if let Some(p) = prev {
                    start = line.common_length(p);
                    paragraph = start <= e.start && p.moves.len() > start;
                }
                let year = line
                    .date
                    .year()
                    .map_or_else(|| "?".to_owned(), |y| y.to_string());
                let text = format!(
                    "{} {}",
                    note_moves(line, first_number, start),
                    summary(line, !line.short_game, &year)
                );
                match (prev, paragraph, lines.last_mut()) {
                    (Some(_), false, Some(last)) => {
                        last.push_str("; ");
                        last.push_str(&text);
                    }
                    _ => lines.push(text),
                }
                prev = Some(line);
            }
            NoteModel {
                number: note.number,
                lines,
            }
        })
        .collect()
}

impl Model {
    /// Every text the model holds, for tests that look for a word.
    pub fn all_texts(&self) -> Vec<String> {
        let mut out = vec![self.header.title.clone()];
        out.extend(self.header.rows.iter().map(|r| format!("{}: {}", r.0, r.1)));
        out.extend(self.header.warnings.iter().cloned());
        for block in &self.blocks {
            match block {
                Block::Section(t) | Block::Sub(t) | Block::Para(t) => out.push(t.clone()),
                Block::Grid(g) => out.extend(g.texts().map(str::to_owned)),
                Block::Games(g) => out.extend(g.iter().map(|r| r.text.clone())),
                Block::Theory(t) => {
                    out.push(t.stem.clone());
                    out.push(t.heading.clone());
                    out.extend(t.rows.iter().map(|r| r.line.to_string()));
                }
                Block::Notes(notes) => {
                    for n in notes {
                        out.extend(n.lines.iter().cloned());
                    }
                }
            }
        }
        out
    }
}
