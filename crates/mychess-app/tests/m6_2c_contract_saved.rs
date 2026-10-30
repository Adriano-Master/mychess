//! Extra contract tests of the user's patterns (spec
//! `m6-2-taticas-e-padroes.md`: D3276, D3277, D3278, rule 23 and the
//! delivery notes of slice c, items 32-33): names that try to leave the
//! folder, replacing and importing, hostile and truncated files, files that
//! are not in the folder, the folder that is not there and the family of
//! notices. They complement `m6_2c_saved.rs`.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_2c_common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use m6_2c_common::*;
use mychess_app::pattern_search::model::Scope;
use mychess_app::{ConditionKind, EditRefusal, PatternEdit, PatternIssue, SavedPattern};
use proptest::prelude::*;

fn fresh() -> (Rig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.pat(Pat::Open);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Phase,
    }));
    (rig, dir)
}

fn folder(dir: &Path) -> PathBuf {
    dir.join("patterns")
}

fn issue(rig: &Rig) -> Option<PatternSearchNotice> {
    rig.view().issue
}

fn saved(rig: &Rig) -> Vec<SavedPattern> {
    rig.view().saved
}

fn names(rig: &Rig) -> Vec<String> {
    saved(rig).into_iter().map(|s| s.name).collect()
}

fn files_in(dir: &Path) -> BTreeSet<String> {
    match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => BTreeSet::new(),
    }
}

/// The text of a valid pattern file called `name`.
fn valid_file(name: &str) -> String {
    format!(
        "format = \"mychess-pattern\"\nversion = 1\nname = \"{name}\"\n\n[root]\nkind = \"all\"\n\n\
         [[root.items]]\nkind = \"phase\"\nphase = \"endgame\"\n"
    )
}

// ------------------------------------------------------------------ names

#[test]
fn names_that_try_to_escape_or_clash_never_leave_the_folder() {
    let hostile = [
        "../escape",
        "..\\escape",
        "a/b",
        "a\\b",
        "..",
        ".",
        "/abs",
        "C:\\x",
        "x:y",
        "a<b>c",
        "q\"r",
        "p|q",
        "s?t*u",
        "CON",
        "nul",
        "Com1",
        "LPT9",
        "aux.toml",
        "trailing.",
        "trailing ",
        ".hidden",
        "tab\tname",
        "new\nline",
        "nul\0byte",
        "\u{202e}rtl",
        "é ünï çödé",
        "日本語",
        &"x".repeat(80),
        &"y".repeat(81),
        "",
        "   ",
    ];
    for name in hostile {
        let (mut rig, dir) = fresh();
        rig.pat(Pat::RefreshSaved);
        rig.pat(Pat::SaveAs {
            name: name.into(),
            description: String::new(),
        });
        // Nothing but the folder exists beside the temp contents.
        let top = files_in(dir.path());
        assert!(
            top.is_empty() || top == BTreeSet::from(["patterns".to_owned()]),
            "{name:?} escaped: {top:?}"
        );
        for file in files_in(&folder(dir.path())) {
            let lower = file.to_lowercase();
            assert!(
                Path::new(&lower).extension().is_some_and(|e| e == "toml"),
                "{name:?}: {file}"
            );
            assert!(
                !file.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\0']),
                "{name:?}: {file}"
            );
            let stem = lower.trim_end_matches(".toml");
            assert!(
                !["con", "prn", "aux", "nul", "com1", "lpt9"].contains(&stem),
                "{name:?}: reserved {file}"
            );
        }
        match issue(&rig) {
            Some(PatternSearchNotice::Saved(shown)) => {
                // What was saved can be found again under that name.
                assert!(names(&rig).contains(&shown), "{name:?}: {shown:?}");
                rig.pat(Pat::LoadSaved(shown.clone()));
                assert_eq!(rig.view().draft.name, shown, "{name:?}");
            }
            Some(PatternSearchNotice::InvalidSavedName) => {
                assert!(files_in(&folder(dir.path())).is_empty(), "{name:?}");
            }
            other => panic!("{name:?}: {other:?}"),
        }
    }
}

#[test]
fn names_the_spec_says_are_invalid_are_refused_and_good_ones_are_kept() {
    // 1 to 80 characters, no control characters (Pattern::validate).
    for bad in ["", "   ", "tab\tname", "new\nline", &"y".repeat(81)] {
        let (mut rig, dir) = fresh();
        rig.pat(Pat::SaveAs {
            name: bad.into(),
            description: String::new(),
        });
        assert_eq!(
            issue(&rig),
            Some(PatternSearchNotice::InvalidSavedName),
            "{bad:?}"
        );
        assert_eq!(
            issue(&rig).unwrap().message_key(),
            "pattern-notice-invalid-saved-name"
        );
        assert!(files_in(&folder(dir.path())).is_empty(), "{bad:?}");
    }
    let (mut rig, _dir) = fresh();
    for good in ["Fork", &"x".repeat(80), "Peão passado", "日本語"] {
        rig.pat(Pat::SaveAs {
            name: good.into(),
            description: String::new(),
        });
        assert_eq!(
            issue(&rig),
            Some(PatternSearchNotice::Saved(good.to_owned())),
            "{good:?}"
        );
    }
    assert_eq!(saved(&rig).len(), 4);
}

#[test]
fn a_description_of_501_characters_is_refused_and_500_is_kept() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::SaveAs {
        name: "long".into(),
        description: "d".repeat(501),
    });
    assert!(matches!(
        issue(&rig),
        Some(PatternSearchNotice::InvalidPattern(_) | PatternSearchNotice::InvalidSavedName)
    ));
    assert!(files_in(&folder(dir.path())).is_empty());
    rig.pat(Pat::SaveAs {
        name: "long".into(),
        description: "d".repeat(500),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Saved("long".into())));
    assert_eq!(saved(&rig)[0].description.chars().count(), 500);
}

// -------------------------------------------------------------- replacing

#[test]
fn saving_under_an_existing_name_replaces_it_whatever_the_case() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::SaveAs {
        name: "Fork".into(),
        description: "first".into(),
    });
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Status,
    }));
    let second = rig.view().draft.clone();
    rig.pat(Pat::SaveAs {
        name: "Fork".into(),
        description: "second".into(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Saved("Fork".into())));
    assert_eq!(saved(&rig).len(), 1);
    assert_eq!(saved(&rig)[0].description, "second");
    // Another case is another name, but the two files must not be told
    // apart only by case (Windows and macOS volumes would merge them).
    rig.pat(Pat::SaveAs {
        name: "FORK".into(),
        description: "third".into(),
    });
    let files = files_in(&folder(dir.path()));
    let lower: BTreeSet<String> = files.iter().map(|f| f.to_lowercase()).collect();
    assert_eq!(files.len(), lower.len(), "{files:?}");
    // Saving "Fork" again still replaces the first one.
    rig.pat(Pat::SaveAs {
        name: "Fork".into(),
        description: "fourth".into(),
    });
    let again = names(&rig);
    assert_eq!(
        again.iter().filter(|n| *n == "Fork").count(),
        1,
        "{again:?}"
    );
    assert_eq!(files_in(&folder(dir.path())), files);
    // What is loaded is the last one saved.
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::LoadSaved("Fork".into()));
    assert_eq!(rig.view().draft.root, second.root);
    assert_eq!(rig.view().draft.description, "fourth");
}

#[test]
fn loading_switches_to_the_custom_tab_and_applies_the_scope_of_the_file() {
    let (mut rig, _dir) = fresh();
    let scope = Scope {
        min_move: 4,
        max_move: 33,
        hold: 3,
        end_only: false,
        ignore_colors: true,
    };
    rig.pat(Pat::SetScope(scope.clone()));
    rig.pat(Pat::SaveAs {
        name: "S".into(),
        description: String::new(),
    });
    rig.pat(Pat::SetScope(Scope::default()));
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::LoadSaved("S".into()));
    let view = rig.view();
    assert_eq!(view.tab, PatternTab::Advanced);
    assert_eq!(view.scope, scope);
    assert_eq!(view.draft.scope, scope);
    assert_eq!(view.draft.name, "S");
    assert!(view.selection.is_none());
    assert!(view.issue.is_none());
}

// ----------------------------------------------- traversal on load/delete

#[test]
fn load_and_delete_by_name_never_reach_a_file_outside_the_folder() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::SaveAs {
        name: "In".into(),
        description: String::new(),
    });
    let outside = dir.path().join("outside.toml");
    std::fs::write(&outside, valid_file("Outside")).unwrap();
    let draft = rig.view().draft;
    for name in [
        "../outside",
        "../outside.toml",
        "..\\outside",
        "outside/../../outside",
        &outside.to_string_lossy(),
        "",
        "..",
    ] {
        rig.pat(Pat::LoadSaved(name.into()));
        assert!(
            matches!(
                issue(&rig),
                Some(PatternSearchNotice::SavedNotFound(_) | PatternSearchNotice::InvalidSavedName)
            ),
            "{name:?}: {:?}",
            issue(&rig)
        );
        assert_eq!(rig.view().draft, draft, "{name:?}");
        rig.pat(Pat::DeleteSaved(name.into()));
        assert!(outside.exists(), "{name:?} deleted a file outside");
        assert!(
            matches!(
                issue(&rig),
                Some(PatternSearchNotice::SavedNotFound(_) | PatternSearchNotice::InvalidSavedName)
            ),
            "{name:?}: {:?}",
            issue(&rig)
        );
    }
    assert_eq!(names(&rig), ["In"]);
}

#[test]
fn deleting_a_missing_name_tells_the_name_with_its_own_key() {
    let (mut rig, _dir) = fresh();
    rig.pat(Pat::DeleteSaved("ghost".into()));
    let notice = issue(&rig).unwrap();
    assert_eq!(notice, PatternSearchNotice::SavedNotFound("ghost".into()));
    assert_eq!(notice.message_key(), "pattern-notice-saved-not-found");
    assert!(!notice.is_form_error());
    // The notice reaches the status bar too.
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::PatternSearch(notice.clone()))
    );
    rig.pat(Pat::LoadSaved("ghost".into()));
    assert_eq!(issue(&rig), Some(notice));
}

#[test]
fn deleting_a_file_outside_the_folder_or_not_a_pattern_is_refused() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::SaveAs {
        name: "Keep".into(),
        description: String::new(),
    });
    let patterns = folder(dir.path());
    let outside = dir.path().join("outside.toml");
    std::fs::write(&outside, valid_file("Outside")).unwrap();
    std::fs::write(patterns.join("notes.txt"), "text").unwrap();
    std::fs::create_dir(patterns.join("sub.toml")).unwrap();
    std::fs::write(
        patterns.join("sub.toml").join("inner.toml"),
        valid_file("I"),
    )
    .unwrap();
    let before = files_in(&patterns);
    let targets = [
        outside.clone(),
        patterns.join("..").join("outside.toml"),
        patterns.join("sub.toml").join("inner.toml"),
        patterns.join("sub.toml"),
        patterns.join("notes.txt"),
        patterns.clone(),
        dir.path().to_path_buf(),
        patterns.join("missing.toml"),
        PathBuf::new(),
    ];
    for target in targets {
        rig.pat(Pat::DeleteSavedFile(target.clone()));
        assert!(outside.exists(), "{target:?}");
        assert!(
            patterns.join("sub.toml").join("inner.toml").exists(),
            "{target:?}"
        );
        assert_eq!(files_in(&patterns), before, "{target:?}");
        let notice = issue(&rig).unwrap_or_else(|| panic!("{target:?} was silent"));
        assert!(
            !matches!(notice, PatternSearchNotice::Deleted(_)),
            "{target:?}: {notice:?}"
        );
    }
    // The one in the list is deleted through its path.
    let entry = saved(&rig).remove(0);
    rig.pat(Pat::DeleteSavedFile(entry.path.clone()));
    assert!(!entry.path.exists());
    assert!(saved(&rig).is_empty());
    assert!(matches!(issue(&rig), Some(PatternSearchNotice::Deleted(_))));
}

// ------------------------------------------------------------------ import

#[test]
fn importing_a_valid_file_adds_it_applies_it_and_never_replaces() {
    let (mut rig, dir) = fresh();
    let source = dir.path().join("elsewhere.toml");
    std::fs::write(&source, valid_file("Shared")).unwrap();
    for expected in ["Shared", "Shared (2)", "Shared (3)"] {
        rig.pat(Pat::Import(source.clone()));
        let notice = issue(&rig).expect("notice");
        assert!(
            matches!(&notice, PatternSearchNotice::Imported(n) if n == expected),
            "{expected}: {notice:?}"
        );
        assert_eq!(notice.message_key(), "pattern-notice-imported");
        assert!(!notice.is_form_error());
        assert_eq!(rig.view().draft.name, expected);
        assert_eq!(rig.view().tab, PatternTab::Advanced);
    }
    let listed = names(&rig);
    assert_eq!(listed.len(), 3, "{listed:?}");
    for expected in ["Shared", "Shared (2)", "Shared (3)"] {
        assert!(listed.iter().any(|n| n == expected), "{listed:?}");
    }
    // The source is left alone.
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        valid_file("Shared")
    );
}

#[test]
fn importing_a_file_that_does_not_exist_or_is_a_folder_changes_nothing() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::RefreshSaved);
    let draft = rig.view().draft;
    for path in [
        dir.path().join("missing.toml"),
        dir.path().to_path_buf(),
        PathBuf::new(),
    ] {
        rig.pat(Pat::Import(path.clone()));
        let notice = issue(&rig).unwrap_or_else(|| panic!("{path:?} silent"));
        assert!(
            matches!(
                notice,
                PatternSearchNotice::ImportFailed | PatternSearchNotice::FileUnreadable
            ),
            "{path:?}: {notice:?}"
        );
        assert_eq!(rig.view().draft, draft);
        assert!(saved(&rig).is_empty());
        assert!(files_in(&folder(dir.path())).is_empty());
    }
}

#[test]
fn importing_a_well_formed_file_with_an_invalid_pattern_adds_nothing() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::RefreshSaved);
    let draft = rig.view().draft;
    let good = valid_file("X");
    let cases = [
        (
            "empty group",
            good.replace(
                "[[root.items]]\nkind = \"phase\"\nphase = \"endgame\"\n",
                "",
            ),
        ),
        ("empty name", good.replace("name = \"X\"", "name = \"\"")),
        (
            "long name",
            good.replace("name = \"X\"", &format!("name = \"{}\"", "n".repeat(81))),
        ),
        (
            "hold 99",
            good.replace("\n[root]", "\n[scope]\nhold = 99\n\n[root]"),
        ),
        (
            "reversed range",
            good.replace(
                "\n[root]",
                "\n[scope]\nmin_move = 9\nmax_move = 2\n\n[root]",
            ),
        ),
        ("bad phase", good.replace("endgame", "nowhere")),
        (
            "bad kind",
            good.replace("kind = \"phase\"", "kind = \"teleport\""),
        ),
        (
            "bad square",
            good.replace(
                "kind = \"phase\"\nphase = \"endgame\"",
                "kind = \"piece\"\nside = \"white\"\nregion = {squares = [\"z9\"]}",
            ),
        ),
        (
            "control char in name",
            good.replace("name = \"X\"", "name = \"a\\u0007b\""),
        ),
        (
            "duplicate key",
            good.replace("version = 1\n", "version = 1\nversion = 1\n"),
        ),
        (
            "format missing",
            good.replace("format = \"mychess-pattern\"\n", ""),
        ),
        (
            "wrong format",
            good.replace("mychess-pattern", "mychess-patterns"),
        ),
        ("version 0", good.replace("version = 1", "version = 0")),
        (
            "version huge",
            good.replace("version = 1", "version = 4294967296"),
        ),
        ("bom only", "\u{feff}".to_owned()),
        ("empty", String::new()),
        ("crlf", good.replace('\n', "\r\n")),
    ];
    let mut told = 0;
    for (label, text) in cases {
        let path = dir.path().join("in.toml");
        std::fs::write(&path, &text).unwrap();
        let before = files_in(&folder(dir.path()));
        rig.pat(Pat::Import(path));
        if label == "crlf" {
            // Line endings of any kind are accepted (multiplatform rules).
            assert!(
                matches!(issue(&rig), Some(PatternSearchNotice::Imported(_))),
                "crlf: {:?}",
                issue(&rig)
            );
            continue;
        }
        let notice = issue(&rig).unwrap_or_else(|| panic!("{label} silent"));
        assert!(
            !matches!(notice, PatternSearchNotice::Imported(_)),
            "{label}: imported"
        );
        assert_eq!(files_in(&folder(dir.path())), before, "{label}");
        assert_eq!(rig.view().draft, draft, "{label}");
        told += 1;
    }
    assert_eq!(told, 16);
}

#[test]
fn a_file_of_a_newer_version_is_told_with_its_own_key() {
    let (mut rig, dir) = fresh();
    let path = dir.path().join("future.toml");
    std::fs::write(&path, valid_file("F").replace("version = 1", "version = 2")).unwrap();
    rig.pat(Pat::Import(path));
    let notice = issue(&rig).unwrap();
    assert_eq!(notice, PatternSearchNotice::NewerVersion);
    assert_eq!(notice.message_key(), "pattern-error-newer-version");
    assert!(!notice.is_form_error());
    assert!(saved(&rig).is_empty());
}

#[test]
fn a_file_truncated_at_any_byte_never_imports_and_never_panics() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::RefreshSaved);
    let draft = rig.view().draft;
    let full = valid_file("Cut");
    let path = dir.path().join("cut.toml");
    let mut imported = 0;
    for len in 0..=full.len() {
        // Cut inside a UTF-8 sequence is impossible here (ASCII).
        std::fs::write(&path, &full.as_bytes()[..len]).unwrap();
        rig.pat(Pat::Import(path.clone()));
        if matches!(issue(&rig), Some(PatternSearchNotice::Imported(_))) {
            imported += 1;
            // A cut that still is a whole pattern (only the last newline
            // is lost) is fine; reset the folder for the next round.
            for entry in saved(&rig) {
                rig.pat(Pat::DeleteSavedFile(entry.path));
            }
            rig.pat(Pat::Edit(PatternEdit::Clear));
            rig.pat(Pat::Edit(PatternEdit::Add {
                parent: vec![],
                kind: ConditionKind::Phase,
            }));
        } else {
            assert_eq!(rig.view().draft, draft, "cut at {len}");
            assert!(files_in(&folder(dir.path())).is_empty(), "cut at {len}");
        }
    }
    // Only the complete file (and maybe its last newline) are patterns.
    assert!((1..=2).contains(&imported), "{imported}");
}

#[test]
fn a_hostile_oversized_or_deep_file_is_refused_without_reading_it_all() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::RefreshSaved);
    let draft = rig.view().draft;
    let path = dir.path().join("big.toml");
    let deep_open = "{kind = \"not\", item = ".repeat(5000);
    let deep_close = "}".repeat(5000);
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("2 MiB of comments", vec![b'#'; 2 * 1024 * 1024]),
        ("binary", (0..=255u8).cycle().take(5000).collect()),
        ("nul bytes", vec![0u8; 1000]),
        ("latin1", b"format = \"mychess-pattern\"\nname = \"caf\xe9\"\n".to_vec()),
        (
            "5000 nested tables",
            format!(
                "format = \"mychess-pattern\"\nversion = 1\nname = \"d\"\nroot = {deep_open}{{kind = \"phase\", phase = \"endgame\"}}{deep_close}\n"
            )
            .into_bytes(),
        ),
        (
            "huge array",
            format!(
                "format = \"mychess-pattern\"\nversion = 1\nname = \"a\"\nroot = {{kind = \"piece\", side = \"white\", region = {{squares = [{}]}}}}\n",
                "\"e4\",".repeat(100_000)
            )
            .into_bytes(),
        ),
    ];
    for (label, bytes) in files {
        std::fs::write(&path, &bytes).unwrap();
        rig.pat(Pat::Import(path.clone()));
        let notice = issue(&rig).unwrap_or_else(|| panic!("{label} silent"));
        assert!(
            !matches!(notice, PatternSearchNotice::Imported(_)),
            "{label}: imported"
        );
        assert_eq!(rig.view().draft, draft, "{label}");
        assert!(files_in(&folder(dir.path())).is_empty(), "{label}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn importing_arbitrary_bytes_never_panics_and_adds_only_whole_patterns(
        bytes in proptest::collection::vec(any::<u8>(), 0..400)
    ) {
        let (mut rig, dir) = fresh();
        let path = dir.path().join("fuzz.toml");
        std::fs::write(&path, &bytes).unwrap();
        let draft = rig.view().draft;
        rig.pat(Pat::Import(path));
        if matches!(issue(&rig), Some(PatternSearchNotice::Imported(_))) {
            prop_assert!(rig.view().tree_issue.is_none());
        } else {
            prop_assert_eq!(rig.view().draft, draft);
            prop_assert!(files_in(&folder(dir.path())).is_empty());
        }
    }

    #[test]
    fn a_file_with_a_random_byte_flipped_is_imported_whole_or_not_at_all(
        at in 0usize..210, byte in any::<u8>()
    ) {
        let (mut rig, dir) = fresh();
        let mut bytes = valid_file("Flip").into_bytes();
        let at = at % bytes.len();
        bytes[at] = byte;
        let path = dir.path().join("flip.toml");
        std::fs::write(&path, &bytes).unwrap();
        let draft = rig.view().draft;
        rig.pat(Pat::Import(path));
        match issue(&rig) {
            Some(PatternSearchNotice::Imported(_)) => {
                prop_assert!(rig.view().tree_issue.is_none());
                prop_assert_eq!(saved(&rig).len(), 1);
            }
            Some(_) => {
                prop_assert_eq!(rig.view().draft, draft);
                prop_assert!(files_in(&folder(dir.path())).is_empty());
            }
            None => prop_assert!(false, "silent"),
        }
    }
}

// ------------------------------------------------------------------ export

#[test]
fn exporting_needs_no_folder_and_leaves_the_folder_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.state.set_pattern_dir(None);
    rig.pat(Pat::Open);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Status,
    }));
    let target = dir.path().join("out.toml");
    rig.pat(Pat::Export {
        path: target.clone(),
        name: "Out".into(),
        description: "desc".into(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Exported));
    assert_eq!(
        issue(&rig).unwrap().message_key(),
        "pattern-notice-exported"
    );
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.starts_with("format = \"mychess-pattern\""), "{text}");
    assert!(text.contains("name = \"Out\""));
    // A file written by us has no carriage return and ends in a newline.
    assert!(!text.contains('\r'));
    assert!(text.ends_with('\n'));
    assert_eq!(
        files_in(dir.path()),
        BTreeSet::from(["out.toml".to_owned()])
    );
}

#[test]
fn exporting_to_a_place_that_cannot_be_written_is_told_and_creates_nothing() {
    let (mut rig, dir) = fresh();
    let targets = [
        dir.path().join("no-such-dir").join("x.toml"),
        dir.path().to_path_buf(),
        PathBuf::new(),
    ];
    for target in targets {
        rig.pat(Pat::Export {
            path: target.clone(),
            name: "X".into(),
            description: String::new(),
        });
        // An empty path is told as an invalid name by the store.
        let notice = issue(&rig).unwrap();
        assert!(
            notice == PatternSearchNotice::ExportFailed
                || (target.as_os_str().is_empty()
                    && notice == PatternSearchNotice::InvalidSavedName),
            "{target:?}: {notice:?}"
        );
        if notice == PatternSearchNotice::ExportFailed {
            assert_eq!(notice.message_key(), "pattern-notice-export-failed");
        }
    }
    assert!(!dir.path().join("no-such-dir").exists());
    assert!(files_in(&folder(dir.path())).is_empty());
}

#[test]
fn exporting_replaces_an_existing_file_atomically_and_leaves_no_temporary() {
    let (mut rig, dir) = fresh();
    let target = dir.path().join("e.toml");
    std::fs::write(&target, "old").unwrap();
    rig.pat(Pat::Export {
        path: target.clone(),
        name: "New".into(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Exported));
    assert!(
        std::fs::read_to_string(&target)
            .unwrap()
            .contains("name = \"New\"")
    );
    assert_eq!(files_in(dir.path()), BTreeSet::from(["e.toml".to_owned()]));
}

// ----------------------------------------------------------------- listing

#[test]
fn refreshing_picks_up_files_added_outside_and_orders_by_name_without_case() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::RefreshSaved);
    assert!(saved(&rig).is_empty());
    let patterns = folder(dir.path());
    std::fs::create_dir_all(&patterns).unwrap();
    for (file, name) in [
        ("b.toml", "bravo"),
        ("a.toml", "Alpha"),
        ("c.toml", "charlie"),
    ] {
        std::fs::write(patterns.join(file), valid_file(name)).unwrap();
    }
    std::fs::write(patterns.join("readme.md"), "x").unwrap();
    std::fs::write(patterns.join(".hidden.toml"), valid_file("hidden")).unwrap();
    rig.pat(Pat::RefreshSaved);
    let listed = saved(&rig);
    assert!(listed.iter().all(|s| !s.damaged), "{listed:?}");
    let got: Vec<&str> = listed.iter().map(|s| s.name.as_str()).collect();
    assert!(got.starts_with(&["Alpha", "bravo", "charlie"]), "{got:?}");
    assert!(!got.contains(&"readme"), "{got:?}");
    // Refreshing is stable.
    rig.pat(Pat::RefreshSaved);
    assert_eq!(saved(&rig), listed);
}

#[test]
fn a_damaged_entry_names_its_file_and_a_thousand_files_are_listed_at_most() {
    let (mut rig, dir) = fresh();
    let patterns = folder(dir.path());
    std::fs::create_dir_all(&patterns).unwrap();
    std::fs::write(patterns.join("broken.toml"), "= = =").unwrap();
    rig.pat(Pat::RefreshSaved);
    let entry = saved(&rig).remove(0);
    assert!(entry.damaged);
    assert!(entry.name.contains("broken"), "{entry:?}");
    assert_eq!(entry.path, patterns.join("broken.toml"));
    // Loading a damaged entry by its name is not a way in.
    let draft = rig.view().draft;
    rig.pat(Pat::LoadSaved(entry.name.clone()));
    assert_eq!(rig.view().draft, draft);
    assert!(issue(&rig).is_some());
    for i in 0..1100 {
        std::fs::write(
            patterns.join(format!("p{i:04}.toml")),
            valid_file(&format!("P{i}")),
        )
        .unwrap();
    }
    rig.pat(Pat::RefreshSaved);
    assert!(saved(&rig).len() <= 1000, "{}", saved(&rig).len());
}

#[test]
fn a_folder_that_does_not_exist_yet_lists_nothing_and_is_created_by_the_first_save() {
    let (mut rig, dir) = fresh();
    assert!(!folder(dir.path()).exists());
    rig.pat(Pat::RefreshSaved);
    assert!(saved(&rig).is_empty());
    assert!(issue(&rig).is_none());
    rig.pat(Pat::SaveAs {
        name: "First".into(),
        description: String::new(),
    });
    assert!(folder(dir.path()).is_dir());
    assert_eq!(names(&rig), ["First"]);
}

#[test]
fn the_folder_can_be_a_file_and_everything_fails_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    std::fs::write(dir.path().join("patterns"), "I am a file").unwrap();
    rig.pat(Pat::Open);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Phase,
    }));
    rig.pat(Pat::RefreshSaved);
    rig.pat(Pat::SaveAs {
        name: "X".into(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::SavedFailed));
    assert_eq!(
        issue(&rig).unwrap().message_key(),
        "pattern-notice-saved-failed"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("patterns")).unwrap(),
        "I am a file"
    );
    rig.pat(Pat::LoadSaved("X".into()));
    rig.pat(Pat::DeleteSaved("X".into()));
    assert!(rig.state.pattern_search_view().is_some());
}

#[test]
fn every_operation_without_a_folder_says_so_except_listing_and_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.state.set_pattern_dir(None);
    rig.pat(Pat::Open);
    assert!(!rig.view().can_save);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Phase,
    }));
    let src = dir.path().join("s.toml");
    std::fs::write(&src, valid_file("S")).unwrap();
    let ops = [
        Pat::SaveAs {
            name: "n".into(),
            description: String::new(),
        },
        Pat::LoadSaved("n".into()),
        Pat::DeleteSaved("n".into()),
        Pat::DeleteSavedFile(src.clone()),
        Pat::Import(src.clone()),
    ];
    for op in ops {
        rig.pat(Pat::Close);
        rig.pat(Pat::Open);
        let effects = rig.raw(Message::PatternSearch(op.clone()));
        assert!(effects.is_empty(), "{op:?}: {effects:?}");
        let notice = issue(&rig).unwrap_or_else(|| panic!("{op:?} silent"));
        assert_eq!(notice, PatternSearchNotice::NoSavedFolder, "{op:?}");
        assert_eq!(notice.message_key(), "pattern-notice-no-saved-folder");
        assert!(!notice.is_form_error());
    }
    assert!(src.exists());
    // Listing is silent and export works.
    assert!(
        rig.raw(Message::PatternSearch(Pat::RefreshSaved))
            .is_empty()
    );
    rig.pat(Pat::Export {
        path: dir.path().join("e.toml"),
        name: "E".into(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Exported));
}

// ------------------------------------------------------------------ notices

#[test]
fn the_notices_have_distinct_keys_and_only_form_errors_are_marked() {
    let invalid = PatternIssue::InvalidValue { path: "p".into() };
    let all: Vec<(PatternSearchNotice, &str, bool)> = vec![
        (
            PatternSearchNotice::InvalidPattern(invalid.clone()),
            "pattern-error-invalid",
            true,
        ),
        (
            PatternSearchNotice::InvalidParam("n".into()),
            "pattern-error-invalid-param",
            true,
        ),
        (
            PatternSearchNotice::UnknownPreset("x".into()),
            "pattern-notice-unknown-preset",
            true,
        ),
        (
            PatternSearchNotice::EditRefused(EditRefusal::TooDeep),
            "pattern-edit-refused-too-deep",
            true,
        ),
        (
            PatternSearchNotice::NoDatabase,
            "pattern-notice-no-database",
            false,
        ),
        (PatternSearchNotice::Stale, "pattern-notice-stale", false),
        (
            PatternSearchNotice::Interrupted,
            "pattern-interrupted",
            false,
        ),
        (
            PatternSearchNotice::Unreadable { count: 1 },
            "pattern-unreadable",
            false,
        ),
        (
            PatternSearchNotice::UnknownPositions { count: 1 },
            "pattern-unknown",
            false,
        ),
        (PatternSearchNotice::Failed, "pattern-notice-failed", false),
        (
            PatternSearchNotice::HitsFailed,
            "pattern-notice-hits-failed",
            false,
        ),
        (
            PatternSearchNotice::NoSavedFolder,
            "pattern-notice-no-saved-folder",
            false,
        ),
        (
            PatternSearchNotice::Saved("a".into()),
            "pattern-notice-saved",
            false,
        ),
        (
            PatternSearchNotice::Deleted("a".into()),
            "pattern-notice-deleted",
            false,
        ),
        (
            PatternSearchNotice::SavedNotFound("a".into()),
            "pattern-notice-saved-not-found",
            false,
        ),
        (
            PatternSearchNotice::InvalidSavedName,
            "pattern-notice-invalid-saved-name",
            false,
        ),
        (
            PatternSearchNotice::SavedFailed,
            "pattern-notice-saved-failed",
            false,
        ),
        (
            PatternSearchNotice::Imported("a".into()),
            "pattern-notice-imported",
            false,
        ),
        (
            PatternSearchNotice::ImportFailed,
            "pattern-notice-import-failed",
            false,
        ),
        (
            PatternSearchNotice::Exported,
            "pattern-notice-exported",
            false,
        ),
        (
            PatternSearchNotice::ExportFailed,
            "pattern-notice-export-failed",
            false,
        ),
        (
            PatternSearchNotice::FileUnreadable,
            "pattern-error-file",
            false,
        ),
        (
            PatternSearchNotice::NewerVersion,
            "pattern-error-newer-version",
            false,
        ),
    ];
    let mut seen = BTreeSet::new();
    for (notice, key, form) in &all {
        assert_eq!(notice.message_key(), *key, "{notice:?}");
        assert_eq!(notice.is_form_error(), *form, "{notice:?}");
        assert!(seen.insert(*key), "duplicate key {key}");
    }
    // The key of an invalid pattern follows the kind of problem.
    for (issue, key) in [
        (
            PatternIssue::EmptyGroup { path: "p".into() },
            "pattern-error-empty-group",
        ),
        (PatternIssue::TooDeep { max: 6 }, "pattern-error-too-deep"),
        (
            PatternIssue::TooManyNodes { max: 48 },
            "pattern-error-too-many-nodes",
        ),
        (
            PatternIssue::MoverNotAllowed { path: "p".into() },
            "pattern-error-mover-not-allowed",
        ),
        (PatternIssue::Other, "pattern-error-invalid"),
    ] {
        assert_eq!(
            PatternSearchNotice::InvalidPattern(issue).message_key(),
            key
        );
    }
    // And the refusals of an edit are ten different keys.
    let refusals = [
        EditRefusal::BadPath,
        EditRefusal::NotAGroup,
        EditRefusal::RootCannotBeRemoved,
        EditRefusal::TooDeep,
        EditRefusal::TooManyNodes,
        EditRefusal::GroupFull,
        EditRefusal::MoveIntoItself,
        EditRefusal::NeedsOneItem,
        EditRefusal::NoSuchRegion,
        EditRefusal::NoMarks,
    ];
    let keys: BTreeSet<_> = refusals.iter().map(|r| r.message_key()).collect();
    assert_eq!(keys.len(), 10);
    assert!(keys.iter().all(|k| k.starts_with("pattern-edit-refused-")));
}

#[test]
fn a_notice_goes_to_both_the_window_and_the_status_bar() {
    let (mut rig, _dir) = fresh();
    rig.pat(Pat::SaveAs {
        name: "Both".into(),
        description: String::new(),
    });
    let notice = PatternSearchNotice::Saved("Both".into());
    assert_eq!(issue(&rig), Some(notice.clone()));
    assert_eq!(rig.state.notice, Some(AppNotice::PatternSearch(notice)));
    // Changing tab clears the line of the window, not the history of use.
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    assert!(issue(&rig).is_none());
}
