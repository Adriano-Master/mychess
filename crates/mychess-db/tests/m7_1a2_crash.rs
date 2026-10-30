//! M7-1a2: the state a KILLED process leaves behind (`FailPoint::kill_at_label`
//! skips every cleanup), for `strip_games` and `edit_names`; plus the tag 255
//! debt (D3577) and the guards of D3578 and D3579.
//! (`docs/specs/m7-1-manutencao.md`).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use mychess_core::PartialDate;
use mychess_db::si4::{
    FailPoint, GameFlags, NameEdit, NameEditError, NameEditTarget, NameKind, NameScope, Si4Error,
    Si4Writer, StripWhat, compact_games, encode_record,
};
use mychess_db::{CancelToken, NoProgress};
use support::{build_games, line, open, plain, rich, shape, snapshot};

fn killed_strip(label: &str, games: &[u32]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), plain(), rich(), plain(), rich()]);
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_fail_point(FailPoint::kill_at_label(label));
    let error = writer
        .strip_games(games, StripWhat::Both, &CancelToken::new(), &mut NoProgress)
        .unwrap_err();
    assert!(
        matches!(error, Si4Error::InjectedFailure { .. }),
        "{label}: {error}"
    );
    drop(writer);
    (dir, base)
}

#[test]
fn a_process_killed_after_the_append_leaves_orphan_records_that_do_no_harm() {
    for label in ["rewrite:appended", "rewrite:entries"] {
        let dir = tempfile::tempdir().unwrap();
        let base = build_games(dir.path(), &[rich(), plain(), rich(), plain(), rich()]);
        let before = snapshot(&base);
        drop(dir);
        let (_dir, base) = killed_strip(label, &[1, 3, 5]);
        let after = snapshot(&base);
        // The index is the old one and the games file kept the orphan tail:
        // this is what a real crash leaves (nothing cut it).
        assert_eq!(after[0], before[0], "{label}: index untouched");
        assert!(
            after[2].len() > before[2].len(),
            "{label}: the orphan records are still there"
        );

        let mut db = open(&base);
        assert_eq!(db.len(), 5);
        for n in 1..=5 {
            db.game(n).unwrap();
        }
        assert_eq!(shape(&mut db, 1).comments, 3, "{label}: old record");
        drop(db);

        // A writer accepts new games after the dead space ...
        let mut writer = Si4Writer::open(&base).unwrap();
        let number = writer.add_game(&plain(), GameFlags::default()).unwrap();
        assert_eq!(number, 6);
        // ... and compaction reclaims it.
        let len_before = std::fs::metadata(base.with_extension("sg4")).unwrap().len();
        let (writer, report) = compact_games(writer).unwrap();
        assert_eq!(report.kept, 6);
        drop(writer);
        let mut db = open(&base);
        for n in 1..=6 {
            db.game(n).unwrap();
        }
        assert_eq!(db.game(6).unwrap().mainline().count(), 4);
        drop(db);
        assert!(
            std::fs::metadata(base.with_extension("sg4")).unwrap().len() < len_before,
            "{label}: compaction freed the orphans"
        );
    }
}

#[test]
fn a_process_killed_with_half_of_the_entries_patched_leaves_every_game_readable() {
    // Games 1, 3 and 5 are not neighbours: three separate writes, the kill
    // comes before the third.
    let (_dir, base) = killed_strip("rewrite:mid", &[1, 3, 5]);
    let mut db = open(&base);
    assert_eq!(db.len(), 5);
    for n in 1..=5 {
        db.game(n).unwrap();
    }
    let comments: Vec<usize> = [1, 3, 5].map(|n| shape(&mut db, n).comments).to_vec();
    assert_eq!(comments, vec![0, 0, 3], "two games patched, one still old");
    for n in [1u32, 3, 5] {
        // The entry counts agree with the record each entry points to.
        let entry = db.entry(n).unwrap();
        assert_eq!(usize::from(entry.comment_count), shape(&mut db, n).comments);
    }
    drop(db);
    // Repeating the strip finishes the job.
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .strip_games(
            &[1, 3, 5],
            StripWhat::Both,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!((report.changed, report.unchanged), (1, 2));
    drop(writer);
    let mut db = open(&base);
    for n in [1u32, 3, 5] {
        assert_eq!(shape(&mut db, n).comments, 0);
    }
}

fn event_edit(old: &str, new: &str) -> NameEdit {
    NameEdit {
        target: NameEditTarget::Name {
            kind: NameKind::Event,
            old: old.into(),
            new: new.into(),
        },
        pattern: false,
    }
}

fn six_plain() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<_> = (0..6).map(|_| line(&["e2e4", "e7e5"])).collect();
    let base = build_games(dir.path(), &games);
    (dir, base)
}

fn frequency(base: &std::path::Path, text: &str) -> u32 {
    let db = open(base);
    let names = db.names();
    (0..names.num_names(NameKind::Event))
        .find(|&id| names.get(NameKind::Event, id) == Some(text.as_bytes()))
        .and_then(|id| names.frequency(NameKind::Event, id))
        .unwrap_or(0)
}

fn events(base: &std::path::Path) -> Vec<String> {
    let mut db = open(base);
    (1..=6)
        .map(|n| db.game(n).unwrap().tags().event.clone())
        .collect()
}

#[test]
fn a_name_edit_killed_between_two_chunks_can_be_repeated() {
    let (_dir, base) = six_plain();
    let scope = [1u32, 3, 5];
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_fail_point(FailPoint::kill_at_label("names:chunk"));
    let error = writer
        .edit_names(
            &event_edit("Aberto", "Open"),
            NameScope::Games(&scope),
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        NameEditError::Si4(Si4Error::InjectedFailure { .. })
    ));
    drop(writer);

    // Half done: the database opens and the first run of games has the new name.
    let now = events(&base);
    assert_eq!(&now[..3], ["Open", "Aberto", "Aberto"]);
    // The frequencies on disk are the stale ones.
    assert_eq!(frequency(&base, "Open"), 0);

    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .edit_names(
            &event_edit("Aberto", "Open"),
            NameScope::Games(&scope),
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!(report.games_changed, 2, "game 1 was already done");
    drop(writer);
    assert_eq!(
        events(&base),
        ["Open", "Aberto", "Open", "Aberto", "Open", "Aberto"]
    );
    assert_eq!(frequency(&base, "Open"), 3);
    assert_eq!(frequency(&base, "Aberto"), 3);
}

#[test]
fn a_name_edit_killed_before_the_final_names_is_put_right_by_repeating_it() {
    let (_dir, base) = six_plain();
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_fail_point(FailPoint::kill_at_label("names:final"));
    writer
        .edit_names(
            &event_edit("Aberto", "Open"),
            NameScope::All,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap_err();
    drop(writer);
    assert_eq!(events(&base), vec!["Open"; 6]);
    assert_eq!(frequency(&base, "Aberto"), 6, "stale on disk");
    assert_eq!(frequency(&base, "Open"), 0, "stale on disk");

    // Every entry is already done, yet the repetition recounts the file.
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .edit_names(
            &event_edit("Aberto", "Open"),
            NameScope::All,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!(report.games_changed, 0);
    drop(writer);
    assert_eq!(frequency(&base, "Open"), 6);
    assert_eq!(frequency(&base, "Aberto"), 0);
}

#[test]
fn an_edit_that_changes_nothing_leaves_the_names_file_byte_identical() {
    let (_dir, base) = six_plain();
    let before = snapshot(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .edit_names(
            &event_edit("Aberto", "Aberto"),
            NameScope::All,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!(report.games_changed, 0);
    drop(writer);
    assert_eq!(snapshot(&base), before);
}

// ---- D3577: the tag 255 of the Scid records ----

fn date_edit(old: &str, new: &str) -> NameEdit {
    NameEdit {
        target: NameEditTarget::EventDate {
            old: old.into(),
            new: new.into(),
        },
        pattern: false,
    }
}

/// A one-game database whose record has the tag 255 (event date 2010.03.04,
/// more than 3 years from the game date) and whose entry has no event date,
/// as the Scid writes it.
fn tag_255_base() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[plain()]);
    let mut db = open(&base);
    let mut raw = db.raw_game(1).unwrap();
    drop(db);
    raw.event_date = Some(PartialDate::parse_lenient("2010.03.04"));
    let record = encode_record(&raw).unwrap().bytes;
    let games_path = base.with_extension("sg4");
    let mut games = std::fs::read(&games_path).unwrap();
    let offset = u32::try_from(games.len()).unwrap();
    games.extend_from_slice(&record);
    std::fs::write(&games_path, &games).unwrap();
    let index_path = base.with_extension("si4");
    let mut index = std::fs::read(&index_path).unwrap();
    let at = 182;
    index[at..at + 4].copy_from_slice(&offset.to_be_bytes());
    let length = u16::try_from(record.len()).unwrap();
    index[at + 4..at + 6].copy_from_slice(&length.to_be_bytes());
    index[at + 6] &= 0x7F;
    std::fs::write(&index_path, &index).unwrap();
    (dir, base)
}

#[test]
fn the_event_date_of_a_tag_255_record_is_not_edited_by_the_name_editor() {
    let (_dir, base) = tag_255_base();
    let mut db = open(&base);
    assert!(db.entry(1).unwrap().event_date.is_unknown());
    assert_eq!(
        db.game(1).unwrap().tags().event_date.to_string(),
        "2010.03.04"
    );
    drop(db);

    // The real date never matches `old`: the entry says "unknown".
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .edit_names(
            &date_edit("2010.03.04", "2020.01.01"),
            NameScope::Games(&[1]),
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!(report.games_changed, 0);

    // An edit that matches the entry writes the entry, but the record wins on
    // reading (documented debt, D3577).
    let report = writer
        .edit_names(
            &date_edit("????.??.??", "2020.01.01"),
            NameScope::Games(&[1]),
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!(report.games_changed, 1);
    drop(writer);
    let mut db = open(&base);
    assert_eq!(db.entry(1).unwrap().event_date.to_string(), "2020.01.01");
    assert_eq!(
        db.game(1).unwrap().tags().event_date.to_string(),
        "2010.03.04"
    );
}

// ---- D3578: patterns that match everything ----

#[test]
fn patterns_without_a_literal_are_refused_like_a_lone_star() {
    let (_dir, base) = six_plain();
    let before = snapshot(&base);
    for pattern in ["*", "**", "*?*", "?*", "***"] {
        for (kind, scope_all) in [(NameKind::Player, false), (NameKind::Event, true)] {
            let edit = NameEdit {
                target: NameEditTarget::Name {
                    kind,
                    old: pattern.into(),
                    new: "X".into(),
                },
                pattern: true,
            };
            let scope: &[u32] = &[1, 2];
            let outcome = {
                let mut writer = Si4Writer::open(&base).unwrap();
                writer.edit_names(
                    &edit,
                    if scope_all {
                        NameScope::All
                    } else {
                        NameScope::Games(scope)
                    },
                    &CancelToken::new(),
                    &mut NoProgress,
                )
            };
            let expected_player = kind == NameKind::Player;
            assert!(
                matches!(
                    (&outcome, expected_player),
                    (Err(NameEditError::GlobPlayer), true) | (Err(NameEditError::GlobAll), false)
                ),
                "{pattern:?} {kind:?}: {outcome:?}"
            );
        }
    }
    assert_eq!(snapshot(&base), before);
    // A literal makes the pattern fine.
    let edit = NameEdit {
        target: NameEditTarget::Name {
            kind: NameKind::Event,
            old: "*Aber*".into(),
            new: "Open".into(),
        },
        pattern: true,
    };
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .edit_names(&edit, NameScope::All, &CancelToken::new(), &mut NoProgress)
        .unwrap();
    assert_eq!(report.games_changed, 6);
}

#[test]
fn an_old_name_longer_than_the_limit_is_not_found_instead_of_cut() {
    let dir = tempfile::tempdir().unwrap();
    let long = "L".repeat(255);
    let mut game = line(&["e2e4"]);
    game.tags_mut().event = long.clone();
    let base = build_games(dir.path(), &[game]);
    let mut writer = Si4Writer::open(&base).unwrap();
    let outcome = writer.edit_names(
        &event_edit(&format!("{long}tail"), "Open"),
        NameScope::All,
        &CancelToken::new(),
        &mut NoProgress,
    );
    assert!(
        matches!(outcome, Err(NameEditError::NameNotFound { .. })),
        "{outcome:?}"
    );
    // The exact name still works.
    let report = writer
        .edit_names(
            &event_edit(&long, "Open"),
            NameScope::All,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap();
    assert_eq!(report.games_changed, 1);
}

// ---- D3579: the games file changed under the rewrite ----

#[cfg(unix)]
#[test]
fn a_games_file_that_grew_during_the_rewrite_is_not_overwritten_or_cut() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<_> = (0..300).map(|_| rich()).collect();
    let base = build_games(dir.path(), &games);
    let games_path = base.with_extension("sg4");
    let old_len = std::fs::metadata(&games_path).unwrap().len();
    let index_before = std::fs::read(base.with_extension("si4")).unwrap();
    // A Scid (it ignores the advisory lock) appends a game while we scan.
    let mut intruded = false;
    let path = games_path.clone();
    let mut sink = move |_done: u64, _total: u64| {
        if !intruded {
            intruded = true;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            file.write_all(&[0xEE; 64]).unwrap();
        }
    };
    let mut writer = Si4Writer::open(&base).unwrap();
    let error = writer
        .strip_games(
            &(1..=300).collect::<Vec<_>>(),
            StripWhat::Both,
            &CancelToken::new(),
            &mut sink,
        )
        .unwrap_err();
    assert!(matches!(error, Si4Error::Io { .. }), "{error}");
    drop(writer);
    let after = std::fs::read(&games_path).unwrap();
    assert_eq!(
        after.len() as u64,
        old_len + 64,
        "neither cut nor overwritten"
    );
    assert!(after[after.len() - 64..].iter().all(|&b| b == 0xEE));
    assert_eq!(
        std::fs::read(base.with_extension("si4")).unwrap(),
        index_before
    );
}
