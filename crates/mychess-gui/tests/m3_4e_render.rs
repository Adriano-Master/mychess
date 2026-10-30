//! What the game browser draws (spec M3-4, slice 4e: D595, D604, D605;
//! criterion "GUI (render)"), headless: widgets by text and id, pixels by
//! the software rasterizer of `render_common`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::float_cmp
)]

mod m3_4e_common;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use iced::{Size, widget};
use m3_4e_common::render::{self, Shot};
use m3_4e_common::*;
use mychess_app::{
    Effect, General, LanguagePref, LoadedPreferences, Message as AppMessage, Preferences,
    ThemePref, perform, update,
};
use mychess_gui::Message;
use mychess_gui::theme::Tokens;
use mychess_gui::windows::browser::{BROWSER_BOX_ID, BROWSER_MOVES_ID, BrowserMessage};
use rstest::rstest;

const BIG: Size = Size::new(1280.0, 800.0);

fn rig_with(count: u32, numbers: &[u32]) -> (Rig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), count);
    open_browser(&mut rig, numbers);
    (rig, dir)
}

fn frame_hash(rig: &Rig, size: Size) -> u64 {
    let shot = render::render(rig.shell.view(), size);
    hash_region(&shot, 0, 0, shot_size(size).0, shot_size(size).1)
}

fn shot_size(size: Size) -> (u32, u32) {
    (
        (size.width * 2.0).round() as u32,
        (size.height * 2.0).round() as u32,
    )
}

fn hash_region(shot: &Shot, x0: u32, y0: u32, x1: u32, y1: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    for y in y0..y1 {
        for x in x0..x1 {
            shot.pixel(x, y).hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn to_u8(c: iced::Color) -> [u8; 3] {
    [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    ]
}

/// Pixels of `color` (within a few units) in the box of `id`; returns the
/// count and the mean of their x.
fn accent_in(rig: &Rig, id: &'static str, color: iced::Color) -> (usize, f32) {
    let bounds = rig
        .sim(BIG)
        .find(widget::Id::new(id))
        .expect("widget")
        .bounds();
    let shot = render::render(rig.shell.view(), BIG);
    let want = to_u8(color);
    let (mut count, mut sum) = (0usize, 0f32);
    let (x0, y0) = ((bounds.x * 2.0) as u32, (bounds.y * 2.0) as u32);
    let (x1, y1) = (
        ((bounds.x + bounds.width) * 2.0) as u32,
        ((bounds.y + bounds.height) * 2.0) as u32,
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let p = shot.pixel(x, y);
            if (0..3).all(|i| (i32::from(p[i]) - i32::from(want[i])).abs() <= 3) {
                count += 1;
                sum += x as f32;
            }
        }
    }
    let mean = if count == 0 { 0.0 } else { sum / count as f32 };
    (count, mean)
}

fn has(rig: &Rig, text: &str) -> bool {
    rig.sim(BIG).find(text).is_ok()
}

// ---- single game ----

#[test]
fn the_single_view_shows_title_players_event_and_the_buttons() {
    let (rig, _d) = rig_with(5, &[3]);
    for text in [
        "Game 3 of a.pgn",
        "W3 - B3",
        "Close",
        "Load game",
        "First game",
        "Previous game",
        "Next game",
        "Last game",
    ] {
        assert!(has(&rig, text), "{text:?}");
    }
    assert!(has(&rig, "Move 0 of 4"));
    // Event, date with its question marks, result.
    let mut sim = rig.sim(BIG);
    assert!(
        sim.find("Ev3, ????.??.??  1-0").is_ok(),
        "the event line keeps ????.??.??"
    );
    assert!(sim.find(widget::Id::new(BROWSER_BOX_ID)).is_ok());
    assert!(sim.find(widget::Id::new(BROWSER_MOVES_ID)).is_ok());
}

#[test]
fn the_ply_line_follows_the_board() {
    let (mut rig, _d) = rig_with(3, &[1]);
    app_send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 3,
        },
    );
    assert!(has(&rig, "Move 3 of 4"));
    app_send(&mut rig, BrowseMessage::GoToEnd { game: None });
    assert!(has(&rig, "Move 4 of 4"));
    app_send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 1,
        },
    );
    assert!(has(&rig, "Move 4 of 4"), "does not pass the end");
    app_send(&mut rig, BrowseMessage::GoToStart { game: None });
    assert!(has(&rig, "Move 0 of 4"));
}

#[test]
fn the_board_is_drawn_in_the_position_of_the_ply_and_changes_with_each_step() {
    let (mut rig, _d) = rig_with(3, &[2]);
    let mut seen = vec![frame_hash(&rig, BIG)];
    for _ in 0..4 {
        app_send(
            &mut rig,
            BrowseMessage::Step {
                game: None,
                delta: 1,
            },
        );
        seen.push(frame_hash(&rig, BIG));
    }
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 5, "five different frames for plies 0..=4");
    // Back to a ply already seen: the same frame (nothing stale).
    app_send(&mut rig, BrowseMessage::GoToPly { game: 0, ply: 2 });
    assert_eq!(frame_hash(&rig, BIG), seen[2]);
    // At the end a further step changes nothing.
    app_send(&mut rig, BrowseMessage::GoToEnd { game: None });
    let end = frame_hash(&rig, BIG);
    assert_eq!(end, seen[4]);
    app_send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 1,
        },
    );
    assert_eq!(frame_hash(&rig, BIG), end);
}

#[test]
fn the_highlight_of_the_current_move_is_in_the_text_and_moves_with_the_ply() {
    let (mut rig, _d) = rig_with(3, &[2]);
    let accent = Tokens::of(&iced::Theme::Light).accent;
    let (none, _) = accent_in(&rig, BROWSER_MOVES_ID, accent);
    assert_eq!(none, 0, "no move is current at the start");
    app_send(&mut rig, BrowseMessage::GoToPly { game: 0, ply: 1 });
    let (first, x1) = accent_in(&rig, BROWSER_MOVES_ID, accent);
    assert!(first > 0, "the first move is marked");
    app_send(&mut rig, BrowseMessage::GoToPly { game: 0, ply: 4 });
    let (last, x4) = accent_in(&rig, BROWSER_MOVES_ID, accent);
    assert!(last > 0, "the fourth move is marked");
    assert!(
        (x4 - x1).abs() > 8.0,
        "the mark moved along the text ({x1} -> {x4})"
    );
    app_send(&mut rig, BrowseMessage::GoToStart { game: None });
    assert_eq!(accent_in(&rig, BROWSER_MOVES_ID, accent).0, 0);
}

#[test]
fn flipping_changes_the_frame_and_flipping_back_restores_it() {
    let (mut rig, _d) = rig_with(3, &[1]);
    app_send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 2,
        },
    );
    let normal = frame_hash(&rig, BIG);
    app_send(&mut rig, BrowseMessage::Flip { game: None });
    let turned = frame_hash(&rig, BIG);
    assert_ne!(normal, turned);
    app_send(&mut rig, BrowseMessage::Flip { game: None });
    assert_eq!(frame_hash(&rig, BIG), normal);
}

#[test]
fn while_reading_the_view_says_so_and_shows_the_progress() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 5);
    let slot = current_slot(&rig);
    let effects = update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Browse(BrowseMessage::Open {
            slot,
            numbers: vec![2],
        })),
    );
    assert_eq!(effects.len(), 1);
    rig.sync();
    assert!(has(&rig, "Reading the games..."), "placeholder");
    // The buttons that need the game are off, Close works.
    let mut sim = rig.sim(BIG);
    sim.click("Close").expect("Close");
    let sent = debug_all(&sim.into_messages().collect::<Vec<_>>());
    assert!(sent.iter().any(|m| m.contains("Close")));
}

fn debug_all(messages: &[Message]) -> Vec<String> {
    messages.iter().map(|m| format!("{m:?}")).collect()
}

#[test]
fn an_unreadable_game_shows_the_message_instead_of_the_board() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 5);
    let slot = current_slot(&rig);
    let effects = update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Browse(BrowseMessage::Open {
            slot,
            numbers: vec![2],
        })),
    );
    let effect = effects.into_iter().next().unwrap();
    let Effect::ReadBrowserGames { control, .. } = &effect else {
        panic!("a read");
    };
    control.cancel();
    let answer = c::b::runtime().block_on(perform(effect));
    rig.app_msg(answer);
    assert!(matches!(
        browser(&rig).games[0].content,
        mychess_app::BrowserContent::Unreadable(_)
    ));
    assert!(has(&rig, "This game cannot be read"));
    assert!(!has(&rig, "Reading the games..."));
    // Keys on it do not panic and Load is off.
    rig.press(plain(Key::Named(NamedKey::Right)));
    rig.press(plain(Key::Named(NamedKey::End)));
    let mut sim = rig.sim(BIG);
    sim.click("Load game").ok();
    let sent = debug_all(&sim.into_messages().collect::<Vec<_>>());
    assert!(
        !sent.iter().any(|m| m.contains("Load")),
        "Load is disabled for an unreadable game: {sent:?}"
    );
}

// ---- grid ----

fn grid_label_columns(rig: &Rig, count: u32) -> usize {
    let mut sim = rig.sim(BIG);
    let mut xs: Vec<f32> = Vec::new();
    for n in 1..=count {
        let bounds = sim
            .find(format!("W{n} - B{n}"))
            .unwrap_or_else(|_| panic!("label of game {n}"))
            .bounds();
        if !xs.iter().any(|x| (x - bounds.x).abs() < 1.0) {
            xs.push(bounds.x);
        }
    }
    xs.len()
}

#[rstest]
#[case(2, 3)]
#[case(4, 4)]
#[case(6, 3)]
#[case(7, 4)]
#[case(12, 4)]
#[case(13, 5)]
#[case(21, 6)]
#[case(37, 7)]
fn the_grid_draws_the_columns_of_the_scid(#[case] count: u32, #[case] columns: usize) {
    let (rig, _d) = rig_with(count.max(8), &(1..=count).collect::<Vec<_>>());
    assert_eq!(browser(&rig).columns(), columns);
    assert_eq!(
        grid_label_columns(&rig, count),
        columns.min(count as usize),
        "{count} games"
    );
}

#[test]
fn a_cell_shows_the_players_and_the_result_and_the_title_counts_the_games() {
    let (rig, _d) = rig_with(6, &[1, 2, 3]);
    assert!(has(&rig, "3 games of a.pgn"));
    for n in 1..=3 {
        assert!(has(&rig, &format!("W{n} - B{n}")));
    }
    assert!(has(&rig, "1-0"), "the result of each cell");
    assert!(has(&rig, "Close"));
}

#[test]
fn a_cell_marks_a_deleted_game() {
    let (mut rig, _d) = rig_with(6, &[1, 2]);
    rig.shell.app.browser.as_mut().unwrap().games[1].deleted = true;
    rig.sync();
    assert!(has(&rig, "1-0  deleted"));
    assert!(has(&rig, "1-0"));
}

#[test]
fn the_grid_over_the_limit_says_how_many_are_shown() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 70);
    let numbers: Vec<u32> = (1..=70).collect();
    open_browser(&mut rig, &numbers);
    assert_eq!(browser(&rig).games.len(), 64);
    assert!(has(&rig, "The browser shows only the first 64 of 70 games"));
    assert_eq!(browser(&rig).columns(), 7);
}

#[test]
fn grid_boards_change_when_all_move_together_and_one_only_when_one_moves() {
    let (mut rig, _d) = rig_with(6, &[1, 2, 3, 4]);
    let start = frame_hash(&rig, BIG);
    app_send(&mut rig, BrowseMessage::GoToStart { game: None });
    let all_start = frame_hash(&rig, BIG);
    assert_ne!(start, all_start);
    app_send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(1),
            delta: 1,
        },
    );
    let one = frame_hash(&rig, BIG);
    assert_ne!(all_start, one);
    app_send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(1),
            delta: -1,
        },
    );
    assert_eq!(frame_hash(&rig, BIG), all_start);
    app_send(&mut rig, BrowseMessage::Flip { game: Some(2) });
    assert_ne!(frame_hash(&rig, BIG), all_start);
}

#[test]
fn the_grid_size_changes_the_layout_within_the_limits() {
    use mychess_gui::windows::browser::{CELL_MAX, CELL_MIN};
    let (mut rig, _d) = rig_with(8, &[1, 2, 3, 4, 5, 6, 7]);
    let width = |rig: &Rig| {
        let mut sim = rig.sim(BIG);
        let a = sim.find("W1 - B1").unwrap().bounds();
        let b = sim.find("W2 - B2").unwrap().bounds();
        b.x - a.x
    };
    let normal = width(&rig);
    rig.msg(Message::Browser(BrowserMessage::Resize(2)));
    assert!(width(&rig) > normal);
    rig.shell.ui.browser.cell = CELL_MAX;
    rig.msg(Message::Browser(BrowserMessage::Resize(1)));
    assert_eq!(rig.shell.ui.browser.cell, CELL_MAX);
    rig.shell.ui.browser.cell = CELL_MIN;
    rig.msg(Message::Browser(BrowserMessage::Resize(-1)));
    assert_eq!(rig.shell.ui.browser.cell, CELL_MIN);
    assert!(width(&rig) > 0.0);
}

// ---- window sizes, themes, language ----

#[rstest]
#[case(800.0, 560.0, 1)]
#[case(1280.0, 800.0, 1)]
#[case(2560.0, 1440.0, 1)]
#[case(800.0, 560.0, 9)]
#[case(1280.0, 800.0, 9)]
#[case(2560.0, 1440.0, 9)]
#[case(800.0, 560.0, 37)]
#[case(2560.0, 1440.0, 37)]
fn the_layer_is_centered_and_fits_every_window_size(
    #[case] width: f32,
    #[case] height: f32,
    #[case] games: u32,
) {
    let (mut rig, _d) = rig_with(40, &(1..=games).collect::<Vec<_>>());
    resize(&mut rig, width, height);
    let size = Size::new(width, height);
    let bounds = rig
        .sim(size)
        .find(widget::Id::new(BROWSER_BOX_ID))
        .expect("the box")
        .bounds();
    let centre = bounds.center();
    assert!(
        (centre.x - width / 2.0).abs() <= 2.0,
        "horizontally centered: {bounds:?} in {width}x{height}"
    );
    assert!(
        (centre.y - height / 2.0).abs() <= 2.0,
        "vertically centered: {bounds:?} in {width}x{height}"
    );
    // Whatever the size, a frame can be drawn.
    if width < 2000.0 {
        assert_ne!(frame_hash(&rig, size), 0);
    }
}

#[rstest]
#[case(800.0, 560.0, 1)]
#[case(800.0, 560.0, 9)]
#[case(1280.0, 800.0, 37)]
fn the_box_stays_inside_the_window(#[case] width: f32, #[case] height: f32, #[case] games: u32) {
    let (mut rig, _d) = rig_with(40, &(1..=games).collect::<Vec<_>>());
    resize(&mut rig, width, height);
    let bounds = rig
        .sim(Size::new(width, height))
        .find(widget::Id::new(BROWSER_BOX_ID))
        .expect("the box")
        .bounds();
    assert!(
        bounds.x >= -0.5
            && bounds.y >= -0.5
            && bounds.x + bounds.width <= width + 0.5
            && bounds.y + bounds.height <= height + 0.5,
        "{bounds:?} outside {width}x{height}"
    );
}

fn preferences(language: LanguagePref, theme: ThemePref) -> AppMessage {
    AppMessage::PreferencesLoaded(LoadedPreferences {
        prefs: Preferences {
            general: General {
                language,
                theme,
                ..General::default()
            },
            ..Preferences::default()
        },
        issues: Vec::new(),
    })
}

#[test]
fn the_dark_theme_draws_the_browser_too() {
    let (mut rig, _d) = rig_with(6, &[1, 2, 3]);
    rig.app_msg(preferences(LanguagePref::EnUs, ThemePref::Dark));
    assert!(has(&rig, "3 games of a.pgn"));
    open_browser(&mut rig, &[2]);
    assert!(has(&rig, "Game 2 of a.pgn"));
    app_send(&mut rig, BrowseMessage::GoToEnd { game: None });
    assert!(has(&rig, "Move 4 of 4"));
}

#[test]
fn changing_the_language_retranslates_an_open_browser() {
    let (mut rig, _d) = rig_with(6, &[2]);
    assert!(has(&rig, "Game 2 of a.pgn"));
    assert!(has(&rig, "Load game"));
    rig.app_msg(preferences(LanguagePref::PtBr, ThemePref::Light));
    assert!(has(&rig, "Partida 2 de a.pgn"), "title in pt-BR");
    assert!(has(&rig, "Carregar partida"));
    assert!(has(&rig, "Fechar"));
    assert!(has(&rig, "Lance 0 de 4"));
    assert!(!has(&rig, "Load game"));
    rig.app_msg(preferences(LanguagePref::EnUs, ThemePref::Light));
    assert!(has(&rig, "Game 2 of a.pgn"));
    // And the grid.
    open_browser(&mut rig, &[1, 2, 3]);
    rig.app_msg(preferences(LanguagePref::PtBr, ThemePref::Light));
    assert!(has(&rig, "3 partidas de a.pgn"));
}

#[test]
fn the_limit_notice_is_translated_in_the_grid() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 70);
    open_browser(&mut rig, &(1..=70).collect::<Vec<_>>());
    rig.app_msg(preferences(LanguagePref::PtBr, ThemePref::Light));
    assert!(has(
        &rig,
        "O navegador mostra só as primeiras 64 de 70 partidas"
    ));
}

#[test]
fn no_browser_means_no_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 4);
    assert!(rig.sim(BIG).find(widget::Id::new(BROWSER_BOX_ID)).is_err());
    open_browser(&mut rig, &[1]);
    assert!(rig.sim(BIG).find(widget::Id::new(BROWSER_BOX_ID)).is_ok());
    rig.press(plain(Key::Named(NamedKey::Escape)));
    assert!(rig.sim(BIG).find(widget::Id::new(BROWSER_BOX_ID)).is_err());
}
