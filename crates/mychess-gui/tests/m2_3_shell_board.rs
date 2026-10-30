//! M2-3 board events reaching the `Shell` (rules 2, 39, 51, 56, 57): the
//! `BoardEvent`s become application messages, and the visual state follows.

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use std::time::{Duration, Instant};

use mychess_app::Action;
use mychess_core::{BoardMark, MarkKind, Move, Role, Square};
use mychess_gui::widgets::board::geometry::Orientation;
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::{Message, Shell, UiMessage};

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn shell() -> Shell {
    Shell::new(None)
}

fn board(shell: &mut Shell, event: BoardEvent) {
    drop(shell.update(Message::Ui(UiMessage::Board(event))));
}

fn legal(shell: &Shell, from: &str, to: &str) -> Move {
    shell
        .app
        .session
        .position()
        .find_move(sq(from), sq(to), None)
        .unwrap_or_else(|| panic!("{from}{to} is not legal"))
}

fn play(shell: &mut Shell, from: &str, to: &str, dragged: bool) {
    let mv = legal(shell, from, to);
    board(shell, BoardEvent::PlayMove { mv, dragged });
}

fn frame(shell: &mut Shell, at: Instant) {
    drop(shell.update(Message::Ui(UiMessage::AnimationFrame(at))));
}

#[test]
fn m2_3_shell_play_move_event_reaches_the_game() {
    let mut s = shell();
    play(&mut s, "e2", "e4", false);
    let pos = s.app.session.position();
    assert_eq!(pos.turn(), mychess_core::Color::Black);
    assert!(pos.piece_at(sq("e4")).is_some());
    assert!(pos.piece_at(sq("e2")).is_none());
}

#[test]
fn m2_3_shell_click_move_starts_an_animation_and_frames_end_it() {
    let mut s = shell();
    assert!(!s.ui.is_animating());
    play(&mut s, "e2", "e4", false);
    assert!(s.ui.is_animating(), "click-click animates (rule 56)");
    let t0 = Instant::now();
    frame(&mut s, t0);
    assert!(s.ui.is_animating(), "first frame only starts the clock");
    frame(&mut s, t0 + Duration::from_millis(100));
    assert!(s.ui.is_animating(), "100 of 200 ms");
    frame(&mut s, t0 + Duration::from_millis(199));
    assert!(s.ui.is_animating());
    frame(&mut s, t0 + Duration::from_millis(200));
    assert!(!s.ui.is_animating(), "done at 200 ms");
    // A stray frame without animation is harmless.
    frame(&mut s, t0 + Duration::from_secs(5));
    assert!(!s.ui.is_animating());
}

#[test]
fn m2_3_shell_frames_going_backwards_in_time_do_not_panic_or_end_early() {
    let mut s = shell();
    play(&mut s, "e2", "e4", false);
    let t0 = Instant::now() + Duration::from_secs(10);
    frame(&mut s, t0);
    frame(&mut s, t0.checked_sub(Duration::from_secs(5)).unwrap_or(t0));
    assert!(s.ui.is_animating());
}

#[test]
fn m2_3_shell_dragged_move_does_not_animate_and_the_next_click_move_does() {
    let mut s = shell();
    play(&mut s, "e2", "e4", true);
    assert!(!s.ui.is_animating(), "dragged moves never animate");
    // The suppression is one-shot.
    play(&mut s, "e7", "e5", false);
    assert!(s.ui.is_animating());
}

#[test]
fn m2_3_shell_zero_animation_time_turns_animation_off() {
    let mut s = shell();
    s.app.prefs.board.animation_ms = 0;
    play(&mut s, "e2", "e4", false);
    assert!(!s.ui.is_animating());
}

#[test]
fn m2_3_shell_navigation_back_animates_and_a_new_move_replaces_the_animation() {
    let mut s = shell();
    play(&mut s, "e2", "e4", true);
    drop(s.update(Message::Ui(UiMessage::Action(Action::Back))));
    assert!(s.ui.is_animating(), "stepping back one ply slides back");
    // A new change ends the running animation and starts the next one.
    play(&mut s, "d2", "d4", false);
    assert!(s.ui.is_animating());
}

#[test]
fn m2_3_shell_jumping_to_the_end_of_a_long_game_does_not_animate() {
    let mut s = shell();
    for (from, to) in [("e2", "e4"), ("e7", "e5"), ("g1", "f3")] {
        play(&mut s, from, to, true);
    }
    drop(s.update(Message::Ui(UiMessage::Action(Action::GoStart))));
    assert!(!s.ui.is_animating(), "three plies at once is a jump");
    drop(s.update(Message::Ui(UiMessage::Action(Action::GoEnd))));
    assert!(!s.ui.is_animating());
}

#[test]
fn m2_3_shell_flipping_turns_the_board_and_stops_the_animation() {
    let mut s = shell();
    assert_eq!(s.ui.orientation, Orientation::White);
    play(&mut s, "e2", "e4", false);
    assert!(s.ui.is_animating());
    drop(s.update(Message::Ui(UiMessage::FlipBoard)));
    assert_eq!(s.ui.orientation, Orientation::Black);
    assert!(!s.ui.is_animating(), "rule 56: flipping ends the animation");
    drop(s.update(Message::Ui(UiMessage::Action(Action::FlipBoard))));
    assert_eq!(s.ui.orientation, Orientation::White);
}

#[test]
fn m2_3_shell_null_move_passes_the_turn_without_animation() {
    let mut s = shell();
    board(&mut s, BoardEvent::PlayNullMove);
    assert_eq!(s.app.session.position().turn(), mychess_core::Color::Black);
    assert!(!s.ui.is_animating(), "the null move has no sprite");
}

#[test]
fn m2_3_shell_toggle_mark_writes_and_erases_the_comment_mark() {
    let mut s = shell();
    play(&mut s, "e2", "e4", true);
    let circle = BoardMark::square(MarkKind::Circle, sq("d4"), "green");
    let arrow = BoardMark::arrow(sq("d4"), sq("f6"), "firebrick");
    board(&mut s, BoardEvent::ToggleMark(circle.clone()));
    assert_eq!(s.ui.marks(), std::slice::from_ref(&circle));
    board(&mut s, BoardEvent::ToggleMark(arrow.clone()));
    assert_eq!(s.ui.marks(), [circle.clone(), arrow.clone()]);
    // The same gesture twice restores the comment (acceptance criterion).
    board(&mut s, BoardEvent::ToggleMark(arrow));
    assert_eq!(s.ui.marks(), std::slice::from_ref(&circle));
    board(&mut s, BoardEvent::ToggleMark(circle));
    assert!(s.ui.marks().is_empty());
}

#[test]
fn m2_3_shell_marks_belong_to_the_current_node_only() {
    let mut s = shell();
    play(&mut s, "e2", "e4", true);
    let mark = BoardMark::square(MarkKind::Disk, sq("a1"), "blue");
    board(&mut s, BoardEvent::ToggleMark(mark.clone()));
    assert_eq!(s.ui.marks(), std::slice::from_ref(&mark));
    drop(s.update(Message::Ui(UiMessage::Action(Action::GoStart))));
    assert!(s.ui.marks().is_empty(), "the start node has no marks");
    drop(s.update(Message::Ui(UiMessage::Action(Action::GoEnd))));
    assert_eq!(s.ui.marks(), [mark]);
}

#[test]
fn m2_3_shell_marking_turns_show_marks_on() {
    // Scid end.tcl:1523 and 1548: any mark gesture sets showMarks.
    let mut s = shell();
    s.options.apply(mychess_gui::ViewChange::ShowMarks(false));
    assert!(!s.options.show_marks);
    play(&mut s, "e2", "e4", true);
    board(
        &mut s,
        BoardEvent::ToggleMark(BoardMark::square(MarkKind::Circle, sq("d4"), "green")),
    );
    assert!(s.options.show_marks);
}

#[test]
fn m2_3_shell_redraw_event_changes_nothing() {
    let mut s = shell();
    let before = s.app.session.position().zobrist();
    board(&mut s, BoardEvent::Redraw);
    assert_eq!(s.app.session.position().zobrist(), before);
    assert!(!s.ui.is_animating());
}

// M2-5 (D234, D240): the material left the canvas; the strips come from the
// application and follow the cursor.
#[test]
fn m2_5_shell_player_strips_follow_the_position() {
    let mut s = shell();
    for (from, to) in [("e2", "e4"), ("d7", "d5"), ("e4", "d5"), ("d8", "d5")] {
        play(&mut s, from, to, true);
    }
    // 1.e4 d5 2.exd5 Qxd5: a pawn each, so nobody is ahead (`Captured` is
    // the default mode).
    let strips = s.app.player_strips();
    assert_eq!(strips.white.captured, vec![(Role::Pawn, 1)]);
    assert_eq!(strips.black.captured, vec![(Role::Pawn, 1)]);
    assert_eq!(strips.white.advantage, None);
    assert_eq!(strips.black.advantage, None);
}

#[test]
fn m2_3_shell_a_new_game_after_one_move_is_a_jump_not_an_animation() {
    // Rule 54 (4): "outra partida" is a jump. The board of the new game
    // equals the position before the last move, but nothing slides.
    let mut s = shell();
    play(&mut s, "e2", "e4", true);
    drop(s.update(Message::Ui(UiMessage::Action(Action::NewGame))));
    drop(s.update(Message::App(mychess_app::Message::AnswerDiscard(
        mychess_app::DiscardChoice::DontSave,
    ))));
    assert_eq!(
        s.app.session.position().zobrist(),
        mychess_core::Position::default().zobrist(),
        "the new game is on the start position"
    );
    assert!(!s.ui.is_animating(), "a new game must not slide e4 back");
}
