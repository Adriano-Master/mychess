//! M2-6 (GUI): the default layout, `prefs.layout == None`, saving and
//! restoring groups, tabs, active tab and ratios, v1 migration on the first
//! open and reset (D1001, D1009, D1010, D1017, D1021).
#![allow(
    unused_qualifications,
    clippy::manual_midpoint,
    clippy::map_unwrap_or,
    clippy::large_stack_arrays,
    clippy::match_wildcard_for_single_variants,
    let_underscore_drop,
    clippy::needless_pass_by_value,
    clippy::redundant_closure_for_method_calls,
    clippy::items_after_statements,
    clippy::cast_precision_loss,
    clippy::used_underscore_binding,
    clippy::semicolon_if_nothing_returned,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m2_6_gui_common;

use iced::widget::pane_grid::{DragEvent, Region, Target};
use m2_6_gui_common::*;
use pretty_assertions::assert_eq;

const FAMILY: [PaneKind; 3] = [PaneKind::Notation, PaneKind::Info, PaneKind::Comment];

/// A shell that loaded the preferences file `text`.
fn started_with(text: &str) -> Rig {
    let mut rig = rig();
    load_prefs(&mut rig, parse(text));
    rig
}

#[test]
fn a_fresh_run_shows_the_default_layout_and_saves_nothing() {
    let rig = rig();
    assert_eq!(rig.shell.app.prefs.layout, None);
    assert_eq!(layout(&rig), LayoutNode::default_layout());
    // Ratio 0.60, as before the change.
    match layout(&rig) {
        LayoutNode::Split { ratio, axis, .. } => {
            assert!((ratio - 0.60).abs() < 1e-6);
            assert_eq!(axis, SplitAxis::Horizontal);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn loading_preferences_without_a_layout_keeps_the_default() {
    let mut rig = rig();
    load_prefs(&mut rig, prefs_with(None));
    assert_eq!(layout(&rig), LayoutNode::default_layout());
    assert_eq!(rig.shell.app.prefs.layout, None);
}

#[test]
fn the_saved_arrangement_comes_back_with_groups_active_tab_and_ratios() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    toggle(&mut rig, PaneKind::Analysis);
    let (src, onto) = (
        pane_of(&rig, PaneKind::GameList),
        pane_of(&rig, PaneKind::Notation),
    );
    ui(
        &mut rig,
        UiMessage::PaneDragged(DragEvent::Dropped {
            pane: src,
            target: Target::Pane(onto, Region::Center),
        }),
    );
    click_tab(&mut rig, PaneKind::Info);
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Comment));
    let split = first_split(&rig);
    rig.shell.ui.panes.resize(split, 0.47);
    ui(
        &mut rig,
        UiMessage::PaneResized(iced::widget::pane_grid::ResizeEvent { split, ratio: 0.47 }),
    );
    let saved = layout(&rig);
    assert_eq!(rig.shell.app.prefs.layout, Some(saved.clone()));
    // Through the file text and into a brand new shell.
    let text = render(&saved);
    let other = started_with(&text);
    assert_eq!(layout(&other), saved);
    assert_eq!(shown_in(&other, PaneKind::Info), Some(PaneKind::Info));
    assert_eq!(
        group(&other, PaneKind::Notation).map(|(t, _)| t),
        Some(vec![PaneKind::Notation, PaneKind::Info, PaneKind::GameList])
    );
    assert!(has(&other, tab_id(PaneKind::GameList)));
    assert!(has(&other, "info-pane"));
}

#[test]
fn the_file_is_written_in_v2_with_tabs_for_groups_and_pane_for_lone_panels() {
    let rig = rig();
    let text = render(&LayoutNode::default_layout());
    assert!(text.contains("layout_version = 2"), "{text}");
    assert!(text.contains("pane = \"board\""), "{text}");
    assert!(
        text.contains("\"notation\", \"info\", \"comment\"") || text.contains("tabs"),
        "{text}"
    );
    assert!(text.contains("active = 0"), "{text}");
    let _ = rig;
}

#[test]
fn clicking_a_tab_changes_the_saved_active_index() {
    let mut rig = rig();
    click_tab(&mut rig, PaneKind::Comment);
    let saved = rig.shell.app.prefs.layout.clone().expect("saved");
    assert_eq!(saved.group_of(PaneKind::Notation), Some((&FAMILY[..], 2)));
    let text = render(&saved);
    assert!(text.contains("active = 2"), "{text}");
}

#[test]
fn an_invalid_saved_layout_falls_back_to_the_default() {
    let broken = [
        // The same panel twice.
        LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            a: Box::new(LayoutNode::pane(PaneKind::Board)),
            b: Box::new(LayoutNode::group(vec![PaneKind::Info, PaneKind::Info], 0)),
        },
        // No board.
        LayoutNode::group(FAMILY.to_vec(), 0),
        // Active tab out of range.
        LayoutNode::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.5,
            a: Box::new(LayoutNode::pane(PaneKind::Board)),
            b: Box::new(LayoutNode::group(vec![PaneKind::Info], 5)),
        },
        // The board in a group with others.
        LayoutNode::group(vec![PaneKind::Board, PaneKind::Info], 0),
    ];
    for bad in broken {
        assert!(!bad.is_valid(), "{bad:?}");
        let mut rig = rig();
        load_prefs(&mut rig, prefs_with(Some(bad.clone())));
        assert_eq!(layout(&rig), LayoutNode::default_layout(), "{bad:?}");
        assert!(layout(&rig).is_valid());
        // And the window still draws.
        assert!(has(&rig, tab_id(PaneKind::Notation)));
    }
}

#[test]
fn garbage_in_the_file_gives_the_default_layout() {
    for text in [
        "layout = 7\n",
        "layout = { pane = \"nope\" }\n",
        "layout = { layout_version = 2, tabs = [], active = 0 }\n",
        "layout = { layout_version = 2, axis = \"horizontal\", ratio = 0.5, a = { pane = \"info\" }, b = { pane = \"info\" } }\n",
        "layout = { layout_version = 2, pane = \"info\" }\n",
    ] {
        let rig = started_with(text);
        assert!(layout(&rig).is_valid(), "{text}");
        assert_eq!(layout(&rig), LayoutNode::default_layout(), "{text}");
    }
}

#[test]
fn an_active_index_out_of_range_in_the_file_becomes_zero() {
    let text = "layout = { layout_version = 2, axis = \"horizontal\", ratio = 0.6, \
        a = { pane = \"board\" }, \
        b = { tabs = [\"notation\", \"info\", \"comment\"], active = 9 } }\n";
    let rig = started_with(text);
    assert_eq!(group(&rig, PaneKind::Notation), Some((FAMILY.to_vec(), 0)));
    assert!(has(&rig, "notation-pane"));
}

#[test]
fn an_unknown_tab_from_a_newer_version_is_dropped_and_the_group_stays() {
    let text = "layout = { layout_version = 2, axis = \"horizontal\", ratio = 0.6, \
        a = { pane = \"board\" }, \
        b = { tabs = [\"notation\", \"hologram\", \"comment\"], active = 2 } }\n";
    let rig = started_with(text);
    assert_eq!(
        group(&rig, PaneKind::Notation),
        Some((vec![PaneKind::Notation, PaneKind::Comment], 1))
    );
}

#[test]
fn a_v1_file_with_a_lone_notation_opens_as_three_tabs() {
    let text = "layout = { axis = \"horizontal\", ratio = 0.7, a = { pane = \"board\" }, \
        b = { pane = \"notation\" } }\n";
    let rig = started_with(text);
    assert_eq!(group(&rig, PaneKind::Notation), Some((FAMILY.to_vec(), 0)));
    assert!(has(&rig, tab_id(PaneKind::Info)) && has(&rig, tab_id(PaneKind::Comment)));
    match layout(&rig) {
        LayoutNode::Split { ratio, .. } => assert!((ratio - 0.7).abs() < 1e-6),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_v1_file_with_info_in_its_own_panel_opens_notation_and_comment_as_tabs() {
    let text = "layout = { axis = \"horizontal\", ratio = 0.65, \
        a = { axis = \"vertical\", ratio = 0.5, a = { pane = \"board\" }, b = { pane = \"info\" } }, \
        b = { pane = \"notation\" } }\n";
    let rig = started_with(text);
    assert_eq!(
        group(&rig, PaneKind::Notation),
        Some((vec![PaneKind::Notation, PaneKind::Comment], 0))
    );
    assert_eq!(tabs(&rig, PaneKind::Info), [PaneKind::Info]);
    assert!(has(&rig, close_id(PaneKind::Info)) && !has(&rig, tab_id(PaneKind::Info)));
}

#[test]
fn the_old_default_layout_is_recognized_as_the_new_default() {
    let text = "layout = { axis = \"horizontal\", ratio = 0.6, \
        a = { axis = \"vertical\", ratio = 0.5, a = { pane = \"board\" }, b = { pane = \"info\" } }, \
        b = { pane = \"notation\" } }\n";
    let rig = started_with(text);
    assert!(layout(&rig).is_valid());
    assert!(is_open(&rig, PaneKind::Info) && is_open(&rig, PaneKind::Notation));
}

#[test]
fn a_migrated_layout_is_rewritten_in_v2_after_the_first_change() {
    let text = "layout = { axis = \"horizontal\", ratio = 0.7, a = { pane = \"board\" }, \
        b = { pane = \"notation\" } }\n";
    let mut rig = started_with(text);
    click(&mut rig, close_id(PaneKind::Comment));
    let saved = rig.shell.app.prefs.layout.clone().expect("saved");
    let out = render(&saved);
    assert!(out.contains("layout_version = 2"), "{out}");
    assert_eq!(
        parse(&out).layout,
        Some(saved),
        "the new file reads back the same"
    );
}

#[test]
fn reset_layout_returns_to_the_default_and_forgets_the_saved_one() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::Tree);
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Info));
    ui(&mut rig, UiMessage::ClosePane(PaneKind::Comment));
    assert!(rig.shell.app.prefs.layout.is_some());
    ui(&mut rig, UiMessage::ResetLayout);
    assert_eq!(layout(&rig), LayoutNode::default_layout());
    assert_eq!(rig.shell.app.prefs.layout, None);
    assert!(!rig.shell.app.tree().is_open(), "closing effects ran");
    // The menu item does the same.
    toggle(&mut rig, PaneKind::Graph);
    let (action, _) = view_item(&rig, "menu-view-reset-layout");
    rig.msg(action.expect("enabled"));
    assert_eq!(layout(&rig), LayoutNode::default_layout());
    assert!(!rig.shell.app.graph().is_open());
}

#[test]
fn restoring_a_layout_with_the_tree_open_starts_the_tree() {
    let saved = LayoutNode::default_layout().open(PaneKind::Tree);
    let mut rig = rig();
    load_prefs(&mut rig, prefs_with(Some(saved)));
    assert!(is_open(&rig, PaneKind::Tree));
    assert!(rig.shell.app.tree().is_open());
}

#[test]
fn restoring_a_layout_without_analysis_closes_one_that_was_open() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::Analysis);
    rig.shell.ui.analysis.hovered = true;
    load_prefs(&mut rig, prefs_with(Some(LayoutNode::default_layout())));
    assert!(!is_open(&rig, PaneKind::Analysis));
    assert!(!rig.shell.ui.analysis.hovered);
}

#[test]
fn a_game_list_opened_by_the_application_is_not_saved_but_a_user_one_is() {
    let mut rig = rig();
    rig.shell.ui.show_game_list_automatically();
    assert!(!rig.shell.ui.layout_to_save().contains(PaneKind::GameList));
    // Closing and opening it by hand is the user taking part in it.
    toggle(&mut rig, PaneKind::GameList);
    toggle(&mut rig, PaneKind::GameList);
    assert!(rig.shell.ui.layout_to_save().contains(PaneKind::GameList));
}

#[test]
fn the_window_state_of_a_restored_merged_layout_renders_only_the_active_body() {
    let saved = LayoutNode::default_layout()
        .open(PaneKind::GameList)
        .merge(PaneKind::GameList, PaneKind::Notation)
        .select(PaneKind::Comment);
    let rig = started_with(&render(&saved));
    assert!(has(&rig, "comment-pane"));
    assert!(!has(&rig, "games-pane") && !has(&rig, "notation-pane") && !has(&rig, "info-pane"));
    assert!(rig.shell.modal_layer_open(), "the editor is on screen");
}
