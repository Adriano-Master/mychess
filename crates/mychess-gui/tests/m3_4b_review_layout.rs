//! Review fixes of slice 4b: opening a database keeps the arrangement the
//! user made (D551, D552), and only a new slot opens the panel.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::redundant_closure_for_method_calls
)]

#[path = "m2_4_common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};

use common::Rig;
use iced::widget::pane_grid;
use mychess_app::{BasesMessage, Effect, LayoutNode, Message as AppMessage, perform};
use mychess_gui::windows::game_list::GameListMessage;
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::{Message, UiMessage};

fn pgn(dir: &Path, name: &str, count: u32) -> PathBuf {
    let mut text = String::new();
    for n in 1..=count {
        text.push_str(&format!(
            "[Event \"E{n}\"]\n[White \"W{n}\"]\n[Black \"B{n}\"]\n[Result \"*\"]\n\n1. e4 *\n\n"
        ));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

fn open(rig: &mut Rig, path: &Path) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Open(path.to_path_buf())),
    );
    for effect in effects {
        assert!(matches!(effect, Effect::OpenBase { .. }));
        let answer = rt.block_on(perform(effect));
        rig.app_msg(answer);
    }
}

fn root_ratio(node: &LayoutNode) -> f32 {
    match node {
        LayoutNode::Split { ratio, .. } => *ratio,
        LayoutNode::Group { .. } => panic!("a split"),
    }
}

fn resize_root(rig: &mut Rig, ratio: f32) {
    let split = match rig.shell.ui.panes.layout() {
        pane_grid::Node::Split { id, .. } => *id,
        pane_grid::Node::Pane(_) => panic!("splits"),
    };
    rig.msg(Message::Ui(UiMessage::PaneResized(
        pane_grid::ResizeEvent { split, ratio },
    )));
}

#[test]
fn opening_a_database_keeps_the_sizes_the_user_chose() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    resize_root(&mut rig, 0.35);
    open(&mut rig, &pgn(dir.path(), "a.pgn", 5));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    let layout = rig.shell.ui.layout();
    let LayoutNode::Split { a, .. } = &layout else {
        panic!("a split");
    };
    assert!(
        (root_ratio(a) - 0.35).abs() < 1e-3,
        "the old tree is the top part"
    );
}

#[test]
fn closing_and_reopening_by_hand_keeps_the_rest_and_is_saved() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &pgn(dir.path(), "a.pgn", 5));
    assert!(rig.shell.app.prefs.layout.is_none(), "automatic: not saved");
    resize_root(&mut rig, 0.3);
    // The resize saves what is on screen minus the automatic list.
    let saved = rig.shell.app.prefs.layout.clone().expect("saved");
    assert!(!saved.panes().contains(&PaneKind::GameList));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    let saved = rig.shell.app.prefs.layout.clone().expect("saved");
    assert!(
        saved.panes().contains(&PaneKind::GameList),
        "a gesture of the user"
    );
    resize_root(&mut rig, 0.3);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    let without = rig.shell.ui.layout();
    assert!(
        (root_ratio(&without) - 0.6).abs() < 1e-3,
        "the board/notation split is back as it was"
    );
}

#[test]
fn an_already_open_file_does_not_reopen_a_panel_the_user_closed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let path = pgn(dir.path(), "a.pgn", 5);
    open(&mut rig, &path);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    open(&mut rig, &path);
    assert!(
        !rig.shell.ui.is_open(PaneKind::GameList),
        "AlreadyOpen only activates the slot"
    );
}

#[test]
fn a_full_set_of_slots_does_not_reopen_the_panel_either() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    for n in 0..8 {
        open(&mut rig, &pgn(dir.path(), &format!("f{n}.pgn"), 3));
    }
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    open(&mut rig, &pgn(dir.path(), "ninth.pgn", 3));
    assert!(
        !rig.shell.ui.is_open(PaneKind::GameList),
        "NoFreeSlot opens nothing"
    );
}

#[test]
fn closing_the_panel_forgets_that_the_pointer_was_over_it() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &pgn(dir.path(), "a.pgn", 5));
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert!(!rig.shell.ui.games.hovered);
}
