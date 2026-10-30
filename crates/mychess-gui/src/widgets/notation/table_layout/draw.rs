//! Drawing of the table (spec M2-5, D246-D250): row backgrounds and cards
//! first, then (by the panel) the marks, then the text.

use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph};
use iced::border::Radius;
use iced::{Border, Color, Font, Point, Rectangle};
use mychess_core::Color as Side;

use super::{
    Body, CARD_BAR, Flow, FlowKind, GUTTER, PILL_PAD, Shaped, TIME_BAR_MAX, TRow, TableCache,
    time_bar_width,
};
use crate::theme::notation_colors::NotationPalette;
use crate::widgets::notation::fill;

/// Alpha of the bar of an exact time and of an estimated one.
const BAR_ALPHA: f32 = 0.7;
const BAR_ALPHA_DERIVED: f32 = 0.35;
/// Height of a time bar.
const BAR_HEIGHT: f32 = 4.0;
/// Space between the bar and the time text.
const BAR_GAP: f32 = 4.0;

impl<P: Paragraph<Font = Font>> TableCache<P> {
    fn shifted(rect: Rectangle, bounds: Rectangle, offset: f32) -> Rectangle {
        Rectangle {
            x: bounds.x + rect.x,
            y: bounds.y + rect.y - offset,
            ..rect
        }
    }

    /// Zebra rows and cards.
    pub(crate) fn draw_backgrounds<R: renderer::Renderer>(
        &self,
        renderer: &mut R,
        bounds: Rectangle,
        offset: f32,
        palette: &NotationPalette,
    ) {
        let range = self.visible(offset, offset + bounds.height);
        for row in self.rows.get(range).unwrap_or_default() {
            match &row.body {
                Body::Moves { .. } if row.zebra => {
                    let rect = Rectangle {
                        x: 0.0,
                        y: row.y,
                        width: self.columns.total,
                        height: row.height,
                    };
                    fill(
                        renderer,
                        Self::shifted(rect, bounds, offset),
                        palette.zebra,
                        Border::default(),
                    );
                }
                Body::Flow(flow) => {
                    self.draw_card(renderer, row, &flow.kind, bounds, offset, palette);
                }
                Body::Moves { .. } => {}
            }
        }
    }

    fn draw_card<R: renderer::Renderer>(
        &self,
        renderer: &mut R,
        row: &TRow<P>,
        kind: &FlowKind,
        bounds: Rectangle,
        offset: f32,
        palette: &NotationPalette,
    ) {
        let FlowKind::Card { toggle, .. } = kind else {
            return;
        };
        let card = Self::shifted(self.card_rect(row), bounds, offset);
        let radius = palette.card_radius;
        if toggle.is_none() {
            // The placeholder of a variation to come: an outline only.
            let border = Border {
                color: palette.muted.scale_alpha(0.5),
                width: 1.0,
                radius: radius.into(),
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: card,
                    border,
                    ..renderer::Quad::default()
                },
                Color::TRANSPARENT,
            );
            return;
        }
        fill(
            renderer,
            card,
            palette.card,
            Border {
                radius: radius.into(),
                ..Border::default()
            },
        );
        let bar = Rectangle {
            width: CARD_BAR,
            ..card
        };
        fill(
            renderer,
            bar,
            palette.card_bar,
            Border {
                radius: Radius {
                    top_left: radius,
                    bottom_left: radius,
                    top_right: 0.0,
                    bottom_right: 0.0,
                },
                ..Border::default()
            },
        );
    }

    /// The text of the visible rows. `current` is the cell on the pill: its
    /// text takes the color made for the pill.
    pub(crate) fn draw_text<R>(
        &self,
        renderer: &mut R,
        bounds: Rectangle,
        offset: f32,
        palette: &NotationPalette,
        current: Option<(usize, Side)>,
    ) where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let range = self.visible(offset, offset + bounds.height);
        let first = range.start;
        let rows = self.rows.get(range).unwrap_or_default();
        for (index, row) in rows.iter().enumerate().map(|(i, row)| (first + i, row)) {
            let frame = Frame {
                bounds,
                top: bounds.y + row.y - offset,
                palette,
            };
            match &row.body {
                Body::Moves {
                    shaped: Some(shaped),
                    ..
                } => {
                    let on_pill = |side| current == Some((index, side));
                    self.draw_cells(renderer, row, shaped, &frame, on_pill);
                    self.draw_times(renderer, row, shaped, &frame);
                }
                Body::Moves { .. } => {}
                Body::Flow(flow) => self.draw_flow(renderer, row, flow, &frame),
            }
        }
    }

    /// The number, the dots and the two cells of a move row.
    fn draw_cells<R>(
        &self,
        renderer: &mut R,
        row: &TRow<P>,
        shaped: &Shaped<P>,
        frame: &Frame<'_>,
        on_pill: impl Fn(Side) -> bool,
    ) where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let Frame {
            bounds,
            top,
            palette,
        } = *frame;
        let centered = |p: &P| top + ((row.height - p.min_bounds().height) / 2.0).max(0.0);
        renderer.fill_paragraph(
            &shaped.number,
            Point::new(bounds.x + self.columns.number, centered(&shaped.number)),
            palette.muted,
            bounds,
        );
        if let Some(dots) = &shaped.dots {
            renderer.fill_paragraph(
                dots,
                Point::new(bounds.x + self.columns.white + PILL_PAD, centered(dots)),
                palette.muted,
                bounds,
            );
        }
        for (side, cell) in [(Side::White, &shaped.white), (Side::Black, &shaped.black)] {
            let Some(cell) = cell else { continue };
            let column = bounds.x + self.columns.column(side);
            let clip = Rectangle {
                x: column,
                y: top,
                width: self.columns.cell,
                height: row.height,
            }
            .intersection(&bounds)
            .unwrap_or(bounds);
            let pilled = on_pill(side);
            for part in &cell.parts {
                let color = if pilled { palette.on_pill } else { part.color };
                renderer.fill_paragraph(
                    &part.paragraph,
                    Point::new(
                        column + PILL_PAD + part.x,
                        centered(&part.paragraph) + part.dy,
                    ),
                    color,
                    clip,
                );
            }
        }
    }

    /// The time column: a bar and the time spent, White above, Black below.
    fn draw_times<R>(&self, renderer: &mut R, row: &TRow<P>, shaped: &Shaped<P>, frame: &Frame<'_>)
    where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let Frame {
            bounds,
            top,
            palette,
        } = *frame;
        for (slot, time) in shaped.times.iter().enumerate() {
            let Some(time) = time else { continue };
            let middle = top + row.height * if slot == 0 { 0.27 } else { 0.73 };
            let width = time_bar_width(time.spent, self.max_spent);
            let alpha = if time.derived {
                BAR_ALPHA_DERIVED
            } else {
                BAR_ALPHA
            };
            let x = bounds.x + self.columns.time;
            fill(
                renderer,
                Rectangle {
                    x,
                    y: middle - BAR_HEIGHT / 2.0,
                    width,
                    height: BAR_HEIGHT,
                },
                palette.time_bar.scale_alpha(alpha),
                Border {
                    radius: (BAR_HEIGHT / 2.0).into(),
                    ..Border::default()
                },
            );
            renderer.fill_paragraph(
                &time.paragraph,
                Point::new(
                    x + TIME_BAR_MAX + BAR_GAP,
                    middle - time.paragraph.min_bounds().height / 2.0,
                ),
                palette.muted,
                bounds,
            );
        }
    }

    /// A comment, card, placeholder or result, and the chevron of a card.
    fn draw_flow<R>(&self, renderer: &mut R, row: &TRow<P>, flow: &Flow<P>, frame: &Frame<'_>)
    where
        R: text::Renderer<Paragraph = P, Font = Font>,
    {
        let Frame {
            bounds,
            top,
            palette,
        } = *frame;
        renderer.fill_paragraph(
            &flow.paragraph,
            Point::new(bounds.x + flow.x, top + flow.top),
            palette.main,
            bounds,
        );
        let FlowKind::Card {
            toggle: Some(_),
            collapsed,
        } = &flow.kind
        else {
            return;
        };
        let chevron = self
            .chevrons
            .as_ref()
            .and_then(|pair| pair.get(usize::from(!*collapsed)));
        if let Some(chevron) = chevron {
            let card = self.card_rect(row);
            let inset = (GUTTER - chevron.min_bounds().width) / 2.0;
            renderer.fill_paragraph(
                chevron,
                Point::new(bounds.x + card.x + CARD_BAR + inset, top + flow.top),
                palette.muted,
                bounds,
            );
        }
    }
}

/// Where and with what a row is drawn.
#[derive(Clone, Copy)]
struct Frame<'a> {
    /// The widget, on the window.
    bounds: Rectangle,
    /// The top of the row on the window.
    top: f32,
    palette: &'a NotationPalette,
}
