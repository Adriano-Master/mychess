//! M8-8b contract tests of the `[tablebase]` preferences (spec D2562) and of
//! the effect that reads the folders (D2563, D2584).
#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;

use mychess_app::tablebase::{MAX_DIRECTORIES, TablebaseEffect, TablebaseMessage, TablebasePrefs};
use mychess_app::{
    Effect, Message, PrefsIssue, RequestId, load_preferences, perform, render_preferences,
};

#[test]
fn the_defaults_are_everything_off() {
    let p = TablebasePrefs::default();
    assert!(!p.local && !p.online && p.directories.is_empty());
    let loaded = load_preferences("");
    assert_eq!(*loaded.prefs.tablebase, p);
    assert!(loaded.issues.is_empty());
}

#[test]
fn the_table_round_trips() {
    let mut loaded = load_preferences("");
    *loaded.prefs.tablebase = TablebasePrefs {
        local: true,
        directories: vec!["a b".to_owned(), "c\\d".to_owned()],
        online: true,
    };
    let text = render_preferences(&loaded.prefs);
    assert!(text.contains("[tablebase]"), "{text}");
    let again = load_preferences(&text);
    assert!(again.issues.is_empty(), "{:?}", again.issues);
    assert_eq!(again.prefs.tablebase, loaded.prefs.tablebase);
}

#[test]
fn wrong_types_give_remarks_and_the_defaults() {
    let loaded = load_preferences(
        "[tablebase]\nlocal = \"yes\"\nonline = 3\ndirectories = \"not a list\"\n",
    );
    let mut keys: Vec<String> = loaded
        .issues
        .iter()
        .filter_map(|i| match i {
            PrefsIssue::InvalidValue { key } => Some(key.clone()),
            _ => None,
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "tablebase.directories",
            "tablebase.local",
            "tablebase.online"
        ]
    );
    assert_eq!(*loaded.prefs.tablebase, TablebasePrefs::default());
}

#[test]
fn the_list_of_folders_is_limited_and_cleaned() {
    let mut text = String::from("[tablebase]\nlocal = true\ndirectories = [\"\", \"x\", \"x\", 7");
    for i in 0..30 {
        write!(text, ", \"d{i}\"").unwrap();
    }
    text.push_str("]\n");
    let loaded = load_preferences(&text);
    let dirs = &loaded.prefs.tablebase.directories;
    assert_eq!(dirs.len(), MAX_DIRECTORIES);
    assert_eq!(dirs[0], "x");
    // The number among the folders is dropped with a remark.
    assert!(
        loaded.issues.iter().any(
            |i| matches!(i, PrefsIssue::InvalidValue { key } if key == "tablebase.directories")
        ),
        "{:?}",
        loaded.issues
    );
    assert!(loaded.prefs.tablebase.local);
}

#[tokio::test]
async fn opening_reads_the_folders_off_the_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("KQvK.rtbw"), vec![0u8; 80]).unwrap();
    let answer = perform(Effect::Tablebase(TablebaseEffect::Open {
        request: RequestId(5),
        dirs: vec![dir.path().to_path_buf(), dir.path().join("missing")],
    }))
    .await;
    let Message::Tablebase(TablebaseMessage::Opened { request, result }) = answer else {
        panic!("not an Opened answer");
    };
    assert_eq!(request, RequestId(5));
    let set = (*result).expect("opened");
    assert_eq!(set.handle.max_pieces(), 3);
    assert_eq!(set.report.wdl_files, 1);
    assert_eq!(set.report.directories.len(), 2);
}

#[tokio::test]
async fn a_delay_answers_with_its_kind() {
    use mychess_app::tablebase::DelayKind;
    let answer = perform(Effect::Tablebase(TablebaseEffect::Delay {
        request: RequestId(9),
        kind: DelayKind::Reply,
        ms: 1,
    }))
    .await;
    assert_eq!(
        answer,
        Message::Tablebase(TablebaseMessage::DelayElapsed {
            request: RequestId(9),
            kind: DelayKind::Reply
        })
    );
}
