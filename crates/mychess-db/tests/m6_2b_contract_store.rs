//! Contract tests of the storage of the user's patterns (spec
//! `docs/specs/m6-2-taticas-e-padroes.md`, "Dados e formatos" and rules 21,
//! 23, 24; `docs/formatos/padrao-mychess.md` section 4): safe file names on
//! the three platforms, replacement, never overwriting on import, broken
//! files in the list, the limit, hostile files, atomic writes.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::format_collect,
    clippy::map_unwrap_or,
    let_underscore_drop,
    clippy::let_underscore_must_use,
    dead_code
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mychess_db::pattern::model::{
    Condition, MAX_PATTERN_FILE_BYTES, Pattern, PatternError, PatternFile, Scope, Side,
};
use mychess_db::pattern::{
    MAX_PATTERNS, PatternSummary, delete_pattern, delete_pattern_file, export_pattern,
    import_pattern, list_patterns, load_pattern, load_pattern_named, name_exists, save_pattern,
};
use pretty_assertions::assert_eq;

fn file(name: &str) -> PatternFile {
    PatternFile {
        pattern: Pattern {
            name: name.into(),
            description: format!("about {name}"),
            scope: Scope::default(),
            root: Condition::SideToMove { side: Side::White },
        },
        created: Some("2026-10-27".into()),
    }
}

fn listing(dir: &Path) -> Vec<PatternSummary> {
    list_patterns(dir).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    listing(dir).into_iter().map(|s| s.name).collect()
}

fn all_entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

fn is_windows_reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

// ----------------------------------------------------------- safe names

#[test]
fn hostile_but_valid_names_become_safe_files_inside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let long = "x".repeat(80);
    let hostile: Vec<&str> = vec![
        "a/b",
        "a\\b",
        "a:b",
        "CON",
        "con",
        "nul",
        "NUL",
        "PRN",
        "AUX",
        "COM1",
        "com9",
        "LPT1",
        "lpt9",
        "CON.txt",
        "nul.",
        "..",
        ".",
        "...",
        "../../escape",
        "..\\..\\escape",
        "C:\\Windows\\system32",
        "/etc/passwd",
        "name.",
        "name ",
        " name",
        "a|b?c*d\"e<f>g",
        "Ação – 日本語 ♞",
        "ÉCOLE",
        "ß",
        "İstanbul",
        &long,
    ];
    let mut paths = Vec::new();
    for name in &hostile {
        let path = save_pattern(dir.path(), &file(name)).unwrap();
        assert_eq!(path.parent().unwrap(), dir.path(), "{name:?} escaped");
        let fname = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(fname.ends_with(".toml"), "{name:?}: {fname}");
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert!(!stem.is_empty(), "{name:?}");
        assert!(!stem.contains(FORBIDDEN), "{name:?}: {stem}");
        assert!(!stem.chars().any(char::is_control), "{name:?}");
        assert!(!is_windows_reserved(&stem), "{name:?}: {stem}");
        assert!(
            !stem.starts_with(['.', ' ']) && !stem.ends_with(['.', ' ']),
            "{name:?}: {stem}"
        );
        assert!(fname.chars().count() <= 100, "{name:?}");
        // The real name lives inside the file.
        assert_eq!(load_pattern(&path).unwrap().pattern.name, *name);
        paths.push(path);
    }
    // No two files collide, even where the file system ignores the case.
    let mut lower: Vec<String> = paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_lowercase())
        .collect();
    lower.sort();
    lower.dedup();
    assert_eq!(lower.len(), hostile.len());
    assert_eq!(all_entries(dir.path()).len(), hostile.len());
    assert_eq!(listing(dir.path()).len(), hostile.len());
}

#[test]
fn invalid_names_are_refused_and_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let mut too_long = "n".repeat(300);
    let bad = [
        String::new(),
        too_long.clone(),
        "x".repeat(81),
        "tab\there".to_owned(),
        "nul\0byte".to_owned(),
        "line\nbreak".to_owned(),
    ];
    too_long.clear();
    for name in &bad {
        let r = save_pattern(dir.path(), &file(name));
        assert!(
            r.is_err(),
            "{:?} must be refused",
            name.chars().take(10).collect::<String>()
        );
    }
    assert_eq!(all_entries(dir.path()), Vec::<String>::new());
}

#[test]
fn eighty_characters_of_multibyte_text_is_a_valid_name() {
    let dir = tempfile::tempdir().unwrap();
    let name = "é".repeat(80);
    let path = save_pattern(dir.path(), &file(&name)).unwrap();
    assert_eq!(load_pattern(&path).unwrap().pattern.name, name);
    assert!(
        path.file_name().unwrap().len() < 255,
        "fits a file name limit"
    );
}

#[test]
fn names_that_map_to_the_same_stem_get_separate_files() {
    let dir = tempfile::tempdir().unwrap();
    let a = save_pattern(dir.path(), &file("A/B")).unwrap();
    let b = save_pattern(dir.path(), &file("a_b")).unwrap();
    let c = save_pattern(dir.path(), &file("A:B")).unwrap();
    let d = save_pattern(dir.path(), &file("a*b")).unwrap();
    let set: std::collections::HashSet<_> = [&a, &b, &c, &d].into_iter().collect();
    assert_eq!(set.len(), 4);
    // Saving each again replaces its own file, never another one.
    assert_eq!(save_pattern(dir.path(), &file("A:B")).unwrap(), c);
    assert_eq!(save_pattern(dir.path(), &file("A/B")).unwrap(), a);
    assert_eq!(listing(dir.path()).len(), 4);
}

#[test]
fn names_differing_only_in_case_do_not_share_a_file_name_even_ignoring_case() {
    let dir = tempfile::tempdir().unwrap();
    let a = save_pattern(dir.path(), &file("Fork")).unwrap();
    let b = save_pattern(dir.path(), &file("fork")).unwrap();
    let c = save_pattern(dir.path(), &file("FORK")).unwrap();
    let lower = |p: &PathBuf| p.file_name().unwrap().to_string_lossy().to_lowercase();
    let set: std::collections::HashSet<_> = [lower(&a), lower(&b), lower(&c)].into_iter().collect();
    assert_eq!(set.len(), 3);
    assert_eq!(listing(dir.path()).len(), 3);
}

// ------------------------------------------------------------- replace

#[test]
fn saving_the_same_name_replaces_the_content_and_keeps_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = save_pattern(dir.path(), &file("Same")).unwrap();
    let mut changed = file("Same");
    changed.pattern.description = "second".into();
    changed.pattern.scope.hold = 3;
    let b = save_pattern(dir.path(), &changed).unwrap();
    assert_eq!(a, b);
    assert_eq!(all_entries(dir.path()).len(), 1);
    assert_eq!(load_pattern(&b).unwrap(), changed);
    assert!(name_exists(dir.path(), &changed.pattern).unwrap());
    assert!(!name_exists(dir.path(), &file("Other").pattern).unwrap());
}

#[test]
fn saving_creates_the_directory_and_its_parents() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("a").join("b").join("patterns");
    let path = save_pattern(&dir, &file("x")).unwrap();
    assert!(path.starts_with(&dir) && path.is_file());
}

#[test]
fn a_directory_that_does_not_exist_lists_as_empty() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(listing(&root.path().join("nothing")).len(), 0);
    assert!(!name_exists(&root.path().join("nothing"), &file("x").pattern).unwrap());
    assert!(matches!(
        load_pattern_named(&root.path().join("nothing"), "x"),
        Err(PatternError::NotFound(_))
    ));
}

#[test]
fn listing_a_path_that_is_a_file_is_an_error_not_a_panic() {
    let root = tempfile::tempdir().unwrap();
    let f = root.path().join("plain");
    fs::write(&f, "x").unwrap();
    assert!(list_patterns(&f).is_err());
    assert!(save_pattern(&f, &file("x")).is_err());
}

// --------------------------------------------------------------- import

#[test]
fn import_never_overwrites_and_numbers_the_copies() {
    let data = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let src = out.path().join("pattern.toml");
    export_pattern(&src, &file("Shared")).unwrap();
    save_pattern(data.path(), &file("Shared")).unwrap();
    let original = fs::read(save_pattern(data.path(), &file("Shared")).unwrap()).unwrap();
    for n in 2..=5 {
        let p = import_pattern(data.path(), &src).unwrap();
        assert_eq!(
            load_pattern(&p).unwrap().pattern.name,
            format!("Shared ({n})")
        );
    }
    assert_eq!(
        names(data.path()),
        vec![
            "Shared",
            "Shared (2)",
            "Shared (3)",
            "Shared (4)",
            "Shared (5)"
        ]
    );
    // The first one is intact.
    let first = load_pattern_named(data.path(), "Shared").unwrap();
    assert_eq!(first, file("Shared"));
    assert_eq!(
        fs::read(save_pattern(data.path(), &file("Shared")).unwrap()).unwrap(),
        original
    );
}

#[test]
fn import_into_a_fresh_directory_keeps_the_name_and_leaves_the_source() {
    let data = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let src = out.path().join("whatever-name.toml");
    export_pattern(&src, &file("Fresh one")).unwrap();
    let before = fs::read(&src).unwrap();
    let p = import_pattern(data.path(), &src).unwrap();
    assert_eq!(load_pattern(&p).unwrap(), file("Fresh one"));
    assert_eq!(fs::read(&src).unwrap(), before, "the source is only read");
    assert!(p.starts_with(data.path()));
}

#[test]
fn import_keeps_a_long_name_within_eighty_characters() {
    let data = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let src = out.path().join("long.toml");
    let long = file(&"L".repeat(80));
    export_pattern(&src, &long).unwrap();
    save_pattern(data.path(), &long).unwrap();
    for _ in 0..3 {
        let p = import_pattern(data.path(), &src).unwrap();
        let name = load_pattern(&p).unwrap().pattern.name;
        assert!(name.chars().count() <= 80, "{name}");
    }
    assert_eq!(listing(data.path()).len(), 4);
    let mut unique = names(data.path());
    unique.dedup();
    assert_eq!(unique.len(), 4);
}

#[test]
fn import_validates_the_source_like_a_load() {
    let data = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let w = |n: &str, b: &[u8]| {
        let p = out.path().join(n);
        fs::write(&p, b).unwrap();
        p
    };
    let cases = [
        w("garbage.toml", b"not toml ["),
        w("empty.toml", b""),
        w("nul.toml", b"format = \"mychess-pattern\"\0\n"),
        w(
            "future.toml",
            b"format = \"mychess-pattern\"\nversion = 99\n",
        ),
        w(
            "huge.toml",
            &vec![b'#'; usize::try_from(MAX_PATTERN_FILE_BYTES).unwrap() + 1],
        ),
        out.path().join("missing.toml"),
        out.path().to_path_buf(),
    ];
    for c in &cases {
        assert!(import_pattern(data.path(), c).is_err(), "{}", c.display());
    }
    assert_eq!(all_entries(data.path()).len(), 0, "nothing was copied");
}

// ---------------------------------------------------------- the listing

#[test]
fn a_broken_file_is_listed_as_broken_and_does_not_stop_the_list() {
    let dir = tempfile::tempdir().unwrap();
    save_pattern(dir.path(), &file("Good one")).unwrap();
    save_pattern(dir.path(), &file("another good")).unwrap();
    fs::write(dir.path().join("broken.toml"), "this is = not [valid").unwrap();
    fs::write(dir.path().join("zero.toml"), b"").unwrap();
    fs::write(dir.path().join("binary.toml"), [0u8, 159, 146, 150, 0]).unwrap();
    fs::write(dir.path().join("UPPER.TOML"), "garbage").unwrap();
    let l = listing(dir.path());
    assert_eq!(l.len(), 6);
    let good: Vec<_> = l
        .iter()
        .filter(|s| s.broken.is_none())
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(good, vec!["another good", "Good one"]);
    let broken: Vec<_> = l.iter().filter(|s| s.broken.is_some()).collect();
    assert_eq!(broken.len(), 4);
    for b in &broken {
        assert!(!b.broken.as_ref().unwrap().is_empty(), "a reason is given");
        assert!(b.path.is_file());
        assert!(b.description.is_empty() && b.created.is_none());
    }
    // A broken pattern is not found by name, and is deletable by path.
    assert!(matches!(
        load_pattern_named(dir.path(), "broken"),
        Err(PatternError::NotFound(_))
    ));
    for b in broken {
        delete_pattern_file(dir.path(), &b.path).unwrap();
    }
    assert_eq!(names(dir.path()), vec!["another good", "Good one"]);
}

#[test]
fn the_list_is_sorted_by_name_ignoring_case_and_ignores_other_files() {
    let dir = tempfile::tempdir().unwrap();
    for n in ["delta", "Bravo", "alpha", "Charlie", "échec", "Zulu"] {
        save_pattern(dir.path(), &file(n)).unwrap();
    }
    fs::write(dir.path().join("notes.txt"), "x").unwrap();
    fs::write(dir.path().join(".hidden.toml"), "x").unwrap();
    fs::write(dir.path().join("toml"), "x").unwrap();
    fs::create_dir(dir.path().join("sub.toml")).unwrap();
    let got = names(dir.path());
    let mut want = got.clone();
    want.sort_by_key(|s| s.to_lowercase());
    assert_eq!(got, want);
    assert_eq!(got.len(), 6);
    let l = listing(dir.path());
    assert_eq!(l[0].created.as_deref(), Some("2026-10-27"));
    assert_eq!(l[0].description, "about alpha");
}

#[test]
fn the_list_is_not_affected_by_a_stale_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".x.toml.tmp"), "half written").unwrap();
    let p = save_pattern(dir.path(), &file("x")).unwrap();
    assert_eq!(names(dir.path()), vec!["x"]);
    assert!(load_pattern(&p).is_ok());
}

// --------------------------------------------------------------- limit

fn fill_to_limit(dir: &Path) {
    let first = save_pattern(dir, &file("p0")).unwrap();
    let text = fs::read_to_string(&first).unwrap();
    for i in 1..MAX_PATTERNS {
        fs::write(
            dir.join(format!("copy-{i:05}.toml")),
            text.replace("p0", &format!("p{i}")),
        )
        .unwrap();
    }
}

#[test]
fn at_the_limit_a_new_name_is_refused_but_an_existing_one_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    fill_to_limit(dir.path());
    assert_eq!(listing(dir.path()).len(), MAX_PATTERNS);
    let before = all_entries(dir.path());
    assert!(save_pattern(dir.path(), &file("one too many")).is_err());
    assert_eq!(all_entries(dir.path()), before, "nothing was written");
    let mut changed = file("p5");
    changed.pattern.description = "edited".into();
    save_pattern(dir.path(), &changed).unwrap();
    assert_eq!(load_pattern_named(dir.path(), "p5").unwrap(), changed);
    assert_eq!(all_entries(dir.path()), before);
    // Importing a new name also respects the limit.
    let out = tempfile::tempdir().unwrap();
    let src = out.path().join("new.toml");
    export_pattern(&src, &file("brand new")).unwrap();
    assert!(import_pattern(dir.path(), &src).is_err());
    // Deleting one makes room again.
    delete_pattern(dir.path(), "p7").unwrap();
    save_pattern(dir.path(), &file("one too many")).unwrap();
}

#[test]
fn a_directory_with_more_than_the_limit_lists_exactly_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    fill_to_limit(dir.path());
    let text = fs::read_to_string(dir.path().join("p0.toml")).unwrap();
    for i in 0..50 {
        fs::write(
            dir.path().join(format!("extra-{i:03}.toml")),
            text.replace("p0", &format!("e{i}")),
        )
        .unwrap();
    }
    assert_eq!(listing(dir.path()).len(), MAX_PATTERNS);
}

// ------------------------------------------------------ delete / named

#[test]
fn delete_of_a_missing_pattern_is_not_found_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    save_pattern(dir.path(), &file("keep")).unwrap();
    for name in ["missing", "", "KEEP2", "../keep", "keep/"] {
        assert!(
            matches!(
                delete_pattern(dir.path(), name),
                Err(PatternError::NotFound(_))
            ),
            "{name:?}"
        );
    }
    assert_eq!(names(dir.path()), vec!["keep"]);
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        delete_pattern(empty.path(), "x"),
        Err(PatternError::NotFound(_))
    ));
    delete_pattern(dir.path(), "  keep  ").unwrap();
    assert_eq!(names(dir.path()).len(), 0);
}

#[test]
fn delete_by_path_only_touches_toml_files_directly_inside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let nested = sub.join("n.toml");
    fs::write(&nested, "x").unwrap();
    let outside = other.path().join("o.toml");
    fs::write(&outside, "x").unwrap();
    let txt = dir.path().join("t.txt");
    fs::write(&txt, "x").unwrap();
    let sneaky = dir.path().join("sub").join("..").join("s.toml");
    fs::write(dir.path().join("s.toml"), "x").unwrap();
    for p in [
        &nested,
        &outside,
        &txt,
        &sneaky,
        &PathBuf::new(),
        &PathBuf::from("/"),
    ] {
        assert!(
            delete_pattern_file(dir.path(), p).is_err(),
            "{}",
            p.display()
        );
    }
    assert!(nested.exists() && outside.exists() && txt.exists());
    assert!(dir.path().join("s.toml").exists());
    // A plain good path works.
    delete_pattern_file(dir.path(), &dir.path().join("s.toml")).unwrap();
    // A missing file is an error, not a panic.
    assert!(delete_pattern_file(dir.path(), &dir.path().join("gone.toml")).is_err());
}

// --------------------------------------------------- export and round trip

#[test]
fn export_then_load_returns_the_same_pattern_file() {
    let out = tempfile::tempdir().unwrap();
    let mut f = file("Ida e volta ♞");
    f.pattern.description = "linha 1\nlinha \"2\" \\ com 'aspas'\ttab".into();
    f.pattern.scope.hold = 4;
    f.pattern.scope.ignore_colors = true;
    f.created = None;
    for name in ["a.toml", "no-extension", "with space.TOML"] {
        let dest = out.path().join(name);
        export_pattern(&dest, &f).unwrap();
        assert_eq!(load_pattern(&dest).unwrap(), f, "{name}");
    }
    assert!(
        all_entries(out.path()).iter().all(|n| !n.starts_with('.')),
        "no temporaries"
    );
}

#[test]
fn export_replaces_an_existing_file_atomically_and_keeps_it_on_failure() {
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("p.toml");
    export_pattern(&dest, &file("first")).unwrap();
    export_pattern(&dest, &file("second")).unwrap();
    assert_eq!(load_pattern(&dest).unwrap().pattern.name, "second");
    let before = fs::read(&dest).unwrap();
    let mut bad = file("third");
    bad.pattern.root = Condition::All { items: vec![] };
    assert!(export_pattern(&dest, &bad).is_err());
    assert_eq!(fs::read(&dest).unwrap(), before);
    assert_eq!(all_entries(out.path()), vec!["p.toml"]);
}

#[test]
fn export_to_a_missing_directory_is_an_error_without_leftovers() {
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("no").join("such").join("p.toml");
    assert!(export_pattern(&dest, &file("x")).is_err());
    assert_eq!(all_entries(out.path()).len(), 0);
    // Exporting onto a directory fails and leaves it alone.
    let d = out.path().join("adir");
    fs::create_dir(&d).unwrap();
    assert!(export_pattern(&d, &file("x")).is_err());
    assert!(d.is_dir());
    assert_eq!(all_entries(out.path()), vec!["adir"]);
    assert!(export_pattern(Path::new(""), &file("x")).is_err());
}

#[test]
fn no_temporary_file_is_left_after_any_sequence_of_operations() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    for i in 0..5 {
        save_pattern(dir.path(), &file(&format!("p{}", i % 3))).unwrap();
    }
    let mut bad = file("p1");
    bad.pattern.root = Condition::Any { items: vec![] };
    assert!(save_pattern(dir.path(), &bad).is_err());
    let src = out.path().join("s.toml");
    export_pattern(&src, &file("p1")).unwrap();
    import_pattern(dir.path(), &src).unwrap();
    delete_pattern(dir.path(), "p0").unwrap();
    for d in [dir.path(), out.path()] {
        for e in all_entries(d) {
            assert!(!e.starts_with('.') && !e.ends_with(".tmp"), "leftover {e}");
        }
    }
}

// ------------------------------------------------------- hostile files

fn valid_text() -> String {
    file("Valid").to_toml().unwrap()
}

#[test]
fn every_truncation_of_a_valid_file_is_an_error_or_a_valid_pattern_never_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let text = valid_text();
    let bytes = text.as_bytes();
    let p = dir.path().join("t.toml");
    let mut ok = 0;
    for cut in 0..bytes.len() {
        fs::write(&p, &bytes[..cut]).unwrap();
        if load_pattern(&p).is_ok() {
            ok += 1;
        }
    }
    assert!(ok < bytes.len(), "the empty prefix at least is invalid");
    fs::write(&p, bytes).unwrap();
    assert!(load_pattern(&p).is_ok());
    // The same truncated files in a listing never panic nor hide the good one.
    save_pattern(dir.path(), &file("alive")).unwrap();
    fs::write(&p, &bytes[..bytes.len() / 2]).unwrap();
    assert!(
        listing(dir.path())
            .iter()
            .any(|s| s.name == "alive" && s.broken.is_none())
    );
}

#[test]
fn byte_flips_in_a_valid_file_never_panic() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("f.toml");
    let base = valid_text().into_bytes();
    for i in (0..base.len()).step_by(3) {
        for v in [0u8, b'[', b'$', b'"', 0xFF, b'\n', b'=', 0x80] {
            let mut b = base.clone();
            b[i] = v;
            fs::write(&p, &b).unwrap();
            let _ = load_pattern(&p);
        }
    }
}

#[test]
fn hostile_texts_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("h.toml");
    let deep_arrays = format!("x = {}{}", "[".repeat(50_000), "]".repeat(50_000));
    let deep_inline = format!("x = {}1{}", "{a=".repeat(20_000), "}".repeat(20_000));
    let many_tables: String = (0..20_000).map(|i| format!("[t{i}]\n")).collect();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("dollar", b"$ = [".to_vec()),
        ("brackets", b"[[[[[[".to_vec()),
        ("nul", b"\0\0\0".to_vec()),
        ("bom only", vec![0xEF, 0xBB, 0xBF]),
        ("utf16", "format = \"mychess-pattern\"".encode_utf16().flat_map(u16::to_le_bytes).collect()),
        ("deep arrays", deep_arrays.into_bytes()),
        ("deep inline tables", deep_inline.into_bytes()),
        ("many tables", many_tables.into_bytes()),
        ("huge int", b"format = \"mychess-pattern\"\nversion = 99999999999999999999999\n".to_vec()),
        ("negative version", b"format = \"mychess-pattern\"\nversion = -1\nname = \"x\"\n".to_vec()),
        ("string version", b"format = \"mychess-pattern\"\nversion = \"1\"\nname = \"x\"\n".to_vec()),
        ("no root", b"format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n".to_vec()),
        ("no version", b"format = \"mychess-pattern\"\nname = \"x\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n".to_vec()),
        ("control name", b"format = \"mychess-pattern\"\nversion = 1\nname = \"a\\u0007b\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n".to_vec()),
        ("name too long", format!("format = \"mychess-pattern\"\nversion = 1\nname = \"{}\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n", "n".repeat(81)).into_bytes()),
        ("empty name", b"format = \"mychess-pattern\"\nversion = 1\nname = \"\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n".to_vec()),
        ("unknown kind", b"format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\n[root]\nkind = \"queen_of_hearts\"\n".to_vec()),
        ("bad date", b"format = \"mychess-pattern\"\nversion = 1\nname = \"x\"\ncreated = \"yesterday\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n".to_vec()),
    ];
    for (name, bytes) in cases {
        fs::write(&p, &bytes).unwrap();
        assert!(load_pattern(&p).is_err(), "{name} must be an error");
    }
}

#[test]
fn a_file_exactly_at_the_size_limit_is_read_and_one_byte_more_is_too_large() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.toml");
    let limit = usize::try_from(MAX_PATTERN_FILE_BYTES).unwrap();
    let mut text = valid_text();
    // Pad with a comment up to exactly the limit.
    text.push_str("\n# ");
    let pad = limit - text.len();
    text.push_str(&"p".repeat(pad));
    assert_eq!(text.len(), limit);
    fs::write(&p, &text).unwrap();
    assert_eq!(load_pattern(&p).unwrap().pattern.name, "Valid");
    text.push('p');
    fs::write(&p, &text).unwrap();
    assert!(matches!(
        load_pattern(&p),
        Err(PatternError::TooLarge { .. })
    ));
}

#[test]
fn encodings_utf8_bom_and_windows_1252_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("e.toml");
    let body = |name: &[u8]| {
        let mut v = b"format = \"mychess-pattern\"\nversion = 1\nname = \"".to_vec();
        v.extend_from_slice(name);
        v.extend_from_slice(b"\"\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n");
        v
    };
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend(body("Ação".as_bytes()));
    fs::write(&p, &bom).unwrap();
    assert_eq!(load_pattern(&p).unwrap().pattern.name, "Ação");
    fs::write(&p, body(b"A\xE7\xE3o")).unwrap();
    assert_eq!(load_pattern(&p).unwrap().pattern.name, "Ação");
    let crlf = String::from_utf8(body(b"crlf"))
        .unwrap()
        .replace('\n', "\r\n");
    fs::write(&p, crlf).unwrap();
    assert_eq!(load_pattern(&p).unwrap().pattern.name, "crlf");
}

#[test]
fn a_directory_is_not_a_pattern_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("looks-like-a-pattern.toml");
    fs::create_dir(&d).unwrap();
    assert!(load_pattern(&d).is_err());
    assert!(load_pattern(dir.path()).is_err());
    assert!(import_pattern(dir.path(), &d).is_err());
    assert_eq!(listing(dir.path()).len(), 0, "directories are not listed");
}

#[cfg(unix)]
#[test]
fn a_fifo_never_blocks_a_load_an_import_or_a_listing() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe.toml");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("mkfifo not available: skipped");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    save_pattern(dir.path(), &file("beside the pipe")).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (f2, d2, dd) = (
        fifo.clone(),
        dir.path().to_path_buf(),
        data.path().to_path_buf(),
    );
    std::thread::spawn(move || {
        let a = load_pattern(&f2).is_err();
        let b = import_pattern(&dd, &f2).is_err();
        let c = list_patterns(&d2).map(|l| l.len()).unwrap_or(usize::MAX);
        let d = d2.join("pipe.toml");
        let e = delete_pattern_file(&d2, &d).is_ok();
        tx.send((a, b, c, e)).ok();
    });
    let got = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("a FIFO must not block the storage");
    assert_eq!((got.0, got.1, got.2), (true, true, 1));
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_missing_target_is_not_listed_and_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join("dangling.toml"))
        .unwrap();
    save_pattern(dir.path(), &file("fine")).unwrap();
    assert_eq!(names(dir.path()), vec!["fine"]);
    assert!(load_pattern(&dir.path().join("dangling.toml")).is_err());
}

#[test]
fn load_by_name_trims_and_matches_the_exact_name() {
    let dir = tempfile::tempdir().unwrap();
    save_pattern(dir.path(), &file("Exact")).unwrap();
    assert_eq!(
        load_pattern_named(dir.path(), " Exact ").unwrap(),
        file("Exact")
    );
    for n in ["exact", "Exac", "Exact2", ""] {
        assert!(
            matches!(
                load_pattern_named(dir.path(), n),
                Err(PatternError::NotFound(_))
            ),
            "{n:?}"
        );
    }
}
