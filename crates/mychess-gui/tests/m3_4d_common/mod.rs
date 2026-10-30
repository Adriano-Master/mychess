//! Shared helpers of the `m3_4d_*` GUI contract tests (spec
//! `docs/specs/m3-4-slots-clipbase-lista-de-partidas.md`, slice 4d): mouse
//! gestures on the strip of databases, headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m3_4c_common/mod.rs"]
pub mod c;

use iced::{Point, Rectangle, Settings, Size, widget};
use iced_test::Simulator;
use iced_test::core::{Event, mouse};
pub use mychess_app::{
    BasesMessage, CopyScope, ListMessage, Message as AppMessage, PickMode, SlotId,
};
use mychess_app::{Key, KeyInput, Modifiers, NamedKey};
use mychess_gui::Message;
use mychess_gui::windows::game_list::GameListMessage;
use mychess_gui::windows::notation::NotationMessage;

pub use c::{Rig, clear_notice, notice, open_more, panel};

pub const SIZE: Size = Size::new(1280.0, 800.0);
pub const CLIP: SlotId = SlotId::CLIPBASE;

/// Slot 1 holds `a.pgn` (12 games `White n`), slot 2 `b.pgn` (8 games
/// `Other n`), slot 3 `c.pgn` (5 games `Third n`); slot 1 is current.
pub fn three(dir: &std::path::Path) -> Rig {
    reset_pointer();
    let mut rig = panel(dir, 12);
    open_more(&mut rig, dir, "b.pgn", 8);
    let path = c::b::numbered(dir, "c.pgn", "Third", 5);
    c::b::open(&mut rig, &path);
    rig.send(BasesMessage::Select(c::b::slot(1)));
    rig
}

pub fn slot(n: usize) -> SlotId {
    c::b::slot(n)
}

/// Where to press on the tab of `slot`: inside its label, away from the
/// close button.
pub fn tab_point(rig: &Rig, slot: SlotId) -> Point {
    let b = bounds(rig, &format!("games-tab-{}", slot.number()));
    Point::new(b.x + 14.0, b.y + 10.0)
}

pub fn bounds(rig: &Rig, id: &str) -> Rectangle {
    rig.sim(SIZE)
        .find(widget::Id::from(id.to_owned()))
        .unwrap_or_else(|_| panic!("widget {id}"))
        .bounds()
}

pub fn row_point(rig: &Rig, number: u32) -> Point {
    let b = bounds(rig, &format!("games-row-{number}"));
    Point::new(b.x + 40.0, b.y + b.height / 2.0)
}

/// A point over the board, outside the strip and the list.
pub fn elsewhere() -> Point {
    Point::new(200.0, 120.0)
}

thread_local! {
    /// Where the pointer was after the last gesture. Every gesture builds
    /// a new simulator, which knows nothing of the widgets the pointer was
    /// over; replaying the last position first gives `on_exit` and
    /// `on_enter` what a real pointer would.
    static LAST: std::cell::Cell<Option<Point>> = const { std::cell::Cell::new(None) };
}

/// Forgets the pointer position (a new test starts with the pointer
/// nowhere).
pub fn reset_pointer() {
    LAST.with(|last| last.set(None));
}

/// Moves the pointer to `at` (from where it was) and then runs `events`;
/// returns the messages the widgets published, not applied.
pub fn gesture(rig: &Rig, at: Point, events: Vec<Event>) -> Vec<Message> {
    let mut sim = rig.sim(SIZE);
    let mut all = Vec::new();
    if let Some(previous) = LAST.with(std::cell::Cell::get)
        && previous != at
    {
        sim.point_at(previous);
        all.push(Event::Mouse(mouse::Event::CursorMoved {
            position: previous,
        }));
        drop(sim.simulate(std::mem::take(&mut all)));
    }
    sim.point_at(at);
    all.push(Event::Mouse(mouse::Event::CursorMoved { position: at }));
    all.extend(events);
    drop(sim.simulate(all));
    LAST.with(|last| last.set(Some(at)));
    sim.into_messages().collect()
}

pub fn moved(rig: &Rig, at: Point) -> Vec<Message> {
    gesture(rig, at, Vec::new())
}

pub fn pressed(rig: &Rig, at: Point, button: mouse::Button) -> Vec<Message> {
    gesture(
        rig,
        at,
        vec![Event::Mouse(mouse::Event::ButtonPressed(button))],
    )
}

pub fn released(rig: &Rig, at: Point, button: mouse::Button) -> Vec<Message> {
    gesture(
        rig,
        at,
        vec![Event::Mouse(mouse::Event::ButtonReleased(button))],
    )
}

/// Feeds the messages to the shell the way the runtime would, and returns
/// the application messages they turned into (the window messages of the
/// list pass through `GameListUi::update`, whose output is run).
pub fn apply(rig: &mut Rig, messages: Vec<Message>) -> Vec<AppMessage> {
    let mut sent = Vec::new();
    for message in messages {
        match message {
            Message::Games(g) => {
                let out = rig.shell.ui.games.update(g, &rig.shell.app);
                for m in out {
                    sent.push(m.clone());
                    rig.send(m);
                }
            }
            Message::App(m) => {
                sent.push(m.clone());
                rig.send(m);
            }
            other => rig.msg(other),
        }
    }
    sent
}

/// Where a gesture happens, resolved against the current layout.
#[derive(Debug, Clone, Copy)]
pub enum Spot {
    Tab(SlotId),
    Row(u32),
    Away,
}

impl Spot {
    pub fn point(self, rig: &Rig) -> Point {
        match self {
            Spot::Tab(slot) => tab_point(rig, slot),
            Spot::Row(n) => row_point(rig, n),
            Spot::Away => elsewhere(),
        }
    }
}

pub fn press(rig: &mut Rig, at: Spot) -> Vec<AppMessage> {
    let at = at.point(rig);
    let out = pressed(rig, at, mouse::Button::Left);
    apply(rig, out)
}

pub fn hover(rig: &mut Rig, at: Spot) -> Vec<AppMessage> {
    let at = at.point(rig);
    let out = moved(rig, at);
    apply(rig, out)
}

/// A release: the widget under the pointer first, then the global release
/// (the order of the runtime, D584).
pub fn release(rig: &mut Rig, at: Spot) -> Vec<AppMessage> {
    let at = at.point(rig);
    let out = released(rig, at, mouse::Button::Left);
    let sent = apply(rig, out);
    rig.msg(Message::Notation(NotationMessage::PointerReleased));
    sent
}

pub fn escape(rig: &mut Rig) {
    rig.key(
        KeyInput {
            key: Key::Named(NamedKey::Escape),
            modifiers: Modifiers::NONE,
        },
        false,
        None,
    );
}

pub fn has_copy(sent: &[AppMessage], from: SlotId, to: SlotId, scope: CopyScope) -> bool {
    sent.contains(&AppMessage::Bases(BasesMessage::List(
        ListMessage::CopyGames { from, to, scope },
    )))
}

pub fn any_copy(sent: &[AppMessage]) -> bool {
    sent.iter().any(|m| {
        matches!(
            m,
            AppMessage::Bases(BasesMessage::List(ListMessage::CopyGames { .. }))
        )
    })
}

pub fn selected_slot(sent: &[AppMessage], slot: SlotId) -> bool {
    sent.contains(&AppMessage::Bases(BasesMessage::Select(slot)))
}
