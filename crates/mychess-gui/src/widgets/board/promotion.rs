//! Pure geometry of the promotion column drawn over the board.
//!
//! The first cell (queen) sits exactly on the destination square and the
//! others follow towards the inside of the board (Scid: `getPromoPiece`,
//! `tcl/main.tcl:1203-1243`, shows the same four choices in a dialog).

use iced::{Point, Rectangle};
use mychess_core::{Role, Square};

use super::geometry::{BoardLayout, Orientation, display_cell};
use super::moves::PROMOTION_ROLES;

/// One choice of the promotion column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PromotionCell {
    /// The piece the pawn becomes.
    pub role: Role,
    /// Where the choice is drawn and clicked.
    pub rect: Rectangle,
}

/// The four cells (queen, rook, bishop, knight) for a promotion on `to`.
pub fn promotion_cells(layout: &BoardLayout, to: Square, o: Orientation) -> [PromotionCell; 4] {
    let (col, row) = display_cell(to, o);
    // Towards the inside of the board: down from the top half, up from the
    // bottom half (promotion squares are on display row 0 or 7).
    let down = row <= 3;
    PROMOTION_ROLES.iter().enumerate().fold(
        [PromotionCell {
            role: Role::Queen,
            rect: Rectangle::default(),
        }; 4],
        |mut cells, (i, role)| {
            let step = u8::try_from(i).unwrap_or(0);
            let r = if down { row + step } else { row - step };
            cells[i] = PromotionCell {
                role: *role,
                rect: layout.cell_rect(col, r),
            };
            cells
        },
    )
}

/// The piece whose cell contains `p`, if any.
pub fn promotion_role_at(cells: &[PromotionCell; 4], p: Point) -> Option<Role> {
    cells.iter().find(|c| c.rect.contains(p)).map(|c| c.role)
}

#[cfg(test)]
mod tests {
    use iced::Size;
    use rstest::rstest;

    use super::*;
    use crate::widgets::board::geometry::{CoordsMode, LayoutOptions};

    fn layout() -> BoardLayout {
        BoardLayout::compute(
            Size::new(800.0, 800.0),
            LayoutOptions {
                coords: CoordsMode::None,
            },
        )
        .expect("layout fits")
    }

    #[rstest]
    #[case("a8", Orientation::White)]
    #[case("h8", Orientation::White)]
    #[case("a1", Orientation::White)]
    #[case("h1", Orientation::White)]
    #[case("a8", Orientation::Black)]
    #[case("h1", Orientation::Black)]
    fn first_cell_covers_the_target_and_all_cells_stay_inside(
        #[case] to: &str,
        #[case] o: Orientation,
    ) {
        let layout = layout();
        let to: Square = to.parse().expect("square");
        let cells = promotion_cells(&layout, to, o);
        assert_eq!(cells[0].rect, layout.square_rect(to, o));
        assert_eq!(cells[0].role, Role::Queen);
        let roles: Vec<Role> = cells.iter().map(|c| c.role).collect();
        assert_eq!(roles, PROMOTION_ROLES);
        for cell in &cells {
            assert!(layout.board.contains(cell.rect.center()));
        }
        // Same column, consecutive rows.
        for pair in cells.windows(2) {
            assert_eq!(pair[0].rect.x, pair[1].rect.x);
            assert_eq!((pair[0].rect.y - pair[1].rect.y).abs(), layout.s);
        }
    }

    #[test]
    fn column_grows_downwards_from_the_top_row() {
        let layout = layout();
        let cells = promotion_cells(&layout, "e8".parse().expect("square"), Orientation::White);
        assert!(cells[1].rect.y > cells[0].rect.y);
    }

    #[test]
    fn column_grows_upwards_from_the_bottom_row() {
        let layout = layout();
        let cells = promotion_cells(&layout, "e1".parse().expect("square"), Orientation::White);
        assert!(cells[1].rect.y < cells[0].rect.y);
    }

    #[test]
    fn role_at_finds_the_clicked_cell() {
        let layout = layout();
        let cells = promotion_cells(&layout, "e8".parse().expect("square"), Orientation::White);
        let knight = cells[3].rect.center();
        assert_eq!(promotion_role_at(&cells, knight), Some(Role::Knight));
        assert_eq!(promotion_role_at(&cells, Point::new(1.0, 799.0)), None);
    }
}
