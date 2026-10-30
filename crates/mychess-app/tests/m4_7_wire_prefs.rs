//! M4-7 contract tests of the preferences `[graph]` (D871), the shortcut
//! (D870) and the `graph` pane in the layout (D874).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m4_7_wire_common;

use std::collections::HashSet;

use m4_7_wire_common::*;
use mychess_app::{
    Action, Binding, KeyInput, LayoutNode, PaneKind, PrefChange, Preferences, PrefsMessage,
    Shortcuts, SplitAxis, load_preferences, render_preferences,
};
use rstest::rstest;

fn loaded(text: &str) -> (GraphPrefs, usize) {
    let l = load_preferences(text);
    (l.prefs.graph, l.issues.len())
}

#[test]
fn defaults_are_auto_without_inversion_and_highlighting() {
    let d = GraphPrefs::default();
    assert_eq!(d.kind, GraphKind::Auto);
    assert_eq!(
        d.invert,
        Invert {
            white: false,
            black: false
        }
    );
    assert!(d.highlight_current);
    assert_eq!(Preferences::default().graph, d);
}

#[rstest]
#[case(GraphKind::Auto, "auto")]
#[case(GraphKind::Score, "score")]
#[case(GraphKind::Time, "time")]
#[case(GraphKind::ScoreCombo, "score_combo")]
#[case(GraphKind::TimeCombo, "time_combo")]
fn every_kind_round_trips_through_the_file(#[case] kind: GraphKind, #[case] name: &str) {
    for (w, b, h) in [
        (false, false, false),
        (true, false, true),
        (false, true, false),
        (true, true, true),
    ] {
        let prefs = Preferences {
            graph: GraphPrefs {
                kind,
                invert: Invert { white: w, black: b },
                highlight_current: h,
                ..GraphPrefs::default()
            },
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[graph]"), "{text}");
        assert!(text.contains(&format!("kind = \"{name}\"")), "{text}");
        let l = load_preferences(&text);
        assert_eq!(l.prefs.graph, prefs.graph);
        assert!(l.issues.is_empty(), "{:?}", l.issues);
    }
}

#[test]
fn an_old_file_without_the_table_keeps_the_defaults_silently() {
    let (g, issues) = loaded("version = 1\n[general]\ntheme = \"dark\"\n");
    assert_eq!(g, GraphPrefs::default());
    assert_eq!(issues, 0);
    let (g, issues) = loaded("");
    assert_eq!(g, GraphPrefs::default());
    assert_eq!(issues, 0);
}

#[rstest]
#[case("kind = \"pie\"")]
#[case("kind = 3")]
#[case("kind = true")]
#[case("kind = \"AUTO\"")]
#[case("kind = \"\"")]
#[case("kind = [\"time\"]")]
#[case("invert_white = 1")]
#[case("invert_white = \"true\"")]
#[case("invert_black = 2.5")]
#[case("highlight_current = \"no\"")]
#[case("highlight_current = []")]
fn a_hostile_field_falls_back_to_default_with_one_issue(#[case] line: &str) {
    let (g, issues) = loaded(&format!("[graph]\n{line}\n"));
    assert_eq!(g, GraphPrefs::default(), "{line}");
    assert_eq!(issues, 1, "{line}");
}

#[test]
fn a_bad_field_does_not_spoil_the_good_ones() {
    let (g, issues) = loaded(
        "[graph]\nkind = \"time\"\ninvert_white = 9\ninvert_black = true\nhighlight_current = false\n",
    );
    assert_eq!(g.kind, GraphKind::Time);
    assert_eq!(
        g.invert,
        Invert {
            white: false,
            black: true
        }
    );
    assert!(!g.highlight_current);
    assert_eq!(issues, 1);
}

#[test]
fn a_graph_that_is_not_a_table_falls_back() {
    for text in ["graph = 7\n", "graph = \"x\"\n", "graph = [1,2]\n"] {
        let (g, issues) = loaded(text);
        assert_eq!(g, GraphPrefs::default(), "{text}");
        assert!(issues >= 1, "{text}");
    }
}

#[test]
fn unknown_keys_in_the_table_do_not_break_reading() {
    let (g, _) = loaded("[graph]\nkind = \"score\"\nfuture = 1\n");
    assert_eq!(g.kind, GraphKind::Score);
}

#[test]
fn a_graph_change_through_prefs_message_is_applied() {
    let mut state = AppState::default();
    let g = GraphPrefs {
        kind: GraphKind::Time,
        ..GraphPrefs::default()
    };
    send(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Graph(g))),
    );
    assert_eq!(state.prefs.graph, g);
}

#[test]
fn graph_options_persist_in_the_file_after_messages() {
    let mut state = AppState::default();
    send(&mut state, GraphMessage::SetKind(GraphKind::TimeCombo));
    send(&mut state, GraphMessage::ToggleInvertBlack);
    let l = load_preferences(&render_preferences(&state.prefs));
    assert_eq!(l.prefs.graph.kind, GraphKind::TimeCombo);
    assert!(l.prefs.graph.invert.black && !l.prefs.graph.invert.white);
}

// ---- shortcut (D870) ----

fn key(text: &str) -> KeyInput {
    let b = Binding::parse(text).unwrap();
    KeyInput {
        key: b.key,
        modifiers: b.modifiers,
    }
}

#[test]
fn command_shift_g_toggles_the_graph_pane() {
    let s = Shortcuts::defaults();
    assert_eq!(
        s.lookup(&key("Command+Shift+G")),
        Some(Action::ToggleGraphPane)
    );
    assert_eq!(s.bindings(Action::ToggleGraphPane).len(), 1);
}

#[test]
fn command_shift_z_stays_redo() {
    let s = Shortcuts::defaults();
    assert_eq!(s.lookup(&key("Command+Shift+Z")), Some(Action::Redo));
}

#[test]
fn the_default_map_has_no_binding_owned_by_two_actions() {
    let s = Shortcuts::defaults();
    let mut seen: HashSet<(String, Action)> = HashSet::new();
    let mut by_binding: Vec<(Binding, Action)> = Vec::new();
    for (action, keys) in s.iter() {
        for b in keys {
            for (other, owner) in &by_binding {
                assert!(
                    !(other.key == b.key && other.modifiers == b.modifiers),
                    "{b:?} bound to {action:?} and {owner:?}"
                );
            }
            by_binding.push((*b, action));
            seen.insert((format!("{b:?}"), action));
        }
    }
    assert!(!seen.is_empty());
}

#[test]
fn the_action_has_a_stable_name_and_message_key() {
    assert_eq!(
        Action::from_name(Action::ToggleGraphPane.name()),
        Some(Action::ToggleGraphPane)
    );
    assert!(!Action::ToggleGraphPane.message_key().is_empty());
}

#[test]
fn the_shortcut_is_text_focus_safe() {
    let s = Shortcuts::defaults();
    assert_eq!(
        s.lookup_in(&key("Command+Shift+G"), true),
        Some(Action::ToggleGraphPane)
    );
}

// ---- layout (D874) ----

fn split(a: LayoutNode, b: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        axis: SplitAxis::Vertical,
        ratio: 0.7,
        a: Box::new(a),
        b: Box::new(b),
    }
}

fn layout_text(layout: &LayoutNode) -> String {
    render_preferences(&Preferences {
        layout: Some(layout.clone()),
        ..Preferences::default()
    })
}

#[test]
fn the_graph_pane_round_trips_with_the_name_graph() {
    let layout = split(
        LayoutNode::pane(PaneKind::Board),
        split(
            LayoutNode::pane(PaneKind::Notation),
            LayoutNode::pane(PaneKind::Graph),
        ),
    );
    assert!(layout.is_valid());
    let text = layout_text(&layout);
    assert!(text.contains("pane = \"graph\""), "{text}");
    let l = load_preferences(&text);
    assert_eq!(l.prefs.layout, Some(layout));
    assert!(l.issues.is_empty(), "{:?}", l.issues);
}

#[test]
fn a_layout_without_the_graph_still_loads_unchanged() {
    let layout = LayoutNode::default_layout();
    let l = load_preferences(&layout_text(&layout));
    assert_eq!(l.prefs.layout, Some(layout));
    assert!(!l.prefs.layout.unwrap().panes().contains(&PaneKind::Graph));
}

#[test]
fn the_graph_twice_makes_the_layout_invalid() {
    let layout = split(
        LayoutNode::pane(PaneKind::Board),
        split(
            LayoutNode::pane(PaneKind::Graph),
            LayoutNode::pane(PaneKind::Graph),
        ),
    );
    assert!(!layout.is_valid());
}

#[test]
fn a_layout_without_a_board_but_with_the_graph_is_invalid() {
    assert!(!LayoutNode::pane(PaneKind::Graph).is_valid());
}

#[test]
fn an_unknown_pane_name_falls_back_to_the_default_layout() {
    let l = load_preferences("[layout]\npane = \"grafh\"\n");
    assert_ne!(l.prefs.layout, Some(LayoutNode::pane(PaneKind::Graph)));
}
