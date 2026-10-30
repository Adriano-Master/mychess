//! Builds the blocks of the document from the data of the report (the body
//! of the Scid's `::optable::report`).

use std::fmt::Write as _;

use mychess_core::{Color, GameResult};

use super::{Align, Block, GameItem, Grid, ReportPart, StemView, Summary, theory};
use crate::filter_stats::{FilterStatsRow, StatsQuery};
use crate::report::labels::{ReportLabels, fill, thousands};
use crate::report::model::{
    EndTheme, NUM_THEMES, OpeningReport, RANGES, ReportCore, ReportLine, THEME_PLIES,
};
use crate::report::options::{ReportOptions, SectionSet};
use crate::report::stats::{BestKind, PlayerRow, oriented, score_tenths};

pub(super) fn cap(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// `57.3%` from tenths of a per cent.
pub(super) fn tenths(value: u32, decimal: char) -> String {
    format!("{}{decimal}{}%", value / 10, value % 10)
}

/// The line of an opening report as the shared blocks see it.
fn stem_view(report: &OpeningReport) -> StemView<'_> {
    StemView {
        stem: &report.stem,
        omitted: report.stem_omitted,
        excluded: report.excluded.as_deref(),
    }
}

/// The line to the position, numbered: `1.e4 c5 2.Nf3`.
pub(super) fn stem_line(line: &StemView<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if line.omitted > 0 {
        out.push("...".to_owned());
    }
    for (i, san) in line.stem.iter().enumerate() {
        let ply = line.omitted + i;
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

fn summary(line: &ReportLine, show_moves: bool) -> Summary {
    let surname = |name: &str| name.split(',').next().unwrap_or("").to_owned();
    Summary {
        result: line.result,
        moves: line.full_moves(),
        show_moves,
        white: surname(&line.white),
        black: surname(&line.black),
        white_elo: line.white_elo,
        black_elo: line.black_elo,
        site: line.site.clone(),
        year: line
            .date
            .year()
            .map_or_else(|| "?".to_owned(), |y| y.to_string()),
        date: line.date.short(),
    }
}

pub(super) fn game_summary(line: &ReportLine, show_moves: bool) -> Summary {
    summary(line, show_moves || !line.short_game)
}

pub(super) fn games(core: &ReportCore, kind: BestKind, count: u8, full_date: bool) -> Block {
    let items = core
        .best(kind, usize::from(count))
        .into_iter()
        .enumerate()
        .map(|(i, l)| GameItem {
            rank: i + 1,
            summary: summary(l, true),
            full_date,
            note: l.note,
        })
        .collect();
    Block::Games(items)
}

/// The blocks of a document being built, with the numbering of the
/// sections.
pub(super) struct Builder<'a> {
    pub blocks: Vec<Block>,
    pub labels: &'a ReportLabels,
    pub section: u32,
    pub sub: u32,
}

impl Builder<'_> {
    pub(super) fn section(&mut self, text: &str) {
        self.section += 1;
        self.sub = 0;
        self.blocks.push(Block::Section {
            number: self.section,
            text: text.to_owned(),
        });
    }

    pub(super) fn sub(&mut self, text: &str) {
        self.sub += 1;
        self.blocks.push(Block::Subsection {
            section: self.section,
            number: self.sub,
            text: text.to_owned(),
        });
    }

    pub(super) fn para(&mut self, text: String) {
        self.blocks.push(Block::Paragraph(text));
    }
}

/// Builds the document.
pub(super) fn build(
    report: &OpeningReport,
    opts: &ReportOptions,
    labels: &ReportLabels,
    part: ReportPart,
) -> Vec<Block> {
    let mut b = Builder {
        blocks: Vec::new(),
        labels,
        section: 0,
        sub: 0,
    };
    let core = &report.core;
    header(&mut b, report);
    if part == ReportPart::TableOnly {
        theory::push(
            &mut b.blocks,
            core,
            &stem_view(report),
            Color::White,
            opts,
            labels,
            false,
        );
        return b.blocks;
    }
    if core.report_games() == 0 {
        return b.blocks;
    }
    if opts.has_stats_section() {
        stats_section(&mut b, report, opts);
    }
    if opts.has_ratings_section() {
        ratings_section(&mut b, core, opts);
    }
    if opts.has_trends_section() {
        trends_section(&mut b, core, opts);
    }
    if opts.has_moves_section() {
        moves_section(&mut b, report, opts);
    }
    if part == ReportPart::Full && opts.max_table_games > 0 {
        b.section(&labels.theory_table);
        theory::push(
            &mut b.blocks,
            core,
            &stem_view(report),
            Color::White,
            opts,
            labels,
            true,
        );
    }
    b.blocks
}

fn header(b: &mut Builder<'_>, report: &OpeningReport) {
    let l = b.labels;
    let core = &report.core;
    let line = stem_line(&stem_view(report));
    let text = if line.is_empty() {
        l.title.clone()
    } else {
        line.join(" ")
    };
    b.blocks.push(Block::Title {
        text,
        line: line.clone(),
    });
    b.blocks.push(Block::Board {
        fen: report.position.fen(),
    });
    let sep = &l.thousands;
    let mut rows = vec![
        (
            l.database.clone(),
            format!(
                "{} ({} {})",
                l.database_name,
                thousands(core.database.games, sep),
                l.games
            ),
        ),
        (
            l.report.clone(),
            format!(
                "{} ({} {})",
                line.join(" "),
                thousands(core.report_games(), sep),
                l.games
            ),
        ),
    ];
    if let Some(eco) = &report.eco {
        rows.push((l.eco.clone(), eco.clone()));
    }
    rows.push((l.generated.clone(), format!("{}, {}", l.generator, l.date)));
    b.blocks.push(Block::Header(rows));
    if core.interrupted {
        b.para(l.partial.clone());
    }
    if core.skipped > 0 {
        b.para(fill(&l.skipped, core.skipped, None));
    }
}

fn stats_section(b: &mut Builder<'_>, report: &OpeningReport, opts: &ReportOptions) {
    let l = b.labels;
    let core = &report.core;
    b.section(&l.stats_hist);
    if core.theory_games() > opts.max_games {
        b.para(fill(&l.table_comment, opts.max_games, None));
    }
    if opts.sections.contains(SectionSet::STATS) {
        b.sub(&l.stats);
        b.blocks.push(Block::Grid(stats_grid(core, l)));
    }
    if opts.oldest > 0 && !core.lines.is_empty() {
        b.sub(&l.oldest);
        b.blocks
            .push(games(core, BestKind::Oldest, opts.oldest, true));
    }
    if opts.newest > 0 && !core.lines.is_empty() {
        b.sub(&l.newest);
        b.blocks
            .push(games(core, BestKind::Newest, opts.newest, true));
    }
    if opts.sections.contains(SectionSet::POPULAR) {
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
                b.blocks.push(Block::Grid(players_grid(
                    core,
                    color,
                    usize::from(opts.most_frequent),
                )));
            }
        }
    }
}

fn stats_grid(core: &ReportCore, l: &ReportLabels) -> Grid {
    stats_grid_of(
        core,
        l,
        Color::White,
        ["1-0", "=-=", "0-1"].map(String::from),
    )
}

/// The statistics lines seen from `view`; `marks` are the titles of the
/// wins, draws and losses columns.
pub(super) fn stats_grid_of(
    core: &ReportCore,
    l: &ReportLabels,
    view: Color,
    marks: [String; 3],
) -> Grid {
    let [wins, draws, losses] = marks;
    let mut grid = Grid {
        aligns: vec![
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        header: vec![vec![
            String::new(),
            cap(&l.games),
            wins,
            draws,
            losses,
            l.score.clone(),
        ]],
        rows: Vec::new(),
    };
    for line in &core.stats {
        let label = match line.query {
            StatsQuery::All => l.stat_all.clone(),
            StatsQuery::BothRated(elo) => format!("{} {elo}+", l.stat_both),
            StatsQuery::Since(year) => format!("{} {year}.01.01", l.stat_since),
            StatsQuery::Years { .. } => continue,
        };
        let r = &line.row;
        let (won, lost) = match view {
            Color::White => (r.white, r.black),
            Color::Black => (r.black, r.white),
        };
        let seen = FilterStatsRow {
            white: won,
            black: lost,
            ..*r
        };
        grid.rows.push(vec![
            label,
            r.total.to_string(),
            won.to_string(),
            r.draw.to_string(),
            lost.to_string(),
            tenths(seen.score_tenths(), l.decimal),
        ]);
    }
    grid
}

fn popularity(b: &mut Builder<'_>, core: &ReportCore) {
    let l = b.labels;
    let p = &core.popularity;
    let every_x = cap(&fill(&l.every, "X", None));
    let mut header = vec![l.year.clone()];
    let mut row = vec![every_x];
    for (from, to) in RANGES {
        header.push(format!("{}-{:02}", from, to % 100));
    }
    for r in &p.ranges {
        row.push(r.map_or_else(|| l.none.clone(), |n| n.to_string()));
    }
    b.blocks.push(Block::Grid(Grid {
        aligns: vec![Align::Left; header.len()],
        header: vec![header],
        rows: vec![row],
    }));
    let mut rows = vec![vec![
        l.freq_all.clone(),
        fill(&l.every, p.ratio_all, None),
        String::new(),
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
            label.clone(),
            fill(&l.every, t.ratio, None),
            format!("({change})"),
        ]);
    }
    b.blocks.push(Block::Grid(Grid {
        aligns: vec![Align::Left; 3],
        header: Vec::new(),
        rows,
    }));
}

fn players_grid(core: &ReportCore, color: Color, count: usize) -> Grid {
    players_grid_of(core.top_players(color, count))
}

/// The table of frequent players (or opponents) for the rows given.
pub(super) fn players_grid_of(players: Vec<PlayerRow>) -> Grid {
    let rows = players
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
                format!("{}:", p.rank),
                p.games.to_string(),
                years,
                format!("{}%", p.score_percent),
                if p.max_elo == 0 {
                    String::new()
                } else {
                    p.max_elo.to_string()
                },
                name,
            ]
        })
        .collect();
    Grid {
        aligns: vec![
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
        header: Vec::new(),
        rows,
    }
}

fn ratings_section(b: &mut Builder<'_>, core: &ReportCore, opts: &ReportOptions) {
    let l = b.labels;
    b.section(&l.ratings_perf);
    if opts.sections.contains(SectionSet::AVG_PERF) {
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
            black.average
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
            white.average
        ));
    }
    if opts.high_rating > 0 {
        b.sub(&l.high_rating);
        b.blocks
            .push(games(core, BestKind::HighRating, opts.high_rating, false));
    }
}

fn trends_section(b: &mut Builder<'_>, core: &ReportCore, opts: &ReportOptions) {
    let l = b.labels;
    b.section(&l.trends);
    if opts.sections.contains(SectionSet::RESULTS) {
        b.sub(&l.results);
        b.blocks.push(Block::Grid(results_grid(core, l)));
    }
    if opts.shortest > 0 {
        for (on, color, name) in [
            (opts.shortest_white, Color::White, &l.white),
            (opts.shortest_black, Color::Black, &l.black),
        ] {
            if on {
                b.sub(&format!("{} ({name})", l.shortest));
                b.blocks
                    .push(games(core, BestKind::Shortest(color), opts.shortest, false));
            }
        }
    }
}

fn results_grid(core: &ReportCore, l: &ReportLabels) -> Grid {
    results_grid_of(
        core,
        l,
        Color::White,
        &["1-0", "=-=", "0-1"].map(String::from),
    )
}

/// The lengths and frequencies of the results seen from `view`: the first
/// column is the wins of that colour, the last one its losses. `marks` are
/// the three column titles.
pub(super) fn results_grid_of(
    core: &ReportCore,
    l: &ReportLabels,
    view: Color,
    marks: &[String; 3],
) -> Grid {
    let results = match view {
        Color::White => [GameResult::White, GameResult::Draw, GameResult::Black],
        Color::Black => [GameResult::Black, GameResult::Draw, GameResult::White],
    };
    let row = |label: &str, score: u32, lengths: [u32; 3], freqs: [u32; 3]| {
        let mut cells = vec![label.to_owned(), tenths(score, l.decimal)];
        cells.extend(lengths.iter().map(u32::to_string));
        cells.extend(freqs.iter().map(|&f| tenths(f, l.decimal)));
        cells
    };
    let report = row(
        &l.report_games,
        score_tenths(&oriented(&core.total, view)),
        results.map(|r| core.avg_length(r)),
        results.map(|r| core.freq_tenths(r)),
    );
    let all = row(
        &l.all_games,
        score_tenths(&oriented(&core.database.results, view)),
        results.map(|r| core.all_avg_length(r)),
        results.map(|r| core.all_freq_tenths(r)),
    );
    let marks = || marks.iter().cloned();
    let mut head1 = vec![
        String::new(),
        String::new(),
        l.length.clone(),
        String::new(),
        String::new(),
    ];
    head1.extend([l.frequency.clone(), String::new(), String::new()]);
    let mut head2 = vec![String::new(), cap(&l.score)];
    head2.extend(marks());
    head2.extend(marks());
    Grid {
        aligns: vec![Align::Left; 8]
            .into_iter()
            .enumerate()
            .map(|(i, a)| if i == 0 { a } else { Align::Right })
            .collect(),
        header: vec![head1, head2],
        rows: vec![report, all],
    }
}

fn moves_section(b: &mut Builder<'_>, report: &OpeningReport, opts: &ReportOptions) {
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
                    format!("{}:", i + 1),
                    o.moves.clone(),
                    format!("({})", o.count),
                ]
            })
            .collect();
        b.blocks.push(Block::Grid(Grid {
            aligns: vec![Align::Right, Align::Left, Align::Right],
            header: Vec::new(),
            rows,
        }));
    }
    if opts.sections.contains(SectionSet::MOVES_FROM) {
        b.sub(&l.moves_from);
        b.blocks.push(Block::Grid(tree_grid(report, l)));
    }
    if opts.sections.contains(SectionSet::THEMES) {
        themes_sub(b, core);
    }
    if opts.sections.contains(SectionSet::ENDGAMES) {
        endgames_sub(b, core);
    }
}

/// The subsection of the positional themes.
pub(super) fn themes_sub(b: &mut Builder<'_>, core: &ReportCore) {
    let l = b.labels;
    b.sub(&l.themes);
    b.para(fill(&l.theme_description, THEME_PLIES / 2, None));
    let half = NUM_THEMES.div_ceil(2);
    let rows = (0..half)
        .map(|i| {
            let cell = |t: usize| -> [String; 2] {
                match l.theme_names.get(t) {
                    Some(name) => [name.clone(), format!("{}%", core.theme_percent(t))],
                    None => [String::new(), String::new()],
                }
            };
            let [a, pa] = cell(i);
            let [c, pc] = cell(i + half);
            vec![a, pa, c, pc]
        })
        .collect();
    b.blocks.push(Block::Grid(Grid {
        aligns: vec![Align::Left, Align::Right, Align::Left, Align::Right],
        header: Vec::new(),
        rows,
    }));
}

/// The subsection of the endgame classes.
pub(super) fn endgames_sub(b: &mut Builder<'_>, core: &ReportCore) {
    let l = b.labels;
    b.sub(&l.endgames);
    b.para(format!("{}:", l.end_class));
    let mut header = vec![String::new()];
    header.extend(EndTheme::ALL.iter().map(|t| t.short_name().to_owned()));
    let row = |label: &str, all: bool| {
        let mut cells = vec![label.to_owned()];
        cells
            .extend((0..EndTheme::ALL.len()).map(|i| format!("{}%", core.endgame_percent(i, all))));
        cells
    };
    b.blocks.push(Block::Grid(Grid {
        aligns: std::iter::once(Align::Left)
            .chain(std::iter::repeat_n(Align::Right, 8))
            .collect(),
        header: vec![header],
        rows: vec![row(&l.report_games, false), row(&l.all_games, true)],
    }));
}

fn tree_grid(report: &OpeningReport, l: &ReportLabels) -> Grid {
    let header = vec![
        String::new(),
        l.tree_move.clone(),
        l.tree_frequency.clone(),
        "%".into(),
        l.score.clone(),
        format!("% {}", l.tree_draws),
        l.tree_elo.clone(),
        l.tree_perf.clone(),
        l.tree_year.clone(),
        l.eco.clone(),
    ];
    let opt = |v: Option<u32>| v.map_or_else(String::new, |n| n.to_string());
    let mut rows = Vec::new();
    let total_games = report.tree.total.as_ref().map_or(0, |t| t.total);
    for (i, r) in report.tree.rows.iter().enumerate() {
        let (whole, tenth) = r.share_tenths(total_games);
        let (sp, st) = r.score_tenths();
        rows.push(vec![
            format!("{}", i + 1),
            r.san.clone(),
            r.total.to_string(),
            format!("{whole}{}{tenth}%", l.decimal),
            format!("{sp}{}{st}%", l.decimal),
            format!("{}%", r.draw_percent()),
            opt(r.avg_elo),
            opt(r.perf),
            opt(r.avg_year),
            r.eco.map_or_else(String::new, |e| e.to_string()),
        ]);
    }
    if let Some(t) = &report.tree.total {
        let (sp, st) = t.score_tenths();
        rows.push(vec![
            String::new(),
            l.tree_total.clone(),
            t.total.to_string(),
            format!("100{}0%", l.decimal),
            format!("{sp}{}{st}%", l.decimal),
            format!("{}%", t.draw_percent()),
            opt(t.avg_elo),
            opt(t.perf),
            opt(t.avg_year),
            String::new(),
        ]);
    }
    Grid {
        aligns: vec![
            Align::Right,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
        header: vec![header],
        rows,
    }
}
