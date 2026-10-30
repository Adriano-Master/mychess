//! Review fixes of the M6-4 controller (D3451 to D3454): the table is shared
//! with the export and copy effects, a cancelled graph says so by its own
//! result, and a name proposed for one table is not used for another.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]
mod m6_4_common;

use std::sync::Arc;

use m6_4_common::*;
use mychess_app::charts::{ChartsFailure, ChartsNotice, StatsTab};
use mychess_app::crosstable::{CrosstableNotice, ExportFormat};

fn export_table(effects: &[Effect]) -> Arc<mychess_db::Crosstable> {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Crosstable(CrosstableEffect::Export { table, .. }) => Some(Arc::clone(table)),
            _ => None,
        })
        .expect("an export effect")
}

fn render_table(effects: &[Effect]) -> Arc<mychess_db::Crosstable> {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Crosstable(CrosstableEffect::Render { table, .. }) => Some(Arc::clone(table)),
            _ => None,
        })
        .expect("a render effect")
}

fn export(rig: &mut Rig) -> Vec<Effect> {
    rig.raw(CrosstableMessage::Export {
        format: ExportFormat::Text,
        path: std::path::PathBuf::from("out.txt"),
    })
}

#[test]
fn export_and_copy_carry_the_shared_table_not_a_copy() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let first = export_table(&export(&mut rig));
    let second = export_table(&export(&mut rig));
    let copy = render_table(&rig.raw(CrosstableMessage::Copy));
    assert!(Arc::ptr_eq(&first, &second), "export shares the table");
    assert!(Arc::ptr_eq(&first, &copy), "copy shares the same table");
    // The one on screen is that table too: the token holds the same Arc.
    let token = rig.state.crosstable_token().expect("a token");
    assert_eq!(
        token,
        rig.state.crosstable_token().expect("a token"),
        "the token is stable while the table is"
    );
    // The effects and the state hold it: more than the effects alone.
    assert!(Arc::strong_count(&first) >= 4);
}

#[test]
fn a_name_proposed_for_another_table_writes_nothing() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let token = rig.state.crosstable_token().expect("a table");
    let path = std::path::PathBuf::from("out.txt");
    // Same table: the export goes ahead.
    let effects = rig.raw(CrosstableMessage::ExportOf {
        token: token.clone(),
        format: ExportFormat::Text,
        path: path.clone(),
    });
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Export { .. })))
    );
    // The table is computed again while the dialog is open: another one.
    rig.send(CrosstableMessage::Refresh);
    assert_ne!(rig.state.crosstable_token(), Some(token.clone()));
    let effects = rig.raw(CrosstableMessage::ExportOf {
        token,
        format: ExportFormat::Text,
        path,
    });
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Export { .. }))),
        "nothing is written: {effects:?}"
    );
    assert_eq!(
        rig.state.crosstable_view().notice,
        Some(CrosstableNotice::ExportStale)
    );
}

#[test]
fn a_cancelled_graph_says_so_even_when_the_table_has_a_result() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    rig.send(ChartsMessage::OpenStats);
    assert!(
        !rig.state.stats_view().lines.is_empty(),
        "the table is ready"
    );
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Relative));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Graph { request, .. }) => Some(*request),
            _ => None,
        })
        .expect("the graph is asked for");
    rig.raw(ChartsMessage::GraphComputed {
        request,
        result: Box::new(Err(ChartsFailure::Cancelled)),
    });
    let view = rig.state.stats_view();
    assert!(view.graph.is_none());
    assert_eq!(view.issue, Some(ChartsNotice::Cancelled));
}

#[test]
fn a_cancelled_table_does_not_mark_a_graph_that_is_on_view() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    // The table is still being computed (its answer is not delivered) when
    // the graph tab is chosen: the table has no result to show.
    let effects = rig.raw(ChartsMessage::OpenStats);
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Stats { request, .. }) => Some(*request),
            _ => None,
        })
        .expect("the table is asked for");
    rig.send(ChartsMessage::SelectTab(StatsTab::Relative));
    let view = rig.state.stats_view();
    assert!(view.graph.is_some(), "the graph is ready");
    assert!(view.lines.is_empty(), "the table has no result");
    // The table is stopped from outside: no remark over the graph that is
    // fine.
    rig.raw(ChartsMessage::StatsComputed {
        request,
        result: Box::new(Err(ChartsFailure::Cancelled)),
    });
    assert_eq!(rig.state.stats_view().issue, None);
}

#[test]
fn the_remark_of_a_cancelled_graph_does_not_stay_on_the_table_tab() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    rig.send(ChartsMessage::OpenStats);
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Relative));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Graph { request, .. }) => Some(*request),
            _ => None,
        })
        .expect("the graph is asked for");
    rig.raw(ChartsMessage::GraphComputed {
        request,
        result: Box::new(Err(ChartsFailure::Cancelled)),
    });
    assert_eq!(rig.state.stats_view().issue, Some(ChartsNotice::Cancelled));
    rig.send(ChartsMessage::SelectTab(StatsTab::Table));
    assert_eq!(rig.state.stats_view().issue, None, "the graph's remark");
}

#[test]
fn clicking_the_tab_already_active_keeps_the_remark_of_the_failure() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    rig.send(ChartsMessage::OpenStats);
    let effects = rig.raw(ChartsMessage::SelectTab(StatsTab::Relative));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::Charts(ChartsEffect::Graph { request, .. }) => Some(*request),
            _ => None,
        })
        .expect("the graph is asked for");
    rig.raw(ChartsMessage::GraphComputed {
        request,
        result: Box::new(Err(ChartsFailure::Cancelled)),
    });
    assert_eq!(rig.state.stats_view().issue, Some(ChartsNotice::Cancelled));
    // The click on the tab that is already on view changes nothing (D3512).
    rig.raw(ChartsMessage::SelectTab(StatsTab::Relative));
    assert_eq!(
        rig.state.stats_view().issue,
        Some(ChartsNotice::Cancelled),
        "the remark stays"
    );
}
