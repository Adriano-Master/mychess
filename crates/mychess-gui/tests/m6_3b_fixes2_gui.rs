//! Review round of M6-3c (GUI): the File menu cancels, reloads and unloads
//! the spelling file, and the start-up without a preferences file loads it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_3c_common;

use m6_3c_common::*;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::{Message, Shell};

/// The action of the File menu entry `key`, when it is enabled.
fn file_action(t: &Tools, key: &str) -> Option<Message> {
    // Spec M9-7: the spelling items are in a submenu of the Database menu.
    fn walk(entries: &[MenuEntry], key: &str, out: &mut Vec<Option<Message>>) {
        for entry in entries {
            match entry {
                MenuEntry::Item {
                    label: Label::Key(k),
                    action,
                    ..
                } if *k == key => out.push(action.clone()),
                MenuEntry::Submenu { entries, .. } => walk(entries, key, out),
                _ => {}
            }
        }
    }
    let specs = menu_specs(t.app(), &t.shell().ui, mychess_gui::ViewOptions::default());
    let mut found = Vec::new();
    for spec in &specs {
        walk(&spec.entries, key, &mut found);
    }
    assert_eq!(found.len(), 1, "{key} is exactly once in the menus");
    found.remove(0)
}

const KEYS: [&str; 3] = [
    "menu-file-reload-spelling",
    "menu-file-unload-spelling",
    "menu-file-cancel-spelling-load",
];

#[test]
fn without_a_spelling_file_none_of_the_commands_is_enabled() {
    let mut t = Tools::new();
    // The rig may have started a load from the real data folder.
    t.rig.app_msg(mychess_app::SpellingMessage::Unload);
    for key in KEYS {
        assert!(file_action(&t, key).is_none(), "{key}");
    }
}

#[test]
fn reload_reads_the_file_again() {
    let mut t = Tools::new();
    t.load_ssp();
    let before = t.app().spelling_status_view().counts.expect("counts");
    let path = t.app().spelling_status_view().path.expect("path");
    std::fs::write(&path, "@PLAYER \"\"\nAlfa, A\n").expect("write");
    let reload = file_action(&t, "menu-file-reload-spelling").expect("enabled");
    assert!(file_action(&t, "menu-file-cancel-spelling-load").is_none());
    t.rig.msg(reload);
    let after = t.app().spelling_status_view().counts.expect("counts");
    assert_eq!(after.players, 1);
    assert_ne!(before.players, after.players);
}

#[test]
fn unload_forgets_the_file() {
    let mut t = Tools::new();
    t.load_ssp();
    let unload = file_action(&t, "menu-file-unload-spelling").expect("enabled");
    t.rig.msg(unload);
    assert!(!t.app().spelling().is_loaded());
    for key in KEYS {
        assert!(file_action(&t, key).is_none(), "{key}");
    }
}

#[test]
fn cancel_stops_a_load_in_flight() {
    let mut t = Tools::new();
    let path = t.dir.path().join("spelling.ssp");
    std::fs::write(&path, SSP).expect("write");
    // The task is not run yet: the load is in flight.
    let task = t.rig.rig.shell.update(Message::App(
        mychess_app::SpellingMessage::Load(path).into(),
    ));
    assert!(t.app().spelling_status_view().loading.is_some());
    assert!(file_action(&t, "menu-file-reload-spelling").is_none());
    let cancel = file_action(&t, "menu-file-cancel-spelling-load").expect("enabled");
    t.rig.msg(cancel);
    t.rig.drain(task);
    assert!(!t.app().spelling().is_loaded(), "the load was cut");
    assert!(t.app().spelling_status_view().loading.is_none());
}

#[test]
fn a_shell_without_a_preferences_file_still_loads_the_spelling_file_at_start() {
    let (shell, _task) = Shell::boot(None, None);
    assert!(shell.ui.dialogs.player_tools.started);
    assert!(shell.app.spelling_status_view().loading.is_some());
}
