//! Shared helpers of the `m3_4e_*` GUI contract tests (spec
//! `docs/specs/m3-4-slots-clipbase-lista-de-partidas.md`, slice 4e: the game
//! browser).
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
#[path = "../render_common/mod.rs"]
pub mod render;

use std::path::{Path, PathBuf};

pub use c::{Rig, command, key_units, named, plain, slot, with};
use iced::Size;
pub use mychess_app::{
    BasesMessage, BrowseMessage, BrowseMode, Key, KeyInput, ListMessage, Modifiers, NamedKey,
    SlotId,
};
use mychess_gui::windows::game_list::GameListMessage;
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::{Message, UiMessage};

/// `count` games `W<n>` - `B<n>` of four plies.
pub fn games_file(dir: &Path, name: &str, count: u32) -> PathBuf {
    let mut text = String::new();
    for n in 1..=count {
        text.push_str(&format!(
            "[Event \"Ev{n}\"]\n[White \"W{n}\"]\n[Black \"B{n}\"]\n[Result \"1-0\"]\n\n\
             1. e4 e5 2. Nf3 Nc6 1-0\n\n"
        ));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write the pgn");
    path
}

/// A shell with `count` games open in the list panel, the pointer over the
/// list.
pub fn panel(dir: &Path, count: u32) -> Rig {
    let mut rig = Rig::new();
    let path = games_file(dir, "a.pgn", count);
    c::b::open(&mut rig, &path);
    c::b::viewport(&mut rig, 1280.0, 400.0);
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    rig
}

pub fn current_slot(rig: &Rig) -> SlotId {
    rig.shell.app.bases.current()
}

/// Opens the browser through the application and lets the read finish.
pub fn open_browser(rig: &mut Rig, numbers: &[u32]) {
    let slot = current_slot(rig);
    rig.send(BasesMessage::Browse(BrowseMessage::Open {
        slot,
        numbers: numbers.to_vec(),
    }));
    assert!(rig.shell.app.browser.is_some());
}

pub fn browser(rig: &Rig) -> &mychess_app::BrowserState {
    rig.shell.app.browser.as_ref().expect("browser open")
}

pub fn plies(rig: &Rig) -> Vec<usize> {
    browser(rig)
        .games
        .iter()
        .map(|g| g.ready().map_or(usize::MAX, mychess_app::ReadyGame::ply))
        .collect()
}

pub fn app_send(rig: &mut Rig, message: BrowseMessage) {
    rig.send(BasesMessage::Browse(message));
}

pub fn resize(rig: &mut Rig, width: f32, height: f32) {
    rig.msg(Message::Ui(UiMessage::WindowResized(Size::new(
        width, height,
    ))));
}

pub fn select(rig: &mut Rig, numbers: &[u32]) {
    let slot = current_slot(rig);
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: numbers.to_vec(),
    }));
}

pub fn chord(key: Key, command: bool) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers {
            command,
            shift: false,
            alt: false,
        },
    }
}
