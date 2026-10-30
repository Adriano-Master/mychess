//! Review fixes of the merge dialog (spec `m7-6-mesclar-partida.md`, slice
//! c, D3881-D3885): the cached notation of the preview, the stale preview
//! and the refused "OK".
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]
mod m7_6b_common;

use m7_6b_common::*;
use mychess_app::online::MarkerInfo;

#[test]
fn the_view_reads_the_notation_built_once_per_outcome() {
    let (mut rig, slot, _dir) = sample(A);
    let before = rig.state.merge_doc_builds();
    rig.open(slot, B);
    assert_eq!(rig.state.merge_doc_builds(), before + 1);
    let first = std::ptr::from_ref(rig.state.merge_view().expect("dialog").preview_doc);
    for _ in 0..50 {
        let view = rig.state.merge_view().expect("dialog");
        assert!(std::ptr::eq(view.preview_doc, first));
    }
    assert_eq!(
        rig.state.merge_doc_builds(),
        before + 1,
        "the view does not build"
    );
    // The document is the one of the preview, with the new moves.
    let view = rig.state.merge_view().unwrap();
    assert!(view.first_position().is_some());
    assert!(view.preview_doc.flat_text().contains("Bc4"));
    // A new ply builds exactly one more document.
    rig.raw(MergeMessage::SetEndPly(Some(6)));
    assert_eq!(rig.state.merge_doc_builds(), before + 2);
    let view = rig.state.merge_view().unwrap();
    assert!(!view.preview_doc.flat_text().contains("c3"), "cut at ply 6");
}

#[test]
fn the_preview_follows_a_game_that_changed_under_the_dialog() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    let builds = rig.state.merge_doc_builds();
    // A remote move or an observed game replaces the game, without a modal
    // message: game E (1.e4 e5) still meets B.
    let mut game = mychess_core::Game::new();
    let mut at = game.root();
    for san in ["e4", "e5"] {
        let mv = game
            .position_at(at)
            .unwrap()
            .parse_san(san, mychess_core::SanMode::Strict)
            .unwrap()
            .mv;
        at = game.add_move(at, mv).unwrap();
    }
    rig.state.session.replace_game(game.clone());
    rig.raw(MergeMessage::SetEndPly(Some(80)));
    let view = rig.state.merge_view().expect("still open");
    assert!(view.original.same_content(&game), "the preview is of E");
    assert_eq!(view.preview.mainline().count(), 8);
    assert!(rig.state.merge_doc_builds() > builds);
}

#[test]
fn the_dialog_closes_with_a_notice_when_the_changed_game_no_longer_merges() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    // The game that starts from another position never meets B.
    let (other, _s, _d) = sample(OTHER_START);
    rig.state
        .session
        .replace_game(other.state.session.game().clone());
    rig.raw(MergeMessage::SetEndPly(Some(80)));
    assert!(rig.state.merge_view().is_none());
    assert_eq!(rig.notice(), Some(MergeNotice::NoMatch));
}

#[test]
fn a_refused_ok_keeps_the_dialog_and_can_be_retried() {
    let (mut rig, slot, _dir) = sample(A);
    rig.open(slot, B);
    let nodes = rig.nodes();
    rig.state.online.marker = Some(MarkerInfo {
        game_id: "g".to_owned(),
        date: "2026-01-01".to_owned(),
    });
    rig.apply();
    assert!(rig.state.merge_view().is_some(), "the dialog stays open");
    assert_eq!(rig.nodes(), nodes);
    rig.state.online.marker = None;
    rig.apply();
    assert!(rig.state.merge_view().is_none());
    assert_eq!(rig.nodes(), nodes + 4);
}
