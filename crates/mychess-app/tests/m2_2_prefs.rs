//! Preferences in TOML and the save/quit flow (spec M2-2, rules 52-58,
//! D142-D147).
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

use std::path::{Path, PathBuf};

use m2_2_common::*;
use mychess_app::{
    AUTOPLAY_DELAY_RANGE_MS, BoardPrefs, CoordinatesPref, DiscardChoice, EncodingPref,
    FileErrorKind, LanguagePref, LoadedPreferences, MAX_ANIMATION_MS, MIN_WINDOW_HEIGHT,
    MIN_WINDOW_WIDTH, NagStyle, PREFERENCES_VERSION, PrefChange, Preferences, PrefsIssue,
    PrefsMessage, ThemePref, WindowGeometry, load_preferences, preferences_path,
    render_preferences,
};
use proptest::prelude::*;
use rstest::rstest;

fn load(text: &str) -> LoadedPreferences {
    load_preferences(text)
}

fn invalid(key: &str) -> PrefsIssue {
    PrefsIssue::InvalidValue {
        key: key.to_owned(),
    }
}

fn clamped(key: &str) -> PrefsIssue {
    PrefsIssue::Clamped {
        key: key.to_owned(),
    }
}

// ------------------------------------------------------------------ rule 52

#[test]
fn rule52_empty_text_gives_the_defaults_without_issues() {
    let l = load("");
    assert_eq!(l.prefs, Preferences::default());
    assert!(l.issues.is_empty());
    assert_eq!(l.prefs.version, PREFERENCES_VERSION);
}

#[test]
fn d146_defaults_follow_the_spec_table() {
    let p = Preferences::default();
    assert_eq!(PREFERENCES_VERSION, 1);
    assert_eq!(p.general.language, LanguagePref::System);
    assert_eq!(p.general.theme, ThemePref::Dark);
    assert_eq!(
        p.board,
        BoardPrefs {
            theme: "mychess-green".into(),
            piece_set: "cburnett".into(),
            coordinates: CoordinatesPref::Inside,
            show_side_to_move: true,
            highlight_last_move: true,
            animation_ms: 200,
        }
    );
    assert_eq!(p.playback.autoplay_delay_ms, 5000);
    assert_eq!(
        (p.window.width, p.window.height, p.window.x, p.window.y),
        (None, None, None, None)
    );
    assert!(!p.window.maximized);
    assert!(p.files.recent.is_empty());
    assert_eq!(
        (
            &p.files.last_open_dir,
            &p.files.last_save_dir,
            &p.files.eco_file
        ),
        (&None, &None, &None)
    );
    // Strict export (D132): numeric NAGs, no indentation, 80 columns, UTF-8.
    assert_eq!(p.pgn_export.nags, NagStyle::Numeric);
    assert!(!p.pgn_export.indent_variations && !p.pgn_export.indent_comments);
    assert_eq!(p.pgn_export.encoding, EncodingPref::Utf8);
    assert_eq!(p.pgn_export.width, 80);
    assert!(!p.game_info.hide_next_move && !p.game_info.full_comment && !p.game_info.show_fen);
    assert!(p.game_info.show_material);
    assert!(p.shortcuts.is_empty());
}

#[test]
fn d143_rendered_file_uses_the_documented_sections_and_keys() {
    let mut p = Preferences::default();
    p.window.width = Some(800);
    p.window.height = Some(600);
    p.window.x = Some(-10);
    p.window.y = Some(20);
    p.files.recent.push(PathBuf::from("a.pgn"));
    p.files.last_open_dir = Some(PathBuf::from("o"));
    p.files.last_save_dir = Some(PathBuf::from("s"));
    p.files.eco_file = Some(PathBuf::from("scid.eco"));
    p.shortcuts
        .insert("FlipBoard".into(), "Command+Shift+F".into());
    let text = render_preferences(&p);
    let doc: toml::Table = text.parse().unwrap();
    assert_eq!(doc["version"].as_integer(), Some(1));
    for (section, keys) in [
        ("general", &["language", "theme"][..]),
        (
            "board",
            &[
                "theme",
                "piece_set",
                "coordinates",
                "show_side_to_move",
                "highlight_last_move",
                "animation_ms",
            ],
        ),
        ("playback", &["autoplay_delay_ms"]),
        ("window", &["width", "height", "x", "y", "maximized"]),
        (
            "files",
            &["recent", "last_open_dir", "last_save_dir", "eco_file"],
        ),
        (
            "pgn_export",
            &[
                "nags",
                "indent_variations",
                "indent_comments",
                "strip_marks",
                "encoding",
                "width",
            ],
        ),
        (
            "game_info",
            &[
                "hide_next_move",
                "full_comment",
                "show_fen",
                "show_material",
            ],
        ),
        ("shortcuts", &["FlipBoard"]),
    ] {
        let table = doc
            .get(section)
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("[{section}] missing in:\n{text}"));
        for key in keys {
            assert!(
                table.contains_key(*key),
                "{section}.{key} missing in:\n{text}"
            );
        }
    }
    assert_eq!(doc["general"]["language"].as_str(), Some("system"));
    assert_eq!(doc["board"]["coordinates"].as_str(), Some("inside"));
    assert_eq!(doc["board"]["theme"].as_str(), Some("mychess-green"));
    assert_eq!(doc["board"]["piece_set"].as_str(), Some("cburnett"));
}

#[test]
fn d143_documented_keys_and_values_are_read() {
    let text = r#"
version = 1
[general]
language = "pt-BR"
theme = "dark"
[board]
theme = "walnut"
piece_set = "staunty"
coordinates = "inside"
show_side_to_move = false
highlight_last_move = false
animation_ms = 0
[playback]
autoplay_delay_ms = 1500
[window]
width = 800
height = 600
x = -20
y = 30
maximized = true
[files]
recent = ["one.pgn", "two with space.pgn"]
last_open_dir = "open"
last_save_dir = "save"
eco_file = "scid.eco"
[game_info]
hide_next_move = true
full_comment = true
show_fen = true
show_material = false
[shortcuts]
FlipBoard = "Command+Shift+F"
Quit = ""
"#;
    let l = load(text);
    assert!(l.issues.is_empty(), "{:?}", l.issues);
    let p = l.prefs;
    assert_eq!(
        (p.general.language, p.general.theme),
        (LanguagePref::PtBr, ThemePref::Dark)
    );
    assert_eq!(
        (p.board.theme.as_str(), p.board.piece_set.as_str()),
        ("walnut", "staunty")
    );
    assert_eq!(p.board.coordinates, CoordinatesPref::Inside);
    assert!(!p.board.show_side_to_move && !p.board.highlight_last_move);
    assert_eq!(p.board.animation_ms, 0);
    assert_eq!(p.playback.autoplay_delay_ms, 1500);
    assert_eq!(
        (p.window.width, p.window.height, p.window.x, p.window.y),
        (Some(800), Some(600), Some(-20), Some(30))
    );
    assert!(p.window.maximized);
    assert_eq!(
        p.files.recent.iter().cloned().collect::<Vec<_>>(),
        vec![
            PathBuf::from("one.pgn"),
            PathBuf::from("two with space.pgn")
        ]
    );
    assert_eq!(p.files.last_open_dir, Some(PathBuf::from("open")));
    assert_eq!(p.files.last_save_dir, Some(PathBuf::from("save")));
    assert_eq!(p.files.eco_file, Some(PathBuf::from("scid.eco")));
    assert!(p.game_info.hide_next_move && p.game_info.full_comment && p.game_info.show_fen);
    assert!(!p.game_info.show_material);
    assert_eq!(
        p.shortcuts.get("FlipBoard").map(String::as_str),
        Some("Command+Shift+F")
    );
    assert_eq!(p.shortcuts.get("Quit").map(String::as_str), Some(""));
}

fn arb_path() -> impl Strategy<Value = PathBuf> {
    "[A-Za-z0-9 \u{e9}\u{e3}_.-]{1,12}".prop_map(PathBuf::from)
}

fn arb_prefs() -> impl Strategy<Value = Preferences> {
    let general = (0usize..3, 0usize..3);
    let board = (
        "[a-z0-9_-]{1,10}",
        "[a-z0-9_-]{1,10}",
        0usize..3,
        any::<bool>(),
        any::<bool>(),
        0u32..=MAX_ANIMATION_MS,
    );
    let window = (
        proptest::option::of(MIN_WINDOW_WIDTH..4000),
        proptest::option::of(MIN_WINDOW_HEIGHT..3000),
        proptest::option::of(-3000i32..3000),
        proptest::option::of(-3000i32..3000),
        any::<bool>(),
    );
    let files = (
        proptest::collection::vec(arb_path(), 0..12),
        proptest::option::of(arb_path()),
        proptest::option::of(arb_path()),
        proptest::option::of(arb_path()),
    );
    let export = (
        0usize..3,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        0usize..3,
        20u16..=500,
    );
    let info = (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>());
    let shortcuts = proptest::collection::btree_map("[A-Za-z]{1,12}", "[A-Za-z+]{0,12}", 0..5);
    (
        general,
        board,
        AUTOPLAY_DELAY_RANGE_MS.0..=AUTOPLAY_DELAY_RANGE_MS.1,
        window,
        files,
        export,
        info,
        shortcuts,
    )
        .prop_map(|(g, b, a, w, f, e, i, s)| {
            let mut p = Preferences::default();
            p.general.language =
                [LanguagePref::System, LanguagePref::EnUs, LanguagePref::PtBr][g.0];
            p.general.theme = [ThemePref::System, ThemePref::Light, ThemePref::Dark][g.1];
            p.board.theme = b.0;
            p.board.piece_set = b.1;
            p.board.coordinates = [
                CoordinatesPref::None,
                CoordinatesPref::Outside,
                CoordinatesPref::Inside,
            ][b.2];
            p.board.show_side_to_move = b.3;
            p.board.highlight_last_move = b.4;
            p.board.animation_ms = b.5;
            p.playback.autoplay_delay_ms = a;
            (
                p.window.width,
                p.window.height,
                p.window.x,
                p.window.y,
                p.window.maximized,
            ) = w;
            for path in f.0.into_iter().rev() {
                p.files.recent.push(path);
            }
            p.files.last_open_dir = f.1;
            p.files.last_save_dir = f.2;
            p.files.eco_file = f.3;
            p.pgn_export.nags = [NagStyle::Numeric, NagStyle::Symbolic, NagStyle::None][e.0];
            p.pgn_export.indent_variations = e.1;
            p.pgn_export.indent_comments = e.2;
            p.pgn_export.strip_marks = e.3;
            p.pgn_export.encoding = [
                EncodingPref::Utf8,
                EncodingPref::Utf8Bom,
                EncodingPref::Latin1,
            ][e.4];
            p.pgn_export.width = e.5;
            p.game_info.hide_next_move = i.0;
            p.game_info.full_comment = i.1;
            p.game_info.show_fen = i.2;
            p.game_info.show_material = i.3;
            p.shortcuts = s;
            p
        })
}

proptest! {
    #[test]
    fn rule52_render_then_load_is_the_identity(p in arb_prefs()) {
        let text = render_preferences(&p);
        let loaded = load_preferences(&text);
        prop_assert!(loaded.issues.is_empty(), "{:?}\n{}", loaded.issues, text);
        prop_assert_eq!(loaded.prefs, p);
    }
}

#[test]
fn rule52_default_render_round_trips() {
    let p = Preferences::default();
    assert_eq!(load(&render_preferences(&p)).prefs, p);
}

// ------------------------------------------------------------------ rule 53

#[test]
fn rule53_missing_fields_are_defaults_and_unknown_keys_and_sections_are_ignored_silently() {
    let l = load(
        "version = 1\nfuture_flag = true\n[general]\ntheme = \"dark\"\nfuture_key = [1, 2]\n[brand_new_section]\nx = 1\n[brand_new_section.sub]\ny = \"z\"\n",
    );
    assert!(l.issues.is_empty(), "{:?}", l.issues);
    assert_eq!(l.prefs.general.theme, ThemePref::Dark);
    assert_eq!(l.prefs.general.language, LanguagePref::System);
    assert_eq!(l.prefs.board, BoardPrefs::default());
}

#[test]
fn rule53_a_bad_value_only_resets_its_own_field() {
    let l = load(
        "[board]\nanimation_ms = \"r\u{e1}pido\"\ntheme = \"walnut\"\n[playback]\nautoplay_delay_ms = 1234\n",
    );
    assert_eq!(l.issues, vec![invalid("board.animation_ms")]);
    assert_eq!(l.prefs.board.animation_ms, 200);
    assert_eq!(l.prefs.board.theme, "walnut", "neighbours survive");
    assert_eq!(l.prefs.playback.autoplay_delay_ms, 1234);
}

#[test]
fn rule53_autoplay_delay_below_the_minimum_is_clamped_to_100() {
    let l = load("[playback]\nautoplay_delay_ms = 5\n");
    assert_eq!(l.prefs.playback.autoplay_delay_ms, 100);
    assert_eq!(l.issues, vec![clamped("playback.autoplay_delay_ms")]);
}

#[test]
fn rule53_autoplay_delay_above_300_seconds_is_clamped() {
    let l = load("[playback]\nautoplay_delay_ms = 999999999\n");
    assert_eq!(l.prefs.playback.autoplay_delay_ms, 300_000);
    assert_eq!(l.issues, vec![clamped("playback.autoplay_delay_ms")]);
}

#[test]
fn rule53_window_size_below_the_minimum_is_clamped() {
    let l = load("[window]\nwidth = 10\nheight = 20\n");
    assert_eq!(l.prefs.window.width, Some(320));
    assert_eq!(l.prefs.window.height, Some(240));
    assert_eq!(
        l.issues,
        vec![clamped("window.width"), clamped("window.height")]
    );
}

#[test]
fn rule53_animation_above_2000_is_clamped() {
    let l = load("[board]\nanimation_ms = 5000\n");
    assert_eq!(l.prefs.board.animation_ms, 2000);
    assert_eq!(l.issues, vec![clamped("board.animation_ms")]);
}

#[rstest]
#[case("[general]\nlanguage = \"klingon\"", "general.language")]
#[case("[general]\nlanguage = 3", "general.language")]
#[case("[general]\ntheme = true", "general.theme")]
#[case("[board]\ncoordinates = \"diagonal\"", "board.coordinates")]
#[case("[board]\nshow_side_to_move = \"yes\"", "board.show_side_to_move")]
#[case("[board]\nhighlight_last_move = 1", "board.highlight_last_move")]
#[case("[board]\ntheme = 5", "board.theme")]
#[case("[board]\npiece_set = [1]", "board.piece_set")]
#[case("[board]\nanimation_ms = 1.5", "board.animation_ms")]
#[case(
    "[playback]\nautoplay_delay_ms = \"slow\"",
    "playback.autoplay_delay_ms"
)]
#[case("[window]\nmaximized = \"si\"", "window.maximized")]
#[case("[window]\nx = \"left\"", "window.x")]
#[case("[game_info]\nshow_fen = 0", "game_info.show_fen")]
#[case("[game_info]\nshow_material = \"on\"", "game_info.show_material")]
#[case("[files]\nlast_open_dir = 7", "files.last_open_dir")]
#[case("[files]\neco_file = false", "files.eco_file")]
fn rule53_wrong_type_or_unknown_choice_resets_only_that_field(
    #[case] text: &str,
    #[case] key: &str,
) {
    let l = load(text);
    assert!(l.issues.contains(&invalid(key)), "{key}: {:?}", l.issues);
    // Everything else is untouched.
    let mut expected = Preferences::default();
    // Compare the sections that the case does not touch.
    let section = key.split('.').next().unwrap();
    if section != "general" {
        assert_eq!(l.prefs.general, expected.general);
    }
    if section != "board" {
        assert_eq!(l.prefs.board, expected.board);
    }
    expected.version = l.prefs.version;
    if section != "game_info" {
        assert_eq!(l.prefs.game_info, expected.game_info);
    }
    assert_eq!(l.prefs.shortcuts, expected.shortcuts);
}

#[test]
fn rule53_wrong_section_types_do_not_panic_and_fall_back_to_defaults() {
    for text in [
        "general = 5",
        "board = \"x\"",
        "files = [1, 2]",
        "window = true",
        "shortcuts = 3",
        "game_info = []",
        "pgn_export = 1.5",
        "playback = \"fast\"",
    ] {
        let l = load(text);
        assert_eq!(l.prefs, Preferences::default(), "{text}");
        assert!(
            !l.issues.is_empty(),
            "{text}: a section of the wrong type is reported"
        );
    }
}

#[test]
fn rule53_recent_files_tolerate_bad_entries_and_cap_at_ten() {
    let many: Vec<String> = (0..25).map(|i| format!("\"f{i}.pgn\"")).collect();
    let l = load(&format!("[files]\nrecent = [{}]\n", many.join(", ")));
    assert!(l.prefs.files.recent.len() <= 10);
    assert_eq!(
        l.prefs.files.recent.get(0),
        Some(&PathBuf::from("f0.pgn")),
        "the order of the file is kept"
    );
    let l = load("[files]\nrecent = [\"a.pgn\", 5, \"a.pgn\", \"\", \"b.pgn\"]\n");
    let names: Vec<_> = l.prefs.files.recent.iter().cloned().collect();
    assert!(names.contains(&PathBuf::from("a.pgn")) && names.contains(&PathBuf::from("b.pgn")));
    assert_eq!(
        names
            .iter()
            .filter(|p| p.as_path() == Path::new("a.pgn"))
            .count(),
        1,
        "no duplicates"
    );
    let l = load("[files]\nrecent = \"not a list\"\n");
    assert!(l.prefs.files.recent.is_empty());
    assert!(!l.issues.is_empty());
}

#[test]
fn rule53_shortcuts_with_non_string_values_are_reported() {
    let l = load("[shortcuts]\nFlipBoard = 5\nSave = \"Command+K\"\n");
    assert_eq!(
        l.prefs.shortcuts.get("Save").map(String::as_str),
        Some("Command+K")
    );
    assert!(!l.prefs.shortcuts.contains_key("FlipBoard"));
    assert!(!l.issues.is_empty());
}

#[test]
fn rule53_integers_beyond_the_range_of_the_target_type_never_overflow() {
    let l = load(
        "[window]\nwidth = 9223372036854775807\nx = 9223372036854775807\ny = -9223372036854775808\n[board]\nanimation_ms = -9223372036854775808\n",
    );
    assert!(l.prefs.window.width.is_some());
    assert_eq!(l.prefs.board.animation_ms, 0);
    assert!(!l.issues.is_empty());
}

// ------------------------------------------------------------------ rule 54

#[test]
fn rule54_missing_version_is_migrated_to_1_keeping_known_fields() {
    let l = load("[general]\ntheme = \"light\"\n[playback]\nautoplay_delay_ms = 2500\n");
    assert_eq!(l.prefs.version, 1);
    assert_eq!(l.prefs.general.theme, ThemePref::Light);
    assert_eq!(l.prefs.playback.autoplay_delay_ms, 2500);
    assert!(l.issues.is_empty(), "{:?}", l.issues);
}

#[test]
fn rule54_version_0_is_migrated_too() {
    let l = load("version = 0\n[general]\ntheme = \"dark\"\n");
    assert_eq!(l.prefs.version, 1);
    assert_eq!(l.prefs.general.theme, ThemePref::Dark);
    assert!(l.issues.is_empty());
}

#[test]
fn rule54_a_newer_version_reads_what_it_understands_and_reports_it() {
    let l = load("version = 99\n[general]\ntheme = \"dark\"\nsparkle = 1\n[sparkly]\nz = 2\n");
    assert!(
        l.issues.contains(&PrefsIssue::NewerVersion { found: 99 }),
        "{:?}",
        l.issues
    );
    assert_eq!(l.prefs.general.theme, ThemePref::Dark);
    assert_eq!(l.prefs.version, 1, "what we write is our version");
}

#[rstest]
#[case("version = \"one\"")]
#[case("version = -3")]
#[case("version = 1.5")]
#[case("version = [1]")]
#[case("version = 9223372036854775807")]
fn rule54_weird_version_values_do_not_panic(#[case] text: &str) {
    let l = load(text);
    assert_eq!(l.prefs.version, 1);
}

#[test]
fn rule54_the_backup_is_made_before_the_next_save_and_is_not_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let original = "version = 99\n[general]\ntheme = \"dark\"\n[future]\nkeep = \"me\"\n";
    std::fs::write(&path, original).unwrap();
    let h = Harness::new();
    let Message::PreferencesLoaded(loaded) =
        h.perform(Effect::LoadPreferences { path: path.clone() })
    else {
        panic!()
    };
    assert!(
        loaded
            .issues
            .contains(&PrefsIssue::NewerVersion { found: 99 })
    );
    let saved = h.perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::new(loaded.prefs.clone()),
    });
    assert_eq!(saved, Message::PreferencesSaved(Ok(())));
    let backup = dir.path().join("preferences.toml.bak-v99");
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        original,
        "backup holds the old bytes"
    );
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("version = 1")
    );

    // A second time with a v99 file again: the existing backup is kept.
    std::fs::write(&backup, "marker").unwrap();
    std::fs::write(&path, "version = 99\n").unwrap();
    h.perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::new(loaded.prefs),
    });
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "marker");
    assert_eq!(
        dir_names(dir.path()),
        vec!["preferences.toml", "preferences.toml.bak-v99"]
    );
}

#[test]
fn rule54_no_backup_is_made_for_current_or_older_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::write(&path, "version = 1\n").unwrap();
    let h = Harness::new();
    h.perform(Effect::SavePreferences {
        path,
        prefs: Box::default(),
    });
    assert_eq!(dir_names(dir.path()), vec!["preferences.toml"]);
}

// ------------------------------------------------------------------ rule 55

#[rstest]
#[case::syntax(b"= = broken".to_vec())]
#[case::unclosed_table(b"[general\ntheme = 1".to_vec())]
#[case::binary(vec![0xff, 0xfe, 0x00, 0x01, 0x80])]
#[case::invalid_utf8_inside(b"[general]\ntheme = \"\xff\"\n".to_vec())]
#[case::duplicate_key(b"a = 1\na = 2\n".to_vec())]
fn rule55_unreadable_files_are_quarantined_and_defaults_are_returned(#[case] bytes: Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "preferences.toml", &bytes);
    let h = Harness::new();
    let Message::PreferencesLoaded(l) = h.perform(Effect::LoadPreferences { path: path.clone() })
    else {
        panic!()
    };
    assert_eq!(l.prefs, Preferences::default());
    assert!(l.issues.contains(&PrefsIssue::Unreadable), "{:?}", l.issues);
    assert!(!path.exists(), "the corrupt file was moved away");
    assert_eq!(
        std::fs::read(dir.path().join("preferences.toml.corrupt")).unwrap(),
        bytes
    );
}

#[test]
fn rule55_an_older_corrupt_copy_is_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "preferences.toml", b"first = = bad");
    let h = Harness::new();
    h.perform(Effect::LoadPreferences { path: path.clone() });
    std::fs::write(&path, b"second = = bad").unwrap();
    h.perform(Effect::LoadPreferences { path });
    assert_eq!(
        std::fs::read(dir.path().join("preferences.toml.corrupt")).unwrap(),
        b"second = = bad"
    );
}

#[test]
fn rule55_missing_and_empty_files_are_not_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let h = Harness::new();
    let path = dir.path().join("preferences.toml");
    let Message::PreferencesLoaded(l) = h.perform(Effect::LoadPreferences { path: path.clone() })
    else {
        panic!()
    };
    assert_eq!(l, LoadedPreferences::default());
    std::fs::write(&path, b"").unwrap();
    let Message::PreferencesLoaded(l) = h.perform(Effect::LoadPreferences { path: path.clone() })
    else {
        panic!()
    };
    assert!(l.issues.is_empty());
    assert!(path.exists());
    assert_eq!(dir_names(dir.path()), vec!["preferences.toml"]);
}

#[test]
fn rule55_a_directory_in_place_of_the_file_is_reported_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::create_dir(&path).unwrap();
    let h = Harness::new();
    let Message::PreferencesLoaded(l) = h.perform(Effect::LoadPreferences { path: path.clone() })
    else {
        panic!()
    };
    assert_eq!(l.prefs, Preferences::default());
    assert!(l.issues.contains(&PrefsIssue::Unreadable));
    assert!(path.is_dir());
}

#[test]
fn rule55_bom_and_crlf_in_the_file_are_accepted_or_quarantined_never_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let text = "version = 1\r\n[general]\r\ntheme = \"dark\"\r\n";
    let path = write_file(dir.path(), "preferences.toml", text.as_bytes());
    let h = Harness::new();
    let Message::PreferencesLoaded(l) = h.perform(Effect::LoadPreferences { path }) else {
        panic!()
    };
    assert_eq!(l.prefs.general.theme, ThemePref::Dark, "CRLF is plain TOML");
    assert!(l.issues.is_empty());
}

// ------------------------------------------------------------------ rule 56

#[test]
fn rule56_save_creates_the_directory_and_a_second_save_replaces_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("deep")
        .join("er dir")
        .join("preferences.toml");
    let h = Harness::new();
    let mut p = Preferences::default();
    p.general.theme = ThemePref::Dark;
    assert_eq!(
        h.perform(Effect::SavePreferences {
            path: path.clone(),
            prefs: Box::new(p.clone())
        }),
        Message::PreferencesSaved(Ok(()))
    );
    p.general.theme = ThemePref::Light;
    h.perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::new(p.clone()),
    });
    assert_eq!(load(&std::fs::read_to_string(&path).unwrap()).prefs, p);
    assert_eq!(
        dir_names(path.parent().unwrap()),
        vec!["preferences.toml"],
        "no temporary left"
    );
}

#[test]
fn rule56_saved_text_is_utf8_without_bom_and_lf() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let mut p = Preferences::default();
    p.board.theme = "caf\u{e9}".into();
    Harness::new().perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::new(p),
    });
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
    assert!(!bytes.contains(&b'\r'));
    assert!(String::from_utf8(bytes).unwrap().contains("caf\u{e9}"));
}

#[test]
fn rule56_failure_gives_an_error_message_and_leaves_the_old_file_and_no_temporaries() {
    let dir = tempfile::tempdir().unwrap();
    // The "directory" of the preferences is a regular file.
    let blocker = write_file(dir.path(), "blocker", b"x");
    let path = blocker.join("preferences.toml");
    let h = Harness::new();
    let m = h.perform(Effect::SavePreferences {
        path,
        prefs: Box::default(),
    });
    let Message::PreferencesSaved(Err(e)) = m else {
        panic!("{m:?}")
    };
    assert!(
        matches!(
            e.kind,
            FileErrorKind::Io(_)
                | FileErrorKind::NotFound
                | FileErrorKind::PermissionDenied
                | FileErrorKind::AlreadyExists
        ),
        "{e:?}"
    );
    assert_eq!(dir_names(dir.path()), vec!["blocker"]);
    assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
}

#[test]
fn rule56_target_that_is_a_directory_keeps_it_and_leaves_no_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::create_dir(&path).unwrap();
    let m = Harness::new().perform(Effect::SavePreferences {
        path,
        prefs: Box::default(),
    });
    assert!(matches!(m, Message::PreferencesSaved(Err(_))), "{m:?}");
    assert_eq!(dir_names(dir.path()), vec!["preferences.toml"]);
}

#[cfg(unix)]
#[test]
fn rule56_read_only_directory_gives_a_failure_and_keeps_the_previous_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "preferences.toml", b"version = 1\n");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable = std::fs::write(dir.path().join("probe"), b"x").is_ok();
    let m = Harness::new().perform(Effect::SavePreferences {
        path: path.clone(),
        prefs: Box::default(),
    });
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return; // running as root
    }
    assert!(
        matches!(m, Message::PreferencesSaved(Err(e)) if e.kind == FileErrorKind::PermissionDenied)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"version = 1\n");
    assert_eq!(dir_names(dir.path()), vec!["preferences.toml"]);
}

#[test]
fn rule56_a_failed_save_becomes_a_save_failed_issue_and_the_app_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = write_file(dir.path(), "blocker", b"x");
    let mut h = Harness::with_prefs_path(Some(blocker.join("preferences.toml")));
    h.send(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    assert!(
        h.state
            .prefs_issues
            .iter()
            .any(|i| matches!(i, PrefsIssue::SaveFailed(_))),
        "{:?}",
        h.state.prefs_issues
    );
    assert_eq!(
        h.state.prefs.general.theme,
        ThemePref::Dark,
        "the change stays in memory"
    );
    h.play("e2e4");
    assert_eq!(h.plies(), 1);
}

// Concurrency / debounce (rule 56, 57, D145)

fn prefs_state(dir: &Path) -> (Harness, PathBuf) {
    let path = dir.join("preferences.toml");
    (Harness::with_prefs_path(Some(path.clone())), path)
}

fn saves(effects: &[Effect]) -> Vec<&Effect> {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::SavePreferences { .. }))
        .collect()
}

#[test]
fn rule56_never_two_saves_at_once_and_the_follow_up_has_the_latest_state() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, path) = prefs_state(dir.path());
    let first = h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    assert_eq!(saves(&first).len(), 1);
    // While the first is in flight, two more changes arrive: no effects.
    assert!(
        h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Light)))
            .is_empty()
    );
    assert!(
        h.raw(PrefsMessage::Set(PrefChange::AnimationMs(77)))
            .is_empty()
    );
    // The first finishes: exactly one new save, with the newest state.
    let done = h.perform(first.into_iter().next().unwrap());
    assert_eq!(done, Message::PreferencesSaved(Ok(())));
    let follow = h.raw(done);
    let [Effect::SavePreferences { prefs, .. }] = follow.as_slice() else {
        panic!("{follow:?}")
    };
    assert_eq!(prefs.general.theme, ThemePref::Light);
    assert_eq!(prefs.board.animation_ms, 77);
    // While that one is in flight there is still only one at a time.
    assert!(
        h.raw(PrefsMessage::Set(PrefChange::AnimationMs(88)))
            .is_empty()
    );
    let done = h.perform(follow.into_iter().next().unwrap());
    let last = h.raw(done);
    let [Effect::SavePreferences { prefs, .. }] = last.as_slice() else {
        panic!("{last:?}")
    };
    assert_eq!(prefs.board.animation_ms, 88);
    let done = h.perform(last.into_iter().next().unwrap());
    assert!(h.raw(done).is_empty(), "nothing pending: the chain stops");
    assert_eq!(
        load(&std::fs::read_to_string(path).unwrap())
            .prefs
            .board
            .animation_ms,
        88
    );
}

#[test]
fn rule56_after_the_chain_ends_a_new_change_saves_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = prefs_state(dir.path());
    h.send(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    let eff = h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Light)));
    assert_eq!(saves(&eff).len(), 1);
}

#[test]
fn rule57_window_geometry_only_marks_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = prefs_state(dir.path());
    let g = WindowGeometry {
        width: 900,
        height: 700,
        x: Some(10),
        y: None,
        maximized: false,
    };
    for _ in 0..200 {
        let eff = h.raw(Message::WindowGeometryChanged(g));
        assert!(eff.is_empty(), "resizing never saves by itself");
    }
    assert!(h.state.prefs_dirty);
    assert_eq!(
        (h.state.prefs.window.width, h.state.prefs.window.height),
        (Some(900), Some(700))
    );
    assert_eq!(h.state.prefs.window.x, Some(10));
    assert_eq!(h.state.prefs.window.y, None);
}

#[test]
fn rule57_flush_emits_one_save_when_dirty_and_nothing_when_clean() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, path) = prefs_state(dir.path());
    assert!(
        h.raw(Message::FlushPreferences).is_empty(),
        "clean: nothing to do"
    );
    h.raw(Message::WindowGeometryChanged(WindowGeometry {
        width: 800,
        height: 600,
        x: None,
        y: None,
        maximized: true,
    }));
    let eff = h.raw(Message::FlushPreferences);
    assert_eq!(saves(&eff).len(), 1);
    assert!(!h.state.prefs_dirty);
    assert!(h.raw(Message::FlushPreferences).is_empty(), "once");
    let done = h.perform(eff.into_iter().next().unwrap());
    h.raw(done);
    let saved = load(&std::fs::read_to_string(path).unwrap()).prefs;
    assert_eq!(saved.window.width, Some(800));
    assert!(saved.window.maximized);
}

#[test]
fn rule57_dialog_changes_save_at_once_and_layout_is_debounced() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = prefs_state(dir.path());
    let eff = h.raw(PrefsMessage::Set(PrefChange::Language(LanguagePref::PtBr)));
    assert_eq!(saves(&eff).len(), 1);
    let dir2 = tempfile::tempdir().unwrap();
    let (mut h2, _) = prefs_state(dir2.path());
    let eff = h2.raw(PrefsMessage::Set(PrefChange::Layout(None)));
    assert!(eff.is_empty());
    assert!(h2.state.prefs_dirty);
}

#[test]
fn rule57_recent_files_and_dirs_only_mark_dirty_until_flushed() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_file(dir.path(), "g.pgn", multi_pgn(1).as_bytes());
    let (mut h, prefs_path) = prefs_state(&dir.path().join("cfg"));
    h.send(Command::Open);
    let eff = h.pick(Some(&pgn));
    assert!(eff.is_empty());
    assert!(!prefs_path.exists(), "no write yet");
    assert!(h.state.prefs_dirty);
    h.send(Message::FlushPreferences);
    let saved = load(&std::fs::read_to_string(&prefs_path).unwrap()).prefs;
    assert_eq!(saved.files.recent.len(), 1);
    assert!(saved.files.last_open_dir.is_some());
}

#[test]
fn rule57_quit_saves_before_exiting_and_exits_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, path) = prefs_state(dir.path());
    let eff = h.raw(Message::Quit);
    assert_eq!(saves(&eff).len(), 1, "{eff:?}");
    assert!(
        !eff.contains(&Effect::Exit),
        "exit only after the save: {eff:?}"
    );
    let done = h.perform(eff.into_iter().next().unwrap());
    let after = h.raw(done);
    assert_eq!(after, vec![Effect::Exit]);
    assert!(path.exists());
}

#[test]
fn rule57_quit_with_a_save_in_flight_waits_and_saves_the_latest() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = prefs_state(dir.path());
    let first = h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Light)));
    let eff = h.raw(Message::Quit);
    assert!(
        eff.is_empty(),
        "a save is in flight: nothing at once ({eff:?})"
    );
    let mut effects = h.raw(h.perform(first.into_iter().next().unwrap()));
    // The follow-up save, then the exit.
    let mut guard = 0;
    while !effects.contains(&Effect::Exit) {
        let e = effects.pop().expect("an effect");
        assert!(matches!(e, Effect::SavePreferences { .. }), "{e:?}");
        let done = h.perform(e);
        effects = h.raw(done);
        guard += 1;
        assert!(guard < 5);
    }
    assert_eq!(effects.iter().filter(|e| **e == Effect::Exit).count(), 1);
}

#[test]
fn rule57_quit_exits_even_if_the_final_save_fails() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = write_file(dir.path(), "b", b"x");
    let mut h = Harness::with_prefs_path(Some(blocker.join("p.toml")));
    let host = h.send(Message::Quit);
    assert_eq!(host, vec![Effect::Exit]);
}

#[test]
fn rule57_quit_without_a_preferences_path_exits_immediately() {
    let mut h = Harness::with_prefs_path(None);
    assert_eq!(h.raw(Message::Quit), vec![Effect::Exit]);
}

#[test]
fn d103_quit_with_unsaved_changes_asks_first_and_cancel_keeps_the_app_open() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = prefs_state(dir.path());
    h.play("e2e4");
    let host = h.send(Message::Quit);
    assert!(host.is_empty());
    assert!(h.state.discard_prompt.is_some());
    h.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(h.host.iter().all(|e| *e != Effect::Exit));
    h.send(Message::Quit);
    let host = h.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(host, vec![Effect::Exit]);
}

#[test]
fn d142_startup_loads_from_the_path_and_applies_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(
        dir.path(),
        "preferences.toml",
        b"[general]\ntheme = \"dark\"\n[playback]\nautoplay_delay_ms = 2000\n[shortcuts]\nFlipBoard = \"Command+Shift+F\"\n",
    );
    let mut h = Harness::with_prefs_path(Some(path));
    h.send(Message::Startup);
    assert_eq!(h.state.prefs.general.theme, ThemePref::Dark);
    assert_eq!(h.state.prefs.playback.autoplay_delay_ms, 2000);
    assert_eq!(
        h.state.session.autoplay().delay.as_millis(),
        2000,
        "the autoplay delay reaches the session"
    );
    // The custom shortcut is active.
    use mychess_app::{Action, Key, KeyInput, Modifiers};
    let input = KeyInput {
        key: Key::Char('f'),
        modifiers: Modifiers {
            command: true,
            shift: true,
            alt: false,
        },
    };
    assert_eq!(h.state.shortcuts.lookup(&input), Some(Action::FlipBoard));
}

#[test]
fn d142_startup_loads_the_eco_book_from_the_preference() {
    let dir = tempfile::tempdir().unwrap();
    let eco = write_file(dir.path(), "x.eco", b"B20 \"Sicilian\" 1.e4 c5 *\n");
    let text = format!(
        "[files]\neco_file = {}\n",
        toml::Value::String(eco.to_str().unwrap().to_owned())
    );
    let path = write_file(dir.path(), "preferences.toml", text.as_bytes());
    let mut h = Harness::with_prefs_path(Some(path));
    h.send(Message::Startup);
    assert!(h.state.eco_book.is_some());
}

#[test]
fn d142_no_config_dir_runs_with_defaults_and_warns_exactly_once() {
    let mut h = Harness::with_prefs_path(None);
    assert!(h.raw(Message::Startup).is_empty());
    h.raw(Message::Startup);
    h.raw(Message::Quit);
    h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    let n = h
        .state
        .prefs_issues
        .iter()
        .filter(|i| **i == PrefsIssue::NoConfigDir)
        .count();
    assert_eq!(n, 1);
    assert_eq!(
        h.state.prefs.general.theme,
        ThemePref::Dark,
        "changes still apply in memory"
    );
}

#[test]
fn d142_preferences_path_is_a_toml_file_named_preferences() {
    if let Some(p) = preferences_path() {
        assert_eq!(
            p.file_name().and_then(|n| n.to_str()),
            Some("preferences.toml")
        );
        assert!(p.is_absolute());
    }
}

#[test]
fn startup_with_a_corrupt_file_keeps_the_app_usable_and_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_file(dir.path(), "preferences.toml", b"= = =");
    let mut h = Harness::with_prefs_path(Some(path.clone()));
    h.send(Message::Startup);
    assert_eq!(h.state.prefs, Preferences::default());
    assert!(h.state.prefs_issues.contains(&PrefsIssue::Unreadable));
    assert!(dir.path().join("preferences.toml.corrupt").exists());
    h.send(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    assert!(path.exists(), "a fresh file is written next");
    assert!(
        load(&std::fs::read_to_string(&path).unwrap())
            .issues
            .is_empty()
    );
}

// ------------------------------------------------------------------ rule 58

#[test]
fn rule58_recent_files_and_directories_survive_the_round_trip_with_spaces_and_accents() {
    let mut p = Preferences::default();
    for name in [
        "Jos\u{e9} & Mar\u{ed}a.pgn",
        "with space.pgn",
        "caf\u{e9} \"quoted\".pgn",
        "back\\slash.pgn",
    ] {
        p.files.recent.push(Path::new("dir with space").join(name));
    }
    p.files.last_open_dir = Some(Path::new("Z\u{fc}rich").join("open dir"));
    p.files.last_save_dir = Some(PathBuf::from("s a v e"));
    p.files.eco_file = Some(Path::new("data").join("scid ECO.eco"));
    let back = load(&render_preferences(&p));
    assert!(back.issues.is_empty(), "{:?}", back.issues);
    assert_eq!(back.prefs.files, p.files);
}

#[cfg(unix)]
#[test]
fn rule58_paths_that_are_not_utf8_are_left_out_when_saving() {
    use std::os::unix::ffi::OsStringExt;
    let mut p = Preferences::default();
    p.files.recent.push(PathBuf::from("good.pgn"));
    p.files
        .recent
        .push(PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'b', 0xff, b'd',
        ])));
    p.files.last_open_dir = Some(PathBuf::from(std::ffi::OsString::from_vec(vec![
        0xfe, 0xfe,
    ])));
    let text = render_preferences(&p);
    assert!(!text.is_empty(), "the rest of the file is still written");
    let back = load(&text).prefs;
    assert_eq!(
        back.files.recent.iter().cloned().collect::<Vec<_>>(),
        vec![PathBuf::from("good.pgn")]
    );
    assert_eq!(back.files.last_open_dir, None);
}

// ------------------------------------------------------------------ rule 66

proptest! {
    #[test]
    fn rule66_arbitrary_text_never_panics_and_yields_valid_preferences(text in "\\PC{0,400}") {
        let l = load_preferences(&text);
        check_invariants(&l.prefs)?;
    }

    #[test]
    fn rule66_arbitrary_bytes_as_text_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let l = load_preferences(&text);
        check_invariants(&l.prefs)?;
    }

    #[test]
    fn rule66_every_known_key_with_every_kind_of_value_keeps_the_invariants(
        values in proptest::collection::vec(arb_value(), KEYS.len())
    ) {
        let mut doc = toml::Table::new();
        for ((section, key), value) in KEYS.iter().zip(values) {
            let entry = doc.entry(*section).or_insert_with(|| toml::Value::Table(toml::Table::new()));
            entry.as_table_mut().unwrap().insert((*key).to_owned(), value);
        }
        let text = toml::to_string(&doc).unwrap();
        let l = load_preferences(&text);
        check_invariants(&l.prefs)?;
        // And what we load can be written and read back unchanged.
        let again = load_preferences(&render_preferences(&l.prefs));
        prop_assert_eq!(again.prefs, l.prefs);
    }
}

const KEYS: &[(&str, &str)] = &[
    ("general", "language"),
    ("general", "theme"),
    ("board", "theme"),
    ("board", "piece_set"),
    ("board", "coordinates"),
    ("board", "show_side_to_move"),
    ("board", "highlight_last_move"),
    ("board", "animation_ms"),
    ("playback", "autoplay_delay_ms"),
    ("window", "width"),
    ("window", "height"),
    ("window", "x"),
    ("window", "y"),
    ("window", "maximized"),
    ("files", "recent"),
    ("files", "last_open_dir"),
    ("files", "last_save_dir"),
    ("files", "eco_file"),
    ("pgn_export", "nags"),
    ("pgn_export", "indent_variations"),
    ("pgn_export", "indent_comments"),
    ("pgn_export", "strip_marks"),
    ("pgn_export", "encoding"),
    ("pgn_export", "width"),
    ("game_info", "hide_next_move"),
    ("game_info", "full_comment"),
    ("game_info", "show_fen"),
    ("game_info", "show_material"),
    ("shortcuts", "FlipBoard"),
];

fn arb_value() -> impl Strategy<Value = toml::Value> {
    prop_oneof![
        any::<i64>().prop_map(toml::Value::Integer),
        Just(toml::Value::Integer(0)),
        Just(toml::Value::Integer(-1)),
        Just(toml::Value::Integer(i64::MAX)),
        Just(toml::Value::Integer(i64::MIN)),
        "\\PC{0,12}".prop_map(toml::Value::String),
        prop_oneof![
            Just("system"),
            Just("dark"),
            Just("pt-BR"),
            Just("inside"),
            Just("utf8-bom"),
            Just("symbolic")
        ]
        .prop_map(|s| toml::Value::String(s.to_owned())),
        any::<bool>().prop_map(toml::Value::Boolean),
        any::<f64>().prop_map(toml::Value::Float),
        proptest::collection::vec("\\PC{0,8}", 0..14)
            .prop_map(|v| toml::Value::Array(v.into_iter().map(toml::Value::String).collect())),
        Just(toml::Value::Table(toml::Table::new())),
    ]
}

fn check_invariants(p: &Preferences) -> Result<(), TestCaseError> {
    prop_assert!(p.board.animation_ms <= MAX_ANIMATION_MS);
    prop_assert!(
        (AUTOPLAY_DELAY_RANGE_MS.0..=AUTOPLAY_DELAY_RANGE_MS.1)
            .contains(&p.playback.autoplay_delay_ms)
    );
    prop_assert!(p.window.width.is_none_or(|w| w >= MIN_WINDOW_WIDTH));
    prop_assert!(p.window.height.is_none_or(|h| h >= MIN_WINDOW_HEIGHT));
    prop_assert!(p.files.recent.len() <= 10);
    prop_assert_eq!(p.version, PREFERENCES_VERSION);
    prop_assert!((20..=500).contains(&p.pgn_export.width) || p.pgn_export.width == 80);
    Ok(())
}
