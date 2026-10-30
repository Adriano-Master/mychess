//! Shared helpers of the `m3_4c_*` GUI contract tests (spec
//! `docs/specs/m3-4-slots-clipbase-lista-de-partidas.md`, slice 4c).
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m3_4b_common/mod.rs"]
pub mod b;

use std::path::Path;
use std::sync::{Arc, Mutex};

use iced::{Rectangle, Size, widget};
use iced_test::selector::{Candidate, Target};
use mychess_app::{Key, KeyInput, Modifiers, NamedKey, SlotId};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::game_list::GameListMessage;
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::{Message, windows::game_list::ListMenu};

pub use b::{Rig, command, list, named, numbered, open, plain, slot, viewport, write_games};

/// A shell with a PGN of `count` games (`White n`) open in a slot, the list
/// panel shown, a viewport of 1280x400 and the pointer over the list.
pub fn panel(dir: &Path, count: u32) -> Rig {
    let mut rig = Rig::new();
    let path = numbered(dir, "a.pgn", "White", count);
    open(&mut rig, &path);
    viewport(&mut rig, 1280.0, 400.0);
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    assert!(
        rig.shell.ui.is_open(PaneKind::GameList),
        "a multi-game PGN shows the panel"
    );
    rig
}

/// Opens another PGN of `count` games in the next free slot.
pub fn open_more(rig: &mut Rig, dir: &Path, name: &str, count: u32) {
    let path = numbered(dir, name, "Other", count);
    open(rig, &path);
}

pub fn shift_tab() -> KeyInput {
    KeyInput {
        key: Key::Named(NamedKey::Tab),
        modifiers: Modifiers {
            command: false,
            shift: true,
            alt: false,
        },
    }
}

pub fn with(key: Key, command: bool, shift: bool) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers {
            command,
            shift,
            alt: false,
        },
    }
}

/// Sends a key to the shell and returns the units of work of the task it
/// produced (0 when the key did nothing that needs the runtime).
pub fn key_units(rig: &mut Rig, input: KeyInput, text_focused: bool) -> usize {
    rig.shell
        .update(Message::Dialog(DialogMessage::Key {
            input,
            text_focused,
            typed: None,
        }))
        .units()
}

pub fn current(rig: &Rig) -> &mychess_app::BaseSlot {
    rig.shell.app.bases.current_slot().expect("slot")
}

/// The boxes of the icon buttons of the toolbar, left to right (24x24 each:
/// 16 px of icon and 4 px of padding), without duplicates.
pub fn tool_buttons(rig: &Rig) -> Vec<Rectangle> {
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    let toolbar = sim
        .find(widget::Id::new("glist-toolbar"))
        .expect("the toolbar")
        .bounds();
    let found: Arc<Mutex<Vec<Rectangle>>> = Arc::default();
    let sink = Arc::clone(&found);
    let selector = move |candidate: Candidate<'_>| -> Option<Target> {
        if let Candidate::Container { bounds, .. } = candidate
            && (bounds.width - 24.0).abs() < 1.5
            && (bounds.height - 24.0).abs() < 1.5
            && toolbar.contains(bounds.center())
        {
            sink.lock().unwrap().push(bounds);
        }
        None
    };
    drop(sim.find(selector));
    let mut boxes = found.lock().unwrap().clone();
    boxes.sort_by(|a, b| a.x.total_cmp(&b.x));
    boxes.dedup_by(|a, b| (a.x - b.x).abs() < 1.0);
    boxes
}

/// Clicks the centre of `bounds` and returns the messages that came out.
pub fn click_box(rig: &Rig, bounds: Rectangle) -> Vec<Message> {
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.point_at(bounds.center());
    drop(sim.simulate(iced_test::simulator::click()));
    // Pointing at a button also makes the panel see the pointer enter.
    sim.into_messages()
        .filter(|m| !format!("{m:?}").contains("Hover("))
        .collect()
}

/// The `Debug` text of messages, to match on without `PartialEq`.
pub fn debug(messages: &[Message]) -> Vec<String> {
    messages.iter().map(|m| format!("{m:?}")).collect()
}

pub fn has_text(rig: &Rig, text: &str) -> bool {
    rig.sim(Size::new(1280.0, 800.0)).find(text).is_ok()
}

/// Shows a context menu without the selection side effect of the right
/// click (see `GameListMessage::Menu`).
pub fn open_menu(rig: &mut Rig, menu: ListMenu) {
    rig.shell.ui.games.menu = Some(menu);
}

/// The remark the user would see: the shell moves the application's notice
/// to the status bar after each message.
pub fn notice(rig: &Rig) -> Option<mychess_app::AppNotice> {
    rig.shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
}

pub fn clear_notice(rig: &mut Rig) {
    rig.shell.ui.status_notice = None;
    rig.shell.app.notice = None;
}
