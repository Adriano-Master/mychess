//! Shared helpers of the `m4_6_gui_*` and `m4_8_gui_*` contract tests (specs
//! `docs/specs/m4-6-anotacao-automatica.md`, D910-D913, and
//! `docs/specs/m4-8-ameaca-shootout-automove.md`, D914-D918), headless.
//!
//! Built on the helpers of the analysis panel tests: engines are never
//! spawned, their answers are synthetic events.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m4_4_gui_common/mod.rs"]
pub mod base;

use std::collections::{BTreeMap, BTreeSet};

pub use base::*;
pub use base::common::fingerprint;
pub use mychess_app::{
    AnnotateMessage, AnnotateNotice, AnnotateOptions, AppNotice, AutomoveState, Control,
    EngineMode, Informant, ModeKind, ModeMessage, ModeNotice, StopReason,
};
pub use mychess_app::engines::{EngineId, EngineMessage, EngineRunState, Protocol, RunId};
use mychess_app::AppState;
pub use iced::{Settings, Size};
pub use iced_test::Simulator;
pub use mychess_core::Game;
pub use mychess_gui::i18n::{I18n, Language};

/// A game 1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 as the main line, cursor at the start.
pub fn six_ply_game() -> Game {
    let mut game = Game::new();
    let root = game.root();
    common::line(&mut game, root, &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"]);
    game
}

/// A rig with the six-ply game and one UCI engine running (MultiPV 1..=5).
pub fn rig_with_game_and_engine() -> (Rig, RunId) {
    let mut rig = Rig::with_game(six_ply_game());
    let run = uci_run(&mut rig);
    rig.goto(rig.game().root());
    (rig, run)
}

pub fn annotate(rig: &mut Rig, message: AnnotateMessage) {
    rig.app_msg(AppMessage::Annotate(message));
}

pub fn mode(rig: &mut Rig, message: ModeMessage) {
    rig.app_msg(AppMessage::Mode(message));
}

pub fn app(rig: &Rig) -> &AppState {
    &rig.shell.app
}

pub fn dialog_options(rig: &Rig) -> AnnotateOptions {
    rig.shell
        .app
        .annotate_dialog
        .as_ref()
        .expect("the dialog is open")
        .options
}

/// The `AnnotateMessage`s of a list of GUI messages.
pub fn annotates(messages: &[Message]) -> Vec<AnnotateMessage> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::Annotate(a)) => Some(a.clone()),
            _ => None,
        })
        .collect()
}

/// The `ModeMessage`s of a list of GUI messages.
pub fn modes(messages: &[Message]) -> Vec<ModeMessage> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::App(AppMessage::Mode(a)) => Some(*a),
            _ => None,
        })
        .collect()
}

/// `id -> placeholders` of every message of one file.
pub fn ftl_messages(language: &str, file: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = ftl_text(language, file);
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ')
            && let Some((id, _)) = line.split_once(" = ")
        {
            current = Some(id.to_owned());
            out.entry(id.to_owned()).or_default();
        }
        if let Some(id) = &current {
            let set = out.get_mut(id).unwrap();
            let mut rest = line;
            while let Some(at) = rest.find('$') {
                rest = &rest[at + 1..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                set.insert(name);
            }
        }
    }
    out
}

/// The 1st running engine is an XBoard one.
pub fn xboard_run(rig: &mut Rig) -> RunId {
    let id = add_engine(rig, "Xb", Protocol::XBoard);
    start_with(rig, id, identity("Xb", None, false))
}

/// The dialog's view alone (no menu bar), in a simulator.
pub fn dialog_sim<'a>(
    rig: &'a Rig,
    language: mychess_gui::i18n::Language,
    informant_open: bool,
    size: iced::Size,
) -> iced_test::Simulator<'a, Message> {
    let i18n = I18n::new(language);
    let element = mychess_gui::windows::annotate_dialog::view(&rig.shell.app, informant_open, &i18n)
        .expect("the dialog is open");
    // The element borrows only the application state; the translations are
    // read while building it.
    Simulator::with_size(Settings::default(), size, element)
}

// ---- the dialog, scrolled ----

use iced::Rectangle;
use iced_test::selector::Candidate;

pub const DIALOG_SIZE: Size = Size::new(900.0, 900.0);

/// Bounds and visible bounds of the `n`-th text with this content.
pub fn locate(
    s: &mut Simulator<'_, Message>,
    label: &str,
    n: usize,
) -> Option<(Rectangle, Option<Rectangle>)> {
    let mut seen = 0usize;
    s.find(move |c: Candidate<'_>| match c {
        Candidate::Text {
            content,
            bounds,
            visible_bounds,
            ..
        } if content == label => {
            seen += 1;
            (seen == n + 1).then_some((bounds, visible_bounds))
        }
        _ => None,
    })
    .ok()
}

/// Number of texts with this content in the simulator.
pub fn count_in(s: &mut Simulator<'_, Message>, label: &str) -> usize {
    let mut n = 0;
    while locate(s, label, n).is_some() {
        n += 1;
    }
    n
}

/// Scrolls the body of the dialog (a scrollable) until the `n`-th text with
/// this content is visible, then clicks it. `false` when there is no such
/// text.
pub fn click_scrolled(s: &mut Simulator<'_, Message>, label: &str, n: usize) -> bool {
    for _ in 0..40 {
        let Some((_, visible)) = locate(s, label, n) else {
            return false;
        };
        if let Some(visible) = visible {
            s.point_at(visible.center());
            drop(s.simulate(iced_test::simulator::click()));
            return true;
        }
        s.point_at(iced::Point::new(450.0, 450.0));
        drop(s.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
        })]));
    }
    false
}

/// What a click on the `n`-th text `label` of the English dialog sends.
pub fn dclick(rig: &Rig, label: &str, n: usize) -> Vec<Message> {
    dclick_in(rig, Language::EnUs, false, label, n)
}

pub fn dclick_in(
    rig: &Rig,
    language: Language,
    informant_open: bool,
    label: &str,
    n: usize,
) -> Vec<Message> {
    let mut s = dialog_sim(rig, language, informant_open, DIALOG_SIZE);
    assert!(
        click_scrolled(&mut s, label, n),
        "no text #{n} {label:?} in the dialog"
    );
    s.into_messages().collect()
}

/// Whether the dialog (English, informant block closed) has such a text.
pub fn dhas(rig: &Rig, label: &str) -> bool {
    let mut s = dialog_sim(rig, Language::EnUs, false, DIALOG_SIZE);
    locate(&mut s, label, 0).is_some()
}

pub fn dcount(rig: &Rig, label: &str) -> usize {
    let mut s = dialog_sim(rig, Language::EnUs, false, DIALOG_SIZE);
    count_in(&mut s, label)
}
