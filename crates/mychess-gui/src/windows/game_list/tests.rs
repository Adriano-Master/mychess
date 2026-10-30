#![allow(clippy::assert_is_empty, clippy::format_push_string)]
use mychess_app::Effect;

use std::path::Path;

use iced::mouse::ScrollDelta;
use iced::{Point, Size, alignment};
use mychess_app::{
    AppState, BasesMessage, ColumnAlign, ColumnSetup, GameListPrefs, Key, KeyInput, ListColumn,
    ListMessage, Message as AppMessage, Modifiers, NamedKey, PickMode,
};
use mychess_core::GameResult;

use super::body::cut;
use super::columns::{elo_text, result_text};
use super::update::{list, select};
use super::*;
use crate::i18n::I18n;
use crate::i18n::Language;
use crate::shell::Shell;

/// A PGN file of `count` games, game `n` played by `White n`.
pub(super) fn pgn_with(dir: &Path, name: &str, count: u32) -> std::path::PathBuf {
    let mut text = String::new();
    for n in 1..=count {
        text.push_str(&format!(
            "[Event \"Event {n}\"]\n[White \"White {n}\"]\n[Black \"Black {n}\"]\n\
                 [Result \"1-0\"]\n[WhiteElo \"2{n:03}\"]\n\n1. e4 e5 2. Nf3 1-0\n\n"
        ));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write the pgn");
    path
}

/// An application with `path` opened in a slot.
pub(super) fn app_with(path: &Path) -> AppState {
    let mut app = AppState::default();
    let mut queue = vec![AppMessage::Bases(BasesMessage::Open(path.to_path_buf()))];
    while let Some(message) = queue.pop() {
        for effect in mychess_app::update(&mut app, message) {
            if matches!(
                effect,
                Effect::OpenBase { .. } | Effect::LoadBaseGame { .. }
            ) {
                queue.push(crate::test_support::run_effect(effect));
            }
        }
    }
    app
}

fn step(app: &mut AppState, ui: &mut GameListUi, message: GameListMessage) {
    let out = ui.update(message, app);
    apply(app, out);
}

fn apply(app: &mut AppState, messages: Vec<AppMessage>) {
    for message in messages {
        drop(mychess_app::update(app, message));
    }
}

#[test]
fn rows_fit_the_height_and_there_is_always_one() {
    assert_eq!(visible_rows(0.0), 1);
    assert_eq!(visible_rows(-5.0), 1);
    assert_eq!(visible_rows(f32::NAN), 1);
    assert_eq!(visible_rows(ROW_HEIGHT * 10.0 + 3.0), 10);
    assert_eq!(visible_rows(1.0e9), MAX_ROWS);
}

#[test]
fn the_last_first_row_leaves_the_list_full() {
    assert_eq!(max_first(100, 10), 91);
    assert_eq!(max_first(10, 10), 1);
    assert_eq!(max_first(3, 10), 1);
    assert_eq!(max_first(0, 10), 1);
    assert_eq!(max_first(u32::MAX, 1), u32::MAX);
}

#[test]
fn the_wheel_moves_three_rows_a_notch_and_pixels_by_rows() {
    let lines = |y| ScrollDelta::Lines { x: 0.0, y };
    assert_eq!(wheel_rows(lines(-1.0)), 3, "wheel down moves forward");
    assert_eq!(wheel_rows(lines(2.0)), -6);
    assert_eq!(wheel_rows(lines(f32::NAN)), 0);
    let pixels = ScrollDelta::Pixels {
        x: 0.0,
        y: -ROW_HEIGHT * 2.0,
    };
    assert_eq!(wheel_rows(pixels), 2);
}

#[test]
fn wide_panels_add_the_elo_columns_and_cells_follow_the_scid() {
    assert_eq!(columns_for_width(500.0).len(), 8);
    assert_eq!(columns_for_width(900.0).len(), 10);
    assert_eq!(Column::Number.chars(), 7);
    assert_eq!(Column::White.chars(), 14);
    assert_eq!(Column::Event.chars(), 10);
    assert_eq!(Column::Number.align(), alignment::Horizontal::Right);
    assert_eq!(Column::White.align(), alignment::Horizontal::Left);
    assert_eq!(result_text(GameResult::Draw), "=-=");
    assert_eq!(result_text(GameResult::Unknown), "*");
    assert_eq!(elo_text(0), "");
    assert_eq!(elo_text(2750), "2750");
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for column in Column::WITH_ELO {
            assert!(i18n.has(column.label_key()), "{column:?} {language:?}");
        }
    }
}

#[test]
fn tab_names_are_cut_with_an_ellipsis() {
    assert_eq!(cut("short.pgn", 18), "short.pgn");
    let cut_name = cut("a-very-long-name-of-a-database.pgn", 18);
    assert_eq!(cut_name.chars().count(), 18);
    assert!(cut_name.ends_with('…'));
}

#[test]
fn keys_of_the_list_are_the_plain_ones() {
    let plain = |key| KeyInput {
        key: Key::Named(key),
        modifiers: Modifiers::NONE,
    };
    assert_eq!(ListKey::of(&plain(NamedKey::PageUp)), Some(ListKey::PageUp));
    assert_eq!(ListKey::of(&plain(NamedKey::Enter)), Some(ListKey::Enter));
    assert_eq!(ListKey::of(&plain(NamedKey::Left)), None);
    let with_command = KeyInput {
        key: Key::Named(NamedKey::Down),
        modifiers: Modifiers::COMMAND,
    };
    assert_eq!(ListKey::of(&with_command), None);
    assert!(!ListKey::PageDown.needs_pointer());
    assert!(ListKey::Home.needs_pointer());
}

#[test]
fn scrolling_stays_inside_the_range_and_reaches_the_last_game() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = app_with(&pgn_with(dir.path(), "a.pgn", 40));
    let slot = app.bases.current();
    let mut ui = GameListUi {
        rows: 10,
        ..GameListUi::default()
    };
    let first = |app: &AppState| app.bases.slot(slot).expect("slot").list.first;
    step(&mut app, &mut ui, GameListMessage::Scroll { slot, rows: 3 });
    assert_eq!(first(&app), 4);
    step(
        &mut app,
        &mut ui,
        GameListMessage::Scroll { slot, rows: -99 },
    );
    assert_eq!(first(&app), 1);
    step(
        &mut app,
        &mut ui,
        GameListMessage::Scroll { slot, rows: 999 },
    );
    assert_eq!(
        first(&app),
        31,
        "40 games, 10 rows: the last row is game 40"
    );
    assert!(
        ui.update(GameListMessage::Scroll { slot, rows: 5 }, &app)
            .is_empty(),
        "already at the end: nothing to send"
    );
    step(
        &mut app,
        &mut ui,
        GameListMessage::ScrollTo { slot, first: 12 },
    );
    assert_eq!(first(&app), 12);
    ui.rows = 30;
    let clamp = ui.clamp(&app).expect("the list is out of range now");
    apply(&mut app, vec![clamp]);
    assert_eq!(first(&app), 11);
    assert!(ui.clamp(&app).is_none());
}

#[test]
fn arrows_select_and_scroll_the_selection_into_view() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = app_with(&pgn_with(dir.path(), "a.pgn", 40));
    let slot = app.bases.current();
    let mut ui = GameListUi {
        rows: 10,
        ..GameListUi::default()
    };
    let state = |app: &AppState| {
        let s = app.bases.slot(slot).expect("slot");
        (s.list.selected, s.list.first)
    };
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::Down));
    assert_eq!(
        state(&app),
        (Some(1), 1),
        "the first press selects the first row"
    );
    for _ in 0..10 {
        step(&mut app, &mut ui, GameListMessage::Key(ListKey::Down));
    }
    assert_eq!(
        state(&app),
        (Some(11), 2),
        "game 11 scrolled in at the bottom"
    );
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::End));
    assert_eq!(state(&app), (Some(40), 31));
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::Home));
    assert_eq!(state(&app), (Some(1), 1));
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::Up));
    assert_eq!(state(&app), (Some(1), 1), "stays on the first game");
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::PageDown));
    assert_eq!(state(&app), (Some(10), 10), "a page is rows - 1");
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::PageUp));
    assert_eq!(state(&app), (Some(1), 1));
}

#[test]
fn enter_and_double_click_load_the_game_and_clicks_select() {
    let dir = tempfile::tempdir().expect("dir");
    let app = app_with(&pgn_with(dir.path(), "a.pgn", 5));
    let slot = app.bases.current();
    let mut ui = GameListUi::default();
    assert!(
        ui.update(GameListMessage::Key(ListKey::Enter), &app)
            .is_empty(),
        "nothing selected, nothing to load"
    );
    assert_eq!(
        ui.update(GameListMessage::Click { slot, number: 3 }, &app),
        [list(ListMessage::Pick {
            slot,
            number: 3,
            mode: PickMode::Only
        })]
    );
    assert_eq!(
        ui.update(GameListMessage::Open { slot, number: 4 }, &app),
        [
            select(slot, Some(4)),
            AppMessage::Bases(BasesMessage::LoadGame { slot, number: 4 })
        ]
    );
    let mut app = app;
    apply(&mut app, vec![select(slot, Some(2))]);
    assert_eq!(
        ui.update(GameListMessage::Key(ListKey::Enter), &app),
        [AppMessage::Bases(BasesMessage::LoadGame {
            slot,
            number: 2
        })]
    );
}

#[test]
fn viewport_and_hover_are_visual_state() {
    let app = AppState::default();
    let mut ui = GameListUi::default();
    assert!(
        ui.update(GameListMessage::Viewport(Size::new(500.0, 250.0)), &app)
            .is_empty()
    );
    assert_eq!((ui.rows, ui.width), (10, 500.0));
    assert!(ui.update(GameListMessage::Hover(true), &app).is_empty());
    assert!(ui.hovered);
    ui.update(
        GameListMessage::Viewport(Size::new(f32::NAN, f32::NAN)),
        &app,
    );
    assert_eq!(ui.rows, 1);
    assert!(ui.width.is_finite());
}

#[test]
fn the_22_columns_map_both_ways_and_have_texts_in_both_languages() {
    for list_column in ListColumn::ALL {
        let column = Column::of(list_column);
        assert_eq!(column.list_column(), list_column);
        assert_eq!(column.chars(), list_column.default_chars());
    }
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for list_column in ListColumn::ALL {
            let key = Column::of(list_column).label_key();
            assert!(i18n.has(key), "{key} {language:?}");
        }
    }
}

#[test]
fn new_cells_print_flags_counts_and_blank_zeroes() {
    let dir = tempfile::tempdir().expect("dir");
    let app = app_with(&pgn_with(dir.path(), "a.pgn", 3));
    let id = app.bases.current();
    let rows = app.bases.list_page(id, 1, 3);
    assert_eq!(Column::Round.cell(&rows[0]), "?", "a PGN without Round");
    assert_eq!(Column::Deleted.cell(&rows[0]), "");
    assert_eq!(Column::Variations.cell(&rows[0]), "", "zero is blank");
    assert_eq!(Column::Start.cell(&rows[0]), "");
    assert_eq!(Column::Country.cell(&rows[0]), "?");
    assert_eq!(Column::Event.cell(&rows[1]), "Event 2");
}

#[test]
fn editing_columns_shows_hides_moves_aligns_and_resets() {
    let base = GameListPrefs::default();
    let order =
        |p: &GameListPrefs| -> Vec<ListColumn> { p.columns.iter().map(|c| c.column).collect() };
    let added = edit_columns(&base, &ColumnEdit::Toggle(Column::Site));
    assert_eq!(order(&added).last(), Some(&ListColumn::Site));
    let removed = edit_columns(&added, &ColumnEdit::Toggle(Column::Site));
    assert_eq!(removed, base);
    let moved = edit_columns(&base, &ColumnEdit::MoveRight(Column::Number));
    assert_eq!(order(&moved)[..2], [ListColumn::White, ListColumn::Number]);
    assert_eq!(
        edit_columns(&base, &ColumnEdit::MoveLeft(Column::Number)),
        base
    );
    let aligned = edit_columns(
        &base,
        &ColumnEdit::Align(Column::White, ColumnAlign::Center),
    );
    assert_eq!(aligned.columns[1].align, ColumnAlign::Center);
    let wider = edit_columns(&base, &ColumnEdit::Wider(Column::White));
    assert_eq!(wider.columns[1].chars, 16);
    let narrow = edit_columns(&base, &ColumnEdit::Narrower(Column::White));
    assert_eq!(narrow.columns[1].chars, 12);
    assert_eq!(edit_columns(&wider, &ColumnEdit::Reset), base);
    // The last column cannot be hidden.
    let one = GameListPrefs {
        columns: vec![ColumnSetup::new(ListColumn::Number)],
    };
    let only = edit_columns(&one, &ColumnEdit::Toggle(Column::Number));
    assert_eq!(only.columns.len(), 1);
}

#[test]
fn the_scid_chords_of_the_list_are_keys_of_the_list() {
    let chord = |key, modifiers| KeyInput { key, modifiers };
    assert_eq!(
        ListKey::of(&chord(Key::Char('a'), Modifiers::COMMAND)),
        Some(ListKey::SelectAll)
    );
    assert_eq!(
        ListKey::of(&chord(Key::Named(NamedKey::Delete), Modifiers::NONE)),
        Some(ListKey::Remove)
    );
    assert_eq!(
        ListKey::of(&chord(Key::Named(NamedKey::Delete), Modifiers::COMMAND)),
        Some(ListKey::ToggleDeleted)
    );
    assert_eq!(
        ListKey::of(&chord(Key::Char('f'), Modifiers::COMMAND)),
        Some(ListKey::FocusSearch)
    );
    assert_eq!(ListKey::of(&chord(Key::Char('a'), Modifiers::NONE)), None);
}

#[test]
fn selection_actions_use_the_view_and_the_header_click_sorts() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = app_with(&pgn_with(dir.path(), "a.pgn", 30));
    let slot = app.bases.current();
    let mut ui = GameListUi {
        rows: 10,
        ..GameListUi::default()
    };
    // Ctrl+A selects the visible rows only.
    step(
        &mut app,
        &mut ui,
        GameListMessage::Act(ListAction::SelectVisible),
    );
    let selected = |app: &AppState| -> Vec<u32> {
        app.bases
            .slot(slot)
            .expect("slot")
            .list
            .selection
            .iter()
            .copied()
            .collect()
    };
    assert_eq!(selected(&app), (1..=10).collect::<Vec<u32>>());
    // Delete removes them from the filter, not from the database.
    step(&mut app, &mut ui, GameListMessage::Key(ListKey::Remove));
    let state = app.bases.slot(slot).expect("slot");
    assert_eq!((state.filtered(), state.total()), (20, 30));
    assert!(state.list.selection.is_empty());
    // A header click asks for a sort (the result comes with the effect).
    let out = ui.update(GameListMessage::Header(Column::White), &app);
    assert_eq!(
        out,
        [list(ListMessage::SortBy {
            slot,
            column: ListColumn::White
        })]
    );
    // Over a sort that is not possible, nothing happens in the app.
    let before = app.bases.slot(slot).expect("slot").sort();
    step(&mut app, &mut ui, GameListMessage::Header(Column::Number));
    assert_eq!(app.bases.slot(slot).expect("slot").sort(), before);
}

#[test]
fn right_clicks_open_context_menus_and_a_choice_closes_them() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = Shell::new(None);
    shell.app = app_with(&pgn_with(dir.path(), "a.pgn", 12));
    shell
        .ui
        .show_pane(crate::windows::layout::PaneKind::GameList);
    let id = shell.app.bases.current();
    assert!(overlay(&shell).is_none());
    for target in [
        MenuTarget::Row(3),
        MenuTarget::Header(Column::White),
        MenuTarget::Columns,
    ] {
        let menu = ListMenu {
            target,
            position: Point::new(100.0, 100.0),
        };
        let out = shell
            .ui
            .games
            .update(GameListMessage::Menu(menu), &shell.app);
        apply(&mut shell.app, out);
        assert!(overlay(&shell).is_some(), "{target:?}");
    }
    // A right click on a game outside the selection selects it.
    let menu = ListMenu {
        target: MenuTarget::Row(5),
        position: Point::new(10.0, 10.0),
    };
    let out = shell
        .ui
        .games
        .update(GameListMessage::Menu(menu), &shell.app);
    apply(&mut shell.app, out);
    assert_eq!(
        shell.app.bases.slot(id).expect("slot").list.selected,
        Some(5)
    );
    let out = shell
        .ui
        .games
        .update(GameListMessage::Act(ListAction::SelectVisible), &shell.app);
    apply(&mut shell.app, out);
    assert!(shell.ui.games.menu.is_none(), "an action closes the menu");
    assert!(overlay(&shell).is_none());
    // The list of columns stays open while columns are toggled.
    shell.ui.games.update(
        GameListMessage::ColumnsButton(Point::new(5.0, 5.0)),
        &shell.app,
    );
    shell.ui.games.update(
        GameListMessage::Columns(ColumnEdit::Toggle(Column::Site)),
        &shell.app,
    );
    assert!(shell.ui.games.menu.is_some());
    shell
        .ui
        .games
        .update(GameListMessage::CloseMenu, &shell.app);
    assert!(overlay(&shell).is_none());
}

#[test]
fn edits_of_the_columns_are_preferences() {
    let app = AppState::default();
    let mut ui = GameListUi::default();
    let out = ui.update(
        GameListMessage::Columns(ColumnEdit::Toggle(Column::Site)),
        &app,
    );
    assert!(matches!(
        &out[..],
        [AppMessage::Prefs(mychess_app::PrefsMessage::Set(
            mychess_app::PrefChange::GameList(p)
        ))] if p.shows(ListColumn::Site)
    ));
}

#[test]
fn the_narrow_panel_drops_only_the_elo_columns_of_the_setup() {
    let setups = GameListPrefs::default().columns;
    assert_eq!(visible_setups(&setups, 900.0).len(), 10);
    let narrow = visible_setups(&setups, 500.0);
    assert_eq!(narrow.len(), 8);
    assert!(narrow.iter().all(|s| s.column != ListColumn::WhiteElo));
}

#[test]
fn a_game_can_start_the_calculation_of_variants_from_its_menu() {
    use super::menus::row_entries;
    use crate::widgets::context_menu::Entry;
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = Shell::new(None);
    shell.app = app_with(&pgn_with(dir.path(), "a.pgn", 6));
    let slot = shell.app.bases.current();
    let label = shell.i18n.tr("games-menu-calvar");
    let item = |shell: &Shell| {
        row_entries(shell, 3).into_iter().find_map(|e| match e {
            Entry::Item { label: l, message } if l == label => Some(message),
            _ => None,
        })
    };
    let enabled = item(&shell).expect("the entry is there");
    assert!(enabled.is_some(), "enabled by default");
    let out = shell
        .ui
        .games
        .update(GameListMessage::Act(ListAction::Calvar(3)), &shell.app);
    assert!(matches!(
        out.as_slice(),
        [AppMessage::Calvar(mychess_app::calvar::CalvarMessage::StartFromGame {
            slot: s,
            number: 3,
            ..
        })] if *s == slot
    ));
    // Help of the engines: off during an online game.
    shell.app.online.phase =
        mychess_app::online::OnlinePhase::Playing(mychess_app::online::PlayingState::Live);
    assert!(item(&shell).expect("still there").is_none(), "disabled");
}
