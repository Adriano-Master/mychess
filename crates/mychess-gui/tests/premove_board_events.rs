//! Premove entry through the real board canvas (spec M8-3, D1321, D1390-D1394).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use iced::keyboard::{self, key::Named};
use iced::{Event, Point, Settings, Size, mouse};
use iced_test::Simulator;
use iced_test::simulator::press_key;
use mychess_core::{Color, Position, Square};
use mychess_gui::widgets::board::geometry::BoardLayout;
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::premove::{PremoveBoardEvent, PremoveEvent, PremoveSetup};
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};

/// White has played e4; it is Black's turn, so White premoves.
const BLACK_TO_MOVE: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
const SIZE: Size = Size::new(800.0, 800.0);

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn at(name: &str) -> Point {
    BoardLayout::compute(SIZE, BoardOptions::default().layout_options())
        .expect("layout")
        .square_center(
            sq(name),
            mychess_gui::widgets::board::geometry::Orientation::default(),
        )
}

/// Same-file moves only: enough to tell accepted from refused.
fn same_file(from: Square, to: Square) -> bool {
    from.file() == to.file() && from != to
}

fn run(
    premove: Option<(Square, Square)>,
    accepts: bool,
    script: impl FnOnce(&mut Simulator<'_, PremoveBoardEvent>),
) -> Vec<PremoveBoardEvent> {
    let position = Position::from_fen(BLACK_TO_MOVE).expect("fen");
    let (brush, pieces, caches) = (
        MarkBrush::default(),
        PieceSet::default(),
        BoardCaches::default(),
    );
    let data = BoardData {
        position: &position,
        last_move: None,
        marks: &[],
        orientation: mychess_gui::widgets::board::geometry::Orientation::default(),
        options: BoardOptions::default(),
        accepts_moves: accepts,
        brush: &brush,
        pieces: &pieces,
        animation: None,
    };
    let can = same_file;
    let widget = BoardWidget::new(data, &caches).with_premove(PremoveSetup {
        color: Color::White,
        premove,
        can_premove: &can,
    });
    let mut sim = Simulator::with_size(Settings::default(), SIZE, widget.into_premove_element());
    script(&mut sim);
    sim.into_messages().collect()
}

fn move_to(sim: &mut Simulator<'_, PremoveBoardEvent>, p: Point) {
    sim.point_at(p);
    sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: p })]);
}

fn click(sim: &mut Simulator<'_, PremoveBoardEvent>, p: Point) {
    move_to(sim, p);
    sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
        mouse::Button::Left,
    ))]);
    sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(
        mouse::Button::Left,
    ))]);
}

fn sets(events: &[PremoveBoardEvent]) -> Vec<(Square, Square)> {
    events
        .iter()
        .filter_map(|e| match e {
            PremoveBoardEvent::Premove(PremoveEvent::Set { from, to }) => Some((*from, *to)),
            _ => None,
        })
        .collect()
}

fn cancels(events: &[PremoveBoardEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, PremoveBoardEvent::Premove(PremoveEvent::Cancel)))
        .count()
}

#[test]
fn click_click_publishes_a_premove_and_no_board_move() {
    let events = run(None, true, |sim| {
        click(sim, at("d2"));
        click(sim, at("d4"));
    });
    assert_eq!(sets(&events), [(sq("d2"), sq("d4"))]);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, PremoveBoardEvent::Board(_))),
        "the board itself plays nothing: {events:?}"
    );
}

#[test]
fn a_drag_publishes_a_premove() {
    let events = run(None, true, |sim| {
        move_to(sim, at("a1"));
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
        move_to(sim, at("a2"));
        move_to(sim, at("a3"));
        sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))]);
    });
    assert_eq!(sets(&events), [(sq("a1"), sq("a3"))]);
}

#[test]
fn a_refused_destination_publishes_nothing() {
    let events = run(None, true, |sim| {
        click(sim, at("d2"));
        click(sim, at("e3"));
    });
    assert!(sets(&events).is_empty(), "{events:?}");
}

#[test]
fn enemy_pieces_do_not_start_a_premove() {
    let events = run(None, true, |sim| {
        click(sim, at("d7"));
        click(sim, at("d5"));
    });
    assert!(sets(&events).is_empty(), "{events:?}");
}

#[test]
fn escape_cancels_the_premove_kept() {
    let events = run(Some((sq("d2"), sq("d4"))), true, |sim| {
        sim.simulate([press_key(keyboard::Key::Named(Named::Escape), None)]);
    });
    assert_eq!(cancels(&events), 1, "{events:?}");
}

#[test]
fn right_click_cancels_the_premove_kept() {
    let events = run(Some((sq("d2"), sq("d4"))), true, |sim| {
        move_to(sim, at("e5"));
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right,
        ))]);
    });
    assert_eq!(cancels(&events), 1, "{events:?}");
}

#[test]
fn escape_without_a_premove_publishes_nothing() {
    let events = run(None, true, |sim| {
        sim.simulate([press_key(keyboard::Key::Named(Named::Escape), None)]);
    });
    assert_eq!(cancels(&events), 0);
}

#[test]
fn the_board_takes_no_marks_in_premove_mode() {
    // Without the layer a right drag would publish a mark.
    let events = run(None, true, |sim| {
        move_to(sim, at("e5"));
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right,
        ))]);
        move_to(sim, at("e6"));
        sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Right,
        ))]);
    });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, PremoveBoardEvent::Board(_))),
        "{events:?}"
    );
}
