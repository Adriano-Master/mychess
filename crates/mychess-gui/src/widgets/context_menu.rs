//! A popup menu over everything else (spec M2-4, D172).
//!
//! [`layer`] is a window-sized layer for a `stack`: a transparent backdrop
//! that closes the menu when clicked (any button) and the card pinned at
//! the pointer, moved inwards so that it stays inside the window
//! ([`place`]).

use iced::widget::{
    button, column, container, mouse_area, opaque, pin, rule, scrollable, space, stack, text,
};
use iced::{Element, Length, Padding, Point, Size};

use crate::theme;

/// Height of a menu row in logical pixels (used to size the card).
pub const ROW_HEIGHT: f32 = 28.0;

/// Height of a separator.
pub const SEPARATOR_HEIGHT: f32 = 9.0;

/// One row of the menu.
pub enum Entry<'a, M> {
    /// A clickable row; without a message it is shown disabled.
    Item {
        /// The text.
        label: String,
        /// What pressing it sends.
        message: Option<M>,
    },
    /// A line between groups.
    Separator,
    /// A caption, not clickable.
    Heading(String),
    /// Any widget of the given height.
    Block(Element<'a, M>, f32),
}

impl<M> Entry<'_, M> {
    fn height(&self) -> f32 {
        match self {
            Entry::Item { .. } | Entry::Heading(_) => ROW_HEIGHT,
            Entry::Separator => SEPARATOR_HEIGHT,
            Entry::Block(_, height) => *height,
        }
    }
}

/// The size a menu with these rows takes.
pub fn card_size<M>(entries: &[Entry<'_, M>], width: f32) -> Size {
    let rows: f32 = entries.iter().map(Entry::height).sum();
    Size::new(width + 2.0 * CARD_PADDING, rows + 2.0 * CARD_PADDING + 2.0)
}

/// Space between the card border and its rows.
const CARD_PADDING: f32 = 4.0;

/// The size of the card as it is drawn in a window of `window`: never
/// taller than the window (the rows scroll then).
pub fn shown_size(size: Size, window: Size) -> Size {
    if window.height.is_finite() && window.height > 0.0 {
        Size::new(size.width, size.height.min(window.height))
    } else {
        size
    }
}

/// The card with the rows. When the rows are taller than `max_height` (a
/// low window) the card is `max_height` tall and the rows scroll.
pub fn card<'a, M: Clone + 'a>(
    entries: Vec<Entry<'a, M>>,
    width: f32,
    max_height: f32,
) -> Element<'a, M> {
    let natural = card_size(&entries, width).height;
    let mut rows = column![].width(Length::Fixed(width));
    for entry in entries {
        rows = match entry {
            Entry::Item { label, message } => rows.push(
                button(text(label).size(14))
                    .width(Length::Fill)
                    .height(Length::Fixed(ROW_HEIGHT))
                    .padding(Padding::from([4, 10]))
                    .style(theme::menu_item)
                    .on_press_maybe(message),
            ),
            Entry::Separator => rows.push(
                container(rule::horizontal(1))
                    .height(Length::Fixed(SEPARATOR_HEIGHT))
                    .padding(Padding::from([4, 6])),
            ),
            Entry::Heading(label) => rows.push(
                container(text(label).size(13).style(theme::muted_text))
                    .height(Length::Fixed(ROW_HEIGHT))
                    .padding(Padding::from([5, 10])),
            ),
            Entry::Block(element, height) => rows.push(
                container(element)
                    .height(Length::Fixed(height))
                    .padding(Padding::from([0, 6])),
            ),
        };
    }
    if max_height.is_finite() && natural > max_height {
        let inner = (max_height - 2.0 * CARD_PADDING - 2.0).max(ROW_HEIGHT);
        return container(scrollable(rows).height(Length::Fixed(inner)))
            .padding(CARD_PADDING)
            .style(theme::pane_body)
            .into();
    }
    container(rows)
        .padding(CARD_PADDING)
        .style(theme::pane_body)
        .into()
}

/// Where to put a card of `size` opened at `at` in a window of `window`:
/// at the pointer when it fits, else moved left and up until it does.
pub fn place(at: Point, size: Size, window: Size) -> Point {
    let fit = |start: f32, length: f32, limit: f32| {
        if !(start.is_finite() && limit.is_finite()) {
            return 0.0;
        }
        (limit - length).min(start).max(0.0)
    };
    Point::new(
        fit(at.x, size.width, window.width),
        fit(at.y, size.height, window.height),
    )
}

/// The layer for a `stack`: the backdrop and `card` at `position`.
/// `close` is sent by a click outside the card.
pub fn layer<'a, M: Clone + 'a>(card: Element<'a, M>, position: Point, close: M) -> Element<'a, M> {
    let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
        .on_press(close.clone())
        .on_right_press(close.clone())
        .on_middle_press(close);
    stack![backdrop, pin(opaque(card)).x(position.x).y(position.y)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_that_fits_opens_at_the_pointer() {
        let at = place(
            Point::new(100.0, 50.0),
            Size::new(200.0, 300.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(at, Point::new(100.0, 50.0));
    }

    #[test]
    fn a_card_near_the_edges_is_moved_inside_the_window() {
        let size = Size::new(200.0, 300.0);
        let window = Size::new(800.0, 600.0);
        assert_eq!(
            place(Point::new(790.0, 590.0), size, window),
            Point::new(600.0, 300.0)
        );
        assert_eq!(
            place(Point::new(-5.0, f32::NAN), size, window),
            Point::new(0.0, 0.0)
        );
    }

    #[test]
    fn a_card_larger_than_the_window_sticks_to_the_top_left() {
        let at = place(
            Point::new(40.0, 40.0),
            Size::new(900.0, 900.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(at, Point::new(0.0, 0.0));
    }

    #[test]
    fn a_tall_card_is_cut_to_the_window() {
        let tall = Size::new(200.0, 700.0);
        let window = Size::new(800.0, 400.0);
        assert_eq!(shown_size(tall, window), Size::new(200.0, 400.0));
        let small = Size::new(200.0, 100.0);
        assert_eq!(shown_size(small, window), small);
        assert_eq!(shown_size(tall, Size::new(800.0, f32::NAN)), tall);
    }

    #[test]
    fn the_card_height_is_the_sum_of_the_rows() {
        let entries: Vec<Entry<'_, ()>> = vec![
            Entry::Item {
                label: String::new(),
                message: None,
            },
            Entry::Separator,
            Entry::Heading(String::new()),
        ];
        let size = card_size(&entries, 200.0);
        assert_eq!(size.width, 208.0);
        assert_eq!(size.height, ROW_HEIGHT * 2.0 + SEPARATOR_HEIGHT + 10.0);
    }
}
