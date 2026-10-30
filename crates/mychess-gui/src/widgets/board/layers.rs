//! Drawing of the board layers (spec rules 24-28, 30-31, 42-44).
//!
//! From bottom to top: `squares` (cached), `under` (selection and hints,
//! dynamic), `pieces` (cached), `marks` (cached) and `overlay` (dynamic).
//! Every function takes a [`Scene`] and a frame; nothing here owns state.
// Pixel math on bounded values: counts are at most 64 and sizes are capped.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use iced::advanced::svg::Svg;
use iced::alignment::Vertical;
use iced::widget::canvas::{Frame, LineJoin, Path, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Size};
use mychess_app::{ArrowKind, EngineArrow};
use mychess_core::{BoardMark, MarkKind, Move, Piece, Position, Square};
use smallvec::SmallVec;

use super::animation::MoveAnimation;
use super::geometry::{BoardLayout, CoordsMode, Orientation, coordinate_labels};
use super::interaction::{Interaction, PendingPromotion};
use super::marks::{self, CIRCLE_PORTION, DEFAULT_SHRINK, TEXT_PORTION};
use super::pieces::PieceSet;
use super::promotion::{promotion_cells, promotion_role_at};
use crate::fonts::NOTO_SANS_BOLD;
use crate::theme::board_theme::BoardTheme;
use crate::theme::contrast_on;
use crate::theme::engine_overlay as eo;
use crate::theme::mark_colors::mark_color_or_default;
use crate::widgets::board::BoardOptions;

/// Alpha of circles, disks, text and arrows (so pieces stay visible).
const MARK_ALPHA: f32 = 0.85;
/// Alpha of the last move fill.
const LAST_MOVE_ALPHA: f32 = 0.6;
/// Alpha of the origin ghost of a dragged piece.
const GHOST_ALPHA: f32 = 0.4;

/// Everything the layers read.
pub(super) struct Scene<'a> {
    /// Where things go.
    pub layout: BoardLayout,
    /// Who is at the bottom.
    pub o: Orientation,
    /// Board colors.
    pub theme: BoardTheme,
    /// Display preferences.
    pub opts: BoardOptions,
    /// The drawn position.
    pub position: &'a Position,
    /// The move that led here.
    pub last_move: Option<Move>,
    /// Marks of the current comment.
    pub marks: &'a [BoardMark],
    /// The piece images.
    pub pieces: &'a PieceSet,
    /// Squares the piece layer leaves empty (animation targets).
    pub hidden: &'a [Square],
    /// The animation and its eased progress, if one runs.
    pub animation: Option<(&'a MoveAnimation, f32)>,
}

pub(super) fn fill_rect(frame: &mut Frame, rect: Rectangle, color: Color) {
    frame.fill_rectangle(rect.position(), rect.size(), color);
}

pub(super) fn draw_piece(
    frame: &mut Frame,
    set: &PieceSet,
    piece: Piece,
    rect: Rectangle,
    opacity: f32,
) {
    if let Some(handle) = set.handle(piece) {
        frame.draw_svg(rect, Svg::new(handle.clone()).opacity(opacity));
    }
}

fn is_light_square(sq: Square) -> bool {
    (u8::from(sq.file()) + u8::from(sq.rank())) % 2 == 1
}

/// The fills the squares layer applies to `sq`, bottom first: the square
/// color, its `Full` marks and the last-move tint.
fn base_fills(scene: &Scene<'_>, sq: Square) -> SmallVec<[Color; 4]> {
    let mut fills = SmallVec::new();
    fills.push(square_color(&scene.theme, sq));
    if scene.opts.show_marks {
        fills.extend(
            scene
                .marks
                .iter()
                .filter(|m| m.kind == MarkKind::Full && m.from == sq)
                .map(|m| mark_color_or_default(&m.color)),
        );
    }
    if scene.opts.highlight_last_move
        && let Some(mv) = scene.last_move
        && (mv.from() == Some(sq) || mv.to() == Some(sq))
    {
        fills.push(scene.theme.last_move.scale_alpha(LAST_MOVE_ALPHA));
    }
    fills
}

/// Paints `sq` as the squares layer did, erasing what is drawn above it
/// without losing the last-move tint or the `Full` marks underneath.
fn restore_square(frame: &mut Frame, scene: &Scene<'_>, sq: Square) {
    let rect = scene.layout.square_rect(sq, scene.o);
    for color in base_fills(scene, sq) {
        fill_rect(frame, rect, color);
    }
}

fn square_color(theme: &BoardTheme, sq: Square) -> Color {
    if is_light_square(sq) {
        theme.light
    } else {
        theme.dark
    }
}

fn text(content: String, position: Point, size: f32, color: Color) -> Text {
    Text {
        content,
        position,
        color,
        size: Pixels(size),
        font: NOTO_SANS_BOLD,
        align_x: iced::alignment::Horizontal::Center.into(),
        align_y: Vertical::Center,
        ..Text::default()
    }
}

// ---- layer 1: squares ----

/// Frame, squares, `Full` marks, last move and coordinates.
pub(super) fn draw_squares(frame: &mut Frame, scene: &Scene<'_>) {
    let layout = &scene.layout;
    let theme = &scene.theme;
    fill_rect(frame, layout.bounds, theme.frame);
    let outline = Rectangle::new(
        Point::new(layout.board.x - 1.0, layout.board.y - 1.0),
        Size::new(layout.board.width + 2.0, layout.board.height + 2.0),
    );
    fill_rect(frame, outline, theme.frame);
    fill_rect(frame, layout.board, theme.dark);
    for sq in Square::ALL.into_iter().filter(|&sq| is_light_square(sq)) {
        fill_rect(frame, layout.square_rect(sq, scene.o), theme.light);
    }
    if scene.opts.show_marks {
        for mark in scene.marks.iter().filter(|m| m.kind == MarkKind::Full) {
            let color = mark_color_or_default(&mark.color);
            fill_rect(frame, layout.square_rect(mark.from, scene.o), color);
        }
    }
    if scene.opts.highlight_last_move
        && let (Some(from), Some(to)) = (
            scene.last_move.and_then(Move::from),
            scene.last_move.and_then(Move::to),
        )
    {
        let color = theme.last_move.scale_alpha(LAST_MOVE_ALPHA);
        fill_rect(frame, layout.square_rect(from, scene.o), color);
        fill_rect(frame, layout.square_rect(to, scene.o), color);
    }
    draw_coordinates(frame, scene);
}

fn draw_coordinates(frame: &mut Frame, scene: &Scene<'_>) {
    let layout = &scene.layout;
    let (files, ranks) = coordinate_labels(scene.o);
    let size = 0.22 * layout.s;
    match scene.opts.coords {
        CoordsMode::None => {}
        CoordsMode::Outside | CoordsMode::OutsideBothSides => {
            let color = contrast_on(scene.theme.frame);
            let margin = layout.board.x - layout.bounds.x;
            let board = layout.board;
            let both = scene.opts.coords == CoordsMode::OutsideBothSides;
            for (i, (file, rank)) in files.iter().zip(ranks.iter()).enumerate() {
                let offset = (i as f32 + 0.5) * layout.s;
                let x = board.x + offset;
                let y = board.y + offset;
                let below = board.y + board.height + margin / 2.0;
                let left = board.x - margin / 2.0;
                frame.fill_text(text(file.to_string(), Point::new(x, below), size, color));
                frame.fill_text(text(rank.to_string(), Point::new(left, y), size, color));
                if both {
                    let above = board.y - margin / 2.0;
                    let right = board.x + board.width + margin / 2.0;
                    frame.fill_text(text(file.to_string(), Point::new(x, above), size, color));
                    frame.fill_text(text(rank.to_string(), Point::new(right, y), size, color));
                }
            }
        }
        CoordsMode::Inside => {
            for label in inside_labels(scene) {
                frame.fill_text(text(
                    label.text.to_string(),
                    label.at,
                    label.size,
                    label.color,
                ));
            }
        }
    }
}

/// The corner of a square a label sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Corner {
    /// Rank numbers, on the left column.
    TopLeft,
    /// File letters, on the bottom row.
    BottomRight,
}

/// A coordinate label drawn inside an edge square (`CoordsMode::Inside`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct InsideLabel {
    /// The square that carries the label.
    pub square: Square,
    /// The rank digit or file letter.
    pub text: char,
    /// Where in the square it sits.
    pub corner: Corner,
    /// Center of the glyph.
    pub at: Point,
    /// Glyph size.
    pub size: f32,
    /// The color of the opposite square (contrast with the square's own).
    pub color: Color,
}

/// The sixteen labels of the inside mode, files first: letters at the
/// bottom-right corner of the squares of the bottom row, numbers at the
/// top-left corner of the squares of the left column, both following the
/// orientation. They belong to the squares layer, which is drawn below the
/// pieces (see `BoardWidget::into_element`).
pub(super) fn inside_labels(scene: &Scene<'_>) -> Vec<InsideLabel> {
    let layout = &scene.layout;
    let (files, ranks) = coordinate_labels(scene.o);
    let size = 0.2 * layout.s;
    let mut labels = Vec::with_capacity(16);
    let mut push = |col: u8, row: u8, text: char, corner: Corner| {
        let Some(square) = super::geometry::square_from_cell(col, row, scene.o) else {
            return;
        };
        let rect = layout.cell_rect(col, row);
        let at = match corner {
            Corner::BottomRight => Point::new(
                rect.x + rect.width - 0.16 * layout.s,
                rect.y + rect.height - 0.16 * layout.s,
            ),
            Corner::TopLeft => Point::new(rect.x + 0.14 * layout.s, rect.y + 0.16 * layout.s),
        };
        labels.push(InsideLabel {
            square,
            text,
            corner,
            at,
            size,
            color: other_color(&scene.theme, square),
        });
    };
    for (col, file) in files.iter().enumerate() {
        push(col as u8, 7, *file, Corner::BottomRight);
    }
    for (row, rank) in ranks.iter().enumerate() {
        push(0, row as u8, *rank, Corner::TopLeft);
    }
    labels
}

fn other_color(theme: &BoardTheme, sq: Square) -> Color {
    if is_light_square(sq) {
        theme.dark
    } else {
        theme.light
    }
}

// ---- layer 2: selection and hints (under the pieces) ----

/// Selected square, drop target and legal destination hints.
pub(super) fn draw_under(frame: &mut Frame, scene: &Scene<'_>, interaction: &Interaction) {
    let layout = &scene.layout;
    let Some(selected) = interaction.selected() else {
        return;
    };
    fill_rect(
        frame,
        layout.square_rect(selected, scene.o),
        scene.theme.selected,
    );
    let destinations = interaction.destinations();
    if interaction.drag().is_some()
        && let Some(hover) = interaction.hover()
        && destinations.iter().any(|d| d.square == hover)
    {
        fill_rect(
            frame,
            layout.square_rect(hover, scene.o),
            scene.theme.selected,
        );
    }
    if !scene.opts.show_legal_hints {
        return;
    }
    let hint = scene.theme.hint.scale_alpha(0.7);
    for dest in destinations {
        let rect = layout.square_rect(dest.square, scene.o);
        let center = rect.center();
        if dest.capture {
            let thickness = 0.08 * layout.s;
            let radius = 0.48 * layout.s - thickness / 2.0;
            frame.stroke(
                &Path::circle(center, radius),
                Stroke::default().with_width(thickness).with_color(hint),
            );
        } else {
            frame.fill(&Path::circle(center, 0.16 * layout.s), hint);
        }
    }
}

// ---- layer 3: pieces ----

/// The pieces of the position, except the hidden squares.
pub(super) fn draw_pieces(frame: &mut Frame, scene: &Scene<'_>) {
    for (sq, piece) in scene.position.pieces() {
        if scene.hidden.contains(&sq) {
            continue;
        }
        draw_piece(
            frame,
            scene.pieces,
            piece,
            scene.layout.square_rect(sq, scene.o),
            1.0,
        );
    }
}

// ---- layer 4: marks ----

/// Circles, disks, text, outlines and arrows (arrows last, with the piece of
/// each origin square redrawn on top of them).
pub(super) fn draw_marks(frame: &mut Frame, scene: &Scene<'_>) {
    if !scene.opts.show_marks {
        return;
    }
    let layout = &scene.layout;
    for mark in scene.marks {
        let rect = layout.square_rect(mark.from, scene.o);
        let color = mark_color_or_default(&mark.color).scale_alpha(MARK_ALPHA);
        match mark.kind {
            MarkKind::Circle => {
                let b = marks::inner_box(rect, CIRCLE_PORTION);
                let thickness = 0.075 * CIRCLE_PORTION * layout.s;
                frame.stroke(
                    &Path::circle(b.center(), b.width / 2.0 - thickness / 2.0),
                    Stroke::default().with_width(thickness).with_color(color),
                );
            }
            MarkKind::Disk => {
                let b = marks::inner_box(rect, CIRCLE_PORTION);
                frame.fill(&Path::circle(b.center(), b.width / 2.0), color);
            }
            MarkKind::Text(c) => {
                let b = marks::inner_box(rect, TEXT_PORTION);
                let size = 0.55 * layout.s;
                let shadow = contrast_on(color).scale_alpha(MARK_ALPHA);
                let at = b.center();
                frame.fill_text(text(
                    c.to_string(),
                    Point::new(at.x + 1.0, at.y + 1.0),
                    size,
                    shadow,
                ));
                frame.fill_text(text(c.to_string(), at, size, color));
            }
            MarkKind::Square => {
                let t = 0.075 * layout.s;
                let inset = Rectangle::new(
                    Point::new(rect.x + t / 2.0, rect.y + t / 2.0),
                    Size::new(rect.width - t, rect.height - t),
                );
                frame.stroke_rectangle(
                    inset.position(),
                    inset.size(),
                    Stroke::default().with_width(t).with_color(color),
                );
            }
            // `Full` is drawn with the squares, `Arrow` below, `Tux` never
            // (its icon has no registered license).
            MarkKind::Full | MarkKind::Arrow | MarkKind::Tux | _ => {}
        }
    }
    for mark in scene.marks.iter().filter(|m| m.kind == MarkKind::Arrow) {
        let Some(to) = mark.to else { continue };
        let color = mark_color_or_default(&mark.color).scale_alpha(MARK_ALPHA);
        draw_arrow(frame, scene, mark.from, to, color);
        if let Some(piece) = scene.position.piece_at(mark.from)
            && !scene.hidden.contains(&mark.from)
        {
            draw_piece(
                frame,
                scene.pieces,
                piece,
                layout.square_rect(mark.from, scene.o),
                1.0,
            );
        }
    }
}

fn draw_arrow(frame: &mut Frame, scene: &Scene<'_>, from: Square, to: Square, color: Color) {
    let Some(arrow) = marks::arrow_geometry(&scene.layout, from, to, scene.o, DEFAULT_SHRINK)
    else {
        return;
    };
    frame.stroke(
        &Path::line(arrow.tail, arrow.base),
        Stroke::default()
            .with_width(arrow.shaft_width)
            .with_color(color),
    );
    let head = Path::new(|b| {
        b.move_to(arrow.head[0]);
        b.line_to(arrow.head[1]);
        b.line_to(arrow.head[2]);
        b.close();
    });
    frame.fill(&head, color);
}

// ---- layer 4b: engine arrows ----

/// Shaft of the arrows of the other lines, as a fraction of a square (D459).
const OTHER_SHAFT: f32 = 0.10;
/// Shaft of the sequence arrows, as a fraction of a square (D459).
const SEQUENCE_SHAFT: f32 = 0.07;

/// The green of the best move of the guided review (spec M4-11, D1664),
/// distinct from the blue of the live engine; the outline of the engine
/// arrows keeps it readable on both square colours.
pub(crate) const REVIEW_BEST_ARROW: Color = Color::from_rgb(0.16, 0.62, 0.28);

/// The order the arrows are drawn in: the best one last, on top.
pub(super) fn arrow_paint_order(arrows: &[EngineArrow]) -> impl Iterator<Item = &EngineArrow> {
    let best = |a: &&EngineArrow| matches!(a.kind, ArrowKind::Best | ArrowKind::ReviewBest);
    arrows
        .iter()
        .filter(move |a| !best(a))
        .chain(arrows.iter().filter(best))
}

/// The shaft width (in squares) of an arrow kind; `None` keeps the default.
pub(super) fn shaft_of(kind: ArrowKind) -> Option<f32> {
    match kind {
        ArrowKind::Best | ArrowKind::ReviewBest => None,
        ArrowKind::Other => Some(OTHER_SHAFT),
        ArrowKind::Sequence(_) => Some(SEQUENCE_SHAFT),
    }
}

fn engine_arrow_color(arrow: EngineArrow) -> Color {
    let base = match arrow.kind {
        ArrowKind::Best => eo::BEST_ARROW,
        ArrowKind::ReviewBest => REVIEW_BEST_ARROW,
        ArrowKind::Sequence(_) => match arrow.side {
            mychess_core::Color::White => eo::SEQUENCE_WHITE,
            mychess_core::Color::Black => eo::SEQUENCE_BLACK,
        },
        ArrowKind::Other => eo::OTHER_ARROW,
    };
    base.scale_alpha(eo::ARROW_ALPHA)
}

/// The arrows of the engine, from the lists the application prepared; the
/// best one is on top and the numbered ones carry their number.
pub(super) fn draw_engine_arrows(
    frame: &mut Frame,
    layout: &BoardLayout,
    o: Orientation,
    arrows: &[EngineArrow],
) {
    for arrow in arrow_paint_order(arrows) {
        let Some(shape) = marks::engine_arrow_geometry(
            layout,
            arrow.from,
            arrow.to,
            o,
            shaft_of(arrow.kind).unwrap_or(0.14),
        ) else {
            continue;
        };
        let color = engine_arrow_color(*arrow);
        let dark = eo::ARROW_OUTLINE.scale_alpha(eo::OUTLINE_ALPHA);
        let light = eo::ARROW_LIGHT_EDGE.scale_alpha(eo::OUTLINE_ALPHA);
        let edge = eo::outline_width(layout.s);
        let polygon = |points: &[Point]| {
            Path::new(|b| {
                b.move_to(points[0]);
                for p in &points[1..] {
                    b.line_to(*p);
                }
                b.close();
            })
        };
        // One polygon (shaft and head); the fill lets the pieces show
        // through. Two thin rings run inside its edge, the light one
        // outside the dark one: on dark squares the light ring is the edge,
        // on light squares it fades and the dark ring is (no square colour
        // is within 3:1 of both); the outer edge stays where the geometry
        // puts it.
        frame.fill(&polygon(&shape.outline()), color);
        for (ring, tint) in [(0.0, light), (1.0, dark)] {
            if let Some(points) = shape.outline_inset(edge * (ring + 0.5)) {
                frame.stroke(
                    &polygon(&points),
                    Stroke::default()
                        .with_width(edge)
                        .with_color(tint)
                        .with_line_join(LineJoin::Miter),
                );
            }
        }
        if let Some(number) = arrow.label {
            let at = Point::new(
                f32::midpoint(shape.tail.x, shape.base.x),
                f32::midpoint(shape.tail.y, shape.base.y),
            );
            let radius = 0.18 * layout.s;
            // Opaque, so that the digit reads on it whatever is below.
            let disc = Color { a: 1.0, ..color };
            frame.fill(&Path::circle(at, radius), disc);
            frame.stroke(
                &Path::circle(at, radius - edge / 2.0),
                Stroke::default().with_width(edge).with_color(dark),
            );
            frame.fill_text(text(
                number.to_string(),
                at,
                0.26 * layout.s,
                contrast_on(disc),
            ));
        }
    }
}

/// The centre and the radius of the badge disc of `square`: the top-right
/// corner of the square as the board is drawn (D2193).
fn badge_disc(layout: &BoardLayout, o: Orientation, square: Square) -> (Point, f32) {
    let rect = layout.square_rect(square, o);
    let radius = 0.21 * layout.s;
    let center = Point::new(rect.x + rect.width - radius, rect.y + radius);
    (center, radius)
}

/// The class of the move under the cursor on its destination square, as a
/// disc in the top-right corner of the square (spec M4-10, D1434): the
/// colour of the class with the glyph on it, read in either theme because
/// the glyph takes the colour that contrasts with the disc.
pub(super) fn draw_class_badge(
    frame: &mut Frame,
    layout: &BoardLayout,
    o: Orientation,
    badge: &super::ClassBadge,
) {
    let (center, radius) = badge_disc(layout, o, badge.square);
    let disc = Color {
        a: 1.0,
        ..badge.color
    };
    let edge = eo::outline_width(layout.s);
    frame.fill(&Path::circle(center, radius), disc);
    frame.stroke(
        &Path::circle(center, radius - edge / 2.0),
        Stroke::default()
            .with_width(edge)
            .with_color(eo::ARROW_LIGHT_EDGE),
    );
    let size = if badge.glyph.chars().count() > 1 {
        0.2 * layout.s
    } else {
        0.28 * layout.s
    };
    frame.fill_text(text(
        badge.glyph.to_owned(),
        center,
        size,
        contrast_on(disc),
    ));
}

// ---- layer 5: overlay ----

/// Sliding pieces, the dragged piece with its ghost, the mark preview and
/// the promotion column.
pub(super) fn draw_overlay(
    frame: &mut Frame,
    scene: &Scene<'_>,
    interaction: &Interaction,
    cursor: Option<Point>,
) {
    let layout = &scene.layout;
    if let Some((animation, eased)) = scene.animation {
        for sprite in &animation.sprites {
            let a = layout.square_center(sprite.from, scene.o);
            let b = layout.square_center(sprite.to, scene.o);
            let center = Point::new(a.x + (b.x - a.x) * eased, a.y + (b.y - a.y) * eased);
            draw_piece(
                frame,
                scene.pieces,
                sprite.piece,
                centered(center, layout.s),
                1.0,
            );
        }
    }
    if let Some(drag) = interaction.drag()
        && let Some(piece) = scene.position.piece_at(drag.from)
    {
        let origin = layout.square_rect(drag.from, scene.o);
        restore_square(frame, scene, drag.from);
        fill_rect(frame, origin, scene.theme.selected);
        draw_piece(frame, scene.pieces, piece, origin, GHOST_ALPHA);
        draw_piece(
            frame,
            scene.pieces,
            piece,
            centered(drag.cursor, layout.s),
            1.0,
        );
    }
    if let Some(gesture) = interaction.mark_drag()
        && let Some(target) = gesture.current.filter(|&t| t != gesture.start)
    {
        let name = marks::color_for_modifiers(gesture.mods);
        draw_arrow(
            frame,
            scene,
            gesture.start,
            target,
            mark_color_or_default(name).scale_alpha(0.5),
        );
    }
    if let Some(start) = interaction.arrow_start() {
        let b = marks::inner_box(layout.square_rect(start, scene.o), CIRCLE_PORTION);
        let color = scene.theme.selected.scale_alpha(0.8);
        frame.stroke(
            &Path::circle(b.center(), b.width / 2.0),
            Stroke::default()
                .with_width(0.06 * layout.s)
                .with_color(color),
        );
    }
    if let Some(pending) = interaction.pending_promotion() {
        draw_promotion(frame, scene, pending, cursor);
    }
}

pub(super) fn centered(center: Point, side: f32) -> Rectangle {
    Rectangle::new(
        Point::new(center.x - side / 2.0, center.y - side / 2.0),
        Size::new(side, side),
    )
}

fn draw_promotion(
    frame: &mut Frame,
    scene: &Scene<'_>,
    pending: &PendingPromotion,
    cursor: Option<Point>,
) {
    let layout = &scene.layout;
    // The pawn leaves its square and stands on the target while the column
    // is open (spec rule 38).
    restore_square(frame, scene, pending.from);
    if let Some(pawn) = scene.position.piece_at(pending.from) {
        draw_piece(
            frame,
            scene.pieces,
            pawn,
            layout.square_rect(pending.to, scene.o),
            1.0,
        );
    }
    let cells = promotion_cells(layout, pending.to, scene.o);
    let hovered = cursor.and_then(|p| promotion_role_at(&cells, p));
    let side = scene.position.turn();
    for cell in &cells {
        let fill = if hovered == Some(cell.role) {
            scene.theme.selected
        } else {
            scene.theme.light
        };
        fill_rect(frame, cell.rect, scene.theme.frame);
        fill_rect(
            frame,
            Rectangle::new(
                Point::new(cell.rect.x + 1.0, cell.rect.y + 1.0),
                Size::new(cell.rect.width - 2.0, cell.rect.height - 2.0),
            ),
            fill,
        );
        draw_piece(
            frame,
            scene.pieces,
            Piece {
                color: side,
                role: cell.role,
            },
            cell.rect,
            1.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use iced::Size;

    use super::super::BoardOptions;
    use super::*;
    use crate::theme::board_theme::BoardThemeId;

    fn sq(name: &str) -> Square {
        name.parse().expect("square")
    }

    fn fills(options: BoardOptions, marks: &[BoardMark], last: Option<Move>, at: &str) -> usize {
        let position = Position::new();
        let pieces = PieceSet::default();
        let layout = BoardLayout::compute(
            Size::new(800.0, 800.0),
            BoardOptions::default().layout_options(),
        )
        .expect("layout fits");
        let scene = Scene {
            layout,
            o: Orientation::White,
            theme: BoardTheme::of(BoardThemeId::default()),
            opts: options,
            position: &position,
            last_move: last,
            marks,
            pieces: &pieces,
            hidden: &[],
            animation: None,
        };
        base_fills(&scene, sq(at)).len()
    }

    #[test]
    fn the_badge_sits_in_the_corner_of_its_square_with_the_board_turned() {
        let layout = BoardLayout::compute(
            Size::new(800.0, 800.0),
            BoardOptions::default().layout_options(),
        )
        .expect("layout fits");
        for o in [Orientation::White, Orientation::Black] {
            let square = sq("f6");
            let rect = layout.square_rect(square, o);
            let (center, radius) = badge_disc(&layout, o, square);
            assert!(center.x + radius <= rect.x + rect.width + 0.01, "{o:?}");
            assert!(center.y - radius >= rect.y - 0.01, "{o:?}");
            assert!(center.x > rect.x + rect.width / 2.0, "right half {o:?}");
            assert!(center.y < rect.y + rect.height / 2.0, "top half {o:?}");
        }
        let white = layout.square_rect(sq("f6"), Orientation::White);
        let black = layout.square_rect(sq("f6"), Orientation::Black);
        assert!(
            (white.x - black.x).abs() > 1.0 && (white.y - black.y).abs() > 1.0,
            "the square moves when the board is turned"
        );
    }

    fn labels(o: Orientation) -> Vec<InsideLabel> {
        let position = Position::new();
        let pieces = PieceSet::default();
        let options = BoardOptions {
            coords: CoordsMode::Inside,
            ..BoardOptions::default()
        };
        let layout = BoardLayout::compute(Size::new(800.0, 800.0), options.layout_options())
            .expect("layout fits");
        let scene = Scene {
            layout,
            o,
            theme: BoardTheme::of(BoardThemeId::default()),
            opts: options,
            position: &position,
            last_move: None,
            marks: &[],
            pieces: &pieces,
            hidden: &[],
            animation: None,
        };
        inside_labels(&scene)
    }

    #[test]
    fn inside_labels_sit_in_the_corners_of_the_edge_squares() {
        let theme = BoardTheme::of(BoardThemeId::default());
        for o in [Orientation::White, Orientation::Black] {
            let all = labels(o);
            assert_eq!(all.len(), 16, "{o:?}");
            let layout = BoardLayout::compute(
                Size::new(800.0, 800.0),
                BoardOptions {
                    coords: CoordsMode::Inside,
                    ..BoardOptions::default()
                }
                .layout_options(),
            )
            .expect("layout fits");
            for label in &all {
                let rect = layout.square_rect(label.square, o);
                assert!(rect.contains(label.at), "{label:?} is inside its square");
                // The glyph box stays inside the square it labels.
                assert!(label.size <= 0.25 * layout.s);
                let (dx, dy) = (label.at.x - rect.x, label.at.y - rect.y);
                match label.corner {
                    Corner::TopLeft => assert!(dx < rect.width / 3.0 && dy < rect.height / 3.0),
                    Corner::BottomRight => {
                        assert!(dx > 2.0 * rect.width / 3.0 && dy > 2.0 * rect.height / 3.0);
                    }
                }
                // Contrast: the label wears the other square color.
                let expected = if is_light_square(label.square) {
                    theme.dark
                } else {
                    theme.light
                };
                assert_eq!(label.color, expected, "{label:?}");
            }
            // Numbers on the left column, letters on the bottom row.
            let bottom_left = layout.cell_rect(0, 7);
            for label in all.iter().filter(|l| l.corner == Corner::TopLeft) {
                assert!(label.text.is_ascii_digit());
                assert!(label.at.x < bottom_left.x + bottom_left.width);
            }
            for label in all.iter().filter(|l| l.corner == Corner::BottomRight) {
                assert!(label.text.is_ascii_lowercase());
                assert!(label.at.y > bottom_left.y);
            }
        }
    }

    #[test]
    fn inside_labels_follow_the_orientation() {
        let text_of = |o, corner, nth: usize| {
            labels(o)
                .into_iter()
                .filter(|l| l.corner == corner)
                .nth(nth)
                .map(|l| l.text)
        };
        // White at the bottom: a1 holds "1" ... top-left of a8 is "8".
        assert_eq!(
            text_of(Orientation::White, Corner::BottomRight, 0),
            Some('a')
        );
        assert_eq!(text_of(Orientation::White, Corner::TopLeft, 0), Some('8'));
        assert_eq!(
            text_of(Orientation::Black, Corner::BottomRight, 0),
            Some('h')
        );
        assert_eq!(text_of(Orientation::Black, Corner::TopLeft, 0), Some('1'));
    }

    #[test]
    fn restored_square_keeps_last_move_tint_and_full_marks() {
        let position = Position::new();
        let last = position.find_move(sq("e2"), sq("e4"), None);
        let full = [BoardMark::square(MarkKind::Full, sq("e2"), "red")];
        let options = BoardOptions::default();
        assert_eq!(fills(options, &[], None, "e2"), 1, "just the square");
        assert_eq!(fills(options, &[], last, "e2"), 2, "square and tint");
        assert_eq!(fills(options, &full, last, "e2"), 3, "plus the Full mark");
        assert_eq!(
            fills(options, &full, last, "d4"),
            1,
            "other squares untouched"
        );
        let quiet = BoardOptions {
            show_marks: false,
            highlight_last_move: false,
            ..options
        };
        assert_eq!(fills(quiet, &full, last, "e2"), 1, "options are respected");
    }
}
