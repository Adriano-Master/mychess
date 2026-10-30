//! M5-3b contract tests of the preferences of the tree cache (D1760, D1761):
//! `[tree] cache_size` and `cache_autosave`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{
    CACHE_SIZE_RANGE, CACHE_SIZES, DEFAULT_CACHE_SIZE, Preferences, load_preferences,
    render_preferences,
};

#[test]
fn the_defaults_are_the_scids() {
    let prefs = TreePrefs::default();
    assert_eq!(prefs.cache_size, 1000);
    assert_eq!(DEFAULT_CACHE_SIZE, 1000);
    assert!(!prefs.cache_autosave, "auto-save is opt-in, as in the Scid");
    assert_eq!(*CACHE_SIZE_RANGE.start(), 250);
    assert_eq!(*CACHE_SIZE_RANGE.end(), 10_000);
    assert_eq!(CACHE_SIZES, [250, 500, 1000, 2000, 5000, 10_000]);
}

#[test]
fn the_cache_options_round_trip_through_the_file() {
    for size in CACHE_SIZES {
        let prefs = Preferences {
            tree: TreePrefs {
                cache_size: size,
                cache_autosave: size % 2 == 0,
                ..TreePrefs::default()
            },
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("cache_size"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.tree, prefs.tree);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }
}

#[test]
fn a_size_out_of_range_is_limited_and_reported() {
    let loaded = load_preferences("[tree]\ncache_size = 50\n");
    assert_eq!(loaded.prefs.tree.cache_size, 250);
    assert_eq!(loaded.issues.len(), 1);
    let loaded = load_preferences("[tree]\ncache_size = 99999999\n");
    assert_eq!(loaded.prefs.tree.cache_size, 10_000);
    assert_eq!(loaded.issues.len(), 1);
}

#[test]
fn a_wrong_type_keeps_the_default_and_unknown_keys_are_ignored() {
    let loaded = load_preferences(
        "[tree]\ncache_size = \"big\"\ncache_autosave = true\nfuture_option = 7\n",
    );
    assert_eq!(loaded.prefs.tree.cache_size, 1000);
    assert!(loaded.prefs.tree.cache_autosave);
    // Only the wrong type is an issue; the key of a newer version is not.
    assert_eq!(loaded.issues.len(), 1, "{:?}", loaded.issues);
}

#[test]
fn an_old_file_without_the_keys_gets_the_defaults_without_issues() {
    let loaded = load_preferences("[tree]\nshort = false\n");
    assert_eq!(loaded.prefs.tree.cache_size, 1000);
    assert!(!loaded.prefs.tree.cache_autosave);
    assert!(loaded.issues.is_empty());
}

#[test]
fn the_messages_change_the_preferences_and_limit_the_size() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::SetCacheSize(2000));
    assert_eq!(rig.state.prefs.tree.cache_size, 2000);
    assert_eq!(rig.view().cache.size, 2000);
    rig.send(TreeMessage::SetCacheSize(1));
    assert_eq!(rig.state.prefs.tree.cache_size, 250);
    rig.send(TreeMessage::SetCacheSize(1_000_000));
    assert_eq!(rig.state.prefs.tree.cache_size, 10_000);
    rig.send(TreeMessage::SetCacheAutosave(true));
    assert!(rig.view().cache.autosave);
}
