//! M4-10b: the report pane in the saved layout (spec D1566): persisted as
//! `"report"`, restored, tolerant with unknown names, and absent from the
//! default layout.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "m2_6_layout_common/mod.rs"]
mod m2_6_layout_common;

use m2_6_layout_common::*;
use mychess_app::PaneKind::{Board, Comment, Info, Notation, Report};
use toml::{Table, Value};

fn layout_table(text: &str) -> Table {
    let root: Table = text.parse().expect("valid toml");
    match root.get("layout") {
        Some(Value::Table(t)) => t.clone(),
        other => panic!("no [layout] table: {other:?}"),
    }
}

#[test]
fn a_lone_report_pane_is_written_as_report() {
    let layout = h(0.6, pane(Board), pane(Report));
    let table = layout_table(&render(&layout));
    let b = table["b"].as_table().unwrap();
    assert_eq!(b.get("pane").and_then(Value::as_str), Some("report"));
}

#[test]
fn a_report_tab_is_written_in_the_tabs_list_and_comes_back() {
    let layout = h(0.6, pane(Board), grp(&[Notation, Report, Info], 1));
    let table = layout_table(&render(&layout));
    let tabs: Vec<_> = table["b"]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(tabs, ["notation", "report", "info"]);
    assert_eq!(round_trip(&layout), Some(layout));
}

#[test]
fn the_report_pane_survives_a_round_trip_in_every_position() {
    for layout in [
        h(0.6, pane(Board), pane(Report)),
        v(0.7, pane(Board), pane(Report)),
        h(0.5, grp(&[Board], 0), grp(&[Report, Comment], 0)),
        h(0.6, pane(Board), v(0.5, pane(Report), pane(Info))),
    ] {
        assert_eq!(round_trip(&layout).as_ref(), Some(&layout));
    }
}

#[test]
fn the_default_layout_does_not_open_the_report() {
    assert!(!default_layout().panes().contains(&Report));
}

#[test]
fn an_unknown_neighbour_of_the_report_tab_is_dropped_and_the_report_kept() {
    let text = "{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = { pane = \"board\" }, b = { tabs = [\"future\", \"report\", \"reports\"], active = 1 } }";
    let got = effective(text);
    assert!(got.is_valid());
    assert_eq!(got, h(0.6, pane(Board), grp(&[Report], 0)));
}

#[test]
fn a_misspelt_report_pane_is_unknown_and_never_panics() {
    for name in ["Report", "REPORT", "reports", "report ", ""] {
        let text = format!(
            "{{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = {{ pane = \"board\" }}, b = {{ pane = \"{name}\" }} }}"
        );
        let got = effective(&text);
        assert!(got.is_valid(), "{name:?}");
        assert!(!got.panes().contains(&Report), "{name:?}");
    }
}

#[test]
fn the_report_pane_in_a_file_written_by_this_version_loads_without_issues() {
    let layout = h(0.6, pane(Board), grp(&[Notation, Report], 1));
    let loaded = load(&render(&layout));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.prefs.layout, Some(layout));
}
