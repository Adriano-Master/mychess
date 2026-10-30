//! M7-2a: the opening report engine on the report database of the spec
//! (`docs/specs/m7-2-relatorio-de-abertura.md`, acceptance criteria 1 to 11,
//! 13, 14, 16 and 17). Expected values are computed by hand from the rules
//! of the spec.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m7_2a_support.rs"]
mod support;

use mychess_core::{Color, GameResult};
use mychess_db::report::{BestKind, EndTheme, ReportOptions, SectionSet, Selector, select_games};
use mychess_db::tree::performance;
use mychess_db::{CancelToken, StatsQuery};
use pretty_assertions::assert_eq;
use support::{E4_E5_NF3, fixture, line_games, open, pgn_game, report, report_with};

fn default_report() -> mychess_db::report::OpeningReport {
    report(&mut fixture(), E4_E5_NF3, &ReportOptions::default())
}

#[test]
fn counts_the_five_games_that_continue_after_the_position() {
    let r = default_report();
    assert_eq!(line_games(&r), vec![1, 2, 3, 4, 6]);
    let t = r.core.total;
    assert_eq!((t.white, t.draws, t.black, t.unknown), (3, 1, 1, 0));
    assert_eq!(r.core.report_games(), 5);
    assert_eq!(r.core.skipped, 0);
    assert!(!r.core.interrupted);
    assert_eq!(r.core.database.games, 7);
    assert_eq!(r.game_count(), 5);
    // Game 5 ends at the position, game 7 never gets there.
    assert!(r.core.lines.iter().all(|l| l.game != 5 && l.game != 7));
    assert!(r.core.lines.iter().all(|l| l.start_ply == 3));
}

#[test]
fn score_frequency_and_lengths() {
    let c = default_report().core;
    assert_eq!(c.score_tenths(), 700);
    assert_eq!(c.freq_tenths(GameResult::White), 600);
    assert_eq!(c.freq_tenths(GameResult::Draw), 200);
    // 1-0 lines are games 1 (10 plies), 4 (5) and 6 (5): 5 + 3 + 3 over 3.
    assert_eq!(c.avg_length(GameResult::White), 3);
    assert_eq!(c.avg_length(GameResult::Unknown), 0);
    // The whole database: the same 1-0 games, rounded: (11 + 1) / 3.
    assert_eq!(c.all_avg_length(GameResult::White), 4);
    assert_eq!(c.all_freq_tenths(GameResult::White), 3 * 1000 / 7);
    assert_eq!(c.all_score_tenths(), (2 * 3 + 1) * 500 / 6);
}

#[test]
fn empty_database_has_zero_everywhere_without_dividing_by_zero() {
    let c = mychess_db::report::ReportCore::default();
    assert_eq!(c.score_tenths(), 0);
    assert_eq!(c.all_freq_tenths(GameResult::White), 0);
    assert_eq!(c.freq_tenths(GameResult::Draw), 0);
    assert_eq!(c.all_avg_length(GameResult::Black), 0);
    assert_eq!(c.theme_percent(3), 0);
    assert_eq!(c.endgame_percent(0, true), 0);
}

#[test]
fn rating_and_performance_of_each_colour() {
    let c = default_report().core;
    let white = c.elo_report(Color::White);
    assert_eq!((white.average, white.games), (2440, 5));
    let black = c.elo_report(Color::Black);
    assert_eq!((black.average, black.games), (2375, 4));
    // Games 1, 2, 3 and 6 give White 2 + 1 + 0 + 2 = 5 points of 4 games.
    assert_eq!(black.opponent_percent, (5 * 50 + 2) / 4);
    assert_eq!(black.opponent_percent, 63);
    assert_eq!(black.opponent_performance, performance(2375, 63));
    // Black against the 5 rated Whites: only game 3 is a Black win (2), game
    // 2 is a draw (1): 3 points, (3 * 50 + 2) / 5 = 30.
    assert_eq!(white.opponent_percent, 30);
    assert_eq!(white.opponent_performance, performance(2440, 30));
}

#[test]
fn most_frequent_white_players() {
    let c = default_report().core;
    let rows = c.top_players(Color::White, 3);
    let summary: Vec<(&str, u32)> = rows.iter().map(|p| (p.name.as_str(), p.games)).collect();
    assert_eq!(summary, [("Alfa, A", 3), ("Beta, B", 1), ("Gama, G", 1)]);
    let alfa = &rows[0];
    assert_eq!((alfa.first_year, alfa.last_year), (2019, 2020));
    assert_eq!((alfa.score_percent, alfa.max_elo), (100, 2500));
    // Beta (2400) before Gama (2300) by the higher rating; Delta never has
    // White in the lines.
    assert!(rows.iter().all(|p| p.name != "Delta, D"));
    assert_eq!(rows[0].rank, 1);
}

#[test]
fn most_frequent_black_players_score_from_the_black_side() {
    let c = default_report().core;
    let rows = c.top_players(Color::Black, 5);
    // Black in the lines: Beta (1), Alfa (2), Gama (3), Delta (4), Gama (6).
    assert_eq!(rows[0].name, "Gama, G");
    assert_eq!(rows[0].games, 2);
    // Gama: 0-1 as Black in game 3 (2 points) and 0-1 in game 6 (0): 1/2.
    assert_eq!(rows[0].score_percent, 50);
}

#[test]
fn move_orders_count_transpositions() {
    let c = default_report().core;
    assert_eq!(c.move_orders.len(), 2);
    assert_eq!(c.move_orders[0].moves, "1.e4 e5 2.Nf3");
    assert_eq!(c.move_orders[0].count, 4);
    assert_eq!(c.move_orders[1].moves, "1.Nf3 e5 2.e4");
    assert_eq!(c.move_orders[1].count, 1);
    // The ids refer to the lines.
    let id = c.move_orders[1].id;
    let by_id: Vec<u32> = c
        .lines
        .iter()
        .filter(|l| l.move_order == id)
        .map(|l| l.game)
        .collect();
    assert_eq!(by_id, vec![6]);
}

#[test]
fn endgames_count_report_games_and_all_games() {
    let c = default_report().core;
    let q = EndTheme::QueenRookMinor.index();
    assert_eq!(c.endgames.report[q], 5);
    assert_eq!(c.endgames.all[q], 7);
    assert_eq!(c.endgame_percent(q, false), 100);
    assert_eq!(c.endgame_percent(q, true), 100);
    assert!(
        c.lines
            .iter()
            .all(|l| l.end_theme == EndTheme::QueenRookMinor)
    );
}

#[test]
fn oldest_and_newest_games_skip_unknown_dates_and_break_ties_by_number() {
    let c = default_report().core;
    let nums = |kind, n| -> Vec<u32> { c.best(kind, n).iter().map(|l| l.game).collect() };
    assert_eq!(nums(BestKind::Oldest, 2), vec![2, 3]);
    assert_eq!(nums(BestKind::Newest, 2), vec![1, 6]);
    // The undated game 4 never is among the oldest, even when asking for all.
    assert_eq!(nums(BestKind::Oldest, 10), vec![2, 3, 6, 1]);
}

#[test]
fn highest_rated_and_shortest_wins() {
    let c = default_report().core;
    let nums = |kind, n| -> Vec<u32> { c.best(kind, n).iter().map(|l| l.game).collect() };
    let avg: Vec<(u32, u16)> = c.lines.iter().map(|l| (l.game, l.avg_elo)).collect();
    assert_eq!(avg, [(1, 2450), (2, 2400), (3, 2350), (4, 1250), (6, 2400)]);
    // Equal ratings: the higher number first.
    assert_eq!(nums(BestKind::HighRating, 5), vec![1, 6, 2, 3, 4]);
    // White wins: 4 and 6 (3 moves) before 1 (5); 6 has the higher rating.
    assert_eq!(nums(BestKind::Shortest(Color::White), 5), vec![6, 4, 1]);
    assert_eq!(nums(BestKind::Shortest(Color::Black), 5), vec![3]);
}

#[test]
fn tree_of_the_position_is_kept() {
    let r = default_report();
    let sans: Vec<&str> = r.tree.rows.iter().map(|x| x.san.as_str()).collect();
    assert!(sans.contains(&"Nc6") && sans.contains(&"d6") && sans.contains(&"[end]"));
    assert_eq!(r.tree.total.as_ref().unwrap().total, 6);
}

#[test]
fn stats_lines_are_all_both_rated_and_since() {
    let r = default_report();
    let kinds: Vec<StatsQuery> = r.core.stats.iter().map(|l| l.query).collect();
    assert_eq!(kinds[0], StatsQuery::All);
    assert!(kinds.contains(&StatsQuery::BothRated(2400)));
    assert!(kinds.contains(&StatsQuery::Since(2020)));
    assert!(!kinds.iter().any(|k| matches!(k, StatsQuery::Years { .. })));
    let all = r.core.stats[0].row;
    assert_eq!((all.total, all.white, all.draw, all.black), (5, 3, 1, 1));
    // Both players rated 2400+: games 1 (2500/2400) only; 3 has a 2300.
    let both = r
        .core
        .stats
        .iter()
        .find(|l| l.query == StatsQuery::BothRated(2400))
        .unwrap();
    assert_eq!(both.row.total, 1);
}

#[test]
fn theory_table_for_black_to_move() {
    let r = default_report();
    let c = &r.core;
    assert_eq!(r.stem, ["e4", "e5", "Nf3"]);
    assert_eq!(r.first_move_number(), 2);
    // Every line starts with the last move of the stem.
    assert!(c.lines.iter().all(|l| l.move_at(0) == "Nf3"));
    assert_eq!(c.lines[0].moves.join(" "), "Nf3 Nc6 Bb5 a6 Ba4 Nf6 OO Be7");
    assert_eq!((c.theory.white, c.theory.draws, c.theory.black), (3, 1, 1));
    assert_eq!(c.theory_points2(), 7);
    assert_eq!((c.theory_score_tenths() + 5) / 10, 70);
    // Most frequent move first; the short line merges into the next.
    let heads: Vec<u32> = c.rows.iter().map(|row| c.lines[row.line].game).collect();
    assert_eq!(heads, vec![1, 2, 4]);
    let first = &c.rows[0];
    let merged: Vec<u32> = first.merged.iter().map(|&i| c.lines[i].game).collect();
    assert_eq!(merged, vec![6, 3]);
    assert_eq!(
        (first.count, first.points, first.score_percent()),
        (3, 4, 67)
    );
    assert_eq!(c.rows.iter().map(|x| x.count).sum::<u32>(), 5);
    // Notes are numbered row by row, cell by cell.
    let anchors: Vec<(usize, usize)> = c.notes.iter().map(|n| (n.row, n.cell)).collect();
    assert_eq!(anchors, vec![(0, 3), (0, 4), (0, 7), (1, 3), (2, 2)]);
    assert_eq!(c.lines[c.rows[0].line].note, 3);
}

#[test]
fn white_to_move_table_starts_with_the_next_white_move() {
    let r = report(&mut fixture(), "e2e4 e7e5", &ReportOptions::default());
    assert_eq!(r.first_move_number(), 2);
    assert_eq!(r.core.report_games(), 5);
    // The move played from the position is the first cell, 2.Nf3.
    assert!(r.core.lines.iter().all(|l| l.move_at(0) == "Nf3"));
    assert_eq!(r.core.lines[0].move_at(1), "Nc6");
}

#[test]
fn black_to_move_after_one_move_shows_the_stem_move_first() {
    let r = report(&mut fixture(), "e2e4", &ReportOptions::default());
    assert_eq!(r.first_move_number(), 1);
    assert!(r.core.lines.iter().all(|l| l.move_at(0) == "e4"));
    assert!(r.core.lines.iter().all(|l| l.move_at(1) == "e5"));
}

#[test]
fn excluding_a_move_leaves_the_table_and_the_theory_counts_only() {
    let opts = ReportOptions {
        exclude: Some("Nc6".into()),
        ..ReportOptions::default()
    };
    let r = report(&mut fixture(), E4_E5_NF3, &opts);
    let c = &r.core;
    assert_eq!(r.excluded.as_deref(), Some("Nc6"));
    assert_eq!((c.theory.white, c.theory.draws, c.theory.black), (1, 0, 0));
    assert_eq!(c.theory_games(), 1);
    // The report counts stay.
    assert_eq!((c.total.white, c.total.draws, c.total.black), (3, 1, 1));
    assert_eq!(c.report_games(), 5);
    assert_eq!(line_games(&r), vec![4]);
    assert_eq!(c.stats[0].row.total, 5);
    // A move that does not exist is ignored.
    let none = ReportOptions {
        exclude: Some("Qh5".into()),
        ..ReportOptions::default()
    };
    let r = report(&mut fixture(), E4_E5_NF3, &none);
    assert_eq!(r.excluded, None);
    assert_eq!(r.core.theory_games(), 5);
}

#[test]
fn max_games_keeps_the_highest_rated_and_counts_all() {
    let opts = ReportOptions {
        max_games: 3,
        ..ReportOptions::default()
    };
    let r = report(&mut fixture(), E4_E5_NF3, &opts);
    assert_eq!(
        line_games(&r),
        vec![1, 2, 6],
        "2400 ties go to the lowest number"
    );
    assert_eq!(r.core.report_games(), 5);
    assert_eq!(r.core.score_tenths(), 700);
    let zero = ReportOptions {
        max_games: 0,
        ..ReportOptions::default()
    };
    let r = report(&mut fixture(), E4_E5_NF3, &zero);
    assert_eq!(line_games(&r), vec![1]);
}

#[test]
fn max_table_games_limits_the_table_not_the_lines() {
    let opts = ReportOptions {
        max_table_games: 2,
        ..ReportOptions::default()
    };
    let c = report(&mut fixture(), E4_E5_NF3, &opts).core;
    assert_eq!(c.lines.len(), 5);
    assert_eq!(c.table_order.len(), 2);
    let tabled: Vec<u32> = c.table_order.iter().map(|&i| c.lines[i].game).collect();
    assert!(tabled.contains(&1) && tabled.contains(&2) || tabled.contains(&6));
    assert_eq!(c.lines.iter().filter(|l| !l.moves.is_empty()).count(), 2);
    let off = ReportOptions {
        max_table_games: 0,
        ..ReportOptions::default()
    };
    let c = report(&mut fixture(), E4_E5_NF3, &off).core;
    assert!(c.rows.is_empty() && c.notes.is_empty() && c.table_order.is_empty());
}

#[test]
fn select_games_keeps_the_reference_ply() {
    let r = default_report();
    let n = r.core.database.games;
    let all = select_games(&r.core, Selector::All, n);
    assert_eq!(all.iter_games().collect::<Vec<_>>(), vec![1, 2, 3, 4, 6]);
    // Ply 3 of the position, stored as 4 in the filter.
    assert!(all.iter_games().all(|g| all.ply(g) == 3 && all.get(g) == 4));
    let order = r.core.move_orders[1].id;
    let by_order = select_games(&r.core, Selector::MoveOrder(order), n);
    assert_eq!(by_order.iter_games().collect::<Vec<_>>(), vec![6]);
    let note3 = select_games(&r.core, Selector::Note(3), n);
    assert_eq!(note3.iter_games().collect::<Vec<_>>(), vec![1]);
    let end = select_games(
        &r.core,
        Selector::Endgame(EndTheme::QueenRookMinor.index()),
        n,
    );
    assert_eq!(end.count(), 5);
    let none = select_games(&r.core, Selector::Endgame(0), n);
    assert_eq!(none.count(), 0);
    let theme = select_games(&r.core, Selector::Theme(99), n);
    assert_eq!(theme.count(), 0, "a theme out of range selects nothing");
}

#[test]
fn a_theme_needs_four_positions_in_the_first_twenty_moves() {
    // Black pawn on the 4th rank from ply 4: 4 positions in A, 3 in B.
    let a = "1. a3 e5 2. a4 e4 3. Nc3 Nf6 4. h3";
    let b = "1. a3 e5 2. a4 e4 3. Nc3 Nf6";
    let pgn = [
        pgn_game("A, A", "B, B", "*", "2000.01.01", "x", (0, 0), a),
        pgn_game("A, A", "B, B", "*", "2000.01.01", "x", (0, 0), b),
    ]
    .concat();
    let r = report(&mut open(&pgn), "", &ReportOptions::default());
    let t = mychess_db::report::Theme::BlackAdvancedPawn.index();
    assert_eq!(r.core.lines[0].themes[t], 4);
    assert_eq!(r.core.lines[1].themes[t], 3);
    assert_eq!(r.core.themes[t], 1);
    assert_eq!(r.core.theme_percent(t), 50);
    let sel = select_games(&r.core, Selector::Theme(t), 2);
    assert_eq!(sel.iter_games().collect::<Vec<_>>(), vec![1]);
    assert_eq!(sel.ply(1), 0);
}

#[test]
fn popularity_uses_the_injected_date() {
    let mut pgn = fixture_without_game_7();
    for _ in 0..197 {
        pgn.push_str(&pgn_game(
            "X, X",
            "Y, Y",
            "1-0",
            "2012.01.01",
            "x",
            (0, 0),
            "1. d4 d5",
        ));
    }
    for _ in 0..97 {
        pgn.push_str(&pgn_game(
            "X, X",
            "Y, Y",
            "1-0",
            "2022.02.02",
            "x",
            (0, 0),
            "1. d4 d5",
        ));
    }
    for _ in 0..400 {
        pgn.push_str(&pgn_game(
            "X, X",
            "Y, Y",
            "1-0",
            "????.??.??",
            "x",
            (0, 0),
            "1. d4 d5",
        ));
    }
    let mut db = open(&pgn);
    assert_eq!(mychess_db::GameSource::len(&db), 700);
    let r = report(&mut db, E4_E5_NF3, &ReportOptions::default());
    let p = &r.core.popularity;
    assert_eq!(p.ratio_all, 140);
    // 2010-19: 3 report games of 200; 2020-29: 1 of 99; the rest none.
    assert_eq!(p.ranges[7], Some(200 / 3));
    assert_eq!(p.ranges[8], Some(99));
    assert_eq!(p.ranges[0], None);
    assert_eq!(p.ranges[6], None);
    // 10 years (from 2016.10.29): 3 report games of 101.
    let ten = p.recent[0];
    assert_eq!((ten.years, ten.ratio), (10, 33));
    assert_eq!(ten.delta, 324);
    // 5 years and 1 year: no report game, ratio 0 and delta 0.
    assert_eq!((p.recent[1].ratio, p.recent[1].delta), (0, 0));
    assert_eq!((p.recent[2].ratio, p.recent[2].delta), (0, 0));
}

/// The six first games of the fixture (the seventh is left out so the
/// database has exactly 700 games in the popularity test: 6 + 694).
fn fixture_without_game_7() -> String {
    let all = support::fixture_pgn();
    let cut = all.rfind("[Event").unwrap();
    all[..cut].to_owned()
}

#[test]
fn popularity_goes_down_when_the_recent_ratio_is_larger() {
    // 3 report games in 23: once every 7. In the last years only one report
    // game in 21: once every 21, "down" by (7 / 21 - 1) * 100 + 0.5 = -66.
    let mut pgn = String::new();
    for date in ["2000.01.01", "2000.01.01", "2026.01.01"] {
        pgn.push_str(&pgn_game(
            "A, A",
            "B, B",
            "1-0",
            date,
            "x",
            (0, 0),
            "1. e4 e5",
        ));
    }
    for _ in 0..20 {
        pgn.push_str(&pgn_game(
            "A, A",
            "B, B",
            "1-0",
            "2026.01.01",
            "x",
            (0, 0),
            "1. d4 d5",
        ));
    }
    let r = report(&mut open(&pgn), "e2e4", &ReportOptions::default());
    let p = &r.core.popularity;
    assert_eq!(p.ratio_all, 7);
    for t in p.recent {
        assert_eq!((t.ratio, t.delta), (21, -66), "{t:?}");
    }
}

#[test]
fn cancelled_before_starting_gives_an_interrupted_empty_report() {
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = report_with(
        &mut fixture(),
        E4_E5_NF3,
        &ReportOptions::default(),
        &mut mychess_db::NoProgress,
        &cancel,
    );
    assert!(r.core.interrupted);
    assert_eq!(r.core.lines.len(), 0);
    assert_eq!(r.core.report_games(), 0);
}

#[test]
fn cancelled_in_the_middle_gives_a_coherent_partial_report_and_monotonic_progress() {
    let mut pgn = String::new();
    for _ in 0..5000 {
        pgn.push_str(&pgn_game(
            "A, A",
            "B, B",
            "1-0",
            "2000.01.01",
            "x",
            (0, 0),
            "1. e4 e5 2. Nf3",
        ));
        pgn.push_str(&pgn_game(
            "A, A",
            "B, B",
            "1-0",
            "2000.01.01",
            "x",
            (0, 0),
            "1. e4 e5 2. Nf3 Nc6",
        ));
    }
    let mut db = open(&pgn);
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| {
        seen.push((done, total));
        token.cancel();
    };
    let r = report_with(
        &mut db,
        E4_E5_NF3,
        &ReportOptions::default(),
        &mut sink,
        &cancel,
    );
    assert!(r.core.interrupted);
    let total = r.core.report_games();
    assert!(total > 0 && total < 5000, "partial: {total}");
    // The data agree with each other.
    assert_eq!(u32::try_from(r.core.lines.len()).unwrap(), total);
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {seen:?}"
    );
    assert!(seen.iter().all(|(d, t)| d <= t));
}

#[test]
fn progress_is_monotonic_and_ends_at_the_total() {
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| seen.push((done, total));
    report_with(
        &mut fixture(),
        E4_E5_NF3,
        &ReportOptions::default(),
        &mut sink,
        &CancelToken::new(),
    );
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    let (done, total) = *seen.last().unwrap();
    assert_eq!(done, total);
}

#[test]
fn a_filter_of_another_database_is_an_error() {
    use mychess_db::report::{OpeningInputs, ReportError, opening_report};
    let mut db = fixture();
    let (pos, stem) = support::line(E4_E5_NF3);
    let wrong = mychess_db::Filter::new(3);
    let stats = mychess_db::PlayerStats::from_database(&db);
    let main = db.new_filter();
    let tree = mychess_db::tree::search(
        &mut db,
        &mychess_db::tree::TreeQuery {
            position: &pos,
            ply: 3,
            main_filter: &main,
        },
        &mychess_db::tree::TreeOptions::default(),
        &mut mychess_db::tree::TreeState::default(),
        &mut mychess_db::tree::NoCache,
        &CancelToken::new(),
        &mut mychess_db::NoProgress,
    )
    .unwrap();
    let opts = ReportOptions::default();
    let err = opening_report(
        &mut db,
        OpeningInputs {
            position: &pos,
            start: None,
            stem: &stem,
            tree_filter: &wrong,
            tree,
            options: &opts,
            eco_book: None,
            player_stats: &stats,
            stats_config: None,
            today: support::today(),
        },
        &mut mychess_db::NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ReportError::FilterSize {
            filter: 3,
            games: 7
        }
    ));
}

#[test]
fn a_stem_that_does_not_lead_to_the_position_is_an_error() {
    use mychess_db::report::{OpeningInputs, ReportError, opening_report};
    let mut db = fixture();
    let (pos, _) = support::line(E4_E5_NF3);
    let (_, other_stem) = support::line("d2d4");
    let stats = mychess_db::PlayerStats::from_database(&db);
    let main = db.new_filter();
    let mut state = mychess_db::tree::TreeState::default();
    let tree = mychess_db::tree::search(
        &mut db,
        &mychess_db::tree::TreeQuery {
            position: &pos,
            ply: 3,
            main_filter: &main,
        },
        &mychess_db::tree::TreeOptions::default(),
        &mut state,
        &mut mychess_db::tree::NoCache,
        &CancelToken::new(),
        &mut mychess_db::NoProgress,
    )
    .unwrap();
    let filter = state.tree_filter().unwrap().clone();
    let opts = ReportOptions::default();
    let err = opening_report(
        &mut db,
        OpeningInputs {
            position: &pos,
            start: None,
            stem: &other_stem,
            tree_filter: &filter,
            tree,
            options: &opts,
            eco_book: None,
            player_stats: &stats,
            stats_config: None,
            today: support::today(),
        },
        &mut mychess_db::NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(err, ReportError::StemMismatch));
}

#[test]
fn the_result_is_the_same_with_one_thread_and_many() {
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| report(&mut fixture(), E4_E5_NF3, &ReportOptions::default()).core)
    };
    assert_eq!(run(1), run(4));
}

#[test]
fn sections_off_do_not_change_the_data() {
    let opts = ReportOptions {
        sections: SectionSet::empty(),
        oldest: 0,
        newest: 0,
        ..ReportOptions::default()
    };
    let r = report(&mut fixture(), E4_E5_NF3, &opts);
    assert_eq!(r.core.report_games(), 5);
}
