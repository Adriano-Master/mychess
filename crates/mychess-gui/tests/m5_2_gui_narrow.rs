//! M5-2a GUI contract: a narrow panel keeps its rows whole and scrolls
//! sideways instead of squeezing the cells, and nothing depends on physical
//! pixels (spec `docs/specs/m5-2-janela-da-arvore.md`, D968, D983).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod m5_2_gui_common;
#[path = "render_common/mod.rs"]
mod render_common;

use iced::Size;
use m5_2_gui_common::*;
use tempfile::tempdir;

fn common_fingerprint(sim: &mut iced_test::Simulator<'_, Message>) -> String {
    common::fingerprint(sim)
}

fn open(dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    rig
}

#[test]
fn rows_keep_their_width_and_height_when_the_panel_is_narrow() {
    let dir = tempdir().unwrap();
    let rig = open(dir.path());
    let mut wide = pane_sim_at(&rig, Size::new(1000.0, 500.0));
    let w_row = row_bounds(&mut wide, "tree-row-0");
    let w_total = row_bounds(&mut wide, "tree-total");
    let w_text = wide.find("e4").unwrap().bounds();
    drop(wide);
    let mut narrow = pane_sim_at(&rig, Size::new(220.0, 500.0));
    let n_row = row_bounds(&mut narrow, "tree-row-0");
    let n_total = row_bounds(&mut narrow, "tree-total");
    let n_text = narrow.find("e4").unwrap().bounds();
    assert_eq!(n_row.width, w_row.width, "the row is not squeezed");
    assert_eq!(n_row.height, w_row.height, "the text does not wrap");
    assert_eq!(n_total.width, w_total.width);
    assert_eq!(n_text.width, w_text.width);
    assert!(
        n_row.width > 220.0,
        "the table is wider than the panel, so it scrolls: {}",
        n_row.width
    );
}

#[test]
fn headers_do_not_wrap_in_a_narrow_panel_in_either_language() {
    let dir = tempdir().unwrap();
    for language in [
        mychess_app::LanguagePref::EnUs,
        mychess_app::LanguagePref::PtBr,
    ] {
        let mut rig = TreeRig::with_language(language);
        rig.open_pgn(dir.path(), &format!("s{}.pgn", language as u8), STD);
        rig.open_panel();
        rig.app_msg(TreeMessage::SetShort(false));
        let headers: &[&str] = if language == mychess_app::LanguagePref::EnUs {
            &["Move", "Games", "Score", "Draws", "Elo", "ECO"]
        } else {
            &["Lance", "Partidas", "Score", "Empates", "Elo", "ECO"]
        };
        let heights: Vec<(String, f32, f32)> = {
            let mut wide = pane_sim_at(&rig, Size::new(1200.0, 500.0));
            headers
                .iter()
                .map(|h| {
                    let b = wide.find(*h).unwrap().bounds();
                    ((*h).to_owned(), b.width, b.height)
                })
                .collect()
        };
        let mut narrow = pane_sim_at(&rig, Size::new(200.0, 500.0));
        for (h, width, height) in heights {
            let b = narrow.find(h.as_str()).unwrap().bounds();
            assert_eq!((b.width, b.height), (width, height), "{h}");
        }
    }
}

#[test]
fn the_narrow_table_scrolls_sideways_with_the_wheel() {
    let dir = tempdir().unwrap();
    let rig = open(dir.path());
    let mut sim = pane_sim_at(&rig, Size::new(220.0, 500.0));
    let before = common_fingerprint(&mut sim);
    // The toolbar wraps in a narrow panel: aim at the first row, not at a fixed height.
    let row = row_bounds(&mut sim, "tree-row-0");
    let at = iced::Point::new(row.x + 20.0, row.y + row.height / 2.0);
    sim.point_at(at);
    drop(sim.simulate([
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at }),
        iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: -80.0, y: 0.0 },
        }),
    ]));
    let after = common_fingerprint(&mut sim);
    assert_ne!(before, after, "the wheel moved the table sideways");
}

#[test]
fn a_short_panel_scrolls_the_rows_under_a_fixed_header() {
    let dir = tempdir().unwrap();
    let rig = open(dir.path());
    let mut sim = pane_sim_at(&rig, Size::new(900.0, 210.0));
    let before = common_fingerprint(&mut sim);
    // The toolbar wraps with the "Mask" menu in it (two lines at this width),
    // so the rows start where the layout puts them: aim at the first one
    // (spec M5-4c).
    let row = row_bounds(&mut sim, "tree-row-0");
    let at = iced::Point::new(100.0, row.y + row.height / 2.0);
    sim.point_at(at);
    drop(sim.simulate([
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at }),
        iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -60.0 },
        }),
    ]));
    let after = common_fingerprint(&mut sim);
    assert_ne!(before, after, "the rows scrolled");
}

#[test]
fn the_bar_has_the_same_logical_size_whatever_the_panel_width() {
    use mychess_gui::theme::tokens::Tokens;
    use mychess_gui::theme::tree_colors::TreeBarColors;
    let dir = tempdir().unwrap();
    let rig = open(dir.path());
    let colors = TreeBarColors::of(&Tokens::of(&iced::Theme::Light));
    let run_at = |size: Size| -> u32 {
        let mut sim = pane_sim_at(&rig, size);
        let id = format!("tree-row-{}", rig.row("d4").index);
        let area = row_bounds(&mut sim, &id);
        drop(sim);
        let shot = render_common::render(mychess_gui::windows::tree::view(&rig.rig.shell), size);
        let near = |p: [u8; 4]| {
            let [r, g, b, _] = colors.black.into_rgba8();
            (i32::from(p[0]) - i32::from(r)).abs() <= 3
                && (i32::from(p[1]) - i32::from(g)).abs() <= 3
                && (i32::from(p[2]) - i32::from(b)).abs() <= 3
        };
        let scale = render_common::SCALE;
        let mut best = 0;
        for y in (area.y * scale) as u32..((area.y + area.height) * scale) as u32 {
            let mut run = 0;
            for x in (area.x * scale) as u32..((area.x + area.width.min(size.width)) * scale) as u32
            {
                if near(shot.pixel(x, y)) {
                    run += 1;
                    best = best.max(run);
                } else {
                    run = 0;
                }
            }
        }
        best
    };
    let wide = run_at(Size::new(1000.0, 400.0));
    // 60 of the 61 units, 2 device pixels per logical pixel.
    assert!((110..=124).contains(&wide), "{wide}");
    // Scrolled out of a narrow panel the bar is clipped, never rescaled:
    // a panel wide enough to show it (again) draws the same length.
    let medium = run_at(Size::new(700.0, 400.0));
    assert_eq!(wide, medium);
}

#[test]
fn the_panel_code_uses_no_physical_pixel_or_scale_factor() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows/tree.rs");
    let text = std::fs::read_to_string(src).unwrap();
    let cut = text.find("#[cfg(test)]").unwrap_or(text.len());
    let code: String = text[..cut]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for banned in ["scale_factor", "physical", "devicePixel", "dpi", "window::"] {
        assert!(!code.contains(banned), "tree.rs mentions {banned}");
    }
}

#[test]
fn a_tiny_window_with_the_panel_open_draws_and_stays_consistent() {
    let dir = tempdir().unwrap();
    let rig = open(dir.path());
    for size in [
        Size::new(320.0, 240.0),
        Size::new(120.0, 100.0),
        Size::new(1.0, 1.0),
    ] {
        let mut sim = rig.sim_at(size);
        // No panic while laying out; finding things is allowed to fail.
        drop(sim.find("TOTAL"));
    }
    assert!(rig.rig.shell.ui.layout().is_valid());
}
