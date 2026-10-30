//! Colors and symbols of the results of the tablebase (spec M8-8, D2572).
//! The colors come from the tokens of the palette; the symbol and the word
//! always go with the color (a result never depends on hue alone).

use iced::Color;
use mychess_app::tablebase::TbCategory;

use super::tokens::Tokens;

/// The color of `category` in the palette of `t`: green for the wins, the
/// evaluation tone for the draws, the danger tone for the losses, amber for
/// the results the 50-move rule turns into draws, muted for the unknown.
pub fn category_color(t: &Tokens, category: TbCategory) -> Color {
    match category {
        TbCategory::Win | TbCategory::MaybeWin => t.nag.good,
        TbCategory::CursedWin | TbCategory::BlessedLoss => t.nag.dubious,
        TbCategory::Draw => t.nag.eval,
        TbCategory::Loss | TbCategory::MaybeLoss => t.danger,
        _ => t.muted,
    }
}

/// The short symbol of `category`. Plain ASCII: it renders with any font.
pub fn category_symbol(category: TbCategory) -> &'static str {
    match category {
        TbCategory::Win => "+",
        TbCategory::MaybeWin => "+?",
        TbCategory::CursedWin => "(+)",
        TbCategory::Draw => "=",
        TbCategory::BlessedLoss => "(-)",
        TbCategory::MaybeLoss => "-?",
        TbCategory::Loss => "-",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast::{MIN_TEXT, contrast_ratio};

    #[test]
    fn every_category_has_its_own_symbol() {
        let mut symbols: Vec<&str> = TbCategory::ALL
            .iter()
            .map(|c| category_symbol(*c))
            .collect();
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(symbols.len(), TbCategory::ALL.len());
    }

    #[test]
    fn win_draw_and_loss_differ_in_both_palettes() {
        for t in [Tokens::dark(), Tokens::light()] {
            let win = category_color(&t, TbCategory::Win);
            let draw = category_color(&t, TbCategory::Draw);
            let loss = category_color(&t, TbCategory::Loss);
            assert_ne!(win, draw);
            assert_ne!(draw, loss);
            assert_ne!(win, loss);
        }
    }

    #[test]
    fn the_colors_can_be_read_as_text_on_the_panel() {
        for t in [Tokens::dark(), Tokens::light()] {
            for category in TbCategory::ALL {
                let ratio = contrast_ratio(category_color(&t, category), t.panel);
                assert!(ratio >= MIN_TEXT, "{category:?}: {ratio}");
            }
        }
    }
}
