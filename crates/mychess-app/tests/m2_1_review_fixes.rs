//! Regression tests for the findings of the M2-1 review.
#![allow(clippy::unwrap_used, unreachable_pub)]

mod m2_1_common;

use m2_1_common::*;
use mychess_app::{GameMessage, GameSession, PlayOptions};

fn drop_on(session: &mut GameSession, from: Square, to: Square) {
    session.update(GameMessage::PlaySquares {
        from,
        to,
        promotion: None,
        options: PlayOptions::default(),
    });
}

#[test]
fn dropping_a_king_on_its_own_square_is_not_a_null_move() {
    let mut session = GameSession::new(Game::new());
    drop_on(&mut session, Square::E1, Square::E1);
    assert_eq!(session.game().node_count(), 1);
    assert!(!session.is_dirty());
    assert!(!session.can_undo());
}

#[test]
fn changes_grows_on_edits_inside_trial_undo_redo_and_trial_exit() {
    let mut session = GameSession::new(Game::new());
    let c0 = session.changes();
    drop_on(&mut session, Square::E2, Square::E4);
    let c1 = session.changes();
    assert!(c1 > c0);
    session.update(GameMessage::Undo);
    let c2 = session.changes();
    assert!(c2 > c1);
    session.update(GameMessage::Redo);
    let c3 = session.changes();
    assert!(c3 > c2);
    session.update(GameMessage::EnterTrial { null_move: false });
    drop_on(&mut session, Square::E7, Square::E5);
    let c4 = session.changes();
    assert!(c4 > c3);
    session.update(GameMessage::ExitTrial);
    assert!(session.changes() > c4);
}

#[test]
fn enter_move_text_ignores_surrounding_whitespace_for_uci() {
    let mut session = GameSession::new(Game::new());
    session.update(GameMessage::EnterMoveText(" e2e4 ".to_owned()));
    assert_eq!(session.game().node_count(), 2);
}

/// Cost of one edit on a 1000-ply game (recorded in `docs/benchmarks.md`);
/// ignored by default because it is a timing measurement.
#[test]
#[ignore = "timing measurement; run with --release -- --ignored --nocapture"]
fn edit_cost_on_a_thousand_plies() {
    let mut game = Game::new();
    let mut cur = game.root();
    let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
    for i in 0..1000 {
        cur = add(&mut game, cur, cycle[i % 4]);
    }
    let mut session = GameSession::new(game);
    let start = std::time::Instant::now();
    let n = 200;
    for i in 0..n {
        session.update(GameMessage::SetComment {
            node: cur,
            text: format!("comment {i}"),
        });
    }
    let per_edit = start.elapsed() / n;
    eprintln!("1000-ply edit: {per_edit:?} per edit");
    assert!(per_edit < std::time::Duration::from_millis(5));
}
