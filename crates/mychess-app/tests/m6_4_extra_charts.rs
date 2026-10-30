//! Extra contract tests of the statistics panel, the graphs of the filter and
//! the rating graph (spec `m6-4-tabela-estatisticas-rastreador.md`, sections
//! 2.2 to 2.4, rules S4, G6, R1 to R4, D3352, D3410, D3415, D3416, D3419,
//! D3420).
#![allow(
    let_underscore_drop,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::float_cmp
)]
mod m6_4_common;

use m6_4_common::*;
use mychess_app::charts::{ChartsNotice, FilterGraphKind, GraphField, StatsTab};
use mychess_app::load_preferences;

fn find_stats(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Stats { .. })))
        .cloned()
}

fn find_graph(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Graph { .. })))
        .cloned()
}

fn find_rating(effects: &[Effect]) -> Option<Effect> {
    effects
        .iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Rating { .. })))
        .cloned()
}

fn control_of(effect: &Effect) -> Option<OpenControl> {
    match effect {
        Effect::Charts(
            ChartsEffect::Stats { control, .. }
            | ChartsEffect::Graph { control, .. }
            | ChartsEffect::Rating { control, .. },
        )
        | Effect::Crosstable(CrosstableEffect::Compute { control, .. })
        | Effect::PieceTracker(PieceTrackerEffect::Track { control, .. }) => Some(control.clone()),
        _ => None,
    }
}

fn opened() -> (Rig, SlotId, tempfile::TempDir) {
    let (mut rig, slot, dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    rig.send(ChartsMessage::OpenStats);
    (rig, slot, dir)
}

fn stats_years(effect: &Effect) -> Vec<(u16, bool)> {
    let Effect::Charts(ChartsEffect::Stats { config, .. }) = effect else {
        panic!("not a stats effect");
    };
    config.years.iter().map(|(y, on)| (*y, *on)).collect()
}

/// The configuration of the graph asked for after two edits of one field
/// (the first one moves the key away from the default, so the second one is
/// always a new request).
fn graph_config(
    rig: &mut Rig,
    kind: FilterGraphKind,
    field: GraphField,
    first: i32,
    value: i32,
) -> mychess_db::FilterGraphConfig {
    rig.raw(ChartsMessage::SetGraphKind(kind));
    rig.raw(ChartsMessage::SetGraphField {
        kind,
        field,
        value: first,
    });
    let effects = rig.raw(ChartsMessage::SetGraphField { kind, field, value });
    let Some(Effect::Charts(ChartsEffect::Graph { config, .. })) = find_graph(&effects) else {
        panic!("no graph effect for {kind:?} {field:?} {value}: {effects:?}");
    };
    config
}

/// Games of one month and of unusual dates for the player `P`.
fn rating_base() -> [Spec; 9] {
    let g = |date, white, black, we, be| -> Spec {
        (
            "E",
            "S",
            date,
            "1",
            white,
            black,
            "1-0",
            we,
            be,
            "1. e4 1-0",
        )
    };
    [
        g("2020.01.05", "P", "Q", 2400, 2000),
        g("2020.01.20", "Q", "P", 2000, 2500),
        g("2020.01.28", "P", "Q", 2450, 2000),
        g("2020.01.29", "P", "Q", 0, 2000),
        g("2019.??.??", "P", "Q", 2300, 2000),
        g("2020.12.31", "P", "Q", 2470, 2000),
        g("????.??.??", "P", "Q", 2600, 2000),
        g("2020.07.01", "P", "Q", 2455, 2000),
        g("2021.10.01", "P", "Q", 2480, 2000),
    ]
}

fn points_of(rig: &Rig, line: usize) -> Vec<(f64, u16)> {
    rig.state.rating_view().graph.expect("graph").lines[line]
        .points
        .iter()
        .map(|&(x, e)| (f64::from(x), e))
        .collect()
}

#[test]
fn the_year_extends_the_default_years_and_keeps_what_the_user_unchecked() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2013));
    let effects = rig.raw(ChartsMessage::OpenStats);
    let years = stats_years(&find_stats(&effects).unwrap());
    // S4: 2011 up to the current year are checked by default.
    for y in [2011, 2012, 2013] {
        assert!(years.contains(&(y, true)), "{y}: {years:?}");
    }
    assert!(!years.iter().any(|(y, _)| *y == 2014));

    // The user unchecks 2012; a later year (the program runs across New
    // Year) extends the list but does not undo the choice.
    rig.send(ChartsMessage::SetStatsYear(2012, false));
    rig.send(ChartsMessage::SetYear(2015));
    rig.fire_delays();
    let effects = rig.raw(ChartsMessage::RefreshStats);
    let years = stats_years(&find_stats(&effects).expect("a refresh recomputes"));
    assert!(years.contains(&(2012, false)) || !years.iter().any(|(y, _)| *y == 2012));
    assert!(!years.contains(&(2012, true)), "{years:?}");
    for y in [2011, 2013, 2014, 2015] {
        assert!(years.contains(&(y, true)), "{y}: {years:?}");
    }
}

#[test]
fn the_year_never_comes_from_inside_update() {
    // Without `SetYear` no stats effect is ever produced: the panel asks for
    // the year once (`QueryYear`) and waits.
    let (mut rig, slot, _dir) = sample();
    let first = rig.raw(ChartsMessage::OpenStats);
    let queries = first
        .iter()
        .filter(|e| matches!(e, Effect::Charts(ChartsEffect::QueryYear)))
        .count();
    assert_eq!(queries, 1);
    let second = rig.raw(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(find_stats(&second).is_none());
    assert!(
        !second
            .iter()
            .any(|e| matches!(e, Effect::Charts(ChartsEffect::QueryYear))),
        "the year is asked for once"
    );
    // The year comes back as a message and the work starts.
    let effects = rig.raw(ChartsMessage::SetYear(2026));
    assert!(find_stats(&effects).is_some(), "{effects:?}");
}

#[test]
fn absurd_years_do_not_panic() {
    for year in [0u16, 1, 2010, 2011, 3000] {
        let (mut rig, _slot, _dir) = sample();
        rig.send(ChartsMessage::SetYear(year));
        rig.send(ChartsMessage::OpenStats);
        rig.send(ChartsMessage::SelectTab(StatsTab::Relative));
        let view = rig.state.stats_view();
        assert!(!view.lines.is_empty(), "{year}");
        assert_eq!(view.lines[0].total, 5, "{year}");
    }
    // The largest value is only checked for not panicking.
    let (mut rig, _slot, _dir) = sample();
    rig.raw(ChartsMessage::SetYear(u16::MAX));
    rig.raw(ChartsMessage::OpenStats);
    let _ = rig.state.stats_view();
}

#[test]
fn a_burst_of_filter_changes_computes_the_statistics_once() {
    let (mut rig, slot, _dir) = opened();
    rig.negate_filter(slot);
    rig.negate_filter(slot);
    rig.negate_filter(slot);
    let delays = std::mem::take(&mut rig.delays);
    assert_eq!(delays.len(), 3);
    let mut computes = 0;
    for delay in &delays {
        let message = Rig::elapsed(delay).unwrap();
        computes += rig
            .raw(message)
            .iter()
            .filter(|e| find_stats(std::slice::from_ref(e)).is_some())
            .count();
    }
    assert_eq!(computes, 1);
}

#[test]
fn a_stale_delay_after_the_panel_was_closed_starts_nothing() {
    let (mut rig, slot, _dir) = opened();
    rig.negate_filter(slot);
    let delay = rig.delays.pop().expect("a debounce");
    rig.raw(ChartsMessage::CloseStats);
    let effects = rig.raw(Rig::elapsed(&delay).unwrap());
    assert!(find_stats(&effects).is_none() && find_graph(&effects).is_none());
}

#[test]
fn closing_and_reopening_drops_the_answers_of_the_first_computation() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    let first = rig.raw(ChartsMessage::OpenStats);
    let old = find_stats(&first).unwrap();
    rig.raw(ChartsMessage::CloseStats);
    let second = rig.raw(ChartsMessage::OpenStats);
    let new = find_stats(&second).expect("a new computation");
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(rig.state.stats_view().lines.is_empty());
    let answer = rig.perform(new);
    rig.raw(answer);
    assert_eq!(rig.state.stats_view().lines[0].total, 5);

    let first = rig.raw(ChartsMessage::OpenRating);
    let old = find_rating(&first).unwrap();
    rig.raw(ChartsMessage::CloseRating);
    let second = rig.raw(ChartsMessage::OpenRating);
    let new = find_rating(&second).expect("a new computation");
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(rig.state.rating_view().graph.is_none());
    let answer = rig.perform(new);
    rig.raw(answer);
    assert!(rig.state.rating_view().graph.is_some());
}

#[test]
fn leaving_the_graph_tab_cancels_it_and_coming_back_starts_at_once() {
    let (mut rig, _slot, _dir) = opened();
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Absolute));
    let graph = find_graph(&effects).expect("graph");
    let control = control_of(&graph).unwrap();
    // The table tab does not need the graph.
    rig.raw(ChartsMessage::SelectTab(StatsTab::Table));
    assert!(control.is_cancelled());
    let answer = rig.perform(graph);
    rig.raw(answer);
    assert!(rig.state.stats_view().graph.is_none());
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Relative));
    assert!(find_graph(&effects).is_some(), "re-entering starts at once");
}

#[test]
fn relative_and_absolute_share_one_histogram() {
    let (mut rig, _slot, _dir) = opened();
    rig.send(ChartsMessage::SelectTab(StatsTab::Absolute));
    let absolute = rig.state.stats_view().graph.unwrap();
    assert!(!absolute.relative);
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Relative));
    assert!(
        find_graph(&effects).is_none(),
        "the same counts: no new computation"
    );
    assert!(rig.delays.is_empty());
    let relative = rig.state.stats_view().graph.unwrap();
    assert!(relative.relative);
    assert_eq!(relative.points.len(), absolute.points.len());
    assert!(!rig.state.stats_view().stale);
}

#[test]
fn the_estimate_box_recomputes_only_the_graph_not_the_table() {
    let (mut rig, _slot, _dir) = opened();
    rig.send(ChartsMessage::SelectTab(StatsTab::Absolute));
    rig.send(ChartsMessage::SetGraphKind(FilterGraphKind::Elo));
    let effects = rig.raw(ChartsMessage::SetEstimate(false));
    let Some(Effect::Charts(ChartsEffect::Graph { config, .. })) = find_graph(&effects) else {
        panic!("{effects:?}");
    };
    assert!(!config.estimate);
    assert!(find_stats(&effects).is_none());
}

#[test]
fn a_new_axis_cancels_the_graph_in_flight_and_drops_its_answer() {
    let (mut rig, _slot, _dir) = opened();
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Absolute));
    let old = find_graph(&effects).unwrap();
    let control = control_of(&old).unwrap();
    let effects = rig.raw(ChartsMessage::SetGraphField {
        kind: FilterGraphKind::Year,
        field: GraphField::Min,
        value: 2020,
    });
    assert!(control.is_cancelled());
    let new = find_graph(&effects).expect("restarts at once");
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(rig.state.stats_view().graph.is_none());
    let answer = rig.perform(new);
    rig.raw(answer);
    assert_eq!(rig.state.stats_view().graph.unwrap().points.len(), 7);
}

#[test]
fn the_axis_is_normalised_when_asked_for_g6() {
    let (mut rig, _slot, _dir) = opened();
    rig.raw(ChartsMessage::SelectTab(StatsTab::Absolute));
    let kind = FilterGraphKind::Year;
    let c = graph_config(&mut rig, kind, GraphField::Min, 2000, 0);
    assert_eq!(c.year_min, 1995, "year below 1 -> 1995");
    let c = graph_config(&mut rig, kind, GraphField::Step, 5, 0);
    assert_eq!(c.year_step, 1);
    let c = graph_config(&mut rig, kind, GraphField::Max, 2000, 0);
    assert_eq!(c.year_max, 2026, "no maximum -> the current year");

    let kind = FilterGraphKind::Moves;
    let c = graph_config(&mut rig, kind, GraphField::Min, 9, 0);
    assert_eq!(c.moves_min, 1);
    let c = graph_config(&mut rig, kind, GraphField::Step, 3, 0);
    assert_eq!(c.moves_step, 1);
    let c = graph_config(&mut rig, kind, GraphField::Max, 90, 0);
    assert_eq!(c.moves_max, 80);

    let kind = FilterGraphKind::Elo;
    let c = graph_config(&mut rig, kind, GraphField::Step, 50, 0);
    assert_eq!(c.elo_step, 100);
    let c = graph_config(&mut rig, kind, GraphField::Max, 2500, 0);
    assert_eq!(c.elo_max, 2800);

    // Negative and huge entries are limited before they reach the engine.
    let c = graph_config(&mut rig, kind, GraphField::Step, 50, -7);
    assert_eq!(c.elo_step, 100);
    rig.send(ChartsMessage::SetGraphField {
        kind,
        field: GraphField::Min,
        value: i32::MAX,
    });
    assert!(rig.state.prefs.filter_graph.elo_min <= 9999);
    rig.send(ChartsMessage::SetGraphField {
        kind,
        field: GraphField::Min,
        value: i32::MIN,
    });
    assert!(rig.state.prefs.filter_graph.elo_min >= 0);
    let _ = rig.state.stats_view();
}

#[test]
fn rating_names_are_trimmed_exact_and_unique() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::ClearPlayers);
    rig.send(ChartsMessage::AddPlayer("  A ".into()));
    rig.send(ChartsMessage::AddPlayer("A".into()));
    assert_eq!(rig.state.charts().players(), ["A"]);
    rig.send(ChartsMessage::AddPlayer("a".into()));
    assert_eq!(rig.state.charts().players(), ["A", "a"], "case matters");
    for hostile in ["", " ", "\t", "?", " ? "] {
        rig.send(ChartsMessage::AddPlayer(hostile.into()));
    }
    assert_eq!(rig.state.charts().players(), ["A", "a"]);
    rig.send(ChartsMessage::RemovePlayer("nobody".into()));
    assert_eq!(rig.state.charts().players(), ["A", "a"]);
    // Very long and non-ASCII names are only names.
    let long = "é".repeat(5000);
    rig.send(ChartsMessage::AddPlayer(long.clone()));
    assert_eq!(rig.state.charts().players()[2], long);
    let graph = rig.state.rating_view().graph.expect("graph");
    assert_eq!(graph.lines.len(), 3);
    assert!(graph.lines[2].points.is_empty(), "nobody has that name");
}

#[test]
fn at_nine_players_the_tenth_is_refused_and_a_toggle_frees_a_place() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::ClearPlayers);
    for n in 0..9 {
        rig.send(ChartsMessage::AddPlayer(format!("P{n}")));
    }
    let view = rig.state.rating_view();
    assert_eq!(view.players.len(), 9);
    assert!(!view.can_add);
    let graph = view.graph.expect("graph");
    assert_eq!(graph.lines.len(), 9);
    for i in 0..9 {
        for j in (i + 1)..9 {
            assert_ne!(graph.lines[i].color, graph.lines[j].color, "{i} {j}");
        }
    }
    rig.send(ChartsMessage::TogglePlayer("tenth".into()));
    assert_eq!(rig.state.charts().players().len(), 9);
    assert_eq!(
        rig.state.rating_view().issue,
        Some(ChartsNotice::TooManyPlayers)
    );
    // Toggling an existing one removes it and frees the place.
    rig.send(ChartsMessage::TogglePlayer("P4".into()));
    assert!(rig.state.rating_view().can_add);
    rig.send(ChartsMessage::TogglePlayer("tenth".into()));
    let players = rig.state.charts().players().to_vec();
    assert_eq!(players.len(), 9);
    assert_eq!(players.last().map(String::as_str), Some("tenth"));
    assert!(!players.contains(&"P4".to_owned()));
}

#[test]
fn the_players_of_the_game_skip_unknown_names_and_repeats() {
    let dir = tempfile::tempdir().unwrap();
    let g = |white, black| -> Spec {
        (
            "E",
            "S",
            "2020.01.01",
            "1",
            white,
            black,
            "1-0",
            2000,
            2000,
            "1. e4 1-0",
        )
    };
    let specs = [g("A", "?"), g("A", "A"), g("?", "?"), g("?", "B")];
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "g.pgn", &specs);

    rig.load(slot, 1);
    rig.send(ChartsMessage::OpenRating);
    assert_eq!(rig.state.charts().players(), ["A"]);

    rig.load(slot, 2);
    rig.send(ChartsMessage::UseGamePlayers);
    assert_eq!(rig.state.charts().players(), ["A"], "the same name twice");

    // Nobody known: the list stays and says why.
    rig.load(slot, 4);
    rig.send(ChartsMessage::UseGamePlayers);
    assert_eq!(rig.state.charts().players(), ["B"]);
    rig.load(slot, 3);
    rig.send(ChartsMessage::UseGamePlayers);
    assert_eq!(rig.state.charts().players(), ["B"]);
    assert_eq!(rig.state.rating_view().issue, Some(ChartsNotice::NoPlayers));
}

#[test]
fn opening_the_rating_graph_without_a_game_has_no_players_and_no_work() {
    let mut rig = Rig::new();
    let effects = rig.raw(ChartsMessage::OpenRating);
    assert!(find_rating(&effects).is_none());
    let view = rig.state.rating_view();
    assert!(view.players.is_empty() && view.graph.is_none());
    rig.send(ChartsMessage::AddPlayer("Z".into()));
    let _ = rig.state.rating_view();
}

#[test]
fn the_rating_of_a_month_is_the_last_game_with_a_rating() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_games(dir.path(), "r.pgn", &rating_base());
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::ClearPlayers);
    rig.send(ChartsMessage::AddPlayer("P".into()));
    let got = points_of(&rig, 0);
    // R2/R3: January 2020 keeps the last game with a rating (rating 0 does
    // not erase it); the game of 2019 with unknown month is January; the
    // unknown year is dropped.
    let expected = [
        (2019.0, 2300),
        (2020.0, 2450),
        (2020.5, 2455),
        (2020.91, 2470),
        (2021.75, 2480),
    ];
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for ((x, e), (ex, ee)) in got.iter().zip(expected) {
        assert!((x - ex).abs() < 1e-3 && *e == ee, "{got:?}");
    }

    // The start year and the start rating cut the series.
    rig.send(ChartsMessage::SetRatingStartYear(2020));
    let got = points_of(&rig, 0);
    assert_eq!(
        got.iter().map(|p| p.1).collect::<Vec<_>>(),
        [2450, 2455, 2470, 2480]
    );
    rig.send(ChartsMessage::SetRatingStartElo(2460));
    let got = points_of(&rig, 0);
    // R2: the minimum applies before "last of the month": January keeps the
    // 2500 of the second game, not the 2450 of the later one.
    assert_eq!(
        got.iter().map(|p| p.1).collect::<Vec<_>>(),
        [2500, 2470, 2480]
    );
    rig.send(ChartsMessage::SetRatingStartElo(3000));
    assert!(rig.state.rating_view().graph.unwrap().empty);
}

#[test]
fn the_rating_graph_ignores_the_filter_unless_asked() {
    let (mut rig, slot, _dir) = sample();
    rig.send(ChartsMessage::OpenRating);
    rig.send(ChartsMessage::SetRatingOnlyFilter(false));
    let effects = rig.raw(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(find_rating(&effects).is_none());
    assert!(
        rig.delays.is_empty()
            && !effects
                .iter()
                .any(|e| matches!(e, Effect::Charts(ChartsEffect::Delay { .. }))),
        "the filter is not part of the key"
    );
    let effects = rig.raw(ChartsMessage::SetRatingOnlyFilter(true));
    let Some(Effect::Charts(ChartsEffect::Rating { filter, .. })) = find_rating(&effects) else {
        panic!("{effects:?}");
    };
    assert_eq!(filter.expect("the filter travels").count(), 0);
}

#[test]
fn quit_cancels_every_computation_of_every_panel_and_late_answers_are_harmless() {
    let (mut rig, _slot, _dir) = sample();
    let mut all = Vec::new();
    all.extend(rig.raw(ChartsMessage::SetYear(2026)));
    all.extend(rig.raw(ChartsMessage::OpenStats));
    all.extend(rig.raw(ChartsMessage::SelectTab(StatsTab::Absolute)));
    all.extend(rig.raw(ChartsMessage::OpenRating));
    all.extend(rig.raw(CrosstableMessage::Open));
    all.extend(rig.raw(PieceTrackerMessage::Open));
    let running: Vec<_> = all
        .into_iter()
        .filter(|e| control_of(e).is_some())
        .collect();
    assert_eq!(running.len(), 5, "{running:?}");
    let controls: Vec<_> = running.iter().filter_map(control_of).collect();
    assert!(controls.iter().all(|c| !c.is_cancelled()));
    rig.raw(Message::Quit);
    assert!(controls.iter().all(OpenControl::is_cancelled));
    for effect in running {
        let answer = rig.perform(effect);
        rig.raw(answer);
    }
    let _ = rig.state.stats_view();
    let _ = rig.state.rating_view();
    let _ = rig.state.crosstable_view();
    let _ = rig.state.piece_tracker_view();
}

#[test]
fn the_panels_cost_nothing_while_closed_and_one_closing_does_not_stop_the_others() {
    let (mut rig, slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    let a = rig.raw(ChartsMessage::OpenStats);
    let b = rig.raw(ChartsMessage::OpenRating);
    let stats = control_of(&find_stats(&a).unwrap()).unwrap();
    let rating = control_of(&find_rating(&b).unwrap()).unwrap();
    rig.raw(ChartsMessage::CloseRating);
    assert!(rating.is_cancelled());
    assert!(!stats.is_cancelled(), "closing the rating graph");
    rig.raw(ChartsMessage::CloseStats);
    assert!(stats.is_cancelled());
    let effects = rig.raw(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(effects.iter().all(|e| !matches!(e, Effect::Charts(_))));
}

#[test]
fn corrupt_preferences_for_the_charts_give_bounded_values_without_panic() {
    let big = (0..=9999u16)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let bulk = format!("[stats]\nelo_checked = [{big}]\nyears_checked = [{big}]\n");
    let cases = [
        "[[[[stats".to_owned(),
        "[stats]\nelo_checked = \"x\"\nyears_checked = 5\n".to_owned(),
        "[stats]\nelo_checked = [1e5, \"x\", -3, 70000, 2400, 2400, 100]\n".to_owned(),
        "[filter_graph]\nyear_min = -5\nyear_max = 99999999999\nelo_step = \"x\"\nkind = 5\ntab = []\n"
            .to_owned(),
        "[filter_graph]\nyear_step = 0\nmoves_step = 0\nmoves_max = 0\nelo_max = 0\n".to_owned(),
        "[rating_graph]\nstart_year = 70000\nstart_elo = -1\nonly_filter = \"yes\"\n".to_owned(),
        "[rating_graph]\nstart_year = 0\nstart_elo = 65535\n".to_owned(),
        bulk,
    ];
    for text in cases {
        let loaded = load_preferences(&text);
        let s = &loaded.prefs.stats;
        for list in [&s.elo_checked, &s.years_checked].into_iter().flatten() {
            assert!(list.len() <= 200, "bounded: {}", list.len());
            assert!(list.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
            assert!(list.iter().all(|n| *n <= 3000));
        }
        let g = &loaded.prefs.filter_graph;
        for v in [
            g.year_min,
            g.year_max,
            g.year_step,
            g.elo_min,
            g.elo_max,
            g.elo_step,
            g.moves_min,
            g.moves_max,
            g.moves_step,
        ] {
            assert!((0..=9999).contains(&v), "{text:.60}: {v}");
        }
        // The panels run on whatever was read.
        let (mut rig, _slot, _dir) = sample();
        rig.state.prefs = loaded.prefs;
        rig.send(ChartsMessage::SetYear(2026));
        rig.send(ChartsMessage::OpenStats);
        rig.send(ChartsMessage::SelectTab(StatsTab::Absolute));
        rig.send(ChartsMessage::OpenRating);
        let _ = rig.state.stats_view();
        let _ = rig.state.rating_view();
    }
    let loaded = load_preferences("[[[[stats");
    assert!(!loaded.issues.is_empty());
    assert_eq!(*loaded.prefs.stats, *load_preferences("").prefs.stats);
}
