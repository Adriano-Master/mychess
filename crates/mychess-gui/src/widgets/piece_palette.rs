//! The twelve pieces of the "set up position" dialog (spec M2-4, D194).

use iced::widget::{button, column, row, svg};
use iced::{Element, Length};
use mychess_core::{Color, Piece, Role};

use crate::widgets::board::pieces::PieceSet;

/// The order of the palette (and of the Scid's brush): `K Q R B N P`.
pub const ROLES: [Role; 6] = [
    Role::King,
    Role::Queen,
    Role::Rook,
    Role::Bishop,
    Role::Knight,
    Role::Pawn,
];

/// Every piece of the palette: white row first, then black.
pub fn pieces() -> [Piece; 12] {
    let make = |color| ROLES.map(|role| Piece { color, role });
    let white = make(Color::White);
    let black = make(Color::Black);
    [
        white[0], white[1], white[2], white[3], white[4], white[5], black[0], black[1], black[2],
        black[3], black[4], black[5],
    ]
}

/// The palette: two rows of six buttons; the `selected` piece is
/// highlighted. `size` is the side of a button in logical pixels.
pub fn view<'a, M: Clone + 'a>(
    set: &PieceSet,
    selected: Piece,
    size: f32,
    on_select: impl Fn(Piece) -> M,
) -> Element<'a, M> {
    let make_row = |color| {
        let mut out = row![].spacing(2);
        for role in ROLES {
            let piece = Piece { color, role };
            let mut label: Element<'a, M> = iced::widget::space().into();
            if let Some(handle) = set.handle(piece) {
                label = svg(handle.clone())
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size))
                    .into();
            }
            let chosen = piece == selected;
            out = out.push(button(label).padding(2).on_press(on_select(piece)).style(
                move |theme, status| {
                    if chosen {
                        button::primary(theme, status)
                    } else {
                        button::secondary(theme, status)
                    }
                },
            ));
        }
        out
    };
    column![make_row(Color::White), make_row(Color::Black)]
        .spacing(2)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_lists_twelve_distinct_pieces_white_first() {
        let all = pieces();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(all[..6].iter().all(|p| p.color == Color::White));
        assert!(all[6..].iter().all(|p| p.color == Color::Black));
        assert_eq!(all[0].role, Role::King);
        assert_eq!(all[5].role, Role::Pawn);
    }
}
