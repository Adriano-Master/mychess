//! Premove entry for the board (spec M8-3, D1321, D1390-D1394).
//!
//! While it is not the user's turn, the board can take one premove: a click
//! on one of his pieces and a click on a destination, or a drag. The move is
//! only checked for being pseudo-legal (by a predicate the caller supplies,
//! so this module knows nothing about Lichess or the application state);
//! the application validates it against the position that results from the
//! opponent's move and sends it, or drops it without a word.
//!
//! [`PremoveEntry`] is the pure state machine (no drawing, no iced widgets);
//! `PremoveLayer` is the canvas on top of the board that feeds it and draws
//! the premove. `BoardWidget::into_premove_element` assembles both.

use iced::widget::canvas::{self, Action, Event, Frame};
use iced::{Color, Point, Rectangle, mouse};
use mychess_core::{Color as Side, Position, Square, ZobristHash};

use super::BoardData;
use super::geometry::{BoardLayout, Orientation};
use super::interaction::{BoardEvent, DRAG_THRESHOLD, Input};
use super::layers::{centered, draw_piece, fill_rect};
use super::pieces::PieceSet;
use iced::keyboard;

/// Alpha of the fills of the premove squares.
const PREMOVE_ALPHA: f32 = 0.55;
/// Alpha of the ghost left on the origin of a dragged piece.
const GHOST_ALPHA: f32 = 0.4;

/// The color of the premove squares (a blue that reads on every board theme;
/// Lichess uses the same family for premoves).
pub fn premove_color() -> Color {
    Color::from_rgb8(0x14, 0x1e, 0x55).scale_alpha(PREMOVE_ALPHA)
}

/// What the premove entry asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PremoveEvent {
    /// The user entered a premove (replaces the previous one). A pawn that
    /// reaches the last rank promotes to a queen unless the owner picks
    /// another piece (`mychess_app` `Premove::enter`).
    Set {
        /// Origin.
        from: Square,
        /// Destination.
        to: Square,
    },
    /// The user asked to drop the premove (`Esc` or right click).
    Cancel,
    /// Only a redraw is needed.
    Redraw,
}

/// What the premove layer publishes: the events of the board below it and
/// the premove events.
#[derive(Debug, Clone, PartialEq)]
pub enum PremoveBoardEvent {
    /// An event of the board canvas.
    Board(BoardEvent),
    /// An event of the premove entry.
    Premove(PremoveEvent),
}

/// What the owner tells the board about the premove.
#[derive(Clone, Copy)]
pub struct PremoveSetup<'a> {
    /// The side of the user: only his pieces can start a premove.
    pub color: Side,
    /// The premove kept by the application, drawn on the board.
    pub premove: Option<(Square, Square)>,
    /// Whether `(from, to)` is an acceptable premove for the position shown
    /// (`mychess_app` `is_pseudo_legal` bound to that position and `color`).
    pub can_premove: &'a dyn Fn(Square, Square) -> bool,
}

impl std::fmt::Debug for PremoveSetup<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PremoveSetup")
            .field("color", &self.color)
            .field("premove", &self.premove)
            .finish_non_exhaustive()
    }
}

/// What [`PremoveEntry::handle`] reads about the board.
#[derive(Clone, Copy)]
pub struct PremoveContext<'a> {
    /// Where the board is.
    pub layout: BoardLayout,
    /// Who is at the bottom.
    pub orientation: Orientation,
    /// The drawn position.
    pub position: &'a Position,
    /// The setup given by the owner.
    pub setup: PremoveSetup<'a>,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    square: Square,
    origin: Point,
    was_selected: bool,
}

/// A piece dragged for a premove.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PremoveDrag {
    /// Origin square.
    pub from: Square,
    /// Pointer position.
    pub cursor: Point,
}

/// The pointer state of the premove entry.
#[derive(Debug, Clone, Default)]
pub struct PremoveEntry {
    selected: Option<Square>,
    press: Option<Press>,
    drag: Option<PremoveDrag>,
    hover: Option<Square>,
}

impl PremoveEntry {
    /// The square chosen as origin.
    pub fn selected(&self) -> Option<Square> {
        self.selected
    }

    /// The piece being dragged.
    pub fn drag(&self) -> Option<PremoveDrag> {
        self.drag
    }

    /// Whether nothing is selected, pressed or dragged.
    pub fn is_idle(&self) -> bool {
        self.selected.is_none() && self.press.is_none() && self.drag.is_none()
    }

    fn clear(&mut self) {
        *self = PremoveEntry {
            hover: self.hover,
            ..PremoveEntry::default()
        };
    }

    fn reset(&mut self) -> Vec<PremoveEvent> {
        if self.is_idle() {
            return Vec::new();
        }
        self.clear();
        vec![PremoveEvent::Redraw]
    }

    fn owns(ctx: &PremoveContext<'_>, sq: Square) -> bool {
        ctx.position
            .piece_at(sq)
            .is_some_and(|p| p.color == ctx.setup.color)
    }

    /// Applies one input (the same [`Input`]s as the board; modifiers are
    /// ignored).
    pub fn handle(&mut self, input: Input, ctx: &PremoveContext<'_>) -> Vec<PremoveEvent> {
        match input {
            Input::PositionChanged | Input::OrientationChanged => self.reset(),
            Input::LeftPressed { pos, .. } => self.left_pressed(pos, ctx),
            Input::LeftReleased { pos, .. } => self.left_released(pos, ctx),
            Input::RightPressed { .. } | Input::Escape => self.cancel(ctx),
            Input::RightReleased { .. } | Input::Enter => Vec::new(),
            Input::CursorMoved { pos } => self.cursor_moved(pos, ctx),
            Input::CursorLeft => {
                self.hover = None;
                Vec::new()
            }
        }
    }

    /// `Esc` and right click: the selection first, then the premove.
    fn cancel(&mut self, ctx: &PremoveContext<'_>) -> Vec<PremoveEvent> {
        let was_active = !self.is_idle();
        self.clear();
        // D1392: a selection (or drag) is dropped first; only an idle
        // press cancels the stored premove.
        if was_active {
            vec![PremoveEvent::Redraw]
        } else if ctx.setup.premove.is_some() {
            vec![PremoveEvent::Cancel, PremoveEvent::Redraw]
        } else {
            Vec::new()
        }
    }

    fn enter(&mut self, from: Square, to: Square, ctx: &PremoveContext<'_>) -> Vec<PremoveEvent> {
        self.clear();
        if (ctx.setup.can_premove)(from, to) {
            vec![PremoveEvent::Set { from, to }, PremoveEvent::Redraw]
        } else {
            vec![PremoveEvent::Redraw]
        }
    }

    fn left_pressed(&mut self, pos: Point, ctx: &PremoveContext<'_>) -> Vec<PremoveEvent> {
        if self.drag.take().is_some() {
            // A second press during a drag: the release was lost.
            self.clear();
            return vec![PremoveEvent::Redraw];
        }
        let Some(target) = ctx.layout.square_at(pos, ctx.orientation) else {
            let changed = !self.is_idle();
            self.clear();
            return if changed {
                vec![PremoveEvent::Redraw]
            } else {
                Vec::new()
            };
        };
        if let Some(from) = self.selected.filter(|&from| from != target)
            && (ctx.setup.can_premove)(from, target)
        {
            return self.enter(from, target, ctx);
        }
        if Self::owns(ctx, target) {
            self.press = Some(Press {
                square: target,
                origin: pos,
                was_selected: self.selected == Some(target),
            });
            self.selected = Some(target);
        } else {
            self.selected = None;
            self.press = None;
        }
        vec![PremoveEvent::Redraw]
    }

    fn left_released(&mut self, pos: Point, ctx: &PremoveContext<'_>) -> Vec<PremoveEvent> {
        let target = ctx.layout.square_at(pos, ctx.orientation);
        if let Some(drag) = self.drag.take() {
            self.press = None;
            return match target {
                Some(t) if t != drag.from => self.enter(drag.from, t, ctx),
                Some(_) => vec![PremoveEvent::Redraw],
                None => {
                    self.selected = None;
                    vec![PremoveEvent::Redraw]
                }
            };
        }
        if let Some(press) = self.press.take()
            && press.was_selected
            && target == Some(press.square)
        {
            self.selected = None;
            return vec![PremoveEvent::Redraw];
        }
        Vec::new()
    }

    fn cursor_moved(&mut self, pos: Point, ctx: &PremoveContext<'_>) -> Vec<PremoveEvent> {
        self.hover = ctx.layout.square_at(pos, ctx.orientation);
        if let Some(drag) = &mut self.drag {
            drag.cursor = pos;
            return vec![PremoveEvent::Redraw];
        }
        if let Some(press) = self.press {
            let (dx, dy) = (pos.x - press.origin.x, pos.y - press.origin.y);
            if dx.hypot(dy) > DRAG_THRESHOLD {
                self.drag = Some(PremoveDrag {
                    from: press.square,
                    cursor: pos,
                });
                return vec![PremoveEvent::Redraw];
            }
        }
        Vec::new()
    }
}

/// The canvas above the board: takes the pointer and the keys of the
/// premove and draws it. It only exists while a premove can be entered.
pub(super) struct PremoveLayer<'a> {
    pub(super) data: BoardData<'a>,
    pub(super) setup: PremoveSetup<'a>,
    pub(super) pieces: &'a PieceSet,
}

#[derive(Debug, Default)]
pub(super) struct LayerState {
    entry: PremoveEntry,
    seen: Option<(ZobristHash, Orientation)>,
}

impl PremoveLayer<'_> {
    fn layout(&self, bounds: Rectangle) -> Option<BoardLayout> {
        BoardLayout::compute(bounds.size(), self.data.options.layout_options())
    }

    fn context(&self, layout: BoardLayout) -> PremoveContext<'_> {
        PremoveContext {
            layout,
            orientation: self.data.orientation,
            position: self.data.position,
            setup: self.setup,
        }
    }
}

fn translate(event: &Event, local: Option<Point>, inside: bool) -> Option<Input> {
    use keyboard::{Key, key::Named};
    let pos = local.unwrap_or(Point::new(f32::NAN, f32::NAN));
    let mods = super::interaction::Mods::default();
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if inside => {
            Some(Input::LeftPressed { pos, mods })
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Input::LeftReleased { pos, mods })
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) if inside => {
            Some(Input::RightPressed { pos, mods })
        }
        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
            local.map(|pos| Input::CursorMoved { pos })
        }
        Event::Mouse(mouse::Event::CursorLeft) => Some(Input::CursorLeft),
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) => Some(Input::Escape),
        _ => None,
    }
}

impl canvas::Program<PremoveBoardEvent> for PremoveLayer<'_> {
    type State = LayerState;

    fn update(
        &self,
        state: &mut LayerState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<PremoveBoardEvent>> {
        let layout = self.layout(bounds)?;
        let ctx = self.context(layout);
        let mut events = Vec::new();
        let identity = (self.data.position.zobrist(), self.data.orientation);
        if state.seen.is_some_and(|seen| seen != identity) {
            let input = if state.seen.is_some_and(|(h, _)| h != identity.0) {
                Input::PositionChanged
            } else {
                Input::OrientationChanged
            };
            events.extend(state.entry.handle(input, &ctx));
        }
        state.seen = Some(identity);

        let local = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        let inside = cursor.position_in(bounds).is_some();
        let input = translate(event, local, inside);
        let captured = input.is_some_and(|i| {
            matches!(
                i,
                Input::LeftPressed { .. }
                    | Input::LeftReleased { .. }
                    | Input::RightPressed { .. }
                    | Input::Escape
            )
        }) && (inside || !state.entry.is_idle() || self.setup.premove.is_some());
        let before = events.len();
        if let Some(input) = input {
            events.extend(state.entry.handle(input, &ctx));
        }
        // An Esc that cancelled something is consumed, so the shell does
        // not also act on it.
        let esc_used = matches!(input, Some(Input::Escape)) && events.len() > before;
        let published = events
            .iter()
            .find(|e| !matches!(e, PremoveEvent::Redraw))
            .copied();
        let action = match published {
            Some(e) => Some(Action::publish(PremoveBoardEvent::Premove(e))),
            None => (!events.is_empty()).then(Action::request_redraw),
        };
        // Mouse presses inside the board belong to the premove: the canvas
        // below (which takes no moves in this mode) must not also act.
        if (captured && matches!(event, Event::Mouse(_)))
            || (esc_used && matches!(event, Event::Keyboard(_)))
        {
            return Some(action.map_or_else(Action::capture, Action::and_capture));
        }
        action
    }

    fn draw(
        &self,
        state: &LayerState,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(layout) = self.layout(bounds) else {
            return Vec::new();
        };
        let o = self.data.orientation;
        let mut frame = Frame::new(renderer, bounds.size());
        let color = premove_color();
        if let Some((from, to)) = self.setup.premove {
            fill_rect(&mut frame, layout.square_rect(from, o), color);
            fill_rect(&mut frame, layout.square_rect(to, o), color);
        }
        let live = state
            .seen
            .is_none_or(|seen| seen == (self.data.position.zobrist(), o));
        if live {
            if let Some(sel) = state.entry.selected() {
                fill_rect(&mut frame, layout.square_rect(sel, o), color);
            }
            if let Some(drag) = state.entry.drag()
                && let Some(piece) = self.data.position.piece_at(drag.from)
            {
                if let Some(hover) = layout.square_at(drag.cursor, o) {
                    fill_rect(&mut frame, layout.square_rect(hover, o), color);
                }
                draw_piece(
                    &mut frame,
                    self.pieces,
                    piece,
                    layout.square_rect(drag.from, o),
                    GHOST_ALPHA,
                );
                draw_piece(
                    &mut frame,
                    self.pieces,
                    piece,
                    centered(drag.cursor, layout.s),
                    1.0,
                );
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &LayerState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.entry.drag().is_some() {
            return mouse::Interaction::Grabbing;
        }
        let Some(layout) = self.layout(bounds) else {
            return mouse::Interaction::default();
        };
        let over_own = cursor
            .position_in(bounds)
            .and_then(|p| layout.square_at(p, self.data.orientation))
            .is_some_and(|sq| {
                self.data
                    .position
                    .piece_at(sq)
                    .is_some_and(|p| p.color == self.setup.color)
            });
        if over_own {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::Size;

    use super::*;
    use crate::widgets::board::geometry::{CoordsMode, LayoutOptions};
    use crate::widgets::board::interaction::Mods;

    fn sq(name: &str) -> Square {
        name.parse().expect("square")
    }

    struct Rig {
        layout: BoardLayout,
        position: Position,
        premove: Option<(Square, Square)>,
        entry: PremoveEntry,
    }

    impl Rig {
        fn new() -> Rig {
            // Black to move: White premoves.
            Rig {
                layout: BoardLayout::compute(
                    Size::new(800.0, 800.0),
                    LayoutOptions {
                        coords: CoordsMode::None,
                    },
                )
                .expect("layout"),
                position: Position::from_fen(
                    "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
                )
                .expect("fen"),
                premove: None,
                entry: PremoveEntry::default(),
            }
        }

        fn send(&mut self, input: Input) -> Vec<PremoveEvent> {
            let position = self.position.clone();
            // A short-range predicate: any square of a different file or
            // rank within two steps (enough to tell yes from no).
            let can = move |from: Square, to: Square| {
                position.piece_at(from).is_some()
                    && i32::from(u8::from(from.file())) == i32::from(u8::from(to.file()))
                    && from != to
            };
            let ctx = PremoveContext {
                layout: self.layout,
                orientation: Orientation::White,
                position: &self.position,
                setup: PremoveSetup {
                    color: Side::White,
                    premove: self.premove,
                    can_premove: &can,
                },
            };
            self.entry.handle(input, &ctx)
        }

        fn at(&self, name: &str) -> Point {
            self.layout.square_center(sq(name), Orientation::White)
        }

        fn click(&mut self, name: &str) -> Vec<PremoveEvent> {
            let pos = self.at(name);
            let mods = Mods::default();
            let mut out = self.send(Input::LeftPressed { pos, mods });
            out.extend(self.send(Input::LeftReleased { pos, mods }));
            out
        }
    }

    #[test]
    fn click_then_click_enters_a_premove() {
        let mut r = Rig::new();
        r.click("d2");
        assert_eq!(r.entry.selected(), Some(sq("d2")));
        let events = r.click("d4");
        assert!(events.contains(&PremoveEvent::Set {
            from: sq("d2"),
            to: sq("d4")
        }));
        assert!(r.entry.is_idle());
    }

    #[test]
    fn only_the_pieces_of_the_user_start_a_premove() {
        let mut r = Rig::new();
        r.click("d7");
        assert_eq!(r.entry.selected(), None, "an enemy piece");
        r.click("d5");
        assert_eq!(r.entry.selected(), None, "an empty square");
    }

    #[test]
    fn a_destination_the_predicate_refuses_enters_nothing() {
        let mut r = Rig::new();
        r.click("d2");
        let events = r.click("e3");
        assert!(!events.iter().any(|e| matches!(e, PremoveEvent::Set { .. })));
        // The click on another own piece... e3 is empty, selection dropped.
        assert_eq!(r.entry.selected(), None);
    }

    #[test]
    fn a_drag_enters_a_premove() {
        let mut r = Rig::new();
        let mods = Mods::default();
        let from = r.at("a1");
        r.send(Input::LeftPressed { pos: from, mods });
        let far = Point::new(from.x, from.y - 120.0);
        r.send(Input::CursorMoved { pos: far });
        assert!(r.entry.drag().is_some());
        let to = r.at("a4");
        let events = r.send(Input::LeftReleased { pos: to, mods });
        assert!(events.contains(&PremoveEvent::Set {
            from: sq("a1"),
            to: sq("a4")
        }));
        assert!(r.entry.drag().is_none());
    }

    #[test]
    fn escape_and_right_click_cancel_the_premove() {
        let mut r = Rig::new();
        assert!(r.send(Input::Escape).is_empty(), "nothing to cancel");
        r.premove = Some((sq("d2"), sq("d4")));
        assert!(r.send(Input::Escape).contains(&PremoveEvent::Cancel));
        let pos = r.at("e5");
        let right = Input::RightPressed {
            pos,
            mods: Mods::default(),
        };
        assert!(r.send(right).contains(&PremoveEvent::Cancel));
    }

    #[test]
    fn escape_drops_the_selection_without_cancelling_when_no_premove() {
        let mut r = Rig::new();
        r.click("d2");
        let events = r.send(Input::Escape);
        assert_eq!(events, vec![PremoveEvent::Redraw]);
        assert_eq!(r.entry.selected(), None);
    }

    #[test]
    fn escape_with_a_selection_and_a_premove_drops_only_the_selection() {
        let mut r = Rig::new();
        r.premove = Some((sq("d2"), sq("d4")));
        r.click("a2");
        assert_eq!(r.entry.selected(), Some(sq("a2")));
        // D1392: the selection first, the premove on the next press.
        let first = r.send(Input::Escape);
        assert_eq!(first, vec![PremoveEvent::Redraw]);
        assert!(r.send(Input::Escape).contains(&PremoveEvent::Cancel));
    }

    fn escape_event() -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    }

    /// Runs the canvas program on a key event, returning whether the
    /// event came back captured and the message published.
    fn key_through_layer(premove: Option<(Square, Square)>) -> (bool, Option<PremoveBoardEvent>) {
        use canvas::Program;
        use iced::event::Status;
        let position =
            Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
                .expect("fen");
        let (brush, pieces) = (
            crate::widgets::board::marks::MarkBrush::default(),
            PieceSet::default(),
        );
        let data = BoardData {
            position: &position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options: crate::widgets::board::BoardOptions::default(),
            accepts_moves: false,
            brush: &brush,
            pieces: &pieces,
            animation: None,
        };
        let can = |_: Square, _: Square| true;
        let layer = PremoveLayer {
            data,
            setup: PremoveSetup {
                color: Side::White,
                premove,
                can_premove: &can,
            },
            pieces: &pieces,
        };
        let mut state = LayerState::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 800.0));
        let action = layer.update(
            &mut state,
            &escape_event(),
            bounds,
            mouse::Cursor::Unavailable,
        );
        match action {
            None => (false, None),
            Some(action) => {
                let (message, _, status) = action.into_inner();
                (status == Status::Captured, message)
            }
        }
    }

    #[test]
    fn escape_that_cancels_a_premove_is_captured() {
        let (captured, message) = key_through_layer(Some((sq("d2"), sq("d4"))));
        assert_eq!(
            message,
            Some(PremoveBoardEvent::Premove(PremoveEvent::Cancel))
        );
        assert!(captured, "the shell must not also see this Esc");
    }

    #[test]
    fn escape_with_nothing_to_cancel_is_left_to_the_shell() {
        let (captured, message) = key_through_layer(None);
        assert_eq!(message, None);
        assert!(!captured);
    }

    #[test]
    fn a_new_position_clears_the_selection() {
        let mut r = Rig::new();
        r.click("d2");
        assert_eq!(r.send(Input::PositionChanged), vec![PremoveEvent::Redraw]);
        assert!(r.entry.is_idle());
    }

    #[test]
    fn clicking_the_selected_piece_again_deselects_it() {
        let mut r = Rig::new();
        r.click("d2");
        r.click("d2");
        assert_eq!(r.entry.selected(), None);
    }
}
