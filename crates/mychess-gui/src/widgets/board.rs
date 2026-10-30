//! The chessboard widget and its pure parts.
//!
//! [`BoardWidget`] is an iced `canvas::Program`: it translates events into
//! [`Input`]s for the pure [`Interaction`] and draws five layers (see
//! `layers`), three of them cached in [`BoardCaches`]. The player strips, the
//! material and the side-to-move marker are not part of it: they are iced
//! widgets around the canvas (`windows::board_column`, spec M2-5, D230).

pub mod animation;
pub mod geometry;
pub mod interaction;
mod layers;
pub mod marks;
pub mod moves;
pub mod pieces;
pub mod premove;
pub mod promotion;

use std::cell::Cell;

use iced::keyboard::{self, Key, key::Named};
use iced::widget::canvas::{self, Action, Cache, Event, Frame};
use iced::{Point, Rectangle, mouse};
use mychess_app::{ArrowKind, EngineArrow};
use mychess_core::{BoardMark, Move, Position, Square, ZobristHash};

use self::animation::MoveAnimation;
use self::geometry::{BoardLayout, CoordsMode, LayoutOptions, Orientation};
use self::interaction::{BoardContext, BoardEvent, CursorKind, Input, Interaction, Mods};
use self::layers::Scene;
use self::marks::MarkBrush;
use self::pieces::{PieceSet, PieceSetId};
use self::premove::{PremoveBoardEvent, PremoveLayer, PremoveSetup};
use crate::theme::board_theme::{BoardTheme, BoardThemeId};

/// Display preferences of the board.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardOptions {
    /// Color preset.
    pub theme: BoardThemeId,
    /// Coordinate labels.
    pub coords: CoordsMode,
    /// Whether to tint the last move.
    pub highlight_last_move: bool,
    /// Whether to show dots and rings on legal destinations.
    pub show_legal_hints: bool,
    /// Whether to draw the marks of the comment.
    pub show_marks: bool,
}

impl Default for BoardOptions {
    fn default() -> BoardOptions {
        BoardOptions {
            theme: BoardThemeId::default(),
            coords: CoordsMode::default(),
            highlight_last_move: true,
            show_legal_hints: true,
            show_marks: true,
        }
    }
}

impl BoardOptions {
    /// The layout options (only the coordinates take room).
    pub fn layout_options(self) -> LayoutOptions {
        LayoutOptions {
            coords: self.coords,
        }
    }
}

/// What the widget draws and reads, borrowed from the application state.
#[derive(Clone, Copy)]
pub struct BoardData<'a> {
    /// The drawn position.
    pub position: &'a Position,
    /// The move that led to it.
    pub last_move: Option<Move>,
    /// Marks of the current comment.
    pub marks: &'a [BoardMark],
    /// Who is at the bottom.
    pub orientation: Orientation,
    /// Display preferences.
    pub options: BoardOptions,
    /// Whether moves and marks can be entered.
    pub accepts_moves: bool,
    /// The brush for `Shift`/`Ctrl` clicks.
    pub brush: &'a MarkBrush,
    /// The piece images.
    pub pieces: &'a PieceSet,
    /// The running animation and its eased progress.
    pub animation: Option<(&'a MoveAnimation, f32)>,
}

impl BoardData<'_> {
    fn hidden(&self) -> &[Square] {
        self.animation.map_or(&[], |(a, _)| a.hidden.as_slice())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CommonSig {
    orientation: Orientation,
    layout: LayoutOptions,
    set: PieceSetId,
}

#[derive(Debug, Clone, PartialEq)]
struct SquaresSig {
    common: CommonSig,
    theme: BoardThemeId,
    last_move: Option<Move>,
    full_marks: Vec<(Square, String)>,
}

#[derive(Debug, Clone, PartialEq)]
struct PiecesSig {
    common: CommonSig,
    position: ZobristHash,
    hidden: Vec<Square>,
}

#[derive(Debug, Clone, PartialEq)]
struct MarksSig {
    common: CommonSig,
    position: ZobristHash,
    marks: Vec<BoardMark>,
    // The arrow pass redraws the piece of its origin unless an animation
    // hides that square, so the layer changes when the animation ends.
    hidden: Vec<Square>,
}

/// The class of the move under the cursor, ready to draw: where, with which
/// glyph and in which colour (spec M4-10, D1434). The colour is resolved by
/// the caller from the theme, so a change of theme is a change of badge and
/// the cached layer is redrawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassBadge {
    /// The destination square of the move.
    pub square: Square,
    /// The short symbol of the class (plain ASCII).
    pub glyph: &'static str,
    /// The colour of the class.
    pub color: iced::Color,
}

/// What each cached layer was built from; compared with `==` to decide
/// which caches to clear.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardSignatures {
    squares: SquaresSig,
    pieces: PiecesSig,
    marks: MarksSig,
}

/// Which cached layers [`BoardCaches::sync_data`] found out of date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dirty {
    /// The squares layer.
    pub squares: bool,
    /// The pieces layer.
    pub pieces: bool,
    /// The marks layer.
    pub marks: bool,
}

/// Assigns `value` to `slot` when they differ; reports whether it did.
fn assign<T: PartialEq + Clone>(slot: &mut T, value: &T) -> bool {
    if slot == value {
        false
    } else {
        slot.clone_from(value);
        true
    }
}

impl BoardSignatures {
    /// The signatures of the data to draw.
    pub fn of(data: &BoardData<'_>) -> BoardSignatures {
        let common = CommonSig {
            orientation: data.orientation,
            layout: data.options.layout_options(),
            set: data.pieces.id(),
        };
        let marks = if data.options.show_marks {
            data.marks
        } else {
            &[]
        };
        let hash = data.position.zobrist();
        BoardSignatures {
            squares: SquaresSig {
                common: common.clone(),
                theme: data.options.theme,
                last_move: data
                    .options
                    .highlight_last_move
                    .then_some(data.last_move)
                    .flatten(),
                full_marks: marks
                    .iter()
                    .filter(|m| m.kind == mychess_core::MarkKind::Full)
                    .map(|m| (m.from, m.color.clone()))
                    .collect(),
            },
            pieces: PiecesSig {
                common: common.clone(),
                position: hash,
                hidden: data.hidden().to_vec(),
            },
            marks: MarksSig {
                common,
                position: hash,
                marks: marks.to_vec(),
                hidden: data.hidden().to_vec(),
            },
        }
    }

    /// Brings the signatures up to date with `data`, reusing their
    /// allocations (nothing is allocated when nothing changed). The result
    /// equals `BoardSignatures::of(data)`.
    fn refresh(&mut self, data: &BoardData<'_>) -> Dirty {
        let common = CommonSig {
            orientation: data.orientation,
            layout: data.options.layout_options(),
            set: data.pieces.id(),
        };
        let marks = if data.options.show_marks {
            data.marks
        } else {
            &[]
        };
        let hash = data.position.zobrist();
        let last_move = data
            .options
            .highlight_last_move
            .then_some(data.last_move)
            .flatten();
        let full_marks_same = self
            .squares
            .full_marks
            .iter()
            .map(|(s, c)| (*s, c.as_str()))
            .eq(marks
                .iter()
                .filter(|m| m.kind == mychess_core::MarkKind::Full)
                .map(|m| (m.from, m.color.as_str())));

        let sq = &mut self.squares;
        let mut squares = assign(&mut sq.common, &common);
        squares |= assign(&mut sq.theme, &data.options.theme);
        squares |= assign(&mut sq.last_move, &last_move);
        if !full_marks_same {
            sq.full_marks = marks
                .iter()
                .filter(|m| m.kind == mychess_core::MarkKind::Full)
                .map(|m| (m.from, m.color.clone()))
                .collect();
            squares = true;
        }

        let pc = &mut self.pieces;
        let mut pieces = assign(&mut pc.common, &common);
        pieces |= assign(&mut pc.position, &hash);
        if pc.hidden.as_slice() != data.hidden() {
            pc.hidden = data.hidden().to_vec();
            pieces = true;
        }

        let mk = &mut self.marks;
        let mut marks_dirty = assign(&mut mk.common, &common);
        marks_dirty |= assign(&mut mk.position, &hash);
        if mk.marks.as_slice() != marks {
            mk.marks = marks.to_vec();
            marks_dirty = true;
        }
        if mk.hidden.as_slice() != data.hidden() {
            mk.hidden = data.hidden().to_vec();
            marks_dirty = true;
        }
        Dirty {
            squares,
            pieces,
            marks: marks_dirty,
        }
    }
}

/// The cached layers and the signatures they were built from.
#[derive(Debug, Default)]
pub struct BoardCaches {
    squares: Cache,
    pieces: Cache,
    marks: Cache,
    seen: Option<BoardSignatures>,
    arrows: Cache,
    arrows_seen: Option<(Vec<EngineArrow>, Orientation)>,
    badge_seen: Option<ClassBadge>,
    /// The orientation the badge was drawn with (D2193).
    badge_orientation: Option<Orientation>,
    /// How many times the arrows layer was cleared (to test the keys).
    arrow_clears: u32,
    /// Set by the board canvas while the arrows must not be drawn (a piece
    /// slides or is dragged); read by the arrows canvas above it.
    arrows_hidden: Cell<bool>,
}

impl BoardCaches {
    /// Clears the layer of the engine arrows when the list or the
    /// orientation changed (spec M4-5, D459); reports whether it did.
    pub fn sync_arrows(&mut self, arrows: &[EngineArrow], orientation: Orientation) -> bool {
        let same = self
            .arrows_seen
            .as_ref()
            .is_some_and(|(list, o)| list.as_slice() == arrows && *o == orientation);
        if same {
            return false;
        }
        self.clear_arrows();
        self.arrows_seen = Some((arrows.to_vec(), orientation));
        true
    }

    /// Clears the layer of the arrows (where the class badge is drawn) when
    /// the badge or the orientation changed; reports whether it did. It does
    /// not depend on `sync_arrows` running first (D2193).
    pub fn sync_badge(&mut self, badge: Option<ClassBadge>, orientation: Orientation) -> bool {
        // Without a badge the orientation draws nothing.
        let same = self.badge_seen == badge
            && (badge.is_none() || self.badge_orientation == Some(orientation));
        if same {
            return false;
        }
        self.clear_arrows();
        self.badge_seen = badge;
        self.badge_orientation = Some(orientation);
        true
    }

    fn clear_arrows(&mut self) {
        self.arrows.clear();
        self.arrow_clears = self.arrow_clears.wrapping_add(1);
    }

    /// How many times the layer of the arrows was cleared.
    #[cfg(test)]
    pub(crate) fn arrow_clears(&self) -> u32 {
        self.arrow_clears
    }

    /// Clears the caches whose signature changed.
    pub fn sync(&mut self, now: BoardSignatures) {
        let changed = |same: fn(&BoardSignatures, &BoardSignatures) -> bool| {
            self.seen.as_ref().is_none_or(|before| !same(before, &now))
        };
        if changed(|a, b| a.squares == b.squares) {
            self.squares.clear();
            self.arrows.clear();
            self.arrow_clears = self.arrow_clears.wrapping_add(1);
        }
        if changed(|a, b| a.pieces == b.pieces) {
            self.pieces.clear();
        }
        if changed(|a, b| a.marks == b.marks) {
            self.marks.clear();
        }
        self.seen = Some(now);
    }

    /// Like [`sync`](Self::sync) but compares against `data` in place, so
    /// nothing is cloned or allocated when the board did not change.
    pub fn sync_data(&mut self, data: &BoardData<'_>) -> Dirty {
        let dirty = if let Some(seen) = &mut self.seen {
            seen.refresh(data)
        } else {
            self.seen = Some(BoardSignatures::of(data));
            Dirty {
                squares: true,
                pieces: true,
                marks: true,
            }
        };
        if dirty.squares {
            self.squares.clear();
            // The arrows depend on the layout (coordinates), the
            // orientation and the size of the squares, all in this layer's
            // signature.
            self.clear_arrows();
        }
        if dirty.pieces {
            self.pieces.clear();
        }
        if dirty.marks {
            self.marks.clear();
        }
        dirty
    }
}

/// What the canvas remembers between events.
#[derive(Debug, Default)]
pub struct State {
    interaction: Interaction,
    mods: Mods,
    seen: Option<(ZobristHash, u32, Orientation)>,
    /// The engine arrow whose tip took the left press (its release decides).
    arrow_press: Option<(Square, Square, u8)>,
}

impl State {
    /// The interaction, unless it predates the drawn position or
    /// orientation (then it is stale and must be ignored).
    fn live(&self, identity: (ZobristHash, u32, Orientation)) -> Option<&Interaction> {
        (self.seen == Some(identity)).then_some(&self.interaction)
    }
}

/// The board as an iced canvas program publishing [`BoardEvent`]s.
#[derive(Clone)]
pub struct BoardWidget<'a> {
    data: BoardData<'a>,
    caches: &'a BoardCaches,
    // Computed once per `view`: events (every pointer move) reuse it.
    identity: (ZobristHash, u32, Orientation),
    arrows: &'a [EngineArrow],
    badge: Option<ClassBadge>,
    click_to_play: bool,
    premove: Option<PremoveSetup<'a>>,
}

impl<'a> BoardWidget<'a> {
    /// A widget over `data`, drawing through `caches`.
    pub fn new(data: BoardData<'a>, caches: &'a BoardCaches) -> BoardWidget<'a> {
        let identity = (
            data.position.zobrist(),
            data.position.ply(),
            data.orientation,
        );
        BoardWidget {
            data,
            caches,
            identity,
            arrows: &[],
            badge: None,
            click_to_play: false,
            premove: None,
        }
    }

    /// Adds the badge of the class of the move (drawn with the arrows, above
    /// the pieces; it hides while a piece slides or is dragged). The caller
    /// keeps the cache up to date with [`BoardCaches::sync_badge`].
    #[must_use]
    pub fn with_class_badge(mut self, badge: Option<ClassBadge>) -> BoardWidget<'a> {
        self.badge = badge;
        self
    }

    /// Adds the layer of the engine arrows (between the marks and the
    /// overlay); with `click_to_play` a click on a tip publishes
    /// [`BoardEvent::PlayEngineArrow`]. The caller keeps the layer's cache
    /// up to date with [`BoardCaches::sync_arrows`].
    #[must_use]
    pub fn with_engine_arrows(
        mut self,
        arrows: &'a [EngineArrow],
        click_to_play: bool,
    ) -> BoardWidget<'a> {
        self.arrows = arrows;
        self.click_to_play = click_to_play;
        self
    }

    /// Lets the board take a premove (spec M8-3, D1321) when it is not the
    /// user's turn. Build the element with
    /// [`into_premove_element`](Self::into_premove_element) afterwards.
    ///
    /// The layer consumes `Esc` whenever it cancels a selection or a
    /// premove, whatever has the focus. A caller that opens a modal layer
    /// (dialog, menu) must stop passing the setup while it is open (see
    /// `modal_layer_open()`), or `Esc` meant for the modal would cancel the
    /// premove instead.
    ///
    /// How the online game panel uses it:
    /// - while the opponent is to move, pass `Some(PremoveSetup)` with the
    ///   user's colour, the premove kept in `mychess_app` (as squares) and a
    ///   predicate bound to the shown position, for example
    ///   `|f, t| is_pseudo_legal(&position, color, f, t)`; when it is the
    ///   user's turn, or the preference `premove` is off, do not call this
    ///   and use [`into_element`](Self::into_element) as always;
    /// - on [`PremoveEvent::Set`](premove::PremoveEvent::Set) build the
    ///   premove with `Premove::enter` (promotion to a queen by default) and
    ///   keep it in the `PremoveSlot`; on `Cancel` empty the slot (and send
    ///   `OnlineMessage::CancelPremove`);
    /// - when the opponent's move arrives, call `take_if_legal` on the
    ///   resulting position and send the move, or drop it silently.
    #[must_use]
    pub fn with_premove(mut self, setup: PremoveSetup<'a>) -> BoardWidget<'a> {
        self.premove = Some(setup);
        self
    }

    /// Whether [`with_premove`](Self::with_premove) was called.
    pub fn has_premove(&self) -> bool {
        self.premove.is_some()
    }

    /// Like [`into_element`](Self::into_element), but publishing
    /// [`PremoveBoardEvent`]s. Without [`with_premove`](Self::with_premove)
    /// it only wraps the board events. With it, the board itself takes no
    /// moves or marks (`accepts_moves` is forced off): a layer on top takes
    /// the pointer and `Esc`, enters and draws the premove, and a right
    /// click cancels it.
    pub fn into_premove_element(mut self) -> iced::Element<'a, PremoveBoardEvent> {
        let Some(setup) = self.premove else {
            return self.into_element().map(PremoveBoardEvent::Board);
        };
        self.data.accepts_moves = false;
        let layer = PremoveLayer {
            data: self.data,
            setup,
            pieces: self.data.pieces,
        };
        iced::widget::stack![
            self.into_element().map(PremoveBoardEvent::Board),
            canvas::Canvas::new(layer)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
        ]
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
    }

    /// The arrows are hidden while a piece slides or is dragged (spec rule
    /// 10).
    fn arrows_visible(&self, interaction: &Interaction) -> bool {
        (!self.arrows.is_empty() || self.badge.is_some())
            && self.data.animation.is_none()
            && interaction.drag().is_none()
    }

    /// The clickable arrow whose tip is under `p`, the one painted on top
    /// first.
    fn arrow_at(&self, layout: &BoardLayout, p: Point) -> Option<(Square, Square, u8)> {
        let order: Vec<&EngineArrow> = layers::arrow_paint_order(self.arrows).collect();
        order.into_iter().rev().find_map(|a| {
            // Only the move to play now is playable (D826).
            if matches!(a.kind, ArrowKind::Sequence(ply) if ply > 0)
                || a.kind == ArrowKind::ReviewBest
            {
                return None;
            }
            let shape = marks::engine_arrow_geometry(
                layout,
                a.from,
                a.to,
                self.data.orientation,
                layers::shaft_of(a.kind).unwrap_or(0.14),
            )?;
            (shape.head_distance(p) <= marks::HEAD_HIT_MARGIN).then_some((a.from, a.to, a.rank))
        })
    }

    /// The widget as an element filling its container.
    ///
    /// Three canvases in a `stack`: the squares layer (with the coordinate
    /// labels) below, the pieces, marks and overlay (and all the input) in
    /// the middle, and the engine arrows on top. A renderer layer draws all
    /// its images before its meshes and its text, so labels sharing a layer
    /// with the piece SVGs would be painted over the pieces, and arrows
    /// would end up under them; the stack gives each its own layer.
    pub fn into_element(self) -> iced::Element<'a, BoardEvent> {
        let backdrop = Backdrop {
            data: self.data,
            caches: self.caches,
        };
        let arrows = ArrowsLayer {
            data: self.data,
            caches: self.caches,
            arrows: self.arrows,
            badge: self.badge,
        };
        iced::widget::stack![
            canvas::Canvas::new(backdrop)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
            canvas(self)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
            canvas::Canvas::new(arrows)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
        ]
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
    }

    fn layout(&self, bounds: Rectangle) -> Option<BoardLayout> {
        BoardLayout::compute(bounds.size(), self.data.options.layout_options())
    }

    fn identity(&self) -> (ZobristHash, u32, Orientation) {
        self.identity
    }

    fn context(&self, layout: BoardLayout) -> BoardContext<'_> {
        BoardContext {
            layout,
            orientation: self.data.orientation,
            position: self.data.position,
            accepts_moves: self.data.accepts_moves,
            brush: self.data.brush,
        }
    }

    fn scene(&self, layout: BoardLayout) -> Scene<'_> {
        self.data.scene(layout)
    }
}

impl BoardData<'_> {
    fn scene(&self, layout: BoardLayout) -> Scene<'_> {
        Scene {
            layout,
            o: self.orientation,
            theme: BoardTheme::of(self.options.theme),
            opts: self.options,
            position: self.position,
            last_move: self.last_move,
            marks: self.marks,
            pieces: self.pieces,
            hidden: self.hidden(),
            animation: self.animation,
        }
    }
}

/// The upper canvas: the engine arrows, above the pieces. It takes no
/// input; the middle canvas decides whether they show (`arrows_hidden`).
struct ArrowsLayer<'a> {
    data: BoardData<'a>,
    caches: &'a BoardCaches,
    arrows: &'a [EngineArrow],
    badge: Option<ClassBadge>,
}

impl canvas::Program<BoardEvent> for ArrowsLayer<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        if (self.arrows.is_empty() && self.badge.is_none())
            || self.data.animation.is_some()
            || self.caches.arrows_hidden.get()
        {
            return Vec::new();
        }
        let Some(layout) = BoardLayout::compute(bounds.size(), self.data.options.layout_options())
        else {
            return Vec::new();
        };
        vec![self.caches.arrows.draw(renderer, bounds.size(), |frame| {
            layers::draw_engine_arrows(frame, &layout, self.data.orientation, self.arrows);
            if let Some(badge) = &self.badge {
                layers::draw_class_badge(frame, &layout, self.data.orientation, badge);
            }
        })]
    }
}

/// The lower canvas: the cached squares layer, frame and labels. It
/// takes no input (the upper canvas does).
struct Backdrop<'a> {
    data: BoardData<'a>,
    caches: &'a BoardCaches,
}

impl canvas::Program<BoardEvent> for Backdrop<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(layout) = BoardLayout::compute(bounds.size(), self.data.options.layout_options())
        else {
            return Vec::new();
        };
        let scene = self.data.scene(layout);
        vec![self.caches.squares.draw(renderer, bounds.size(), |frame| {
            layers::draw_squares(frame, &scene);
        })]
    }
}

fn canvas(program: BoardWidget<'_>) -> canvas::Canvas<BoardWidget<'_>, BoardEvent> {
    canvas::Canvas::new(program)
}

fn mods_from(m: keyboard::Modifiers) -> Mods {
    Mods {
        shift: m.shift(),
        command: m.command(),
        alt: m.alt(),
    }
}

/// Translates an iced event into an [`Input`]; `local` is the pointer in
/// canvas coordinates (NaN when the pointer is not available).
fn translate(event: &Event, local: Option<Point>, inside: bool, mods: Mods) -> Option<Input> {
    let pos = local.unwrap_or(Point::new(f32::NAN, f32::NAN));
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
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)) => {
            Some(Input::RightReleased { pos, mods })
        }
        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
            local.map(|pos| Input::CursorMoved { pos })
        }
        Event::Mouse(mouse::Event::CursorLeft) => Some(Input::CursorLeft),
        Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key {
            Key::Named(Named::Escape) => Some(Input::Escape),
            Key::Named(Named::Enter) => Some(Input::Enter),
            _ => None,
        },
        _ => None,
    }
}

impl BoardWidget<'_> {
    /// Handles the left click on the tip of an engine arrow: the press is
    /// taken (the board must not start a piece gesture) and the release
    /// over the same tip publishes the move. `Alt`, `Shift` and `Ctrl`
    /// clicks go to the marks and the pieces (D457).
    fn arrow_click(
        &self,
        state: &mut State,
        event: &Event,
        layout: BoardLayout,
        local: Option<Point>,
        inside: bool,
    ) -> Option<Action<BoardEvent>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if inside
                    && self.click_to_play
                    && self.data.accepts_moves
                    && !(state.mods.alt || state.mods.shift || state.mods.command)
                    && state.interaction.is_idle()
                    && self.arrows_visible(&state.interaction) =>
            {
                let hit = self.arrow_at(&layout, local?)?;
                state.arrow_press = Some(hit);
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let pressed = state.arrow_press.take()?;
                let still_there = local.and_then(|p| self.arrow_at(&layout, p)) == Some(pressed);
                if still_there && self.arrows_visible(&state.interaction) {
                    let (from, to, rank) = pressed;
                    Some(
                        Action::publish(BoardEvent::PlayEngineArrow { from, to, rank })
                            .and_capture(),
                    )
                } else {
                    Some(Action::capture())
                }
            }
            _ => None,
        }
    }
}

impl canvas::Program<BoardEvent> for BoardWidget<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<BoardEvent>> {
        // Before the layout check: a board too small to draw still has to
        // know which modifiers are held when it grows again.
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(m)) = event {
            state.mods = mods_from(*m);
        }
        let layout = self.layout(bounds)?;
        let ctx = self.context(layout);
        let mut events = Vec::new();
        let identity = self.identity();
        if let Some((hash, ply, orientation)) = state.seen {
            if hash != identity.0 || ply != identity.1 {
                state.arrow_press = None;
                events.extend(state.interaction.handle(Input::PositionChanged, &ctx));
            } else if orientation != identity.2 {
                state.arrow_press = None;
                events.extend(state.interaction.handle(Input::OrientationChanged, &ctx));
            }
        }
        state.seen = Some(identity);

        let local = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        let inside = cursor.position_in(bounds).is_some();
        if let Some(action) = self.arrow_click(state, event, layout, local, inside) {
            return Some(action);
        }
        if let Some(input) = translate(event, local, inside, state.mods) {
            events.extend(state.interaction.handle(input, &ctx));
        }
        let published = events.iter().find(|e| !matches!(e, BoardEvent::Redraw));
        if let Some(event) = published {
            return Some(Action::publish(event.clone()));
        }
        (!events.is_empty()).then(Action::request_redraw)
    }

    fn draw(
        &self,
        state: &State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some(layout) = self.layout(bounds) else {
            return Vec::new();
        };
        let scene = self.scene(layout);
        let size = bounds.size();
        let pieces = self
            .caches
            .pieces
            .draw(renderer, size, |frame| layers::draw_pieces(frame, &scene));
        let marks = self
            .caches
            .marks
            .draw(renderer, size, |frame| layers::draw_marks(frame, &scene));
        // Interaction state that predates the drawn position is stale.
        let stale = Interaction::default();
        let interaction = state.live(self.identity()).unwrap_or(&stale);
        self.caches
            .arrows_hidden
            .set(!self.arrows_visible(interaction));
        let mut under = Frame::new(renderer, size);
        layers::draw_under(&mut under, &scene, interaction);
        let mut overlay = Frame::new(renderer, size);
        let local = cursor.position_in(bounds);
        layers::draw_overlay(&mut overlay, &scene, interaction, local);
        vec![
            under.into_geometry(),
            pieces,
            marks,
            overlay.into_geometry(),
        ]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(layout) = self.layout(bounds) else {
            return mouse::Interaction::default();
        };
        let ctx = self.context(layout);
        let stale = Interaction::default();
        let interaction = state.live(self.identity()).unwrap_or(&stale);
        let local = cursor.position_in(bounds);
        let on_tip = self.click_to_play
            && self.data.accepts_moves
            && interaction.is_idle()
            && self.arrows_visible(interaction)
            && local.is_some_and(|p| self.arrow_at(&layout, p).is_some());
        if on_tip {
            return mouse::Interaction::Pointer;
        }
        match interaction.cursor_kind(local, &ctx) {
            CursorKind::Default => mouse::Interaction::default(),
            CursorKind::Grab => mouse::Interaction::Grab,
            CursorKind::Grabbing => mouse::Interaction::Grabbing,
            CursorKind::Pointer => mouse::Interaction::Pointer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::animation::animation_between;
    use super::*;

    fn data<'a>(
        position: &'a Position,
        brush: &'a MarkBrush,
        pieces: &'a PieceSet,
        animation: Option<&'a MoveAnimation>,
    ) -> BoardData<'a> {
        BoardData {
            position,
            last_move: None,
            marks: &[],
            orientation: Orientation::White,
            options: BoardOptions::default(),
            accepts_moves: true,
            brush,
            pieces,
            animation: animation.map(|a| (a, 0.5)),
        }
    }

    fn after_e4() -> (Position, Position, MoveAnimation) {
        let old = Position::new();
        let mut new = old.clone();
        let mv = old
            .find_move("e2".parse().expect("sq"), "e4".parse().expect("sq"), None)
            .expect("legal");
        new.play(mv).expect("plays");
        let animation = animation_between(&old, &new).expect("one move apart");
        (old, new, animation)
    }

    #[test]
    fn marks_signature_changes_when_the_animation_ends() {
        let (_, new, animation) = after_e4();
        let (brush, pieces) = (MarkBrush::default(), PieceSet::default());
        let running = BoardSignatures::of(&data(&new, &brush, &pieces, Some(&animation)));
        let done = BoardSignatures::of(&data(&new, &brush, &pieces, None));
        assert_ne!(running.marks, done.marks);
    }

    #[test]
    fn sync_data_marks_the_marks_layer_dirty_when_the_animation_ends() {
        let (_, new, animation) = after_e4();
        let (brush, pieces) = (MarkBrush::default(), PieceSet::default());
        let mut caches = BoardCaches::default();
        caches.sync_data(&data(&new, &brush, &pieces, Some(&animation)));
        let same = caches.sync_data(&data(&new, &brush, &pieces, Some(&animation)));
        assert_eq!(same, Dirty::default(), "a frame changes no signature");
        let end = caches.sync_data(&data(&new, &brush, &pieces, None));
        assert!(end.marks && end.pieces);
        assert!(
            !end.squares,
            "the squares no longer depend on the animation (the side-to-move marker left the canvas)"
        );
    }

    #[test]
    fn refresh_agrees_with_building_the_signatures_again() {
        let (old, new, animation) = after_e4();
        let (brush, pieces) = (MarkBrush::default(), PieceSet::default());
        let mut sigs = BoardSignatures::of(&data(&old, &brush, &pieces, None));
        let now = data(&new, &brush, &pieces, Some(&animation));
        let dirty = sigs.refresh(&now);
        assert_eq!(sigs, BoardSignatures::of(&now));
        assert!(dirty.pieces);
        assert_eq!(sigs.refresh(&now), Dirty::default());
    }

    fn arrow(from: &str, to: &str, rank: u8, kind: ArrowKind) -> EngineArrow {
        EngineArrow {
            from: from.parse().expect("sq"),
            to: to.parse().expect("sq"),
            rank,
            kind,
            side: mychess_core::Color::White,
            label: None,
        }
    }

    const BOUNDS: Rectangle = Rectangle {
        x: 10.0,
        y: 20.0,
        width: 480.0,
        height: 480.0,
    };

    /// The pointer over the tip of `a`, in window coordinates.
    fn over_tip(a: EngineArrow) -> mouse::Cursor {
        let layout = BoardLayout::compute(BOUNDS.size(), BoardOptions::default().layout_options())
            .expect("fits");
        let shape = marks::engine_arrow_geometry(
            &layout,
            a.from,
            a.to,
            Orientation::White,
            layers::shaft_of(a.kind).unwrap_or(0.14),
        )
        .expect("geometry");
        // A point a little behind the tip, inside the head triangle.
        let c = shape.base;
        let p = Point::new(
            (c.x * 2.0 + shape.tip.x) / 3.0 + BOUNDS.x,
            (c.y * 2.0 + shape.tip.y) / 3.0 + BOUNDS.y,
        );
        mouse::Cursor::Available(p)
    }

    fn press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    fn release() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }

    fn published(action: Option<Action<BoardEvent>>) -> Option<BoardEvent> {
        action.and_then(|a| a.into_inner().0)
    }

    #[test]
    fn sync_badge_clears_the_layer_only_when_the_badge_changes() {
        let badge = |square: &str, glyph: &'static str| ClassBadge {
            square: square.parse().expect("square"),
            glyph,
            color: iced::Color::from_rgb(0.8, 0.2, 0.2),
        };
        let mut caches = BoardCaches::default();
        assert!(
            !caches.sync_badge(None, Orientation::White),
            "nothing to draw, nothing seen"
        );
        let before = caches.arrow_clears();
        assert!(caches.sync_badge(Some(badge("e5", "??")), Orientation::White));
        assert_eq!(caches.arrow_clears(), before + 1);
        assert!(
            !caches.sync_badge(Some(badge("e5", "??")), Orientation::White),
            "the same badge"
        );
        assert_eq!(caches.arrow_clears(), before + 1);
        assert!(
            caches.sync_badge(Some(badge("e5", "?")), Orientation::White),
            "another class"
        );
        assert!(
            caches.sync_badge(Some(badge("f6", "?")), Orientation::White),
            "another square"
        );
        let mut other = badge("f6", "?");
        other.color = iced::Color::from_rgb(0.1, 0.2, 0.9);
        assert!(
            caches.sync_badge(Some(other), Orientation::White),
            "another colour (theme)"
        );
        assert!(
            caches.sync_badge(Some(badge("f6", "?")), Orientation::White),
            "back"
        );
        assert!(
            caches.sync_badge(Some(badge("f6", "?")), Orientation::Black),
            "the board was turned"
        );
        assert!(
            !caches.sync_badge(Some(badge("f6", "?")), Orientation::Black),
            "same badge, same orientation"
        );
        assert!(
            caches.sync_badge(None, Orientation::Black),
            "the badge went away"
        );
        assert!(!caches.sync_badge(None, Orientation::White));
    }

    #[test]
    fn sync_arrows_clears_the_layer_only_when_the_list_or_orientation_changes() {
        let mut caches = BoardCaches::default();
        let one = [arrow("e2", "e4", 1, ArrowKind::Best)];
        assert!(caches.sync_arrows(&one, Orientation::White), "first time");
        assert!(!caches.sync_arrows(&one, Orientation::White));
        assert!(caches.sync_arrows(&one, Orientation::Black), "turned");
        assert!(caches.sync_arrows(&[], Orientation::Black), "emptied");
    }

    #[test]
    fn a_change_of_the_coordinate_mode_clears_the_arrows_layer() {
        // Same list, same orientation: only the layout of the squares moves.
        let position = Position::new();
        let (brush, pieces) = (MarkBrush::default(), PieceSet::default());
        let mut caches = BoardCaches::default();
        let one = [arrow("e2", "e4", 1, ArrowKind::Best)];
        let mut d = data(&position, &brush, &pieces, None);
        caches.sync_arrows(&one, Orientation::White);
        caches.sync_data(&d);
        let before = caches.arrow_clears();
        caches.sync_data(&d);
        assert_eq!(caches.arrow_clears(), before, "nothing changed");
        d.options.coords = CoordsMode::Inside;
        caches.sync_data(&d);
        assert_eq!(caches.arrow_clears(), before + 1, "the layout changed");
        assert!(!caches.sync_arrows(&one, Orientation::White));
    }

    #[test]
    fn the_hit_target_of_a_tip_has_a_margin() {
        let (position, brush, pieces) =
            (Position::new(), MarkBrush::default(), PieceSet::default());
        let caches = BoardCaches::default();
        let arrows = [arrow("e2", "e4", 1, ArrowKind::Best)];
        let widget = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, true);
        let layout = BoardLayout::compute(
            iced::Size::new(800.0, 800.0),
            BoardOptions::default().layout_options(),
        )
        .expect("layout");
        let shape = marks::engine_arrow_geometry(
            &layout,
            "e2".parse().expect("sq"),
            "e4".parse().expect("sq"),
            Orientation::White,
            0.14,
        )
        .expect("arrow");
        // Just beyond the tip (upwards on White's board): inside the margin.
        let near = Point::new(shape.tip.x, shape.tip.y - 2.0);
        assert!(!shape.head_contains(near));
        assert!(widget.arrow_at(&layout, near).is_some());
        let far = Point::new(shape.tip.x, shape.tip.y - 12.0);
        assert!(widget.arrow_at(&layout, far).is_none());
    }

    #[test]
    fn a_click_on_the_tip_of_an_arrow_publishes_it_on_release() {
        let (position, brush, pieces) =
            (Position::new(), MarkBrush::default(), PieceSet::default());
        let caches = BoardCaches::default();
        let arrows = [arrow("e2", "e4", 1, ArrowKind::Best)];
        let widget = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, true);
        let mut state = State::default();
        let cursor = over_tip(arrows[0]);
        let down = canvas::Program::update(&widget, &mut state, &press(), BOUNDS, cursor);
        assert_eq!(published(down), None, "the press is only taken");
        assert!(state.interaction.is_idle(), "no piece gesture started");
        let up = canvas::Program::update(&widget, &mut state, &release(), BOUNDS, cursor);
        assert_eq!(
            published(up),
            Some(BoardEvent::PlayEngineArrow {
                from: arrows[0].from,
                to: arrows[0].to,
                rank: 1
            })
        );
    }

    #[test]
    fn releasing_away_from_the_tip_publishes_nothing() {
        let (position, brush, pieces) =
            (Position::new(), MarkBrush::default(), PieceSet::default());
        let caches = BoardCaches::default();
        let arrows = [arrow("e2", "e4", 1, ArrowKind::Best)];
        let widget = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, true);
        let mut state = State::default();
        let cursor = over_tip(arrows[0]);
        canvas::Program::update(&widget, &mut state, &press(), BOUNDS, cursor);
        let elsewhere = mouse::Cursor::Available(Point::new(BOUNDS.x + 5.0, BOUNDS.y + 5.0));
        let up = canvas::Program::update(&widget, &mut state, &release(), BOUNDS, elsewhere);
        assert_eq!(published(up), None);
    }

    #[test]
    fn the_arrow_of_the_review_is_a_hint_and_never_plays_a_move() {
        let (position, brush, pieces) =
            (Position::new(), MarkBrush::default(), PieceSet::default());
        let caches = BoardCaches::default();
        let arrows = [arrow("e2", "e4", 1, ArrowKind::ReviewBest)];
        let widget = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, true);
        let layout = BoardLayout::compute(BOUNDS.size(), BoardOptions::default().layout_options())
            .expect("fits");
        assert_eq!(widget.arrow_at(&layout, Point::new(0.0, 0.0)), None);
        let cursor = over_tip(arrows[0]);
        let mut state = State::default();
        canvas::Program::update(&widget, &mut state, &press(), BOUNDS, cursor);
        let up = canvas::Program::update(&widget, &mut state, &release(), BOUNDS, cursor);
        assert_eq!(published(up), None);
        // Drawn on top, like the best line of the engine.
        let mixed = [
            arrow("e2", "e4", 1, ArrowKind::ReviewBest),
            arrow("d2", "d4", 2, ArrowKind::Other),
        ];
        let order: Vec<_> = layers::arrow_paint_order(&mixed).map(|a| a.kind).collect();
        assert_eq!(order, [ArrowKind::Other, ArrowKind::ReviewBest]);
    }

    #[test]
    fn arrows_are_not_clickable_when_off_hidden_or_with_alt() {
        let (position, brush, pieces) =
            (Position::new(), MarkBrush::default(), PieceSet::default());
        let caches = BoardCaches::default();
        let arrows = [arrow("e2", "e4", 1, ArrowKind::Best)];
        let cursor = over_tip(arrows[0]);
        let click = |widget: &BoardWidget<'_>, state: &mut State| {
            canvas::Program::update(widget, state, &press(), BOUNDS, cursor);
            published(canvas::Program::update(
                widget,
                state,
                &release(),
                BOUNDS,
                cursor,
            ))
        };
        // `click_to_play` off.
        let off = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, false);
        assert_eq!(click(&off, &mut State::default()), None);
        // A sliding piece hides the arrows.
        let (old, new, animation) = after_e4();
        let _ = old;
        let moving = BoardWidget::new(data(&new, &brush, &pieces, Some(&animation)), &caches)
            .with_engine_arrows(&arrows, true);
        assert_eq!(click(&moving, &mut State::default()), None);
        // `Alt` passes the click to the marks below.
        let on = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, true);
        let mut alt = State::default();
        alt.mods.alt = true;
        assert_eq!(click(&on, &mut alt), None);
    }

    #[test]
    fn later_sequence_arrows_are_not_clickable() {
        let (position, brush, pieces) =
            (Position::new(), MarkBrush::default(), PieceSet::default());
        let caches = BoardCaches::default();
        let arrows = [
            arrow("e2", "e4", 1, ArrowKind::Sequence(0)),
            arrow("e7", "e5", 1, ArrowKind::Sequence(1)),
        ];
        let widget = BoardWidget::new(data(&position, &brush, &pieces, None), &caches)
            .with_engine_arrows(&arrows, true);
        let layout = BoardLayout::compute(BOUNDS.size(), BoardOptions::default().layout_options())
            .expect("fits");
        let tip = |a: &EngineArrow| {
            marks::engine_arrow_geometry(&layout, a.from, a.to, Orientation::White, 0.07)
                .map(|g| {
                    Point::new(
                        (g.base.x + g.tip.x * 2.0) / 3.0,
                        (g.base.y + g.tip.y * 2.0) / 3.0,
                    )
                })
                .expect("geometry")
        };
        assert_eq!(
            widget.arrow_at(&layout, tip(&arrows[0])),
            Some((arrows[0].from, arrows[0].to, 1))
        );
        assert_eq!(widget.arrow_at(&layout, tip(&arrows[1])), None);
    }

    #[test]
    fn stale_interaction_is_ignored_by_the_shared_guard() {
        let id = (Position::new().zobrist(), 0, Orientation::White);
        let mut state = State::default();
        assert!(state.live(id).is_none(), "nothing seen yet");
        state.seen = Some(id);
        assert!(state.live(id).is_some());
        let other = (id.0, 1, Orientation::White);
        assert!(state.live(other).is_none());
        let flipped = (id.0, 0, Orientation::Black);
        assert!(state.live(flipped).is_none());
    }
}
