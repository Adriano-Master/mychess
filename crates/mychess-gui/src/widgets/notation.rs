//! The notation widget (spec M2-4, D161, D168-D171; M2-5, D241-D254).
//!
//! [`NotationView`] is an `advanced::Widget` over the application's
//! [`NotationDoc`]. The rich-text widget of the toolkit only reports the
//! left button, so this one keeps its own shaped paragraphs, draws the current
//! move as a rectangle behind the text (moving the cursor never shapes
//! anything) and does its own hit testing, scrolling and scroll bar.
//!
//! It has two ways of laying the document out:
//!
//! - **text modes** (`Indented`, `Inline`): one paragraph per block
//!   ([`layout::LayoutCache`]), as in the M2-4;
//! - the **table** (`NotationView::table`): one row per full move, with
//!   cards for the variations ([`table_layout::TableCache`]).
//!
//! The widget publishes [`NotationEvent`]s; what they mean (go to a node,
//! open the editor, show a menu, preview a position, fold a card) is decided
//! by the panel in `windows::notation`.

pub mod figurine;
pub mod hit;
pub mod layout;
#[cfg(test)]
mod mock;
pub mod scroll;
pub mod style;
pub mod table_layout;

use std::collections::HashSet;
use std::sync::LazyLock;

use iced::advanced::text::{self, Paragraph};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout as iced_layout, mouse, renderer};
use iced::{Border, Element, Event, Font, Length, Point, Rectangle, Size, Theme};
use mychess_app::{CommentTarget, NotationDoc, NotationTable};
use mychess_core::{Color as Side, NodeId};

pub use self::hit::Hit;
use self::layout::{Inputs, LayoutCache};
use self::scroll::{BAR_WIDTH, Scroll};
pub use self::style::{Labels, TextStyle, nag_label};
use self::table_layout::{Mark, Place, TableCache, TableInputs};
use crate::theme::notation_colors::NotationPalette;

/// No card is folded.
static NO_CARDS: LazyLock<HashSet<NodeId>> = LazyLock::new(HashSet::new);

/// Lines moved by one notch of the mouse wheel.
const WHEEL_LINES: f32 = 3.0;

/// Something the user did on the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NotationEvent {
    /// Left click on a move or an annotation symbol: go there.
    Click(NodeId),
    /// Left click on a comment: edit it.
    CommentClick(CommentTarget),
    /// The pointer entered a move (or left all moves). The point is in
    /// window coordinates.
    Hover(Option<(NodeId, Point)>),
    /// The middle button went down on a move (with the pointer position in
    /// window coordinates), or was released.
    Middle(Option<(NodeId, Point)>),
    /// Right click (`hit` is `None` on empty space). The point is in window
    /// coordinates.
    Menu {
        /// What was under the pointer.
        hit: Option<Hit>,
        /// Where to open the menu.
        position: Point,
    },
    /// Left click on the header of a variation card: fold or unfold it. The
    /// node is the first move of the card.
    ToggleCard(NodeId),
    /// The content scrolled (previews and menus close).
    Scrolled,
}

/// What marks the position of a move in the text: where a highlight goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkKey {
    /// The move of this node.
    Node(NodeId),
    /// The start of the game: a thin bar before the first move.
    Start,
}

/// A highlighted place, cached against the layout it was computed for.
#[derive(Debug, Default)]
struct Marker {
    key: Option<MarkKey>,
    stamp: u64,
    /// Rectangles in content coordinates.
    rects: Vec<Rectangle>,
    /// The cell of the table the mark is on, if it is on a cell (a pill or
    /// an outline) and not on text.
    cell: Option<(usize, Side)>,
}

impl Marker {
    /// Recomputes the rectangles if the target or the layout changed.
    /// Returns whether it did.
    fn refresh<P: Paragraph<Font = Font>>(
        &mut self,
        key: Option<MarkKey>,
        cache: &LayoutCache<P>,
    ) -> bool {
        if self.key == key && self.stamp == cache.stamp() {
            return false;
        }
        self.key = key;
        self.stamp = cache.stamp();
        self.cell = None;
        self.rects.clear();
        let span = match key {
            Some(MarkKey::Node(node)) => cache.find_move(node),
            Some(MarkKey::Start) => cache.first_move(),
            None => None,
        };
        if let Some((entry, span)) = span {
            if key == Some(MarkKey::Start) {
                // A bar of no width at the left edge of the first move.
                self.rects = cache.span_rects(entry, span);
                self.rects.truncate(1);
                for rect in &mut self.rects {
                    rect.width = 0.0;
                }
            } else {
                self.rects = cache.group_rects(entry, span);
            }
        }
        true
    }

    /// [`Marker::refresh`] over the table. `mark` is how much of a cell the
    /// rectangle covers; the start of the game has no mark in the table.
    fn refresh_table<P: Paragraph<Font = Font>>(
        &mut self,
        key: Option<MarkKey>,
        table: &mut TableCache<P>,
        mark: Mark,
    ) -> bool {
        let stamp = table_stamp(table);
        if self.key == key && self.stamp == stamp {
            return false;
        }
        self.key = key;
        self.cell = None;
        self.rects.clear();
        if let Some(MarkKey::Node(node)) = key
            && let Some((rects, place)) = table.move_rects(node, mark)
        {
            self.rects = rects;
            if let Place::Cell { row, side } = place {
                self.cell = Some((row, side));
            }
        }
        // Shaping a row for the mark does not move anything, but read the
        // stamp after, in case the table was laid out meanwhile.
        self.stamp = table_stamp(table);
        true
    }

    /// The smallest rectangle that holds all of them.
    fn bounds(&self) -> Option<Rectangle> {
        let mut rects = self.rects.iter();
        let first = *rects.next()?;
        Some(rects.fold(first, |acc, r| acc.union(r)))
    }
}

/// The stamp of a table, apart from the stamps of the text layouts.
fn table_stamp<P: Paragraph<Font = Font>>(table: &TableCache<P>) -> u64 {
    table.stamp() | (1 << 63)
}

/// The item under the pointer.
#[derive(Debug, Default)]
struct Hover {
    hit: Option<Hit>,
    /// The paragraph (or row) and span the hit is in.
    at: Option<(usize, usize)>,
    /// Rectangles in content coordinates.
    rects: Vec<Rectangle>,
    /// The rectangles are in a card or a paragraph, not on a table cell.
    on_text: bool,
}

/// What a pointer movement changed.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Moved {
    /// The highlight under the pointer changed: draw again.
    pub(crate) redraw: bool,
    /// The application should know (a different move is under the pointer).
    pub(crate) event: Option<NotationEvent>,
}

/// Everything the widget remembers between frames. Generic over the
/// paragraph type so the logic can be tested with a stand-in.
#[derive(Debug)]
pub(crate) struct Panel<P> {
    cache: LayoutCache<P>,
    table: TableCache<P>,
    /// The last frame was laid out as a table.
    table_mode: bool,
    scroll: Scroll,
    seen_token: Option<u64>,
    reveal: bool,
    current: Marker,
    target: Marker,
    hover: Hover,
    pressed: Option<Hit>,
    middle: bool,
    /// While the thumb is dragged: where inside the thumb it was grabbed.
    drag: Option<f32>,
    size: Size,
    line_height: f32,
}

impl<P> Default for Panel<P> {
    fn default() -> Panel<P> {
        Panel {
            cache: LayoutCache::default(),
            table: TableCache::default(),
            table_mode: false,
            scroll: Scroll::default(),
            seen_token: None,
            reveal: false,
            current: Marker::default(),
            target: Marker::default(),
            hover: Hover::default(),
            pressed: None,
            middle: false,
            drag: None,
            size: Size::ZERO,
            line_height: 16.0,
        }
    }
}

/// The table and what is folded in it: with it the widget draws the table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TableParams<'a> {
    pub(crate) table: &'a NotationTable,
    /// The first moves of the folded cards.
    pub(crate) collapsed: Option<&'a HashSet<NodeId>>,
}

/// What the widget draws; borrowed from the application state per frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Params<'a> {
    pub(crate) doc: &'a NotationDoc,
    pub(crate) labels: &'a Labels,
    pub(crate) style: &'a TextStyle,
    /// The node of the cursor, `None` at the start of the game.
    pub(crate) current: Option<NodeId>,
    /// The node a context menu acts on.
    pub(crate) menu_target: Option<NodeId>,
    pub(crate) reveal_token: u64,
    /// `Some`: lay out the table instead of the text paragraphs.
    pub(crate) table: Option<TableParams<'a>>,
}

impl<P: Paragraph<Font = Font>> Panel<P> {
    /// The scroll bar track, if the content overflows.
    fn thumb(&self) -> Option<(f32, f32)> {
        self.scroll.thumb(self.size.height)
    }

    fn in_bar(&self, local: Point) -> bool {
        self.thumb().is_some() && local.x >= self.size.width - BAR_WIDTH
    }

    fn content_height(&self) -> f32 {
        if self.table_mode {
            self.table.content_height()
        } else {
            self.cache.content_height()
        }
    }

    /// Shapes the rows near the window after the scroll position changed
    /// (the move rows of the table are shaped when they come near).
    fn scroll_changed(&mut self) {
        if self.table_mode {
            let top = self.scroll.offset();
            self.table.ensure_shaped(top, top + self.size.height);
        }
    }

    /// Brings paragraphs, scroll and highlights up to date for a frame in
    /// `size`.
    pub(crate) fn prepare(&mut self, params: &Params<'_>, size: Size) {
        self.size = size;
        let width = size.width - BAR_WIDTH;
        let was_table = self.table_mode;
        self.table_mode = params.table.is_some();
        let mut changed = was_table != self.table_mode;
        if let Some(t) = &params.table {
            self.line_height = self.table.row_height();
            changed |= self.table.sync(&TableInputs {
                table: t.table,
                doc: params.doc,
                labels: params.labels,
                style: params.style,
                width,
                collapsed: t.collapsed.unwrap_or(&NO_CARDS),
            });
            self.line_height = self.table.row_height();
        } else {
            self.line_height = params.style.line_height_px();
            changed |= self.cache.sync(&Inputs {
                doc: params.doc,
                labels: params.labels,
                style: params.style,
                width,
            });
        }
        if changed {
            self.hover = Hover::default();
            self.pressed = None;
        }
        self.scroll.set_extent(self.content_height(), size.height);
        let key = match params.current {
            Some(node) => MarkKey::Node(node),
            None => MarkKey::Start,
        };
        let target = params.menu_target.map(MarkKey::Node);
        if self.table_mode {
            self.current
                .refresh_table(Some(key), &mut self.table, Mark::Pill);
            self.target
                .refresh_table(target, &mut self.table, Mark::Cell);
        } else {
            self.current.refresh(Some(key), &self.cache);
            self.target.refresh(target, &self.cache);
        }
        if self.seen_token != Some(params.reveal_token) {
            self.seen_token = Some(params.reveal_token);
            self.reveal = true;
        }
        if self.reveal {
            self.reveal = false;
            match params.current {
                None => self.scroll.set_offset(0.0),
                Some(_) => {
                    if let Some(rect) = self.current.bounds() {
                        self.scroll
                            .reveal(rect.y, rect.y + rect.height, self.line_height);
                    }
                }
            }
        }
        self.scroll_changed();
    }

    /// What is at `local` (content coordinates).
    fn locate(&self, point: Point) -> Option<layout::Located> {
        if self.table_mode {
            self.table.locate(point)
        } else {
            self.cache.locate(point)
        }
    }

    /// The pointer moved to `local` (inside the widget) or left.
    pub(crate) fn pointer(&mut self, local: Option<Point>, window: Option<Point>) -> Moved {
        let located = local
            .filter(|p| !self.in_bar(*p))
            .and_then(|p| self.locate(Point::new(p.x, p.y + self.scroll.offset())))
            .filter(|l| l.hit.is_some());
        let at = located.map(|l| (l.entry, l.span));
        if at == self.hover.at {
            return Moved::default();
        }
        let before = self.hover.hit.and_then(Hit::node);
        self.hover.at = at;
        self.hover.hit = located.and_then(|l| l.hit);
        self.hover.on_text =
            located.is_some_and(|l| !self.table_mode || !self.table.is_move_row(l.entry));
        self.hover.rects = located.map_or_else(Vec::new, |l| {
            if self.table_mode {
                self.table.hit_rects(&l)
            } else {
                self.cache.group_rects(l.entry, l.span)
            }
        });
        let after = self.hover.hit.and_then(Hit::node);
        Moved {
            redraw: true,
            event: (before != after).then(|| NotationEvent::Hover(after.zip(window))),
        }
    }

    /// Left button down at `local`. Returns whether the widget used it.
    pub(crate) fn press_left(&mut self, local: Point) -> bool {
        if self.in_bar(local) {
            self.press_bar(local.y);
            return true;
        }
        let _ = self.pointer(Some(local), None);
        self.pressed = self.hover.hit;
        self.pressed.is_some()
    }

    fn press_bar(&mut self, y: f32) {
        let Some((top, height)) = self.thumb() else {
            return;
        };
        if y >= top && y <= top + height {
            self.drag = Some(y - top);
        } else {
            // A click on the track centers the thumb there.
            let target = y - height / 2.0;
            self.scroll
                .set_offset(self.scroll.offset_for_thumb(self.size.height, target));
            self.drag = Some(height / 2.0);
            self.scroll_changed();
        }
    }

    /// The pointer moved while the thumb is dragged.
    pub(crate) fn drag_to(&mut self, local_y: f32) -> bool {
        let Some(grab) = self.drag else {
            return false;
        };
        let offset = self
            .scroll
            .offset_for_thumb(self.size.height, local_y - grab);
        self.scroll.set_offset(offset);
        self.scroll_changed();
        true
    }

    /// Left button up at `local`.
    pub(crate) fn release_left(&mut self, local: Option<Point>) -> (bool, Option<NotationEvent>) {
        let dragged = self.drag.take().is_some();
        let Some(pressed) = self.pressed.take() else {
            return (dragged, None);
        };
        let _ = self.pointer(local, None);
        let event = (self.hover.hit == Some(pressed)).then_some(match pressed {
            Hit::Move(node) | Hit::Nag(node) => NotationEvent::Click(node),
            Hit::Comment(target) => NotationEvent::CommentClick(target),
            Hit::Card(node) => NotationEvent::ToggleCard(node),
        });
        (true, event)
    }

    /// Right button down at `local`.
    pub(crate) fn press_right(&mut self, local: Point, window: Point) -> NotationEvent {
        let _ = self.pointer(Some(local), None);
        NotationEvent::Menu {
            hit: self.hover.hit,
            position: window,
        }
    }

    /// Middle button down at `local`: the node to preview, if any.
    pub(crate) fn press_middle(&mut self, local: Point, window: Point) -> Option<NotationEvent> {
        let _ = self.pointer(Some(local), None);
        let node = self.hover.hit.and_then(Hit::node)?;
        self.middle = true;
        Some(NotationEvent::Middle(Some((node, window))))
    }

    /// Middle button up.
    pub(crate) fn release_middle(&mut self) -> Option<NotationEvent> {
        std::mem::take(&mut self.middle).then_some(NotationEvent::Middle(None))
    }

    /// The wheel moved by `lines` notches (positive: content moves down).
    /// Returns whether the content scrolled.
    pub(crate) fn wheel(&mut self, lines: f32, local: Option<Point>) -> bool {
        let moved = self.scroll_by_pixels(-lines * WHEEL_LINES * self.line_height);
        // The item under a still pointer changed.
        let _ = self.pointer(local, None);
        moved
    }

    /// Scrolls by pixels (positive: further down the document).
    pub(crate) fn scroll_by_pixels(&mut self, delta: f32) -> bool {
        let moved = self.scroll.scroll_by(delta);
        if moved {
            self.scroll_changed();
        }
        moved
    }

    fn draw_into<R>(&self, renderer: &mut R, bounds: Rectangle, style: &TextStyle)
    where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let offset = self.scroll.offset();
        let palette = &style.palette;
        renderer.with_layer(bounds, |renderer| {
            if self.table_mode {
                self.draw_table(renderer, bounds, offset, palette);
            } else {
                self.draw_text(renderer, bounds, offset, palette);
            }
            self.draw_bar(renderer, bounds, palette);
        });
    }

    /// The table: backgrounds, marks (outline of the menu target, pill of
    /// the current move, hover), then the text.
    fn draw_table<R>(
        &self,
        renderer: &mut R,
        bounds: Rectangle,
        offset: f32,
        palette: &NotationPalette,
    ) where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let shift = |rect: &Rectangle| shifted(rect, bounds, offset);
        self.table
            .draw_backgrounds(renderer, bounds, offset, palette);
        let soft = Border {
            radius: palette.pill_radius.into(),
            ..Border::default()
        };
        for rect in &self.target.rects {
            if self.target.cell.is_some() {
                // The item a menu acts on: an outline.
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: shift(rect),
                        border: Border {
                            color: palette.target,
                            width: 1.5,
                            radius: palette.pill_radius.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    iced::Color::TRANSPARENT,
                );
            } else {
                fill(renderer, shift(rect), palette.target.scale_alpha(0.3), soft);
            }
        }
        for rect in &self.current.rects {
            let color = if self.current.cell.is_some() {
                palette.pill
            } else {
                palette.current
            };
            fill(renderer, shift(rect), color, soft);
        }
        let hover = if self.hover.on_text {
            palette.card_hover
        } else {
            palette.hover
        };
        for rect in &self.hover.rects {
            fill(renderer, shift(rect), hover, soft);
        }
        self.table
            .draw_text(renderer, bounds, offset, palette, self.current.cell);
    }

    /// The text paragraphs of the Indented and Inline layouts.
    fn draw_text<R>(
        &self,
        renderer: &mut R,
        bounds: Rectangle,
        offset: f32,
        palette: &NotationPalette,
    ) where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let shift = |rect: &Rectangle| shifted(rect, bounds, offset);
        let round = Border {
            radius: 2.0.into(),
            ..Border::default()
        };
        for rect in &self.target.rects {
            fill(
                renderer,
                shift(rect),
                palette.target.scale_alpha(0.3),
                round,
            );
        }
        if self.current.key == Some(MarkKey::Start) {
            for rect in &self.current.rects {
                let bar = Rectangle {
                    x: rect.x - 3.0,
                    width: 2.0,
                    ..*rect
                };
                fill(renderer, shift(&bar), palette.header, Border::default());
            }
        } else {
            for rect in &self.current.rects {
                fill(renderer, shift(rect), palette.current, round);
            }
        }
        for rect in &self.hover.rects {
            fill(renderer, shift(rect), palette.hover, round);
        }
        let visible = self.cache.visible(offset, offset + bounds.height);
        self.cache.for_each(visible, |paragraph, x, y| {
            renderer.fill_paragraph(
                paragraph,
                Point::new(bounds.x + x, bounds.y + y - offset),
                palette.main,
                bounds,
            );
        });
    }

    /// The scroll bar, when the content overflows.
    fn draw_bar<R: renderer::Renderer>(
        &self,
        renderer: &mut R,
        bounds: Rectangle,
        palette: &NotationPalette,
    ) {
        let Some((top, height)) = self.thumb() else {
            return;
        };
        let track = Rectangle {
            x: bounds.x + bounds.width - BAR_WIDTH,
            y: bounds.y,
            width: BAR_WIDTH,
            height: bounds.height,
        };
        fill(renderer, track, palette.track, Border::default());
        let thumb = Rectangle {
            y: bounds.y + top,
            height,
            ..track
        };
        let round = Border {
            radius: 2.0.into(),
            ..Border::default()
        };
        fill(renderer, thumb, palette.thumb, round);
    }
}

/// A rectangle of the content as it is on the window, scrolled by `offset`.
fn shifted(rect: &Rectangle, bounds: Rectangle, offset: f32) -> Rectangle {
    Rectangle {
        x: bounds.x + rect.x,
        y: bounds.y + rect.y - offset,
        ..*rect
    }
}

fn fill<R: renderer::Renderer>(
    renderer: &mut R,
    bounds: Rectangle,
    color: iced::Color,
    border: Border,
) {
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border,
            ..renderer::Quad::default()
        },
        color,
    );
}

/// The document as an interactive, scrollable widget.
pub struct NotationView<'a, Message> {
    doc: &'a NotationDoc,
    labels: &'a Labels,
    style: TextStyle,
    current: Option<NodeId>,
    menu_target: Option<NodeId>,
    reveal_token: u64,
    table: Option<&'a NotationTable>,
    collapsed: Option<&'a HashSet<NodeId>>,
    on_event: Box<dyn Fn(NotationEvent) -> Message + 'a>,
}

impl<'a, Message> NotationView<'a, Message> {
    /// A view of `doc`. `current` is the node of the cursor (`None` at the
    /// start of the game); `reveal_token` changes when the view must scroll
    /// to it.
    #[allow(clippy::large_types_passed_by_value)] // `TextStyle` is `Copy`, built inline by callers
    pub fn new(
        doc: &'a NotationDoc,
        labels: &'a Labels,
        style: TextStyle,
        current: Option<NodeId>,
        on_event: impl Fn(NotationEvent) -> Message + 'a,
    ) -> Self {
        NotationView {
            doc,
            labels,
            style,
            current,
            menu_target: None,
            reveal_token: 0,
            table: None,
            collapsed: None,
            on_event: Box::new(on_event),
        }
    }

    /// Lays the document out as the table `table` (spec M2-5), which must
    /// have been projected from the same document. `collapsed` holds the
    /// first moves of the cards that are folded.
    #[must_use]
    pub fn table(mut self, table: &'a NotationTable, collapsed: &'a HashSet<NodeId>) -> Self {
        self.table = Some(table);
        self.collapsed = Some(collapsed);
        self
    }

    /// The node a context menu is open for; it stays highlighted.
    #[must_use]
    pub fn menu_target(mut self, node: Option<NodeId>) -> Self {
        self.menu_target = node;
        self
    }

    /// The scroll-to-current token (spec D170).
    #[must_use]
    pub fn reveal_token(mut self, token: u64) -> Self {
        self.reveal_token = token;
        self
    }

    fn params(&self) -> Params<'_> {
        Params {
            doc: self.doc,
            labels: self.labels,
            style: &self.style,
            current: self.current,
            menu_target: self.menu_target,
            reveal_token: self.reveal_token,
            table: self.table.map(|table| TableParams {
                table,
                collapsed: self.collapsed,
            }),
        }
    }
}

impl<'a, Message> NotationView<'a, Message>
where
    Message: 'a,
{
    /// The widget as an element.
    pub fn into_element(self) -> Element<'a, Message> {
        Element::new(self)
    }
}

/// The text style for a theme and the options of the panel (figurines off;
/// see [`TextStyle::with_figurines`]).
pub fn text_style(theme: &Theme, size: f32, symbolic_nags: bool, bold_main: bool) -> TextStyle {
    TextStyle {
        size,
        line_height: 1.35,
        palette: NotationPalette::of(theme),
        symbolic_nags,
        bold_main,
        figurines: false,
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for NotationView<'_, Message>
where
    Renderer: text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Panel<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Panel::<Renderer::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &iced_layout::Limits,
    ) -> iced_layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        let panel = tree.state.downcast_mut::<Panel<Renderer::Paragraph>>();
        panel.prepare(&self.params(), size);
        iced_layout::Node::new(size)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let panel = tree.state.downcast_ref::<Panel<Renderer::Paragraph>>();
        panel.draw_into(renderer, layout.bounds(), &self.style);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Mouse(event) = event else {
            return;
        };
        let panel = tree.state.downcast_mut::<Panel<Renderer::Paragraph>>();
        let bounds = layout.bounds();
        let local = cursor.position_in(bounds);
        let window = cursor.position();
        let publish = |shell: &mut Shell<'_, Message>, event: NotationEvent| {
            shell.publish((self.on_event)(event));
        };
        match event {
            mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft => {
                if panel.drag.is_some() {
                    if let Some(p) = cursor.position() {
                        panel.drag_to(p.y - bounds.y);
                        shell.request_redraw();
                        shell.capture_event();
                    }
                } else {
                    let moved = panel.pointer(local, window);
                    if let Some(ev) = moved.event {
                        publish(shell, ev);
                    }
                    if moved.redraw {
                        shell.request_redraw();
                    }
                }
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                if let Some(local) = local
                    && panel.press_left(local)
                {
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                let (used, ev) = panel.release_left(local);
                if let Some(ev) = ev {
                    publish(shell, ev);
                }
                if used {
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            mouse::Event::ButtonPressed(mouse::Button::Right) => {
                if let (Some(local), Some(window)) = (local, window) {
                    let ev = panel.press_right(local, window);
                    publish(shell, ev);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            mouse::Event::ButtonPressed(mouse::Button::Middle) => {
                if let (Some(local), Some(window)) = (local, window)
                    && let Some(ev) = panel.press_middle(local, window)
                {
                    publish(shell, ev);
                    shell.capture_event();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Middle) => {
                if let Some(ev) = panel.release_middle() {
                    publish(shell, ev);
                    shell.capture_event();
                }
            }
            mouse::Event::WheelScrolled { delta } if local.is_some() => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / panel.line_height.max(1.0),
                };
                if panel.wheel(lines, local) {
                    publish(shell, NotationEvent::Scrolled);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let panel = tree.state.downcast_ref::<Panel<Renderer::Paragraph>>();
        if panel.hover.hit.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod tests;
