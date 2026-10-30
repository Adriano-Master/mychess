//! Builds the blocks of the player report (the body of the Scid's
//! `::preport::report`), reusing the blocks of the opening report. Every
//! figure is seen from the player (decision M7-3/D3652).

use mychess_core::Color;

use super::blocks::{
    Builder, endgames_sub, games, players_grid_of, results_grid_of, stats_grid_of, stem_line,
    themes_sub,
};
use super::{Align, Block, Grid, ReportPart, StemView, theory};
use crate::report::labels::{fill, thousands};
use crate::report::player::{PlayerLabels, PlayerReport, PlayerReportOptions, natural_name};
use crate::report::stats::BestKind;

/// Builds the document.
pub(super) fn build(
    report: &PlayerReport,
    opts: &PlayerReportOptions,
    labels: &PlayerLabels,
    part: ReportPart,
) -> Vec<Block> {
    let l = &labels.base;
    let ropts = opts.to_report_options();
    let mut b = Builder {
        blocks: Vec::new(),
        labels: l,
        section: 0,
        sub: 0,
    };
    let core = &report.core;
    let color = report.color;
    let line = StemView {
        stem: &report.stem,
        omitted: report.stem_omitted,
        excluded: None,
    };
    header(&mut b, report, labels, &line, part);
    if part == ReportPart::TableOnly {
        theory::push(&mut b.blocks, core, &line, color, &ropts, l, false);
        return b.blocks;
    }
    if core.report_games() == 0 {
        return b.blocks;
    }
    let marks = [&labels.wins, &labels.draws, &labels.losses].map(String::clone);
    if opts.has_stats_section() {
        b.section(&l.stats_hist);
        if opts.stats {
            b.sub(&l.stats);
            b.blocks
                .push(Block::Grid(stats_grid_of(core, l, color, marks.clone())));
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
        if opts.opponents > 0 {
            b.sub(&labels.most_frequent_opponents);
            let rows = report
                .opponents
                .iter()
                .take(usize::from(opts.opponents))
                .cloned()
                .collect();
            b.blocks.push(Block::Grid(players_grid_of(rows)));
        }
        if opts.results {
            b.sub(&l.results);
            b.blocks
                .push(Block::Grid(results_grid_of(core, l, color, &marks)));
        }
    }
    if opts.has_ratings_section() {
        ratings(&mut b, report, opts, labels);
    }
    if opts.has_moves_section() {
        b.section(&l.moves_themes);
        if opts.eco_codes > 0 {
            b.sub(&labels.openings);
            b.blocks.push(Block::Grid(eco_grid(report, opts)));
        }
        if opts.themes {
            themes_sub(&mut b, core);
        }
        if opts.endgames {
            endgames_sub(&mut b, core);
        }
    }
    if part == ReportPart::Full && opts.max_table_games > 0 {
        b.section(&l.theory_table);
        theory::push(&mut b.blocks, core, &line, color, &ropts, l, true);
    }
    b.blocks
}

fn header(
    b: &mut Builder<'_>,
    report: &PlayerReport,
    labels: &PlayerLabels,
    line: &StemView<'_>,
    part: ReportPart,
) {
    let l = &labels.base;
    let core = &report.core;
    let display = natural_name(&report.name);
    b.blocks.push(Block::Title {
        text: display.clone(),
        line: Vec::new(),
    });
    if let Some(position) = &report.position {
        b.blocks.push(Block::Board {
            fen: position.fen(),
        });
    }
    let sep = &l.thousands;
    let color_text = match report.color {
        Color::White => &labels.with_white,
        Color::Black => &labels.with_black,
    };
    let mut player = format!("{display} {color_text}");
    // "after" and the line only when there is a line: a report of the start
    // position has an empty stem.
    let stem = stem_line(line);
    if report.position.is_some() && !stem.is_empty() {
        player.push(' ');
        player.push_str(&fill(&labels.after, stem.join(" "), None));
    }
    let mut rows = vec![
        (labels.player.clone(), player),
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
            format!("{} {}", thousands(core.report_games(), sep), l.games),
        ),
    ];
    if let Some(eco) = &report.eco {
        rows.push((l.eco.clone(), eco.clone()));
    }
    rows.push((l.generated.clone(), format!("{}, {}", l.generator, l.date)));
    b.blocks.push(Block::Header(rows));
    b.para(fill(&labels.pov, &display, None));
    if core.interrupted {
        b.para(l.partial.clone());
    }
    if core.skipped > 0 {
        b.para(fill(&l.skipped, core.skipped, None));
    }
    if core.report_games() == 0 && part != ReportPart::TableOnly {
        b.para(fill(&labels.none, &display, None));
    }
}

fn ratings(
    b: &mut Builder<'_>,
    report: &PlayerReport,
    opts: &PlayerReportOptions,
    labels: &PlayerLabels,
) {
    let l = &labels.base;
    let core = &report.core;
    b.section(&l.ratings_perf);
    if opts.avg_perf {
        b.sub(&l.avg_perf);
        let mine = core.elo_report(report.color);
        let theirs = core.elo_report(!report.color);
        // `opponent_*` of a colour is what the other colour scored against
        // it: the performance of the player is read in the opponents' report.
        b.para(format!(
            "{}: {} ({} {}); {}: {} ({}% vs {})",
            labels.player_rating,
            mine.average,
            mine.games,
            l.games,
            labels.player_perf,
            theirs.opponent_performance,
            theirs.opponent_percent,
            theirs.average
        ));
        b.para(format!(
            "{}: {} ({} {}); {}: {} ({}% vs {})",
            labels.opponents_rating,
            theirs.average,
            theirs.games,
            l.games,
            labels.opponents_perf,
            mine.opponent_performance,
            mine.opponent_percent,
            mine.average
        ));
    }
    if opts.high_rating > 0 {
        b.sub(&l.high_rating);
        b.blocks
            .push(games(core, BestKind::HighRating, opts.high_rating, false));
    }
}

fn eco_grid(report: &PlayerReport, opts: &PlayerReportOptions) -> Grid {
    let rows = report
        .eco_groups
        .iter()
        .take(usize::from(opts.eco_codes))
        .enumerate()
        .map(|(i, g)| {
            vec![
                format!("{}:", i + 1),
                format!("{}-{}", g.first, g.last),
                g.games.to_string(),
                format!("{}%", g.score_percent),
            ]
        })
        .collect();
    Grid {
        aligns: vec![Align::Right, Align::Left, Align::Right, Align::Right],
        header: Vec::new(),
        rows,
    }
}
