//! Shared helpers of the `m4_7_gui_*` contract tests (spec
//! `docs/specs/m4-7-graficos-avaliacao-tempo.md`, D870-D876: the graph
//! panel), headless. The language is fixed by the `Rig` (preferences), never
//! by the machine.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::assigning_clones
)]

#[path = "../m2_4_common/mod.rs"]
pub mod common;

pub use common::{Rig, ch, command, command_shift, fingerprint, named, plain};
use iced::{Point, Settings, Size};
use iced_test::Simulator;
pub use mychess_app::graphs::{GraphKind, GraphMessage};
pub use mychess_app::{
    GameMessage, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, Nav,
};
pub use mychess_core::{Game, Position};
use mychess_gui::i18n::{I18n, Language};
pub use mychess_gui::windows::layout::PaneKind;
pub use mychess_gui::{Message, UiMessage};

pub const SIZE: Size = Size::new(1400.0, 900.0);

pub fn en() -> I18n {
    I18n::new(Language::EnUs)
}

pub fn pt() -> I18n {
    I18n::new(Language::PtBr)
}

pub fn ftl_text(language: &str, file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A game of `n` plies (knights shuffling) where ply `i` has the comment
/// `comment(i)`; White and Black are "Anna" and "Boris".
pub fn game_with(n: usize, comment: impl Fn(usize) -> String) -> Game {
    let mut game = Game::new();
    game.tags_mut().white = "Anna".to_owned();
    game.tags_mut().black = "Boris".to_owned();
    let mut pos = Position::new();
    let mut node = game.root();
    for i in 0..n {
        let uci = ["g1f3", "g8f6", "f3g1", "f6g8"][i % 4];
        let mv = pos.parse_uci(uci).expect("legal");
        pos.play(mv).expect("play");
        node = game.add_move(node, mv).expect("add");
        let text = comment(i);
        if !text.is_empty() {
            game.set_comment(node, &text).expect("comment");
        }
    }
    game
}

/// Six plies with `[%eval]` values, the last one a mate.
pub fn evals_game() -> Game {
    let values = ["0.17", "0.25", "0.3", "-0.4", "0.5", "#-3"];
    game_with(6, |i| format!("[%eval {}]", values[i]))
}

/// Six plies with `[%emt]` times only.
pub fn times_game() -> Game {
    let secs = [5, 7, 9, 2, 3, 4];
    game_with(6, |i| format!("[%emt 0:00:{:02}]", secs[i]))
}

/// A rig with `game` loaded and the graph panel open (the app builds the
/// view on the `update` that follows the opening).
pub fn rig_with_graph(game: Game) -> Rig {
    let mut rig = Rig::with_game(game);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Graph)));
    // The first update after the panel opens builds the view.
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Start)));
    rig
}

pub fn graph_msg(rig: &mut Rig, message: GraphMessage) {
    rig.app_msg(AppMessage::Graph(message));
}

/// The SHA-256 of a snapshot of `sim` drawn with `theme`.
pub fn fingerprint_in<M>(sim: &mut Simulator<'_, M>, theme: &iced::Theme) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mychess-m4-7-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let snapshot = sim.snapshot(theme).expect("snapshot");
    assert!(snapshot.matches_hash(dir.join("scene")).expect("hash"));
    let file = std::fs::read_dir(&dir)
        .expect("hash dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sha256"))
        .expect("hash file");
    let hash = std::fs::read_to_string(file).expect("read hash");
    drop(std::fs::remove_dir_all(&dir));
    hash
}

/// Hash of the whole window of `rig` at `size` (light theme).
pub fn shot(rig: &Rig, size: Size) -> String {
    let mut sim = rig.sim(size);
    fingerprint(&mut sim)
}

/// Presses the left button along the row `y` every `step` pixels of a window
/// of `size`; the graph x of every click that reached the canvas, in order.
pub fn scan_row(rig: &Rig, size: Size, y: f32, step: usize) -> Vec<f32> {
    let mut sim = Simulator::with_size(Settings::default(), size, rig.shell.view());
    let events = [
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        )),
    ];
    let mut px = 0usize;
    while (px as f32) < size.width {
        sim.point_at(Point::new(px as f32, y));
        drop(sim.simulate(events.clone()));
        px += step;
    }
    sim.into_messages()
        .filter_map(|m| match m {
            Message::App(AppMessage::Graph(GraphMessage::Click(x))) => Some(x),
            _ => None,
        })
        .collect()
}
