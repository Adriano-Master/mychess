use pretty_assertions::assert_eq;

use super::*;
use crate::search::{MinMax, Tri};
use crate::si4::GameFlags;

fn sample(name: &str) -> SavedSearch {
    let mut search = HeaderSearch {
        white: "Carlsen".to_owned(),
        white_elo: MinMax::new(2700, 4000),
        ..HeaderSearch::default()
    };
    search.flags.set(GameFlags::TACTICS, Tri::Yes);
    search.pgn_text = vec!["Bxh7+".to_owned()];
    SavedSearch::new(name, search).unwrap()
}

#[test]
fn toml_round_trips_a_search() {
    let mut s = sample("Tactics by Carlsen");
    s.created = Some("2026-10-23".to_owned());
    let text = s.to_toml().unwrap();
    assert!(text.contains("format = \"mychess-saved-search\""), "{text}");
    assert!(text.contains("version = 1"));
    assert_eq!(SavedSearch::from_toml(&text).unwrap(), s);
}

#[test]
fn default_search_toml_is_stable_and_round_trips() {
    let s = SavedSearch::new("d", HeaderSearch::default()).unwrap();
    let text = s.to_toml().unwrap();
    assert_eq!(SavedSearch::from_toml(&text).unwrap(), s);
    assert!(!text.contains("[search.eco]") && !text.contains("[search.length]"));
}

#[test]
fn missing_search_fields_use_the_defaults() {
    let text = "format = \"mychess-saved-search\"\nversion = 1\nname = \"x\"\n\
                [search]\nwhite = \"A\"\n";
    let s = SavedSearch::from_toml(text).unwrap();
    assert_eq!(s.search.white, "A");
    assert_eq!(s.search.black, "");
    assert_eq!(s.search.white_elo, HeaderSearch::default().white_elo);
}

#[test]
fn rejects_wrong_format_and_future_versions() {
    let bad = [
        "name = \"x\"\n",
        "format = \"other\"\nversion = 1\nname = \"x\"\n",
        "format = \"mychess-saved-search\"\nname = \"x\"\n",
    ];
    for t in bad {
        assert!(matches!(
            SavedSearch::from_toml(t),
            Err(SavedSearchError::WrongFormat)
        ));
    }
    let future = "format = \"mychess-saved-search\"\nversion = 2\nname = \"x\"\n\
                  [search]\nbrand_new = 1\n";
    assert!(matches!(
        SavedSearch::from_toml(future),
        Err(SavedSearchError::UnsupportedVersion { found: 2 })
    ));
}

#[test]
fn invalid_text_values_are_typed_errors() {
    let head = "format = \"mychess-saved-search\"\nversion = 1\nname = \"x\"\n[search]\n";
    for body in [
        "date_min = \"20x4\"\n",
        "op = \"maybe\"\n",
        "game_end = \"draw\"\n",
        "white_titles = [\"king\"]\n",
        "[search.flags]\ntactics = \"perhaps\"\n",
        "[search.flags]\nnope = \"yes\"\n",
        "[search.eco]\nmin = \"Z9\"\nmax = \"E99\"\ninclude_none = true\n",
    ] {
        let err = SavedSearch::from_toml(&format!("{head}{body}")).unwrap_err();
        assert!(
            matches!(err, SavedSearchError::InvalidValue { .. }),
            "{body}: {err:?}"
        );
    }
}

#[test]
fn type_errors_and_unknown_fields_are_toml_errors() {
    let head = "format = \"mychess-saved-search\"\nversion = 1\nname = \"x\"\n[search]\n";
    for body in [
        "white_elo = 5\n",
        "unknown_field = 1\n",
        "[search.white_elo]\nmin = 70000\nmax = 1\n",
        "ignore_case = \"yes\"\n",
    ] {
        let err = SavedSearch::from_toml(&format!("{head}{body}")).unwrap_err();
        assert!(
            matches!(err, SavedSearchError::Toml { .. }),
            "{body}: {err:?}"
        );
    }
    assert!(matches!(
        SavedSearch::from_toml("not = [toml"),
        Err(SavedSearchError::Toml { .. })
    ));
}

#[test]
fn names_are_validated() {
    for bad in ["", "   ", "a\nb", &"x".repeat(101)] {
        assert!(
            SavedSearch::new(bad, HeaderSearch::default()).is_err(),
            "{bad:?}"
        );
    }
    assert!(SavedSearch::new(&"é".repeat(100), HeaderSearch::default()).is_ok());
    let mut s = sample("ok");
    s.name = " padded ".to_owned();
    assert!(matches!(
        s.to_toml(),
        Err(SavedSearchError::InvalidName { .. })
    ));
    s.name = "ok".to_owned();
    s.created = Some("23/10/2026".to_owned());
    assert!(matches!(
        s.to_toml(),
        Err(SavedSearchError::InvalidDate { .. })
    ));
}

#[test]
fn file_stems_are_safe() {
    for (name, stem) in [
        ("My Search", "my search"),
        ("a<b>c:d\"e/f\\g|h?i*j", "a_b_c_d_e_f_g_h_i_j"),
        ("../etc/passwd", "_etc_passwd"),
        ("CON", "_con"),
        ("com1", "_com1"),
        ("Lpt9.txt", "_lpt9.txt"),
        ("nul.", "_nul"),
        ("...", "search"),
        ("  trailing. ", "trailing"),
        (".hidden", "hidden"),
        ("com10", "com10"),
        ("É", "é"),
    ] {
        assert_eq!(file_stem_for(name), stem, "{name:?}");
    }
    assert_eq!(file_stem_for(&"a".repeat(300)).len(), 80);
    for name in ["a\u{0}b", "x\ty", "..", "/", "\\"] {
        let stem = file_stem_for(name);
        assert!(
            !stem.contains(['/', '\\', '\0', '\t']) && stem != "..",
            "{stem:?}"
        );
    }
}

#[test]
fn save_load_list_delete_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("nested").join("searches");
    let a = sample("Beta");
    let b = sample("alpha");
    let pa = save(&base, &a).unwrap();
    let pb = save(&base, &b).unwrap();
    assert_eq!(pa, base.join("beta.toml"));
    assert_eq!(load(&base, "Beta").unwrap(), a);
    assert_eq!(load(&base, " alpha ").unwrap(), b);
    let l = list(&base).unwrap();
    let names: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["alpha", "Beta"]);
    assert_eq!(l.entries[0].path, pb);
    assert!(l.skipped.is_empty());
    assert!(delete(&base, "Beta").unwrap());
    assert!(!delete(&base, "Beta").unwrap());
    assert!(matches!(
        load(&base, "Beta"),
        Err(SavedSearchError::NotFound { .. })
    ));
    assert!(!pa.exists());
}

#[test]
fn saving_the_same_name_replaces_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sample("Same");
    let p1 = save(dir.path(), &s).unwrap();
    s.search.black = "Anand".to_owned();
    let p2 = save(dir.path(), &s).unwrap();
    assert_eq!(p1, p2);
    assert_eq!(list(dir.path()).unwrap().entries.len(), 1);
    assert_eq!(load(dir.path(), "Same").unwrap().search.black, "Anand");
}

#[test]
fn names_with_the_same_file_stem_do_not_collide() {
    let dir = tempfile::tempdir().unwrap();
    let names = ["A/B", "A_B", "a:b", "A?B"];
    let mut paths = Vec::new();
    for n in names {
        paths.push(save(dir.path(), &sample(n)).unwrap());
    }
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), 4);
    for p in &paths {
        assert_eq!(p.parent(), Some(dir.path()), "{p:?}");
    }
    for n in names {
        assert_eq!(load(dir.path(), n).unwrap().name, n);
    }
}

#[test]
fn dangerous_names_stay_inside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    for n in ["../../evil", "CON", "a\\b", "NUL", "..", "x/../../y"] {
        let p = save(dir.path(), &sample(n)).unwrap();
        assert_eq!(p.parent(), Some(dir.path()), "{n}: {p:?}");
        assert_eq!(load(dir.path(), n).unwrap().name, n);
    }
}

#[test]
fn list_skips_bad_files_and_non_toml() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), &sample("good")).unwrap();
    fs::write(dir.path().join("broken.toml"), "= = =").unwrap();
    fs::write(dir.path().join("binary.TOML"), [0u8, 1, 2, 255]).unwrap();
    fs::write(dir.path().join("notes.txt"), "hello").unwrap();
    fs::create_dir(dir.path().join("dir.toml")).unwrap();
    let l = list(dir.path()).unwrap();
    assert_eq!(l.entries.len(), 1);
    assert_eq!(l.skipped.len(), 2);
}

#[test]
fn list_of_a_missing_directory_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let l = list(&dir.path().join("nope")).unwrap();
    assert!(l.entries.is_empty() && l.skipped.is_empty());
}

#[test]
fn does_not_overwrite_a_foreign_file_with_the_same_stem() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("mine.toml"), "user data").unwrap();
    let p = save(dir.path(), &sample("Mine")).unwrap();
    assert_eq!(p, dir.path().join("mine-2.toml"));
    assert_eq!(
        fs::read_to_string(dir.path().join("mine.toml")).unwrap(),
        "user data"
    );
}

#[test]
fn huge_and_binary_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.toml");
    fs::write(
        &big,
        vec![b'#'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1],
    )
    .unwrap();
    assert!(matches!(
        load_file(&big),
        Err(SavedSearchError::TooLarge { .. })
    ));
    let bin = dir.path().join("bin.toml");
    fs::write(&bin, b"format\0").unwrap();
    assert!(matches!(load_file(&bin), Err(SavedSearchError::NotText)));
    let missing = dir.path().join("none.toml");
    assert!(matches!(
        load_file(&missing),
        Err(SavedSearchError::Io { .. })
    ));
}

#[test]
fn atomic_write_leaves_no_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let p = save(dir.path(), &sample("x")).unwrap();
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [p.file_name().unwrap().to_string_lossy().into_owned()]
    );
}

#[test]
fn failed_rename_removes_the_temporary_and_keeps_the_destination() {
    let dir = tempfile::tempdir().unwrap();
    // A directory in the way makes the final rename fail on every platform.
    let dest = dir.path().join("x.toml");
    fs::create_dir(&dest).unwrap();
    fs::write(dest.join("keep"), "1").unwrap();
    let err = write_atomic(&dest, b"data").unwrap_err();
    assert!(matches!(err, SavedSearchError::Io { .. }));
    assert!(!dir.path().join(".x.toml.tmp").exists());
    assert!(dest.join("keep").exists());
}

#[test]
fn a_stale_temporary_is_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("y.toml");
    fs::write(dir.path().join(".y.toml.tmp"), "stale stale stale").unwrap();
    write_atomic(&dest, b"new").unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join(".y.toml.tmp").exists());
}

#[test]
fn imported_sso_is_saved_and_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let sso = dir.path().join("my.sso");
    fs::write(
        &sso,
        "set searchType Header\nset sWhite {Kasparov, G}\nset pMin(wq) 1\n",
    )
    .unwrap();
    let report = import_sso(&sso).unwrap();
    assert_eq!(report.warnings.len(), 1);
    let saved = report.into_saved("From Scid", Some("2026-10-23")).unwrap();
    let store = dir.path().join("store");
    save(&store, &saved).unwrap();
    assert_eq!(
        load(&store, "From Scid").unwrap().search.white,
        "Kasparov, G"
    );
}

#[test]
fn flags_use_all_22_keys() {
    let mut s = sample("flags");
    for (i, (_, flag, _)) in format::FLAGS.iter().enumerate() {
        s.search
            .flags
            .set(*flag, if i % 2 == 0 { Tri::Yes } else { Tri::No });
    }
    let back = SavedSearch::from_toml(&s.to_toml().unwrap()).unwrap();
    assert_eq!(back, s);
}

#[cfg(unix)]
#[test]
fn unwritable_directory_is_an_io_error_without_leftovers() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("ro");
    fs::create_dir(&ro).unwrap();
    fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
    let result = save(&ro, &sample("x"));
    fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
    // Running as root ignores the permission bits.
    if let Err(e) = result {
        assert!(matches!(e, SavedSearchError::Io { .. }));
        assert_eq!(fs::read_dir(&ro).unwrap().count(), 0);
    }
}
