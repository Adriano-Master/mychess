//! M9-1 (app): the `general.accent` preference (spec
//! `docs/specs/m9-1-cor-de-realce.md`, section 3, D1600, D1602, D1608 and
//! criterion 2). Contract tests; no I/O.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use mychess_app::{
    AccentPref, AppState, Effect, LanguagePref, Message, PrefChange, Preferences, PrefsIssue,
    PrefsMessage, ThemePref, load_preferences, render_preferences, update,
};

const IDS: [(&str, AccentPref); 9] = [
    ("mychess", AccentPref::Mychess),
    ("blue", AccentPref::Blue),
    ("purple", AccentPref::Purple),
    ("pink", AccentPref::Pink),
    ("red", AccentPref::Red),
    ("orange", AccentPref::Orange),
    ("yellow", AccentPref::Yellow),
    ("green", AccentPref::Green),
    ("gray", AccentPref::Gray),
];

#[test]
fn the_default_is_the_mychess_green() {
    assert_eq!(Preferences::default().general.accent, AccentPref::Mychess);
}

#[test]
fn the_ids_of_the_table_are_the_toml_spellings() {
    for (id, value) in IDS {
        assert_eq!(value.as_str(), id);
        assert_eq!(AccentPref::from_name(id), Some(value));
    }
    assert_eq!(AccentPref::from_name("teal"), None);
    assert_eq!(AccentPref::from_name(""), None);
    assert_eq!(AccentPref::from_name("Blue"), None);
}

#[test]
fn every_id_round_trips_through_the_toml() {
    for (id, value) in IDS {
        let mut prefs = Preferences::default();
        prefs.general.accent = value;
        let text = render_preferences(&prefs);
        assert!(text.contains(&format!("accent = \"{id}\"")), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.general.accent, value);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(loaded.prefs, prefs);
    }
}

#[test]
fn a_missing_key_gives_the_default_without_a_remark() {
    let loaded = load_preferences("[general]\ntheme = \"dark\"\n");
    assert_eq!(loaded.prefs.general.accent, AccentPref::Mychess);
    assert_eq!(loaded.prefs.general.theme, ThemePref::Dark);
    assert!(loaded.issues.is_empty());
}

#[test]
fn an_unknown_value_gives_the_default_a_remark_and_keeps_the_other_fields() {
    let mut prefs = Preferences::default();
    prefs.general.language = LanguagePref::PtBr;
    prefs.general.theme = ThemePref::Dark;
    prefs.general.accent = AccentPref::Orange;
    let text = render_preferences(&prefs).replace("accent = \"orange\"", "accent = \"teal\"");
    assert!(text.contains("accent = \"teal\""));
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.general.accent, AccentPref::Mychess);
    assert_eq!(loaded.prefs.general.language, LanguagePref::PtBr);
    assert_eq!(loaded.prefs.general.theme, ThemePref::Dark);
    assert!(
        loaded.issues.iter().any(|i| matches!(
            i,
            PrefsIssue::InvalidValue { key } if key.contains("accent")
        )),
        "{:?}",
        loaded.issues
    );
    // Everything else in the file is untouched.
    let mut expected = prefs.clone();
    expected.general.accent = AccentPref::Mychess;
    assert_eq!(loaded.prefs, expected);
}

#[test]
fn a_value_of_the_wrong_type_is_treated_as_invalid_without_panic() {
    for bad in ["42", "true", "[\"blue\"]", "\"\"", "\"BLUE\""] {
        let loaded = load_preferences(&format!("[general]\naccent = {bad}\ntheme = \"light\"\n"));
        assert_eq!(loaded.prefs.general.accent, AccentPref::Mychess, "{bad}");
        assert_eq!(loaded.prefs.general.theme, ThemePref::Light, "{bad}");
        assert!(!loaded.issues.is_empty(), "{bad}");
    }
}

#[test]
fn the_message_changes_the_preference_and_asks_to_save_at_once() {
    let dir = tempfile::tempdir().unwrap();
    for (_, value) in IDS {
        let mut app = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
        let change = PrefChange::Accent(value);
        assert!(!change.is_debounced());
        let effects = update(&mut app, Message::Prefs(PrefsMessage::Set(change)));
        assert_eq!(app.prefs.general.accent, value);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SavePreferences { .. })),
            "{effects:?}"
        );
    }
}

#[test]
fn choosing_an_accent_leaves_the_theme_preference_alone() {
    let mut app = AppState::default();
    drop(update(
        &mut app,
        Message::Prefs(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark))),
    ));
    drop(update(
        &mut app,
        Message::Prefs(PrefsMessage::Set(PrefChange::Accent(AccentPref::Red))),
    ));
    assert_eq!(app.prefs.general.theme, ThemePref::Dark);
    assert_eq!(app.prefs.general.accent, AccentPref::Red);
}
