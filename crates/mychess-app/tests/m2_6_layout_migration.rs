//! M2-6 phase A: migration of the v1 layout (no `layout_version`, D1010,
//! D1017).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m2_6_layout_common;

use m2_6_layout_common::*;
use mychess_app::{LayoutNode, PaneKind::*};

fn v1(table: &str) -> Option<LayoutNode> {
    read_inline(table).prefs.layout
}

const BOARD: &str = "{ pane = \"board\" }";

fn pn(name: &str) -> String {
    format!("{{ pane = \"{name}\" }}")
}

fn hsplit(ratio: &str, a: &str, b: &str) -> String {
    format!("{{ axis = \"horizontal\", ratio = {ratio}, a = {a}, b = {b} }}")
}

fn vsplit(ratio: &str, a: &str, b: &str) -> String {
    format!("{{ axis = \"vertical\", ratio = {ratio}, a = {a}, b = {b} }}")
}

#[test]
fn a_lone_notation_without_info_becomes_notation_info_comment() {
    let got = v1(&hsplit("0.7", BOARD, &pn("notation"))).expect("layout");
    assert_eq!(got, h(0.7, pane(Board), grp(&[Notation, Info, Comment], 0)));
    assert!(got.is_valid());
}

#[test]
fn a_lone_notation_with_info_elsewhere_becomes_notation_comment() {
    let text = hsplit("0.65", &vsplit("0.5", BOARD, &pn("info")), &pn("notation"));
    let got = v1(&text).expect("layout");
    assert_eq!(
        got,
        h(
            0.65,
            v(0.5, pane(Board), pane(Info)),
            grp(&[Notation, Comment], 0)
        )
    );
    assert!(got.is_valid());
}

#[test]
fn info_anywhere_counts_even_beyond_the_notation_sibling() {
    let text = vsplit(
        "0.7",
        &hsplit("0.6", BOARD, &pn("notation")),
        &hsplit("0.5", &pn("games"), &pn("info")),
    );
    let got = v1(&text).expect("layout");
    assert_eq!(tabs_of(&got, Notation), (vec![Notation, Comment], 0));
    assert_eq!(tabs_of(&got, Info), (vec![Info], 0));
}

#[test]
fn a_v1_layout_without_notation_is_not_changed() {
    let text = hsplit("0.6", BOARD, &pn("info"));
    assert_eq!(v1(&text), Some(h(0.6, pane(Board), pane(Info))));
    let text = vsplit("0.58", BOARD, &pn("games"));
    assert_eq!(v1(&text), Some(v(0.58, pane(Board), pane(GameList))));
    assert_eq!(v1(BOARD), Some(pane(Board)));
}

#[test]
fn a_v1_layout_that_already_has_comment_is_not_changed() {
    let text = vsplit(
        "0.5",
        &hsplit("0.6", BOARD, &pn("notation")),
        &pn("comment"),
    );
    let got = v1(&text).expect("layout");
    assert_eq!(
        got,
        v(0.5, h(0.6, pane(Board), pane(Notation)), pane(Comment))
    );
}

#[test]
fn other_panels_next_to_notation_are_left_alone() {
    let text = hsplit("0.6", BOARD, &vsplit("0.5", &pn("notation"), &pn("graph")));
    let got = v1(&text).expect("layout");
    assert_eq!(
        got,
        h(
            0.6,
            pane(Board),
            v(0.5, grp(&[Notation, Info, Comment], 0), pane(Graph))
        )
    );
}

#[test]
fn the_exact_old_default_is_recognised_before_the_migration() {
    let old = hsplit("0.6", &vsplit("0.78", BOARD, &pn("info")), &pn("notation"));
    assert_eq!(v1(&old), Some(default_layout()));
    // Within the 0.005 tolerance.
    let near = hsplit(
        "0.603",
        &vsplit("0.783", BOARD, &pn("info")),
        &pn("notation"),
    );
    assert_eq!(v1(&near), Some(default_layout()));
}

#[test]
fn the_legacy_default_layout_written_by_the_old_version_converts_to_the_new_default() {
    let loaded = load(&render(&LayoutNode::legacy_default_layout()));
    assert_eq!(loaded.prefs.layout, Some(default_layout()));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn a_customised_old_default_is_migrated_not_replaced() {
    let custom = hsplit("0.6", &vsplit("0.6", BOARD, &pn("info")), &pn("notation"));
    let got = v1(&custom).expect("layout");
    assert_eq!(
        got,
        h(
            0.6,
            v(0.6, pane(Board), pane(Info)),
            grp(&[Notation, Comment], 0)
        )
    );
    // Panels swapped with the same ratios: also a custom layout.
    let swapped = hsplit("0.6", &pn("notation"), &vsplit("0.78", BOARD, &pn("info")));
    let got = v1(&swapped).expect("layout");
    assert_eq!(tabs_of(&got, Notation), (vec![Notation, Comment], 0));
}

#[test]
fn migration_is_idempotent_through_a_write_and_a_read() {
    let inputs = [
        hsplit("0.7", BOARD, &pn("notation")),
        hsplit("0.65", &vsplit("0.5", BOARD, &pn("info")), &pn("notation")),
        hsplit("0.6", BOARD, &pn("info")),
        hsplit("0.6", &vsplit("0.78", BOARD, &pn("info")), &pn("notation")),
        vsplit(
            "0.5",
            &hsplit("0.6", BOARD, &pn("notation")),
            &pn("comment"),
        ),
    ];
    for text in inputs {
        let first = v1(&text).expect("layout");
        let written = render(&first);
        assert!(written.contains("layout_version = 2"), "{written}");
        let second = load(&written).prefs.layout.expect("layout");
        assert_eq!(second, first, "{text}");
        // And once more: v2 is never migrated again.
        assert_eq!(round_trip(&second), Some(second.clone()), "{text}");
    }
}

#[test]
fn a_v2_lone_notation_is_not_migrated() {
    let text = format!(
        "{{ layout_version = 2, axis = \"horizontal\", ratio = 0.6, a = {BOARD}, b = {} }}",
        pn("notation")
    );
    assert_eq!(v1(&text), Some(h(0.6, pane(Board), pane(Notation))));
}

#[test]
fn the_migrated_layout_always_has_every_tab_known_and_valid() {
    for text in [
        hsplit("0.7", BOARD, &pn("notation")),
        vsplit("0.3", &pn("notation"), BOARD),
        hsplit("0.2", &pn("notation"), &vsplit("0.5", BOARD, &pn("tree"))),
    ] {
        let got = v1(&text).expect("layout");
        assert!(got.is_valid(), "{text}");
        assert!(got.contains(Comment) && got.contains(Notation), "{text}");
    }
}
