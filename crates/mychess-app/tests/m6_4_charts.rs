//! Contract tests of the statistics panel, the graphs of the filter and the
//! rating graph (spec `m6-4-tabela-estatisticas-rastreador.md`, slice b:
//! D3352, D3370, D3371, D3375, D3391, D3394, D3402 to D3404).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_4_common;

use m6_4_common::*;
use mychess_app::charts::{ChartsNotice, FilterGraphKind, GraphField, StatsLabel, StatsTab};
use mychess_app::{load_preferences, render_preferences};

fn find_stats(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Stats { .. })))
        .cloned()
}

fn find_rating(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Rating { .. })))
        .cloned()
}

fn opened() -> (Rig, SlotId, tempfile::TempDir) {
    let (mut rig, slot, dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    rig.send(ChartsMessage::OpenStats);
    (rig, slot, dir)
}

#[test]
fn opening_without_the_year_asks_for_it_then_computes() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(ChartsMessage::OpenStats);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Charts(ChartsEffect::QueryYear))),
        "{effects:?}"
    );
    assert!(find_stats(&effects).is_none(), "waits for the year");
    // A fresh state, driven through `perform`: the year comes back.
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::OpenStats);
    assert!(rig.state.charts().stats_is_open());
    assert!(!rig.state.stats_view().lines.is_empty());
}

#[test]
fn the_table_counts_the_filter_with_the_given_year() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2013));
    let effects = rig.raw(ChartsMessage::OpenStats);
    let Some(Effect::Charts(ChartsEffect::Stats { config, filter, .. })) = find_stats(&effects)
    else {
        panic!("no stats effect: {effects:?}");
    };
    assert_eq!(filter.count(), 5);
    assert_eq!(config.years.get(&2013), Some(&true));
    assert_eq!(
        config.years.get(&2014),
        None,
        "the year is an input, not the clock"
    );
    rig.send(ChartsMessage::OpenStats);
    let view = rig.state.stats_view();
    let all = view.lines[0];
    assert_eq!(all.label, StatsLabel::All);
    // Games 1, 3, 5 white wins; 2 draw; 4 black win.
    assert_eq!((all.total, all.white, all.draw, all.black), (5, 3, 1, 1));
    // (2 * 3 + 1) * 500 / 5 = 700 tenths of a percent.
    assert_eq!(all.score_tenths, 700);
    assert_eq!(all.score_text(), "70.0%");
}

#[test]
fn rating_lines_use_both_ratings() {
    let (mut rig, _slot, _dir) = opened();
    let view = rig.state.stats_view();
    let both_2400 = view
        .lines
        .iter()
        .find(|l| l.label == StatsLabel::BothRated(2400))
        .expect("the default threshold 2400");
    assert_eq!(both_2400.total, 0);
    rig.send(ChartsMessage::SetStatsElo(2200, true));
    let line = rig
        .state
        .stats_view()
        .lines
        .into_iter()
        .find(|l| l.label == StatsLabel::BothRated(2200))
        .unwrap();
    // Games 1, 2, 3 and 5 have both ratings at 2200 or more.
    assert_eq!(line.total, 4);
}

#[test]
fn the_choice_of_lines_is_saved_and_comes_back() {
    let (mut rig, _slot, _dir) = opened();
    rig.send(ChartsMessage::SetStatsElo(2400, false));
    rig.send(ChartsMessage::SetStatsYear(2020, true));
    rig.fire_delays();
    let text = render_preferences(&rig.state.prefs);
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.stats, *rig.state.prefs.stats);
    let options = rig.state.stats_view().options.unwrap();
    assert!(options.elo.contains(&(2400, false)));
    assert!(options.years.contains(&(2020, true)));
    assert!(
        !rig.state
            .stats_view()
            .lines
            .iter()
            .any(|l| l.label == StatsLabel::BothRated(2400))
    );
    rig.send(ChartsMessage::ResetStats);
    assert!(rig.state.prefs.stats.elo_checked.is_none());
}

#[test]
fn a_change_of_the_filter_recomputes_after_the_debounce() {
    let (mut rig, slot, _dir) = opened();
    assert_eq!(rig.state.stats_view().lines[0].total, 5);
    rig.negate_filter(slot);
    // The filter is empty now; the table on show is the old one.
    let view = rig.state.stats_view();
    assert!(view.stale);
    assert_eq!(view.lines[0].total, 5);
    assert!(
        rig.delays
            .iter()
            .any(|e| matches!(e, Effect::Charts(ChartsEffect::Delay { .. })))
    );
    rig.fire_delays();
    let view = rig.state.stats_view();
    assert!(!view.stale);
    assert_eq!(view.lines[0].total, 0);
    assert_eq!(view.filter_games, 0);
}

#[test]
fn a_change_of_database_recomputes_for_the_new_one() {
    let (mut rig, _slot, dir) = opened();
    let other = rig.open_games(dir.path(), "two.pgn", &GAMES[..2]);
    assert_eq!(rig.state.bases.current(), other);
    rig.fire_delays();
    let view = rig.state.stats_view();
    assert_eq!(view.lines[0].total, 2);
    assert_eq!(view.base_games, 2);
}

#[test]
fn a_stale_answer_is_dropped() {
    let (mut rig, slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    let first = rig.raw(ChartsMessage::OpenStats);
    let old = find_stats(&first).unwrap();
    let Effect::Charts(ChartsEffect::Stats { control, .. }) = &old else {
        unreachable!()
    };
    let control = control.clone();
    // Before the answer the filter changes: a new computation, the old one
    // is cancelled and its answer is not used.
    let second = rig.raw(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(control.is_cancelled());
    let new = find_stats(&second).expect("recomputed at once: no result to wait for");
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(
        rig.state.stats_view().lines.is_empty(),
        "the old answer is dropped"
    );
    let answer = rig.perform(new);
    rig.raw(answer);
    assert_eq!(rig.state.stats_view().lines[0].total, 0);
}

#[test]
fn the_graph_is_computed_only_on_its_tab() {
    let (mut rig, _slot, _dir) = opened();
    assert!(rig.state.stats_view().graph.is_none());
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Absolute));
    let graph = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Graph { .. })))
        .expect("a graph effect on its tab");
    let answer = rig.perform(graph);
    rig.raw(answer);
    let view = rig.state.stats_view();
    let graph = view.graph.expect("graph");
    assert!(!graph.relative);
    assert_eq!(graph.kind, FilterGraphKind::Year);
    // Years 1995..=2026 by 1.
    assert_eq!(graph.points.len(), 32);
    let by_label = |l: &str| graph.points.iter().find(|p| p.label == l).map(|p| p.value);
    assert_eq!(by_label("2020"), Some(3.0));
    assert_eq!(by_label("2021"), Some(2.0));
    assert!(graph.axis.max >= 3.0);
    assert_eq!(graph.title_key, "graph-title-absolute");
}

#[test]
fn the_relative_graph_is_per_thousand_of_the_base() {
    let (mut rig, slot, _dir) = opened();
    rig.send(ChartsMessage::SetGraphKind(FilterGraphKind::Year));
    rig.send(ChartsMessage::SelectTab(StatsTab::Relative));
    // Keep only the games of the tournament in the filter.
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::SetFilter(None));
    rig.fire_delays();
    let graph = rig.state.stats_view().graph.expect("graph");
    assert!(graph.relative);
    let p2020 = graph.points.iter().find(|p| p.label == "2020").unwrap();
    // 3 of the 3 games of 2020 are in the filter: 1000 per thousand,
    // limited to 999.9.
    assert!((p2020.value - 999.9).abs() < 0.2, "{p2020:?}");
    let p2021 = graph.points.iter().find(|p| p.label == "2021").unwrap();
    assert!(p2021.value.abs() < f32::EPSILON);
    assert_eq!(graph.title_key, "graph-title-relative");
    let _ = slot;
}

#[test]
fn the_kind_and_the_axis_change_the_query_and_are_saved() {
    let (mut rig, _slot, _dir) = opened();
    rig.send(ChartsMessage::SelectTab(StatsTab::Absolute));
    rig.send(ChartsMessage::SetGraphKind(FilterGraphKind::Decade));
    let graph = rig.state.stats_view().graph.unwrap();
    assert_eq!(graph.points.len(), 11, "eleven decades (D3400)");
    assert_eq!(graph.points.last().map(|p| p.label.as_str()), Some("2010+"));
    assert_eq!(graph.points.last().map(|p| p.value), Some(5.0));
    rig.send(ChartsMessage::SetGraphKind(FilterGraphKind::Year));
    rig.send(ChartsMessage::SetGraphField {
        kind: FilterGraphKind::Year,
        field: GraphField::Min,
        value: 2020,
    });
    rig.send(ChartsMessage::SetGraphField {
        kind: FilterGraphKind::Year,
        field: GraphField::Max,
        value: 2021,
    });
    assert_eq!(rig.state.stats_view().graph.unwrap().points.len(), 2);
    // Out of range values are limited to four digits.
    rig.send(ChartsMessage::SetGraphField {
        kind: FilterGraphKind::Elo,
        field: GraphField::Max,
        value: 1_000_000,
    });
    assert_eq!(rig.state.prefs.filter_graph.elo_max, 9999);
    let text = render_preferences(&rig.state.prefs);
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.filter_graph, *rig.state.prefs.filter_graph);
    assert_eq!(loaded.prefs.filter_graph.tab, StatsTab::Absolute);
    rig.send(ChartsMessage::ResetGraphAxes);
    assert_eq!(rig.state.prefs.filter_graph.year_min, 1995);
    assert_eq!(rig.state.prefs.filter_graph.year_max, 2026);
}

#[test]
fn the_rating_graph_starts_with_the_players_of_the_game() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(ChartsMessage::OpenRating);
    let Some(Effect::Charts(ChartsEffect::Rating {
        players, options, ..
    })) = find_rating(&effects)
    else {
        panic!("no rating effect: {effects:?}");
    };
    assert_eq!(
        players,
        [
            mychess_db::RatingPlayer::Name("A".into()),
            mychess_db::RatingPlayer::Name("B".into())
        ]
    );
    assert_eq!(options.start_year, 1900);
    rig.send(ChartsMessage::OpenRating);
    let view = rig.state.rating_view();
    assert_eq!(view.players, ["A", "B"]);
    let graph = view.graph.expect("graph");
    assert!(!graph.empty);
    // A: last game of May 2020 is game 2 (Elo 2410); x = 2020 + 33/100.
    let a = &graph.lines[0];
    assert_eq!(a.name, "A");
    assert!(a.points.contains(&(2020.33, 2410)), "{:?}", a.points);
    assert!(
        a.points
            .iter()
            .any(|&(x, e)| (x - 2021.5).abs() < 0.01 && e == 2420)
    );
    assert_ne!(graph.lines[0].color, graph.lines[1].color);
}

#[test]
fn players_are_added_removed_and_limited_to_nine() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::AddPlayer("?".into()));
    rig.send(ChartsMessage::AddPlayer("  ".into()));
    rig.send(ChartsMessage::AddPlayer("A".into()));
    assert_eq!(rig.state.charts().players(), ["A", "B"]);
    rig.send(ChartsMessage::TogglePlayer("C".into()));
    assert_eq!(rig.state.charts().players(), ["A", "B", "C"]);
    rig.send(ChartsMessage::TogglePlayer("A".into()));
    rig.send(ChartsMessage::RemovePlayer("B".into()));
    assert_eq!(rig.state.charts().players(), ["C"]);
    for n in 0..12 {
        rig.send(ChartsMessage::AddPlayer(format!("P{n}")));
    }
    assert_eq!(rig.state.charts().players().len(), 9);
    assert!(!rig.state.rating_view().can_add);
    assert_eq!(
        rig.state.rating_view().issue,
        Some(ChartsNotice::TooManyPlayers)
    );
    rig.send(ChartsMessage::ClearPlayers);
    let view = rig.state.rating_view();
    assert!(view.players.is_empty() && view.graph.is_none());
    rig.send(ChartsMessage::UseGamePlayers);
    assert_eq!(rig.state.charts().players(), ["A", "B"]);
}

#[test]
fn a_player_that_is_not_in_the_base_has_an_empty_line() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::ClearPlayers);
    rig.send(ChartsMessage::AddPlayer("Nobody".into()));
    let graph = rig.state.rating_view().graph.unwrap();
    assert!(graph.empty);
    assert!(graph.lines[0].points.is_empty());
}

#[test]
fn the_options_of_the_rating_graph_recompute_and_are_saved() {
    let (mut rig, slot, _dir) = sample();
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::SetRatingStartYear(2021));
    let a = rig.state.rating_view().graph.unwrap().lines[0]
        .points
        .clone();
    assert!(a.iter().all(|&(x, _)| x >= 2021.0), "{a:?}");
    rig.send(ChartsMessage::SetRatingStartYear(1900));
    rig.send(ChartsMessage::SetRatingStartElo(2415));
    let a = rig.state.rating_view().graph.unwrap().lines[0]
        .points
        .clone();
    assert_eq!(a, [(2021.5, 2420)]);
    rig.send(ChartsMessage::SetRatingStartElo(0));
    // Only the games of the filter count.
    rig.send(ChartsMessage::SetRatingOnlyFilter(true));
    rig.negate_filter(slot);
    rig.fire_delays();
    assert!(rig.state.rating_view().graph.unwrap().empty);
    let text = render_preferences(&rig.state.prefs);
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.rating_graph, rig.state.prefs.rating_graph);
    assert!(loaded.prefs.rating_graph.only_filter);
}

#[test]
fn a_stale_rating_answer_is_dropped() {
    let (mut rig, _slot, _dir) = sample();
    let first = rig.raw(ChartsMessage::OpenRating);
    let old = find_rating(&first).unwrap();
    let second = rig.raw(ChartsMessage::AddPlayer("C".into()));
    // No answer yet: a player added restarts at once.
    let second = if let Some(e) = find_rating(&second) {
        e
    } else {
        // The refresh happens at the end of any update.
        find_rating(&rig.raw(ChartsMessage::RefreshRating)).expect("rating effect")
    };
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(rig.state.rating_view().graph.is_none());
    let answer = rig.perform(second);
    rig.raw(answer);
    assert_eq!(rig.state.rating_view().graph.unwrap().lines.len(), 3);
}

#[test]
fn closing_and_quit_stop_the_computations() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    let effects = rig.raw(ChartsMessage::OpenStats);
    let Some(Effect::Charts(ChartsEffect::Stats { control, .. })) = find_stats(&effects) else {
        panic!("no stats");
    };
    rig.raw(ChartsMessage::CloseStats);
    assert!(control.is_cancelled());
    let effects = rig.raw(ChartsMessage::OpenRating);
    let Some(Effect::Charts(ChartsEffect::Rating { control, .. })) = find_rating(&effects) else {
        panic!("no rating");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
}

#[test]
fn a_closed_panel_costs_nothing() {
    let (mut rig, slot, _dir) = sample();
    let effects = rig.raw(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(effects.iter().all(|e| !matches!(e, Effect::Charts(_))));
}
