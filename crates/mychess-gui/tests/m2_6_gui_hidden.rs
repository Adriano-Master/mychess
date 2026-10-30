//! M2-6 (GUI), D1008 and D1027: "open" drives data effects, "visible" drives
//! keyboard, focus and popups. A panel behind another tab is open but gets
//! no keys and holds no focus.
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
    clippy::expect_used
)]

mod m2_6_gui_common;

use m2_6_gui_common::*;

/// The game list as a hidden tab of the notation group.
fn hidden_games() -> Rig {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    let merged = layout(&rig).merge(PaneKind::GameList, PaneKind::Notation);
    rig.shell.ui.set_layout(&merged);
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Notation,
        },
    );
    rig
}

#[test]
fn a_hidden_game_list_is_open_but_not_visible() {
    let rig = hidden_games();
    assert!(is_open(&rig, PaneKind::GameList));
    assert!(!rig.shell.ui.is_visible(PaneKind::GameList));
    assert!(
        pane_checked(&rig, PaneKind::GameList),
        "still checked in the menu"
    );
    assert!(!has(&rig, "games-pane"), "its body is not built");
}

#[test]
fn hiding_the_game_list_drops_its_transient_state() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    let merged = layout(&rig).merge(PaneKind::GameList, PaneKind::Notation);
    rig.shell.ui.set_layout(&merged);
    rig.shell.ui.games.search_focused = true;
    rig.shell.ui.games.hovered = true;
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Notation,
        },
    );
    assert!(!rig.shell.ui.games.search_focused);
    assert!(!rig.shell.ui.games.hovered);
}

#[test]
fn hiding_the_analysis_lets_go_of_its_focus_and_keeps_it_open() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::Analysis);
    let merged = layout(&rig).merge(PaneKind::Analysis, PaneKind::Notation);
    rig.shell.ui.set_layout(&merged);
    rig.shell.ui.analysis.hovered = true;
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Notation,
        },
    );
    assert!(!rig.shell.ui.analysis.hovered);
    assert!(is_open(&rig, PaneKind::Analysis));
    assert!(!rig.shell.modal_layer_open() || rig.shell.ui.comment_visible());
}

#[test]
fn a_hidden_tree_keeps_searching_and_a_hidden_graph_stays_open() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::Tree);
    toggle(&mut rig, PaneKind::Graph);
    for kind in [PaneKind::Tree, PaneKind::Graph] {
        let merged = layout(&rig).merge(kind, PaneKind::Notation);
        rig.shell.ui.set_layout(&merged);
    }
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Notation,
        },
    );
    assert!(rig.shell.app.tree().is_open());
    assert!(rig.shell.app.graph().is_open());
}

#[test]
fn a_saved_layout_that_hides_the_comment_does_not_leave_a_draft_behind() {
    let mut rig = rig();
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Comment,
        },
    );
    rig.msg(Message::Notation(
        mychess_gui::windows::notation::NotationMessage::Editor(
            mychess_gui::windows::comment_editor::EditorMessage::Action(
                iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Insert(
                    'q',
                )),
            ),
        ),
    ));
    // A restored layout with Moves in front.
    load_prefs(&mut rig, prefs_with(Some(LayoutNode::default_layout())));
    assert!(!rig.shell.ui.comment_visible());
    assert!(!rig.shell.ui.notation.editor.is_dirty());
    assert_eq!(rig.shell.app.session.game().initial_comment(), Some("q"));
}
