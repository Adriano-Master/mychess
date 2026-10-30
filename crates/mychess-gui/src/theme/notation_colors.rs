//! Colors of the notation panel by semantic role (spec M2-5, D217, D221,
//! D245, D246).
//!
//! Every color comes from the [`Tokens`] of the theme: the roles of the
//! M2-4 (header, variation, annotation, comment, current move...) keep their
//! names and now map onto the new palette, and the table adds its own (zebra
//! rows, cards, the current-move pill, the colors of the figurines and the
//! tones of the annotations).

use iced::{Color, Theme};
use mychess_core::Nag;

use super::contrast::relative_luminance;
use super::palette::NagTones;
use super::tokens::{Tokens, mix};

/// Share of the accent in the soft highlight of the current move inside the
/// running text and the variation cards (the text color is kept, so the
/// tint has to stay light).
const SOFT_CURRENT: f32 = 0.10;

/// The colors the notation widget paints with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotationPalette {
    /// Game header lines and the result.
    pub header: Color,
    /// Main-line moves and move numbers.
    pub main: Color,
    /// Moves, numbers and parentheses inside variations.
    pub variation: Color,
    /// Annotation symbols without a tone of their own.
    pub nag: Color,
    /// Comments (D247: discreet).
    pub comment: Color,
    /// Soft background of the current move inside running text and cards
    /// (translucent: it blends with what is behind).
    pub current: Color,
    /// The pill behind the current move of a table cell (D246).
    pub pill: Color,
    /// Text over [`pill`](Self::pill).
    pub on_pill: Color,
    /// Background of the item under the pointer.
    pub hover: Color,
    /// Background of the item under the pointer inside a card.
    pub card_hover: Color,
    /// Mark of the item a context menu acts on (outline in the table, soft
    /// fill in the text modes).
    pub target: Color,
    /// The scroll bar track.
    pub track: Color,
    /// The scroll bar thumb.
    pub thumb: Color,
    /// Even rows of the table.
    pub zebra: Color,
    /// Background of a variation card.
    pub card: Color,
    /// The bar at the left edge of a card.
    pub card_bar: Color,
    /// Secondary text: move numbers, times.
    pub muted: Color,
    /// Figurine of a White move.
    pub white_piece: Color,
    /// Figurine of a Black move.
    pub black_piece: Color,
    /// Bar of the time column.
    pub time_bar: Color,
    /// The tones of the annotations.
    pub tones: NagTones,
    /// Corner radius of a card.
    pub card_radius: f32,
    /// Corner radius of the pill and of the highlights of the table.
    pub pill_radius: f32,
}

/// The background under the pointer inside a card. On a dark card the
/// `hover` token is as light as the colored annotations allow; on a light
/// one it would not show against the card, so the text color is mixed in.
fn card_hover(t: &Tokens) -> Color {
    if relative_luminance(t.raised) < 0.35 {
        t.hover
    } else {
        mix(t.text, t.raised, 0.05)
    }
}

impl NotationPalette {
    /// The palette of `theme`.
    pub fn of(theme: &Theme) -> NotationPalette {
        NotationPalette::from_tokens(&Tokens::of(theme))
    }

    /// The palette of a set of tokens.
    pub fn from_tokens(t: &Tokens) -> NotationPalette {
        NotationPalette {
            header: t.text,
            main: t.text,
            variation: t.variation,
            nag: t.muted,
            comment: t.muted,
            current: t.accent.scale_alpha(SOFT_CURRENT),
            pill: t.accent,
            on_pill: t.on_accent,
            hover: t.hover,
            card_hover: card_hover(t),
            target: t.accent,
            track: t.muted.scale_alpha(0.12),
            thumb: t.muted.scale_alpha(0.5),
            zebra: t.zebra,
            card: t.raised,
            card_bar: t.accent.scale_alpha(0.5),
            muted: t.muted,
            // The figurine of the side that moves first is the stronger ink;
            // both pass the text contrast over every row background.
            white_piece: t.text,
            black_piece: t.muted,
            time_bar: t.accent,
            tones: t.nag,
            card_radius: t.radius.card,
            pill_radius: t.radius.pill,
        }
    }

    /// The color of the symbol of `nag`, by tone (D245): `!!`, `!`, `!?`,
    /// `?!`, `?`, `??`, the evaluations `$10` to `$21`, and the rest.
    pub fn nag_color(&self, nag: Nag) -> Color {
        match nag.value() {
            3 => self.tones.brilliant,
            1 => self.tones.good,
            5 => self.tones.interesting,
            6 => self.tones.dubious,
            2 => self.tones.mistake,
            4 => self.tones.blunder,
            10..=21 => self.tones.eval,
            _ => self.nag,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast::{MIN_GRAPHIC, MIN_TEXT, contrast_ratio};

    /// Both themes with every accent (spec M9-1, D1636).
    fn both() -> Vec<(String, Tokens)> {
        [true, false]
            .into_iter()
            .flat_map(|dark| {
                crate::theme::AccentId::ALL.into_iter().map(move |id| {
                    (
                        format!("{} {id:?}", if dark { "dark" } else { "light" }),
                        Tokens::with_accent(dark, id),
                    )
                })
            })
            .collect()
    }

    /// `fg` over a translucent `tint` over `base`.
    fn under(tint: Color, base: Color) -> Color {
        mix(Color { a: 1.0, ..tint }, base, tint.a)
    }

    fn nag(n: u16) -> Nag {
        Nag::new(n).expect("nag")
    }

    #[test]
    fn light_and_dark_palettes_differ_and_follow_the_tokens() {
        let light = NotationPalette::of(&Theme::Light);
        let dark = NotationPalette::of(&Theme::Dark);
        assert_ne!(light, dark);
        assert_eq!(light.main, Tokens::light().text);
        assert_eq!(dark.main, Tokens::dark().text);
        assert_eq!(dark.pill, Tokens::dark().accent);
        assert_eq!(dark.card, Tokens::dark().raised);
    }

    #[test]
    fn nags_take_the_color_of_their_tone() {
        let p = NotationPalette::of(&Theme::Dark);
        assert_eq!(p.nag_color(nag(3)), p.tones.brilliant);
        assert_eq!(p.nag_color(nag(1)), p.tones.good);
        assert_eq!(p.nag_color(nag(5)), p.tones.interesting);
        assert_eq!(p.nag_color(nag(6)), p.tones.dubious);
        assert_eq!(p.nag_color(nag(2)), p.tones.mistake);
        assert_eq!(p.nag_color(nag(4)), p.tones.blunder);
        for n in 10..=21 {
            assert_eq!(p.nag_color(nag(n)), p.tones.eval, "${n}");
        }
        for n in [7, 8, 9, 22, 40, 140, 255] {
            assert_eq!(p.nag_color(nag(n)), p.nag, "${n}");
        }
    }

    #[test]
    fn text_over_every_row_background_reaches_4_5() {
        for (name, t) in both() {
            let p = NotationPalette::from_tokens(&t);
            let soft_panel = under(p.current, t.panel);
            let soft_card = under(p.current, t.raised);
            let backgrounds = [
                ("panel", t.panel),
                ("zebra", p.zebra),
                ("hover", p.hover),
                ("target", t.panel),
            ];
            for (bg_name, bg) in backgrounds {
                for (label, fg) in [
                    ("main", p.main),
                    ("variation", p.variation),
                    ("comment", p.comment),
                    ("muted", p.muted),
                    ("white piece", p.white_piece),
                    ("black piece", p.black_piece),
                    ("neutral nag", p.nag),
                    ("brilliant", p.tones.brilliant),
                    ("good", p.tones.good),
                    ("interesting", p.tones.interesting),
                    ("dubious", p.tones.dubious),
                    ("mistake", p.tones.mistake),
                    ("blunder", p.tones.blunder),
                    ("eval", p.tones.eval),
                ] {
                    let ratio = contrast_ratio(fg, bg);
                    assert!(ratio >= MIN_TEXT, "{name} {label} on {bg_name}: {ratio:.2}");
                }
            }
            // Cards, with and without the pointer, and the soft highlight of
            // the current move inside running text and cards.
            for (bg_name, bg) in [("card", p.card), ("card hover", p.card_hover)] {
                for (label, fg) in [
                    ("variation", p.variation),
                    ("comment", p.comment),
                    ("neutral nag", p.nag),
                    ("brilliant", p.tones.brilliant),
                    ("good", p.tones.good),
                    ("interesting", p.tones.interesting),
                    ("dubious", p.tones.dubious),
                    ("mistake", p.tones.mistake),
                    ("blunder", p.tones.blunder),
                    ("eval", p.tones.eval),
                ] {
                    let ratio = contrast_ratio(fg, bg);
                    assert!(ratio >= MIN_TEXT, "{name} {label} on {bg_name}: {ratio:.2}");
                }
            }
            // The soft highlight sits behind a move only (never behind a
            // comment or a symbol), in the main color or the variation one.
            for (bg_name, bg) in [("soft on panel", soft_panel), ("soft on card", soft_card)] {
                for (label, fg) in [
                    ("main", p.main),
                    ("variation", p.variation),
                    ("comment", p.comment),
                    ("muted", p.muted),
                    ("black piece", p.black_piece),
                ] {
                    let ratio = contrast_ratio(fg, bg);
                    assert!(ratio >= MIN_TEXT, "{name} {label} on {bg_name}: {ratio:.2}");
                }
            }
            // Inside the pill everything uses the text color of the accent.
            let ratio = contrast_ratio(p.on_pill, p.pill);
            assert!(ratio >= MIN_TEXT, "{name} pill: {ratio:.2}");
        }
    }

    #[test]
    fn graphics_reach_3() {
        for (name, t) in both() {
            let p = NotationPalette::from_tokens(&t);
            // The outline of the menu target and the pill against the row.
            for (label, fg, bg) in [
                ("target outline", p.target, t.panel),
                ("target outline on zebra", p.target, p.zebra),
                ("pill", p.pill, t.panel),
                ("pill on zebra", p.pill, p.zebra),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(ratio >= MIN_GRAPHIC, "{name} {label}: {ratio:.2}");
            }
        }
    }
}
