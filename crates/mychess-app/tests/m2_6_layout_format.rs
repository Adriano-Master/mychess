//! M2-6 phase A: saved format v2 (D1010, D1012, D1017), `Comment` naming and
//! the tolerant reading.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_6_layout_common;

use m2_6_layout_common::*;
use mychess_app::{LayoutNode, PaneKind, PaneKind::*, PrefsIssue};
use proptest::prelude::*;
use rstest::rstest;
use toml::{Table, Value};

fn layout_table(layout: &LayoutNode) -> Table {
    let text = render(layout);
    let root: Table = text.parse().expect("valid toml");
    match root.get("layout") {
        Some(Value::Table(t)) => t.clone(),
        other => panic!("no [layout] table: {other:?}\n{text}"),
    }
}

fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("text").to_owned())
        .collect()
}

// ---- writing ----

#[test]
fn writes_layout_version_2_once_at_the_root() {
    let table = layout_table(&default_layout());
    assert_eq!(table.get("layout_version"), Some(&Value::Integer(2)));
    for side in ["a", "b"] {
        let child = table.get(side).and_then(Value::as_table).expect("child");
        assert!(!child.contains_key("layout_version"), "{side}");
    }
}

#[test]
fn writes_a_lone_panel_as_pane_and_a_group_as_tabs_and_active() {
    let layout = h(0.6, pane(Board), grp(&[Notation, Info, Comment], 2));
    let table = layout_table(&layout);
    let a = table["a"].as_table().unwrap();
    assert_eq!(a.get("pane").and_then(Value::as_str), Some("board"));
    assert!(!a.contains_key("tabs"));
    let b = table["b"].as_table().unwrap();
    assert_eq!(names(&b["tabs"]), ["notation", "info", "comment"]);
    assert_eq!(b.get("active"), Some(&Value::Integer(2)));
    assert!(!b.contains_key("pane"));
}

#[test]
fn a_lone_panel_root_is_written_with_the_version() {
    let table = layout_table(&pane(Board));
    assert_eq!(table.get("pane").and_then(Value::as_str), Some("board"));
    assert_eq!(table.get("layout_version"), Some(&Value::Integer(2)));
}

#[test]
fn axis_and_ratio_are_written_for_splits() {
    let table = layout_table(&v(0.58, pane(Board), pane(GameList)));
    assert_eq!(table["axis"].as_str(), Some("vertical"));
    assert!((table["ratio"].as_float().unwrap() - 0.58).abs() < 1e-9);
    let table = layout_table(&default_layout());
    assert_eq!(table["axis"].as_str(), Some("horizontal"));
}

// ---- round trips ----

fn varied() -> Vec<LayoutNode> {
    vec![
        default_layout(),
        pane(Board),
        h(0.5, pane(Board), pane(Info)),
        h(0.6, pane(Board), grp(&[Notation, Comment], 1)),
        h(0.1, pane(Board), grp(&[Tree, Graph, GameList, Analysis], 3)),
        v(
            0.9,
            h(0.33, pane(Board), grp(&[Info, Comment], 0)),
            h(0.4, grp(&[Notation, Tree], 1), pane(GameList)),
        ),
        default_layout()
            .open(GameList)
            .open(Analysis)
            .open(Graph)
            .open(Tree),
        default_layout()
            .merge(Info, Notation)
            .detach(Comment)
            .select(Info),
        // A lone notation panel is kept in v2 (no migration).
        h(0.7, pane(Board), pane(Notation)),
    ]
}

#[test]
fn varied_layouts_round_trip_exactly() {
    for layout in varied() {
        assert!(layout.is_valid(), "{layout:?}");
        assert_eq!(round_trip(&layout), Some(layout.clone()), "{layout:?}");
        let loaded = load(&render(&layout));
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }
}

fn arb_layout() -> impl Strategy<Value = LayoutNode> {
    proptest::collection::vec((0..ALL.len(), 0..ALL.len(), 0..5_usize), 0..40).prop_map(|ops| {
        let mut layout = default_layout();
        for (a, b, which) in ops {
            layout = match which {
                0 => layout.open(ALL[a]),
                1 => layout.close(ALL[a]),
                2 => layout.merge(ALL[a], ALL[b]),
                3 => layout.detach(ALL[a]),
                _ => layout.select(ALL[a]),
            };
        }
        layout
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn any_reachable_layout_survives_the_file(layout in arb_layout()) {
        prop_assert!(layout.is_valid());
        prop_assert_eq!(round_trip(&layout), Some(layout));
    }
}

// ---- names ----

#[test]
fn comment_is_saved_as_comment_and_read_back() {
    let layout = h(0.6, pane(Board), pane(Comment));
    let table = layout_table(&layout);
    assert_eq!(
        table["b"].as_table().unwrap()["pane"].as_str(),
        Some("comment")
    );
    assert_eq!(round_trip(&layout), Some(layout));
    let in_tabs = h(0.6, pane(Board), grp(&[Info, Comment], 1));
    let table = layout_table(&in_tabs);
    assert_eq!(
        names(&table["b"].as_table().unwrap()["tabs"]),
        ["info", "comment"]
    );
}

#[rstest]
#[case("board", Board)]
#[case("info", Info)]
#[case("comment", Comment)]
#[case("games", GameList)]
#[case("analysis", Analysis)]
#[case("graph", Graph)]
#[case("tree", Tree)]
fn every_old_and_new_pane_name_is_read(#[case] name: &str, #[case] kind: PaneKind) {
    if kind == Board {
        return;
    }
    let loaded = read_inline(&format!(
        "{{ axis = \"horizontal\", ratio = 0.6, a = {{ pane = \"board\" }}, b = {{ pane = \"{name}\" }} }}"
    ));
    assert_eq!(
        loaded.prefs.layout,
        Some(h(0.6, pane(Board), pane(kind))),
        "{name}"
    );
    assert!(loaded.issues.is_empty());
}

#[test]
fn the_pane_name_notation_is_read() {
    let loaded = read_inline(
        "{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = { pane = \"board\" }, b = { pane = \"notation\" } }",
    );
    assert_eq!(
        loaded.prefs.layout,
        Some(h(0.6, pane(Board), pane(Notation)))
    );
}

#[test]
fn old_pane_files_without_the_marker_still_load() {
    // A v1 file that never had a notation panel is read as written.
    let loaded = read_inline(
        "{ axis = \"vertical\", ratio = 0.7, a = { pane = \"board\" }, b = { pane = \"games\" } }",
    );
    assert_eq!(
        loaded.prefs.layout,
        Some(v(0.7, pane(Board), pane(GameList)))
    );
}

// ---- tolerant reading ----

const V2: &str = "layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = { pane = \"board\" }";

fn with_b(b: &str) -> String {
    format!("{{ {V2}, b = {b} }}")
}

#[test]
fn unknown_tab_is_dropped_keeping_a_known_active_tab() {
    let got = effective(&with_b(
        "{ tabs = [\"info\", \"future\", \"comment\"], active = 2 }",
    ));
    assert_eq!(got, h(0.6, pane(Board), grp(&[Info, Comment], 1)));
    let got = effective(&with_b(
        "{ tabs = [\"future\", \"info\", \"comment\"], active = 1 }",
    ));
    assert_eq!(got, h(0.6, pane(Board), grp(&[Info, Comment], 0)));
}

#[test]
fn active_tab_that_is_unknown_still_leaves_a_valid_group() {
    let got = effective(&with_b(
        "{ tabs = [\"info\", \"future\", \"comment\"], active = 1 }",
    ));
    assert!(got.is_valid());
    assert_eq!(tabs_of(&got, Info).0, [Info, Comment]);
}

#[rstest]
#[case("9")]
#[case("2")]
#[case("-1")]
#[case("1000000000000")]
fn active_out_of_range_becomes_zero(#[case] active: &str) {
    let got = effective(&with_b(&format!(
        "{{ tabs = [\"info\", \"comment\"], active = {active} }}"
    )));
    assert_eq!(got, h(0.6, pane(Board), grp(&[Info, Comment], 0)));
}

#[test]
fn missing_or_mistyped_active_leaves_a_valid_group() {
    for b in [
        "{ tabs = [\"info\", \"comment\"] }",
        "{ tabs = [\"info\", \"comment\"], active = \"1\" }",
        "{ tabs = [\"info\", \"comment\"], active = 1.5 }",
    ] {
        let got = effective(&with_b(b));
        assert!(got.is_valid(), "{b}");
        assert_eq!(tabs_of(&got, Info).0, [Info, Comment], "{b}");
    }
}

#[test]
fn tabs_with_non_text_elements_drop_them() {
    let got = effective(&with_b(
        "{ tabs = [\"info\", 3, true, \"comment\", [\"x\"]], active = 3 }",
    ));
    assert_eq!(got, h(0.6, pane(Board), grp(&[Info, Comment], 1)));
}

#[test]
fn an_empty_group_is_dropped_and_the_split_becomes_the_other_side() {
    let loaded = read_inline(&with_b("{ tabs = [] }"));
    assert_eq!(loaded.prefs.layout, Some(pane(Board)));
    let loaded = read_inline(&with_b("{ tabs = [\"future\", \"other\"] }"));
    assert_eq!(loaded.prefs.layout, Some(pane(Board)));
    let loaded = read_inline(&with_b("{ pane = \"future\" }"));
    assert_eq!(loaded.prefs.layout, Some(pane(Board)));
}

#[test]
fn a_nested_split_that_loses_a_side_collapses_into_the_other() {
    let nested = "{ axis = \"vertical\", ratio = 0.5, a = { tabs = [] }, b = { pane = \"info\" } }";
    let got = effective(&with_b(nested));
    assert_eq!(got, h(0.6, pane(Board), pane(Info)));
    let nested =
        "{ axis = \"vertical\", ratio = 0.5, a = { pane = \"info\" }, b = { pane = \"zzz\" } }";
    assert_eq!(effective(&with_b(nested)), h(0.6, pane(Board), pane(Info)));
}

#[test]
fn a_board_dropped_or_missing_gives_the_default_layout() {
    // The board side is the unknown one.
    let text = "{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = { pane = \"zzz\" }, b = { pane = \"info\" } }";
    let loaded = read_inline(text);
    assert!(loaded.prefs.layout.is_none() || loaded.prefs.layout == Some(default_layout()));
    assert_eq!(effective(text), default_layout());
    let text = "{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = { pane = \"info\" }, b = { pane = \"tree\" } }";
    assert_eq!(effective(text), default_layout());
}

#[test]
fn a_repeated_board_gives_the_default_layout() {
    let text = "{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = { pane = \"board\" }, b = { pane = \"board\" } }";
    assert_eq!(effective(text), default_layout());
    let text = with_b("{ tabs = [\"board\", \"info\"], active = 0 }");
    assert_eq!(effective(&text), default_layout());
}

#[test]
fn a_repeated_panel_gives_the_default_layout() {
    let text = with_b("{ tabs = [\"info\", \"info\"], active = 0 }");
    assert_eq!(effective(&text), default_layout());
    let text = format!(
        "{{ layout_version = 2, axis = \"vertical\", ratio = 0.5, a = {{ {V2}, b = {{ pane = \"info\" }} }}, b = {{ tabs = [\"info\", \"tree\"], active = 0 }} }}"
    );
    assert_eq!(effective(&text), default_layout());
}

#[rstest]
#[case("\"hello\"")]
#[case("3")]
#[case("true")]
#[case("[]")]
#[case("{}")]
#[case("{ layout_version = 2 }")]
#[case("{ layout_version = 2, tabs = \"info\" }")]
#[case("{ layout_version = 2, tabs = 3 }")]
#[case("{ layout_version = 2, pane = 5 }")]
#[case(
    "{ layout_version = 2, axis = \"diagonal\", ratio = 0.5, a = { pane = \"board\" }, b = { pane = \"info\" } }"
)]
#[case(
    "{ layout_version = 2, axis = \"horizontal\", ratio = \"half\", a = { pane = \"board\" }, b = { pane = \"info\" } }"
)]
#[case(
    "{ layout_version = 2, axis = \"horizontal\", ratio = 0.95, a = { pane = \"board\" }, b = { pane = \"info\" } }"
)]
#[case(
    "{ layout_version = 2, axis = \"horizontal\", ratio = 0.05, a = { pane = \"board\" }, b = { pane = \"info\" } }"
)]
#[case(
    "{ layout_version = 2, axis = \"horizontal\", ratio = nan, a = { pane = \"board\" }, b = { pane = \"info\" } }"
)]
#[case("{ layout_version = 2, axis = \"horizontal\", ratio = 0.5, a = { pane = \"board\" } }")]
#[case("{ layout_version = 2, axis = \"horizontal\", ratio = 0.5, b = { pane = \"board\" } }")]
#[case(
    "{ layout_version = 2, axis = \"horizontal\", ratio = 0.5, a = 3, b = { pane = \"board\" } }"
)]
fn a_bad_value_anywhere_gives_the_default_layout(#[case] table: &str) {
    let loaded = read_inline(table);
    assert!(
        loaded.prefs.layout.is_none() || loaded.prefs.layout == Some(default_layout()),
        "{table}: {:?}",
        loaded.prefs.layout
    );
    assert_eq!(effective(table), default_layout(), "{table}");
}

#[test]
fn a_bad_layout_is_reported_as_an_invalid_layout_value() {
    let loaded = read_inline("{ pane = \"info\" }");
    assert!(
        loaded
            .issues
            .iter()
            .any(|i| matches!(i, PrefsIssue::InvalidValue { key } if key == "layout")),
        "{:?}",
        loaded.issues
    );
}

#[test]
fn layout_version_above_2_is_still_read_as_v2() {
    let got = effective(
        "{ layout_version = 3, axis = \"horizontal\", ratio = 0.6, a = { pane = \"board\" }, b = { pane = \"notation\" } }",
    );
    assert_eq!(got, h(0.6, pane(Board), pane(Notation)));
}

#[test]
fn excessive_depth_never_panics_and_never_yields_an_invalid_layout() {
    let mut text = "{ pane = \"board\" }".to_owned();
    for _ in 0..40 {
        text = format!(
            "{{ layout_version = 2, axis = \"vertical\", ratio = 0.5, a = {text}, b = {{ pane = \"zzz\" }} }}"
        );
    }
    let loaded = read_inline(&text);
    if let Some(layout) = loaded.prefs.layout {
        assert!(layout.is_valid());
    }
    assert!(effective(&text).is_valid());
}
