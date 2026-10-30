//! Contract tests of the user's patterns (spec `m6-2-taticas-e-padroes.md`:
//! D3277, D3278, D3276, rule 23): save, list, load, delete, import and
//! export through effects, in a folder the test chooses; hostile files.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_2c_common;

use std::path::Path;

use m6_2c_common::*;
use mychess_app::pattern_search::model::{RoleName, Scope};
use mychess_app::{ConditionKind, GroupKind, ParamValue, PatternEdit, PatternIssue, SavedPattern};
use mychess_core::Square;

fn fresh() -> (Rig, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.pat(Pat::Open);
    (rig, dir)
}

fn patterns(dir: &Path) -> std::path::PathBuf {
    dir.join("patterns")
}

fn issue(rig: &Rig) -> Option<PatternSearchNotice> {
    rig.view().issue
}

/// A custom pattern with an AND, an OR, a NOT and several kinds of card.
fn build_custom(rig: &mut Rig) {
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    for kind in [
        ConditionKind::Pins,
        ConditionKind::Any,
        ConditionKind::Phase,
        ConditionKind::Mate,
    ] {
        rig.pat(Pat::Edit(PatternEdit::Add {
            parent: vec![],
            kind,
        }));
    }
    rig.pat(Pat::Edit(PatternEdit::Wrap {
        path: vec![2],
        kind: GroupKind::Not,
    }));
    rig.pat(Pat::Edit(PatternEdit::ToggleSquare {
        path: vec![0],
        slot: mychess_app::RegionSlot::Actor,
        square: Square::E4,
    }));
    rig.pat(Pat::SetScope(Scope {
        min_move: 3,
        max_move: 40,
        hold: 2,
        end_only: false,
        ignore_colors: true,
    }));
}

/// A file whose tree is seven levels deep (the limit is six).
fn deep_file() -> Vec<u8> {
    let open = "{kind = \"not\", item = ".repeat(7);
    let close = "}".repeat(7);
    format!(
        "format = \"mychess-pattern\"\nversion = 1\nname = \"d\"\nroot = {open}{{kind = \"phase\", phase = \"endgame\"}}{close}\n"
    )
    .into_bytes()
}

fn saved(rig: &Rig) -> Vec<SavedPattern> {
    rig.view().saved
}

// ------------------------------------------------------------ round trip

#[test]
fn a_custom_pattern_saves_lists_and_loads_back_equal() {
    let (mut rig, dir) = fresh();
    assert!(saved(&rig).is_empty());
    build_custom(&mut rig);
    let original = rig.view().draft;
    rig.pat(Pat::SaveAs {
        name: "Minha tática".into(),
        description: "Descrição escrita pelo usuário".into(),
    });
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::Saved("Minha tática".into()))
    );
    assert_eq!(issue(&rig).unwrap().message_key(), "pattern-notice-saved");
    // One TOML file in the folder, and the list shows it.
    let files: Vec<_> = std::fs::read_dir(patterns(dir.path()))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension().and_then(|e| e.to_str()), Some("toml"));
    let list = saved(&rig);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Minha tática");
    assert_eq!(list[0].description, "Descrição escrita pelo usuário");
    assert!(!list[0].damaged);
    // Clear the editor, then load it back.
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::LoadSaved("Minha tática".into()));
    let view = rig.view();
    assert_eq!(view.tab, PatternTab::Advanced);
    let mut want = original;
    want.name = "Minha tática".into();
    want.description = "Descrição escrita pelo usuário".into();
    assert_eq!(view.draft, want);
    // The scope came back with the pattern.
    assert_eq!(view.scope, want.scope);
    assert!(view.tree_issue.is_none());
}

#[test]
fn a_preset_is_saved_with_the_translated_name_and_description() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::SelectPreset("fork".into()));
    rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
    rig.pat(Pat::SetParam(ParamValue::Roles(vec![RoleName::Knight])));
    rig.pat(Pat::SaveAs {
        name: "Garfo de cavalo (brancas)".into(),
        description: "Um cavalo branco ataca duas peças.".into(),
    });
    let file = mychess_db::load_pattern(
        &patterns(dir.path()).join(
            std::fs::read_dir(patterns(dir.path()))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .file_name(),
        ),
    )
    .unwrap();
    assert_eq!(file.pattern.name, "Garfo de cavalo (brancas)");
    assert_eq!(
        file.pattern.description,
        "Um cavalo branco ataca duas peças."
    );
    let built = mychess_db::pattern::presets::build(
        "fork",
        &Params {
            by: Who::White,
            roles: vec![RoleName::Knight],
            ..Params::default()
        },
    )
    .unwrap();
    assert_eq!(file.pattern.root, built.root);
    // Saving again under the same name replaces it.
    rig.pat(Pat::SaveAs {
        name: "Garfo de cavalo (brancas)".into(),
        description: "Outra descrição".into(),
    });
    let list = saved(&rig);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].description, "Outra descrição");
}

#[test]
fn nothing_is_saved_without_a_name_or_with_an_invalid_pattern() {
    let (mut rig, dir) = fresh();
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    // An empty group is not a pattern.
    let effects = rig.raw(Message::PatternSearch(Pat::SaveAs {
        name: "x".into(),
        description: String::new(),
    }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::InvalidPattern(
            PatternIssue::EmptyGroup {
                path: "root.items".into()
            }
        ))
    );
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Piece,
    }));
    for name in ["", "   ", &"n".repeat(81)] {
        let effects = rig.raw(Message::PatternSearch(Pat::SaveAs {
            name: name.into(),
            description: String::new(),
        }));
        assert!(effects.is_empty(), "{name:?}: {effects:?}");
        assert_eq!(issue(&rig), Some(PatternSearchNotice::InvalidSavedName));
    }
    // A description over the limit is a value error, not a name error.
    let effects = rig.raw(Message::PatternSearch(Pat::SaveAs {
        name: "ok".into(),
        description: "d".repeat(501),
    }));
    assert!(effects.is_empty());
    assert!(matches!(
        issue(&rig),
        Some(PatternSearchNotice::InvalidPattern(
            PatternIssue::InvalidValue { .. }
        ))
    ));
    assert!(!patterns(dir.path()).exists());
}

#[test]
fn without_a_folder_saving_tells_and_listing_is_silent() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.state.set_pattern_dir(None);
    assert!(rig.raw(Message::PatternSearch(Pat::Open)).is_empty());
    assert!(issue(&rig).is_none());
    assert!(!rig.view().can_save);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Piece,
    }));
    let effects = rig.raw(Message::PatternSearch(Pat::SaveAs {
        name: "x".into(),
        description: String::new(),
    }));
    assert!(effects.is_empty());
    assert_eq!(issue(&rig), Some(PatternSearchNotice::NoSavedFolder));
}

// ------------------------------------------------------------------ delete

#[test]
fn deleting_removes_the_file_and_a_missing_name_is_told() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    rig.pat(Pat::SaveAs {
        name: "A".into(),
        description: String::new(),
    });
    rig.pat(Pat::DeleteSaved("B".into()));
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::SavedNotFound("B".into()))
    );
    assert_eq!(saved(&rig).len(), 1);
    rig.pat(Pat::DeleteSaved("A".into()));
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Deleted("A".into())));
    assert!(saved(&rig).is_empty());
    assert_eq!(std::fs::read_dir(patterns(dir.path())).unwrap().count(), 0);
    rig.pat(Pat::LoadSaved("A".into()));
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::SavedNotFound("A".into()))
    );
}

// --------------------------------------------------------- hostile files

#[test]
fn damaged_and_hostile_files_are_listed_as_damaged_and_never_loaded() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    rig.pat(Pat::SaveAs {
        name: "Good".into(),
        description: String::new(),
    });
    let folder = patterns(dir.path());
    std::fs::write(folder.join("binary.toml"), [0u8, 159, 146, 150, 0]).unwrap();
    std::fs::write(folder.join("truncated.toml"), "format = \"mychess-patt").unwrap();
    std::fs::write(
        folder.join("other.toml"),
        "format = \"other\"\nversion = 1\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("future.toml"),
        "format = \"mychess-pattern\"\nversion = 99\nname = \"f\"\n[root]\nkind = \"all\"\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("unknown.toml"),
        "format = \"mychess-pattern\"\nversion = 1\nname = \"u\"\nsurprise = 1\n[root]\nkind = \"phase\"\nphase = \"endgame\"\n",
    )
    .unwrap();
    std::fs::write(folder.join("huge.toml"), vec![b'#'; 300 * 1024]).unwrap();
    std::fs::write(folder.join("notes.txt"), "not a pattern").unwrap();
    std::fs::create_dir(folder.join("dir.toml")).unwrap();
    rig.pat(Pat::RefreshSaved);
    let list = saved(&rig);
    let good: Vec<_> = list.iter().filter(|s| !s.damaged).collect();
    let bad: Vec<_> = list.iter().filter(|s| s.damaged).collect();
    assert_eq!(good.len(), 1);
    assert_eq!(good[0].name, "Good");
    assert_eq!(bad.len(), 6, "{bad:?}");
    // The good one still loads; the others are not reachable by name.
    let draft = rig.view().draft;
    rig.pat(Pat::LoadSaved("Good".into()));
    assert!(issue(&rig).is_none());
    assert_eq!(rig.view().draft.name, "Good");
    assert_ne!(rig.view().draft, draft);
    rig.pat(Pat::LoadSaved("future".into()));
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::SavedNotFound("future".into()))
    );
    // A damaged file can be deleted by its path, and only a .toml inside
    // the folder.
    let victim = bad.iter().find(|s| s.name.contains("truncated")).unwrap();
    rig.pat(Pat::DeleteSavedFile(victim.path.clone()));
    assert!(!victim.path.exists());
    rig.pat(Pat::DeleteSavedFile(folder.join("notes.txt")));
    assert!(folder.join("notes.txt").exists());
    assert_eq!(issue(&rig), Some(PatternSearchNotice::InvalidSavedName));
}

#[test]
fn importing_hostile_files_fails_cleanly_and_changes_nothing() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    rig.pat(Pat::SaveAs {
        name: "Good".into(),
        description: String::new(),
    });
    let draft = rig.view().draft;
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let cases: Vec<(&str, Vec<u8>, PatternSearchNotice)> = vec![
        ("bin.toml", vec![0, 1, 2, 255], PatternSearchNotice::ImportFailed),
        (
            "other.toml",
            b"format = \"x\"\nversion = 1\n".to_vec(),
            PatternSearchNotice::ImportFailed,
        ),
        (
            "future.toml",
            b"format = \"mychess-pattern\"\nversion = 2\nname = \"f\"\n[root]\nkind = \"all\"\n"
                .to_vec(),
            PatternSearchNotice::NewerVersion,
        ),
        ("deep.toml", deep_file(), PatternSearchNotice::InvalidPattern(PatternIssue::TooDeep { max: 6 })),
        ("missing_items.toml", b"format = \"mychess-pattern\"\nversion = 1\nname = \"d\"\n[root]\nkind = \"all\"\n[[root.items]]\nkind = \"all\"\n".to_vec(), PatternSearchNotice::ImportFailed),
        ("huge.toml", vec![b'a'; 300 * 1024], PatternSearchNotice::ImportFailed),
    ];
    for (name, bytes, want) in cases {
        let path = outside.join(name);
        std::fs::write(&path, bytes).unwrap();
        rig.pat(Pat::Import(path));
        let got = issue(&rig).expect(name);
        assert_eq!(got, want, "{name}");
        assert_eq!(rig.view().draft, draft, "{name}");
    }
    assert_eq!(saved(&rig).len(), 1);
    // A path that does not exist.
    rig.pat(Pat::Import(outside.join("missing.toml")));
    assert_eq!(issue(&rig), Some(PatternSearchNotice::ImportFailed));
}

// ------------------------------------------------------ import and export

#[test]
fn export_then_import_gives_the_same_pattern_and_never_replaces() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    let original = rig.view().draft;
    rig.pat(Pat::SaveAs {
        name: "Tema".into(),
        description: "d".into(),
    });
    let out = dir.path().join("exported.toml");
    rig.pat(Pat::Export {
        path: out.clone(),
        name: "Tema".into(),
        description: "d".into(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Exported));
    let file = mychess_db::load_pattern(&out).unwrap();
    assert_eq!(file.pattern.root, original.root);
    assert_eq!(file.pattern.scope, original.scope);
    // Importing a pattern whose name is taken adds " (2)".
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Import(out.clone()));
    assert_eq!(
        issue(&rig),
        Some(PatternSearchNotice::Imported("Tema (2)".into()))
    );
    let names: Vec<_> = saved(&rig).into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Tema", "Tema (2)"]);
    // The imported pattern is shown in the editor.
    let view = rig.view();
    assert_eq!(view.draft.name, "Tema (2)");
    assert_eq!(view.draft.root, original.root);
    // And the first one is intact.
    rig.pat(Pat::LoadSaved("Tema".into()));
    assert_eq!(rig.view().draft.name, "Tema");
}

#[test]
fn exporting_with_no_name_keeps_the_own_name_and_a_bad_target_is_told() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    rig.pat(Pat::Edit(PatternEdit::SetMeta {
        name: "Own".into(),
        description: String::new(),
    }));
    let out = dir.path().join("o.toml");
    rig.pat(Pat::Export {
        path: out.clone(),
        name: String::new(),
        description: String::new(),
    });
    assert_eq!(mychess_db::load_pattern(&out).unwrap().pattern.name, "Own");
    // A directory cannot be written over.
    rig.pat(Pat::Export {
        path: dir.path().to_path_buf(),
        name: "x".into(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::ExportFailed));
    // Exporting needs no folder of patterns.
    rig.state.set_pattern_dir(None);
    rig.pat(Pat::Export {
        path: dir.path().join("p.toml"),
        name: "x".into(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Exported));
    // A pattern that is not valid is not exported.
    rig.pat(Pat::Edit(PatternEdit::Clear));
    let effects = rig.raw(Message::PatternSearch(Pat::Export {
        path: dir.path().join("q.toml"),
        name: "x".into(),
        description: String::new(),
    }));
    assert!(effects.is_empty());
    assert!(!dir.path().join("q.toml").exists());
}

#[test]
fn an_old_answer_of_the_saved_operations_is_dropped() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    // Two listings in flight: only the last one counts.
    let first = rig.raw(Message::PatternSearch(Pat::RefreshSaved));
    std::fs::create_dir_all(patterns(dir.path())).unwrap();
    let second = rig.raw(Message::PatternSearch(Pat::RefreshSaved));
    let late = rig.perform(first.into_iter().next().unwrap());
    let current = rig.perform(second.into_iter().next().unwrap());
    rig.raw(current);
    let count = saved(&rig).len();
    // Same content, but the late one must not even be looked at.
    let before = rig.view();
    rig.raw(late);
    assert_eq!(rig.view(), before);
    assert_eq!(saved(&rig).len(), count);
}

#[test]
fn an_unnamed_pattern_is_never_exported_under_the_placeholder_of_a_run() {
    let (mut rig, dir) = fresh();
    build_custom(&mut rig);
    let out = dir.path().join("none.toml");
    let effects = rig.raw(Message::PatternSearch(Pat::Export {
        path: out.clone(),
        name: "  ".into(),
        description: String::new(),
    }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(issue(&rig), Some(PatternSearchNotice::InvalidSavedName));
    assert!(!out.exists());
    // A preset has a name of its own, so it exports without one.
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pat(Pat::Export {
        path: out.clone(),
        name: String::new(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::Exported));
    assert!(out.exists());
}

#[test]
fn a_target_without_a_file_name_is_an_export_failure_not_a_name_error() {
    let (mut rig, _dir) = fresh();
    build_custom(&mut rig);
    rig.pat(Pat::Export {
        path: std::path::PathBuf::from(".."),
        name: "x".into(),
        description: String::new(),
    });
    assert_eq!(issue(&rig), Some(PatternSearchNotice::ExportFailed));
}
