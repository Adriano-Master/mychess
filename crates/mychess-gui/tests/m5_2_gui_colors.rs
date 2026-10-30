//! M5-2a GUI contract: the colours of the tricolour bar (spec
//! `docs/specs/m5-2-janela-da-arvore.md`, D969, D983): white, grey and black
//! by palette, distinguishable in the light and in the dark theme.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

use iced::Color;
use mychess_gui::theme::contrast::{contrast_ratio, relative_luminance};
use mychess_gui::theme::tokens::Tokens;
use mychess_gui::theme::tree_colors::TreeBarColors;

fn palettes() -> [(&'static str, Tokens); 2] {
    [("light", Tokens::light()), ("dark", Tokens::dark())]
}

#[test]
fn white_grey_and_black_are_ordered_by_luminance_in_both_palettes() {
    for (name, tokens) in palettes() {
        let c = TreeBarColors::of(&tokens);
        let (w, g, b) = (
            relative_luminance(c.white),
            relative_luminance(c.grey),
            relative_luminance(c.black),
        );
        assert!(w > g && g > b, "{name}: {w} {g} {b}");
    }
}

#[test]
fn the_three_parts_are_distinguishable_from_each_other() {
    for (name, tokens) in palettes() {
        let c = TreeBarColors::of(&tokens);
        assert!(
            contrast_ratio(c.white, c.black) >= 7.0,
            "{name}: white x black"
        );
        assert!(
            contrast_ratio(c.white, c.grey) >= 1.5,
            "{name}: white x grey = {}",
            contrast_ratio(c.white, c.grey)
        );
        assert!(
            contrast_ratio(c.grey, c.black) >= 1.5,
            "{name}: grey x black = {}",
            contrast_ratio(c.grey, c.black)
        );
    }
}

#[test]
fn the_bar_stands_out_of_the_panel_it_is_drawn_on() {
    for (name, tokens) in palettes() {
        let c = TreeBarColors::of(&tokens);
        // The white part may equal a light panel: the outline keeps the edge.
        assert!(
            contrast_ratio(c.outline, tokens.panel) > 1.05,
            "{name}: the outline is invisible"
        );
        assert!(
            contrast_ratio(c.black, tokens.panel) >= 1.5
                || contrast_ratio(c.white, tokens.panel) >= 1.5,
            "{name}"
        );
        assert!(
            contrast_ratio(c.grey, tokens.panel) > 1.05,
            "{name}: grey vanishes into the panel"
        );
    }
}

#[test]
fn the_parts_are_opaque_greys_not_theme_accents() {
    for (name, tokens) in palettes() {
        let c = TreeBarColors::of(&tokens);
        for part in [c.white, c.black] {
            assert_eq!(part.a, 1.0, "{name}");
            let spread = part.r.max(part.g).max(part.b) - part.r.min(part.g).min(part.b);
            assert!(spread < 0.02, "{name}: {part:?} is tinted");
        }
    }
}

#[test]
fn the_colours_depend_only_on_the_palette() {
    let a = TreeBarColors::of(&Tokens::of(&iced::Theme::Light));
    let b = TreeBarColors::of(&Tokens::light());
    assert_eq!(a, b);
    let dark = TreeBarColors::of(&Tokens::of(&iced::Theme::Dark));
    assert_eq!(dark, TreeBarColors::of(&Tokens::dark()));
    assert_ne!(a.white, dark.white, "the two palettes are not the same");
    let _: Color = a.grey;
}
