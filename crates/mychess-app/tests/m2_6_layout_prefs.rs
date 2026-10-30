//! M2-6 phase A: the layout inside a real preferences file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m2_6_layout_common;

use std::fs;

use m2_6_layout_common::*;
use mychess_app::{LayoutNode, PaneKind::*, Preferences, load_preferences, render_preferences};

#[test]
fn layout_written_to_a_file_is_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let layout = default_layout()
        .open(GameList)
        .merge(Info, Notation)
        .select(Comment);
    assert!(layout.is_valid());
    fs::write(
        &path,
        render_preferences(&Preferences {
            layout: Some(layout.clone()),
            ..Preferences::default()
        }),
    )
    .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.layout, Some(layout));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn a_v1_file_is_migrated_and_saved_as_v2() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    fs::write(
        &path,
        "version = 1\n[layout]\naxis = \"horizontal\"\nratio = 0.7\n\
         [layout.a]\npane = \"board\"\n[layout.b]\npane = \"notation\"\n",
    )
    .unwrap();
    let loaded = load_preferences(&fs::read_to_string(&path).unwrap());
    let expected = h(0.7, pane(Board), grp(&[Notation, Info, Comment], 0));
    assert_eq!(loaded.prefs.layout, Some(expected.clone()));
    fs::write(&path, render_preferences(&loaded.prefs)).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("layout_version = 2"), "{text}");
    assert_eq!(load_preferences(&text).prefs.layout, Some(expected));
}

#[test]
fn the_layout_sits_beside_other_sections_without_disturbing_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let mut prefs = Preferences::default();
    prefs.board.animation_ms = 123;
    prefs.layout = Some(h(0.4, pane(Board), grp(&[Comment, Tree], 1)));
    fs::write(&path, render_preferences(&prefs)).unwrap();
    let loaded = load_preferences(&fs::read_to_string(&path).unwrap());
    assert_eq!(loaded.prefs, prefs);
}

#[test]
fn a_bad_layout_does_not_drop_the_other_preferences() {
    let loaded = load_preferences(
        "[board]\nanimation_ms = 123\n[layout]\naxis = \"horizontal\"\nratio = 7\n\
         [layout.a]\npane = \"board\"\n[layout.b]\ntabs = [\"info\", \"info\"]\n",
    );
    assert_eq!(loaded.prefs.board.animation_ms, 123);
    assert_eq!(
        loaded.prefs.layout.clone().unwrap_or_else(default_layout),
        default_layout()
    );
    assert!(!loaded.issues.is_empty());
}

#[test]
fn a_layout_that_is_not_a_table_does_not_bring_the_reader_down() {
    for text in [
        "layout = 3\n",
        "layout = [1, 2]\n",
        "layout = \"x\"\n",
        "[layout]\n",
    ] {
        let loaded = load_preferences(text);
        assert_eq!(
            loaded.prefs.layout.unwrap_or_else(default_layout),
            default_layout()
        );
    }
}

#[test]
fn corrupted_files_load_as_defaults_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let good = render(&default_layout());
    let mut garbage: Vec<Vec<u8>> = vec![
        b"\x00\x01\x02 not toml {{{".to_vec(),
        b"[layout\naxis = ".to_vec(),
        vec![0xFF, 0xFE, b'[', b'l', 0xC3],
        good.as_bytes()[..good.len() / 2].to_vec(),
    ];
    garbage.push(Vec::new());
    for bytes in garbage {
        fs::write(&path, &bytes).unwrap();
        let text = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        let loaded = load_preferences(&text);
        // Whatever came out is usable.
        let layout = loaded.prefs.layout.unwrap_or_else(default_layout);
        assert!(layout.is_valid(), "{bytes:?}");
    }
}

#[test]
fn a_truncated_v2_file_never_yields_an_invalid_layout() {
    let full = render(&default_layout().open(Tree).merge(Info, Notation));
    for end in 0..full.len() {
        if !full.is_char_boundary(end) {
            continue;
        }
        let loaded = load_preferences(&full[..end]);
        if let Some(layout) = loaded.prefs.layout {
            assert!(layout.is_valid(), "cut at {end}: {layout:?}");
        }
    }
}

#[test]
fn rendering_a_default_preferences_set_has_no_layout_table() {
    let loaded = load_preferences(&render_preferences(&Preferences::default()));
    assert_eq!(loaded.prefs.layout, None::<LayoutNode>);
}
