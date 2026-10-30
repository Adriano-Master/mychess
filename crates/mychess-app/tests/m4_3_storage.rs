//! M4-3 contract tests: `engines.toml` on disk (rules 1-3, D424, D686-D689),
//! the `[analysis]` preferences (D431, D726) and the load/save effects.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_3_common;

use std::path::Path;

use m4_3_common::*;
use mychess_app::engines::{
    EngineDraft, EngineList, EngineMessage, EngineNotice, Hotkey, LoadIssue,
    MAX_ENGINES_FILE_BYTES, StoreError, backup_path, corrupt_path, load_engine_file,
    load_engine_list, render_engine_list, save_engine_file,
};
use mychess_app::{
    AnalysisPrefs, AppState, Effect, MAX_INFO_INTERVAL_MS, MAX_LOG_LINES, MAX_PV_PLY, Message,
    Preferences, load_preferences, render_preferences, update,
};
use mychess_engine::Protocol;

fn sample() -> EngineList {
    let mut list = EngineList::new();
    let mut d = EngineDraft::new("Stockfish 16 \"x\"", "/opt/sf/stockfish");
    d.args = vec![
        "--threads".into(),
        "a b".into(),
        String::new(),
        "ü\\".into(),
    ];
    d.working_dir = Some("/opt/sf".into());
    d.elo = Some(3600);
    d.url = Some("https://stockfishchess.org".into());
    d.last_used = Some(1_700_000_123);
    d.options = vec![
        ("Hash".into(), "256".into()),
        ("Weird Name".into(), "multi\nline \"v\"".into()),
    ];
    d.multipv = 4;
    d.low_priority = Some(true);
    let a = list.add(d).unwrap();
    let mut d2 = EngineDraft::new("Crafty", r"C:\Program Files\Crafty\crafty.exe");
    d2.protocol = Protocol::XBoard;
    let b = list.add(d2).unwrap();
    list.add(EngineDraft::new("日本語エンジン", "e")).unwrap();
    list.set_hotkey(a, Some(Hotkey::F3)).unwrap();
    list.set_hotkey(b, Some(Hotkey::F2)).unwrap();
    list
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn render_then_load_round_trips_every_field_keys_and_the_next_id() {
    let list = sample();
    let text = render_engine_list(&list);
    let loaded = load_engine_list(&text).unwrap();
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(loaded.list, list);
    assert!(text.contains("version = 1"));
    // And a second round gives the same text.
    assert_eq!(render_engine_list(&loaded.list), text);
}

#[test]
fn a_missing_file_is_an_empty_list_without_remarks_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    let loaded = load_engine_file(&path).unwrap();
    assert!(loaded.list.is_empty() && loaded.issues.is_empty());
    assert!(dir_names(dir.path()).is_empty());
}

#[test]
fn saving_creates_missing_directories_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a").join("b").join("engines.toml");
    save_engine_file(&path, &sample()).unwrap();
    assert_eq!(dir_names(&dir.path().join("a").join("b")), ["engines.toml"]);
    assert_eq!(load_engine_file(&path).unwrap().list, sample());
    // Saving again keeps the previous file as engines.bak and no stray files.
    let mut changed = sample();
    changed.add(EngineDraft::new("Extra", "x")).unwrap();
    save_engine_file(&path, &changed).unwrap();
    assert_eq!(
        dir_names(&dir.path().join("a").join("b")),
        ["engines.bak", "engines.toml"]
    );
    let bak = std::fs::read_to_string(backup_path(&path)).unwrap();
    assert_eq!(load_engine_list(&bak).unwrap().list, sample());
}

#[test]
fn a_broken_toml_file_is_kept_as_engines_corrupt_and_is_not_overwritten_by_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    let garbage = b"[[engine\nname = = oops";
    std::fs::write(&path, garbage).unwrap();
    assert_eq!(load_engine_file(&path).unwrap_err(), StoreError::Syntax);
    assert_eq!(std::fs::read(corrupt_path(&path)).unwrap(), garbage);
    assert_eq!(std::fs::read(&path).unwrap(), garbage);
    assert_eq!(corrupt_path(&path).file_name().unwrap(), "engines.corrupt");
    // The next save does not turn the corrupt file into engines.bak.
    save_engine_file(&path, &sample()).unwrap();
    assert!(
        !backup_path(&path).exists(),
        "corrupt bytes must not become the backup"
    );
    assert_eq!(std::fs::read(corrupt_path(&path)).unwrap(), garbage);
}

#[test]
fn a_file_that_is_not_utf8_is_refused_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    let bytes = [b'v', b'e', 0xff, 0xfe, 0x80, b'\n'];
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(load_engine_file(&path).unwrap_err(), StoreError::NotUtf8);
    assert_eq!(std::fs::read(corrupt_path(&path)).unwrap(), bytes);
}

#[test]
fn a_file_larger_than_4_mib_is_refused_without_reading_it_all_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    let big = vec![b'#'; usize::try_from(MAX_ENGINES_FILE_BYTES).unwrap() + 10];
    std::fs::write(&path, &big).unwrap();
    assert!(matches!(
        load_engine_file(&path).unwrap_err(),
        StoreError::TooLarge { .. }
    ));
    assert!(corrupt_path(&path).exists());
}

#[test]
fn saving_a_list_over_4_mib_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    std::fs::write(&path, "version = 1\n").unwrap();
    let mut list = EngineList::new();
    for i in 0..3 {
        let mut d = EngineDraft::new(format!("E{i}"), "e");
        d.args = (0..512).map(|_| "z".repeat(4096)).collect();
        list.add(d).unwrap();
    }
    assert!(matches!(
        save_engine_file(&path, &list).unwrap_err(),
        StoreError::TooLarge { .. }
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "version = 1\n");
    assert_eq!(dir_names(dir.path()), ["engines.toml"]);
}

#[test]
fn a_newer_version_is_refused_untouched_then_backed_up_once_as_bak_v_n_on_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    let future = "version = 7\n[[engine]]\nname = \"F\"\ncommand = \"f\"\nnew_field = 1\n";
    std::fs::write(&path, future).unwrap();
    assert_eq!(
        load_engine_file(&path).unwrap_err(),
        StoreError::NewerVersion { found: 7 }
    );
    assert!(!corrupt_path(&path).exists(), "a newer file is not corrupt");
    save_engine_file(&path, &sample()).unwrap();
    let backup = dir.path().join("engines.toml.bak-v7");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), future);
    // A second save does not overwrite that backup nor create engines.bak from it.
    save_engine_file(&path, &sample()).unwrap();
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), future);
    assert!(load_engine_file(&path).is_ok());
}

#[test]
fn a_bom_and_crlf_line_endings_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.toml");
    let mut list = sample();
    let first = list.engines()[0].id;
    list.set_options(first, vec![("Hash".into(), "256".into())])
        .unwrap();
    let text = render_engine_list(&list).replace('\n', "\r\n");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(load_engine_file(&path).unwrap().list, list);
}

#[test]
fn rule_3_bad_fields_reset_only_the_field_and_unknown_keys_are_ignored() {
    let text = r#"
version = 1
future_top_level = "x"
[[engine]]
name = "A"
command = "a"
elo = 99999
args = 5
options = "nope"
protocol = "alien"
multipv = "many"
low_priority = "yes"
unknown_key = [1, 2]
hotkey = "F9"
[[engine]]
name = "B"
command = "b"
elo = "abc"
url = 12
last_used = "yesterday"
working_dir = 3
"#;
    let loaded = load_engine_list(text).unwrap();
    assert_eq!(loaded.list.len(), 2);
    let a = &loaded.list.engines()[0];
    let b = &loaded.list.engines()[1];
    assert_eq!(
        (a.name.as_str(), a.elo, a.args.len(), a.options.len()),
        ("A", None, 0, 0)
    );
    assert_eq!(
        a.protocol,
        Protocol::Uci,
        "bad protocol resets to the default"
    );
    assert_eq!(a.multipv, 1);
    assert_eq!(a.low_priority, None);
    assert_eq!(
        (b.elo, b.url.clone(), b.last_used, b.working_dir.clone()),
        (None, None, None, None)
    );
    assert!(
        loaded
            .issues
            .iter()
            .filter(|i| matches!(i, LoadIssue::FieldReset { .. }))
            .count()
            >= 8,
        "{:?}",
        loaded.issues
    );
    // Elo in range survives, 4000 exactly and 0 as unknown.
    for (text, want) in [("4000", Some(4000)), ("0", None), ("4001", None)] {
        let t = format!("[[engine]]\nname=\"n\"\ncommand=\"c\"\nelo={text}\n");
        let l = load_engine_list(&t).unwrap();
        assert_eq!(l.list.engines()[0].elo, want, "elo {text}");
    }
}

#[test]
fn entries_without_name_or_command_are_dropped_with_an_issue() {
    let text = r#"
[[engine]]
command = "c"
[[engine]]
name = "   "
command = "c"
[[engine]]
name = "n"
[[engine]]
name = "n"
command = ""
[[engine]]
name = "ok"
command = "ok"
"#;
    let loaded = load_engine_list(text).unwrap();
    assert_eq!(loaded.list.len(), 1);
    let dropped = loaded
        .issues
        .iter()
        .filter(|i| matches!(i, LoadIssue::EntryDropped { .. }))
        .count();
    assert_eq!(dropped, 4);
}

#[test]
fn duplicate_hotkeys_keep_the_first_and_duplicate_ids_get_fresh_ones() {
    let text = r#"
version = 1
[[engine]]
id = 3
name = "first"
command = "a"
hotkey = "F2"
[[engine]]
id = 3
name = "second"
command = "b"
hotkey = "F2"
[[engine]]
name = "third"
command = "c"
"#;
    let loaded = load_engine_list(text).unwrap();
    let l = &loaded.list;
    assert_eq!(l.len(), 3);
    let ids: Vec<_> = l.engines().iter().map(|e| e.id).collect();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    assert_eq!(l.hotkey_owner(Hotkey::F2), Some(ids[0]));
    assert_eq!(l.hotkey_of(ids[1]), None);
    assert!(
        loaded
            .issues
            .iter()
            .any(|i| matches!(i, LoadIssue::DuplicateHotkey { .. }))
    );
    assert!(l.next_id() > ids.iter().map(|i| i.0).max().unwrap());
}

#[test]
fn header_problems_are_syntax_errors_and_the_missing_version_means_1() {
    for bad in [
        "version = \"1\"",
        "version = 0",
        "version = -1",
        "engine = 5",
        "engine = \"x\"",
        "= =",
    ] {
        assert_eq!(
            load_engine_list(bad).unwrap_err(),
            StoreError::Syntax,
            "{bad}"
        );
    }
    assert_eq!(
        load_engine_list("version = 2").unwrap_err(),
        StoreError::NewerVersion { found: 2 }
    );
    assert!(load_engine_list("").unwrap().list.is_empty());
    assert!(load_engine_list("[[engine]]\nname=\"a\"\ncommand=\"b\"\n").is_ok());
    // An entry that is not a table is skipped, not fatal.
    let l = load_engine_list("engine = [1, 2, {name=\"a\", command=\"b\"}]").unwrap();
    assert_eq!(l.list.len(), 1);
    assert_eq!(l.issues.len(), 2);
}

#[test]
fn more_than_256_entries_in_the_file_load_256_and_report_it() {
    let mut text = String::from("version = 1\n");
    for i in 0..300 {
        text.push_str(&format!("[[engine]]\nname = \"e{i}\"\ncommand = \"c\"\n"));
    }
    let loaded = load_engine_list(&text).unwrap();
    assert_eq!(loaded.list.len(), 256);
    assert!(
        loaded
            .issues
            .iter()
            .any(|i| matches!(i, LoadIssue::TooManyEngines { limit: 256 }))
    );
}

#[test]
fn deeply_nested_or_huge_toml_does_not_overflow_the_stack() {
    let nested = format!("a = {}{}", "[".repeat(5000), "]".repeat(5000));
    let r = load_engine_list(&nested);
    assert!(r.is_ok() || r.is_err()); // any outcome but a crash
    let nested_tables = format!("engine = {}", "[".repeat(3000));
    assert!(load_engine_list(&nested_tables).is_err());
}

#[tokio::test]
async fn load_and_save_effects_work_end_to_end_through_update_and_perform() {
    let dir = tempfile::tempdir().unwrap();
    let prefs = temp_prefs_path(dir.path());
    let mut state = AppState::with_preferences_path(Some(prefs.clone()));
    state.engines = sample();
    let third = state.engines.engines()[2].id;
    let fx = send(
        &mut state,
        EngineMessage::SetHotkey(third, Some(Hotkey::F4)),
    );
    settle(&mut state, fx).await;
    assert!(dir.path().join("cfg").join("engines.toml").is_file());

    let mut other = AppState::with_preferences_path(Some(prefs));
    let fx = update(&mut other, Message::Startup);
    let load: Vec<Effect> = fx
        .into_iter()
        .filter(|e| matches!(e, Effect::LoadEngines { .. }))
        .collect();
    assert_eq!(load.len(), 1);
    settle(&mut other, load).await;
    assert_eq!(other.engines, state.engines);
    assert!(notice(&other).is_none());
}

#[tokio::test]
async fn a_corrupt_file_at_startup_gives_an_empty_list_a_warning_and_a_corrupt_copy() {
    let dir = tempfile::tempdir().unwrap();
    let prefs = temp_prefs_path(dir.path());
    std::fs::create_dir_all(prefs.parent().unwrap()).unwrap();
    let engines = prefs.with_file_name("engines.toml");
    std::fs::write(&engines, "][ not toml").unwrap();
    let mut state = AppState::with_preferences_path(Some(prefs));
    let fx = update(&mut state, Message::Startup);
    let load: Vec<Effect> = fx
        .into_iter()
        .filter(|e| matches!(e, Effect::LoadEngines { .. }))
        .collect();
    settle(&mut state, load).await;
    assert!(state.engines.is_empty());
    assert_eq!(notice(&state), Some(&EngineNotice::FileUnreadable));
    assert!(corrupt_path(&engines).is_file());
}

// ---- [analysis] preferences ----

#[test]
fn old_preferences_without_analysis_load_with_the_scid_defaults_and_no_issue() {
    for text in ["", "version = 1\n", "[board]\nanimation_ms = 100\n"] {
        let loaded = load_preferences(text);
        assert_eq!(*loaded.prefs.analysis, AnalysisPrefs::default(), "{text:?}");
        assert!(loaded.issues.is_empty(), "{text:?} {:?}", loaded.issues);
    }
    let d = AnalysisPrefs::default();
    assert_eq!(
        (
            d.log_lines,
            d.name_in_comments,
            d.max_ply,
            d.wrap_pv,
            d.show_info,
            d.info_interval_ms
        ),
        (5000, true, 0, false, false, 50)
    );
    assert_eq!(d.low_priority, cfg!(windows));
}

#[test]
fn analysis_preferences_round_trip_and_wild_values_are_clamped_not_fatal() {
    let prefs = Preferences {
        analysis: Box::new(AnalysisPrefs {
            log_lines: 0,
            name_in_comments: false,
            max_ply: 20,
            low_priority: !cfg!(windows),
            wrap_pv: true,
            show_info: true,
            info_interval_ms: 500,
        }),
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    assert_eq!(load_preferences(&text).prefs.analysis, prefs.analysis);
    let wild = load_preferences(
        "[analysis]\nlog_lines = -5\nmax_ply = 4294967296\ninfo_interval_ms = 99999999999999\nwrap_pv = \"x\"\n",
    );
    let a = *wild.prefs.analysis;
    assert!(
        a.log_lines <= MAX_LOG_LINES
            && a.max_ply <= MAX_PV_PLY
            && a.info_interval_ms <= MAX_INFO_INTERVAL_MS
    );
    // The rest of the preferences survives a bad `[analysis]`.
    let mixed = load_preferences("[analysis]\n= =\n");
    assert_eq!(mixed.prefs, Preferences::default());
    assert!(!mixed.issues.is_empty());
}

#[test]
fn set_analysis_prefs_changes_the_preferences_clamped_and_feeds_new_starts() {
    let fake = mychess_engine::testing::ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let wild = AnalysisPrefs {
        log_lines: u32::MAX,
        max_ply: u32::MAX,
        info_interval_ms: u32::MAX,
        low_priority: true,
        ..AnalysisPrefs::default()
    };
    send(&mut state, EngineMessage::SetAnalysisPrefs(wild));
    assert_eq!(state.prefs.analysis.log_lines, MAX_LOG_LINES);
    assert_eq!(state.prefs.analysis.max_ply, MAX_PV_PLY);
    let fx = send(
        &mut state,
        EngineMessage::Start(ids[0], mychess_app::engines::StartMode::Analyze),
    );
    let Some(Effect::StartEngine { defaults, .. }) =
        fx.iter().find(|e| matches!(e, Effect::StartEngine { .. }))
    else {
        panic!("no start")
    };
    assert!(defaults.low_priority);
    assert_eq!(defaults.log_lines, MAX_LOG_LINES as usize);
}

#[test]
fn the_entry_low_priority_overrides_the_global_default_in_the_engine_config() {
    let mut d = EngineDraft::new("A", "a");
    d.low_priority = Some(false);
    let defaults = AnalysisPrefs {
        low_priority: true,
        ..AnalysisPrefs::default()
    }
    .run_defaults();
    assert!(!d.to_config(&defaults).low_priority);
    d.low_priority = None;
    assert!(d.to_config(&defaults).low_priority);
    d.low_priority = Some(true);
    let off = AnalysisPrefs {
        low_priority: false,
        ..AnalysisPrefs::default()
    }
    .run_defaults();
    assert!(d.to_config(&off).low_priority);
}
