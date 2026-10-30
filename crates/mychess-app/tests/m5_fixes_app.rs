//! Regression tests of the review of M5-2b/c and M5-7b (specs
//! `m5-2-janela-da-arvore.md` D1740-D1747 and `m5-7-eco-e-novidade.md`
//! D1740): the online lock closes the ECO windows and refuses "open base as
//! tree", the training ends with "Refresh" off, and the best games are not
//! computed for a pass whose answer is thrown away.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::online::OnlineMessage;
use mychess_app::{BestMessage, EcoMessage, EcoWindow, PickSpec};
use tempfile::tempdir;

/// A rig whose state knows where its preferences (and the online marker)
/// live, with the standard base open.
fn rig_in(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(dir.join("preferences.toml")));
    rig.state.open_pgn_in_slots = true;
    rig.open_pgn(dir, "std.pgn", STD);
    rig
}

/// Locks the assistance the way a program restarted in the middle of an
/// online game is locked: by the marker file.
fn lock(rig: &mut Rig, dir: &std::path::Path) {
    std::fs::write(
        mychess_app::online::marker_path(dir),
        "game = \"abcd1234\"\ndate = \"2026.10.24\"\n",
    )
    .unwrap();
    rig.send(Message::Online(OnlineMessage::LoadMarker));
    assert!(rig.state.assist_locked(), "the marker locks");
}

#[test]
fn the_online_lock_closes_the_eco_windows() {
    let dir = tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    rig.send(Message::Eco(EcoMessage::OpenBrowser {
        glossary: Vec::new(),
    }));
    assert_eq!(rig.state.eco_window(), Some(EcoWindow::Browser));
    lock(&mut rig, dir.path());
    assert_eq!(rig.state.eco_window(), None);
}

#[test]
fn the_online_lock_closes_the_novelty_dialog() {
    let dir = tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    rig.send(Message::Eco(EcoMessage::OpenNovelty));
    assert_eq!(rig.state.eco_window(), Some(EcoWindow::Novelty));
    lock(&mut rig, dir.path());
    assert_eq!(rig.state.eco_window(), None);
}

#[test]
fn a_file_picked_for_open_base_as_tree_is_ignored_while_locked() {
    let dir = tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    let host = rig.send(TreeMessage::OpenBaseAsTree);
    let request = host
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(PickSpec { request, .. }) => Some(*request),
            _ => None,
        })
        .expect("a file dialog was asked for");
    lock(&mut rig, dir.path());
    let other = dir.path().join("other.pgn");
    std::fs::write(&other, STD).unwrap();
    let before = rig.state.bases.current();
    rig.send(Message::FilePicked(request, Some(other)));
    assert_eq!(rig.state.bases.current(), before);
    assert!(
        rig.state.base_opening().is_none(),
        "nothing is being opened"
    );
    assert!(!rig.state.tree().locked());
    assert!(!rig.state.tree().is_open());
    assert!(!rig.state.take_tree_show_request());
}

#[test]
fn turning_refresh_off_ends_the_training() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::SetTraining(true));
    assert!(rig.state.tree().training());
    rig.send(TreeMessage::ToggleAutoRefresh);
    assert!(!rig.state.tree().autorefresh());
    assert!(!rig.state.tree().training(), "no refresh, no answers");
}

#[test]
fn the_fast_pass_of_fast_then_slow_does_not_compute_the_best_games() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(TreeMessage::Best(BestMessage::Toggle));
    rig.send(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow));
    // The first job is the fast pass.
    let first = jobs(rig.raw(TreeMessage::Open));
    assert_eq!(first.len(), 1);
    let answer = rig.run_job(first.into_iter().next().unwrap());
    let next = jobs(rig.raw(answer));
    assert!(
        rig.view().best.is_none_or(|b| b.rows.is_empty()),
        "the fast pass leaves the list alone"
    );
    assert_eq!(next.len(), 1, "the slow pass follows");
    let answer = rig.run_job(next.into_iter().next().unwrap());
    rig.raw(answer);
    assert!(
        rig.view().best.is_some_and(|b| !b.rows.is_empty()),
        "the slow pass fills it"
    );
}
