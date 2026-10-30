//! A text field with a list of suggestions under it (spec M2-4, D193 and
//! rule 43).
//!
//! The list belongs to the caller: it passes the suggestions and the
//! highlighted row, and receives messages for typing, `Enter` and clicks.
//! Moving the highlight with the arrow keys is the shell's job (a text
//! field does not use `Up` and `Down`).

use iced::widget::{button, column, text, text_input};
use iced::{Element, Length};

/// Most rows shown, whatever the caller passes (the Scid shows nine).
pub const MAX_ROWS: usize = 9;

/// A suggestion row: flat, highlighted when it is the current one.
fn row_style(highlighted: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let palette = theme.extended_palette();
        let base = button::Style {
            background: None,
            text_color: palette.background.base.text,
            border: iced::Border::default(),
            ..button::Style::default()
        };
        if highlighted || matches!(status, button::Status::Hovered | button::Status::Pressed) {
            button::Style {
                background: Some(palette.primary.weak.color.into()),
                text_color: palette.primary.weak.text,
                ..base
            }
        } else {
            base
        }
    }
}

/// The field and, when `suggestions` is not empty, the list under it.
///
/// `highlight` is the row that `Enter` would pick (the caller decides what
/// `on_submit` means); `on_pick` receives the clicked row.
// The field, the list, the highlight and the three messages are all needed
// to draw and to answer; a parameter struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn view<'a, M: Clone + 'a>(
    id: &'static str,
    placeholder: &str,
    value: &str,
    suggestions: &[String],
    highlight: Option<usize>,
    on_input: impl Fn(String) -> M + 'a,
    on_submit: M,
    on_pick: impl Fn(usize) -> M + 'a,
) -> Element<'a, M> {
    let field = text_input(placeholder, value)
        .id(id)
        .on_input(on_input)
        .on_submit(on_submit)
        .padding(5)
        .width(Length::Fill);
    let mut out = column![field].spacing(1).width(Length::Fill);
    for (index, name) in suggestions.iter().take(MAX_ROWS).enumerate() {
        out = out.push(
            button(text(name.clone()).size(13))
                .on_press(on_pick(index))
                .padding([2, 6])
                .width(Length::Fill)
                .style(row_style(highlight == Some(index))),
        );
    }
    out.into()
}

/// The next highlighted row after moving by `delta` among `count` rows
/// (wrapping; `None` when the list is empty).
pub fn move_highlight(current: Option<usize>, delta: isize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let count = count.min(MAX_ROWS);
    let Ok(len) = isize::try_from(count) else {
        return None;
    };
    let start = match (current, delta >= 0) {
        (Some(at), _) => isize::try_from(at).unwrap_or(0),
        // From "nothing": down goes to the first row, up to the last.
        (None, true) => -1,
        (None, false) => len,
    };
    let next = (start + delta).rem_euclid(len);
    usize::try_from(next).ok()
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Input(String),
        Submit,
        Pick(usize),
    }

    fn sample(rows: &[&str]) -> Element<'static, Msg> {
        let rows: Vec<String> = rows.iter().map(|s| (*s).to_owned()).collect();
        view(
            "sample-field",
            "name",
            "ca",
            &rows,
            Some(0),
            Msg::Input,
            Msg::Submit,
            Msg::Pick,
        )
    }

    #[test]
    fn a_click_on_a_row_picks_it() {
        let mut sim = Simulator::with_size(
            Settings::default(),
            Size::new(400.0, 300.0),
            sample(&["Carlsen, M.", "Caruana, F."]),
        );
        sim.click("Caruana, F.").expect("row");
        let sent: Vec<Msg> = sim.into_messages().collect();
        assert_eq!(sent, [Msg::Pick(1)]);
    }

    #[test]
    fn typing_and_enter_reach_the_caller() {
        let mut sim =
            Simulator::with_size(Settings::default(), Size::new(400.0, 300.0), sample(&[]));
        sim.click(iced::widget::Id::new("sample-field"))
            .expect("field");
        sim.typewrite("r");
        sim.tap_key(iced::keyboard::key::Named::Enter);
        let sent: Vec<Msg> = sim.into_messages().collect();
        assert_eq!(sent, [Msg::Input("car".to_owned()), Msg::Submit]);
    }

    #[test]
    fn no_suggestions_means_only_the_field() {
        let mut sim =
            Simulator::with_size(Settings::default(), Size::new(400.0, 300.0), sample(&[]));
        assert!(sim.find("Carlsen, M.").is_err());
    }

    #[test]
    fn the_highlight_wraps_in_both_directions() {
        assert_eq!(move_highlight(None, 1, 3), Some(0));
        assert_eq!(move_highlight(None, -1, 3), Some(2));
        assert_eq!(move_highlight(Some(2), 1, 3), Some(0));
        assert_eq!(move_highlight(Some(0), -1, 3), Some(2));
        assert_eq!(move_highlight(Some(1), 1, 3), Some(2));
        assert_eq!(move_highlight(Some(0), 1, 0), None);
        assert_eq!(
            move_highlight(Some(0), 1, 50),
            Some(1),
            "capped at nine rows"
        );
    }
}
