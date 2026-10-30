//! Every embedded piece set rasterizes with the SVG renderer of iced (resvg):
//! none of the 96 files comes out empty or fails to parse (CSS styles,
//! gradients and filters are the usual culprits).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

mod render_common;

use iced::widget::{Row, column, container, svg};
use iced::{Element, Length, Size};
use mychess_core::{Color, Piece, Role};
use mychess_gui::widgets::board::pieces::{PieceSet, PieceSetId};
use render_common::{SCALE, Shot, render};

const CELL: u32 = 64;
const COLORS: [Color; 2] = [Color::White, Color::Black];

/// The 12 pieces of a set in a grid: White's row above Black's.
fn sheet(set: &PieceSet) -> Shot {
    let rows = COLORS.map(|color| {
        let cells = Role::ALL.map(|role| {
            let handle = set
                .handle(Piece { color, role })
                .expect("every piece has a handle")
                .clone();
            let image = svg(handle)
                .width(Length::Fixed(CELL as f32))
                .height(Length::Fixed(CELL as f32));
            Element::from(container(image))
        });
        Row::with_children(cells)
    });
    let [white, black] = rows;
    let size = Size::new((6 * CELL) as f32, (2 * CELL) as f32);
    render::<()>(column![white, black], size)
}

/// The device pixels of the cell of `piece`, as `(x, y)` offsets inside it.
fn cell_pixels(piece: Piece) -> impl Iterator<Item = (u32, u32)> {
    let col = Role::ALL
        .iter()
        .position(|&r| r == piece.role)
        .expect("a role") as u32;
    let row = u32::from(piece.color == Color::Black);
    let side = CELL * SCALE as u32;
    let (x0, y0) = (col * side, row * side);
    (0..side).flat_map(move |y| (0..side).map(move |x| (x0 + x, y0 + y)))
}

#[test]
fn every_piece_of_every_set_renders_and_black_differs_from_white() {
    if !cfg!(target_os = "linux") {
        return;
    }
    for id in PieceSetId::ALL {
        let shot = sheet(&PieceSet::load(id));
        let background = shot.pixel(shot.width - 1, 0);
        let side = CELL * SCALE as u32;
        for color in COLORS {
            for role in Role::ALL {
                let piece = Piece { color, role };
                let covered = cell_pixels(piece)
                    .filter(|&(x, y)| shot.pixel(x, y) != background)
                    .count();
                let share = covered as f32 / (side * side) as f32;
                assert!(
                    share > 0.04,
                    "{id:?} {piece:?}: only {:.1}% of the cell is drawn",
                    share * 100.0
                );
                assert!(share < 0.98, "{id:?} {piece:?}: fills the whole cell");
            }
        }
        for role in Role::ALL {
            let white = cell_pixels(Piece {
                color: Color::White,
                role,
            });
            let black = cell_pixels(Piece {
                color: Color::Black,
                role,
            });
            let differing = white
                .zip(black)
                .filter(|&((wx, wy), (bx, by))| shot.pixel(wx, wy) != shot.pixel(bx, by))
                .count();
            assert!(differing > 50, "{id:?} {role:?}: white equals black");
        }
    }
}
