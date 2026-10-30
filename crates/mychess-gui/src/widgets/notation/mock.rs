#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
//! A monospaced stand-in for the toolkit's paragraph, so layout, hit
//! testing and the shaping counters can be tested without a renderer.

use std::cell::Cell;

use iced::advanced::text::{
    Alignment, Difference, Hit, LineHeight, Paragraph, Shaping, Span, Text, Wrapping,
};
use iced::{Font, Pixels, Point, Rectangle, Size, alignment};

thread_local! {
    static SHAPED: Cell<usize> = const { Cell::new(0) };
}

/// How many mock paragraphs this thread shaped.
pub(super) fn shaped_on_this_thread() -> usize {
    SHAPED.with(Cell::get)
}

/// Width of a character as a fraction of the font size.
const CHAR_WIDTH: f32 = 0.5;

/// A paragraph of fixed-width characters that wraps at the glyph level.
#[derive(Debug, Clone, Default)]
pub(super) struct MockParagraph {
    chars: Vec<(char, usize)>,
    bounds: Size,
    size: f32,
    line_height: f32,
}

impl MockParagraph {
    fn char_width(&self) -> f32 {
        self.size * CHAR_WIDTH
    }

    fn lines(&self) -> Vec<Vec<(char, usize)>> {
        let per_line = ((self.bounds.width / self.char_width()).floor().max(1.0)) as usize;
        let mut lines: Vec<Vec<(char, usize)>> = vec![Vec::new()];
        for &(c, span) in &self.chars {
            if c == '\n' {
                lines.push(Vec::new());
                continue;
            }
            if lines.last().is_some_and(|l| l.len() >= per_line) {
                lines.push(Vec::new());
            }
            if let Some(last) = lines.last_mut() {
                last.push((c, span));
            }
        }
        lines
    }
}

impl Paragraph for MockParagraph {
    type Font = Font;

    fn with_text(text: Text<&str, Font>) -> Self {
        SHAPED.with(|s| s.set(s.get() + 1));
        MockParagraph {
            chars: text.content.chars().map(|c| (c, 0)).collect(),
            bounds: text.bounds,
            size: text.size.0,
            line_height: text.line_height.to_absolute(text.size).0,
        }
    }

    fn with_spans<Link>(text: Text<&[Span<'_, Link, Font>], Font>) -> Self {
        SHAPED.with(|s| s.set(s.get() + 1));
        let mut chars = Vec::new();
        for (index, span) in text.content.iter().enumerate() {
            chars.extend(span.text.chars().map(|c| (c, index)));
        }
        MockParagraph {
            chars,
            bounds: text.bounds,
            size: text.size.0,
            line_height: text.line_height.to_absolute(text.size).0,
        }
    }

    fn resize(&mut self, new_bounds: Size) {
        self.bounds = new_bounds;
    }

    fn compare(&self, _text: Text<(), Font>) -> Difference {
        Difference::None
    }

    fn size(&self) -> Pixels {
        Pixels(self.size)
    }

    fn font(&self) -> Font {
        Font::DEFAULT
    }

    fn line_height(&self) -> LineHeight {
        LineHeight::Absolute(Pixels(self.line_height))
    }

    fn align_x(&self) -> Alignment {
        Alignment::Default
    }

    fn align_y(&self) -> alignment::Vertical {
        alignment::Vertical::Top
    }

    fn wrapping(&self) -> Wrapping {
        Wrapping::Glyph
    }

    fn shaping(&self) -> Shaping {
        Shaping::Advanced
    }

    fn bounds(&self) -> Size {
        self.bounds
    }

    fn min_bounds(&self) -> Size {
        let lines = self.lines();
        let widest = lines.iter().map(Vec::len).max().unwrap_or(0);
        Size::new(
            widest as f32 * self.char_width(),
            lines.len() as f32 * self.line_height,
        )
    }

    fn hit_test(&self, _point: Point) -> Option<Hit> {
        None
    }

    fn hit_span(&self, point: Point) -> Option<usize> {
        if point.x < 0.0 || point.y < 0.0 {
            return None;
        }
        let line = (point.y / self.line_height) as usize;
        let column = (point.x / self.char_width()) as usize;
        self.lines().get(line)?.get(column).map(|&(_, span)| span)
    }

    fn span_bounds(&self, index: usize) -> Vec<Rectangle> {
        let mut out = Vec::new();
        for (row, line) in self.lines().iter().enumerate() {
            let first = line.iter().position(|&(_, s)| s == index);
            let count = line.iter().filter(|&&(_, s)| s == index).count();
            if let Some(first) = first {
                out.push(Rectangle {
                    x: first as f32 * self.char_width(),
                    y: row as f32 * self.line_height,
                    width: count as f32 * self.char_width(),
                    height: self.line_height,
                });
            }
        }
        out
    }

    fn grapheme_position(&self, _line: usize, _index: usize) -> Option<Point> {
        None
    }
}
