//! Shared helpers of the `m2_6_gui_*` contract tests (spec
//! `docs/specs/m2-6-paineis-com-abas.md`, D1000-D1025; ADR 0008): panels with
//! a close button and dynamic tabs, headless with `iced_test`.
//!
//! Everything here goes through the public surface the user touches: the
//! real main window in a simulator, its messages fed back to the shell.
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
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::float_cmp
)]

#[path = "../m2_4_common/mod.rs"]
pub mod common;

use std::path::Path;

use iced::widget::pane_grid;
use iced::{Event, Point, Rectangle, Size, mouse, widget};
pub use iced_test::Simulator;
pub use mychess_app::{
    Action as AppAction, General, LanguagePref, LayoutNode, LoadedPreferences,
    Message as AppMessage, Preferences, SplitAxis, ThemePref,
};
pub use mychess_gui::windows::layout::PaneKind;
pub use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
pub use mychess_gui::windows::nav_bar::MOVE_FIELD_ID;
pub use mychess_gui::windows::pane_tabs::{close_id, tab_id};
pub use mychess_gui::{Message, Shell, UiMessage};

pub use common::{Rig, command, plain};

pub const SIZE: Size = Size::new(1280.0, 800.0);

/// Every panel the layout knows, board first.
pub const ALL: [PaneKind; 8] = [
    PaneKind::Board,
    PaneKind::Notation,
    PaneKind::Info,
    PaneKind::Comment,
    PaneKind::GameList,
    PaneKind::Analysis,
    PaneKind::Graph,
    PaneKind::Tree,
];

/// The panels that have a close button (all but the board).
pub const CLOSABLE: [PaneKind; 7] = [
    PaneKind::Notation,
    PaneKind::Info,
    PaneKind::Comment,
    PaneKind::GameList,
    PaneKind::Analysis,
    PaneKind::Graph,
    PaneKind::Tree,
];

pub fn rig() -> Rig {
    Rig::new()
}

pub fn rig_pt() -> Rig {
    Rig::with_language(LanguagePref::PtBr)
}

pub fn ui(rig: &mut Rig, message: UiMessage) {
    rig.msg(Message::Ui(message));
}

pub fn toggle(rig: &mut Rig, kind: PaneKind) {
    ui(rig, UiMessage::TogglePane(kind));
}

pub fn is_open(rig: &Rig, kind: PaneKind) -> bool {
    rig.shell.ui.is_open(kind)
}

pub fn layout(rig: &Rig) -> LayoutNode {
    rig.shell.ui.layout()
}

/// Tabs and active index of the group that holds `kind`.
pub fn group(rig: &Rig, kind: PaneKind) -> Option<(Vec<PaneKind>, usize)> {
    layout(rig)
        .group_of(kind)
        .map(|(tabs, active)| (tabs.to_vec(), active))
}

pub fn tabs(rig: &Rig, kind: PaneKind) -> Vec<PaneKind> {
    group(rig, kind).map(|(t, _)| t).unwrap_or_default()
}

/// The panel shown by the group that holds `kind`.
pub fn shown_in(rig: &Rig, kind: PaneKind) -> Option<PaneKind> {
    group(rig, kind).and_then(|(t, a)| t.get(a).copied())
}

pub fn pane_of(rig: &Rig, kind: PaneKind) -> pane_grid::Pane {
    *rig.shell
        .ui
        .panes
        .iter()
        .find(|(_, g)| g.contains(kind))
        .map(|(pane, _)| pane)
        .unwrap_or_else(|| panic!("{kind:?} is not open"))
}

pub fn pane_count(rig: &Rig) -> usize {
    rig.shell.ui.panes.len()
}

pub fn sim(rig: &Rig) -> Simulator<'_, Message> {
    rig.sim(SIZE)
}

pub fn has(rig: &Rig, id: &'static str) -> bool {
    sim(rig).find(widget::Id::new(id)).is_ok()
}

pub fn has_text(rig: &Rig, text: &str) -> bool {
    sim(rig).find(text).is_ok()
}

pub fn bounds(rig: &Rig, id: &'static str) -> Rectangle {
    sim(rig)
        .find(widget::Id::new(id))
        .unwrap_or_else(|_| panic!("widget {id}"))
        .bounds()
}

/// Clicks the widget with `id` and returns what it sent (nothing applied).
pub fn click_messages(rig: &Rig, id: &'static str) -> Vec<Message> {
    let mut sim = sim(rig);
    sim.click(widget::Id::new(id))
        .unwrap_or_else(|e| panic!("click {id}: {e:?}"));
    sim.into_messages().collect()
}

/// Clicks the widget with `id` and applies the messages it sent.
pub fn click(rig: &mut Rig, id: &'static str) -> Vec<Message> {
    let messages = click_messages(rig, id);
    assert!(!messages.is_empty(), "{id} sent nothing");
    rig.feed(messages.clone());
    messages
}

/// Clicks the text in the window and applies what it sent.
pub fn click_text(rig: &mut Rig, text: &'static str) -> Vec<Message> {
    let messages = {
        let mut sim = sim(rig);
        sim.click(text)
            .unwrap_or_else(|e| panic!("click {text}: {e:?}"));
        sim.into_messages().collect::<Vec<_>>()
    };
    rig.feed(messages.clone());
    messages
}

pub fn dbg(message: &Message) -> String {
    format!("{message:?}")
}

pub fn sent(messages: &[Message], expected: &Message) -> bool {
    let wanted = dbg(expected);
    messages.iter().any(|m| dbg(m) == wanted)
}

// ---- the menu ----

pub fn menu(rig: &Rig, key: &str) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == key)
        .unwrap_or_else(|| panic!("menu {key}"))
        .entries
}

/// Every item (and submenu key) of the entries, flattened: key, action,
/// checked.
pub fn flat(entries: &[MenuEntry]) -> Vec<(&'static str, Option<Message>, Option<bool>)> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            MenuEntry::Item {
                label: Label::Key(key),
                action,
                checked,
                ..
            } => out.push((*key, action.clone(), *checked)),
            MenuEntry::Submenu { label, entries, .. } => {
                if let Label::Key(key) = label {
                    out.push((*key, None, None));
                }
                out.extend(flat(entries));
            }
            _ => {}
        }
    }
    out
}

/// `(action, checked)` of the item of View with this key.
pub fn view_item(rig: &Rig, key: &str) -> (Option<Message>, Option<bool>) {
    flat(&menu(rig, "menu-view"))
        .into_iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, a, c)| (a, c))
        .unwrap_or_else(|| panic!("View item {key}"))
}

pub fn view_has(rig: &Rig, key: &str) -> bool {
    flat(&menu(rig, "menu-view"))
        .iter()
        .any(|(k, _, _)| *k == key)
}

/// The Panes item of a panel: the menu key and whether it is checked.
pub fn pane_menu_key(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::Notation => "menu-view-pane-notation",
        PaneKind::Info => "menu-view-pane-info",
        PaneKind::Comment => "menu-view-pane-comment",
        PaneKind::GameList => "menu-view-pane-games",
        PaneKind::Analysis => "menu-view-pane-analysis",
        PaneKind::Graph => "menu-view-pane-graph",
        PaneKind::Tree => "menu-view-pane-tree",
        _ => panic!("the board has no menu item"),
    }
}

pub fn pane_checked(rig: &Rig, kind: PaneKind) -> bool {
    view_item(rig, pane_menu_key(kind)).1.expect("checkable")
}

/// Does what choosing View > Panes > `kind` does.
pub fn menu_toggle(rig: &mut Rig, kind: PaneKind) {
    let (action, _) = view_item(rig, pane_menu_key(kind));
    rig.msg(action.expect("enabled item"));
}

// ---- pointer ----

pub fn mouse_event(event: mouse::Event) -> Event {
    Event::Mouse(event)
}

/// A real gesture: press at `from`, move to `to`, release.
pub fn drag_messages(rig: &Rig, from: Point, to: Point) -> Vec<Message> {
    let mut sim = sim(rig);
    sim.point_at(from);
    sim.simulate([
        mouse_event(mouse::Event::CursorMoved { position: from }),
        mouse_event(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ]);
    let mid = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
    sim.point_at(mid);
    sim.simulate([mouse_event(mouse::Event::CursorMoved { position: mid })]);
    sim.point_at(to);
    sim.simulate([
        mouse_event(mouse::Event::CursorMoved { position: to }),
        mouse_event(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]);
    sim.into_messages().collect()
}

/// A free spot of the title bar of the group (left of its close button when
/// it is alone, otherwise right of the last tab).
pub fn grip_point(rig: &Rig, kind: PaneKind) -> Point {
    let (tabs, _) = group(rig, kind).expect("open");
    if tabs.len() == 1 {
        let close = bounds(rig, close_id(kind));
        Point::new(close.x - 30.0, close.y + close.height / 2.0)
    } else {
        // The tabs fill the bar (see the ignored bug test in `m2_6_gui_drag`):
        // the only pick area left is the padding strip above them.
        let first = tabs.first().copied().expect("a tab");
        let b = bounds(rig, tab_id(first));
        Point::new(b.x + 20.0, b.y - 1.0)
    }
}

/// The middle of the body of a pane (a point of its `Center` region).
pub fn center_of(rig: &Rig, body_id: &'static str) -> Point {
    bounds(rig, body_id).center()
}

pub fn dropped(messages: &[Message]) -> Vec<&Message> {
    messages
        .iter()
        .filter(|m| dbg(m).contains("Dropped"))
        .collect()
}

pub fn picked(messages: &[Message]) -> bool {
    messages.iter().any(|m| dbg(m).contains("Picked"))
}

// ---- preferences ----

pub fn load_prefs(rig: &mut Rig, prefs: Preferences) {
    rig.app_msg(AppMessage::PreferencesLoaded(LoadedPreferences {
        prefs,
        issues: Vec::new(),
    }));
}

pub fn prefs_with(layout: Option<LayoutNode>) -> Preferences {
    Preferences {
        general: General {
            language: LanguagePref::EnUs,
            theme: ThemePref::Light,
            ..General::default()
        },
        layout,
        ..Preferences::default()
    }
}

/// Preferences file text holding `layout`.
pub fn render(layout: &LayoutNode) -> String {
    mychess_app::render_preferences(&Preferences {
        layout: Some(layout.clone()),
        ..Preferences::default()
    })
}

pub fn parse(text: &str) -> Preferences {
    mychess_app::load_preferences(text).prefs
}

pub fn first_split(rig: &Rig) -> pane_grid::Split {
    match rig.shell.ui.panes.layout() {
        pane_grid::Node::Split { id, .. } => *id,
        pane_grid::Node::Pane(_) => panic!("the layout has splits"),
    }
}

/// Clicks the label of the tab of `kind` (not the center of its cell: the
/// cell is wider than its content and the center may fall on the close
/// button) and applies what it sent.
pub fn click_tab(rig: &mut Rig, kind: PaneKind) -> Vec<Message> {
    let b = bounds(rig, tab_id(kind));
    let at = Point::new(b.x + 12.0, b.y + 10.0);
    let messages = {
        let mut sim = sim(rig);
        sim.point_at(at);
        sim.simulate(iced_test::simulator::click());
        sim.into_messages().collect::<Vec<_>>()
    };
    rig.feed(messages.clone());
    messages
}
