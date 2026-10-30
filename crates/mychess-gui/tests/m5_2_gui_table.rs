//! M5-2a GUI contract: the table of the opening tree drawn from a real
//! search (spec `docs/specs/m5-2-janela-da-arvore.md`, D965-D969, behaviour
//! 2): header, rows in order, total, percentages with the separator of the
//! language, short and full display, `[end]`, the next-move highlight.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod m5_2_gui_common;
#[path = "render_common/mod.rs"]
mod render_common;

use m5_2_gui_common::*;
use mychess_app::LanguagePref;
use tempfile::tempdir;

fn rated_base() -> String {
    // The averages need at least ten rated games: ten e4 games, nine d4.
    let mut text = String::new();
    for _ in 0..10 {
        text.push_str(&rated_game("1. e4 e5", "1-0", 2000, 1900, "2001.01.01"));
    }
    for _ in 0..9 {
        text.push_str(&rated_game("1. d4 d5", "1-0", 2200, 2100, "2010.01.01"));
    }
    text
}

fn full_display(rig: &mut TreeRig) {
    rig.app_msg(TreeMessage::SetShort(false));
}

#[test]
fn the_table_has_the_header_rows_in_the_view_order_and_the_total() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    let order = rig.sans();
    assert_eq!(order[0], "e4", "frequency order: 3 games first");
    let mut sim = pane_sim(&rig);
    for header in ["Move", "Games", "Score", "Draws", "TOTAL"] {
        assert!(sim.find(header).is_ok(), "{header}");
    }
    let ys: Vec<f32> = order.iter().map(|san| y_of(&mut sim, san)).collect();
    assert!(
        ys.windows(2).all(|w| w[0] < w[1]),
        "rows follow the order of the view {order:?}: {ys:?}"
    );
    let total_y = y_of(&mut sim, "TOTAL");
    assert!(ys.iter().all(|y| *y < total_y), "the total comes last");
    let header_y = y_of(&mut sim, "Move");
    assert!(header_y < ys[0], "the header comes first");
}

#[test]
fn numbers_come_from_the_search_with_the_decimal_point_of_en_us() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    let e4 = rig.row("e4");
    assert_eq!(
        (e4.games, e4.share, e4.score, e4.draw_percent),
        (3, (60, 0), (66, 6), 33)
    );
    let mut sim = pane_sim(&rig);
    for text in ["60.0%", "66.6%", "33%", "100.0%", "20.0%", "20%"] {
        assert!(sim.find(text).is_ok(), "{text}");
    }
    // Total: 5 games, 60.0 % score, 20 % draws; `Nf3` is 100.0 %.
    assert!(sim.find("5").is_ok());
    assert!(sim.find("66,6%").is_err());
}

#[test]
fn numbers_use_the_decimal_comma_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.open_panel();
    let mut sim = pane_sim(&rig);
    for text in [
        "Lance", "Partidas", "Empates", "TOTAL", "60,0%", "66,6%", "100,0%", "33%",
    ] {
        assert!(sim.find(text).is_ok(), "{text}");
    }
    for text in ["66.6%", "60.0%", "Move", "Draws"] {
        assert!(sim.find(text).is_err(), "{text} must not show");
    }
}

#[test]
fn the_short_display_is_the_default_and_hides_elo_perf_year_and_eco() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "r", &rated_base());
    rig.open_panel();
    assert!(rig.view().short);
    let mut sim = pane_sim(&rig);
    for header in ["Elo", "Perf", "Year", "ECO"] {
        assert!(
            sim.find(header).is_err(),
            "{header} hidden in the short mode"
        );
    }
    let e4 = rig.row("e4");
    let elo = e4.avg_elo.expect("rated games give an average").to_string();
    assert!(pane_sim(&rig).find(elo.as_str()).is_err(), "no Elo column");
}

#[test]
fn the_full_display_adds_the_four_columns_with_the_values_of_the_search() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_si4(dir.path(), "r", &rated_base());
    rig.open_panel();
    let started = rig.app().tree().searches_started();
    full_display(&mut rig);
    assert_eq!(
        rig.app().tree().searches_started(),
        started,
        "the display change does not search again"
    );
    let mut sim = pane_sim(&rig);
    for header in ["Elo", "Perf", "Year", "ECO"] {
        assert!(sim.find(header).is_ok(), "{header}");
    }
    let e4 = rig.row("e4");
    let year = e4.avg_year.expect("dated games give a year").to_string();
    let elo = e4.avg_elo.expect("average rating").to_string();
    assert!(sim.find(year.as_str()).is_ok(), "year {year}");
    assert!(sim.find(elo.as_str()).is_ok(), "elo {elo}");
    if let Some(perf) = e4.perf {
        assert!(sim.find(perf.to_string().as_str()).is_ok());
    }
    if let Some(eco) = &e4.eco {
        assert!(sim.find(eco.as_str()).is_ok(), "eco {eco}");
    }
    // And back.
    drop(sim);
    rig.app_msg(TreeMessage::SetShort(true));
    assert!(pane_sim(&rig).find("ECO").is_err());
}

#[test]
fn the_end_row_is_drawn_and_a_click_on_it_sends_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    rig.play("e2e4");
    assert!(rig.sans().contains(&"[end]".to_owned()));
    let mut sim = pane_sim(&rig);
    sim.click("[end]").expect("the text is there");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        !sent.iter().any(|m| dbg(m).contains("RowClicked")),
        "{sent:?}"
    );
    // The clickable ones do send their index.
    let c5 = rig.row("c5").index;
    let mut sim = pane_sim(&rig);
    sim.click("c5").expect("row");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(sent_exact(&sent, c5), "{sent:?}");
}

fn sent_exact(messages: &[Message], index: usize) -> bool {
    sent(messages, &tree_msg(TreeMessage::RowClicked(index)))
}

#[test]
fn the_total_line_is_not_clickable() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    let mut sim = pane_sim(&rig);
    sim.click("TOTAL").expect("text");
    assert_eq!(sim.into_messages().count(), 0);
}

// ---- the next move ----

/// Left edge pixel (inside the row, left of the text) of the row `id`.
fn edge_pixel(rig: &TreeRig, id: &str) -> [u8; 4] {
    let mut sim = pane_sim(rig);
    let bounds = row_bounds(&mut sim, id);
    drop(sim);
    let shot = render_common::render(mychess_gui::windows::tree::view(&rig.rig.shell), SIZE);
    // Device pixels: scale 2; stay 3 logical px inside the left edge.
    let x = ((bounds.x + 3.0) * render_common::SCALE) as u32;
    let y = ((bounds.y + bounds.height / 2.0) * render_common::SCALE) as u32;
    shot.pixel(x, y)
}

#[test]
fn the_row_of_the_next_move_of_the_game_gets_the_highlight() {
    let dir = tempdir().unwrap();
    // Without a game: nothing is highlighted.
    let mut plain = TreeRig::with_std(dir.path());
    plain.open_panel();
    // With 1.d4 in the game and the cursor at the start: d4 is next.
    let mut next = TreeRig::with_std(dir.path());
    next.play("d2d4");
    next.nav(Nav::Start);
    next.open_panel();
    let d4_index = next.row("d4").index;
    assert!(next.row("d4").is_next_move);
    let ids: Vec<String> = (0..3).map(|i| format!("tree-row-{i}")).collect();
    let mut differing = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        // Same san on both sides: compare the row of the same move.
        let san = next.sans()[i].clone();
        let other = plain.row(&san).index;
        let a = edge_pixel(&next, id);
        let b = edge_pixel(&plain, &format!("tree-row-{other}"));
        if a != b {
            differing.push(san);
        }
    }
    assert_eq!(
        differing,
        ["d4"],
        "only the next move (row {d4_index}) is painted differently"
    );
}

// ---- the tricolour bar, drawn ----

/// The longest horizontal run, in device pixels, of a colour close to
/// `color` inside the rectangle, over all its pixel rows.
fn longest_run(
    shot: &render_common::Shot,
    area: iced::Rectangle,
    color: iced::Color,
    tolerance: i32,
) -> u32 {
    let scale = render_common::SCALE;
    let [r, g, b, _] = color.into_rgba8();
    let near = |p: [u8; 4]| {
        (i32::from(p[0]) - i32::from(r)).abs() <= tolerance
            && (i32::from(p[1]) - i32::from(g)).abs() <= tolerance
            && (i32::from(p[2]) - i32::from(b)).abs() <= tolerance
    };
    let (x0, x1) = (
        (area.x * scale) as u32,
        ((area.x + area.width) * scale) as u32,
    );
    let (y0, y1) = (
        (area.y * scale) as u32,
        ((area.y + area.height) * scale) as u32,
    );
    let mut best = 0;
    for y in y0..y1 {
        let mut run = 0;
        for x in x0..x1 {
            if near(shot.pixel(x, y)) {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
    }
    best
}

#[test]
fn the_bar_of_a_row_is_drawn_white_grey_and_black_in_proportion() {
    use mychess_gui::theme::tokens::Tokens;
    use mychess_gui::theme::tree_colors::TreeBarColors;
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    let colors = TreeBarColors::of(&Tokens::of(&iced::Theme::Light));
    let mut sim = pane_sim(&rig);
    let rows: Vec<(String, iced::Rectangle)> = rig
        .sans()
        .into_iter()
        .enumerate()
        .map(|(i, san)| (san, row_bounds(&mut sim, &format!("tree-row-{i}"))))
        .collect();
    drop(sim);
    let shot = render_common::render(mychess_gui::windows::tree::view(&rig.rig.shell), SIZE);
    let bounds_of = |san: &str| rows.iter().find(|r| r.0 == san).expect("row").1;
    // 0 % for White (d4): the black part covers 60 of the 61 units.
    let d4 = longest_run(&shot, bounds_of("d4"), colors.black, 3);
    assert!(d4 >= 100, "d4 is almost all black: {d4} device px");
    // 100 % (Nf3): no black at all in the bar.
    let nf3 = longest_run(&shot, bounds_of("Nf3"), colors.black, 3);
    assert!(nf3 < 40, "Nf3 has no black part, got a run of {nf3}");
    // e4: 66.6 % score with 33 % draws: white to 31, grey to 51, black the
    // last 10 units (20 device px).
    let black = longest_run(&shot, bounds_of("e4"), colors.black, 3);
    assert!((14..=26).contains(&black), "black part of e4: {black}");
    let grey = longest_run(&shot, bounds_of("e4"), colors.grey, 3);
    assert!((30..=50).contains(&grey), "grey part of e4: {grey}");
}
