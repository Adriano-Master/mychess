//! File naming and I/O of the cache (M5-3a, D1049, D1050, D1056 of
//! `docs/specs/m5-3-cache-stc.md`): the `.stc` path, the size returned by
//! `write`, the atomic write, the no-op without a file, `remove_stc`.
#![allow(
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

#[path = "common/m5_3_support.rs"]
mod stc;

use std::path::Path;

use mychess_core::Position;
use mychess_db::tree::TreeCache;
use mychess_db::tree::stc::{StcCache, StcError, remove_stc, stc_path};
use pretty_assertions::assert_eq;
use rstest::rstest;
use stc::{header, positions, stc_file, tmp_base, tree_of};

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

// ------------------------------------------------------------ path

#[rstest]
#[case::si4("games.si4", "games.stc")]
#[case::upper_si4("games.SI4", "games.stc")]
#[case::mixed_case("games.Si4", "games.stc")]
#[case::names("games.sn4", "games.stc")]
#[case::games("games.sg4", "games.stc")]
#[case::upper_games("GAMES.SG4", "GAMES.stc")]
#[case::no_extension("games", "games.stc")]
#[case::dotted_name("games.v2", "games.v2.stc")]
fn the_suffix_is_stc_next_to_the_database_in_the_same_directory(
    #[case] given: &str,
    #[case] expected: &str,
) {
    let dir = Path::new("some").join("dir with spaces");
    let path = stc_path(&dir.join(given));
    assert_eq!(path, dir.join(expected));
    assert_eq!(path.parent(), Some(dir.as_path()));
}

#[test]
fn a_dotted_directory_name_is_not_taken_for_an_extension() {
    let p = Path::new("x.si4").join("base");
    assert_eq!(stc_path(&p), Path::new("x.si4").join("base.stc"));
}

#[test]
fn every_path_of_the_database_gives_the_same_cache_file() {
    let (_dir, base) = tmp_base();
    let a = stc_path(&base);
    assert_eq!(stc_path(&base.with_extension("si4")), a);
    assert_eq!(stc_path(&base.with_extension("sn4")), a);
    assert_eq!(stc_path(&base.with_extension("sg4")), a);
}

// ------------------------------------------------------------ write

#[test]
fn write_returns_the_size_of_the_file_and_leaves_no_temporary() {
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base.with_extension("si4")));
    for p in &positions(5) {
        cache.store(p, tree_of(10, 3));
    }
    let size = cache.write(None).unwrap();
    assert_eq!(size, std::fs::metadata(stc_file(&base)).unwrap().len());
    assert!(size > 22);
    assert_eq!(names(dir.path()), ["games.stc"]);
}

#[test]
fn write_to_names_the_base_explicitly() {
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(None);
    cache.store(&Position::new(), tree_of(3, 1));
    let size = cache.write_to(&base, None).unwrap();
    assert!(size > 22);
    assert_eq!(names(dir.path()), ["games.stc"]);
}

#[test]
fn writing_twice_replaces_the_file_without_leftovers() {
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    let first = cache.write(None).unwrap();
    for p in &positions(3) {
        cache.store(p, tree_of(3, 1));
    }
    let second = cache.write(None).unwrap();
    assert!(second > first);
    assert_eq!(std::fs::metadata(stc_file(&base)).unwrap().len(), second);
    assert_eq!(names(dir.path()), ["games.stc"]);
}

#[test]
fn a_cache_without_a_database_writes_nothing_and_returns_zero() {
    let mut cache = StcCache::new(None);
    cache.store(&Position::new(), tree_of(3, 1));
    assert_eq!(cache.write(None).unwrap(), 0);
}

#[test]
fn writing_a_cache_never_used_replaces_the_file_with_an_empty_one() {
    // D1056: write() does not read first (as in the Scid).
    let (_dir, base) = tmp_base();
    let mut full = StcCache::new(Some(&base));
    full.store(&Position::new(), tree_of(3, 1));
    full.write(None).unwrap();
    assert!(std::fs::metadata(stc_file(&base)).unwrap().len() > 22);

    let unused = StcCache::new(Some(&base));
    assert_eq!(unused.write(None).unwrap(), 22);
    assert_eq!(
        std::fs::read(stc_file(&base)).unwrap(),
        header(1000, 0, 0, 0)
    );
    let mut again = StcCache::new(Some(&base));
    assert!(again.lookup(&Position::new(), 3).is_none());
}

#[test]
fn a_failed_write_keeps_the_original_file_and_reports_an_io_error() {
    // D1062: the temporary name is exclusive and not fixed, so the failure is
    // forced elsewhere: the database directory vanishes under the cache.
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    let original = std::fs::read(stc_file(&base)).unwrap();
    for p in &positions(4) {
        cache.store(p, tree_of(3, 1));
    }

    // A base in a directory that does not exist: an error, not a panic, and
    // the file of the real base is untouched.
    let nowhere = dir.path().join("missing").join("games");
    let r = cache.write_to(&nowhere, None);
    assert!(matches!(r, Err(StcError::Io { .. })), "{r:?}");
    assert_eq!(std::fs::read(stc_file(&base)).unwrap(), original);
    assert_eq!(names(dir.path()), ["games.stc"], "no temporary left");
}

#[cfg(unix)]
#[test]
fn a_write_that_cannot_create_the_temporary_keeps_the_original_file() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    let original = std::fs::read(stc_file(&base)).unwrap();
    for p in &positions(4) {
        cache.store(p, tree_of(3, 1));
    }

    // A directory without write permission: no file can be created in it.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.path().join("probe");
    let privileged = std::fs::write(&probe, b"x").is_ok(); // e.g. running as root
    let r = cache.write(None);
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    if privileged {
        return; // permissions do not apply to this user
    }
    assert!(matches!(r, Err(StcError::Io { .. })), "{r:?}");
    assert_eq!(std::fs::read(stc_file(&base)).unwrap(), original);
    assert_eq!(names(dir.path()), ["games.stc"], "no temporary left");
}

#[test]
fn a_failed_write_to_a_directory_destination_keeps_what_is_there() {
    let (dir, base) = tmp_base();
    std::fs::create_dir(stc_file(&base)).unwrap();
    std::fs::write(stc_file(&base).join("inner"), b"keep").unwrap();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    let r = cache.write(None);
    assert!(matches!(r, Err(StcError::Io { .. })), "{r:?}");
    assert_eq!(
        std::fs::read(stc_file(&base).join("inner")).unwrap(),
        b"keep"
    );
    assert_eq!(names(dir.path()), ["games.stc"], "no temporary left");
}

#[test]
fn a_write_that_cannot_replace_the_destination_leaves_no_temporary() {
    let (dir, base) = tmp_base();
    // The destination is a directory: the rename must fail.
    std::fs::create_dir(stc_file(&base)).unwrap();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    let r = cache.write(None);
    assert!(matches!(r, Err(StcError::Io { .. })), "{r:?}");
    assert_eq!(names(dir.path()), ["games.stc"], "no .tmp left");
    assert!(stc_file(&base).is_dir());
}

#[test]
fn writing_into_a_missing_directory_is_an_error_not_a_panic() {
    let (_dir, base) = tmp_base();
    let nowhere = base.parent().unwrap().join("missing").join("games");
    let mut cache = StcCache::new(Some(&nowhere));
    cache.store(&Position::new(), tree_of(3, 1));
    assert!(matches!(cache.write(None), Err(StcError::Io { .. })));
}

#[test]
fn paths_with_spaces_and_non_ascii_letters_work() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("minhas partidas \u{e1}\u{e9}");
    std::fs::create_dir(&folder).unwrap();
    let base = folder.join("\u{f3}rf\u{e3}o.si4");
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    assert!(folder.join("\u{f3}rf\u{e3}o.stc").is_file());
    let mut fresh = StcCache::new(Some(&base));
    assert!(fresh.lookup(&Position::new(), 3).is_some());
}

// ------------------------------------------------------------ remove_stc

#[test]
fn remove_stc_removes_an_existing_file_and_reports_it() {
    let (dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    assert!(remove_stc(&base.with_extension("si4")).unwrap());
    assert!(names(dir.path()).is_empty());
}

#[test]
fn remove_stc_of_a_missing_file_is_ok_and_false() {
    let (_dir, base) = tmp_base();
    assert!(!remove_stc(&base).unwrap());
    // Also when the directory does not exist.
    let nowhere = base.parent().unwrap().join("missing").join("games");
    assert!(!remove_stc(&nowhere).unwrap());
}

#[test]
fn remove_stc_that_cannot_remove_reports_an_io_error() {
    let (_dir, base) = tmp_base();
    std::fs::create_dir(stc_file(&base)).unwrap();
    assert!(matches!(remove_stc(&base), Err(StcError::Io { .. })));
}

#[test]
fn clear_plus_remove_stc_is_the_invalidation_point_of_d1050() {
    let (_dir, base) = tmp_base();
    let mut cache = StcCache::new(Some(&base));
    cache.store(&Position::new(), tree_of(3, 1));
    cache.write(None).unwrap();
    // Clear alone: the file is still there and is read again.
    cache.clear();
    assert!(cache.lookup(&Position::new(), 3).is_some());
    // Clear + remove: gone.
    cache.clear();
    assert!(remove_stc(&base).unwrap());
    assert!(cache.lookup(&Position::new(), 3).is_none());
    assert_eq!((cache.info().0, cache.backup_info().0), (0, 0));
}
