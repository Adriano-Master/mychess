//! M6-4d (GUI, spec D3435-D3437): the statistics table, the relative and
//! absolute graphs and the rating graph, drawn from the views of the
//! application, in both themes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_4d_common;

use m6_4d_common::*;
use mychess_app::charts::{ChartsEffect, ChartsFailure, ChartsNotice, FilterGraphKind, GraphField};

fn messages(sim: Simulator<'_, Message>) -> Vec<Message> {
    sim.into_messages().collect()
}

fn has_charts(messages: Vec<Message>, check: impl Fn(&ChartsMessage) -> bool) -> bool {
    messages
        .into_iter()
        .any(|m| matches!(m, Message::App(AppMessage::Charts(c)) if check(&c)))
}

/// The sample open, the statistics panel open and computed.
fn stats_ready() -> Rig {
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::FilterStats);
    rig.app(ChartsMessage::RefreshStats);
    rig
}

fn draws_in_both_themes(rig: &Rig) {
    for theme in themes() {
        let mut sim = rig.sim(1100.0, 800.0);
        assert!(sim.snapshot(&theme).is_ok());
        let mut small = rig.sim(320.0, 380.0);
        assert!(small.snapshot(&theme).is_ok());
    }
}

#[test]
fn the_table_lists_the_default_lines_with_their_counts() {
    let rig = stats_ready();
    let view = rig.shell.app.stats_view();
    assert!(view.lines.len() > 5, "all games, ratings and years");
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("stats-label-all").as_str()).is_ok());
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("n", 2600);
    assert!(
        sim.find(rig.shell.i18n.tr_args("stats-label-both", &args).as_str())
            .is_ok()
    );
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("year", 2010);
    assert!(
        sim.find(rig.shell.i18n.tr_args("stats-label-since", &args).as_str())
            .is_ok()
    );
    assert!(sim.find(iced::widget::Id::new("stats-row-0")).is_ok());
    assert!(sim.find(iced::widget::Id::new("filter-stats-pane")).is_ok());
}

#[test]
fn the_tabs_switch_between_the_table_and_the_two_graphs() {
    let mut rig = stats_ready();
    let mut sim = rig.sim(1300.0, 900.0);
    sim.click(rig.tr("stats-tab-relative").as_str()).unwrap();
    assert!(has_charts(messages(sim), |c| matches!(
        c,
        ChartsMessage::SelectTab(StatsTab::Relative)
    )));
    for (tab, title) in [
        (StatsTab::Relative, "graph-title-relative"),
        (StatsTab::Absolute, "graph-title-absolute"),
    ] {
        rig.app(ChartsMessage::SelectTab(tab));
        let view = rig.shell.app.stats_view();
        assert_eq!(view.tab, tab);
        assert!(view.graph.is_some(), "{tab:?} has a graph");
        let mut sim = rig.sim(1300.0, 900.0);
        assert!(sim.find(rig.tr(title).as_str()).is_ok(), "{title}");
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("n", 5);
        args.set("m", 5);
        assert!(
            sim.find(rig.shell.i18n.tr_args("stats-filter-count", &args).as_str())
                .is_ok()
        );
        draws_in_both_themes(&rig);
    }
}

#[test]
fn every_kind_of_graph_draws_and_the_axis_fields_follow_the_kind() {
    let mut rig = stats_ready();
    rig.app(ChartsMessage::SelectTab(StatsTab::Absolute));
    for kind in [
        FilterGraphKind::Decade,
        FilterGraphKind::Year,
        FilterGraphKind::Elo,
        FilterGraphKind::Moves,
    ] {
        rig.app(ChartsMessage::SetGraphKind(kind));
        let view = rig.shell.app.stats_view();
        assert_eq!(view.graph_prefs.kind, kind);
        assert!(view.graph.is_some(), "{kind:?}");
        let mut sim = rig.sim(1300.0, 900.0);
        let has_axis = sim.find(rig.tr("stats-axis-min").as_str()).is_ok();
        assert_eq!(has_axis, kind != FilterGraphKind::Decade, "{kind:?}");
        let has_estimate = sim.find(rig.tr("stats-estimate").as_str()).is_ok();
        assert_eq!(has_estimate, kind == FilterGraphKind::Elo, "{kind:?}");
        draws_in_both_themes(&rig);
    }
    let mut sim = rig.sim(1300.0, 900.0);
    sim.click(rig.tr("stats-kind-decade").as_str()).unwrap();
    assert!(has_charts(messages(sim), |c| matches!(
        c,
        ChartsMessage::SetGraphKind(FilterGraphKind::Decade)
    )));
}

#[test]
fn a_typed_axis_field_keeps_four_digits_applies_on_enter_or_blur_and_defaults_clear_it() {
    use mychess_gui::windows::data_panels::AXIS_MIN_ID;
    let mut rig = stats_ready();
    rig.app(ChartsMessage::SelectTab(StatsTab::Relative));
    let default = rig.shell.app.stats_view().graph_prefs.year_min;
    let edit = |text: &str| {
        Message::Panels(DataPanelsMessage::AxisEdit(
            FilterGraphKind::Year,
            GraphField::Min,
            text.into(),
        ))
    };
    rig.ui(edit("19a9x5z7"));
    // Typing sends nothing; the field keeps four digits.
    assert_eq!(
        rig.shell.ui.dialogs.panels.axis_text[0][0].as_deref(),
        Some("1995")
    );
    rig.ui(edit("1990"));
    assert_eq!(rig.shell.app.stats_view().graph_prefs.year_min, default);
    // Enter applies it.
    rig.ui(Message::Panels(DataPanelsMessage::AxisSubmit(
        FilterGraphKind::Year,
        GraphField::Min,
    )));
    assert_eq!(rig.shell.app.stats_view().graph_prefs.year_min, 1990);
    // An empty field is kept as typed and sends nothing.
    rig.ui(edit(""));
    rig.ui(Message::Panels(DataPanelsMessage::AxisSubmit(
        FilterGraphKind::Year,
        GraphField::Min,
    )));
    assert_eq!(rig.shell.app.stats_view().graph_prefs.year_min, 1990);
    assert_eq!(
        rig.shell.ui.dialogs.panels.axis_text[0][0].as_deref(),
        Some("")
    );
    // Losing the focus applies the typed value and forgets the text.
    rig.ui(edit("1900"));
    rig.ui(Message::Panels(DataPanelsMessage::FieldFocus(
        AXIS_MIN_ID,
        false,
    )));
    assert_eq!(rig.shell.app.stats_view().graph_prefs.year_min, 1900);
    assert_eq!(rig.shell.ui.dialogs.panels.axis_text[0][0], None);
    rig.ui(edit("1800"));
    rig.ui(Message::Panels(DataPanelsMessage::ResetAxes));
    assert_eq!(rig.shell.ui.dialogs.panels.axis_text[0][0], None);
    assert_eq!(rig.shell.app.stats_view().graph_prefs.year_min, default);
}

#[test]
fn the_options_list_the_thresholds_and_a_checkbox_sends_its_value() {
    let mut rig = stats_ready();
    rig.ui(Message::Panels(DataPanelsMessage::ToggleStatsOptions));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("stats-opt-both").as_str()).is_ok());
    sim.click("2400+").unwrap();
    assert!(has_charts(messages(sim), |c| matches!(
        c,
        ChartsMessage::SetStatsElo(2400, false)
    )));
    let mut sim = rig.sim(1300.0, 900.0);
    sim.click(rig.tr("stats-defaults").as_str()).unwrap();
    assert!(has_charts(messages(sim), |c| matches!(
        c,
        ChartsMessage::ResetStats
    )));
}

#[test]
fn stop_ends_the_computation_with_the_cancelled_notice() {
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    // The effect of the computation is not run.
    rig.toggle(PaneKind::FilterStats);
    assert!(rig.shell.app.charts().is_running());
    rig.ui(Message::App(AppMessage::Charts(ChartsMessage::CancelStats)));
    let view = rig.shell.app.stats_view();
    assert_eq!(view.issue, Some(ChartsNotice::Cancelled));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("charts-notice-cancelled").as_str()).is_ok());
}

#[test]
fn a_failed_computation_is_told() {
    let mut rig = Rig::new();
    rig.open_games(&GAMES);
    rig.toggle(PaneKind::FilterStats);
    let request = update(&mut rig.shell.app, ChartsMessage::RefreshStats.into())
        .into_iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Stats { request, .. }) => Some(request),
            _ => None,
        })
        .expect("a computation starts");
    rig.app(ChartsMessage::StatsComputed {
        request,
        result: Box::new(Err(ChartsFailure::Internal)),
    });
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(
        sim.find(rig.tr("charts-notice-stats-failed").as_str())
            .is_ok()
    );
}

#[test]
fn an_empty_clipbase_counts_zero_games() {
    let mut rig = Rig::new();
    rig.toggle(PaneKind::FilterStats);
    rig.app(ChartsMessage::RefreshStats);
    let view = rig.shell.app.stats_view();
    assert_eq!(view.lines.first().map(|l| l.total), Some(0));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("stats-label-all").as_str()).is_ok());
    drop(sim);
    rig.app(ChartsMessage::SelectTab(StatsTab::Absolute));
    draws_in_both_themes(&rig);
}

// ---- the rating graph ----

fn rating_ready() -> Rig {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    rig.toggle(PaneKind::RatingGraph);
    rig.app(ChartsMessage::RefreshRating);
    rig
}

#[test]
fn the_rating_graph_starts_with_the_players_of_the_game() {
    let rig = rating_ready();
    let view = rig.shell.app.rating_view();
    assert_eq!(view.players, ["A", "B"]);
    let graph = view.graph.expect("a graph");
    assert_eq!(graph.lines.len(), 2);
    let a = graph.lines.iter().find(|l| l.name == "A").unwrap();
    assert_eq!(a.points.len(), 2, "May 2020 and July 2021");
    assert_eq!(a.points[0].1, 2410, "the last game of the month");
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find("A").is_ok() && sim.find("B").is_ok());
    draws_in_both_themes(&rig);
}

#[test]
fn a_chip_removes_its_player_and_the_field_adds_one() {
    let mut rig = rating_ready();
    let mut sim = rig.sim(1300.0, 900.0);
    sim.click("\u{d7}").unwrap();
    let message = messages(sim)
        .into_iter()
        .find(|m| {
            matches!(
                m,
                Message::App(AppMessage::Charts(ChartsMessage::RemovePlayer(_)))
            )
        })
        .expect("RemovePlayer");
    rig.ui(message);
    assert_eq!(rig.shell.app.rating_view().players, ["B"]);
    rig.ui(Message::Panels(DataPanelsMessage::PlayerEdit(" C ".into())));
    rig.ui(Message::Panels(DataPanelsMessage::AddPlayer));
    assert_eq!(rig.shell.app.rating_view().players, ["B", "C"]);
    assert_eq!(rig.shell.ui.dialogs.panels.player_text, "");
    // A blank field adds nothing.
    rig.ui(Message::Panels(DataPanelsMessage::PlayerEdit("  ".into())));
    rig.ui(Message::Panels(DataPanelsMessage::AddPlayer));
    assert_eq!(rig.shell.app.rating_view().players.len(), 2);
}

#[test]
fn clearing_the_players_shows_the_empty_graph_without_an_error() {
    let mut rig = rating_ready();
    rig.app(ChartsMessage::ClearPlayers);
    rig.app(ChartsMessage::RefreshRating);
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("rating-no-players").as_str()).is_ok());
    drop(sim);
    draws_in_both_themes(&rig);
    rig.app(ChartsMessage::UseGamePlayers);
    assert_eq!(rig.shell.app.rating_view().players.len(), 2);
}

#[test]
fn the_rating_options_unfold_and_the_start_year_filters_the_games() {
    let mut rig = rating_ready();
    rig.ui(Message::Panels(DataPanelsMessage::ToggleRatingOptions));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("rating-start-year").as_str()).is_ok());
    assert!(sim.find(rig.tr("rating-only-filter").as_str()).is_ok());
    drop(sim);
    rig.app(ChartsMessage::SetRatingStartYear(2021));
    let graph = rig.shell.app.rating_view().graph.unwrap();
    let a = graph.lines.iter().find(|l| l.name == "A").unwrap();
    assert_eq!(a.points.len(), 1, "only July 2021 is left");
}

#[test]
fn a_player_without_games_says_so() {
    let mut rig = rating_ready();
    rig.app(ChartsMessage::ClearPlayers);
    rig.app(ChartsMessage::AddPlayer("Nobody".into()));
    rig.app(ChartsMessage::RefreshRating);
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(sim.find(rig.tr("rating-empty").as_str()).is_ok());
}
