//! Surfaces of the opening book panel (spec M5-6c, D2040-D2049): the book
//! information card stands out with the accent, the tools card is dimmer,
//! and the groups, chips and notices are tints of the same tokens. Nothing
//! here hardcodes a color.

use iced::widget::{button, container, progress_bar, text};
use iced::{Border, Theme};

use super::tokens::{self, Tokens};
use super::{ButtonLook, command_button};

/// Share of the accent in the border of the information card.
const INFO_BORDER_TINT: f32 = 0.55;
/// Share of the tone in the background of a notice.
/// Share of `raised` in the surface of the information card.
const CARD_TINT: f32 = 0.5;
/// Share of the tone in the background of a notice.
const NOTICE_TINT: f32 = 0.10;

/// The surface of the information card: halfway between `raised` and
/// `panel`, the closest to `raised` at which every accent keeps 4.5:1 for
/// the title (the test of this module sweeps the themes and accents).
fn card_surface(t: &Tokens) -> iced::Color {
    tokens::mix(t.raised, t.panel, CARD_TINT)
}

/// The main card of a book: raised, with an accent border, so it reads as
/// the information of the book.
pub fn info_card(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(card_surface(&t).into()),
        border: Border {
            color: tokens::mix(t.accent, t.border, INFO_BORDER_TINT),
            width: 1.0,
            radius: t.radius.panel.into(),
        },
        ..container::Style::default()
    }
}

/// The card of the tools and settings: the window color with a hairline, so
/// it recedes from the information.
pub fn tools_card(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.muted),
        background: Some(t.window.into()),
        border: Border {
            color: t.border,
            width: 1.0,
            radius: t.radius.panel.into(),
        },
        ..container::Style::default()
    }
}

/// A group inside the tools card.
pub fn tools_group(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(t.panel.into()),
        border: Border {
            radius: t.radius.card.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A small pill (changed, read-only, opening book). A `danger` chip uses the
/// danger tint; the others the muted text. The text always says what it is.
pub fn chip(danger: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let t = Tokens::of(theme);
        let color = if danger { t.danger } else { t.muted };
        container::Style {
            text_color: Some(color),
            border: Border {
                color,
                width: 1.0,
                radius: t.radius.chip.into(),
            },
            ..container::Style::default()
        }
    }
}

/// The strip of a notice: a tint of the accent, or of the danger color for a
/// failure.
pub fn notice(failure: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let t = Tokens::of(theme);
        let tone = if failure { t.danger } else { t.accent };
        container::Style {
            text_color: Some(t.text),
            background: Some(tokens::mix(tone, t.panel, NOTICE_TINT).into()),
            border: Border {
                color: tone,
                width: 1.0,
                radius: t.radius.card.into(),
            },
            ..container::Style::default()
        }
    }
}

/// The bar of a move weight: accent over a recessed track.
pub fn weight_bar(theme: &Theme) -> progress_bar::Style {
    let t = Tokens::of(theme);
    progress_bar::Style {
        background: t.window.into(),
        bar: t.accent.into(),
        border: Border {
            radius: t.radius.chip.into(),
            ..Border::default()
        },
    }
}

/// The text of a failure: the `danger` token (not the stock iced color, so
/// the contrast checks cover it).
pub fn danger_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(Tokens::of(theme).danger),
    }
}

/// A button of the panel. `inset` buttons sit on the information card (same
/// `raised` surface as the card): they get the `panel` surface and a hairline
/// so they keep a shape at rest. The others are the plain command buttons.
pub fn button_style(inset: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let plain = command_button(ButtonLook::Primary, false)(theme, status);
        if !inset {
            return plain;
        }
        let t = Tokens::of(theme);
        let background = match status {
            button::Status::Active | button::Status::Disabled => t.panel,
            button::Status::Hovered => t.hover,
            button::Status::Pressed => t.pressed,
        };
        button::Style {
            background: Some(background.into()),
            text_color: if status == button::Status::Disabled {
                t.disabled()
            } else {
                t.text
            },
            border: Border {
                color: t.border,
                width: 1.0,
                radius: t.radius.control.into(),
            },
            ..plain
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::{Background, Color};

    use super::super::accent::AccentId;
    use super::super::contrast::{MIN_GRAPHIC, MIN_TEXT, contrast_ratio};
    use super::super::palette::iced_theme;
    use super::*;

    /// Every theme x accent of the application.
    fn themes() -> Vec<(String, Theme)> {
        let mut all = Vec::new();
        for dark in [true, false] {
            for accent in AccentId::ALL {
                all.push((format!("dark={dark}/{accent:?}"), iced_theme(dark, accent)));
            }
        }
        all
    }

    fn fill(style: &container::Style) -> Color {
        match style.background {
            Some(Background::Color(color)) => color,
            other => panic!("no flat background: {other:?}"),
        }
    }

    thread_local! {
        static FAILED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Records a failure (all of them are reported together at the end).
    fn check(name: &str, what: &str, text: Color, background: Color, minimum: f32) {
        let ratio = contrast_ratio(text, background);
        if ratio < minimum {
            FAILED.with(|f| {
                f.borrow_mut()
                    .push(format!("{name}: {what} is {ratio:.2}:1"));
            });
        }
    }

    fn done() {
        let failed = FAILED.with(|f| std::mem::take(&mut *f.borrow_mut()));
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    #[test]
    fn the_text_of_the_information_card_reads_in_every_theme_and_accent() {
        for (name, theme) in themes() {
            let t = Tokens::of(&theme);
            let card = fill(&info_card(&theme));
            // The title (15 px) and the move in the game, in the accent.
            check(&name, "accent on the card", t.accent, card, MIN_TEXT);
            check(&name, "text on the card", t.text, card, MIN_TEXT);
            check(&name, "muted on the card", t.muted, card, MIN_TEXT);
            // The 11 px chips: text and border on the card.
            check(&name, "danger chip", t.danger, card, MIN_TEXT);
            // The weight bar over its track.
            let bar = weight_bar(&theme);
            let Background::Color(track) = bar.background else {
                panic!("flat bar")
            };
            let Background::Color(fill) = bar.bar else {
                panic!("flat bar")
            };
            check(&name, "weight bar", fill, track, MIN_GRAPHIC);
        }
        done();
    }

    #[test]
    fn the_tools_and_their_groups_read_in_every_theme_and_accent() {
        for (name, theme) in themes() {
            let t = Tokens::of(&theme);
            let card = tools_card(&theme);
            let group = tools_group(&theme);
            check(
                &name,
                "muted on the tools card",
                t.muted,
                fill(&card),
                MIN_TEXT,
            );
            check(&name, "muted on a group", t.muted, fill(&group), MIN_TEXT);
            check(&name, "text on a group", t.text, fill(&group), MIN_TEXT);
            // The buttons of the card and of the groups.
            for (what, surface) in [("window", t.window), ("panel", t.panel)] {
                check(&name, what, t.muted, surface, MIN_TEXT);
            }
        }
        done();
    }

    #[test]
    fn the_notices_and_the_failure_text_read_in_every_theme_and_accent() {
        for (name, theme) in themes() {
            let t = Tokens::of(&theme);
            let failure = fill(&notice(true)(&theme));
            let plain = fill(&notice(false)(&theme));
            check(&name, "failure text", t.danger, failure, MIN_TEXT);
            check(&name, "text on a failure", t.text, failure, MIN_TEXT);
            check(&name, "text on a notice", t.text, plain, MIN_TEXT);
            // The chips on the card surface keep their border readable.
            check(
                &name,
                "muted chip",
                t.muted,
                fill(&info_card(&theme)),
                MIN_TEXT,
            );
        }
        done();
    }

    #[test]
    fn the_inset_buttons_have_a_surface_and_a_border_at_rest() {
        for (name, theme) in themes() {
            let t = Tokens::of(&theme);
            let style = button_style(true)(&theme, button::Status::Active);
            let background = match style.background {
                Some(Background::Color(color)) => color,
                other => panic!("{name}: {other:?}"),
            };
            assert_ne!(background, fill(&info_card(&theme)), "{name}");
            assert!(style.border.width > 0.0, "{name}");
            check(
                &name,
                "label on the inset button",
                t.text,
                background,
                MIN_TEXT,
            );
        }
        done();
    }
}
