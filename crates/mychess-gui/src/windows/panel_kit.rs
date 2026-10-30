//! Small widgets and helpers shared by the four panels of slice M6-4d (the
//! crosstable, the statistics, the rating graph and the piece tracker).

use std::fmt;

use iced::alignment::Horizontal;
use iced::font::Weight;
use iced::widget::{button, checkbox, progress_bar, text};
use iced::{Element, Font, Length};

use crate::shell::Message;
use crate::theme::{self, ButtonLook};

/// The size of the text of the tables and the fields.
pub const BODY_TEXT: f32 = 13.0;

/// The bold font.
pub fn bold() -> Font {
    Font {
        weight: Weight::Bold,
        ..Font::DEFAULT
    }
}

/// A flat button that is filled with the accent while `active` (a tab or
/// one of a group of choices).
pub fn choice<'a>(label: String, active: bool, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(BODY_TEXT))
        .padding([3, 10])
        .style(theme::command_button(ButtonLook::Secondary, active))
        .on_press_maybe(message)
        .into()
}

/// A small push button.
pub fn action<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(BODY_TEXT))
        .padding([3, 10])
        .style(button::secondary)
        .on_press_maybe(message)
        .into()
}

/// A checkbox; without `change` it is shown disabled.
pub fn check<'a>(
    label: String,
    on: bool,
    change: Option<impl Fn(bool) -> Message + 'a>,
) -> Element<'a, Message> {
    let b = checkbox(on).label(label).size(15).text_size(BODY_TEXT);
    match change {
        Some(change) => b.on_toggle(change).into(),
        None => b.into(),
    }
}

/// A fixed-width cell of text.
pub fn cell<'a>(content: String, width: f32, align: Horizontal) -> iced::widget::Text<'a> {
    text(content)
        .size(BODY_TEXT)
        .width(Length::Fixed(width))
        .align_x(align)
}

/// A muted line of text.
pub fn muted<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(theme::muted_text).into()
}

/// A line of text in the danger tone (a failure or a warning).
pub fn danger<'a>(content: String) -> Element<'a, Message> {
    text(content).size(12).style(text::danger).into()
}

/// The bar of a computation in flight: `done` of `total`, none when the
/// total is not known yet.
pub fn progress<'a>(done: u64, total: u64) -> Element<'a, Message> {
    let fraction = if total == 0 {
        0.0
    } else {
        // Only a ratio for a bar: the precision lost is irrelevant.
        #[allow(clippy::cast_precision_loss)]
        {
            (done as f32 / total as f32).clamp(0.0, 1.0)
        }
    };
    progress_bar(0.0..=1.0, fraction).girth(6).into()
}

/// A value of a list of choices with the text to show.
#[derive(Debug, Clone)]
pub struct Choice<T> {
    /// The value.
    pub value: T,
    /// The text.
    pub label: String,
}

impl<T> fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl<T: PartialEq> PartialEq for Choice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

pub use crate::theme::chart_colors::parse_hex;

/// The current year in UTC, from the clock (the application only takes it by
/// message, D3415). 2000 if the clock is before 1970.
pub fn current_year() -> u16 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    year_of_days(i64::try_from(seconds / 86_400).unwrap_or(0))
}

/// The civil year of a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
pub fn year_of_days(days: i64) -> u16 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let year = yoe + era * 400 + i64::from(mp >= 10);
    u16::try_from(year).unwrap_or(2000)
}

/// A row whose children flow onto more lines when the pane is narrow.
pub fn flow(row: iced::widget::Row<'_, Message>) -> Element<'_, Message> {
    row.wrap().into()
}

#[cfg(test)]
mod tests {
    use iced::Color;

    use super::*;

    #[test]
    fn hex_colors_are_parsed_strictly() {
        assert_eq!(parse_hex("#cccccc"), Some(Color::from_rgb8(204, 204, 204)));
        assert_eq!(parse_hex("cccccc"), None);
        assert_eq!(parse_hex("#ccc"), None);
        assert_eq!(parse_hex("#gggggg"), None);
        assert_eq!(parse_hex("#ccccccé"), None);
    }

    #[test]
    fn the_year_comes_from_the_day_count() {
        assert_eq!(year_of_days(0), 1970);
        assert_eq!(year_of_days(365), 1971);
        // 2020-02-29 is day 18321; 2020-12-31 is day 18627; 2021-01-01 is 18628.
        assert_eq!(year_of_days(18_321), 2020);
        assert_eq!(year_of_days(18_627), 2020);
        assert_eq!(year_of_days(18_628), 2021);
        assert!(current_year() >= 2024);
    }

    #[test]
    fn choices_compare_by_value() {
        let a = Choice {
            value: 1,
            label: "a".into(),
        };
        let b = Choice {
            value: 1,
            label: "b".into(),
        };
        assert_eq!(a, b);
        assert_eq!(a.to_string(), "a");
    }
}
