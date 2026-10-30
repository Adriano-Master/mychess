//! M2-6 (GUI): merging by dropping on the center of a panel, splitting on
//! the edges, the board that takes no part (D1005, D1019), with real pointer
//! gestures in the simulator and with the drag messages.
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

use iced::widget::pane_grid::{self, DragEvent, Edge, Region, Target};
use iced::{Point, Rectangle};
use m2_6_gui_common::*;
use pretty_assertions::assert_eq;

const FAMILY: [PaneKind; 3] = [PaneKind::Notation, PaneKind::Info, PaneKind::Comment];

fn drop_msg(pane: pane_grid::Pane, onto: pane_grid::Pane, region: Region) -> Message {
    Message::Ui(UiMessage::PaneDragged(DragEvent::Dropped {
        pane,
        target: Target::Pane(onto, region),
    }))
}

fn drop_on(rig: &mut Rig, source: PaneKind, onto: PaneKind, region: Region) {
    let (p, o) = (pane_of(rig, source), pane_of(rig, onto));
    rig.msg(drop_msg(p, o, region));
}

fn with_games() -> Rig {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    rig
}

// ---- by messages ----

#[test]
fn dropping_on_the_center_appends_the_tabs_and_selects_the_dragged_active_one() {
    let mut rig = with_games();
    drop_on(
        &mut rig,
        PaneKind::GameList,
        PaneKind::Notation,
        Region::Center,
    );
    assert_eq!(
        group(&rig, PaneKind::Notation),
        Some((
            vec![
                PaneKind::Notation,
                PaneKind::Info,
                PaneKind::Comment,
                PaneKind::GameList
            ],
            3
        ))
    );
    assert_eq!(pane_count(&rig), 2, "the games pane is gone");
    assert!(layout(&rig).is_valid());
    assert!(has(&rig, tab_id(PaneKind::GameList)));
    assert!(pane_checked(&rig, PaneKind::GameList));
}

#[test]
fn dropping_a_whole_group_merges_all_its_tabs_in_order() {
    let mut rig = with_games();
    // Select Info in the dragged group first: it stays the active one.
    click_tab(&mut rig, PaneKind::Info);
    drop_on(
        &mut rig,
        PaneKind::Notation,
        PaneKind::GameList,
        Region::Center,
    );
    assert_eq!(
        group(&rig, PaneKind::GameList),
        Some((
            vec![
                PaneKind::GameList,
                PaneKind::Notation,
                PaneKind::Info,
                PaneKind::Comment
            ],
            2
        ))
    );
    assert_eq!(shown_in(&rig, PaneKind::Info), Some(PaneKind::Info));
}

#[test]
fn a_merge_that_hides_the_comment_saves_the_draft() {
    use mychess_gui::windows::comment_editor::EditorMessage;
    use mychess_gui::windows::notation::NotationMessage;
    let mut rig = with_games();
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Comment,
        },
    );
    rig.msg(Message::Notation(NotationMessage::Editor(
        EditorMessage::Action(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Insert('d'),
        )),
    )));
    // The game list (active 0) joins the group and takes the front.
    drop_on(
        &mut rig,
        PaneKind::GameList,
        PaneKind::Notation,
        Region::Center,
    );
    assert!(!rig.shell.ui.comment_visible());
    assert_eq!(rig.shell.app.session.game().initial_comment(), Some("d"));
}

#[test]
fn dropping_on_each_edge_splits_instead_of_merging() {
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        let mut rig = with_games();
        drop_on(
            &mut rig,
            PaneKind::GameList,
            PaneKind::Notation,
            Region::Edge(edge),
        );
        assert_eq!(
            tabs(&rig, PaneKind::GameList),
            [PaneKind::GameList],
            "{edge:?}"
        );
        assert_eq!(tabs(&rig, PaneKind::Notation), FAMILY, "{edge:?}");
        assert_eq!(pane_count(&rig), 3, "{edge:?}");
        assert!(layout(&rig).is_valid());
    }
}

#[test]
fn the_board_takes_no_part_in_a_merge_in_either_direction() {
    let mut rig = with_games();
    let before = layout(&rig);
    for (src, onto) in [
        (PaneKind::GameList, PaneKind::Board),
        (PaneKind::Board, PaneKind::GameList),
        (PaneKind::Notation, PaneKind::Board),
        (PaneKind::Board, PaneKind::Notation),
    ] {
        drop_on(&mut rig, src, onto, Region::Center);
        assert_eq!(layout(&rig), before, "{src:?} onto {onto:?}");
    }
}

#[test]
fn dropping_a_pane_on_its_own_center_changes_nothing() {
    let mut rig = with_games();
    let before = layout(&rig);
    for kind in [PaneKind::GameList, PaneKind::Notation, PaneKind::Board] {
        drop_on(&mut rig, kind, kind, Region::Center);
        assert_eq!(layout(&rig), before, "{kind:?}");
    }
}

#[test]
fn a_canceled_or_picked_drag_changes_nothing() {
    let mut rig = with_games();
    let before = layout(&rig);
    let pane = pane_of(&rig, PaneKind::GameList);
    ui(&mut rig, UiMessage::PaneDragged(DragEvent::Picked { pane }));
    ui(
        &mut rig,
        UiMessage::PaneDragged(DragEvent::Canceled { pane }),
    );
    assert_eq!(layout(&rig), before);
}

#[test]
fn dropping_on_the_outer_edge_of_the_window_area_splits_the_whole_grid() {
    let mut rig = with_games();
    let pane = pane_of(&rig, PaneKind::GameList);
    rig.msg(Message::Ui(UiMessage::PaneDragged(DragEvent::Dropped {
        pane,
        target: Target::Pane(pane_of(&rig, PaneKind::Board), Region::Edge(Edge::Right)),
    })));
    assert_eq!(tabs(&rig, PaneKind::GameList), [PaneKind::GameList]);
    assert!(layout(&rig).is_valid());
}

#[test]
fn merging_preserves_the_state_of_the_merged_panels() {
    // Data effects follow `open` (the tree keeps searching, the graph stays
    // open); keyboard and focus follow `visible` (D1027): a hidden analysis
    // lets go of the focus.
    let mut rig = rig();
    toggle(&mut rig, PaneKind::Tree);
    toggle(&mut rig, PaneKind::Graph);
    toggle(&mut rig, PaneKind::Analysis);
    toggle(&mut rig, PaneKind::GameList);
    rig.shell.ui.analysis.hovered = true;
    for kind in [
        PaneKind::Tree,
        PaneKind::Graph,
        PaneKind::Analysis,
        PaneKind::GameList,
    ] {
        drop_on(&mut rig, kind, PaneKind::Notation, Region::Center);
        assert!(is_open(&rig, kind), "{kind:?} lost");
    }
    assert!(rig.shell.app.tree().is_open(), "tree still searching");
    assert!(rig.shell.app.graph().is_open());
    assert!(
        !rig.shell.ui.analysis.hovered,
        "a hidden analysis panel keeps no hover/focus (D1027)"
    );
    assert_eq!(pane_count(&rig), 2);
    // Hidden behind the active tab, yet open: the menu keeps them checked.
    for kind in [PaneKind::Tree, PaneKind::Graph, PaneKind::Analysis] {
        assert!(pane_checked(&rig, kind), "{kind:?}");
    }
}

#[test]
fn the_game_list_keeps_its_content_when_merged_and_shown_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.pgn");
    std::fs::write(
        &path,
        "[White \"Aaa\"]\n\n1. e4 e5 1-0\n\n[White \"Bbb\"]\n\n1. d4 d5 1-0\n\n",
    )
    .unwrap();
    let mut rig = rig();
    rig.shell.app.open_pgn_in_slots = true;
    rig.send(mychess_app::BasesMessage::Open(path));
    for _ in 0..50 {
        if is_open(&rig, PaneKind::GameList) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !is_open(&rig, PaneKind::GameList) {
        toggle(&mut rig, PaneKind::GameList);
    }
    drop_on(
        &mut rig,
        PaneKind::GameList,
        PaneKind::Notation,
        Region::Center,
    );
    assert_eq!(shown_in(&rig, PaneKind::GameList), Some(PaneKind::GameList));
    assert!(has(&rig, "games-pane") || is_open(&rig, PaneKind::GameList));
    click_tab(&mut rig, PaneKind::Info);
    click_tab(&mut rig, PaneKind::GameList);
    assert_eq!(shown_in(&rig, PaneKind::GameList), Some(PaneKind::GameList));
}

#[test]
fn the_command_bar_follows_the_move_list_into_a_merged_group() {
    let mut rig = with_games();
    assert!(has(&rig, MOVE_FIELD_ID));
    drop_on(
        &mut rig,
        PaneKind::GameList,
        PaneKind::Notation,
        Region::Center,
    );
    // GameList is the selected tab now: the bar is under it all the same.
    assert_eq!(shown_in(&rig, PaneKind::Notation), Some(PaneKind::GameList));
    assert!(has(&rig, MOVE_FIELD_ID));
    // Move the move list out into a group of its own: the bar goes with it.
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Notation));
    assert!(has(&rig, MOVE_FIELD_ID));
    let owner = rig.shell.ui.command_bar_owner().expect("an owner");
    assert_eq!(owner, pane_of(&rig, PaneKind::Notation));
}

#[test]
fn the_merge_is_remembered_in_the_preferences() {
    let mut rig = with_games();
    drop_on(
        &mut rig,
        PaneKind::GameList,
        PaneKind::Notation,
        Region::Center,
    );
    let saved = rig.shell.app.prefs.layout.clone().expect("remembered");
    assert_eq!(saved, layout(&rig));
    assert_eq!(
        saved.group_of(PaneKind::GameList).map(|(t, _)| t.len()),
        Some(4)
    );
}

// ---- by real pointer gestures ----

fn gesture(rig: &mut Rig, from: Point, to: Point) -> Vec<Message> {
    let messages = drag_messages(rig, from, to);
    rig.feed(messages.clone());
    messages
}

#[test]
fn a_real_drag_of_the_game_list_title_onto_the_notation_body_merges_them() {
    let mut rig = with_games();
    let from = grip_point(&rig, PaneKind::GameList);
    let to = center_of(&rig, "notation-pane");
    let messages = gesture(&mut rig, from, to);
    assert!(
        picked(&messages),
        "the press on the free bar space picks: {messages:?}"
    );
    assert_eq!(dropped(&messages).len(), 1, "{messages:?}");
    assert_eq!(
        tabs(&rig, PaneKind::Notation),
        [
            PaneKind::Notation,
            PaneKind::Info,
            PaneKind::Comment,
            PaneKind::GameList
        ]
    );
}

#[test]
fn a_real_drag_onto_the_bottom_edge_of_a_panel_splits() {
    let mut rig = with_games();
    let from = grip_point(&rig, PaneKind::GameList);
    let body: Rectangle = bounds(&rig, "notation-pane");
    let to = Point::new(body.x + body.width / 2.0, body.y + body.height - 2.0);
    let messages = gesture(&mut rig, from, to);
    assert_eq!(dropped(&messages).len(), 1, "{messages:?}");
    assert_eq!(tabs(&rig, PaneKind::GameList), [PaneKind::GameList]);
    assert_eq!(tabs(&rig, PaneKind::Notation), FAMILY);
    assert_eq!(pane_count(&rig), 3);
}

#[test]
fn a_real_drag_onto_the_board_is_ignored() {
    let mut rig = with_games();
    let before = layout(&rig);
    let from = grip_point(&rig, PaneKind::GameList);
    let board = bounds(&rig, "board-pane");
    let messages = gesture(&mut rig, from, board.center());
    assert_eq!(dropped(&messages).len(), 1, "{messages:?}");
    assert_eq!(layout(&rig), before);
}

#[test]
fn a_real_drag_of_the_tab_group_title_onto_another_panel_merges_every_tab() {
    let mut rig = with_games();
    let from = grip_point(&rig, PaneKind::Notation);
    let games = bounds(&rig, "games-pane");
    let messages = gesture(&mut rig, from, games.center());
    assert!(
        picked(&messages),
        "free space right of the tabs: {messages:?}"
    );
    assert_eq!(pane_count(&rig), 2);
    assert_eq!(tabs(&rig, PaneKind::GameList).len(), 4);
}

#[test]
fn pressing_a_tab_never_starts_a_drag() {
    let rig = rig();
    for kind in FAMILY {
        let b = bounds(&rig, tab_id(kind));
        let from = Point::new(b.x + 12.0, b.y + 10.0);
        let to = center_of(&rig, "notation-pane");
        let messages = drag_messages(&rig, from, Point::new(to.x - 150.0, to.y + 100.0));
        assert!(!picked(&messages), "{kind:?}: {messages:?}");
    }
}

#[test]
fn pressing_the_close_button_never_starts_a_drag() {
    let mut rig = with_games();
    let b = bounds(&rig, close_id(PaneKind::GameList));
    let from = b.center();
    let messages = drag_messages(&rig, from, Point::new(from.x - 400.0, from.y - 300.0));
    assert!(!picked(&messages), "{messages:?}");
    let _ = &mut rig;
}

#[test]
fn releasing_over_no_pane_cancels_the_drag() {
    let mut rig = with_games();
    let before = layout(&rig);
    let from = grip_point(&rig, PaneKind::GameList);
    let messages = gesture(&mut rig, from, Point::new(-50.0, -50.0));
    assert!(picked(&messages));
    assert_eq!(layout(&rig), before);
}

#[test]
fn a_group_of_tabs_leaves_free_space_in_the_title_bar_to_grab() {
    let rig = rig();
    let last = bounds(&rig, tab_id(PaneKind::Comment));
    let body = bounds(&rig, "notation-pane");
    let free = (body.x + body.width) - (last.x + last.width);
    assert!(free >= 24.0, "only {free}px of free bar after the last tab");
}

/// A staircase with every panel in its own pane: as deep as the format allows.
fn deep_chain() -> LayoutNode {
    let mut node = LayoutNode::pane(PaneKind::Board);
    for (i, kind) in CLOSABLE.into_iter().enumerate() {
        node = LayoutNode::Split {
            axis: if i % 2 == 0 {
                SplitAxis::Horizontal
            } else {
                SplitAxis::Vertical
            },
            ratio: 0.5,
            a: Box::new(node),
            b: Box::new(LayoutNode::pane(kind)),
        };
    }
    node
}

#[test]
fn every_edge_drop_on_the_deepest_layout_leaves_a_valid_layout_and_refuses_invalid_ones() {
    use mychess_gui::windows::layout::to_configuration;
    let chain = deep_chain();
    assert!(
        chain.is_valid(),
        "the staircase itself is allowed: {chain:?}"
    );
    let mut raw_invalid = 0;
    for src in 0..8 {
        for dst in 0..8 {
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                if src == dst {
                    continue;
                }
                // What the bare pane grid would do.
                let mut raw = pane_grid::State::with_configuration(to_configuration(&chain));
                let ids: Vec<_> = raw.iter().map(|(p, _)| *p).collect();
                raw.drop(ids[src], Target::Pane(ids[dst], Region::Edge(edge)));
                let raw_ok =
                    mychess_gui::windows::layout::from_state(&raw).is_some_and(|l| l.is_valid());
                // What the shell does with the same gesture.
                let mut rig = rig();
                rig.shell.ui.set_layout(&chain);
                let ids: Vec<_> = rig.shell.ui.panes.iter().map(|(p, _)| *p).collect();
                rig.msg(Message::Ui(UiMessage::PaneDragged(DragEvent::Dropped {
                    pane: ids[src],
                    target: Target::Pane(ids[dst], Region::Edge(edge)),
                })));
                let after = layout(&rig);
                assert!(after.is_valid(), "{src}->{dst} {edge:?}: {after:?}");
                if !raw_ok {
                    raw_invalid += 1;
                    assert_eq!(after, chain, "{src}->{dst} {edge:?} must be refused");
                }
            }
        }
    }
    // With at most eight panels a nest deeper than the limit cannot be built
    // by the gestures, so the refusal path is a guard that no real layout
    // reaches (gap recorded in the report).
    assert_eq!(raw_invalid, 0);
}
