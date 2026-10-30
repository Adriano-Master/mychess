//! The rows of the notation table and where they are (spec M2-5, D241-D254).
//!
//! [`TableCache`] keeps one entry per row of the application's
//! [`NotationTable`]:
//!
//! - a **move row** (`number | white | black`, and the time column) has a
//!   fixed height, so its place is known without shaping anything. Its text
//!   is shaped **only when the row comes near the viewport**
//!   ([`TableCache::ensure_shaped`]) and kept while the row's revision, the
//!   text style and the time column stay the same. A cell is a handful of
//!   tiny paragraphs (figurine, move, one per annotation), so the text of the
//!   current move can change color (the pill, D246) without shaping again;
//! - a **comment, card, placeholder or result row** has a height that depends
//!   on the width, so it is shaped when it appears and laid out again (not
//!   shaped) when the width changes.
//!
//! Moving the cursor does not reach this module at all, except to shape the
//! row of the new current move if it was far from the viewport.

#[path = "table_layout/draw.rs"]
mod draw;

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use iced::advanced::text::{Alignment, LineHeight, Paragraph, Shaping, Text, Wrapping};
use iced::{Color, Font, Pixels, Point, Rectangle, Size, alignment};
use mychess_app::{
    Cell, HeaderLine, Inline, MAX_CARD_INDENT, NotationDoc, NotationTable, RowContent, RowKey,
};
use mychess_core::{Color as Side, NodeId};

use super::figurine::{Part, san_parts};
use super::hit::{Hit, row_at};
use super::layout::{Located, shape_spans};
use super::style::{
    Labels, NotationSpan, PADDING, TextStyle, block_spans_with, empty_span, header_spans,
    nag_label, result_span,
};
use crate::fonts;
use crate::widgets::glyph::GLYPH_FONT;

/// Height of a move row (D253).
pub const ROW_HEIGHT: f32 = 32.0;
/// Height of a move row when the time column is on.
pub const ROW_HEIGHT_TIMES: f32 = 34.0;
/// Indentation of a card per nesting level.
pub const CARD_STEP: f32 = 12.0;
/// Width of the bar at the left edge of a card.
pub const CARD_BAR: f32 = 3.0;
/// Width of the column of the fold chevron of a card.
pub const GUTTER: f32 = 18.0;
/// Space between a pill and the text inside it.
pub const PILL_PAD: f32 = 6.0;
/// The widest bar of the time column (D250).
pub const TIME_BAR_MAX: f32 = 40.0;
/// The narrowest bar.
pub const TIME_BAR_MIN: f32 = 2.0;
/// Width of the time column.
const TIME_COLUMN: f32 = 92.0;
/// The time column is hidden below this width of the panel (D250; the scroll
/// bar is part of the 260).
const TIME_MIN_PANEL_WIDTH: f32 = 260.0;
/// Smallest width of a column of moves.
const MIN_CELL: f32 = 48.0;
/// Gap between the move and its annotation symbols.
const NAG_GAP: f32 = 3.0;
/// Margin above and below a card.
const CARD_MARGIN: f32 = 3.0;
/// Space inside a card, above and below the text.
const CARD_PAD_V: f32 = 4.0;
/// Space inside a card at the right.
const CARD_PAD_R: f32 = 8.0;
/// Margin above and below a comment.
const COMMENT_MARGIN: f32 = 2.0;
/// A figurine is a little larger than the text around it: the chess glyphs
/// are drawn small in their em box (checked on a rendering at 15 px).
const FIGURINE_SCALE: f32 = 1.2;
/// The glyph font sits high on the line; this brings its baseline down.
const FIGURINE_DY: f32 = 1.5;
/// The fold chevron of a folded card.
const CHEVRON_FOLDED: &str = "\u{25B8}";
/// The fold chevron of an open card.
const CHEVRON_OPEN: &str = "\u{25BE}";
/// The smallest paragraph width worth laying out.
const MIN_WIDTH: f32 = 40.0;
/// Width given to a one-line paragraph: it never wraps.
const NO_WRAP_WIDTH: f32 = 10_000.0;

/// The arguments of [`TableCache::sync`].
#[derive(Debug, Clone, Copy)]
pub struct TableInputs<'a> {
    /// The rows.
    pub table: &'a NotationTable,
    /// The document the comment and card rows point into.
    pub doc: &'a NotationDoc,
    /// Translated words.
    pub labels: &'a Labels,
    /// Size, colors and weights.
    pub style: &'a TextStyle,
    /// The width of the column of rows, scroll bar excluded.
    pub width: f32,
    /// The first moves of the cards that are folded.
    pub collapsed: &'a HashSet<NodeId>,
}

/// Where a move is in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// In a cell of a move row.
    Cell {
        /// Index of the row.
        row: usize,
        /// The column.
        side: Side,
    },
    /// In a span of a card (or of a paragraph of the document).
    Span {
        /// Index of the row.
        row: usize,
        /// Index of the span.
        span: usize,
    },
}

/// How much of a cell a rectangle covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// The text of the cell with some padding: the pill of the current move.
    Pill,
    /// The whole column of the cell: hover and menu target.
    Cell,
}

/// The x of the columns, from the left edge of the rows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Columns {
    /// The move number.
    pub number: f32,
    /// White's moves.
    pub white: f32,
    /// Black's moves.
    pub black: f32,
    /// Width of a column of moves.
    pub cell: f32,
    /// The time column.
    pub time: f32,
    /// Where the comments start (the white column).
    pub text: f32,
    /// Width of all rows.
    pub total: f32,
}

impl Columns {
    /// The columns of a table `width` wide.
    pub fn of(width: f32, style: &TextStyle, times: bool) -> Columns {
        let number_w = (style.size * 2.6).round();
        let time_w = if times { TIME_COLUMN } else { 0.0 };
        let available = (width - 2.0 * PADDING - number_w - time_w).max(0.0);
        let cell = (available / 2.0).floor().max(MIN_CELL);
        let white = PADDING + number_w;
        let black = white + cell;
        Columns {
            number: PADDING,
            white,
            black,
            cell,
            time: black + cell + 4.0,
            text: white,
            total: width,
        }
    }

    fn column(&self, side: Side) -> f32 {
        match side {
            Side::White => self.white,
            Side::Black => self.black,
        }
    }
}

/// What a move row shows, kept to shape it later.
#[derive(Debug, Clone)]
struct MovesData {
    number: u32,
    white: Option<Cell>,
    black: Option<Cell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Glyph,
    Text,
    Nag,
}

/// A small paragraph of a cell and where it sits.
#[derive(Debug)]
struct PartShape<P> {
    paragraph: P,
    /// Its own color; the current move overrides it.
    color: Color,
    /// From the start of the text of the cell.
    x: f32,
    width: f32,
    /// Added to the vertical centering (the glyph font sits high).
    dy: f32,
    /// An annotation symbol: a click on it has its own meaning.
    nag: bool,
}

#[derive(Debug)]
struct CellShape<P> {
    parts: Vec<PartShape<P>>,
    /// Width of all the parts.
    width: f32,
}

#[derive(Debug)]
struct TimeShape<P> {
    paragraph: P,
    spent: u32,
    derived: bool,
}

/// The paragraphs of a move row.
#[derive(Debug)]
struct Shaped<P> {
    number: P,
    /// The `…` of a row that starts with Black's move.
    dots: Option<P>,
    white: Option<CellShape<P>>,
    black: Option<CellShape<P>>,
    times: [Option<TimeShape<P>>; 2],
}

#[derive(Debug, Clone, PartialEq)]
enum FlowKind {
    Header(Vec<HeaderLine>),
    Comment,
    /// A variation card; `toggle` is the first move (what folds it), `None`
    /// for the placeholder.
    Card {
        toggle: Option<NodeId>,
        collapsed: bool,
    },
    Result,
    Empty,
}

/// A row of text of any height.
#[derive(Debug)]
struct Flow<P> {
    kind: FlowKind,
    paragraph: P,
    /// What a click on each span means (parallel to the spans).
    hits: Vec<Option<Hit>>,
    /// Left edge of the paragraph.
    x: f32,
    /// From the top of the row to the top of the paragraph.
    top: f32,
}

#[derive(Debug)]
enum Body<P> {
    Moves {
        data: MovesData,
        shaped: Option<Shaped<P>>,
    },
    Flow(Flow<P>),
}

#[derive(Debug)]
struct TRow<P> {
    key: RowKey,
    rev: u64,
    indent: u8,
    collapsed: bool,
    y: f32,
    height: f32,
    /// A move row with the tinted background.
    zebra: bool,
    body: Body<P>,
}

/// Everything the shaped text depends on besides the rows.
#[derive(Debug, Clone, PartialEq)]
struct Sig {
    style: TextStyle,
    labels: Labels,
    times: bool,
}

/// Shapes the paragraphs of a row. Kept apart from the rows so the two can
/// be borrowed together.
#[derive(Debug)]
struct Shaper {
    style: TextStyle,
    /// Paragraphs shaped so far.
    shaped: usize,
    /// Move rows shaped so far.
    rows: usize,
}

fn text_paragraph<P: Paragraph<Font = Font>>(
    text: &str,
    font: Font,
    size: f32,
    line_height: f32,
) -> P {
    P::with_text(Text {
        content: text,
        bounds: Size::new(NO_WRAP_WIDTH, NO_WRAP_WIDTH),
        size: Pixels(size),
        line_height: LineHeight::Relative(line_height),
        font,
        align_x: Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
    })
}

impl Shaper {
    fn paragraph<P: Paragraph<Font = Font>>(&mut self, text: &str, font: Font, size: f32) -> P {
        self.shaped += 1;
        text_paragraph(text, font, size, self.style.line_height)
    }

    fn cell<P: Paragraph<Font = Font>>(&mut self, cell: &Cell, side: Side) -> CellShape<P> {
        let style = self.style;
        let palette = style.palette;
        let mut parts = Vec::with_capacity(4);
        let mut x = 0.0;
        let mut push =
            |shaper: &mut Shaper, text: &str, font: Font, color: Color, kind: PartKind| {
                let (size, dy) = match kind {
                    PartKind::Glyph => (style.size * FIGURINE_SCALE, FIGURINE_DY),
                    PartKind::Text | PartKind::Nag => (style.size, 0.0),
                };
                let paragraph: P = shaper.paragraph(text, font, size);
                let width = paragraph.min_bounds().width;
                let nag = kind == PartKind::Nag;
                let gap = if nag { NAG_GAP } else { 0.0 };
                parts.push(PartShape {
                    paragraph,
                    color,
                    x: x + gap,
                    width,
                    dy,
                    nag,
                });
                x += gap + width;
            };
        let figurine = match side {
            Side::White => palette.white_piece,
            Side::Black => palette.black_piece,
        };
        let text_font = if style.bold_main {
            fonts::NOTO_SANS_BOLD
        } else {
            Font::DEFAULT
        };
        for part in san_parts(&cell.san, style.figurines) {
            match &part {
                Part::Glyph(_) => push(self, &part.text(), GLYPH_FONT, figurine, PartKind::Glyph),
                Part::Text(text) => push(self, text, text_font, palette.main, PartKind::Text),
            }
        }
        for nag in &cell.nags {
            let label = nag_label(*nag, style.symbolic_nags);
            push(
                self,
                &label,
                Font::DEFAULT,
                palette.nag_color(*nag),
                PartKind::Nag,
            );
        }
        CellShape { parts, width: x }
    }

    fn time<P: Paragraph<Font = Font>>(&mut self, cell: &Cell) -> Option<TimeShape<P>> {
        let spent = cell.times.spent?;
        let mut text = format_time(spent);
        if cell.times.spent_is_derived {
            text.insert(0, '~');
        }
        let size = (self.style.size - 3.0).max(8.0);
        Some(TimeShape {
            paragraph: self.paragraph(&text, Font::DEFAULT, size),
            spent,
            derived: cell.times.spent_is_derived,
        })
    }

    fn moves<P: Paragraph<Font = Font>>(&mut self, data: &MovesData, times: bool) -> Shaped<P> {
        self.rows += 1;
        let size = self.style.size;
        let number = self.paragraph(&format!("{}.", data.number), Font::DEFAULT, size);
        let dots = data
            .white
            .is_none()
            .then(|| self.paragraph("\u{2026}", Font::DEFAULT, size));
        let white = data.white.as_ref().map(|c| self.cell(c, Side::White));
        let black = data.black.as_ref().map(|c| self.cell(c, Side::Black));
        let clocks = if times {
            [
                data.white.as_ref().and_then(|c| self.time(c)),
                data.black.as_ref().and_then(|c| self.time(c)),
            ]
        } else {
            [None, None]
        };
        Shaped {
            number,
            dots,
            white,
            black,
            times: clocks,
        }
    }
}

/// `Ns` below a minute, `m:ss` from then on (D250).
pub fn format_time(seconds: u32) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// The width of a time bar for `spent` seconds when the longest move took
/// `max` (D250): proportional, from [`TIME_BAR_MIN`] to [`TIME_BAR_MAX`].
pub fn time_bar_width(spent: u32, max: u32) -> f32 {
    if max == 0 {
        return TIME_BAR_MIN;
    }
    // Both counts are far below 2^24 for any game, so the cast is exact.
    #[allow(clippy::cast_precision_loss)]
    let share = spent as f32 / max as f32;
    (TIME_BAR_MAX * share).clamp(TIME_BAR_MIN, TIME_BAR_MAX)
}

/// The rows of a table. See the module documentation.
#[derive(Debug)]
pub struct TableCache<P> {
    rows: Vec<TRow<P>>,
    sig: Option<Sig>,
    revision: Option<u64>,
    width: f32,
    collapsed: HashSet<NodeId>,
    columns: Columns,
    times: bool,
    row_height: f32,
    max_spent: u32,
    /// Which place each move of the table has.
    index: HashMap<NodeId, Place>,
    content_height: f32,
    /// Grows each time the layout changed (markers cached against it).
    stamp: u64,
    shaper: Shaper,
    /// The fold chevrons of the cards: folded, then open.
    chevrons: Option<[P; 2]>,
    /// Paragraphs laid out again for a new width, without shaping.
    resized: usize,
}

impl<P> Default for TableCache<P> {
    fn default() -> TableCache<P> {
        TableCache {
            rows: Vec::new(),
            sig: None,
            revision: None,
            width: 0.0,
            collapsed: HashSet::new(),
            columns: Columns::default(),
            times: false,
            row_height: ROW_HEIGHT,
            max_spent: 0,
            index: HashMap::new(),
            content_height: 0.0,
            stamp: 0,
            shaper: Shaper {
                style: TextStyle {
                    size: 15.0,
                    line_height: 1.35,
                    palette: crate::theme::notation_colors::NotationPalette::of(&iced::Theme::Dark),
                    symbolic_nags: true,
                    bold_main: false,
                    figurines: false,
                },
                shaped: 0,
                rows: 0,
            },
            chevrons: None,
            resized: 0,
        }
    }
}

impl<P: Paragraph<Font = Font>> TableCache<P> {
    /// How many paragraphs were shaped since the cache was created.
    pub fn shaped(&self) -> usize {
        self.shaper.shaped
    }

    /// How many move rows were shaped.
    pub fn rows_shaped(&self) -> usize {
        self.shaper.rows
    }

    /// How many paragraphs were laid out again for a new width.
    pub fn resized(&self) -> usize {
        self.resized
    }

    /// Changes whenever rows or positions changed.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The height of everything, padding included.
    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there are no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The height of a move row.
    pub fn row_height(&self) -> f32 {
        self.row_height
    }

    /// The columns of the current width.
    pub fn columns(&self) -> Columns {
        self.columns
    }

    /// Whether the time column is on.
    pub fn has_times(&self) -> bool {
        self.times
    }

    /// The longest time spent on a move, seconds (0 without times).
    pub fn max_spent(&self) -> u32 {
        self.max_spent
    }

    /// The top and height of row `row`.
    pub fn row_span(&self, row: usize) -> Option<(f32, f32)> {
        self.rows.get(row).map(|r| (r.y, r.height))
    }

    /// Whether `row` is a move row (not a comment, card or other text).
    pub fn is_move_row(&self, row: usize) -> bool {
        matches!(
            self.rows.get(row).map(|r| &r.body),
            Some(Body::Moves { .. })
        )
    }

    /// Whether the move row `row` has its paragraphs.
    pub fn is_shaped(&self, row: usize) -> bool {
        matches!(
            self.rows.get(row).map(|r| &r.body),
            Some(Body::Moves {
                shaped: Some(_),
                ..
            })
        )
    }

    /// Brings the rows up to date with `input`. Returns whether anything
    /// changed. Cheap when nothing did.
    pub fn sync(&mut self, input: &TableInputs<'_>) -> bool {
        let times = input.table.has_times && input.width + 8.0 >= TIME_MIN_PANEL_WIDTH;
        let sig_same = self.sig.as_ref().is_some_and(|sig| {
            sig.style == *input.style && sig.labels == *input.labels && sig.times == times
        });
        let width = input.width.max(MIN_WIDTH + 2.0 * PADDING);
        if sig_same
            && self.revision == Some(input.table.revision)
            && (self.width - width).abs() < 0.5
            && self.collapsed == *input.collapsed
        {
            return false;
        }
        self.prepare(input, sig_same, times, width);
        let mut rows = self.build_rows(input, width);
        self.place(&mut rows);
        self.index = build_index(&rows);
        self.rows = rows;
        self.revision = Some(input.table.revision);
        self.width = width;
        self.collapsed.clone_from(input.collapsed);
        self.stamp += 1;
        true
    }

    /// Brings the measures that every row depends on up to date; forgets the
    /// rows when the style changed.
    fn prepare(&mut self, input: &TableInputs<'_>, sig_same: bool, times: bool, width: f32) {
        if !sig_same {
            self.rows.clear();
            self.sig = Some(Sig {
                style: *input.style,
                labels: input.labels.clone(),
                times,
            });
            self.shaper.style = *input.style;
            self.chevrons = None;
        }
        if self.chevrons.is_none() {
            let size = input.style.size;
            self.chevrons = Some([
                self.shaper.paragraph(CHEVRON_FOLDED, Font::DEFAULT, size),
                self.shaper.paragraph(CHEVRON_OPEN, Font::DEFAULT, size),
            ]);
        }
        self.times = times;
        self.columns = Columns::of(width, input.style, times);
        self.row_height = if times { ROW_HEIGHT_TIMES } else { ROW_HEIGHT };
        self.max_spent = if times { max_spent(input.table) } else { 0 };
    }

    /// The rows of the table, reusing the ones of the last sync whose
    /// revision did not change.
    fn build_rows(&mut self, input: &TableInputs<'_>, width: f32) -> Vec<TRow<P>> {
        let mut old: HashMap<RowKey, TRow<P>> = HashMap::with_capacity(self.rows.len());
        let mut old_header = None;
        let mut old_empty = None;
        for row in self.rows.drain(..) {
            match &row.body {
                Body::Flow(Flow {
                    kind: FlowKind::Header(_),
                    ..
                }) => old_header = Some(row),
                Body::Flow(Flow {
                    kind: FlowKind::Empty,
                    ..
                }) => old_empty = Some(row),
                _ => {
                    old.insert(row.key, row);
                }
            }
        }
        let mut rows = Vec::with_capacity(input.table.rows.len() + 3);
        rows.extend(self.header_row(input, width, old_header));
        rows.extend(self.empty_row(input, width, old_empty));
        for source in &input.table.rows {
            let folded = match &source.content {
                RowContent::Variation { .. } => source
                    .key
                    .node
                    .is_some_and(|node| input.collapsed.contains(&node)),
                _ => false,
            };
            let kept = old.remove(&source.key).filter(|row| {
                row.rev == source.rev && row.indent == source.indent && row.collapsed == folded
            });
            let row = match (kept, &source.content) {
                (Some(row), RowContent::Moves { .. }) => row,
                (Some(row), _) => self.refit(row, width),
                (None, content) => self.new_row(source, content, folded, input, width),
            };
            rows.push(row);
        }
        rows
    }

    /// The tags of the game, when the header is on.
    fn header_row(
        &mut self,
        input: &TableInputs<'_>,
        width: f32,
        old: Option<TRow<P>>,
    ) -> Option<TRow<P>> {
        if input.doc.header.is_empty() {
            return None;
        }
        let ident = FlowKind::Header(input.doc.header.clone());
        let kept = old.filter(|row| match &row.body {
            Body::Flow(flow) => flow.kind == ident,
            Body::Moves { .. } => false,
        });
        if let Some(row) = kept {
            return Some(self.refit(row, width));
        }
        let spans = header_spans(&input.doc.header, input.labels, input.style);
        let hits = vec![None; spans.len()];
        Some(self.flow_row(SPECIAL_KEY, 0, 0, ident, &spans, hits, width, input.style))
    }

    /// The "no moves yet" message, when the table has nothing but (maybe)
    /// the result.
    fn empty_row(
        &mut self,
        input: &TableInputs<'_>,
        width: f32,
        old: Option<TRow<P>>,
    ) -> Option<TRow<P>> {
        let only_result = input
            .table
            .rows
            .iter()
            .all(|row| matches!(row.content, RowContent::Result(_)));
        if !only_result {
            return None;
        }
        if let Some(row) = old {
            return Some(self.refit(row, width));
        }
        let spans = [empty_span(input.labels, input.style)];
        let kind = FlowKind::Empty;
        Some(self.flow_row(
            SPECIAL_KEY,
            0,
            0,
            kind,
            &spans,
            vec![None],
            width,
            input.style,
        ))
    }

    fn new_row(
        &mut self,
        source: &mychess_app::Row,
        content: &RowContent,
        folded: bool,
        input: &TableInputs<'_>,
        width: f32,
    ) -> TRow<P> {
        let style = input.style;
        let key = source.key;
        let (rev, indent) = (source.rev, source.indent);
        match content {
            RowContent::Moves {
                number,
                white,
                black,
            } => TRow {
                key,
                rev,
                indent,
                collapsed: false,
                y: 0.0,
                height: self.row_height,
                zebra: false,
                body: Body::Moves {
                    data: MovesData {
                        number: *number,
                        white: white.clone(),
                        black: black.clone(),
                    },
                    shaped: None,
                },
            },
            RowContent::Comment { block } => {
                let (spans, hits) = input
                    .doc
                    .blocks
                    .get(*block)
                    .map(|b| block_spans_with(b, style, false))
                    .unwrap_or_default();
                self.flow_row(
                    key,
                    rev,
                    indent,
                    FlowKind::Comment,
                    &spans,
                    hits,
                    width,
                    style,
                )
            }
            RowContent::Variation { block } => {
                let blocks = input.doc.blocks.get(*block);
                let (spans, hits) = match blocks {
                    Some(b) if folded => folded_spans(b, style),
                    Some(b) => block_spans_with(b, style, true),
                    None => (Vec::new(), Vec::new()),
                };
                let kind = FlowKind::Card {
                    toggle: key.node,
                    collapsed: folded,
                };
                self.flow_row(key, rev, indent, kind, &spans, hits, width, style)
            }
            RowContent::Pending { .. } => {
                let spans = [pending_span(style)];
                let kind = FlowKind::Card {
                    toggle: None,
                    collapsed: false,
                };
                self.flow_row(key, rev, indent, kind, &spans, vec![None], width, style)
            }
            RowContent::Result(result) => {
                let spans = [result_span(*result, style)];
                self.flow_row(
                    key,
                    rev,
                    indent,
                    FlowKind::Result,
                    &spans,
                    vec![None],
                    width,
                    style,
                )
            }
            // `RowContent` is non-exhaustive: a newer application may add
            // rows; they are drawn as an empty move row rather than guessed.
            _ => TRow {
                key,
                rev,
                indent,
                collapsed: false,
                y: 0.0,
                height: self.row_height,
                zebra: false,
                body: Body::Moves {
                    data: MovesData {
                        number: 0,
                        white: None,
                        black: None,
                    },
                    shaped: None,
                },
            },
        }
    }

    /// The left edge and the width of the paragraph of a flow row.
    fn flow_box(&self, kind: &FlowKind, indent: u8, width: f32) -> (f32, f32) {
        let level = f32::from(indent.min(MAX_CARD_INDENT));
        match kind {
            FlowKind::Card { .. } => {
                let card_x = PADDING + level * CARD_STEP;
                let x = card_x + CARD_BAR + GUTTER;
                (x, (width - PADDING - x - CARD_PAD_R).max(MIN_WIDTH))
            }
            FlowKind::Comment => {
                // Under the text of the white column; the main line's
                // comments are one level in (the document's indent), so
                // only the levels beyond that move it.
                let deeper = f32::from(indent.saturating_sub(1).min(MAX_CARD_INDENT));
                let x = self.columns.text + PILL_PAD + deeper * CARD_STEP;
                (x, (width - PADDING - x).max(MIN_WIDTH))
            }
            FlowKind::Result | FlowKind::Empty => {
                let x = self.columns.text + PILL_PAD;
                (x, (width - PADDING - x).max(MIN_WIDTH))
            }
            FlowKind::Header(_) => (PADDING, (width - 2.0 * PADDING).max(MIN_WIDTH)),
        }
    }

    /// Margin and padding above the paragraph of a flow row, and below it.
    fn flow_pads(kind: &FlowKind) -> (f32, f32) {
        match kind {
            FlowKind::Card { .. } => (CARD_MARGIN + CARD_PAD_V, CARD_MARGIN + CARD_PAD_V),
            FlowKind::Comment => (COMMENT_MARGIN, COMMENT_MARGIN),
            FlowKind::Result | FlowKind::Empty => (CARD_MARGIN, CARD_MARGIN),
            FlowKind::Header(_) => (0.0, 4.0),
        }
    }

    /// The size of the text of a kind of row.
    fn flow_size(kind: &FlowKind, style: &TextStyle) -> f32 {
        match kind {
            FlowKind::Comment => (style.size - 1.0).max(8.0),
            _ => style.size,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn flow_row(
        &mut self,
        key: RowKey,
        rev: u64,
        indent: u8,
        kind: FlowKind,
        spans: &[NotationSpan],
        hits: Vec<Option<Hit>>,
        width: f32,
        style: &TextStyle,
    ) -> TRow<P> {
        let (x, avail) = self.flow_box(&kind, indent, width);
        let size = Self::flow_size(&kind, style);
        self.shaper.shaped += 1;
        let paragraph: P = shape_spans(
            spans,
            Size::new(avail, f32::INFINITY),
            size,
            style.line_height,
        );
        let (above, below) = Self::flow_pads(&kind);
        let collapsed = matches!(
            kind,
            FlowKind::Card {
                collapsed: true,
                ..
            }
        );
        TRow {
            key,
            rev,
            indent,
            collapsed,
            y: 0.0,
            height: above + paragraph.min_bounds().height + below,
            zebra: false,
            body: Body::Flow(Flow {
                kind,
                paragraph,
                hits,
                x,
                top: above,
            }),
        }
    }

    /// Lays a kept row out again if the width changed.
    fn refit(&mut self, mut row: TRow<P>, width: f32) -> TRow<P> {
        if let Body::Flow(flow) = &mut row.body {
            let (x, avail) = self.flow_box(&flow.kind, row.indent, width);
            if (flow.paragraph.bounds().width - avail).abs() > 0.5 {
                flow.paragraph.resize(Size::new(avail, f32::INFINITY));
                let (above, below) = Self::flow_pads(&flow.kind);
                row.height = above + flow.paragraph.min_bounds().height + below;
                self.resized += 1;
            }
            flow.x = x;
        }
        row
    }

    /// Stacks the rows from the top.
    fn place(&mut self, rows: &mut [TRow<P>]) {
        let mut y = PADDING;
        let mut zebra = false;
        for row in rows.iter_mut() {
            row.y = y;
            match row.body {
                Body::Moves { .. } => {
                    row.height = self.row_height;
                    row.zebra = zebra;
                    zebra = !zebra;
                }
                Body::Flow(_) => row.zebra = false,
            }
            y += row.height;
        }
        self.content_height = y + PADDING;
    }

    /// Shapes the move rows near `top..bottom` of the content: the window
    /// and half a window on each side. Rows already shaped cost nothing.
    pub fn ensure_shaped(&mut self, top: f32, bottom: f32) {
        let margin = (bottom - top).max(0.0) / 2.0;
        let first = self.rows.partition_point(|r| r.y + r.height < top - margin);
        let last = self.rows.partition_point(|r| r.y <= bottom + margin);
        let times = self.times;
        let Some(window) = self.rows.get_mut(first..last.max(first)) else {
            return;
        };
        for row in window {
            if let Body::Moves { data, shaped } = &mut row.body
                && shaped.is_none()
            {
                *shaped = Some(self.shaper.moves(data, times));
            }
        }
    }

    /// Shapes one move row if it is not shaped yet.
    pub fn ensure_row(&mut self, row: usize) {
        let times = self.times;
        if let Some(TRow {
            body: Body::Moves { data, shaped },
            ..
        }) = self.rows.get_mut(row)
            && shaped.is_none()
        {
            *shaped = Some(self.shaper.moves(data, times));
        }
    }

    /// The rows that touch the vertical range `top..bottom` of the content.
    pub fn visible(&self, top: f32, bottom: f32) -> Range<usize> {
        let first = self.rows.partition_point(|r| r.y + r.height < top);
        let last = self.rows.partition_point(|r| r.y <= bottom);
        first..last.max(first)
    }

    /// Where the move of `node` is.
    pub fn find_move(&self, node: NodeId) -> Option<Place> {
        self.index.get(&node).copied()
    }

    /// The first move of the table, for the start marker.
    pub fn first_move(&self) -> Option<Place> {
        self.rows
            .iter()
            .enumerate()
            .find_map(|(row, r)| match &r.body {
                Body::Moves { data, .. } => {
                    if data.white.is_some() {
                        Some(Place::Cell {
                            row,
                            side: Side::White,
                        })
                    } else if data.black.is_some() {
                        Some(Place::Cell {
                            row,
                            side: Side::Black,
                        })
                    } else {
                        None
                    }
                }
                Body::Flow(_) => None,
            })
    }

    /// The box of a column of a move row (the area that answers a click).
    fn cell_box(&self, row: &TRow<P>, side: Side) -> Rectangle {
        Rectangle {
            x: self.columns.column(side),
            y: row.y,
            width: self.columns.cell,
            height: row.height,
        }
    }

    /// The rectangle highlighted behind a cell.
    fn cell_mark(&self, row: &TRow<P>, side: Side) -> Rectangle {
        let boxed = self.cell_box(row, side);
        Rectangle {
            x: boxed.x,
            y: boxed.y + 2.0,
            width: (boxed.width - 2.0).max(0.0),
            height: (boxed.height - 4.0).max(0.0),
        }
    }

    /// The pill behind the text of a cell.
    fn pill(&self, row: &TRow<P>, side: Side, text_width: f32) -> Rectangle {
        let mark = self.cell_mark(row, side);
        Rectangle {
            width: (text_width + 2.0 * PILL_PAD).min(mark.width),
            ..mark
        }
    }

    /// The rectangles to highlight for the move of `node`, shaping its row if
    /// needed, and the cell it is in. A move inside a card gives the
    /// rectangles of its text.
    pub fn move_rects(&mut self, node: NodeId, mark: Mark) -> Option<(Vec<Rectangle>, Place)> {
        let place = self.find_move(node)?;
        match place {
            Place::Cell { row, side } => {
                self.ensure_row(row);
                let entry = self.rows.get(row)?;
                let rect = match mark {
                    Mark::Cell => self.cell_mark(entry, side),
                    Mark::Pill => {
                        let Body::Moves {
                            shaped: Some(shaped),
                            ..
                        } = &entry.body
                        else {
                            return None;
                        };
                        let cell = match side {
                            Side::White => shaped.white.as_ref(),
                            Side::Black => shaped.black.as_ref(),
                        }?;
                        self.pill(entry, side, cell.width)
                    }
                };
                Some((vec![rect], place))
            }
            Place::Span { row, span } => Some((self.group_rects(row, span), place)),
        }
    }

    /// The rectangles of the whole item at `span` of a flow row, one per
    /// line, in content coordinates.
    pub fn group_rects(&self, row: usize, span: usize) -> Vec<Rectangle> {
        let Some(TRow {
            y,
            body: Body::Flow(flow),
            ..
        }) = self.rows.get(row)
        else {
            return Vec::new();
        };
        super::layout::group(&flow.hits, span)
            .flat_map(|s| flow.paragraph.span_bounds(s))
            .map(|r| Rectangle {
                x: r.x + flow.x,
                y: r.y + y + flow.top,
                ..r
            })
            .collect()
    }

    /// The rectangles to highlight under a located item.
    pub fn hit_rects(&self, at: &Located) -> Vec<Rectangle> {
        let Some(row) = self.rows.get(at.entry) else {
            return Vec::new();
        };
        match &row.body {
            Body::Moves { shaped, .. } => {
                let side = if at.span.is_multiple_of(2) {
                    Side::White
                } else {
                    Side::Black
                };
                let part = at.span / 2;
                if part == 0 {
                    return vec![self.cell_mark(row, side)];
                }
                // An annotation symbol: the rectangle of its paragraph.
                let nag_rect = shaped.as_ref().and_then(|s| {
                    let cell = match side {
                        Side::White => s.white.as_ref(),
                        Side::Black => s.black.as_ref(),
                    }?;
                    let part = cell.parts.get(part - 1)?;
                    Some(self.part_rect(row, side, part))
                });
                nag_rect.into_iter().collect()
            }
            Body::Flow(Flow {
                kind: FlowKind::Card { collapsed, .. },
                ..
            }) if matches!(at.hit, Some(Hit::Card(_))) => {
                let rect = self.card_rect(row);
                // A folded card is all header; an open one has its gutter.
                let width = if *collapsed {
                    rect.width
                } else {
                    CARD_BAR + GUTTER
                };
                vec![Rectangle { width, ..rect }]
            }
            Body::Flow(_) => self.group_rects(at.entry, at.span),
        }
    }

    /// The rectangle of a part of a cell, with a little room around it.
    fn part_rect(&self, row: &TRow<P>, side: Side, part: &PartShape<P>) -> Rectangle {
        let pill = self.cell_mark(row, side);
        Rectangle {
            x: self.columns.column(side) + PILL_PAD + part.x - 2.0,
            y: pill.y + 2.0,
            width: part.width + 4.0,
            height: (pill.height - 4.0).max(0.0),
        }
    }

    /// The rectangle of a card (its frame), in content coordinates.
    fn card_rect(&self, row: &TRow<P>) -> Rectangle {
        let level = f32::from(row.indent.min(MAX_CARD_INDENT));
        let x = PADDING + level * CARD_STEP;
        Rectangle {
            x,
            y: row.y + CARD_MARGIN,
            width: (self.columns.total - PADDING - x).max(0.0),
            height: (row.height - 2.0 * CARD_MARGIN).max(0.0),
        }
    }

    /// What is at `point` (content coordinates). A move row answers with its
    /// whole column; `span` is `side + 2 * k` with `k = 0` for the move and
    /// `k` for the `k`-th annotation symbol.
    pub fn locate(&self, point: Point) -> Option<Located> {
        let index = row_at(&self.rows, point.y, |r| (r.y, r.height))?;
        let row = self.rows.get(index)?;
        match &row.body {
            Body::Moves { data, shaped } => {
                for side in [Side::White, Side::Black] {
                    let (cell, shape) = match side {
                        Side::White => (
                            data.white.as_ref(),
                            shaped.as_ref().and_then(|s| s.white.as_ref()),
                        ),
                        Side::Black => (
                            data.black.as_ref(),
                            shaped.as_ref().and_then(|s| s.black.as_ref()),
                        ),
                    };
                    let boxed = self.cell_box(row, side);
                    if cell.is_none() || point.x < boxed.x || point.x >= boxed.x + boxed.width {
                        continue;
                    }
                    let node = cell?.node;
                    let side_bit = usize::from(side == Side::Black);
                    let nag = shape.and_then(|cell| {
                        cell.parts
                            .iter()
                            .enumerate()
                            .filter(|(_, part)| part.nag)
                            .find(|(_, part)| {
                                let rect = self.part_rect(row, side, part);
                                point.x >= rect.x && point.x < rect.x + rect.width
                            })
                            .map(|(i, _)| i + 1)
                    });
                    return Some(match nag {
                        Some(k) => Located {
                            entry: index,
                            span: side_bit + 2 * k,
                            hit: Some(Hit::Nag(node)),
                        },
                        None => Located {
                            entry: index,
                            span: side_bit,
                            hit: Some(Hit::Move(node)),
                        },
                    });
                }
                None
            }
            Body::Flow(flow) => {
                if let FlowKind::Card {
                    toggle: Some(node),
                    collapsed,
                } = &flow.kind
                {
                    let card = self.card_rect(row);
                    let in_card = point.x >= card.x
                        && point.x < card.x + card.width
                        && point.y >= card.y
                        && point.y < card.y + card.height;
                    let in_gutter = in_card && point.x < card.x + CARD_BAR + GUTTER;
                    if in_gutter || (*collapsed && in_card) {
                        return Some(Located {
                            entry: index,
                            span: 0,
                            hit: Some(Hit::Card(*node)),
                        });
                    }
                }
                let local = Point::new(point.x - flow.x, point.y - row.y - flow.top);
                let span = flow.paragraph.hit_span(local)?;
                let hit = flow.hits.get(span).copied().flatten();
                Some(Located {
                    entry: index,
                    span: super::layout::group(&flow.hits, span).start,
                    hit,
                })
            }
        }
    }
}

/// The key of the rows that are not in the application's table (the header
/// and the "no moves" message). They are found by their kind, never by key.
const SPECIAL_KEY: RowKey = RowKey {
    kind: mychess_app::RowKind::Result,
    node: None,
};

/// The longest time spent on a move of the table.
fn max_spent(table: &NotationTable) -> u32 {
    table
        .rows
        .iter()
        .filter_map(|row| match &row.content {
            RowContent::Moves { white, black, .. } => Some([white.as_ref(), black.as_ref()]),
            _ => None,
        })
        .flatten()
        .flatten()
        .filter_map(|cell| cell.times.spent)
        .max()
        .unwrap_or(0)
}

fn build_index<P>(rows: &[TRow<P>]) -> HashMap<NodeId, Place> {
    let mut index = HashMap::with_capacity(rows.len() * 2);
    for (row, entry) in rows.iter().enumerate() {
        match &entry.body {
            Body::Moves { data, .. } => {
                if let Some(cell) = &data.white {
                    index.insert(
                        cell.node,
                        Place::Cell {
                            row,
                            side: Side::White,
                        },
                    );
                }
                if let Some(cell) = &data.black {
                    index.insert(
                        cell.node,
                        Place::Cell {
                            row,
                            side: Side::Black,
                        },
                    );
                }
            }
            Body::Flow(flow) => {
                // The first span of each move of a card.
                for (span, hit) in flow.hits.iter().enumerate() {
                    if let Some(Hit::Move(node)) = hit {
                        index.entry(*node).or_insert(Place::Span { row, span });
                    }
                }
            }
        }
    }
    index
}

/// The span of a placeholder card: `( … )`.
fn pending_span(style: &TextStyle) -> NotationSpan {
    iced::advanced::text::Span::new("( \u{2026} )").color(style.palette.muted)
}

/// A folded card shows its first move and an ellipsis.
fn folded_spans(
    block: &mychess_app::Block,
    style: &TextStyle,
) -> (Vec<NotationSpan>, Vec<Option<Hit>>) {
    let first_move = block
        .inlines
        .iter()
        .position(|i| matches!(i, Inline::Move { .. }));
    let mut keep = Vec::with_capacity(2);
    if let Some(at) = first_move {
        if at > 0
            && let Some(number @ Inline::Number(_)) = block.inlines.get(at - 1)
        {
            keep.push(number.clone());
        }
        if let Some(mv) = block.inlines.get(at) {
            keep.push(mv.clone());
        }
    }
    let short = mychess_app::Block {
        inlines: keep,
        key: block.key,
        rev: block.rev,
        indent: block.indent,
        kind: block.kind,
    };
    let (mut spans, mut hits) = block_spans_with(&short, style, false);
    spans.push(iced::advanced::text::Span::new(" \u{2026}").color(style.palette.muted));
    hits.push(None);
    (spans, hits)
}

#[cfg(test)]
#[path = "table_layout/tests.rs"]
mod tests;
