//! Contract tests of the crosstable panel (spec
//! `m6-4-tabela-estatisticas-rastreador.md`, slice a: D3352, D3354, D3357,
//! D3363, D3369, D3391, D3394).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_4_common;

use m6_4_common::*;
use mychess_app::crosstable::{
    CrossFlag, CrossFormat, CrossLabels, CrossMode, CrossSort, CrosstableBody, CrosstableNotice,
    ExportFormat,
};
use mychess_app::{load_preferences, render_preferences};

fn compute_effects(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Compute { .. })))
        .collect()
}

fn table(rig: &Rig) -> mychess_app::crosstable::CrossTableView {
    match rig.state.crosstable_view().body {
        CrosstableBody::Table(t) => (*t).clone(),
        other => panic!("not a table: {other:?}"),
    }
}

#[test]
fn without_a_game_of_a_database_there_is_no_table() {
    let mut rig = Rig::new();
    rig.send(CrosstableMessage::Open);
    assert_eq!(
        rig.state.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::NoGame)
    );
}

#[test]
fn closed_panel_computes_nothing_and_shows_nothing() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(Message::Game(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::End,
    )));
    assert!(compute_effects(&effects).is_empty());
    assert_eq!(rig.state.crosstable_view().body, CrosstableBody::Closed);
}

#[test]
fn opening_asks_for_the_tournament_of_the_game_on_screen() {
    let (mut rig, slot, _dir) = sample();
    let effects = rig.raw(CrosstableMessage::Open);
    let Some(Effect::Crosstable(CrosstableEffect::Compute { spec, source, .. })) =
        compute_effects(&effects).into_iter().next().cloned()
    else {
        panic!("no compute effect: {effects:?}");
    };
    assert_eq!(spec.game, 1);
    let tags = spec.tags.expect("tags of the editor");
    assert_eq!((tags.event.as_str(), tags.site.as_str()), ("Club", "Rio"));
    assert_eq!(source, rig.state.bases.slot(slot).unwrap().source);
}

#[test]
fn the_table_of_the_tournament_is_computed_and_ready_to_draw() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let t = table(&rig);
    assert_eq!((t.event.as_str(), t.site.as_str()), ("Club", "Rio"));
    let names: Vec<_> = t.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["A", "B", "C"]);
    let scores: Vec<_> = t.rows.iter().map(|r| r.score.as_str()).collect();
    assert_eq!(scores, ["1.5", "1.0", "0.5"]);
    assert_eq!(t.game_count, 3);
    assert_eq!(t.mode, CrossMode::AllPlayAll);
    // The game on screen is marked in its cell.
    let current = t
        .rows
        .iter()
        .flat_map(|r| r.cells.iter())
        .filter_map(|c| match c {
            mychess_app::crosstable::CrossCellView::Games(g) => Some(g),
            _ => None,
        })
        .flatten()
        .filter(|c| c.current)
        .count();
    assert_eq!(current, 2, "the game shows in both players' cells");
    assert!(!rig.state.crosstable_view().stale);
}

#[test]
fn a_stale_answer_is_dropped() {
    let (mut rig, slot, _dir) = sample();
    let first = rig.raw(CrosstableMessage::Open);
    let Some(Effect::Crosstable(CrosstableEffect::Compute { control, .. })) =
        compute_effects(&first).into_iter().next().cloned()
    else {
        panic!("no compute");
    };
    // Another game, before the first answer: a new request, the first is
    // cancelled.
    let second = rig.raw(BasesMessage::LoadGame { slot, number: 4 });
    // LoadGame is asynchronous; answer its effect, collecting the compute.
    let mut computes = Vec::new();
    let mut queue: Vec<Effect> = second;
    while let Some(effect) = queue.pop() {
        if matches!(effect, Effect::Crosstable(CrosstableEffect::Compute { .. })) {
            computes.push(effect);
        } else if !effect.runs_on_host() && Rig::elapsed(&effect).is_none() {
            let message = rig.perform(effect);
            queue.extend(rig.raw(message));
        }
    }
    assert!(control.is_cancelled(), "the old computation is stopped");
    assert_eq!(computes.len(), 1);
    // The old answer arrives late and is ignored.
    let old = compute_effects(&first).into_iter().next().cloned().unwrap();
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(
        !matches!(rig.state.crosstable_view().body, CrosstableBody::Table(_)),
        "the answer of the old request is not shown"
    );
    let answer = rig.perform(computes.remove(0));
    rig.raw(answer);
    assert_eq!(table(&rig).event, "Open");
}

#[test]
fn a_change_of_game_after_a_result_waits_for_the_debounce() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    assert_eq!(table(&rig).event, "Club");
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    // Nothing computed yet: the old table stays, marked stale.
    assert_eq!(table(&rig).event, "Club");
    assert!(rig.state.crosstable_view().stale);
    assert!(
        rig.delays
            .iter()
            .any(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Delay { .. }))),
        "a debounce was asked for"
    );
    rig.fire_delays();
    assert_eq!(table(&rig).event, "Open");
    assert!(!rig.state.crosstable_view().stale);
}

#[test]
fn only_the_last_debounce_starts_a_computation() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    assert_eq!(rig.delays.len(), 2);
    let first = Rig::elapsed(&rig.delays[0]).unwrap();
    let effects = rig.raw(first);
    assert!(
        compute_effects(&effects).is_empty(),
        "an old delay does nothing"
    );
    rig.fire_delays();
    let t = table(&rig);
    // Game 5 is the Club game of 2021: another tournament of the same event.
    assert_eq!(t.event, "Club");
    assert_eq!(t.game_count, 1, "{t:?}");
}

#[test]
fn changing_an_option_computes_at_once_and_is_saved() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let effects = rig.raw(CrosstableMessage::SetFormat(CrossFormat::Swiss));
    let found = compute_effects(&effects);
    assert_eq!(found.len(), 1, "no debounce for an option");
    let Effect::Crosstable(CrosstableEffect::Compute { spec, .. }) = found[0] else {
        unreachable!()
    };
    assert_eq!(spec.options.format, mychess_db::CrosstableFormat::Swiss);
    assert_eq!(rig.state.prefs.crosstable.format, CrossFormat::Swiss);
    rig.send(CrosstableMessage::SetSort(CrossSort::Name));
    let names: Vec<_> = table(&rig).rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, ["A", "B", "C"]);
    assert_eq!(table(&rig).mode, CrossMode::Swiss);
}

#[test]
fn three_points_for_a_win_changes_the_scores() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::SetFlag(CrossFlag::ThreePoints, true));
    let scores: Vec<_> = table(&rig).rows.iter().map(|r| r.score.clone()).collect();
    assert_eq!(scores, ["4.0", "3.0", "1.0"]);
}

#[test]
fn the_preferences_go_and_come_back() {
    let mut rig = Rig::new();
    rig.send(CrosstableMessage::SetFormat(CrossFormat::Knockout));
    rig.send(CrosstableMessage::SetSort(CrossSort::Rating));
    rig.send(CrosstableMessage::SetFlag(CrossFlag::Tiebreaks, true));
    rig.send(CrosstableMessage::SetFlag(CrossFlag::SwissColors, false));
    rig.send(CrosstableMessage::SetRowColor("#a0b0c0".into()));
    rig.send(CrosstableMessage::SetRowColor("not a colour".into()));
    let text = render_preferences(&rig.state.prefs);
    let loaded = load_preferences(&text);
    assert_eq!(loaded.issues, []);
    assert_eq!(*loaded.prefs.crosstable, *rig.state.prefs.crosstable);
    let p = &loaded.prefs.crosstable;
    assert_eq!(
        (p.format, p.sort),
        (CrossFormat::Knockout, CrossSort::Rating)
    );
    assert!(p.show_tiebreaks && !p.swiss_colors);
    assert_eq!(p.row_color, "#a0b0c0");
}

#[test]
fn a_bad_value_of_the_table_only_resets_its_own_field() {
    let loaded =
        load_preferences("[crosstable]\nformat = \"nope\"\nshow_ages = false\nrow_color = 5\n");
    let p = &loaded.prefs.crosstable;
    assert_eq!(p.format, CrossFormat::Auto);
    assert!(!p.show_ages);
    assert_eq!(p.row_color, mychess_app::crosstable::DEFAULT_ROW_COLOR);
    assert_eq!(loaded.issues.len(), 2);
}

#[test]
fn export_writes_the_three_formats() {
    let (mut rig, _slot, dir) = sample();
    rig.send(CrosstableMessage::Open);
    assert_eq!(
        rig.state
            .crosstable_export_name(ExportFormat::Latex)
            .as_deref(),
        Some("Club Rio.tex")
    );
    for (format, name, needle) in [
        (ExportFormat::Text, "t.txt", "Score"),
        (ExportFormat::Html, "t.html", "<!DOCTYPE html>"),
        (ExportFormat::Latex, "t.tex", "\\begin{longtable}"),
    ] {
        let path = dir.path().join(name);
        rig.send(CrosstableMessage::Export {
            format,
            path: path.clone(),
        });
        let text = std::fs::read_to_string(&path).expect("file");
        assert!(text.contains(needle), "{name}: {text}");
        assert_eq!(
            rig.state.crosstable_view().notice,
            Some(CrosstableNotice::Exported)
        );
    }
}

#[test]
fn export_uses_the_labels_the_front_end_sent() {
    let (mut rig, _slot, dir) = sample();
    rig.send(CrosstableMessage::Open);
    let labels = CrossLabels {
        player: "Jogador".into(),
        ..CrossLabels::default()
    };
    rig.send(CrosstableMessage::SetLabels(Box::new(labels)));
    let path = dir.path().join("t.html");
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Html,
        path: path.clone(),
    });
    assert!(std::fs::read_to_string(path).unwrap().contains("Jogador"));
}

#[test]
fn export_to_a_missing_directory_is_reported() {
    let (mut rig, _slot, dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: dir.path().join("no").join("such").join("t.txt"),
    });
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::ExportFailed)
    );
}

#[test]
fn export_without_a_table_says_so() {
    let mut rig = Rig::new();
    let effects = rig.raw(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: "x.txt".into(),
    });
    assert!(effects.is_empty());
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::NoTable)
    );
}

#[test]
fn copy_sends_the_plain_text_to_the_clipboard() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let host = rig.send(CrosstableMessage::Copy);
    let text = host
        .iter()
        .find_map(|e| match e {
            Effect::WriteClipboard(text) => Some(text.clone()),
            _ => None,
        })
        .expect("a clipboard write");
    assert!(text.starts_with("Club\nRio, 2020.05.01\n"), "{text}");
    assert!(text.contains("3 games: +2 -0 =1"), "{text}");
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::Copied)
    );
}

#[test]
fn a_click_on_a_result_loads_that_game() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(CrosstableMessage::LoadGame(3));
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(3));
    // A game that is not in the table is ignored.
    rig.send(CrosstableMessage::LoadGame(4));
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(3));
}

#[test]
fn set_filter_and_add_to_filter_by_round() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
    rig.send(CrosstableMessage::SetFilter(None));
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3]);
    rig.send(CrosstableMessage::SetFilter(Some(2)));
    assert_eq!(rig.filter_numbers(slot), [2]);
    rig.send(CrosstableMessage::AddToFilter(Some(3)));
    assert_eq!(rig.filter_numbers(slot), [2, 3]);
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::FilterAdded)
    );
    // The table does not depend on the filter.
    assert_eq!(table(&rig).game_count, 3);
}

#[test]
fn the_tournament_of_a_pinned_game_does_not_follow_the_editor() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::OpenGame { slot, game: 4 });
    assert_eq!(table(&rig).event, "Open");
    assert!(!rig.state.crosstable_view().following);
    rig.send(CrosstableMessage::Refresh);
    assert_eq!(table(&rig).event, "Club");
    assert!(rig.state.crosstable_view().following);
    rig.send(CrosstableMessage::OpenGame { slot, game: 99 });
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::InvalidGame)
    );
}

#[test]
fn a_tournament_with_one_player_has_no_table() {
    let (mut rig, slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    rig.fire_delays();
    // Game 4 is a lone game of its event: two players, one game, a table.
    assert_eq!(table(&rig).rows.len(), 2);
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let lone: [Spec; 1] = [GAMES[3]];
    let single = [(
        lone[0].0, lone[0].1, lone[0].2, lone[0].3, "D", "D", "1-0", 0, 0, "1. e4 *",
    )];
    let slot = rig.open_games(dir.path(), "one.pgn", &single);
    rig.load(slot, 1);
    rig.send(CrosstableMessage::Open);
    assert_eq!(
        rig.state.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::NoTable)
    );
}

#[test]
fn closing_stops_the_computation_and_quit_does_too() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(CrosstableMessage::Open);
    let Some(Effect::Crosstable(CrosstableEffect::Compute { control, .. })) =
        compute_effects(&effects).into_iter().next().cloned()
    else {
        panic!("no compute");
    };
    rig.raw(CrosstableMessage::Close);
    assert!(control.is_cancelled());
    assert_eq!(rig.state.crosstable_view().body, CrosstableBody::Closed);

    let effects = rig.raw(CrosstableMessage::Open);
    let Some(Effect::Crosstable(CrosstableEffect::Compute { control, .. })) =
        compute_effects(&effects).into_iter().next().cloned()
    else {
        panic!("no compute");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
}

#[test]
fn a_cancelled_computation_leaves_no_table_and_no_error() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(CrosstableMessage::Open);
    let effect = compute_effects(&effects)
        .into_iter()
        .next()
        .cloned()
        .unwrap();
    rig.raw(CrosstableMessage::Cancel);
    // `Cancel` forgets the computation: its answer is dropped.
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert_eq!(
        rig.state.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::Cancelled),
        "D3422: nothing to show, nothing computing, nothing failed"
    );
}

#[test]
fn the_panel_is_not_assistance_online() {
    for message in [
        Message::Crosstable(CrosstableMessage::Open),
        Message::Charts(ChartsMessage::OpenStats),
        Message::PieceTracker(PieceTrackerMessage::Open),
        Message::Action(mychess_app::Action::ToggleCrosstablePane),
        Message::Action(mychess_app::Action::ToggleStatsPane),
        Message::Action(mychess_app::Action::ToggleRatingPane),
        Message::Action(mychess_app::Action::TogglePieceTrackerPane),
    ] {
        assert_eq!(
            mychess_app::online::family_of(&message),
            None,
            "{message:?}"
        );
    }
}

#[test]
fn the_actions_have_no_default_key_and_can_be_named() {
    let shortcuts = mychess_app::Shortcuts::defaults();
    for action in [
        mychess_app::Action::ToggleCrosstablePane,
        mychess_app::Action::ToggleStatsPane,
        mychess_app::Action::ToggleRatingPane,
        mychess_app::Action::TogglePieceTrackerPane,
    ] {
        assert!(shortcuts.bindings(action).is_empty(), "{action:?}");
        assert_eq!(mychess_app::Action::from_name(action.name()), Some(action));
        assert!(action.message_key().starts_with("action-toggle-"));
    }
}

#[test]
fn the_panes_are_saved_in_the_layout_and_unknown_ones_are_still_dropped() {
    use mychess_app::{LayoutNode, PaneKind};
    for kind in [
        PaneKind::Crosstable,
        PaneKind::FilterStats,
        PaneKind::RatingGraph,
        PaneKind::PieceTracker,
    ] {
        let layout = LayoutNode::default_layout().open(kind);
        assert!(layout.contains(kind), "{kind:?}");
        assert!(layout.is_valid());
    }
}
