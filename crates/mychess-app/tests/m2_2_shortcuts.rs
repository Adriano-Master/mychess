//! The keyboard shortcut map (spec M2-2, rules 59-64, D148-D154) and what
//! the actions do in `update`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::default_trait_access,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::assigning_clones,
    clippy::field_reassign_with_default,
    clippy::type_complexity,
    let_underscore_drop
)]

mod m2_2_common;

use std::collections::BTreeMap;

use m2_2_common::*;
use mychess_app::{
    Action, Binding, BindingParseError, Key, KeyInput, Modifiers, NamedKey, Scope, ShortcutIssue,
    Shortcuts,
};
use proptest::prelude::*;
use rstest::rstest;

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

fn ch(c: char) -> Key {
    Key::Char(c)
}

fn named(n: NamedKey) -> Key {
    Key::Named(n)
}

fn plain(key: Key) -> KeyInput {
    input(key, false, false, false)
}

fn cmd(key: Key) -> KeyInput {
    input(key, true, false, false)
}

fn cmd_shift(key: Key) -> KeyInput {
    input(key, true, true, false)
}

fn overrides(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// The spec table "Ações do M2 e teclas padrão" (the resolved question 7
/// moved the trial mode to `Command+T`).
const TABLE: &[(Action, &str)] = &[
    (Action::Back, "Left"),
    (Action::Forward, "Right"),
    (Action::BackFast, "Up"),
    (Action::ForwardFast, "Down"),
    (Action::GoStart, "Home"),
    (Action::GoEnd, "End"),
    (Action::GotoMove, "Command+G"),
    (Action::FlipBoard, "Command+F"),
    (Action::ClearMoveEntry, "Escape"),
    (Action::TruncateGame, "Delete"),
    (Action::AddVariation, "Command+A"),
    (Action::EnterVariation, "V"),
    (Action::ExitVariation, "Z"),
    (Action::ToggleTrialMode, "Command+T"),
    (Action::Undo, "Command+Z"),
    (Action::Redo, "Command+Y"),
    (Action::Redo, "Command+Shift+Z"),
    (Action::EditGameInfo, "Command+Shift+H"),
    (Action::TruncateGame, "Command+Backspace"),
    (Action::NewGame, "Command+Shift+N"),
    (Action::OpenFile, "Command+O"),
    (Action::Save, "Command+S"),
    (Action::SetupBoard, "Command+Shift+B"),
    (Action::CopyFen, "Command+Shift+C"),
    (Action::PasteFen, "Command+Shift+V"),
    (Action::ImportPgn, "Command+Shift+I"),
    (Action::NextGame, "Command+Down"),
    (Action::PrevGame, "Command+Up"),
    (Action::FirstGame, "Command+Home"),
    (Action::LastGame, "Command+End"),
    (Action::ToggleGameInfo, "Command+I"),
    (Action::ShowNotation, "Command+P"),
    (Action::ShowCommentEditor, "Command+E"),
    (Action::BoardSmaller, "Command+Shift+Left"),
    (Action::BoardLarger, "Command+Shift+Right"),
    (Action::FullScreen, "F11"),
    (Action::Help, "F1"),
    (Action::Quit, "Command+Q"),
];

const NO_DEFAULT: &[Action] = &[
    Action::CopyPgn,
    Action::SaveAs,
    Action::AppendToPgn,
    Action::ToggleAutoplay,
];

// ------------------------------------------------------------------ rule 59

#[test]
fn rule59_defaults_have_no_duplicate_bindings() {
    let s = Shortcuts::defaults();
    let mut seen: Vec<(Key, Modifiers, Action)> = Vec::new();
    for (action, bindings) in s.iter() {
        for b in bindings {
            if let Some((_, _, other)) = seen
                .iter()
                .find(|(k, m, _)| *k == b.key && *m == b.modifiers)
            {
                panic!("{action:?} and {other:?} share {b}");
            }
            seen.push((b.key, b.modifiers, action));
        }
    }
}

#[test]
fn rule59_every_action_of_the_table_has_its_documented_binding() {
    let s = Shortcuts::defaults();
    for (action, text) in TABLE {
        let expected = Binding::parse(text).unwrap();
        let got = s.bindings(*action);
        assert!(
            got.iter()
                .any(|b| b.key == expected.key && b.modifiers == expected.modifiers),
            "{action:?} should be {text}, got {got:?}"
        );
    }
}

#[test]
fn rule59_actions_without_a_default_key_are_exactly_the_documented_ones() {
    let s = Shortcuts::defaults();
    for action in Action::all() {
        let has_binding = !s.bindings(action).is_empty();
        let in_table = TABLE.iter().any(|(a, _)| *a == action);
        let documented_empty = NO_DEFAULT.contains(&action);
        assert!(
            has_binding == in_table && has_binding != documented_empty,
            "{action:?}: bound={has_binding} in_table={in_table} no_default={documented_empty}"
        );
    }
    // OpenRecent(n) has no default key either.
    assert!(s.bindings(Action::OpenRecent(1)).is_empty());
}

// ------------------------------------------------------------------ rule 60

#[test]
fn rule60_lookups_from_the_spec() {
    let s = Shortcuts::defaults();
    assert_eq!(s.lookup(&cmd_shift(ch('b'))), Some(Action::SetupBoard));
    assert_eq!(
        s.lookup(&cmd(ch('b'))),
        None,
        "Command+B without Shift is not bound"
    );
    assert_eq!(
        s.lookup(&plain(named(NamedKey::Up))),
        Some(Action::BackFast)
    );
    assert_eq!(s.lookup(&plain(ch('v'))), Some(Action::EnterVariation));
    assert_eq!(s.lookup(&plain(ch('z'))), Some(Action::ExitVariation));
    assert_eq!(
        s.lookup(&cmd(ch('v'))),
        None,
        "Ctrl+V is reserved for the Clipbase (M3, D153)"
    );
    assert_eq!(s.lookup(&cmd(ch('c'))), None);
    assert_eq!(s.lookup(&cmd_shift(ch('c'))), Some(Action::CopyFen));
    assert_eq!(s.lookup(&cmd_shift(ch('v'))), Some(Action::PasteFen));
}

#[test]
fn rule60_uppercase_letters_are_folded_and_shift_is_a_separate_flag() {
    let s = Shortcuts::defaults();
    assert_eq!(s.lookup(&cmd_shift(ch('B'))), Some(Action::SetupBoard));
    assert_eq!(s.lookup(&plain(ch('V'))), s.lookup(&plain(ch('v'))));
    // Shift+V (the Scid `V`) is not the plain `v` binding.
    let shifted_v = input(ch('v'), false, true, false);
    assert_eq!(s.lookup(&shifted_v), None);
}

#[test]
fn rule60_symbols_that_need_shift_ignore_it() {
    let (s, issues) = Shortcuts::defaults().with_overrides(&overrides(&[("Help", "?")]));
    assert!(issues.is_empty(), "{issues:?}");
    let with = s.lookup(&input(ch('?'), false, true, false));
    let without = s.lookup(&plain(ch('?')));
    assert_eq!(with, Some(Action::Help));
    assert_eq!(with, without);
    let (s, _) = Shortcuts::defaults().with_overrides(&overrides(&[("Help", "Command+?")]));
    assert_eq!(
        s.lookup(&input(ch('?'), true, true, false)),
        Some(Action::Help)
    );
    assert_eq!(s.lookup(&cmd(ch('?'))), Some(Action::Help));
}

#[test]
fn rule60_the_modifiers_must_match_exactly() {
    let s = Shortcuts::defaults();
    // Command+S with an extra Alt or Shift is a different chord.
    assert_eq!(s.lookup(&cmd(ch('s'))), Some(Action::Save));
    assert_eq!(s.lookup(&input(ch('s'), true, false, true)), None);
    // Command+Shift+Q is free in the map (Command+Shift+S is the eval bar;
    // Command+Shift+K is the book panel since M5-6b, D1790).
    assert_eq!(s.lookup(&cmd_shift(ch('q'))), None);
    assert_eq!(s.lookup(&cmd_shift(ch('k'))), Some(Action::ToggleBookPane));
    assert_eq!(s.lookup(&plain(ch('s'))), None);
    // Arrows: plain, Command and Command+Shift are three different things.
    assert_eq!(s.lookup(&plain(named(NamedKey::Left))), Some(Action::Back));
    assert_eq!(
        s.lookup(&cmd_shift(named(NamedKey::Left))),
        Some(Action::BoardSmaller)
    );
    assert_eq!(s.lookup(&cmd(named(NamedKey::Left))), None);
    assert_eq!(
        s.lookup(&cmd(named(NamedKey::Down))),
        Some(Action::NextGame)
    );
}

#[test]
fn d152_the_move_entry_keys_never_become_global_shortcuts() {
    let s = Shortcuts::defaults();
    for c in "abcdefgh12345678KQRBNOkqrbno".chars() {
        assert_eq!(
            s.lookup(&plain(ch(c))),
            None,
            "{c} is reserved for move entry"
        );
        assert_eq!(
            s.lookup(&input(ch(c), false, true, false)),
            None,
            "Shift+{c}"
        );
    }
    for (action, bindings) in s.iter() {
        for b in bindings {
            if let (Key::Char(c), true) = (b.key, b.modifiers == Modifiers::NONE) {
                assert!(
                    !"abcdefgh12345678kqrbnoKQRBNO".contains(c),
                    "{action:?} is bound to the bare move-entry key {c}"
                );
            }
        }
    }
}

#[test]
fn d152_backspace_alone_is_not_bound_and_delete_truncates_everywhere() {
    // M2-4 resolved question 7 added `Command+Backspace` (the Mac's Delete)
    // to truncate; the bare key stays free for the move entry.
    let s = Shortcuts::defaults();
    assert_eq!(s.lookup(&plain(named(NamedKey::Backspace))), None);
    assert_eq!(
        s.lookup(&cmd(named(NamedKey::Backspace))),
        Some(Action::TruncateGame)
    );
    assert_eq!(
        s.lookup(&plain(named(NamedKey::Delete))),
        Some(Action::TruncateGame)
    );
}

#[test]
fn d154_trial_mode_is_command_t_and_command_space_is_free() {
    let s = Shortcuts::defaults();
    assert_eq!(s.lookup(&cmd(ch('t'))), Some(Action::ToggleTrialMode));
    assert_eq!(
        s.lookup(&cmd(named(NamedKey::Space))),
        None,
        "the Spotlight chord is not taken"
    );
}

// ------------------------------------------------------------------ rule 61

#[test]
fn rule61_scopes_follow_the_rule_for_every_default_binding() {
    let s = Shortcuts::defaults();
    for (action, bindings) in s.iter() {
        for b in bindings {
            let text_edit_chord = b.modifiers
                == (Modifiers {
                    command: true,
                    shift: false,
                    alt: false,
                })
                && matches!(b.key, Key::Char(c) if "acvxzy".contains(c));
            // Corrected spec (function keys never type text): F1..F12 are `Always`.
            let function_key = matches!(b.key, Key::Named(NamedKey::F(_)));
            let expected = if (b.modifiers == Modifiers::NONE && !function_key) || text_edit_chord {
                Scope::UnlessTextFocus
            } else {
                Scope::Always
            };
            assert_eq!(b.scope, expected, "{action:?} {b}");
        }
    }
}

#[test]
fn rule61_a_focused_text_field_wins_over_unmodified_and_edit_shortcuts() {
    let s = Shortcuts::defaults();
    for key in [
        plain(named(NamedKey::Left)),
        plain(ch('v')),
        plain(named(NamedKey::Delete)),
        plain(named(NamedKey::Escape)),
        cmd(ch('z')),
        cmd(ch('a')),
        cmd(ch('y')),
    ] {
        assert!(
            s.lookup_in(&key, false).is_some(),
            "{key:?} works with no text focus"
        );
        assert_eq!(
            s.lookup_in(&key, true),
            None,
            "{key:?} is left to the text field"
        );
    }
    for (key, action) in [
        (cmd(ch('s')), Action::Save),
        (cmd_shift(ch('b')), Action::SetupBoard),
        (cmd(ch('f')), Action::FlipBoard),
        (cmd(named(NamedKey::Down)), Action::NextGame),
    ] {
        assert_eq!(
            s.lookup_in(&key, true),
            Some(action),
            "{key:?} works inside text fields"
        );
    }
}

#[test]

fn spec_table_says_help_and_fullscreen_work_while_typing() {
    let s = Shortcuts::defaults();
    assert_eq!(
        s.lookup_in(&plain(named(NamedKey::F(1))), true),
        Some(Action::Help)
    );
    assert_eq!(
        s.lookup_in(&plain(named(NamedKey::F(11))), true),
        Some(Action::FullScreen)
    );
}

// ------------------------------------------------------------------ rule 62

#[test]
fn rule62_display_and_parse_are_inverse_for_every_default() {
    for (action, bindings) in Shortcuts::defaults().iter() {
        for b in bindings {
            let text = b.to_string();
            assert_eq!(Binding::parse(&text).unwrap(), *b, "{action:?}: {text}");
        }
    }
    assert_eq!(
        Binding::parse("Command+Shift+B").unwrap().to_string(),
        "Command+Shift+B"
    );
}

#[rstest]
#[case("Ctrl+B")]
#[case("Cmd+B")]
#[case("Control+B")]
#[case("command+b")]
#[case("COMMAND+B")]
#[case("Command+b")]
#[case(" Command+B ")]
fn rule62_synonyms_and_case_normalise_to_command(#[case] text: &str) {
    let b = Binding::parse(text).unwrap();
    assert_eq!(b.to_string(), "Command+B", "{text}");
    assert!(b.modifiers.command && !b.modifiers.shift && !b.modifiers.alt);
}

#[rstest]
#[case("Command+")]
#[case("Foo+B")]
#[case("Command+Shift")]
#[case("")]
#[case("   ")]
#[case("Command+Command+")]
#[case("Command++B")]
#[case("Command+B+C")]
#[case("F13")]
#[case("F0")]
#[case("Command+Nonsense")]
#[case("Command+Shift+")]
fn rule62_malformed_bindings_are_errors(#[case] text: &str) {
    assert!(
        Binding::parse(text).is_err(),
        "{text:?} should not parse: {:?}",
        Binding::parse(text)
    );
}

#[test]
fn rule62_error_kinds() {
    assert!(
        matches!(Binding::parse("Foo+B"), Err(BindingParseError::UnknownModifier(m)) if m == "Foo")
    );
    assert!(matches!(
        Binding::parse("Command+Nonsense"),
        Err(BindingParseError::UnknownKey(_))
    ));
    assert!(matches!(
        Binding::parse("Command+"),
        Err(BindingParseError::MissingKey(_))
    ));
}

#[test]
fn rule62_the_plus_key_itself_can_be_bound() {
    let b = Binding::parse("Command++").unwrap();
    assert_eq!(b.key, Key::Char('+'));
    assert!(b.modifiers.command);
    assert_eq!(Binding::parse(&b.to_string()).unwrap(), b);
    assert_eq!(Binding::parse("+").unwrap().key, Key::Char('+'));
}

#[test]
fn rule62_function_keys_and_named_keys_round_trip() {
    for n in 1..=12u8 {
        let b = Binding::parse(&format!("F{n}")).unwrap();
        assert_eq!(b.key, Key::Named(NamedKey::F(n)));
        assert_eq!(b.to_string(), format!("F{n}"));
    }
    for name in [
        "Left",
        "Right",
        "Up",
        "Down",
        "Home",
        "End",
        "Delete",
        "Backspace",
        "Enter",
        "Space",
        "Escape",
        "Tab",
    ] {
        let b = Binding::parse(&format!("Command+Shift+{name}")).unwrap();
        assert_eq!(b.to_string(), format!("Command+Shift+{name}"));
        assert_eq!(
            Binding::parse(&name.to_lowercase()).unwrap().to_string(),
            name
        );
    }
}

/// Regression: a multi-byte first character in a key name made
/// `NamedKey::parse` slice inside a UTF-8 sequence (panic). The text comes from
/// the user-editable `[shortcuts]` table of the preferences file.
#[rstest]
#[case("\u{e9}\u{e9}")]
#[case("Command+\u{e9}a")]
#[case("\u{1f574}x")]
#[case("\u{a48}1")]
#[case("Command+Shift+\u{e9}\u{e9}\u{e9}")]
fn rule66_multibyte_key_names_do_not_panic(#[case] text: &str) {
    assert!(Binding::parse(text).is_err(), "{text:?}");
}

#[test]
fn rule66_multibyte_binding_text_in_preferences_does_not_crash_the_app() {
    let (_, issues) = Shortcuts::defaults().with_overrides(&overrides(&[("Save", "\u{e9}\u{e9}")]));
    assert!(matches!(
        issues.as_slice(),
        [ShortcutIssue::InvalidBinding { .. }]
    ));
    let l = mychess_app::load_preferences("[shortcuts]\nSave = \"\u{e9}\u{e9}\"\n");
    let mut h = Harness::new();
    h.send(Message::PreferencesLoaded(l));
    assert!(
        h.state
            .prefs_issues
            .iter()
            .any(|i| matches!(i, mychess_app::PrefsIssue::Shortcut(_)))
    );
}

#[test]
fn rule62_alt_and_modifier_order() {
    let a = Binding::parse("Shift+Alt+Command+X").unwrap();
    let b = Binding::parse("Command+Alt+Shift+X").unwrap();
    assert_eq!(a, b);
    assert_eq!(Binding::parse(&a.to_string()).unwrap(), a);
}

proptest! {
    #[test]
    fn rule66_binding_parse_never_panics(text in "\\PC{0,30}") {
        drop(Binding::parse(&text));
    }

    #[test]
    fn rule62_generated_bindings_round_trip(
        command in any::<bool>(), shift in any::<bool>(), alt in any::<bool>(),
        key in prop_oneof![
            "[a-z0-9]".prop_map(|s| s),
            Just("Left".to_owned()), Just("Home".to_owned()), Just("Space".to_owned()),
            (1u8..=12).prop_map(|n| format!("F{n}")),
        ]
    ) {
        let mut text = String::new();
        if command { text.push_str("Command+"); }
        if alt { text.push_str("Alt+"); }
        if shift { text.push_str("Shift+"); }
        text.push_str(&key);
        let b = Binding::parse(&text).unwrap();
        let again = Binding::parse(&b.to_string()).unwrap();
        prop_assert_eq!(b, again);
        // The scope rule is consistent with the modifiers.
        if b.modifiers == Modifiers::NONE {
            let expected = if matches!(b.key, Key::Named(NamedKey::F(_))) {
                Scope::Always
            } else {
                Scope::UnlessTextFocus
            };
            prop_assert_eq!(b.scope, expected);
        }
    }
}

// ------------------------------------------------------------------ rule 63

#[test]
fn rule63_replacing_a_binding() {
    let (s, issues) =
        Shortcuts::defaults().with_overrides(&overrides(&[("FlipBoard", "Command+Shift+F")]));
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(s.lookup(&cmd_shift(ch('f'))), Some(Action::FlipBoard));
    assert_eq!(s.lookup(&cmd(ch('f'))), None, "the old key is released");
    assert_eq!(s.bindings(Action::FlipBoard).len(), 1);
    // Untouched actions are intact.
    assert_eq!(s.lookup(&cmd(ch('s'))), Some(Action::Save));
}

#[test]
fn rule63_an_empty_text_turns_the_shortcut_off() {
    let (s, issues) =
        Shortcuts::defaults().with_overrides(&overrides(&[("Save", ""), ("Help", "  ")]));
    assert!(issues.is_empty(), "{issues:?}");
    assert!(s.bindings(Action::Save).is_empty());
    assert_eq!(s.lookup(&cmd(ch('s'))), None);
    assert!(s.bindings(Action::Help).is_empty());
}

#[test]
fn rule63_a_binding_of_another_active_action_is_ignored_with_a_conflict() {
    let (s, issues) = Shortcuts::defaults().with_overrides(&overrides(&[("Save", "Command+O")]));
    assert_eq!(
        issues,
        vec![ShortcutIssue::Conflict {
            action: Action::Save,
            other: Action::OpenFile
        }]
    );
    assert_eq!(s.lookup(&cmd(ch('o'))), Some(Action::OpenFile));
    assert_eq!(
        s.lookup(&cmd(ch('s'))),
        Some(Action::Save),
        "nothing was switched off in silence"
    );
}

#[test]
fn rule63_swapping_two_keys_never_loses_a_shortcut() {
    let (s, issues) = Shortcuts::defaults().with_overrides(&overrides(&[
        ("Save", "Command+O"),
        ("OpenFile", "Command+S"),
    ]));
    assert!(!issues.is_empty());
    // Whatever happened, both keys still lead somewhere and no key has two owners.
    assert!(s.lookup(&cmd(ch('o'))).is_some());
    assert!(s.lookup(&cmd(ch('s'))).is_some());
}

#[test]
fn rule63_two_overrides_to_the_same_new_key_give_one_winner_and_one_conflict() {
    let (s, issues) = Shortcuts::defaults()
        .with_overrides(&overrides(&[("Help", "Command+K"), ("Quit", "Command+K")]));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(matches!(issues[0], ShortcutIssue::Conflict { .. }));
    assert!(s.lookup(&cmd(ch('k'))).is_some());
    let (s2, _) = Shortcuts::defaults()
        .with_overrides(&overrides(&[("Help", "Command+K"), ("Quit", "Command+K")]));
    assert_eq!(
        s.lookup(&cmd(ch('k'))),
        s2.lookup(&cmd(ch('k'))),
        "deterministic"
    );
}

#[test]
fn rule63_unknown_actions_and_bad_texts_are_reported_and_change_nothing() {
    let defaults = Shortcuts::defaults();
    let (s, issues) = Shortcuts::defaults().with_overrides(&overrides(&[
        ("Bogus", "Command+K"),
        ("Save", "Command+"),
        ("Help", "Foo+X"),
    ]));
    assert!(issues.contains(&ShortcutIssue::UnknownAction {
        name: "Bogus".into()
    }));
    assert!(issues.contains(&ShortcutIssue::InvalidBinding {
        action: Action::Save,
        text: "Command+".into()
    }));
    assert!(issues.contains(&ShortcutIssue::InvalidBinding {
        action: Action::Help,
        text: "Foo+X".into()
    }));
    assert_eq!(issues.len(), 3);
    assert_eq!(s.lookup(&cmd(ch('k'))), None);
    for (action, bindings) in defaults.iter() {
        assert_eq!(s.bindings(action), bindings, "{action:?}");
    }
}

#[test]
fn rule63_assigning_the_same_key_it_already_has_is_not_a_conflict() {
    let (_, issues) = Shortcuts::defaults().with_overrides(&overrides(&[
        ("FlipBoard", "Command+F"),
        ("Save", "ctrl+s"),
    ]));
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn rule63_a_key_freed_by_a_disabled_action_can_be_reused_in_the_same_batch() {
    // D151: a conflict is with another *active* action. `Quit` is switched
    // off in this very configuration, so `Help` may take Command+Q.
    let (s, issues) =
        Shortcuts::defaults().with_overrides(&overrides(&[("Quit", ""), ("Help", "Command+Q")]));
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(s.lookup(&cmd(ch('q'))), Some(Action::Help));
    // And the reverse alphabetical order behaves the same.
    let (s, issues) = Shortcuts::defaults().with_overrides(&overrides(&[
        ("Zzz", ""),
        ("BoardLarger", ""),
        ("FullScreen", "Command+Shift+Right"),
    ]));
    assert_eq!(
        issues,
        vec![ShortcutIssue::UnknownAction { name: "Zzz".into() }]
    );
    assert_eq!(
        s.lookup(&cmd_shift(named(NamedKey::Right))),
        Some(Action::FullScreen)
    );
}

#[test]
fn rule63_an_override_can_use_a_move_entry_key_only_by_explicit_choice() {
    // Not forbidden by the spec (D152 speaks of defaults); must not panic.
    let (_, issues) = Shortcuts::defaults().with_overrides(&overrides(&[("Help", "e")]));
    assert!(issues.len() <= 1);
}

proptest! {
    #[test]
    fn rule63_arbitrary_overrides_never_panic_and_never_create_duplicate_owners(
        map in proptest::collection::btree_map(
            prop_oneof![Just("Save".to_owned()), Just("Help".to_owned()), Just("Quit".to_owned()), Just("FlipBoard".to_owned()), "[A-Za-z]{0,10}"],
            "\\PC{0,15}",
            0..8,
        )
    ) {
        let (s, _issues) = Shortcuts::defaults().with_overrides(&map);
        let mut seen: Vec<(Key, Modifiers)> = Vec::new();
        for (_, bindings) in s.iter() {
            for b in bindings {
                prop_assert!(!seen.contains(&(b.key, b.modifiers)), "duplicate owner of {b}");
                seen.push((b.key, b.modifiers));
            }
        }
    }
}

// ------------------------------------------------------------------ rule 64

#[test]
fn rule64_no_default_binding_uses_alt() {
    for (action, bindings) in Shortcuts::defaults().iter() {
        for b in bindings {
            assert!(!b.modifiers.alt, "{action:?} uses Alt: {b}");
            assert!(!b.to_string().contains("Alt"));
        }
    }
}

#[test]
fn rule64_alt_chords_never_trigger_a_default_action() {
    let s = Shortcuts::defaults();
    for (_, bindings) in s.iter() {
        for b in bindings {
            let mut m = b.modifiers;
            m.alt = true;
            assert_eq!(
                s.lookup(&KeyInput {
                    key: b.key,
                    modifiers: m
                }),
                None,
                "Alt+{b}"
            );
        }
    }
}

#[test]
fn action_names_round_trip_and_labels_have_keys() {
    for a in Action::all() {
        assert_eq!(Action::from_name(a.name()), Some(a), "{a:?}");
        assert!(a.message_key().starts_with("action-"), "{a:?}");
    }
    assert_eq!(Action::from_name("Nope"), None);
    assert_eq!(Action::from_name(""), None);
    assert_eq!(Action::OpenRecent(3).message_key(), "action-open-recent");
}

#[test]
fn shortcut_issue_keys() {
    for issue in [
        ShortcutIssue::UnknownAction { name: "x".into() },
        ShortcutIssue::InvalidBinding {
            action: Action::Save,
            text: "x".into(),
        },
        ShortcutIssue::Conflict {
            action: Action::Save,
            other: Action::Help,
        },
    ] {
        assert!(issue.message_key().starts_with("shortcut-issue-"));
    }
}

// ------------------------------------------------------------------ what actions do

fn long_game_harness() -> Harness {
    let mut h = Harness::new();
    let ucis = [
        "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1", "f8e7", "f1e1",
        "b7b5", "a4b3", "d7d6", "c2c3", "e8g8", "h2h3", "c6a5", "b3c2", "c7c5", "d2d4", "d8c7",
        "b1d2", "a5c6", "d4d5",
    ];
    h.play_all(&ucis);
    h
}

fn cursor_ply(h: &Harness) -> u32 {
    h.state
        .session
        .game()
        .ply(h.state.session.cursor())
        .unwrap()
}

#[test]
fn d152_up_and_down_move_ten_plies_not_five() {
    let mut h = long_game_harness();
    assert_eq!(cursor_ply(&h), 25);
    h.send(Action::BackFast);
    assert_eq!(cursor_ply(&h), 15);
    h.send(Action::BackFast);
    assert_eq!(cursor_ply(&h), 5);
    h.send(Action::BackFast);
    assert_eq!(cursor_ply(&h), 0, "stops at the start");
    h.send(Action::ForwardFast);
    assert_eq!(cursor_ply(&h), 10);
    h.send(Action::ForwardFast);
    h.send(Action::ForwardFast);
    assert_eq!(cursor_ply(&h), 25, "stops at the end");
}

#[test]
fn d152_up_leaves_the_variation_when_inside_one() {
    let mut h = long_game_harness();
    // Go back to 1.e4 and play 1...c5 as a variation.
    h.send(GameMessage::Navigate(mychess_app::Nav::Start));
    h.play("e2e4");
    h.send(GameMessage::StartVariation);
    let mv = h.state.session.position().parse_uci("c7c5").unwrap();
    h.send(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions {
            force_variation: true,
        },
    });
    h.play("g1f3");
    assert!(
        h.state.game_info().in_variation,
        "setup: inside a variation"
    );
    h.send(Action::BackFast);
    assert!(
        !h.state.game_info().in_variation,
        "Up leaves the variation first"
    );
}

#[test]
fn actions_back_forward_start_end() {
    let mut h = long_game_harness();
    h.send(Action::Back);
    assert_eq!(cursor_ply(&h), 24);
    h.send(Action::Forward);
    assert_eq!(cursor_ply(&h), 25);
    h.send(Action::GoStart);
    assert_eq!(cursor_ply(&h), 0);
    h.send(Action::GoEnd);
    assert_eq!(cursor_ply(&h), 25);
}

#[test]
fn action_flip_board_toggles_the_view_flag_and_not_the_game() {
    let mut h = Harness::new();
    let game = h.state.session.game().clone();
    assert!(!h.state.board_flipped);
    assert!(h.raw(Action::FlipBoard).is_empty());
    assert!(h.state.board_flipped);
    h.send(Action::FlipBoard);
    assert!(!h.state.board_flipped);
    assert!(h.state.session.game().same_content(&game));
    assert!(!h.state.is_dirty());
}

#[test]
fn action_delete_truncates_and_undo_redo_work() {
    let mut h = long_game_harness();
    h.send(GameMessage::Navigate(mychess_app::Nav::Back(5)));
    h.send(Action::TruncateGame);
    assert_eq!(h.plies(), 20);
    h.send(Action::Undo);
    assert_eq!(h.plies(), 25);
    h.send(Action::Redo);
    assert_eq!(h.plies(), 20);
}

#[test]
fn action_trial_mode_toggles() {
    let mut h = Harness::new();
    h.send(Action::ToggleTrialMode);
    assert!(h.state.session.in_trial());
    h.send(Action::ToggleTrialMode);
    assert!(!h.state.session.in_trial());
}

#[test]
fn actions_that_open_dialogs_and_ask_the_host() {
    let mut h = Harness::new();
    h.send(Action::SetupBoard);
    assert!(h.state.setup.is_some());
    h.send(SetupMessage::Cancel);
    let host = h.send(Action::ImportPgn);
    assert!(h.state.import.is_some());
    assert!(host.iter().any(|e| matches!(e, Effect::ReadClipboard(_))));
    h.send(ImportMessage::Close);
    let host = h.send(Action::OpenFile);
    assert!(matches!(host.as_slice(), [Effect::PickFile(spec)] if spec.kind == PickKind::OpenPgn));
    let host = h.send(Action::Save);
    assert!(matches!(host.as_slice(), [Effect::PickFile(spec)] if spec.kind == PickKind::SavePgn));
    let host = h.send(Action::SaveAs);
    assert!(matches!(host.as_slice(), [Effect::PickFile(spec)] if spec.kind == PickKind::SavePgn));
    let host = h.send(Action::AppendToPgn);
    assert!(
        matches!(host.as_slice(), [Effect::PickFile(spec)] if spec.kind == PickKind::AppendPgn)
    );
    assert_eq!(
        h.send(Action::CopyFen),
        vec![Effect::WriteClipboard(START_FEN.to_owned())]
    );
    assert!(
        matches!(h.send(Action::CopyPgn).as_slice(), [Effect::WriteClipboard(t)] if t.contains("[Event"))
    );
    assert!(matches!(
        h.send(Action::PasteFen).as_slice(),
        [Effect::ReadClipboard(_)]
    ));
}

#[test]
fn action_open_recent_is_one_based_and_ignores_missing_entries() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_file(dir.path(), "a.pgn", multi_pgn(1).as_bytes());
    let b = write_file(
        dir.path(),
        "b.pgn",
        multi_pgn(1).replace("P0", "Second").as_bytes(),
    );
    let mut h = Harness::new();
    h.state.prefs.files.recent.push(a);
    h.state.prefs.files.recent.push(b);
    // Most recent first: entry 1 is b.pgn.
    h.send(Action::OpenRecent(1));
    assert_eq!(h.tags().white, "Second");
    assert!(h.raw(Action::OpenRecent(9)).is_empty());
}

#[test]
fn front_end_only_actions_do_nothing_in_update() {
    let mut h = long_game_harness();
    let game = h.state.session.game().clone();
    for a in [
        Action::GotoMove,
        Action::Help,
        Action::FullScreen,
        Action::BoardSmaller,
        Action::BoardLarger,
        Action::ToggleGameInfo,
        Action::ShowNotation,
        Action::ShowCommentEditor,
    ] {
        assert!(h.raw(a).is_empty(), "{a:?}");
    }
    assert!(h.state.session.game().same_content(&game));
    assert!(h.state.notice.is_none());
}

#[test]
fn action_quit_goes_through_the_quit_flow() {
    let mut h = Harness::new();
    assert_eq!(h.raw(Action::Quit), vec![Effect::Exit]);
}

#[test]
fn custom_shortcut_loaded_from_preferences_resolves_to_the_same_action() {
    let mut h = Harness::new();
    h.send(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::Shortcut {
            action: "FlipBoard".into(),
            binding: Some("Command+Shift+F".into()),
        },
    ));
    assert_eq!(
        h.state.shortcuts.lookup(&cmd_shift(ch('f'))),
        Some(Action::FlipBoard)
    );
    assert_eq!(h.state.shortcuts.lookup(&cmd(ch('f'))), None);
    // A conflicting choice is reported in the issues, the old key stays.
    h.send(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::Shortcut {
            action: "Save".into(),
            binding: Some("Command+O".into()),
        },
    ));
    assert!(h.state.prefs_issues.iter().any(|i| matches!(
        i,
        mychess_app::PrefsIssue::Shortcut(ShortcutIssue::Conflict { .. })
    )));
    assert_eq!(h.state.shortcuts.lookup(&cmd(ch('s'))), Some(Action::Save));
    // Removing the stored choice and fixing it clears the old complaint.
    h.send(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::Shortcut {
            action: "Save".into(),
            binding: None,
        },
    ));
    assert!(
        !h.state
            .prefs_issues
            .iter()
            .any(|i| matches!(i, mychess_app::PrefsIssue::Shortcut(_)))
    );
}
