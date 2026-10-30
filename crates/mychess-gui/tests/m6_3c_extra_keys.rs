//! M6-3c extra (GUI): keys with each of the five windows open leak nothing to
//! the game or the board, stacked windows unwind one by one, a scan that
//! runs is cancelled by Esc, and a quit with windows and jobs in flight is
//! quiet (spec M6-3, criteria 40 to 44).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_3c_common;

use m6_3c_common::*;
use mychess_app::{Action, FinderMessage, NameKind, PlayerInfoMessage, SpellingMessage};
use mychess_gui::windows::dialog_ui::ActiveDialog;

fn load_first_game(t: &mut Tools) {
    let slot = t.app().bases.current();
    t.rig.app_msg(BasesMessage::LoadGame { slot, number: 1 });
    t.rig.app_msg(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::Forward(2),
    ));
}

/// What a key must not change while a window is open.
fn snapshot(t: &Tools) -> (String, bool, bool, usize) {
    let app = t.app();
    (
        format!("{:?}", app.session.cursor()),
        app.board_flipped,
        app.session.is_dirty(),
        app.session.game().node_count(),
    )
}

fn open(t: &mut Tools, window: ActiveDialog) {
    match window {
        ActiveDialog::PlayerFinder => t.action(Action::PlayerFinder),
        ActiveDialog::TournamentFinder => t.action(Action::TournamentFinder),
        ActiveDialog::PlayerInfo => t.rig.app_msg(PlayerInfoMessage::Open {
            name: "Alfa, A".into(),
        }),
        ActiveDialog::SpellCheck => t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player)),
        ActiveDialog::AddElo => t.rig.app_msg(SpellingMessage::OpenAddElo),
        other => panic!("{other:?} is not one of the windows"),
    }
    assert_eq!(t.top(), Some(window));
}

const WINDOWS: [ActiveDialog; 5] = [
    ActiveDialog::PlayerFinder,
    ActiveDialog::TournamentFinder,
    ActiveDialog::PlayerInfo,
    ActiveDialog::SpellCheck,
    ActiveDialog::AddElo,
];

fn key(named: NamedKey) -> KeyInput {
    KeyInput {
        key: Key::Named(named),
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn arrows_tab_space_and_letters_do_not_reach_the_game_or_the_board_under_any_window() {
    let keys = [
        key(NamedKey::Left),
        key(NamedKey::Right),
        key(NamedKey::Up),
        key(NamedKey::Down),
        key(NamedKey::Home),
        key(NamedKey::End),
        key(NamedKey::PageUp),
        key(NamedKey::PageDown),
        key(NamedKey::Tab),
        key(NamedKey::Space),
        key(NamedKey::Delete),
        key(NamedKey::Backspace),
        KeyInput {
            key: Key::Char('f'),
            modifiers: Modifiers::NONE,
        },
        KeyInput {
            key: Key::Char('n'),
            modifiers: Modifiers::NONE,
        },
        KeyInput {
            key: Key::Char('e'),
            modifiers: Modifiers::NONE,
        },
    ];
    for window in WINDOWS {
        let mut t = Tools::new();
        t.open_base(&misspelled());
        t.load_ssp();
        load_first_game(&mut t);
        let before = snapshot(&t);
        open(&mut t, window);
        for k in keys {
            // Not `TreeRig::key`: Tab returns a widget-operation task that
            // never ends without a window, and draining it would block.
            t.rig.rig.press(k);
            assert_eq!(snapshot(&t), before, "{window:?} leaked {k:?}");
            assert_eq!(t.top(), Some(window), "{window:?} closed by {k:?}");
        }
        // Closing hands the keys back to the game.
        t.named(NamedKey::Escape);
        t.named(NamedKey::Escape);
        t.rig.key(key(NamedKey::Left));
        assert_ne!(snapshot(&t).0, before.0, "the game answers again");
    }
}

#[test]
fn enter_with_a_window_open_never_edits_the_game() {
    for window in WINDOWS {
        let mut t = Tools::new();
        t.open_base(&misspelled());
        t.load_ssp();
        load_first_game(&mut t);
        let before = snapshot(&t);
        open(&mut t, window);
        t.named(NamedKey::Enter);
        assert_eq!(snapshot(&t), before, "{window:?}");
    }
}

#[test]
fn stacked_windows_unwind_one_escape_at_a_time() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.load_ssp();
    t.action(Action::PlayerFinder);
    t.action(Action::TournamentFinder);
    assert_eq!(t.top(), Some(ActiveDialog::TournamentFinder));
    t.rig.app_msg(PlayerInfoMessage::Open {
        name: "Alfa, A".into(),
    });
    assert_eq!(t.top(), Some(ActiveDialog::PlayerInfo));
    t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
    assert_eq!(t.top(), Some(ActiveDialog::SpellCheck));
    let mut seen = Vec::new();
    for _ in 0..4 {
        t.named(NamedKey::Escape);
        seen.push(t.top());
    }
    assert_eq!(
        seen,
        [
            Some(ActiveDialog::PlayerInfo),
            Some(ActiveDialog::TournamentFinder),
            Some(ActiveDialog::PlayerFinder),
            None
        ]
    );
    assert!(!t.shell().modal_layer_open());
    // One more Esc with nothing open is harmless.
    t.named(NamedKey::Escape);
    assert_eq!(t.top(), None);
}

#[test]
fn the_keys_of_the_top_window_do_not_move_the_cursor_of_the_one_below() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    t.named(NamedKey::Down);
    let below = t.shell().ui.dialogs.player_tools.players.cursor;
    assert_eq!(below, Some(0));
    t.action(Action::TournamentFinder);
    t.named(NamedKey::Down);
    t.named(NamedKey::Down);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, below);
}

#[test]
fn arrows_with_an_empty_list_keep_the_cursor_empty() {
    let mut t = Tools::new();
    // No base filters anything: the defaults of the tournament finder find
    // nothing, and the player finder of the empty clipbase lists nobody.
    t.action(Action::TournamentFinder);
    for k in [
        NamedKey::Down,
        NamedKey::End,
        NamedKey::PageDown,
        NamedKey::Up,
        NamedKey::Home,
        NamedKey::PageUp,
    ] {
        t.named(k);
    }
    t.named(NamedKey::Enter);
    assert_eq!(t.top(), Some(ActiveDialog::TournamentFinder));
    t.named(NamedKey::Escape);
    t.action(Action::PlayerFinder);
    for k in [NamedKey::Down, NamedKey::End, NamedKey::Up, NamedKey::Home] {
        t.named(k);
    }
    let cursor = t.shell().ui.dialogs.player_tools.players.cursor;
    let rows = t.app().player_finder_view().expect("open").rows.len();
    assert!(
        cursor.is_none_or(|c| c < rows.max(1)),
        "{cursor:?} / {rows}"
    );
    t.named(NamedKey::Enter);
}

#[test]
fn escape_stops_a_running_scan_before_it_closes_the_checker() {
    let mut t = Tools::new();
    t.open_base(&misspelled());
    t.load_ssp();
    // The shell turns the open message into a task that is NOT run, so the
    // scan stays "in flight" in the application.
    drop(t.rig.rig.shell.update(Message::App(
        SpellingMessage::OpenCheck(NameKind::Player).into(),
    )));
    assert_eq!(t.top(), Some(ActiveDialog::SpellCheck));
    let scanning = t.app().spell_view().expect("open").scanning.is_some();
    t.named(NamedKey::Escape);
    if scanning {
        // First Esc: the scan is cancelled (CancelScan), the window stays.
        assert_eq!(t.top(), Some(ActiveDialog::SpellCheck));
    }
    // The scan only stops when its task answers (not run here), so the
    // window stays; the application closes it.
    assert_eq!(t.top(), Some(ActiveDialog::SpellCheck));
    t.rig.app_msg(SpellingMessage::CloseCheck);
    assert_eq!(t.top(), None);
    // Drawing afterwards is safe.
    drop(t.sim_default());
}

#[test]
fn quitting_with_a_scan_and_stacked_windows_in_flight_is_quiet() {
    let mut t = Tools::new();
    t.open_base(&misspelled());
    t.load_ssp();
    t.action(Action::PlayerFinder);
    drop(t.rig.rig.shell.update(Message::App(
        SpellingMessage::OpenCheck(NameKind::Player).into(),
    )));
    drop(
        t.rig
            .rig
            .shell
            .update(Message::App(SpellingMessage::OpenAddElo.into())),
    );
    t.rig.app_msg(AppMessage::Quit);
    drop(t.sim_default());
    // Keys after the quit do not panic either.
    t.named(NamedKey::Escape);
    t.named(NamedKey::Enter);
}

#[test]
fn closing_a_window_from_the_application_while_the_keyboard_is_on_a_row_resets_it() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    t.named(NamedKey::Down);
    t.named(NamedKey::Down);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, Some(1));
    t.rig.app_msg(FinderMessage::ClosePlayers);
    t.action(Action::PlayerFinder);
    // A reopened finder starts from a clean list state.
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, None);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.scroll, 0.0);
}
