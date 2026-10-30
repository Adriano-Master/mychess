//! M8-3b contract tests of the premove (spec D1321, D1390, D1391, D1394):
//! the pure rules (`Premove`, `PremoveSlot`, `is_pseudo_legal`) and the
//! premove inside a game against the fake Lichess: validated against the
//! position after the opponent's move, sent at once when legal, dropped
//! silently when not, emptied by a takeback, the end of the game and Esc.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::AppNotice;
use mychess_app::online::{
    OnlineMessage, OnlineNotice, OnlinePhase, PlayingState, Premove, PremoveSlot, RejectReason,
    is_pseudo_legal,
};
use mychess_core::{Color, Position, Role};
use rstest::rstest;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("fen")
}

// ---- the pure rules ----

#[rstest]
#[case::knight_l("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1", "d4", "e6", true)]
#[case::knight_straight("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1", "d4", "d6", false)]
#[case::bishop_diagonal_through_pieces("4k3/8/8/8/3B4/2P5/8/4K3 w - - 0 1", "d4", "a1", true)]
#[case::bishop_straight("4k3/8/8/8/3B4/8/8/4K3 w - - 0 1", "d4", "d6", false)]
#[case::rook_file("4k3/8/8/8/3R4/8/8/4K3 w - - 0 1", "d4", "d8", true)]
#[case::rook_diagonal("4k3/8/8/8/3R4/8/8/4K3 w - - 0 1", "d4", "e5", false)]
#[case::queen_both("4k3/8/8/8/3Q4/8/8/4K3 w - - 0 1", "d4", "g7", true)]
#[case::queen_knight_jump("4k3/8/8/8/3Q4/8/8/4K3 w - - 0 1", "d4", "e6", false)]
#[case::king_one("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "e1", "e2", true)]
#[case::king_two("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "e1", "e3", false)]
#[case::pawn_one("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2", "e3", true)]
#[case::pawn_two_from_home("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1", "e2", "e4", true)]
#[case::pawn_two_not_from_home("4k3/8/8/8/8/4P3/8/4K3 w - - 0 1", "e3", "e5", false)]
#[case::pawn_backwards("4k3/8/8/8/8/4P3/8/4K3 w - - 0 1", "e3", "e2", false)]
#[case::pawn_capture_square_even_if_empty("4k3/8/8/8/8/4P3/8/4K3 w - - 0 1", "e3", "d4", true)]
#[case::pawn_two_sideways("4k3/8/8/8/8/4P3/8/4K3 w - - 0 1", "e3", "g4", false)]
#[case::own_piece_on_target("4k3/8/8/8/8/8/4P3/4KN2 w - - 0 1", "e1", "f1", false)]
#[case::castling_short_is_a_premove("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1", "g1", true)]
#[case::castling_long("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1", "c1", true)]
#[case::castling_three_files("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1", "b1", false)]
#[case::from_equals_to("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "e1", "e1", false)]
#[case::empty_origin("4k3/8/8/8/8/8/8/4K3 w - - 0 1", "a1", "a2", false)]
fn the_piece_must_be_able_to_make_the_move_by_its_kind(
    #[case] fen: &str,
    #[case] from: &str,
    #[case] to: &str,
    #[case] want: bool,
) {
    assert_eq!(
        is_pseudo_legal(&pos(fen), Color::White, sq(from), sq(to)),
        want,
        "{fen} {from}{to}"
    );
}

#[test]
fn a_piece_of_the_other_colour_cannot_be_premoved() {
    let p = pos("4k3/4p3/8/8/8/8/8/4K3 w - - 0 1");
    assert!(!is_pseudo_legal(&p, Color::White, sq("e7"), sq("e6")));
    assert!(is_pseudo_legal(&p, Color::Black, sq("e7"), sq("e6")));
    // Black pawns go down.
    assert!(!is_pseudo_legal(&p, Color::Black, sq("e7"), sq("e8")));
    assert!(is_pseudo_legal(&p, Color::Black, sq("e7"), sq("e5")));
}

#[rstest]
#[case::default_queen(None, Role::Queen)]
#[case::queen(Some(Role::Queen), Role::Queen)]
#[case::rook(Some(Role::Rook), Role::Rook)]
#[case::bishop(Some(Role::Bishop), Role::Bishop)]
#[case::knight(Some(Role::Knight), Role::Knight)]
#[case::king_is_not_a_promotion(Some(Role::King), Role::Queen)]
#[case::pawn_is_not_a_promotion(Some(Role::Pawn), Role::Queen)]
fn a_pawn_that_reaches_the_last_rank_promotes_to_the_choice_or_the_queen(
    #[case] chosen: Option<Role>,
    #[case] want: Role,
) {
    let p = pos("7k/4P3/8/8/8/8/8/K7 w - - 0 1");
    let premove = Premove::enter(&p, Color::White, sq("e7"), sq("e8"), chosen).expect("premove");
    assert_eq!(premove.promotion, Some(want));
    assert_eq!((premove.from, premove.to), (sq("e7"), sq("e8")));
}

#[test]
fn the_chosen_piece_is_ignored_when_nothing_promotes() {
    let p = pos("7k/8/8/8/8/8/4P3/K7 w - - 0 1");
    let premove =
        Premove::enter(&p, Color::White, sq("e2"), sq("e4"), Some(Role::Rook)).expect("premove");
    assert_eq!(premove.promotion, None);
    assert!(Premove::enter(&p, Color::White, sq("e2"), sq("d5"), None).is_none());
}

#[test]
fn the_slot_keeps_one_premove_and_always_ends_empty_after_taking() {
    let start = Position::new();
    let mut slot = PremoveSlot::new();
    assert!(!slot.is_set());
    assert!(!slot.cancel(), "nothing to cancel");
    let first = Premove::enter(&start, Color::White, sq("e2"), sq("e4"), None).unwrap();
    let second = Premove::enter(&start, Color::White, sq("d2"), sq("d4"), None).unwrap();
    slot.set(first);
    slot.set(second);
    assert_eq!(
        slot.get(),
        Some(second),
        "one at a time: the second replaces"
    );
    let mv = slot.take_if_legal(&start).expect("legal at the start");
    assert_eq!(start.uci(mv), "d2d4");
    assert!(!slot.is_set(), "taken");
    // An illegal premove is dropped and the slot is empty anyway.
    slot.set(Premove::enter(&start, Color::White, sq("e2"), sq("e3"), None).unwrap());
    let blocked = pos("4k3/8/8/8/8/4n3/4P3/4K3 w - - 0 1");
    assert!(slot.take_if_legal(&blocked).is_none());
    assert!(!slot.is_set());
    assert!(!slot.cancel());
}

// ---- inside the game ----

async fn black_game(rig: &mut Rig, fen: Option<&str>) {
    let opts = fen.map_or_else(Full::default, Full::from_fen);
    rig.start_with(&full(&opts, &state(""))).await;
}

fn notice(rig: &Rig) -> Option<OnlineNotice> {
    match rig.notice() {
        Some(AppNotice::Online(n)) => Some(n.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn a_move_on_the_turn_of_the_opponent_is_kept_not_sent_and_goes_out_when_legal() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e7e5");
    black_game(&mut rig, None).await;
    rig.play("e7", "e5").await;
    assert!(rig.state.online.premove.is_set());
    assert_eq!(rig.mainline_count(), 0, "a premove is not on the board");
    rig.pump(150).await;
    assert_eq!(
        rig.move_posts("e7e5"),
        0,
        "nothing is sent before the opponent moves"
    );
    // The opponent moves: the premove is checked and sent at once.
    rig.games[0].line(&state("e2e4"));
    rig.wait_post(&format!("/api/board/game/{ID}/move/e7e5"), 1)
        .await;
    assert!(!rig.state.online.premove.is_set());
    rig.until("both plies shown", |s| {
        s.session.game().mainline().count() == 2
    })
    .await;
    // The same state again does not send it twice.
    rig.games[0].line(&state("e2e4"));
    rig.pump(150).await;
    assert_eq!(rig.move_posts("e7e5"), 1);
}

#[tokio::test]
async fn a_second_premove_replaces_the_first() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("d7d5");
    rig.move_ok("e7e5");
    black_game(&mut rig, None).await;
    rig.play("e7", "e5").await;
    rig.play("d7", "d5").await;
    let kept = rig.state.online.premove.get().expect("one premove");
    assert_eq!((kept.from, kept.to), (sq("d7"), sq("d5")));
    rig.games[0].line(&state("e2e4"));
    rig.wait_post(&format!("/api/board/game/{ID}/move/d7d5"), 1)
        .await;
    rig.pump(100).await;
    assert_eq!(rig.move_posts("e7e5"), 0);
}

#[tokio::test]
async fn escape_or_right_click_cancels_the_premove() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("e7e5");
    black_game(&mut rig, None).await;
    rig.play("e7", "e5").await;
    rig.online(OnlineMessage::CancelPremove).await;
    assert!(!rig.state.online.premove.is_set());
    rig.games[0].line(&state("e2e4"));
    rig.until("white move", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.pump(150).await;
    assert_eq!(rig.move_posts("e7e5"), 0);
    // Cancelling with nothing set is harmless.
    rig.online(OnlineMessage::CancelPremove).await;
}

#[tokio::test]
async fn a_premove_that_became_illegal_is_dropped_without_a_word() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("a7a6");
    // Black pawn a7; White's rook a1 will check along the e-file.
    black_game(&mut rig, Some("4k3/p7/8/8/8/8/8/R6K w - - 0 1")).await;
    rig.play("a7", "a6").await;
    assert!(rig.state.online.premove.is_set());
    rig.games[0].line(&state("a1e1"));
    rig.until("white move shown", |s| {
        s.session.game().mainline().count() == 1
    })
    .await;
    rig.pump(200).await;
    assert!(!rig.state.online.premove.is_set(), "the slot ends empty");
    assert_eq!(
        rig.move_posts("a7a6"),
        0,
        "an illegal premove is never sent"
    );
    assert_eq!(rig.mainline_count(), 1);
    assert!(
        !matches!(notice(&rig), Some(OnlineNotice::MoveRejected { .. })),
        "no remark: {:?}",
        notice(&rig)
    );
}

#[tokio::test]
async fn a_takeback_accepted_empties_the_premove() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("d2d4");
    rig.start_with(&full(&Full::white(), &state("e2e4"))).await;
    // White's e2e4 was played; Black is on the move: premove d2d4.
    rig.play("d2", "d4").await;
    assert!(rig.state.online.premove.is_set());
    // The takeback of the move of the user is accepted: fewer moves.
    rig.games[0].line(&state(""));
    rig.until("truncated", |s| {
        s.online.game.as_ref().is_some_and(|g| g.plies() == 0)
    })
    .await;
    assert!(!rig.state.online.premove.is_set());
    rig.pump(150).await;
    assert_eq!(rig.move_posts("d2d4"), 0);
}

#[tokio::test]
async fn the_end_of_the_game_empties_the_premove() {
    let mut rig = Rig::new(1).await;
    rig.move_ok("d2d4");
    rig.start_with(&full(&Full::white(), &state("e2e4"))).await;
    rig.play("d2", "d4").await;
    assert!(rig.state.online.premove.is_set());
    rig.games[0].line(&finished("e2e4", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert!(!rig.state.online.premove.is_set());
    rig.pump(150).await;
    assert_eq!(rig.move_posts("d2d4"), 0);
}

#[tokio::test]
async fn with_the_preference_off_a_move_out_of_turn_is_refused() {
    let mut rig = Rig::new(1).await;
    rig.state.prefs.online.premove = false;
    black_game(&mut rig, None).await;
    rig.play("e7", "e5").await;
    assert!(!rig.state.online.premove.is_set());
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotYourTurn))
    );
}

#[rstest]
#[case::impossible_for_the_piece("e8", "e6")]
#[case::piece_of_the_opponent("e1", "e2")]
#[case::empty_square("a4", "a5")]
#[tokio::test]
async fn a_premove_the_piece_could_never_make_is_refused(#[case] from: &str, #[case] to: &str) {
    let mut rig = Rig::new(1).await;
    black_game(&mut rig, Some("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1")).await;
    rig.play(from, to).await;
    assert!(!rig.state.online.premove.is_set(), "{from}{to}");
    assert_eq!(
        notice(&rig),
        Some(OnlineNotice::Rejected(RejectReason::NotYourTurn)),
        "{from}{to}"
    );
}
