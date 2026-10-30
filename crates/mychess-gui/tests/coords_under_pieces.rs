//! Inside coordinates sit in the corners of the edge squares, on the squares
//! layer, *below* the pieces: a piece pixel is never overpainted by a label
//! pixel, and squares without a label render as without coordinates.
//!
//! Pixel relations inside one run (no stored images), through the real
//! canvas. The renderer draws all the images of a layer before all its text,
//! so this holds only because the board puts the pieces in a renderer layer
//! above the one with the labels.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

mod render_common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use iced::Size;
use mychess_core::{Position, Square};
use mychess_gui::theme::board_theme::BoardThemeId;
use mychess_gui::widgets::board::geometry::{BoardLayout, CoordsMode, Orientation};
use mychess_gui::widgets::board::marks::MarkBrush;
use mychess_gui::widgets::board::pieces::PieceSet;
use mychess_gui::widgets::board::{BoardCaches, BoardData, BoardOptions, BoardWidget};
use render_common::{SCALE, Shot, render};

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
/// Kings away from the edges: every edge square is empty.
const KINGS_INSIDE: &str = "8/8/5k2/8/8/5K2/8/8 w - - 0 1";
const SIZE: Size = Size::new(800.0, 800.0);

fn options(coords: CoordsMode, theme: BoardThemeId) -> BoardOptions {
    BoardOptions {
        theme,
        coords,
        highlight_last_move: false,
        ..BoardOptions::default()
    }
}

type Key = (&'static str, Orientation, CoordsMode, BoardThemeId);

/// Rendering is slow; every scene is rendered once per run.
fn shot(fen: &'static str, o: Orientation, opts: BoardOptions) -> Arc<Shot> {
    static CACHE: Mutex<BTreeMap<String, Arc<Shot>>> = Mutex::new(BTreeMap::new());
    let key: Key = (fen, o, opts.coords, opts.theme);
    let key = format!("{key:?}");
    if let Some(found) = CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Arc::clone(found);
    }
    let rendered = Arc::new(draw(fen, o, opts));
    CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, Arc::clone(&rendered));
    rendered
}

fn draw(fen: &str, o: Orientation, opts: BoardOptions) -> Shot {
    let position = Position::from_fen(fen).expect("fen");
    let brush = MarkBrush::default();
    let pieces = PieceSet::default();
    let caches = BoardCaches::default();
    let data = BoardData {
        position: &position,
        last_move: None,
        marks: &[],
        orientation: o,
        options: opts,
        accepts_moves: false,
        brush: &brush,
        pieces: &pieces,
        animation: None,
    };
    render(BoardWidget::new(data, &caches).into_element(), SIZE)
}

fn layout(opts: BoardOptions) -> BoardLayout {
    BoardLayout::compute(SIZE, opts.layout_options()).expect("layout fits")
}

/// The squares that carry a label: the left column and the bottom row.
fn edge_squares(o: Orientation) -> Vec<Square> {
    let l = layout(options(CoordsMode::Inside, BoardThemeId::default()));
    (0..8u8)
        .flat_map(|i| [(0, i), (i, 7)])
        .filter_map(|(col, row)| {
            mychess_gui::widgets::board::geometry::square_from_cell(col, row, o)
        })
        .inspect(|&sq| assert!(l.square_rect(sq, o).width > 0.0))
        .collect()
}

/// Device pixels of a square.
fn pixels_of(
    board: &BoardLayout,
    square: Square,
    orientation: Orientation,
) -> impl Iterator<Item = (u32, u32)> + use<> {
    let r = board.square_rect(square, orientation);
    let (x0, y0) = ((r.x * SCALE) as u32, (r.y * SCALE) as u32);
    let (w, h) = ((r.width * SCALE) as u32, (r.height * SCALE) as u32);
    (y0..y0 + h).flat_map(move |y| (x0..x0 + w).map(move |x| (x, y)))
}

fn linux_only() -> bool {
    cfg!(target_os = "linux")
}

#[test]
fn the_layout_does_not_depend_on_inside_coordinates() {
    let a = layout(options(CoordsMode::Inside, BoardThemeId::default()));
    let b = layout(options(CoordsMode::None, BoardThemeId::default()));
    assert_eq!(a.board, b.board);
    assert_eq!(a.s, b.s);
}

#[test]
fn inside_labels_are_drawn_on_the_edge_squares() {
    if !linux_only() {
        return;
    }
    for o in [Orientation::White, Orientation::Black] {
        let with = shot(
            KINGS_INSIDE,
            o,
            options(CoordsMode::Inside, BoardThemeId::default()),
        );
        let without = shot(
            KINGS_INSIDE,
            o,
            options(CoordsMode::None, BoardThemeId::default()),
        );
        let l = layout(options(CoordsMode::Inside, BoardThemeId::default()));
        for sq in edge_squares(o) {
            let changed = pixels_of(&l, sq, o)
                .filter(|&(x, y)| with.pixel(x, y) != without.pixel(x, y))
                .count();
            assert!(changed > 20, "{sq} {o:?}: only {changed} label pixels");
        }
    }
}

#[test]
fn squares_without_a_label_render_as_without_coordinates() {
    if !linux_only() {
        return;
    }
    for o in [Orientation::White, Orientation::Black] {
        let theme = BoardThemeId::default();
        let with = shot(START, o, options(CoordsMode::Inside, theme));
        let without = shot(START, o, options(CoordsMode::None, theme));
        let l = layout(options(CoordsMode::Inside, theme));
        let edge = edge_squares(o);
        for sq in Square::ALL.into_iter().filter(|sq| !edge.contains(sq)) {
            assert!(
                pixels_of(&l, sq, o).all(|(x, y)| with.pixel(x, y) == without.pixel(x, y)),
                "{sq} {o:?} changed"
            );
        }
        // Nor does anything outside the board change (frame).
        let board = l.board;
        let (bx0, by0) = ((board.x * SCALE) as u32, (board.y * SCALE) as u32);
        let (bx1, by1) = (
            ((board.x + board.width) * SCALE) as u32,
            ((board.y + board.height) * SCALE) as u32,
        );
        for y in 0..(SIZE.height * SCALE) as u32 {
            for x in 0..(SIZE.width * SCALE) as u32 {
                let on_board = x >= bx0 && x < bx1 && y >= by0 && y < by1;
                if !on_board {
                    assert_eq!(with.pixel(x, y), without.pixel(x, y), "({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn opaque_piece_pixels_are_never_covered_by_a_label() {
    if !linux_only() {
        return;
    }
    for o in [Orientation::White, Orientation::Black] {
        let theme = BoardThemeId::ScidBlueWhite;
        let other = BoardThemeId::ScidBrown;
        let l = layout(options(CoordsMode::Inside, theme));
        // Label pixels: where an empty edge square differs with and without.
        let empty_with = shot(KINGS_INSIDE, o, options(CoordsMode::Inside, theme));
        let empty_without = shot(KINGS_INSIDE, o, options(CoordsMode::None, theme));
        // Opaque piece pixels: the same under two board themes (a pixel that
        // lets the square show through changes with the square color).
        let start_in = shot(START, o, options(CoordsMode::Inside, theme));
        let start_out = shot(START, o, options(CoordsMode::None, theme));
        let start_other = shot(START, o, options(CoordsMode::None, other));
        let mut checked = 0;
        for sq in edge_squares(o) {
            for (x, y) in pixels_of(&l, sq, o) {
                let label = empty_with.pixel(x, y) != empty_without.pixel(x, y);
                let opaque_piece = start_out.pixel(x, y) == start_other.pixel(x, y);
                if label && opaque_piece {
                    checked += 1;
                    assert_eq!(
                        start_in.pixel(x, y),
                        start_out.pixel(x, y),
                        "{sq} {o:?}: label paint over a piece at ({x}, {y})"
                    );
                }
            }
        }
        assert!(
            checked >= 10,
            "{o:?}: only {checked} pixels where a label and a piece overlap: \
             the test would not notice labels over pieces"
        );
    }
}
