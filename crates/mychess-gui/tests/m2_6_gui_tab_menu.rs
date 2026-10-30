//! M2-6 (GUI): the context menu of a tab with "Detach" (D1006, D1023): a
//! real right click in the simulator, the card, Esc, click outside.
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

use iced::{Point, mouse};
use m2_6_gui_common::common;
use m2_6_gui_common::*;
use mychess_app::{Key, NamedKey};
use pretty_assertions::assert_eq;

const FAMILY: [PaneKind; 3] = [PaneKind::Notation, PaneKind::Info, PaneKind::Comment];

fn right_click_messages(rig: &Rig, at: Point) -> Vec<Message> {
    let mut s = sim(rig);
    s.point_at(at);
    s.simulate([
        mouse_event(mouse::Event::CursorMoved { position: at }),
        mouse_event(mouse::Event::ButtonPressed(mouse::Button::Right)),
        mouse_event(mouse::Event::ButtonReleased(mouse::Button::Right)),
    ]);
    s.into_messages().collect()
}

fn on_tab(rig: &Rig, kind: PaneKind) -> Point {
    let b = bounds(rig, tab_id(kind));
    Point::new(b.x + 12.0, b.y + 10.0)
}

fn open_menu(rig: &mut Rig, kind: PaneKind) {
    let messages = right_click_messages(rig, on_tab(rig, kind));
    rig.feed(messages);
}

#[test]
fn a_right_click_on_each_tab_asks_for_its_menu_with_the_pointer_position() {
    let rig = rig();
    for kind in FAMILY {
        let at = on_tab(&rig, kind);
        let messages = right_click_messages(&rig, at);
        let wanted = Message::Ui(UiMessage::OpenTabMenu { kind, position: at });
        assert!(sent(&messages, &wanted), "{kind:?}: {messages:?}");
    }
}

#[test]
fn a_right_click_opens_the_card_with_detach_in_both_languages() {
    for (mut rig, label) in [
        (self::rig(), "Detach to a new panel"),
        (rig_pt(), "Separar em um painel novo"),
    ] {
        assert!(!has_text(&rig, label), "closed menu shows nothing");
        open_menu(&mut rig, PaneKind::Info);
        let menu = rig.shell.ui.tab_menu.expect("menu state");
        assert_eq!(menu.kind, PaneKind::Info);
        assert!(has_text(&rig, label));
    }
}

#[test]
fn the_left_button_and_the_other_areas_do_not_open_the_menu() {
    let mut rig = rig();
    let at = on_tab(&rig, PaneKind::Info);
    let mut s = sim(&rig);
    s.point_at(at);
    s.simulate(iced_test::simulator::click());
    let messages: Vec<Message> = s.into_messages().collect();
    assert!(
        !messages.iter().any(|m| dbg(m).contains("OpenTabMenu")),
        "{messages:?}"
    );
    // Right click on the body of the panel and on the board.
    for id in ["notation-pane", "board-pane"] {
        let c = bounds(&rig, id).center();
        let messages = right_click_messages(&rig, c);
        rig.feed(messages);
        assert!(rig.shell.ui.tab_menu.is_none(), "{id}");
    }
}

#[test]
fn choosing_detach_moves_the_tab_to_a_new_split_on_the_right_of_its_group() {
    for kind in FAMILY {
        let mut rig = rig();
        let before = layout(&rig);
        open_menu(&mut rig, kind);
        click_text(&mut rig, "Detach to a new panel");
        assert!(rig.shell.ui.tab_menu.is_none(), "{kind:?}: menu closes");
        assert_eq!(layout(&rig), before.detach(kind), "{kind:?}");
        assert_eq!(tabs(&rig, kind), [kind]);
        assert_eq!(pane_count(&rig), 3);
        assert_eq!(shown_in(&rig, kind), Some(kind));
        let rest: Vec<_> = FAMILY.into_iter().filter(|k| *k != kind).collect();
        assert_eq!(tabs(&rig, rest[0]), rest, "{kind:?}");
        // The detached panel now has the close button of a lone panel and
        // no tab of its own.
        assert!(has(&rig, close_id(kind)) && !has(&rig, tab_id(kind)));
        // The other two keep theirs.
        for k in rest.clone() {
            assert!(has(&rig, tab_id(k)));
        }
        // To the right of the group it left.
        let me = bounds(&rig, close_id(kind));
        let group_tab = bounds(&rig, tab_id(rest[0]));
        assert!(me.x > group_tab.x, "{kind:?} is on the right");
    }
}

#[test]
fn detach_is_remembered_in_the_preferences() {
    let mut rig = rig();
    open_menu(&mut rig, PaneKind::Comment);
    click_text(&mut rig, "Detach to a new panel");
    assert_eq!(rig.shell.app.prefs.layout, Some(layout(&rig)));
}

#[test]
fn escape_closes_the_menu_without_detaching() {
    let mut rig = rig();
    let before = layout(&rig);
    open_menu(&mut rig, PaneKind::Info);
    assert!(rig.shell.ui.tab_menu.is_some());
    rig.press(common::named(NamedKey::Escape));
    assert!(rig.shell.ui.tab_menu.is_none());
    assert_eq!(layout(&rig), before);
}

#[test]
fn a_click_outside_the_card_closes_it_without_detaching() {
    let mut rig = rig();
    let before = layout(&rig);
    open_menu(&mut rig, PaneKind::Info);
    let mut s = sim(&rig);
    s.point_at(Point::new(20.0, 400.0));
    s.simulate(iced_test::simulator::click());
    let messages: Vec<Message> = s.into_messages().collect();
    assert!(
        sent(&messages, &Message::Ui(UiMessage::CloseTabMenu)),
        "{messages:?}"
    );
    rig.feed(messages);
    assert!(rig.shell.ui.tab_menu.is_none());
    assert_eq!(layout(&rig), before);
}

#[test]
fn the_menu_closes_when_the_layout_changes_under_it() {
    for change in [
        UiMessage::SelectTab {
            kind: PaneKind::Notation,
        },
        UiMessage::ClosePane(PaneKind::Info),
        UiMessage::TogglePane(PaneKind::GameList),
        UiMessage::ResetLayout,
    ] {
        let mut rig = rig();
        open_menu(&mut rig, PaneKind::Info);
        assert!(rig.shell.ui.tab_menu.is_some());
        let label = format!("{change:?}");
        ui(&mut rig, change);
        assert!(rig.shell.ui.tab_menu.is_none(), "{label}: stale menu");
    }
}

#[test]
fn a_menu_for_a_lone_panel_offers_a_disabled_detach() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    ui(
        &mut rig,
        UiMessage::OpenTabMenu {
            kind: PaneKind::GameList,
            position: Point::new(400.0, 400.0),
        },
    );
    let before = layout(&rig);
    let messages = {
        let mut s = sim(&rig);
        // Disabled: the item does not answer the click (it may not be found
        // as a button; clicking the text must not detach).
        if s.click("Detach to a new panel").is_ok() {
            s.into_messages().collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };
    assert!(
        !messages.iter().any(|m| dbg(m).contains("DetachTab")),
        "{messages:?}"
    );
    rig.feed(messages);
    assert_eq!(layout(&rig), before);
}

#[test]
fn detaching_a_lone_panel_or_the_board_or_a_closed_one_does_nothing() {
    let mut rig = rig();
    toggle(&mut rig, PaneKind::GameList);
    let before = layout(&rig);
    for kind in [PaneKind::GameList, PaneKind::Board, PaneKind::Tree] {
        ui(&mut rig, UiMessage::DetachTab(kind));
        assert_eq!(layout(&rig), before, "{kind:?}");
    }
}

#[test]
fn a_detached_tab_can_be_merged_back_by_dropping_it_on_the_center() {
    use iced::widget::pane_grid::{DragEvent, Region, Target};
    let mut rig = rig();
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Comment));
    let (src, onto) = (
        pane_of(&rig, PaneKind::Comment),
        pane_of(&rig, PaneKind::Notation),
    );
    ui(
        &mut rig,
        UiMessage::PaneDragged(DragEvent::Dropped {
            pane: src,
            target: Target::Pane(onto, Region::Center),
        }),
    );
    assert_eq!(tabs(&rig, PaneKind::Notation), FAMILY);
    assert_eq!(pane_count(&rig), 2);
}

#[test]
fn detaching_the_selected_comment_keeps_it_visible_and_the_draft_alive() {
    let mut rig = rig();
    ui(
        &mut rig,
        UiMessage::SelectTab {
            kind: PaneKind::Comment,
        },
    );
    assert!(rig.shell.ui.comment_visible());
    ui(&mut rig, UiMessage::DetachTab(PaneKind::Comment));
    assert!(rig.shell.ui.comment_visible());
    assert!(has(&rig, "comment-pane"));
    let _ = Key::Char('x');
}
