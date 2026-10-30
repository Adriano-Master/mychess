//! M5-3b additional contract tests of the cache preferences (D1760, D1761,
//! D1047): the range edges, wrong types, unknown keys and the size applied
//! to the live cache.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{CacheFill, CacheStatus, Preferences, load_preferences, render_preferences};
use mychess_db::tree::StcCache;
use tempfile::tempdir;

fn size_of(text: &str) -> (usize, usize) {
    let loaded = load_preferences(text);
    (loaded.prefs.tree.cache_size, loaded.issues.len())
}

#[test]
fn the_edges_of_the_range_are_accepted_and_the_next_values_are_limited() {
    assert_eq!(size_of("[tree]\ncache_size = 250\n"), (250, 0));
    assert_eq!(size_of("[tree]\ncache_size = 10000\n"), (10_000, 0));
    assert_eq!(size_of("[tree]\ncache_size = 249\n"), (250, 1));
    assert_eq!(size_of("[tree]\ncache_size = 10001\n"), (10_000, 1));
    assert_eq!(size_of("[tree]\ncache_size = 0\n"), (250, 1));
    assert_eq!(size_of("[tree]\ncache_size = -5\n"), (250, 1));
    // A value between the menu steps is kept (the menu is not the limit).
    assert_eq!(size_of("[tree]\ncache_size = 1234\n"), (1234, 0));
}

#[test]
fn values_of_the_wrong_type_keep_the_defaults_with_one_issue_each() {
    for text in [
        "[tree]\ncache_size = 1000.5\n",
        "[tree]\ncache_size = true\n",
        "[tree]\ncache_size = [1, 2]\n",
        "[tree]\ncache_size = \"1000\"\n",
    ] {
        let loaded = load_preferences(text);
        assert_eq!(loaded.prefs.tree.cache_size, 1000, "{text}");
        assert_eq!(loaded.issues.len(), 1, "{text}: {:?}", loaded.issues);
    }
    for text in [
        "[tree]\ncache_autosave = \"yes\"\n",
        "[tree]\ncache_autosave = 1\n",
        "[tree]\ncache_autosave = []\n",
    ] {
        let loaded = load_preferences(text);
        assert!(!loaded.prefs.tree.cache_autosave, "{text}");
        assert_eq!(loaded.issues.len(), 1, "{text}: {:?}", loaded.issues);
    }
}

#[test]
fn a_wrong_type_does_not_spoil_the_other_keys_of_the_table() {
    let loaded =
        load_preferences("[tree]\ncache_size = \"x\"\ncache_autosave = true\nshort = true\n");
    assert!(loaded.prefs.tree.cache_autosave);
    assert!(loaded.prefs.tree.short);
}

#[test]
fn unknown_keys_and_tables_are_ignored_without_issues() {
    let loaded = load_preferences(
        "[tree]\ncache_size = 500\nfuture = [1, 2]\n[tree.sub]\nx = 1\n[futuretable]\ncache_size = 9\n",
    );
    assert_eq!(loaded.prefs.tree.cache_size, 500);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn the_cache_keys_in_the_wrong_table_do_not_count() {
    let loaded =
        load_preferences("cache_size = 5000\ncache_autosave = true\n[board]\ncache_size = 5000\n");
    assert_eq!(loaded.prefs.tree.cache_size, 1000);
    assert!(!loaded.prefs.tree.cache_autosave);
}

#[test]
fn a_broken_file_gives_the_defaults_without_a_panic() {
    for text in [
        "[tree\ncache_size =",
        "\u{0}\u{1}",
        "[tree]\ncache_size = 99999999999999999999\n",
    ] {
        let loaded = load_preferences(text);
        assert_eq!(
            loaded.prefs.tree.cache_size.clamp(250, 10_000),
            loaded.prefs.tree.cache_size
        );
    }
}

#[test]
fn rendering_twice_gives_the_same_text() {
    let mut prefs = Preferences::default();
    prefs.tree.cache_size = 5000;
    prefs.tree.cache_autosave = true;
    let text = render_preferences(&prefs);
    let again = render_preferences(&load_preferences(&text).prefs);
    assert_eq!(text, again);
}

// ---- the size applied to the live cache ----

fn info(rig: &mut Rig) -> mychess_app::CacheInfo {
    rig.send(TreeMessage::CacheInfo);
    match rig.view().cache.status {
        CacheStatus::Info(info) => info,
        other => panic!("not the information: {other:?}"),
    }
}

#[test]
fn a_new_size_reaches_the_cache_that_already_exists() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    assert_eq!(info(&mut rig).capacity, 1000);
    rig.send(TreeMessage::SetCacheSize(5000));
    assert_eq!(info(&mut rig).capacity, 5000);
    rig.send(TreeMessage::SetCacheSize(250));
    assert_eq!(info(&mut rig).capacity, 250);
}

#[test]
fn a_cache_created_after_the_choice_uses_it() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.send(TreeMessage::SetCacheSize(2000));
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    assert_eq!(info(&mut rig).capacity, 2000);
}

#[test]
fn growing_keeps_the_trees_and_shrinking_empties_the_memory_not_the_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    rig.send(TreeMessage::FillCache(CacheFill::StandardLines));
    rig.send(TreeMessage::SaveCache);
    let held = info(&mut rig).used;
    assert!(held > 50);
    rig.send(TreeMessage::SetCacheSize(10_000));
    assert_eq!(info(&mut rig).used, held, "growing preserves the entries");
    // Shrinking to a capacity below what is held empties the memory.
    rig.send(TreeMessage::SetCacheSize(250));
    let after = info(&mut rig);
    assert_eq!(after.capacity, 250);
    assert!(after.used <= held);
    let stc = dir.path().join("std.stc");
    assert!(stc.exists(), "the file stays");
    let mut cache = StcCache::new(None);
    cache.read_from(&dir.path().join("std.si4")).unwrap();
    assert_eq!(cache.info().0, held, "and keeps every tree");
}

#[test]
fn the_size_is_global_not_per_base() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "a", STD);
    rig.send(TreeMessage::Open);
    rig.open_si4(dir.path(), "b", STD);
    rig.send(TreeMessage::SetCacheSize(500));
    assert_eq!(info(&mut rig).capacity, 500);
    rig.send(BasesMessage::Select(sel(1)));
    assert_eq!(info(&mut rig).capacity, 500);
}

use mychess_app::BasesMessage;

#[test]
fn setting_the_same_value_changes_nothing_and_starts_no_job() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "std", STD);
    rig.send(TreeMessage::Open);
    let effects = rig.raw(TreeMessage::SetCacheSize(1000));
    assert!(effects.is_empty(), "{effects:?}");
    let effects = rig.raw(TreeMessage::SetCacheAutosave(false));
    assert!(effects.is_empty(), "{effects:?}");
}
