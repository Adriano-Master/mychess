//! M2-6 (GUI): the dynamic tabs (Moves, Information, Comment as panels of
//! their own, D1001, D1004, D1007, D1011, D1024) and View > Panes.
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

use m2_6_gui_common::common;
use m2_6_gui_common::*;
use mychess_app::Action;
use pretty_assertions::assert_eq;

const FAMILY: [PaneKind; 3] = [PaneKind::Notation, PaneKind::Info, PaneKind::Comment];

fn body_id(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Notation => "notation-pane",
        PaneKind::Info => "info-pane",
        _ => "comment-pane",
    }
}

#[test]
fn the_default_layout_is_the_board_and_a_group_of_three_tabs_with_moves_selected() {
    let rig = rig();
    assert_eq!(layout(&rig), LayoutNode::default_layout());
    assert_eq!(
        group(&rig, PaneKind::Notation),
        Some((FAMILY.to_vec(), 0usize))
    );
    assert_eq!(tabs(&rig, PaneKind::Board), [PaneKind::Board]);
    assert_eq!(pane_count(&rig), 2);
    for kind in FAMILY {
        assert!(has(&rig, tab_id(kind)));
    }
}

#[test]
fn only_the_body_of_the_selected_tab_is_built() {
    let mut rig = rig();
    for shown in FAMILY {
        click_tab(&mut rig, shown);
        for kind in FAMILY {
            assert_eq!(
                has(&rig, body_id(kind)),
                kind == shown,
                "{shown:?}/{kind:?}"
            );
        }
    }
}

#[test]
fn clicking_a_tab_sends_select_tab_for_that_panel() {
    let rig = rig();
    for kind in FAMILY {
        let b = bounds(&rig, tab_id(kind));
        let mut s = sim(&rig);
        s.point_at(iced::Point::new(b.x + 12.0, b.y + 10.0));
        s.simulate(iced_test::simulator::click());
        let messages: Vec<Message> = s.into_messages().collect();
        assert!(
            sent(&messages, &Message::Ui(UiMessage::SelectTab { kind })),
            "{kind:?}: {messages:?}"
        );
    }
}

#[test]
fn the_tab_labels_are_the_translated_titles_in_both_languages() {
    for (rig, labels) in [
        (rig(), ["Moves", "Information", "Comment"]),
        (rig_pt(), ["Lances", "Informações", "Comentário"]),
    ] {
        for label in labels {
            assert!(has_text(&rig, label), "label {label}");
        }
    }
}

#[test]
fn the_selected_tab_is_marked_apart_from_the_others() {
    // The underline of the tab is a container: compare the render of the
    // window with Moves selected against Information selected.
    let mut rig = rig();
    let first = fingerprint(&rig);
    click_tab(&mut rig, PaneKind::Info);
    let second = fingerprint(&rig);
    assert_ne!(first, second);
}

fn fingerprint(rig: &Rig) -> String {
    let mut s = sim(rig);
    common::fingerprint(&mut s)
}

#[test]
fn the_three_tabs_close_and_reopen_independently_through_both_routes() {
    for kind in FAMILY {
        for via_menu in [false, true] {
            let mut rig = rig();
            if via_menu {
                menu_toggle(&mut rig, kind);
            } else {
                click(&mut rig, close_id(kind));
            }
            assert!(!is_open(&rig, kind) && !pane_checked(&rig, kind));
            for other in FAMILY.into_iter().filter(|k| *k != kind) {
                assert!(is_open(&rig, other) && pane_checked(&rig, other));
            }
            menu_toggle(&mut rig, kind);
            assert!(is_open(&rig, kind) && pane_checked(&rig, kind));
            assert_eq!(shown_in(&rig, kind), Some(kind), "reopened and selected");
            assert_eq!(tabs(&rig, kind).len(), 3);
            assert!(has(&rig, body_id(kind)));
            assert_eq!(
                tabs(&rig, kind).last().copied(),
                Some(kind),
                "a reopened tab goes to the end of the group"
            );
        }
    }
}

#[test]
fn view_panes_lists_the_seven_panels_and_no_notation_tab_submenu() {
    let rig = rig();
    for kind in CLOSABLE {
        let (action, checked) = view_item(&rig, pane_menu_key(kind));
        assert!(action.is_some(), "{kind:?} enabled");
        assert_eq!(checked, Some(FAMILY.contains(&kind)), "{kind:?}");
        assert!(
            sent(
                &[action.unwrap()],
                &Message::Ui(UiMessage::TogglePane(kind))
            ),
            "{kind:?} toggles itself"
        );
    }
    assert!(!view_has(&rig, "menu-view-tabs"), "D1004: submenu removed");
    assert!(view_has(&rig, "menu-view-panes"));
}

#[test]
fn the_checked_state_of_the_menu_follows_the_layout_for_every_panel() {
    let mut rig = rig();
    for kind in CLOSABLE {
        menu_toggle(&mut rig, kind);
        assert_eq!(pane_checked(&rig, kind), is_open(&rig, kind), "{kind:?}");
        menu_toggle(&mut rig, kind);
        assert_eq!(pane_checked(&rig, kind), is_open(&rig, kind), "{kind:?}");
    }
}

#[test]
fn reopening_follows_d1004_with_nothing_of_the_family_open() {
    let mut rig = rig();
    for kind in FAMILY {
        click(&mut rig, close_id(kind));
    }
    // Info comes back beside the board (ratio 0.60), alone.
    menu_toggle(&mut rig, PaneKind::Info);
    assert_eq!(
        layout(&rig),
        LayoutNode::pane(PaneKind::Board).open(PaneKind::Info)
    );
    assert_eq!(tabs(&rig, PaneKind::Info), [PaneKind::Info]);
    // The others join its group as tabs.
    menu_toggle(&mut rig, PaneKind::Comment);
    menu_toggle(&mut rig, PaneKind::Notation);
    assert_eq!(
        group(&rig, PaneKind::Info),
        Some((
            vec![PaneKind::Info, PaneKind::Comment, PaneKind::Notation],
            2
        ))
    );
    assert_eq!(pane_count(&rig), 2);
}

#[test]
fn opening_the_other_panels_does_not_touch_the_tabs_of_the_group() {
    let mut rig = rig();
    click_tab(&mut rig, PaneKind::Info);
    for kind in [
        PaneKind::GameList,
        PaneKind::Analysis,
        PaneKind::Graph,
        PaneKind::Tree,
    ] {
        menu_toggle(&mut rig, kind);
        assert_eq!(
            group(&rig, PaneKind::Notation),
            Some((FAMILY.to_vec(), 1)),
            "{kind:?} changed the group"
        );
    }
}

#[test]
fn show_notation_selects_the_moves_tab_and_opens_it_when_closed() {
    let mut rig = rig();
    click_tab(&mut rig, PaneKind::Info);
    ui(&mut rig, UiMessage::Action(Action::ShowNotation));
    assert_eq!(shown_in(&rig, PaneKind::Notation), Some(PaneKind::Notation));
    // Closed: it is opened (as a tab) and shown.
    click(&mut rig, close_id(PaneKind::Notation));
    ui(&mut rig, UiMessage::Action(Action::ShowNotation));
    assert!(is_open(&rig, PaneKind::Notation));
    assert_eq!(shown_in(&rig, PaneKind::Notation), Some(PaneKind::Notation));
}

#[test]
fn selecting_a_tab_of_a_closed_panel_opens_and_selects_it() {
    let mut rig = rig();
    click(&mut rig, close_id(PaneKind::Info));
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Info,
        },
    );
    assert!(is_open(&rig, PaneKind::Info));
    assert_eq!(shown_in(&rig, PaneKind::Info), Some(PaneKind::Info));
    // Selecting the board (no tab) is harmless.
    let before = layout(&rig);
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Board,
        },
    );
    assert_eq!(layout(&rig), before);
}

#[test]
fn the_comment_dot_marks_the_comment_tab_only_when_the_cursor_has_a_comment() {
    let mut s = common::sample();
    s.game.set_comment(s.e4, "hello").unwrap();
    let (e4, e5) = (s.e4, s.e5);
    let mut rig = Rig::with_game(s.game);
    rig.goto(e4);
    assert!(has_text(&rig, "\u{25cf}"), "dot on the comment tab");
    // Visible whichever tab is selected.
    click_tab(&mut rig, PaneKind::Info);
    assert!(has_text(&rig, "\u{25cf}"));
    rig.goto(e5);
    assert!(!has_text(&rig, "\u{25cf}"), "no comment, no dot");
}
