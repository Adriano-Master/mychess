//! Tests of the storage of the user's patterns (spec
//! `docs/specs/m6-2-taticas-e-padroes.md`, "Armazenamento", D3277, rules 23
//! and 24): atomic writes, safe file names, limits, hostile files.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use mychess_db::pattern::model::{
    Condition, MAX_PATTERN_FILE_BYTES, Pattern, PatternError, PatternFile, Scope, Side,
};
use mychess_db::pattern::{
    MAX_PATTERNS, delete_pattern, delete_pattern_file, export_pattern, import_pattern,
    list_patterns, load_pattern, load_pattern_named, name_exists, save_pattern,
};
use pretty_assertions::assert_eq;

fn pattern(name: &str) -> PatternFile {
    PatternFile {
        pattern: Pattern {
            name: name.into(),
            description: "desc".into(),
            scope: Scope::default(),
            root: Condition::SideToMove { side: Side::Black },
        },
        created: Some("2026-10-27".into()),
    }
}

fn names(dir: &Path) -> Vec<String> {
    list_patterns(dir)
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect()
}

#[test]
fn save_list_load_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("patterns");
    for n in ["beta", "Alpha", "gamma"] {
        save_pattern(&data, &pattern(n)).unwrap();
    }
    assert_eq!(names(&data), vec!["Alpha", "beta", "gamma"]);
    assert_eq!(
        load_pattern_named(&data, " beta ").unwrap(),
        pattern("beta")
    );
    assert!(name_exists(&data, &pattern("beta").pattern).unwrap());
    delete_pattern(&data, "beta").unwrap();
    assert_eq!(names(&data), vec!["Alpha", "gamma"]);
    assert!(matches!(
        load_pattern_named(&data, "beta"),
        Err(PatternError::NotFound(_))
    ));
    assert!(matches!(
        delete_pattern(&data, "beta"),
        Err(PatternError::NotFound(_))
    ));
}

#[test]
fn saving_the_same_name_replaces_and_leaves_no_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let a = save_pattern(dir.path(), &pattern("X")).unwrap();
    let mut changed = pattern("X");
    changed.pattern.description = "new".into();
    let b = save_pattern(dir.path(), &changed).unwrap();
    assert_eq!(a, b);
    assert_eq!(load_pattern(&a).unwrap().pattern.description, "new");
    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files.iter().all(|f| !f.starts_with('.')));
}

#[test]
fn a_failed_save_keeps_the_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = save_pattern(dir.path(), &pattern("Keep")).unwrap();
    let before = fs::read(&path).unwrap();
    let mut bad = pattern("Keep");
    bad.pattern.root = Condition::All { items: vec![] };
    assert!(save_pattern(dir.path(), &bad).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut bad_name = pattern("Keep");
    bad_name.pattern.name = String::new();
    assert!(save_pattern(dir.path(), &bad_name).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn names_become_safe_file_names_inside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let hostile = [
        "CON",
        "nul",
        "COM1",
        "../../etc/passwd",
        "a/b\\c",
        "a:b*c?\"d<e>f|g",
        "..",
        ".",
        " x ",
        "Ação",
        "ünï",
        &"n".repeat(80),
    ];
    for name in hostile {
        let path = save_pattern(dir.path(), &pattern(name)).unwrap();
        assert_eq!(path.parent().unwrap(), dir.path(), "{name:?}");
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert!(!stem.is_empty() && !stem.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']));
        assert!(!stem.eq_ignore_ascii_case("con") && !stem.eq_ignore_ascii_case("nul"));
        assert_eq!(load_pattern(&path).unwrap().pattern.name, name);
    }
    // Different names with the same stem never overwrite each other.
    assert_eq!(names(dir.path()).len(), hostile.len());
}

#[test]
fn names_differing_only_by_case_get_separate_files() {
    let dir = tempfile::tempdir().unwrap();
    let a = save_pattern(dir.path(), &pattern("Fork")).unwrap();
    let b = save_pattern(dir.path(), &pattern("fork")).unwrap();
    assert_ne!(a, b);
    assert_eq!(names(dir.path()).len(), 2);
}

#[test]
fn hostile_files_are_errors_and_broken_entries_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    let good = save_pattern(dir.path(), &pattern("Good")).unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let p = dir.path().join(name);
        fs::write(&p, bytes).unwrap();
        p
    };
    let huge = write(
        "huge.toml",
        &vec![b'#'; usize::try_from(MAX_PATTERN_FILE_BYTES).unwrap() + 10],
    );
    assert!(matches!(
        load_pattern(&huge),
        Err(PatternError::TooLarge { .. })
    ));
    let nul = write("nul.toml", b"format = \"mychess-pattern\"\0");
    assert!(matches!(load_pattern(&nul), Err(PatternError::NotText)));
    let junk = write("junk.toml", &[0xFF, 0xFE, b'x', b'=', 0x80]);
    assert!(load_pattern(&junk).is_err());
    let wrong = write("wrong.toml", b"format = \"other\"\nversion = 1\n");
    assert!(matches!(
        load_pattern(&wrong),
        Err(PatternError::WrongFormat { .. })
    ));
    let future = write(
        "future.toml",
        b"format = \"mychess-pattern\"\nversion = 2\n",
    );
    assert!(matches!(
        load_pattern(&future),
        Err(PatternError::UnsupportedVersion { found: 2 })
    ));
    let mut deep = String::from("format = \"mychess-pattern\"\nversion = 1\nname = \"d\"\n");
    deep.push_str("[root]\nkind = \"not\"\n");
    let mut path = String::from("root");
    for _ in 0..200 {
        path.push_str(".item");
        writeln!(deep, "[{path}]\nkind = \"not\"").unwrap();
    }
    let deep = write("deep.toml", deep.as_bytes());
    assert!(load_pattern(&deep).is_err());
    let unknown = write(
        "unknown.toml",
        b"format = \"mychess-pattern\"\nversion = 1\nname = \"u\"\nbogus = 1\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n",
    );
    assert!(load_pattern(&unknown).is_err());
    // A directory called x.toml is not a pattern and is not listed.
    fs::create_dir(dir.path().join("dir.toml")).unwrap();
    assert!(load_pattern(&dir.path().join("dir.toml")).is_err());
    assert!(load_pattern(&dir.path().join("missing.toml")).is_err());

    let list = list_patterns(dir.path()).unwrap();
    let good_entry = list.iter().find(|s| s.path == good).unwrap();
    assert!(good_entry.broken.is_none());
    let broken: Vec<_> = list.iter().filter(|s| s.broken.is_some()).collect();
    assert_eq!(broken.len(), 7);
    assert!(list.iter().all(|s| !s.path.ends_with("dir.toml")));
    // Broken files can still be removed from the list.
    delete_pattern_file(dir.path(), &huge).unwrap();
    assert!(!huge.exists());
    // A Windows-1252 file is accepted.
    let latin = b"format = \"mychess-pattern\"\nversion = 1\nname = \"A\xE7\xE3o\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n";
    let latin = write("latin.toml", latin);
    assert_eq!(load_pattern(&latin).unwrap().pattern.name, "Ação");
}

#[test]
fn the_directory_holds_at_most_max_patterns() {
    let dir = tempfile::tempdir().unwrap();
    let first = save_pattern(dir.path(), &pattern("p0")).unwrap();
    let text = fs::read_to_string(&first).unwrap();
    for i in 1..MAX_PATTERNS + 5 {
        fs::write(
            dir.path().join(format!("copy-{i:05}.toml")),
            text.replace("p0", &format!("p{i}")),
        )
        .unwrap();
    }
    assert_eq!(list_patterns(dir.path()).unwrap().len(), MAX_PATTERNS);
    assert!(save_pattern(dir.path(), &pattern("brand new")).is_err());
}

#[test]
fn import_never_replaces_and_export_roundtrips() {
    let data = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let file = pattern("Shared");
    let exported = outside.path().join("shared.toml");
    export_pattern(&exported, &file).unwrap();
    assert_eq!(load_pattern(&exported).unwrap(), file);

    save_pattern(data.path(), &file).unwrap();
    let copy = import_pattern(data.path(), &exported).unwrap();
    assert_eq!(load_pattern(&copy).unwrap().pattern.name, "Shared (2)");
    let again = import_pattern(data.path(), &exported).unwrap();
    assert_eq!(load_pattern(&again).unwrap().pattern.name, "Shared (3)");
    assert_eq!(
        names(data.path()),
        vec!["Shared", "Shared (2)", "Shared (3)"]
    );
    let long = pattern(&"L".repeat(80));
    let src = outside.path().join("long.toml");
    export_pattern(&src, &long).unwrap();
    save_pattern(data.path(), &long).unwrap();
    let dup = import_pattern(data.path(), &src).unwrap();
    assert_eq!(load_pattern(&dup).unwrap().pattern.name.chars().count(), 80);
    // An invalid source is refused.
    fs::write(outside.path().join("bad.toml"), "not toml [").unwrap();
    assert!(import_pattern(data.path(), &outside.path().join("bad.toml")).is_err());
}

#[test]
fn only_files_directly_in_the_directory_can_be_deleted_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let stranger = other.path().join("x.toml");
    fs::write(&stranger, "x").unwrap();
    assert!(delete_pattern_file(dir.path(), &stranger).is_err());
    assert!(stranger.exists());
    let txt = dir.path().join("x.txt");
    fs::write(&txt, "x").unwrap();
    assert!(delete_pattern_file(dir.path(), &txt).is_err());
}

#[test]
fn files_that_are_not_patterns_do_not_push_patterns_out_of_the_list() {
    let dir = tempfile::tempdir().unwrap();
    // More than ten times MAX_PATTERNS entries that sort before the patterns
    // and are not patterns (wrong extension, hidden).
    for i in 0..MAX_PATTERNS * 10 + 50 {
        fs::write(dir.path().join(format!("a{i:05}.txt")), "x").unwrap();
    }
    fs::write(dir.path().join(".hidden.toml"), "x").unwrap();
    fs::create_dir(dir.path().join("aaa-dir.toml")).unwrap();
    for n in ["one", "Two", "three"] {
        save_pattern(dir.path(), &pattern(n)).unwrap();
    }
    assert_eq!(names(dir.path()), vec!["one", "three", "Two"]);
    assert_eq!(list_patterns(dir.path()).unwrap().len(), 3);
}

#[test]
fn the_cut_at_max_patterns_is_by_name_without_case() {
    let dir = tempfile::tempdir().unwrap();
    let first = save_pattern(dir.path(), &pattern("p0")).unwrap();
    let text = fs::read_to_string(&first).unwrap();
    for i in 0..MAX_PATTERNS + 3 {
        // Upper and lower case names interleave in the sort without case.
        let name = if i % 2 == 0 {
            format!("copy-{i:05}.toml")
        } else {
            format!("COPY-{i:05}.toml")
        };
        fs::write(dir.path().join(name), text.replace("p0", &format!("p{i}"))).unwrap();
    }
    let list = list_patterns(dir.path()).unwrap();
    assert_eq!(list.len(), MAX_PATTERNS);
    // The files left out are the last ones in the order without case.
    let last = list.last().unwrap();
    assert!(last.name.starts_with('p'));
    assert!(!list.iter().any(
        |s| s.path.file_name().unwrap().to_string_lossy().to_lowercase()
            == format!("copy-{:05}.toml", MAX_PATTERNS + 2)
    ));
}

#[test]
fn importing_into_a_full_namespace_lists_the_directory_once() {
    let data = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let first = pattern("X");
    let text = {
        let p = save_pattern(data.path(), &first).unwrap();
        fs::read_to_string(p).unwrap()
    };
    for n in 2..MAX_PATTERNS {
        fs::write(
            data.path().join(format!("x-{n:04}.toml")),
            text.replace("name = \"X\"", &format!("name = \"X ({n})\"")),
        )
        .unwrap();
    }
    assert_eq!(list_patterns(data.path()).unwrap().len(), MAX_PATTERNS - 1);
    let src = outside.path().join("x.toml");
    export_pattern(&src, &first).unwrap();
    let started = std::time::Instant::now();
    let copy = import_pattern(data.path(), &src).unwrap();
    // Linear in the directory: a listing per candidate name took minutes.
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_eq!(
        load_pattern(&copy).unwrap().pattern.name,
        format!("X ({MAX_PATTERNS})")
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_an_error_and_nothing_is_written() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let src = outside.path().join("p.toml");
    export_pattern(&src, &pattern("P")).unwrap();
    fs::set_permissions(data.path(), fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(data.path()).is_ok();
    let results = (
        import_pattern(data.path(), &src).is_err(),
        save_pattern(data.path(), &pattern("Q")).is_err(),
        name_exists(data.path(), &pattern("P").pattern).is_err(),
        load_pattern_named(data.path(), "P").is_err(),
    );
    fs::set_permissions(data.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        // Running as root: permissions do not apply, nothing to check.
        return;
    }
    assert_eq!(results, (true, true, true, true));
    assert_eq!(fs::read_dir(data.path()).unwrap().count(), 0);
}

#[test]
fn name_exists_trims_like_the_loaders() {
    let dir = tempfile::tempdir().unwrap();
    save_pattern(dir.path(), &pattern("Trim")).unwrap();
    let mut padded = pattern("Trim").pattern;
    padded.name = "  Trim ".into();
    assert!(name_exists(dir.path(), &padded).unwrap());
}
