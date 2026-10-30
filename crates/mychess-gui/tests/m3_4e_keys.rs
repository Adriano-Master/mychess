//! Keys, mouse and opening of the game browser (spec M3-4, slice 4e: D595,
//! D599, D600, D602, D603; Scid `::gbrowser::new` 66-147 and `browseGames`
//! 1488-1650), headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::float_cmp
)]

mod m3_4e_common;

use iced::Size;
use iced::mouse::ScrollDelta;
use m3_4e_common::c::{clear_notice, click_box, debug, notice, open_menu, tool_buttons};
use m3_4e_common::*;
use mychess_app::{Action, AppNotice, BasesNotice, ListMessage, Shortcuts};
use mychess_gui::Message;
use mychess_gui::windows::browser::{
    BrowserMessage, CELL_DEFAULT, CELL_MAX, CELL_MIN, CELL_STEP, key_message, wheel_ply,
};
use mychess_gui::windows::game_list::{GameListMessage, ListKey, ListMenu, MenuTarget};
use mychess_gui::windows::layout::PaneKind;
use rstest::rstest;

fn input(key: Key, command: bool) -> KeyInput {
    chord(key, command)
}

fn msg(mode: BrowseMode, focus: usize, key: Key, command: bool) -> Option<String> {
    key_message(mode, focus, &input(key, command)).map(|m| format!("{m:?}"))
}

// ---- the pure key map ----

#[rstest]
#[case(
    Key::Named(NamedKey::Right),
    false,
    "Act(Step { game: Some(0), delta: 1 })"
)]
#[case(
    Key::Named(NamedKey::Left),
    false,
    "Act(Step { game: Some(0), delta: -1 })"
)]
#[case(
    Key::Named(NamedKey::Down),
    false,
    "Act(Step { game: Some(0), delta: 10 })"
)]
#[case(
    Key::Named(NamedKey::Up),
    false,
    "Act(Step { game: Some(0), delta: -10 })"
)]
#[case(Key::Named(NamedKey::Home), false, "Act(GoToStart { game: Some(0) })")]
#[case(Key::Named(NamedKey::End), false, "Act(GoToEnd { game: Some(0) })")]
#[case(Key::Named(NamedKey::Home), true, "Act(GoToGame(First))")]
#[case(Key::Named(NamedKey::End), true, "Act(GoToGame(Last))")]
#[case(Key::Named(NamedKey::Up), true, "Act(GoToGame(Previous))")]
#[case(Key::Named(NamedKey::Down), true, "Act(GoToGame(Next))")]
#[case(Key::Char('f'), false, "Act(Flip { game: None })")]
#[case(Key::Named(NamedKey::Space), false, "Act(ToggleAutoplay)")]
#[case(Key::Named(NamedKey::Escape), false, "Act(Close)")]
#[case(
    Key::Named(NamedKey::Enter),
    false,
    "Load { game: 0, keep_open: Some(false) }"
)]
#[case(
    Key::Named(NamedKey::Enter),
    true,
    "Load { game: 0, keep_open: Some(true) }"
)]
fn single_game_keys(#[case] key: Key, #[case] command: bool, #[case] expected: &str) {
    assert_eq!(
        msg(BrowseMode::Single, 0, key, command).as_deref(),
        Some(expected)
    );
}

#[rstest]
#[case(Key::Named(NamedKey::Delete), true)]
#[case(Key::Char('+'), true)]
#[case(Key::Char('-'), true)]
#[case(Key::Char('x'), false)]
#[case(Key::Char('b'), false)]
#[case(Key::Named(NamedKey::Delete), false)]
#[case(Key::Named(NamedKey::PageUp), false)]
fn single_game_ignores_the_grid_keys(#[case] key: Key, #[case] command: bool) {
    assert_eq!(msg(BrowseMode::Single, 0, key, command), None);
}

#[rstest]
#[case(
    Key::Named(NamedKey::Right),
    true,
    "Act(Step { game: None, delta: 1 })"
)]
#[case(
    Key::Named(NamedKey::Left),
    true,
    "Act(Step { game: None, delta: -1 })"
)]
#[case(Key::Named(NamedKey::Home), true, "Act(GoToStart { game: None })")]
#[case(Key::Named(NamedKey::End), true, "Act(GoToEnd { game: None })")]
#[case(
    Key::Named(NamedKey::Right),
    false,
    "Act(Step { game: Some(2), delta: 1 })"
)]
#[case(
    Key::Named(NamedKey::Left),
    false,
    "Act(Step { game: Some(2), delta: -1 })"
)]
#[case(Key::Named(NamedKey::Home), false, "Act(GoToStart { game: Some(2) })")]
#[case(Key::Named(NamedKey::End), false, "Act(GoToEnd { game: Some(2) })")]
#[case(Key::Char('f'), false, "Act(Flip { game: None })")]
#[case(Key::Named(NamedKey::Escape), false, "Act(Close)")]
#[case(
    Key::Named(NamedKey::Enter),
    false,
    "Load { game: 2, keep_open: Some(false) }"
)]
#[case(
    Key::Named(NamedKey::Enter),
    true,
    "Load { game: 2, keep_open: Some(false) }"
)]
#[case(Key::Named(NamedKey::Delete), true, "Act(ToggleDeleted { game: 2 })")]
#[case(
    Key::Named(NamedKey::Backspace),
    true,
    "Act(ToggleDeleted { game: 2 })"
)]
#[case(Key::Char('+'), true, "Resize(1)")]
#[case(Key::Char('='), true, "Resize(1)")]
#[case(Key::Char('-'), true, "Resize(-1)")]
fn grid_keys(#[case] key: Key, #[case] command: bool, #[case] expected: &str) {
    assert_eq!(
        msg(BrowseMode::Grid, 2, key, command).as_deref(),
        Some(expected)
    );
}

#[rstest]
#[case(Key::Named(NamedKey::Space), false)]
#[case(Key::Named(NamedKey::Delete), false)]
#[case(Key::Char('+'), false)]
#[case(Key::Char('-'), false)]
#[case(Key::Char('x'), false)]
fn the_grid_has_no_autoplay_and_the_destructive_keys_need_the_command_key(
    #[case] key: Key,
    #[case] command: bool,
) {
    assert_eq!(msg(BrowseMode::Grid, 0, key, command), None);
}

#[test]
fn keys_with_alt_are_never_the_browsers() {
    let alt = KeyInput {
        key: Key::Named(NamedKey::Right),
        modifiers: Modifiers {
            command: false,
            shift: false,
            alt: true,
        },
    };
    assert!(key_message(BrowseMode::Single, 0, &alt).is_none());
    assert!(key_message(BrowseMode::Grid, 0, &alt).is_none());
}

#[rstest]
#[case(ScrollDelta::Lines { x: 0.0, y: -1.0 }, 1)]
#[case(ScrollDelta::Lines { x: 0.0, y: 3.0 }, -1)]
#[case(ScrollDelta::Lines { x: 5.0, y: 0.0 }, 0)]
#[case(ScrollDelta::Pixels { x: 0.0, y: -12.0 }, 1)]
#[case(ScrollDelta::Pixels { x: 0.0, y: 0.0 }, 0)]
fn the_wheel_moves_one_ply_down_is_forward(#[case] delta: ScrollDelta, #[case] expected: i32) {
    assert_eq!(wheel_ply(delta), expected);
}

#[test]
fn the_browser_keys_do_not_take_a_key_of_the_central_map_by_accident() {
    // The central map has no two actions on one key (M2) and the plain `b`
    // of the browser is not in it (it would clash with typing a move).
    let map = Shortcuts::defaults();
    let mut seen: Vec<(Key, Modifiers, Action)> = Vec::new();
    for (action, bindings) in map.iter() {
        for b in bindings {
            if let Some((_, _, other)) = seen
                .iter()
                .find(|(k, m, _)| *k == b.key && *m == b.modifiers)
            {
                panic!("{other:?} and {action:?} share {b}");
            }
            seen.push((b.key, b.modifiers, action));
        }
    }
    assert!(
        map.lookup(&input(Key::Char('b'), false)).is_none(),
        "plain B is a list key, not a global shortcut"
    );
    assert_eq!(
        ListKey::of(&input(Key::Char('b'), false)),
        Some(ListKey::Browse)
    );
    assert_eq!(ListKey::of(&input(Key::Char('b'), true)), None);
}

// ---- opening ----

#[test]
fn b_over_the_list_opens_the_browser_of_the_selected_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[3]);
    rig.press(plain(Key::Char('b')));
    assert!(rig.shell.app.browser.is_some());
    assert_eq!(browser(&rig).mode, BrowseMode::Single);
    assert_eq!(browser(&rig).games[0].number, 3);
    assert!(rig.shell.app.modal_open());
}

#[test]
fn b_with_several_selected_opens_the_grid_in_the_order_of_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[7, 2, 5]);
    rig.press(plain(Key::Char('b')));
    assert_eq!(browser(&rig).mode, BrowseMode::Grid);
    let numbers: Vec<u32> = browser(&rig).games.iter().map(|g| g.number).collect();
    assert_eq!(numbers, [2, 5, 7]);
}

#[test]
fn b_without_a_selection_opens_nothing_and_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    rig.press(plain(Key::Char('b')));
    assert!(rig.shell.app.browser.is_none());
}

#[test]
fn b_needs_the_pointer_over_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[3]);
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    rig.press(plain(Key::Char('b')));
    assert!(rig.shell.app.browser.is_none());
}

#[test]
fn b_is_text_when_a_text_field_has_the_focus() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[3]);
    rig.key(plain(Key::Char('b')), true, Some('b'));
    assert!(rig.shell.app.browser.is_none());
    // The search box of the list, even if the widget did not say so.
    rig.shell.ui.games.search_focused = true;
    rig.key(plain(Key::Char('b')), false, Some('b'));
    assert!(rig.shell.app.browser.is_none());
}

#[test]
fn b_with_the_panel_hidden_does_not_open_the_browser() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[3]);
    rig.shell.ui.close_pane(PaneKind::GameList);
    rig.press(plain(Key::Char('b')));
    assert!(rig.shell.app.browser.is_none());
}

#[test]
fn b_is_not_a_chess_move_and_ctrl_b_is_not_the_browser() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[3]);
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    let before = rig.cursor();
    rig.press(plain(Key::Char('b')));
    assert!(rig.shell.app.browser.is_none());
    assert_eq!(rig.cursor(), before);
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    rig.press(command(Key::Char('b')));
    assert!(rig.shell.app.browser.is_none());
}

#[test]
fn b_while_the_browser_is_open_does_not_open_another() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    open_browser(&mut rig, &[1, 2]);
    select(&mut rig, &[4]);
    rig.press(plain(Key::Char('b')));
    assert_eq!(browser(&rig).games.len(), 2);
}

// ---- the toolbar button and the menu item ----

#[test]
fn the_eye_button_is_last_and_sends_browse_with_a_selection_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    let boxes = tool_buttons(&rig);
    assert_eq!(boxes.len(), 10);
    let last = *boxes.last().unwrap();
    assert!(
        debug(&click_box(&rig, last)).is_empty(),
        "disabled without a selection"
    );
    select(&mut rig, &[2, 3]);
    let last = *tool_buttons(&rig).last().unwrap();
    let sent = debug(&click_box(&rig, last));
    assert!(sent.iter().any(|m| m.contains("Act(Browse)")), "{sent:?}");
}

#[test]
fn the_eye_button_is_disabled_with_an_empty_filter_and_during_a_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[2]);
    let last = *tool_buttons(&rig).last().unwrap();
    // During a job (the effect is never answered).
    let slot = current_slot(&rig);
    let effects = mychess_app::update(
        &mut rig.shell.app,
        mychess_app::Message::Bases(BasesMessage::List(ListMessage::SortBy {
            slot,
            column: mychess_app::ListColumn::White,
        })),
    );
    assert_eq!(effects.len(), 1);
    assert!(rig.shell.app.list_job().is_some());
    assert!(
        debug(&click_box(&rig, last)).is_empty(),
        "disabled during a job"
    );
}

#[test]
fn the_eye_button_with_an_empty_filter_is_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 4);
    let slot = current_slot(&rig);
    rig.send(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.shell.app.bases.current_slot().unwrap().filtered(), 0);
    let last = *tool_buttons(&rig).last().unwrap();
    assert!(debug(&click_box(&rig, last)).is_empty());
}

fn menu_at(target: MenuTarget) -> ListMenu {
    ListMenu {
        target,
        position: iced::Point::new(300.0, 200.0),
    }
}

fn click_text(rig: &Rig, label: &str) -> Vec<String> {
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(label)
        .unwrap_or_else(|e| panic!("{label}: {e:?}"));
    debug(
        &sim.into_messages()
            .filter(|m| !format!("{m:?}").contains("Hover("))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn the_row_menu_has_a_browse_item_that_sends_browse_row_and_stops_during_a_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    open_menu(&mut rig, menu_at(MenuTarget::Row(4)));
    let sent = click_text(&rig, "Browse");
    assert!(
        sent.iter().any(|m| m.contains("Act(BrowseRow(4))")),
        "{sent:?}"
    );
    // During a job the item is off.
    let slot = current_slot(&rig);
    mychess_app::update(
        &mut rig.shell.app,
        mychess_app::Message::Bases(BasesMessage::List(ListMessage::SortBy {
            slot,
            column: mychess_app::ListColumn::White,
        })),
    );
    open_menu(&mut rig, menu_at(MenuTarget::Row(4)));
    assert!(click_text(&rig, "Browse").is_empty());
}

#[test]
fn browse_row_on_a_row_outside_the_selection_browses_that_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 8);
    select(&mut rig, &[1, 2]);
    let slot = current_slot(&rig);
    let state = rig.shell.app.bases.slot(slot).unwrap();
    assert_eq!(state.browse_targets(Some(6)), [6]);
    assert_eq!(state.browse_targets(Some(2)), [1, 2]);
    assert_eq!(state.browse_targets(None), [1, 2]);
}

// ---- keys through the shell ----

fn rig_with_browser(count: u32, numbers: &[u32]) -> (Rig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), count);
    open_browser(&mut rig, numbers);
    (rig, dir)
}

#[test]
fn the_arrows_move_the_browser_and_the_end_keys_clamp() {
    let (mut rig, _d) = rig_with_browser(5, &[2]);
    assert_eq!(plies(&rig), [0]);
    rig.press(plain(Key::Named(NamedKey::Right)));
    rig.press(plain(Key::Named(NamedKey::Right)));
    assert_eq!(plies(&rig), [2]);
    rig.press(plain(Key::Named(NamedKey::Down)));
    assert_eq!(plies(&rig), [4], "clamped to the end, four plies");
    rig.press(plain(Key::Named(NamedKey::Right)));
    assert_eq!(plies(&rig), [4]);
    rig.press(plain(Key::Named(NamedKey::Up)));
    assert_eq!(plies(&rig), [0]);
    rig.press(plain(Key::Named(NamedKey::Left)));
    assert_eq!(plies(&rig), [0]);
    rig.press(plain(Key::Named(NamedKey::End)));
    assert_eq!(plies(&rig), [4]);
    rig.press(plain(Key::Named(NamedKey::Home)));
    assert_eq!(plies(&rig), [0]);
}

#[test]
fn the_keys_do_not_reach_the_board_the_list_or_the_slots_under_the_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 5);
    c::open_more(&mut rig, dir.path(), "c.pgn", 3);
    rig.send(BasesMessage::Select(slot(1)));
    rig.send(BasesMessage::LoadGame {
        slot: slot(1),
        number: 2,
    });
    let cursor = rig.cursor();
    let orientation = rig.shell.ui.orientation;
    select(&mut rig, &[1]);
    open_browser(&mut rig, &[3]);
    let selection = rig
        .shell
        .app
        .bases
        .slot(slot(1))
        .unwrap()
        .list
        .selection
        .clone();
    for input in [
        plain(Key::Named(NamedKey::Home)),
        plain(Key::Named(NamedKey::Right)),
        plain(Key::Named(NamedKey::Up)),
        plain(Key::Char('e')),
        plain(Key::Char('4')),
        plain(Key::Char('b')),
        plain(Key::Named(NamedKey::Delete)),
        plain(Key::Named(NamedKey::Backspace)),
        command(Key::Char('2')),
        command(Key::Char('9')),
        command(Key::Char('a')),
        command(Key::Named(NamedKey::Tab)),
    ] {
        rig.press(input);
        assert!(rig.shell.app.browser.is_some() || input == plain(Key::Named(NamedKey::Escape)));
        assert_eq!(rig.cursor(), cursor, "{input:?}");
        assert_eq!(rig.shell.ui.orientation, orientation, "{input:?}");
        assert_eq!(rig.shell.app.bases.current(), slot(1), "{input:?}");
        assert_eq!(
            rig.shell.app.bases.slot(slot(1)).unwrap().list.selection,
            selection,
            "{input:?}"
        );
    }
    assert_eq!(rig.shell.app.bases.slot(slot(1)).unwrap().filtered(), 5);
}

#[test]
fn f_turns_the_browser_and_not_the_main_board() {
    let (mut rig, _d) = rig_with_browser(3, &[1]);
    let orientation = rig.shell.ui.orientation;
    rig.press(plain(Key::Char('f')));
    assert!(browser(&rig).games[0].flipped);
    assert_eq!(rig.shell.ui.orientation, orientation);
    rig.press(plain(Key::Char('f')));
    assert!(!browser(&rig).games[0].flipped);
}

#[test]
fn escape_closes_and_enter_asks_for_the_game_but_keeps_the_browser_until_it_is_in() {
    let (mut rig, _d) = rig_with_browser(4, &[3]);
    rig.press(plain(Key::Named(NamedKey::Escape)));
    assert!(rig.shell.app.browser.is_none());
    // Enter and Ctrl+Enter produce the read of the game (a task); the
    // browser closes only when the game arrives (D603), which a test that
    // drops the tasks never sees.
    for input in [
        plain(Key::Named(NamedKey::Enter)),
        command(Key::Named(NamedKey::Enter)),
    ] {
        open_browser(&mut rig, &[3]);
        assert!(key_units(&mut rig, input, false) > 0, "{input:?}");
        assert!(rig.shell.app.browser.is_some(), "{input:?}");
    }
}

#[test]
fn space_starts_and_stops_the_autoplay_of_the_single_browser() {
    let (mut rig, _d) = rig_with_browser(3, &[1]);
    let units = key_units(&mut rig, plain(Key::Named(NamedKey::Space)), false);
    assert!(units > 0, "a timer is scheduled");
    assert!(browser(&rig).autoplay);
    assert_eq!(plies(&rig), [1]);
    key_units(&mut rig, plain(Key::Named(NamedKey::Space)), false);
    assert!(!browser(&rig).autoplay);
}

#[test]
fn ctrl_arrows_go_to_other_games_of_the_filter_and_stop_at_the_extremes() {
    let (mut rig, _d) = rig_with_browser(4, &[2]);
    let number = |rig: &Rig| browser(rig).games[0].number;
    rig.press(command(Key::Named(NamedKey::Down)));
    assert_eq!(number(&rig), 3);
    rig.press(command(Key::Named(NamedKey::End)));
    assert_eq!(number(&rig), 4);
    rig.press(command(Key::Named(NamedKey::Down)));
    assert_eq!(number(&rig), 4, "no game after the last");
    rig.press(command(Key::Named(NamedKey::Home)));
    assert_eq!(number(&rig), 1);
    rig.press(command(Key::Named(NamedKey::Up)));
    assert_eq!(number(&rig), 1, "no game before the first");
    rig.press(command(Key::Named(NamedKey::Down)));
    assert_eq!(number(&rig), 2);
}

#[test]
fn grid_command_arrows_move_all_and_plain_arrows_the_board_under_the_pointer() {
    let (mut rig, _d) = rig_with_browser(6, &[1, 2, 3, 4]);
    assert_eq!(plies(&rig), [4, 4, 4, 4]);
    rig.press(command(Key::Named(NamedKey::Left)));
    assert_eq!(plies(&rig), [3, 3, 3, 3]);
    rig.press(command(Key::Named(NamedKey::Home)));
    assert_eq!(plies(&rig), [0, 0, 0, 0]);
    rig.msg(Message::Browser(BrowserMessage::Act(BrowseMessage::Focus(
        2,
    ))));
    rig.press(plain(Key::Named(NamedKey::Right)));
    assert_eq!(plies(&rig), [0, 0, 1, 0]);
    rig.press(plain(Key::Named(NamedKey::End)));
    assert_eq!(plies(&rig), [0, 0, 4, 0]);
    rig.press(command(Key::Named(NamedKey::Right)));
    assert_eq!(plies(&rig), [1, 1, 4, 1]);
    rig.press(command(Key::Named(NamedKey::End)));
    assert_eq!(plies(&rig), [4, 4, 4, 4]);
    rig.press(plain(Key::Char('f')));
    assert!(browser(&rig).games.iter().all(|g| g.flipped));
}

#[test]
fn grid_enter_and_ctrl_enter_ask_for_the_board_under_the_pointer() {
    for input in [
        plain(Key::Named(NamedKey::Enter)),
        command(Key::Named(NamedKey::Enter)),
    ] {
        let (mut rig, _d) = rig_with_browser(6, &[2, 4, 6]);
        rig.msg(Message::Browser(BrowserMessage::Act(BrowseMessage::Focus(
            1,
        ))));
        assert!(key_units(&mut rig, input, false) > 0, "{input:?}");
        assert!(rig.shell.app.browser.is_some(), "waits for the game");
    }
}

#[test]
fn ctrl_delete_on_a_pgn_grid_says_read_only_and_marks_nothing() {
    let (mut rig, _d) = rig_with_browser(4, &[1, 2]);
    clear_notice(&mut rig);
    rig.press(command(Key::Named(NamedKey::Delete)));
    assert_eq!(notice(&rig), Some(AppNotice::Bases(BasesNotice::ReadOnly)));
    assert!(browser(&rig).games.iter().all(|g| !g.deleted));
    assert!(rig.shell.app.browser.is_some());
}

#[test]
fn ctrl_plus_and_minus_resize_the_grid_within_the_limits() {
    let (mut rig, _d) = rig_with_browser(6, &[1, 2, 3]);
    assert_eq!(rig.shell.ui.browser.cell, CELL_DEFAULT);
    rig.press(command(Key::Char('+')));
    assert_eq!(rig.shell.ui.browser.cell, CELL_DEFAULT + CELL_STEP);
    rig.press(command(Key::Char('-')));
    rig.press(command(Key::Char('-')));
    assert_eq!(rig.shell.ui.browser.cell, CELL_DEFAULT - CELL_STEP);
    for _ in 0..40 {
        rig.press(command(Key::Char('-')));
    }
    assert_eq!(rig.shell.ui.browser.cell, CELL_MIN);
    for _ in 0..60 {
        rig.press(command(Key::Char('=')));
    }
    assert_eq!(rig.shell.ui.browser.cell, CELL_MAX);
    for steps in [i32::MIN, i32::MAX, 0, 1000, -1000] {
        rig.msg(Message::Browser(BrowserMessage::Resize(steps)));
        let cell = rig.shell.ui.browser.cell;
        assert!((CELL_MIN..=CELL_MAX).contains(&cell), "{steps}: {cell}");
    }
    // The single browser has no grid to resize.
    open_browser(&mut rig, &[1]);
    let before = rig.shell.ui.browser.cell;
    rig.press(command(Key::Char('+')));
    assert_eq!(rig.shell.ui.browser.cell, before);
}

#[test]
fn the_wheel_moves_a_ply_and_with_the_command_key_resizes() {
    let (mut rig, _d) = rig_with_browser(6, &[1, 2, 3]);
    rig.msg(Message::Browser(BrowserMessage::Act(
        BrowseMessage::GoToStart { game: None },
    )));
    rig.msg(Message::Browser(BrowserMessage::Wheel {
        game: 1,
        delta: ScrollDelta::Lines { x: 0.0, y: -1.0 },
    }));
    assert_eq!(plies(&rig), [0, 1, 0]);
    rig.msg(Message::Browser(BrowserMessage::Wheel {
        game: 1,
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
    }));
    assert_eq!(plies(&rig), [0, 0, 0]);
    let before = rig.shell.ui.browser.cell;
    rig.shell.ui.games.command = true;
    rig.msg(Message::Browser(BrowserMessage::Wheel {
        game: 1,
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
    }));
    assert_eq!(plies(&rig), [0, 0, 0], "no ply with Ctrl");
    assert_ne!(rig.shell.ui.browser.cell, before, "resized");
    rig.shell.ui.games.command = false;
}

#[test]
fn clicking_a_move_of_the_text_goes_to_that_ply_and_a_bad_ply_is_clamped() {
    let (mut rig, _d) = rig_with_browser(3, &[1]);
    rig.msg(Message::Browser(BrowserMessage::Act(
        BrowseMessage::GoToPly { game: 0, ply: 3 },
    )));
    assert_eq!(plies(&rig), [3]);
    rig.msg(Message::Browser(BrowserMessage::Act(
        BrowseMessage::GoToPly { game: 0, ply: 999 },
    )));
    assert_eq!(plies(&rig), [4]);
}

#[test]
fn tab_stays_inside_the_box_and_changes_nothing_in_the_application() {
    let (mut rig, _d) = rig_with_browser(3, &[1]);
    let cursor = rig.cursor();
    for shift in [false, true] {
        let tab = KeyInput {
            key: Key::Named(NamedKey::Tab),
            modifiers: Modifiers {
                command: false,
                shift,
                alt: false,
            },
        };
        rig.press(tab);
        assert!(rig.shell.app.browser.is_some());
        assert_eq!(rig.cursor(), cursor);
        assert_eq!(rig.shell.app.bases.current(), slot(1));
    }
}

#[test]
fn the_buttons_of_the_single_browser_send_their_messages() {
    let (rig, _d) = rig_with_browser(4, &[2]);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click("Close").expect("the Close button");
    let sent = debug(&sim.into_messages().collect::<Vec<_>>());
    assert!(sent.iter().any(|m| m.contains("Close")), "{sent:?}");
    for (label, needle) in [
        ("Load game", "Load"),
        ("First game", "GoToGame(First)"),
        ("Previous game", "GoToGame(Previous)"),
        ("Next game", "GoToGame(Next)"),
        ("Last game", "GoToGame(Last)"),
    ] {
        let mut sim = rig.sim(Size::new(1280.0, 800.0));
        sim.click(label)
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
        let sent = debug(&sim.into_messages().collect::<Vec<_>>());
        assert!(sent.iter().any(|m| m.contains(needle)), "{label}: {sent:?}");
    }
}

#[test]
fn canceling_the_discard_question_leaves_the_main_board_as_it_was() {
    // Scid `::gbrowser::LoadGame` 379-390 turns the main board only after
    // `::game::Load` succeeded; D603 says the browser closes only when the
    // game is in. A cancelled load must not leave the board turned.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 4);
    rig.send(BasesMessage::LoadGame {
        slot: slot(1),
        number: 1,
    });
    rig.send(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::GameEnd,
    ));
    let mv = rig.shell.app.session.position().parse_uci("a2a3").unwrap();
    rig.send(mychess_app::GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    });
    assert!(rig.shell.app.is_dirty());
    let orientation = rig.shell.ui.orientation;
    open_browser(&mut rig, &[3]);
    rig.press(plain(Key::Char('f')));
    // The button Load: the shell turns the board and asks the application
    // (whose task is not run here); the application then asks the question.
    rig.msg(Message::Browser(BrowserMessage::Load {
        game: 0,
        keep_open: Some(false),
    }));
    app_send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert!(rig.shell.app.discard_prompt.is_some());
    rig.send(mychess_app::Message::AnswerDiscard(
        mychess_app::DiscardChoice::Cancel,
    ));
    assert!(rig.shell.app.browser.is_some());
    assert_eq!(
        rig.shell.ui.orientation, orientation,
        "the load was cancelled: the main board keeps its orientation"
    );
}
