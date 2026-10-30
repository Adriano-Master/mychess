//! Pure state machine of the board mouse interaction.
//!
//! Receives [`Input`]s in canvas coordinates and answers [`BoardEvent`]s. It
//! reads no clock, draws nothing and knows nothing about iced's widgets.
//! Spec rules 33-40 and 47. Scid: `pressSquare`/`releaseSquare`
//! (`tcl/main.tcl:1523-1599`), `addMarker`/`drawArrow` (`tcl/end.tcl`).

use iced::Point;
use mychess_core::{BoardMark, Move, Position, Role, Square};

use super::geometry::{BoardLayout, Orientation};
use super::marks::{MarkBrush, color_for_modifiers};
use super::moves::{Destination, DropResolution, is_own_piece, legal_destinations, resolve_drop};
use super::promotion::{promotion_cells, promotion_role_at};

/// Pointer travel (logical pixels) before a press becomes a drag.
pub const DRAG_THRESHOLD: f32 = 4.0;

/// Keyboard modifiers relevant to the board. `command` is `Ctrl` on Linux and
/// Windows and `Cmd` on macOS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    /// `Shift`.
    pub shift: bool,
    /// `Command()` of iced.
    pub command: bool,
    /// `Alt`.
    pub alt: bool,
}

/// An input for [`Interaction::handle`]; positions are canvas-local.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// Left button pressed.
    LeftPressed {
        /// Pointer position.
        pos: Point,
        /// Modifiers held.
        mods: Mods,
    },
    /// Left button released.
    LeftReleased {
        /// Pointer position.
        pos: Point,
        /// Modifiers held.
        mods: Mods,
    },
    /// Right button pressed.
    RightPressed {
        /// Pointer position.
        pos: Point,
        /// Modifiers held.
        mods: Mods,
    },
    /// Right button released.
    RightReleased {
        /// Pointer position.
        pos: Point,
        /// Modifiers held.
        mods: Mods,
    },
    /// The pointer moved.
    CursorMoved {
        /// Pointer position.
        pos: Point,
    },
    /// The pointer left the window.
    CursorLeft,
    /// `Escape`.
    Escape,
    /// `Enter`.
    Enter,
    /// The drawn position changed.
    PositionChanged,
    /// The board was flipped.
    OrientationChanged,
}

/// What [`Interaction::handle`] needs to know about the board.
#[derive(Debug, Clone, Copy)]
pub struct BoardContext<'a> {
    /// Where the board is.
    pub layout: BoardLayout,
    /// Who is at the bottom.
    pub orientation: Orientation,
    /// The drawn position.
    pub position: &'a Position,
    /// `false` disables moving and marking (future training and FICS modes).
    pub accepts_moves: bool,
    /// The brush of `Shift`+click and `Ctrl`+click marks.
    pub brush: &'a MarkBrush,
}

/// What the interaction asks of its owner.
#[derive(Debug, Clone, PartialEq)]
pub enum BoardEvent {
    /// A legal move was entered.
    PlayMove {
        /// The move.
        mv: Move,
        /// Whether the user dragged the piece (the move is then not animated).
        dragged: bool,
    },
    /// The user dropped a king on the other king.
    PlayNullMove,
    /// A mark gesture finished.
    ToggleMark(BoardMark),
    /// The user clicked the tip of an arrow of the engine (spec M4-5,
    /// D457); the squares and the line identify the arrow.
    PlayEngineArrow {
        /// Where the arrow starts.
        from: Square,
        /// Where it ends.
        to: Square,
        /// The MultiPV line of the arrow, from 1.
        rank: u8,
    },
    /// Only a redraw is needed.
    Redraw,
}

/// The mouse cursor to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorKind {
    /// The default arrow.
    Default,
    /// Over a piece that can be picked.
    Grab,
    /// Dragging a piece.
    Grabbing,
    /// Over a promotion choice.
    Pointer,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    square: Square,
    origin: Point,
    was_selected: bool,
}

/// A piece being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    /// Origin square.
    pub from: Square,
    /// Pointer position (the piece is drawn centered there).
    pub cursor: Point,
}

/// A promotion waiting for the user's choice.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingPromotion {
    /// Origin square.
    pub from: Square,
    /// Destination square.
    pub to: Square,
    /// The four moves, one per piece.
    pub candidates: Vec<Move>,
    /// Whether the pawn was dragged there.
    pub dragged: bool,
}

/// A right-button gesture in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkDrag {
    /// Square where the button went down.
    pub start: Square,
    /// Modifiers at that moment (they choose the color).
    pub mods: Mods,
    /// Square under the pointer now.
    pub current: Option<Square>,
}

/// The mouse state of the board.
#[derive(Debug, Clone, Default)]
pub struct Interaction {
    selected: Option<Square>,
    // Legal destinations of `selected`, computed when it is chosen so that
    // drawing a frame of a drag does not generate moves.
    destinations: Vec<Destination>,
    press: Option<Press>,
    drag: Option<Drag>,
    pending: Option<PendingPromotion>,
    mark_drag: Option<MarkDrag>,
    hover: Option<Square>,
    arrow_start: Option<Square>,
}

impl Interaction {
    /// The selected square.
    pub fn selected(&self) -> Option<Square> {
        self.selected
    }

    /// The legal destinations of the selected piece (empty when nothing is
    /// selected).
    pub fn destinations(&self) -> &[Destination] {
        if self.selected.is_some() {
            &self.destinations
        } else {
            &[]
        }
    }

    fn select(&mut self, from: Square, position: &Position) {
        if self.selected != Some(from) || self.destinations.is_empty() {
            self.destinations = legal_destinations(position, from);
        }
        self.selected = Some(from);
    }

    /// The piece being dragged.
    pub fn drag(&self) -> Option<Drag> {
        self.drag
    }

    /// The promotion waiting for a choice.
    pub fn pending_promotion(&self) -> Option<&PendingPromotion> {
        self.pending.as_ref()
    }

    /// The right-button gesture in progress.
    pub fn mark_drag(&self) -> Option<MarkDrag> {
        self.mark_drag
    }

    /// The square under the pointer.
    pub fn hover(&self) -> Option<Square> {
        self.hover
    }

    /// The origin of a two-click arrow.
    pub fn arrow_start(&self) -> Option<Square> {
        self.arrow_start
    }

    /// Whether nothing is selected, pressed or in progress.
    pub fn is_idle(&self) -> bool {
        self.selected.is_none()
            && self.press.is_none()
            && self.drag.is_none()
            && self.pending.is_none()
            && self.mark_drag.is_none()
            && self.arrow_start.is_none()
    }

    fn clear(&mut self) {
        *self = Interaction {
            hover: self.hover,
            ..Interaction::default()
        };
    }

    /// Applies one input.
    pub fn handle(&mut self, input: Input, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        match input {
            Input::PositionChanged | Input::OrientationChanged => self.reset(),
            _ if !ctx.accepts_moves => self.reset(),
            Input::LeftPressed { pos, mods } => self.left_pressed(pos, mods, ctx),
            Input::LeftReleased { pos, .. } => self.left_released(pos, ctx),
            Input::RightPressed { pos, mods } => self.right_pressed(pos, mods, ctx),
            Input::RightReleased { pos, .. } => self.right_released(pos, ctx),
            Input::CursorMoved { pos } => self.cursor_moved(pos, ctx),
            Input::CursorLeft => {
                self.hover = None;
                Vec::new()
            }
            Input::Escape => self.escape(),
            Input::Enter => self.enter(),
        }
    }

    fn reset(&mut self) -> Vec<BoardEvent> {
        if self.is_idle() {
            return Vec::new();
        }
        self.clear();
        vec![BoardEvent::Redraw]
    }

    fn left_pressed(&mut self, pos: Point, mods: Mods, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        if self.mark_drag.is_some() {
            return Vec::new();
        }
        // A second press during a drag means the release was lost (focus
        // change): drop the drag together with the selection it belongs to.
        if self.drag.take().is_some() {
            self.press = None;
            self.selected = None;
            self.destinations.clear();
            return vec![BoardEvent::Redraw];
        }
        if let Some(pending) = self.pending.take() {
            return self.press_in_promotion(&pending, pos, ctx);
        }
        let square = ctx.layout.square_at(pos, ctx.orientation);
        if mods.shift {
            return square
                .map(|sq| {
                    BoardEvent::ToggleMark(BoardMark::square(
                        ctx.brush.kind,
                        sq,
                        ctx.brush.color.clone(),
                    ))
                })
                .into_iter()
                .collect();
        }
        if mods.command {
            return self.arrow_click(square, ctx);
        }
        let had_arrow = self.arrow_start.take().is_some();
        let Some(target) = square else {
            let changed = self.selected.take().is_some() || had_arrow;
            self.press = None;
            return redraw_if(changed);
        };
        if let Some(from) = self.selected.filter(|&from| from != target) {
            self.press = None;
            match resolve_drop(ctx.position, from, target) {
                DropResolution::Illegal => {}
                resolution => {
                    return self.apply_resolution(ctx.position, resolution, from, target, false);
                }
            }
        }
        if is_own_piece(ctx.position, target) {
            self.press = Some(Press {
                square: target,
                origin: pos,
                was_selected: self.selected == Some(target),
            });
            self.select(target, ctx.position);
        } else {
            self.selected = None;
            self.press = None;
        }
        vec![BoardEvent::Redraw]
    }

    fn press_in_promotion(
        &mut self,
        pending: &PendingPromotion,
        pos: Point,
        ctx: &BoardContext<'_>,
    ) -> Vec<BoardEvent> {
        let cells = promotion_cells(&ctx.layout, pending.to, ctx.orientation);
        self.press = None;
        self.selected = None;
        if let Some(mv) = promotion_role_at(&cells, pos).and_then(|role| {
            pending
                .candidates
                .iter()
                .copied()
                .find(|m| m.promotion() == Some(role))
        }) {
            return vec![BoardEvent::PlayMove {
                mv,
                dragged: pending.dragged,
            }];
        }
        vec![BoardEvent::Redraw]
    }

    fn arrow_click(&mut self, square: Option<Square>, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        match (self.arrow_start.take(), square) {
            (None, Some(a)) => {
                self.arrow_start = Some(a);
                vec![BoardEvent::Redraw]
            }
            (Some(a), Some(b)) if a != b => vec![BoardEvent::ToggleMark(BoardMark::arrow(
                a,
                b,
                ctx.brush.color.clone(),
            ))],
            (Some(_), _) => vec![BoardEvent::Redraw],
            (None, None) => Vec::new(),
        }
    }

    fn apply_resolution(
        &mut self,
        position: &Position,
        resolution: DropResolution,
        from: Square,
        to: Square,
        dragged: bool,
    ) -> Vec<BoardEvent> {
        match resolution {
            DropResolution::Move(mv) => {
                self.selected = None;
                vec![BoardEvent::PlayMove { mv, dragged }]
            }
            DropResolution::NullMove => {
                self.selected = None;
                vec![BoardEvent::PlayNullMove]
            }
            DropResolution::NeedsPromotion { candidates } => {
                self.select(from, position);
                self.pending = Some(PendingPromotion {
                    from,
                    to,
                    candidates,
                    dragged,
                });
                vec![BoardEvent::Redraw]
            }
            DropResolution::Illegal => {
                self.selected = None;
                vec![BoardEvent::Redraw]
            }
        }
    }

    fn left_released(&mut self, pos: Point, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        let target = ctx.layout.square_at(pos, ctx.orientation);
        if let Some(drag) = self.drag.take() {
            self.press = None;
            return match target {
                None => {
                    self.selected = None;
                    vec![BoardEvent::Redraw]
                }
                Some(t) if t == drag.from => vec![BoardEvent::Redraw],
                Some(t) => {
                    let resolution = resolve_drop(ctx.position, drag.from, t);
                    self.apply_resolution(ctx.position, resolution, drag.from, t, true)
                }
            };
        }
        if let Some(press) = self.press.take()
            && press.was_selected
            && target == Some(press.square)
        {
            self.selected = None;
            return vec![BoardEvent::Redraw];
        }
        Vec::new()
    }

    fn right_pressed(&mut self, pos: Point, mods: Mods, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        if self.pending.is_some() || self.drag.is_some() || self.press.is_some() {
            return Vec::new();
        }
        match ctx.layout.square_at(pos, ctx.orientation) {
            Some(start) => {
                self.mark_drag = Some(MarkDrag {
                    start,
                    mods,
                    current: Some(start),
                });
                vec![BoardEvent::Redraw]
            }
            None => Vec::new(),
        }
    }

    fn right_released(&mut self, pos: Point, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        let Some(gesture) = self.mark_drag.take() else {
            return Vec::new();
        };
        let color = color_for_modifiers(gesture.mods);
        let mark = match ctx.layout.square_at(pos, ctx.orientation) {
            Some(t) if t == gesture.start => {
                Some(BoardMark::square(mychess_core::MarkKind::Circle, t, color))
            }
            Some(t) => Some(BoardMark::arrow(gesture.start, t, color)),
            None => None,
        };
        match mark {
            Some(mark) => vec![BoardEvent::ToggleMark(mark)],
            None => vec![BoardEvent::Redraw],
        }
    }

    fn cursor_moved(&mut self, pos: Point, ctx: &BoardContext<'_>) -> Vec<BoardEvent> {
        let square = ctx.layout.square_at(pos, ctx.orientation);
        self.hover = square;
        let mut redraw = self.pending.is_some() || self.arrow_start.is_some();
        if let Some(drag) = &mut self.drag {
            drag.cursor = pos;
            redraw = true;
        } else if let Some(press) = self.press {
            let (dx, dy) = (pos.x - press.origin.x, pos.y - press.origin.y);
            if dx.hypot(dy) > DRAG_THRESHOLD {
                self.drag = Some(Drag {
                    from: press.square,
                    cursor: pos,
                });
                redraw = true;
            }
        }
        if let Some(gesture) = &mut self.mark_drag {
            gesture.current = square;
            redraw = true;
        }
        redraw_if(redraw)
    }

    fn escape(&mut self) -> Vec<BoardEvent> {
        if self.pending.take().is_some() {
            self.drag = None;
            self.press = None;
        } else if self.drag.take().is_some() {
            self.press = None;
        } else {
            let marking = self.mark_drag.take().is_some() | self.arrow_start.take().is_some();
            let selection = !marking && self.selected.take().is_some();
            if selection {
                self.press = None;
            }
            if !marking && !selection {
                return Vec::new();
            }
        }
        vec![BoardEvent::Redraw]
    }

    fn enter(&mut self) -> Vec<BoardEvent> {
        let Some(pending) = self.pending.take() else {
            return Vec::new();
        };
        self.selected = None;
        self.press = None;
        pending
            .candidates
            .iter()
            .copied()
            .find(|m| m.promotion() == Some(Role::Queen))
            .map(|mv| BoardEvent::PlayMove {
                mv,
                dragged: pending.dragged,
            })
            .into_iter()
            .collect()
    }

    /// The cursor to show with the pointer at `pos` (`None`: outside).
    pub fn cursor_kind(&self, pos: Option<Point>, ctx: &BoardContext<'_>) -> CursorKind {
        if self.drag.is_some() {
            return CursorKind::Grabbing;
        }
        let Some(pos) = pos else {
            return CursorKind::Default;
        };
        if let Some(pending) = &self.pending {
            let cells = promotion_cells(&ctx.layout, pending.to, ctx.orientation);
            return if promotion_role_at(&cells, pos).is_some() {
                CursorKind::Pointer
            } else {
                CursorKind::Default
            };
        }
        match ctx.layout.square_at(pos, ctx.orientation) {
            Some(sq) if ctx.accepts_moves && is_own_piece(ctx.position, sq) => CursorKind::Grab,
            _ => CursorKind::Default,
        }
    }
}

fn redraw_if(condition: bool) -> Vec<BoardEvent> {
    if condition {
        vec![BoardEvent::Redraw]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use iced::Size;
    use mychess_core::MarkKind;

    use super::*;
    use crate::widgets::board::geometry::{CoordsMode, LayoutOptions};

    struct Rig {
        layout: BoardLayout,
        position: Position,
        brush: MarkBrush,
        orientation: Orientation,
        accepts: bool,
        it: Interaction,
    }

    fn sq(name: &str) -> Square {
        name.parse().expect("square")
    }

    impl Rig {
        fn new(fen: &str) -> Rig {
            Rig {
                layout: BoardLayout::compute(
                    Size::new(800.0, 800.0),
                    LayoutOptions {
                        coords: CoordsMode::None,
                    },
                )
                .expect("layout fits"),
                position: Position::from_fen(fen).expect("fen"),
                brush: MarkBrush::default(),
                orientation: Orientation::White,
                accepts: true,
                it: Interaction::default(),
            }
        }

        fn at(&self, name: &str) -> Point {
            self.layout.square_center(sq(name), self.orientation)
        }

        fn send(&mut self, input: Input) -> Vec<BoardEvent> {
            let ctx = BoardContext {
                layout: self.layout,
                orientation: self.orientation,
                position: &self.position,
                accepts_moves: self.accepts,
                brush: &self.brush,
            };
            self.it.handle(input, &ctx)
        }

        fn press(&mut self, name: &str) -> Vec<BoardEvent> {
            let pos = self.at(name);
            self.send(Input::LeftPressed {
                pos,
                mods: Mods::default(),
            })
        }

        fn release(&mut self, name: &str) -> Vec<BoardEvent> {
            let pos = self.at(name);
            self.send(Input::LeftReleased {
                pos,
                mods: Mods::default(),
            })
        }

        fn mv(&mut self, name: &str) -> Vec<BoardEvent> {
            let pos = self.at(name);
            self.send(Input::CursorMoved { pos })
        }

        fn click(&mut self, name: &str) -> Vec<BoardEvent> {
            let mut out = self.press(name);
            out.extend(self.release(name));
            out
        }

        fn play(events: &[BoardEvent]) -> Option<Move> {
            events.iter().find_map(|e| match e {
                BoardEvent::PlayMove { mv, .. } => Some(*mv),
                _ => None,
            })
        }
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const PROMO: &str = "1n2k3/P7/8/8/8/8/8/4K3 w - - 0 1";

    #[test]
    fn click_then_click_plays_the_move() {
        let mut r = Rig::new(START);
        r.click("e2");
        assert_eq!(r.it.selected(), Some(sq("e2")));
        let events = r.click("e4");
        let mv = Rig::play(&events).expect("move");
        assert_eq!((mv.from(), mv.to()), (Some(sq("e2")), Some(sq("e4"))));
        assert_eq!(r.it.selected(), None);
        assert!(matches!(
            events[0],
            BoardEvent::PlayMove { dragged: false, .. }
        ));
    }

    #[test]
    fn click_on_empty_or_enemy_square_clears_selection() {
        let mut r = Rig::new(START);
        r.click("e2");
        r.click("e5");
        assert_eq!(r.it.selected(), None);
        r.click("e2");
        r.click("e7");
        assert_eq!(r.it.selected(), None);
    }

    #[test]
    fn clicking_another_own_piece_reselects() {
        let mut r = Rig::new(START);
        r.click("e2");
        r.click("d2");
        assert_eq!(r.it.selected(), Some(sq("d2")));
    }

    #[test]
    fn second_click_on_the_same_piece_deselects() {
        let mut r = Rig::new(START);
        r.click("e2");
        r.click("e2");
        assert_eq!(r.it.selected(), None);
    }

    #[test]
    fn destination_before_origin_is_not_accepted() {
        let mut r = Rig::new(START);
        r.click("e4");
        let events = r.click("e2");
        assert!(Rig::play(&events).is_none());
        assert_eq!(r.it.selected(), Some(sq("e2")));
    }

    #[test]
    fn drag_to_a_legal_square_plays_a_dragged_move() {
        let mut r = Rig::new(START);
        r.press("g1");
        r.mv("g1");
        assert!(r.it.drag().is_none(), "no move yet, below threshold");
        r.mv("g2");
        assert!(r.it.drag().is_some());
        let events = r.release("f3");
        assert!(matches!(
            events.as_slice(),
            [BoardEvent::PlayMove { dragged: true, .. }]
        ));
    }

    #[test]
    fn movement_below_the_threshold_is_a_click() {
        let mut r = Rig::new(START);
        let p = r.at("e2");
        r.send(Input::LeftPressed {
            pos: p,
            mods: Mods::default(),
        });
        r.send(Input::CursorMoved {
            pos: Point::new(p.x + 3.0, p.y),
        });
        assert!(r.it.drag().is_none());
        r.send(Input::LeftReleased {
            pos: Point::new(p.x + 3.0, p.y),
            mods: Mods::default(),
        });
        assert_eq!(r.it.selected(), Some(sq("e2")));
    }

    #[test]
    fn dropping_outside_the_board_cancels_and_clears() {
        let mut r = Rig::new(START);
        r.press("e2");
        r.mv("e3");
        let events = r.send(Input::LeftReleased {
            pos: Point::new(-50.0, 400.0),
            mods: Mods::default(),
        });
        assert!(Rig::play(&events).is_none());
        assert_eq!(r.it.selected(), None);
        assert!(r.it.drag().is_none());
    }

    #[test]
    fn dropping_on_the_origin_keeps_the_selection() {
        let mut r = Rig::new(START);
        r.press("e2");
        r.mv("e3");
        r.release("e2");
        assert_eq!(r.it.selected(), Some(sq("e2")));
    }

    #[test]
    fn illegal_drop_cancels_and_clears() {
        let mut r = Rig::new(START);
        r.press("e2");
        r.mv("e5");
        let events = r.release("e5");
        assert!(Rig::play(&events).is_none());
        assert_eq!(r.it.selected(), None);
    }

    #[test]
    fn promotion_column_offers_each_piece() {
        for (role, name) in [
            (Role::Queen, "a8"),
            (Role::Rook, "a7"),
            (Role::Bishop, "a6"),
            (Role::Knight, "a5"),
        ] {
            let mut r = Rig::new(PROMO);
            r.click("a7");
            let events = r.click("a8");
            assert!(Rig::play(&events).is_none());
            assert!(r.it.pending_promotion().is_some());
            let events = r.press(name);
            let mv = Rig::play(&events).expect("promotion move");
            assert_eq!(mv.promotion(), Some(role));
        }
    }

    #[test]
    fn promotion_escape_cancels_and_enter_picks_the_queen() {
        let mut r = Rig::new(PROMO);
        r.click("a7");
        r.click("a8");
        r.send(Input::Escape);
        assert!(r.it.pending_promotion().is_none());
        assert_eq!(r.it.selected(), Some(sq("a7")));
        r.click("a8");
        let events = r.send(Input::Enter);
        assert_eq!(
            Rig::play(&events).and_then(Move::promotion),
            Some(Role::Queen)
        );
    }

    #[test]
    fn click_outside_the_promotion_column_cancels_it() {
        let mut r = Rig::new(PROMO);
        r.click("a7");
        r.click("a8");
        let events = r.press("e4");
        assert!(Rig::play(&events).is_none());
        assert!(r.it.pending_promotion().is_none());
    }

    #[test]
    fn dragged_promotion_marks_the_move_as_dragged() {
        let mut r = Rig::new(PROMO);
        r.press("a7");
        r.mv("a6");
        r.release("a8");
        assert!(r.it.pending_promotion().is_some_and(|p| p.dragged));
        let events = r.press("a5");
        assert!(matches!(
            events.as_slice(),
            [BoardEvent::PlayMove { dragged: true, .. }]
        ));
    }

    #[test]
    fn castling_by_king_destination_and_by_own_rook() {
        for target in ["g1", "h1"] {
            let mut r = Rig::new("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
            r.click("e1");
            let events = r.click(target);
            assert!(Rig::play(&events).is_some_and(Move::is_castle), "{target}");
        }
    }

    #[test]
    fn king_on_king_asks_for_the_null_move() {
        let mut r = Rig::new("8/8/8/3k4/8/8/8/K7 w - - 0 1");
        r.click("a1");
        let events = r.click("d5");
        assert_eq!(events, vec![BoardEvent::PlayNullMove]);
    }

    #[test]
    fn position_or_orientation_change_cancels_everything() {
        for input in [Input::PositionChanged, Input::OrientationChanged] {
            let mut r = Rig::new(PROMO);
            r.click("a7");
            r.click("a8");
            assert_eq!(r.send(input), vec![BoardEvent::Redraw]);
            assert!(r.it.is_idle());
        }
    }

    #[test]
    fn nothing_happens_when_moves_are_not_accepted() {
        let mut r = Rig::new(START);
        r.accepts = false;
        assert!(r.click("e2").is_empty());
        assert!(r.it.is_idle());
        let pos = r.at("e4");
        assert!(
            r.send(Input::RightPressed {
                pos,
                mods: Mods::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn right_click_on_a_square_toggles_a_green_circle() {
        let mut r = Rig::new(START);
        let p = r.at("d4");
        r.send(Input::RightPressed {
            pos: p,
            mods: Mods::default(),
        });
        let events = r.send(Input::RightReleased {
            pos: p,
            mods: Mods::default(),
        });
        assert_eq!(
            events,
            vec![BoardEvent::ToggleMark(BoardMark::square(
                MarkKind::Circle,
                sq("d4"),
                "green"
            ))]
        );
    }

    #[test]
    fn right_drag_makes_an_arrow_colored_by_modifier() {
        let cases = [
            (Mods::default(), "green"),
            (
                Mods {
                    shift: true,
                    ..Mods::default()
                },
                "firebrick",
            ),
            (
                Mods {
                    command: true,
                    ..Mods::default()
                },
                "yellow2",
            ),
            (
                Mods {
                    alt: true,
                    ..Mods::default()
                },
                "blue",
            ),
        ];
        for (mods, color) in cases {
            let mut r = Rig::new(START);
            let (a, b) = (r.at("d4"), r.at("f6"));
            r.send(Input::RightPressed { pos: a, mods });
            r.send(Input::CursorMoved { pos: b });
            assert_eq!(r.it.mark_drag().and_then(|m| m.current), Some(sq("f6")));
            let events = r.send(Input::RightReleased { pos: b, mods });
            assert_eq!(
                events,
                vec![BoardEvent::ToggleMark(BoardMark::arrow(
                    sq("d4"),
                    sq("f6"),
                    color
                ))]
            );
        }
    }

    #[test]
    fn right_release_outside_or_escape_marks_nothing() {
        let mut r = Rig::new(START);
        let a = r.at("d4");
        r.send(Input::RightPressed {
            pos: a,
            mods: Mods::default(),
        });
        let events = r.send(Input::RightReleased {
            pos: Point::new(-10.0, -10.0),
            mods: Mods::default(),
        });
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, BoardEvent::ToggleMark(_)))
        );
        r.send(Input::RightPressed {
            pos: a,
            mods: Mods::default(),
        });
        r.send(Input::Escape);
        assert!(r.it.mark_drag().is_none());
    }

    #[test]
    fn shift_click_paints_the_brush_without_selecting() {
        let mut r = Rig::new(START);
        let p = r.at("e2");
        let events = r.send(Input::LeftPressed {
            pos: p,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        });
        assert_eq!(
            events,
            vec![BoardEvent::ToggleMark(BoardMark::square(
                MarkKind::Circle,
                sq("e2"),
                "skyBlue"
            ))]
        );
        assert_eq!(r.it.selected(), None);
    }

    #[test]
    fn command_click_twice_draws_an_arrow_and_same_square_cancels() {
        let mods = Mods {
            command: true,
            ..Mods::default()
        };
        let mut r = Rig::new(START);
        let (a, b) = (r.at("e2"), r.at("e4"));
        r.send(Input::LeftPressed { pos: a, mods });
        assert_eq!(r.it.arrow_start(), Some(sq("e2")));
        let events = r.send(Input::LeftPressed { pos: b, mods });
        assert_eq!(
            events,
            vec![BoardEvent::ToggleMark(BoardMark::arrow(
                sq("e2"),
                sq("e4"),
                "skyBlue"
            ))]
        );
        r.send(Input::LeftPressed { pos: a, mods });
        r.send(Input::LeftPressed { pos: a, mods });
        assert_eq!(r.it.arrow_start(), None);
    }

    #[test]
    fn drag_has_priority_over_right_button_gestures() {
        let mut r = Rig::new(START);
        r.press("e2");
        r.mv("e3");
        let p = r.at("d4");
        assert!(
            r.send(Input::RightPressed {
                pos: p,
                mods: Mods::default()
            })
            .is_empty()
        );
        assert!(r.it.mark_drag().is_none());
    }

    #[test]
    fn escape_cancels_one_layer_at_a_time() {
        let mut r = Rig::new(START);
        r.press("e2");
        r.mv("e3");
        assert!(r.it.drag().is_some());
        r.send(Input::Escape);
        assert!(r.it.drag().is_none());
        assert!(r.it.selected().is_some());
        r.send(Input::Escape);
        assert!(r.it.selected().is_none());
        assert!(r.send(Input::Escape).is_empty());
    }

    #[test]
    fn cursor_kind_follows_the_state() {
        let mut r = Rig::new(START);
        let own = r.at("e2");
        let ctx_kind = |r: &Rig, p: Option<Point>| {
            let ctx = BoardContext {
                layout: r.layout,
                orientation: r.orientation,
                position: &r.position,
                accepts_moves: r.accepts,
                brush: &r.brush,
            };
            r.it.cursor_kind(p, &ctx)
        };
        assert_eq!(ctx_kind(&r, Some(own)), CursorKind::Grab);
        assert_eq!(ctx_kind(&r, Some(r.at("e7"))), CursorKind::Default);
        r.press("e2");
        r.mv("e3");
        assert_eq!(ctx_kind(&r, Some(own)), CursorKind::Grabbing);
    }

    #[test]
    fn works_with_the_board_flipped() {
        let mut r = Rig::new(START);
        r.orientation = Orientation::Black;
        r.click("e2");
        let events = r.click("e4");
        assert!(Rig::play(&events).is_some());
    }

    #[test]
    fn destinations_are_stored_on_selection_and_forgotten_on_deselection() {
        let mut rig = Rig::new("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert!(rig.it.destinations().is_empty());
        rig.click("e2");
        let squares: Vec<Square> = rig.it.destinations().iter().map(|d| d.square).collect();
        assert_eq!(squares.len(), 2);
        assert!(squares.contains(&sq("e3")) && squares.contains(&sq("e4")));
        rig.click("e2");
        assert!(rig.it.destinations().is_empty(), "deselected");
        rig.click("g1");
        assert_eq!(
            rig.it.destinations().len(),
            2,
            "recomputed for the new piece"
        );
    }

    #[test]
    fn destinations_survive_a_pending_promotion() {
        let mut rig = Rig::new("8/P7/8/8/8/8/8/k6K w - - 0 1");
        rig.click("a7");
        rig.click("a8");
        assert!(rig.it.pending_promotion().is_some());
        assert_eq!(rig.it.destinations().len(), 1);
    }
}
