//! The keys of the game list in the central shortcut map (spec M3-4, slice
//! 4c: D564, D565 and the M2 contract D148-D154).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use mychess_app::{Action, Key, KeyInput, Modifiers, NamedKey, Shortcuts};

fn input(key: Key, command: bool, shift: bool, alt: bool) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers {
            command,
            shift,
            alt,
        },
    }
}

const LIST_ACTIONS: [Action; 4] = [
    Action::ListSelectAll,
    Action::ListRemoveFromFilter,
    Action::ListToggleDeleted,
    Action::ListFocusSearch,
];

#[test]
fn the_four_list_actions_have_the_keys_of_the_decision() {
    let map = Shortcuts::defaults();
    let cases = [
        (Key::Char('a'), true, true, Action::ListSelectAll),
        (
            Key::Named(NamedKey::Delete),
            false,
            true,
            Action::ListRemoveFromFilter,
        ),
        (
            Key::Named(NamedKey::Delete),
            true,
            false,
            Action::ListToggleDeleted,
        ),
        (Key::Char('j'), true, true, Action::ListFocusSearch),
    ];
    for (key, command, shift, action) in cases {
        assert_eq!(
            map.lookup(&input(key, command, shift, false)),
            Some(action),
            "{action:?}"
        );
        assert_eq!(map.bindings(action).len(), 1, "{action:?}");
    }
}

#[test]
fn the_list_actions_are_outside_the_configurable_m2_table() {
    let configurable: Vec<Action> = Action::all().collect();
    for action in LIST_ACTIONS {
        assert!(
            !configurable.contains(&action),
            "{action:?} must stay out of all()"
        );
    }
}

#[test]
fn the_keys_of_the_scid_list_keep_their_m2_meaning_in_the_map() {
    // D564: Ctrl+A, Delete, Ctrl+Delete and Ctrl+F mean other things in the
    // central map; the list acts on them only with the pointer over it.
    let map = Shortcuts::defaults();
    assert_eq!(
        map.lookup(&input(Key::Char('a'), true, false, false)),
        Some(Action::AddVariation)
    );
    assert_eq!(
        map.lookup(&input(Key::Named(NamedKey::Delete), false, false, false)),
        Some(Action::TruncateGame)
    );
    assert_eq!(
        map.lookup(&input(Key::Char('f'), true, false, false)),
        Some(Action::FlipBoard)
    );
}

#[test]
fn no_key_belongs_to_two_actions() {
    let map = Shortcuts::defaults();
    let mut owners: BTreeMap<String, Action> = BTreeMap::new();
    for (action, bindings) in map.iter() {
        for binding in bindings {
            let id = format!("{:?}+{:?}", binding.modifiers, binding.key);
            if let Some(previous) = owners.insert(id.clone(), action) {
                panic!("{id} is bound to {previous:?} and {action:?}");
            }
        }
    }
    assert!(
        owners.len() > 50,
        "the sweep saw the whole map: {}",
        owners.len()
    );
}

#[test]
fn lookup_agrees_with_the_bindings_for_every_action() {
    let map = Shortcuts::defaults();
    for (action, bindings) in map.iter() {
        for binding in bindings {
            let found = map.lookup(&KeyInput {
                key: binding.key,
                modifiers: binding.modifiers,
            });
            assert_eq!(found, Some(action), "{action:?} {binding:?}");
        }
    }
}

#[test]
fn the_shortcuts_of_the_databases_slice_are_intact() {
    let map = Shortcuts::defaults();
    for n in 1..=9u32 {
        let c = char::from_digit(n, 10).unwrap();
        assert_eq!(
            map.lookup(&input(Key::Char(c), true, false, false)),
            Some(Action::SelectBase(u8::try_from(n).unwrap())),
            "Command+{c}"
        );
    }
    assert_eq!(
        map.lookup(&input(Key::Named(NamedKey::Tab), false, true, false)),
        Some(Action::PreviousBase)
    );
}

#[test]
fn the_list_actions_can_be_turned_off_without_touching_the_others() {
    let mut overrides = BTreeMap::new();
    overrides.insert("ListToggleDeleted".to_owned(), String::new());
    let (map, issues) = Shortcuts::defaults().with_overrides(&overrides);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(
        map.lookup(&input(Key::Named(NamedKey::Delete), true, false, false)),
        None
    );
    assert_eq!(
        map.lookup(&input(Key::Char('j'), true, true, false)),
        Some(Action::ListFocusSearch)
    );
}
