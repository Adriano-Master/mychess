//! The shaped paragraphs of the document and where they are (spec M2-4,
//! D168).
//!
//! [`LayoutCache`] keeps one paragraph per block of the
//! [`NotationDoc`]. A sync shapes again only the
//! blocks whose revision changed (or all of them when the text style
//! changed) and lays out again, without shaping, the ones whose width
//! changed. Moving the cursor does not reach this module at all.

use std::collections::HashMap;

use iced::advanced::text::{Alignment, LineHeight, Paragraph, Shaping, Text, Wrapping};
use iced::{Font, Pixels, Point, Rectangle, Size, alignment};
use mychess_app::{Block, BlockKey, HeaderLine, NotationDoc};
use mychess_core::{GameResult, NodeId};

use super::hit::{Hit, row_at};
use super::style::{
    Labels, NotationSpan, PADDING, TextStyle, block_spans, empty_span, header_spans, result_span,
};

/// What an entry shows; two entries with the same identity have the same
/// text, so the shaped paragraph can be kept.
#[derive(Debug, Clone, PartialEq)]
enum Ident {
    Header(Vec<HeaderLine>),
    Block { key: BlockKey, rev: u64, indent: u8 },
    Result(GameResult),
    Empty,
}

/// A shaped paragraph and its place in the column.
#[derive(Debug)]
struct Entry<P> {
    ident: Ident,
    paragraph: P,
    /// What a click on each span means (parallel to the spans).
    hits: Vec<Option<Hit>>,
    /// Left edge, from the left edge of the content.
    x: f32,
    /// Top, from the top of the content.
    y: f32,
    height: f32,
}

/// Everything the shaped text depends on besides the document.
#[derive(Debug, Clone, PartialEq)]
struct StyleSig {
    style: TextStyle,
    labels: Labels,
}

/// The arguments of [`LayoutCache::sync`].
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// The document.
    pub doc: &'a NotationDoc,
    /// Translated words of the header and the empty game.
    pub labels: &'a Labels,
    /// Size, colors and weights.
    pub style: &'a TextStyle,
    /// The width available to the whole column, scroll bar included.
    pub width: f32,
}

/// Where a point landed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// The paragraph.
    pub entry: usize,
    /// The span of the paragraph.
    pub span: usize,
    /// What a click there means, if anything.
    pub hit: Option<Hit>,
}

/// The paragraphs of a document. See the module documentation.
#[derive(Debug)]
pub struct LayoutCache<P> {
    entries: Vec<Entry<P>>,
    sig: Option<StyleSig>,
    revision: Option<u64>,
    width: f32,
    content_height: f32,
    /// Grows each time the layout changed (markers cached against it).
    stamp: u64,
    /// Paragraphs shaped so far (for tests and the performance notes).
    shaped: usize,
    /// Paragraphs laid out again for a new width, without shaping.
    resized: usize,
}

impl<P> Default for LayoutCache<P> {
    fn default() -> LayoutCache<P> {
        LayoutCache {
            entries: Vec::new(),
            sig: None,
            revision: None,
            width: 0.0,
            content_height: 0.0,
            stamp: 0,
            shaped: 0,
            resized: 0,
        }
    }
}

/// Shapes `spans` into a paragraph that wraps at `bounds.width`.
pub(super) fn shape_spans<P: Paragraph<Font = Font>>(
    spans: &[NotationSpan],
    bounds: Size,
    size: f32,
    line_height: f32,
) -> P {
    P::with_spans(Text {
        content: spans,
        bounds,
        size: Pixels(size),
        line_height: LineHeight::Relative(line_height),
        font: Font::DEFAULT,
        align_x: Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::WordOrGlyph,
    })
}

/// The smallest paragraph width worth laying out.
const MIN_WIDTH: f32 = 40.0;

/// The space between two paragraphs, as a fraction of the line height.
const GAP: f32 = 0.2;

impl<P: Paragraph<Font = Font>> LayoutCache<P> {
    /// How many paragraphs were shaped since the cache was created.
    pub fn shaped(&self) -> usize {
        self.shaped
    }

    /// How many paragraphs were laid out again for a new width.
    pub fn resized(&self) -> usize {
        self.resized
    }

    /// Changes whenever paragraphs or positions changed.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The height of everything, padding included.
    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// The number of paragraphs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no paragraphs.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn bounds_for(indent_x: f32, width: f32) -> Size {
        Size::new(
            (width - 2.0 * PADDING - indent_x).max(MIN_WIDTH),
            f32::INFINITY,
        )
    }

    fn shape(&mut self, spans: &[NotationSpan], bounds: Size, style: &TextStyle) -> P {
        self.shaped += 1;
        shape_spans(spans, bounds, style.size, style.line_height)
    }

    /// Brings the paragraphs up to date with `input`. Returns whether
    /// anything changed. Cheap (three comparisons) when nothing did.
    pub fn sync(&mut self, input: &Inputs<'_>) -> bool {
        let sig_same = self
            .sig
            .as_ref()
            .is_some_and(|sig| sig.style == *input.style && sig.labels == *input.labels);
        let width = input.width.max(MIN_WIDTH + 2.0 * PADDING);
        if sig_same && self.revision == Some(input.doc.revision) && (self.width - width).abs() < 0.5
        {
            return false;
        }
        if !sig_same {
            self.entries.clear();
            self.sig = Some(StyleSig {
                style: *input.style,
                labels: input.labels.clone(),
            });
        }
        let mut old = Reuse::take(&mut self.entries);
        let mut entries = Vec::with_capacity(input.doc.blocks.len() + 2);
        let style = input.style;

        if !input.doc.header.is_empty() {
            let ident = Ident::Header(input.doc.header.clone());
            let kept = old.header.take().filter(|e| e.ident == ident);
            let entry = if let Some(entry) = kept {
                self.refit(entry, 0.0, width)
            } else {
                let spans = header_spans(&input.doc.header, input.labels, style);
                let hits = vec![None; spans.len()];
                self.fresh(ident, &spans, hits, 0.0, width, style)
            };
            entries.push(entry);
        }
        for block in &input.doc.blocks {
            entries.push(self.block_entry(block, &mut old, width, style));
        }
        if input.doc.blocks.is_empty() {
            let entry = if let Some(entry) = old.empty.take() {
                self.refit(entry, 0.0, width)
            } else {
                let spans = [empty_span(input.labels, style)];
                self.fresh(Ident::Empty, &spans, vec![None], 0.0, width, style)
            };
            entries.push(entry);
        }
        if let Some(result) = input.doc.result {
            let ident = Ident::Result(result);
            let kept = old.result.take().filter(|e| e.ident == ident);
            let entry = if let Some(entry) = kept {
                self.refit(entry, 0.0, width)
            } else {
                let spans = [result_span(result, style)];
                self.fresh(ident, &spans, vec![None], 0.0, width, style)
            };
            entries.push(entry);
        }

        self.place(&mut entries, style);
        self.entries = entries;
        self.revision = Some(input.doc.revision);
        self.width = width;
        self.stamp += 1;
        true
    }

    fn block_entry(
        &mut self,
        block: &Block,
        old: &mut Reuse<P>,
        width: f32,
        style: &TextStyle,
    ) -> Entry<P> {
        let ident = Ident::Block {
            key: block.key,
            rev: block.rev,
            indent: block.indent,
        };
        let x = f32::from(block.indent) * style.indent_step();
        if let Some(entry) = old.blocks.remove(&block.key).filter(|e| e.ident == ident) {
            self.refit(entry, x, width)
        } else {
            let (spans, hits) = block_spans(block, style);
            self.fresh(ident, &spans, hits, x, width, style)
        }
    }

    fn fresh(
        &mut self,
        ident: Ident,
        spans: &[NotationSpan],
        hits: Vec<Option<Hit>>,
        x: f32,
        width: f32,
        style: &TextStyle,
    ) -> Entry<P> {
        let bounds = Self::bounds_for(x, width);
        let paragraph = self.shape(spans, bounds, style);
        let height = paragraph.min_bounds().height;
        Entry {
            ident,
            paragraph,
            hits,
            x,
            y: 0.0,
            height,
        }
    }

    /// Lays a kept paragraph out again if the width changed.
    fn refit(&mut self, mut entry: Entry<P>, x: f32, width: f32) -> Entry<P> {
        let bounds = Self::bounds_for(x, width);
        let current = entry.paragraph.bounds();
        if (current.width - bounds.width).abs() > 0.5 {
            entry.paragraph.resize(bounds);
            entry.height = entry.paragraph.min_bounds().height;
            self.resized += 1;
        }
        entry.x = x;
        entry
    }

    /// Stacks the entries from the top.
    fn place(&mut self, entries: &mut [Entry<P>], style: &TextStyle) {
        let gap = style.line_height_px() * GAP;
        let mut y = PADDING;
        let mut after_header = false;
        for entry in entries.iter_mut() {
            if after_header {
                // The header is set apart from the moves.
                y += style.line_height_px() * 0.5;
            }
            entry.y = y;
            y += entry.height + gap;
            after_header = matches!(entry.ident, Ident::Header(_));
        }
        self.content_height = y + PADDING - gap;
    }

    /// The paragraphs that touch the vertical range `top..bottom` of the
    /// content.
    pub fn visible(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let first = self.entries.partition_point(|e| e.y + e.height < top);
        let last = self.entries.partition_point(|e| e.y <= bottom);
        first..last.max(first)
    }

    /// Paints: calls `f(paragraph, x, y)` for the paragraphs in `range`
    /// (the position of the paragraph in content coordinates).
    pub fn for_each(&self, range: std::ops::Range<usize>, mut f: impl FnMut(&P, f32, f32)) {
        for entry in self.entries.get(range).unwrap_or_default() {
            f(&entry.paragraph, entry.x + PADDING, entry.y);
        }
    }

    /// What is at `point` (content coordinates).
    pub fn locate(&self, point: Point) -> Option<Located> {
        let index = row_at(&self.entries, point.y, |e| (e.y, e.height))?;
        let entry = self.entries.get(index)?;
        let local = Point::new(point.x - entry.x - PADDING, point.y - entry.y);
        let span = entry.paragraph.hit_span(local)?;
        let hit = entry.hits.get(span).copied().flatten();
        Some(Located {
            entry: index,
            // The first span of the item, so that moving the pointer inside
            // a move drawn with a figurine does not look like a change.
            span: group(&entry.hits, span).start,
            hit,
        })
    }

    /// The rectangles of the whole item at `span` (a move drawn with a
    /// figurine is several spans), one per line.
    pub fn group_rects(&self, entry: usize, span: usize) -> Vec<Rectangle> {
        let Some(e) = self.entries.get(entry) else {
            return Vec::new();
        };
        group(&e.hits, span)
            .flat_map(|s| self.span_rects(entry, s))
            .collect()
    }

    /// The rectangles of a span in content coordinates (one per line).
    pub fn span_rects(&self, entry: usize, span: usize) -> Vec<Rectangle> {
        let Some(entry) = self.entries.get(entry) else {
            return Vec::new();
        };
        entry
            .paragraph
            .span_bounds(span)
            .into_iter()
            .map(|r| Rectangle {
                x: r.x + entry.x + PADDING,
                y: r.y + entry.y,
                ..r
            })
            .collect()
    }

    /// The span of an item.
    pub fn find_hit(&self, hit: Hit) -> Option<(usize, usize)> {
        self.entries.iter().enumerate().find_map(|(e, entry)| {
            entry
                .hits
                .iter()
                .position(|h| *h == Some(hit))
                .map(|s| (e, s))
        })
    }

    /// The span of the move of `node`.
    pub fn find_move(&self, node: NodeId) -> Option<(usize, usize)> {
        self.find_hit(Hit::Move(node))
    }

    /// The span of the first move of the game.
    pub fn first_move(&self) -> Option<(usize, usize)> {
        self.entries.iter().enumerate().find_map(|(e, entry)| {
            entry
                .hits
                .iter()
                .position(|hit| matches!(hit, Some(Hit::Move(_))))
                .map(|s| (e, s))
        })
    }
}

/// The spans around `span` that stand for the same item (a lone span when it
/// is not clickable).
pub(super) fn group(hits: &[Option<Hit>], span: usize) -> std::ops::Range<usize> {
    let Some(Some(hit)) = hits.get(span) else {
        return span..span + 1;
    };
    let same = |s: usize| hits.get(s).copied().flatten() == Some(*hit);
    let mut start = span;
    while start > 0 && same(start - 1) {
        start -= 1;
    }
    let mut end = span + 1;
    while same(end) {
        end += 1;
    }
    start..end
}

/// The entries of the previous sync, indexed for reuse.
struct Reuse<P> {
    header: Option<Entry<P>>,
    result: Option<Entry<P>>,
    empty: Option<Entry<P>>,
    blocks: HashMap<BlockKey, Entry<P>>,
}

impl<P> Reuse<P> {
    fn take(entries: &mut Vec<Entry<P>>) -> Reuse<P> {
        let mut reuse = Reuse {
            header: None,
            result: None,
            empty: None,
            blocks: HashMap::with_capacity(entries.len()),
        };
        for entry in entries.drain(..) {
            match entry.ident {
                Ident::Header(_) => reuse.header = Some(entry),
                Ident::Result(_) => reuse.result = Some(entry),
                Ident::Empty => reuse.empty = Some(entry),
                Ident::Block { key, .. } => {
                    reuse.blocks.insert(key, entry);
                }
            }
        }
        reuse
    }
}
