//! Headless tests of the board widget with `iced_test`: mouse scripts in,
//! `BoardEvent`s out. Coordinates come from the (separately tested) pure
//! `BoardLayout`.

use iced::keyboard::{self, Modifiers};
use iced::{Event, Point, Settings, Size, mouse};
use iced_test::Simulator;
use mychess_core::{BoardMark, Position, Role, Square};
use mychess_gui::widgets::board::geometry::{BoardLayout, Orientation};
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};

const SIZE: Size = Size::new(800.0, 800.0);

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

struct Rig {
    position: Position,
    layout: BoardLayout,
    accepts: bool,
}

impl Rig {
    fn new(fen: &str) -> Rig {
        let options = BoardOptions::default();
        Rig {
            position: Position::from_fen(fen).expect("fen"),
            layout: BoardLayout::compute(SIZE, options.layout_options()).expect("layout fits"),
            accepts: true,
        }
    }

    fn at(&self, name: &str) -> Point {
        self.layout.square_center(sq(name), Orientation::White)
    }

    /// Runs `script` against a fresh simulator over the board and returns
    /// the published events.
    fn run(&self, script: impl FnOnce(&Rig, &mut Simulator<'_, BoardEvent>)) -> Vec<BoardEvent> {
        let options = BoardOptions::default();
        let brush = MarkBrush::default();
        let pieces = PieceSet::default();
        let caches = BoardCaches::default();
        let data = BoardData {
            position: &self.position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options,
            accepts_moves: self.accepts,
            brush: &brush,
            pieces: &pieces,
            animation: None,
        };
        let element = BoardWidget::new(data, &caches).into_element();
        let mut simulator = Simulator::with_size(Settings::default(), SIZE, element);
        script(self, &mut simulator);
        simulator.into_messages().collect()
    }
}

fn move_to(sim: &mut Simulator<'_, BoardEvent>, p: Point) {
    sim.point_at(p);
    sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: p })]);
}

fn press(sim: &mut Simulator<'_, BoardEvent>, button: mouse::Button) {
    sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(button))]);
}

fn release(sim: &mut Simulator<'_, BoardEvent>, button: mouse::Button) {
    sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(button))]);
}

fn click(sim: &mut Simulator<'_, BoardEvent>, p: Point) {
    move_to(sim, p);
    press(sim, mouse::Button::Left);
    release(sim, mouse::Button::Left);
}

fn modifiers(sim: &mut Simulator<'_, BoardEvent>, m: Modifiers) {
    sim.simulate([Event::Keyboard(keyboard::Event::ModifiersChanged(m))]);
}

/// `(from, to, promotion, dragged)` of each published move.
type Played = (Option<Square>, Option<Square>, Option<Role>, bool);

fn played(events: &[BoardEvent]) -> Vec<Played> {
    events
        .iter()
        .filter_map(|e| match e {
            BoardEvent::PlayMove { mv, dragged } => {
                Some((mv.from(), mv.to(), mv.promotion(), *dragged))
            }
            _ => None,
        })
        .collect()
}

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const PROMO: &str = "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1";

#[test]
fn click_then_click_publishes_the_move() {
    let rig = Rig::new(START);
    let events = rig.run(|rig, sim| {
        click(sim, rig.at("e2"));
        click(sim, rig.at("e4"));
    });
    assert_eq!(
        played(&events),
        [(Some(sq("e2")), Some(sq("e4")), None, false)]
    );
}

#[test]
fn dragging_a_knight_publishes_a_dragged_move() {
    let rig = Rig::new(START);
    let events = rig.run(|rig, sim| {
        move_to(sim, rig.at("g1"));
        press(sim, mouse::Button::Left);
        move_to(sim, rig.at("g2"));
        move_to(sim, rig.at("f3"));
        release(sim, mouse::Button::Left);
    });
    assert_eq!(
        played(&events),
        [(Some(sq("g1")), Some(sq("f3")), None, true)]
    );
}

#[test]
fn dragging_a_pawn_to_the_last_rank_opens_the_column_and_the_knight_cell_promotes() {
    let rig = Rig::new(PROMO);
    let events = rig.run(|rig, sim| {
        move_to(sim, rig.at("a7"));
        press(sim, mouse::Button::Left);
        move_to(sim, rig.at("a8"));
        release(sim, mouse::Button::Left);
        // The column runs a8 (queen), a7 (rook), a6 (bishop), a5 (knight).
        click(sim, rig.at("a5"));
    });
    assert_eq!(
        played(&events),
        [(Some(sq("a7")), Some(sq("a8")), Some(Role::Knight), true)]
    );
}

#[test]
fn moves_are_ignored_when_the_board_does_not_accept_them() {
    let mut rig = Rig::new(START);
    rig.accepts = false;
    let events = rig.run(|rig, sim| {
        click(sim, rig.at("e2"));
        click(sim, rig.at("e4"));
        move_to(sim, rig.at("d4"));
        press(sim, mouse::Button::Right);
        move_to(sim, rig.at("f6"));
        release(sim, mouse::Button::Right);
    });
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn right_drag_publishes_a_green_arrow_and_shift_makes_it_firebrick() {
    let rig = Rig::new(START);
    let drag = |color: Option<Modifiers>| {
        rig.run(move |rig, sim| {
            if let Some(m) = color {
                modifiers(sim, m);
            }
            move_to(sim, rig.at("d4"));
            press(sim, mouse::Button::Right);
            move_to(sim, rig.at("f6"));
            release(sim, mouse::Button::Right);
        })
    };
    assert_eq!(
        drag(None),
        [BoardEvent::ToggleMark(BoardMark::arrow(
            sq("d4"),
            sq("f6"),
            "green"
        ))]
    );
    assert_eq!(
        drag(Some(Modifiers::SHIFT)),
        [BoardEvent::ToggleMark(BoardMark::arrow(
            sq("d4"),
            sq("f6"),
            "firebrick"
        ))]
    );
}

#[test]
fn right_click_on_one_square_toggles_a_circle() {
    let rig = Rig::new(START);
    let events = rig.run(|rig, sim| {
        move_to(sim, rig.at("d4"));
        press(sim, mouse::Button::Right);
        release(sim, mouse::Button::Right);
    });
    assert_eq!(
        events,
        [BoardEvent::ToggleMark(BoardMark::square(
            mychess_core::MarkKind::Circle,
            sq("d4"),
            "green"
        ))]
    );
}
