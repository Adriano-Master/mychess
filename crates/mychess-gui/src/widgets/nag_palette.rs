//! The 16 annotation buttons of the Scid's comment editor (spec M2-4,
//! D179): `! ? !! ?? !? ?! +- -+ +/- -/+ += =+ = ~ N D`.
//!
//! Each button toggles its NAG on the current move; a pressed button means
//! the move has it. The same strip is used in the context menu.

use iced::widget::{button, column, row, text, tooltip};
use iced::{Element, Length};
use mychess_core::Nag;

use crate::i18n::I18n;
use crate::widgets::notation::nag_label;

/// The NAG numbers of the buttons, in the Scid's order
/// (`comment.tcl:99-120`).
pub const PALETTE_NAGS: [u16; 16] = [1, 2, 3, 4, 5, 6, 18, 19, 16, 17, 14, 15, 10, 13, 146, 201];

/// Buttons per row.
const PER_ROW: usize = 8;

/// The NAGs of the palette.
pub fn palette_nags() -> impl Iterator<Item = Nag> {
    PALETTE_NAGS.iter().filter_map(|&n| Nag::new(n).ok())
}

/// The Fluent id of the description of a palette NAG.
pub fn tip_key(nag: Nag) -> String {
    format!("nag-{}", nag.value())
}

/// Height of the palette in logical pixels, for popups that must know it.
pub const HEIGHT: f32 = 2.0 * 30.0 + 4.0;

/// The palette. `active` are the NAGs of the move; with `enabled` off the
/// buttons are shown but cannot be pressed (the cursor is at the start).
pub fn nag_palette<'a, M: Clone + 'a>(
    i18n: &I18n,
    active: &[Nag],
    enabled: bool,
    on_toggle: impl Fn(Nag) -> M,
) -> Element<'a, M> {
    let buttons: Vec<Element<'a, M>> = palette_nags()
        .map(|nag| {
            let on = active.contains(&nag);
            let label = text(nag_label(nag, true).into_owned()).center();
            let press = button(label)
                .width(Length::Fixed(38.0))
                .padding([3, 2])
                .style(if on {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press_maybe(enabled.then(|| on_toggle(nag)));
            tooltip(
                press,
                text(i18n.tr(&tip_key(nag))).size(13),
                tooltip::Position::Bottom,
            )
            .into()
        })
        .collect();
    let mut rows = column![].spacing(4);
    let mut buttons = buttons.into_iter();
    loop {
        let line: Vec<Element<'a, M>> = buttons.by_ref().take(PER_ROW).collect();
        if line.is_empty() {
            break;
        }
        rows = rows.push(row(line).spacing(4));
    }
    rows.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn the_palette_has_the_scid_buttons_in_order() {
        let symbols: Vec<String> = palette_nags()
            .map(|n| nag_label(n, true).into_owned())
            .collect();
        assert_eq!(
            symbols[..14],
            [
                "!", "?", "!!", "??", "!?", "?!", "+-", "-+", "+/-", "-/+", "+=", "=+", "=", "~"
            ]
        );
        assert_eq!(palette_nags().count(), 16);
    }

    #[test]
    fn every_button_has_a_description_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for nag in palette_nags() {
                let key = tip_key(nag);
                assert!(i18n.has(&key), "{key} missing in {language:?}");
            }
        }
    }
}
