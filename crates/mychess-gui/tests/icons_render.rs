//! Every embedded Lucide icon rasterizes with the SVG renderer of iced
//! (resvg), draws something, and takes the color of its tone: the
//! `currentColor` stroke follows the theme (spec M2-5, D224).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

mod render_common;

use iced::widget::{Row, container};
use iced::{Element, Length, Size};
use mychess_gui::theme::Tokens;
use mychess_gui::widgets::icon::{Icon, IconTone, icon};
use render_common::{SCALE, Shot, render};

const CELL: u32 = 32;

fn sheet(tone: IconTone) -> Shot {
    let cells = Icon::ALL.map(|which| {
        Element::<()>::from(
            container(icon(which, CELL as f32, tone))
                .width(Length::Fixed(CELL as f32))
                .height(Length::Fixed(CELL as f32)),
        )
    });
    let size = Size::new((Icon::ALL.len() as u32 * CELL) as f32, CELL as f32);
    render::<()>(Row::with_children(cells), size)
}

fn cell_pixels(index: usize) -> impl Iterator<Item = (u32, u32)> {
    let side = CELL * SCALE as u32;
    let x0 = index as u32 * side;
    (0..side).flat_map(move |y| (0..side).map(move |x| (x0 + x, y)))
}

#[test]
fn every_icon_renders_in_the_color_of_its_tone() {
    if !cfg!(target_os = "linux") {
        return;
    }
    // The renderer draws on the light theme of iced, which maps to the
    // light tokens.
    let tokens = Tokens::light();
    let want = tokens.accent;
    let shot = sheet(IconTone::Accent);
    let background = shot.pixel(shot.width - 1, 0);
    let side = CELL * SCALE as u32;
    for (index, which) in Icon::ALL.into_iter().enumerate() {
        let drawn: Vec<[u8; 4]> = cell_pixels(index)
            .map(|(x, y)| shot.pixel(x, y))
            .filter(|p| *p != background)
            .collect();
        let share = drawn.len() as f32 / (side * side) as f32;
        assert!(share > 0.02, "{which:?}: only {:.1}% drawn", share * 100.0);
        assert!(share < 0.9, "{which:?}: fills the cell");
        // The fully covered pixels have exactly the color of the tone.
        let opaque = [
            (want.r * 255.0).round() as u8,
            (want.g * 255.0).round() as u8,
            (want.b * 255.0).round() as u8,
            255,
        ];
        let exact = drawn.iter().filter(|p| **p == opaque).count();
        assert!(exact > 8, "{which:?}: no pixel in the tone color");
        let black = drawn
            .iter()
            .filter(|p| p[0] < 16 && p[1] < 16 && p[2] < 16)
            .count();
        assert_eq!(black, 0, "{which:?}: still drawn in black");
    }
}

#[test]
fn a_disabled_icon_is_lighter_than_an_enabled_one() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let on = sheet(IconTone::Text);
    let off = sheet(IconTone::Disabled);
    let background = on.pixel(on.width - 1, 0);
    let darkest = |shot: &Shot| {
        cell_pixels(0)
            .map(|(x, y)| shot.pixel(x, y))
            .filter(|p| *p != background)
            .map(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]))
            .min()
            .unwrap_or(u32::MAX)
    };
    assert!(darkest(&off) > darkest(&on));
}
