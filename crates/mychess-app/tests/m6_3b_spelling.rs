//! Contract tests of the spelling file, the name checker and "Add Elo" in
//! the application layer (spec `m6-3-jogadores-torneios-spelling.md`, slice
//! M6-3b; rules 1 to 14, 37 to 59 and 65 to 74, D3325 to D3331).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use m6_3b_common::*;
use mychess_app::{NameKind, SpellingLoadError};

fn misspelled() -> Vec<G> {
    vec![
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.10",
            "Alfa, Alf",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.11",
            "Beta, B",
            "Alfa, Alf",
            "0-1",
            0,
            0,
        ),
        g(
            "Aberto Brasileiro",
            "Rio de Janeiro BRA",
            "2020.05.20",
            "Alfa, A",
            "Delt, D",
            "1/2-1/2",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.06.01",
            "Gama, G",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
    ]
}

fn names_of(rig: &Rig, slot: SlotId, kind: mychess_db::si4::NameKind) -> Vec<String> {
    use mychess_db::GameSource;
    let guard = rig.state.bases.slot(slot).expect("slot").source.read();
    let names = guard.names();
    (0..names.num_names(kind))
        .filter_map(|id| names.get(kind, id))
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect()
}

// ---- loading ----

#[test]
fn the_menu_asks_for_a_file_and_the_answer_loads_it() {
    let mut rig = Rig::new();
    let host = rig.send(SpellingMessage::PickFile);
    let Some(Effect::PickFile(spec)) = host.first().cloned() else {
        panic!("a file dialog was expected: {host:?}");
    };
    assert_eq!(spec.kind, PickKind::OpenSpelling);
    let path = rig.data.path().join("my.ssp");
    std::fs::write(&path, SSP).unwrap();
    rig.send(Message::FilePicked(spec.request, Some(path.clone())));
    let status = rig.state.spelling_status_view();
    assert!(status.loaded);
    assert_eq!(status.path.as_deref(), Some(path.as_path()));
    let counts = status.counts.expect("counts");
    assert_eq!(
        (counts.players, counts.events, counts.sites, counts.rounds),
        (3, 1, 1, 1)
    );
    assert!(counts.has_elo);
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::Loaded(counts)),
        "the message of the Scid with the four counts"
    );
    assert_eq!(
        rig.state.prefs.player_tools.spelling_file,
        path.to_string_lossy(),
        "the chosen file becomes the preference"
    );
}

#[test]
fn cancelling_the_dialog_changes_nothing() {
    let mut rig = Rig::new();
    let host = rig.send(SpellingMessage::PickFile);
    let Some(Effect::PickFile(spec)) = host.first().cloned() else {
        panic!("dialog");
    };
    rig.send(Message::FilePicked(spec.request, None));
    assert!(!rig.state.spelling_status_view().loaded);
    assert!(rig.spelling_notice().is_none());
}

#[test]
fn a_bad_file_keeps_the_one_loaded_and_says_why() {
    let mut rig = Rig::new();
    rig.load_ssp(SSP);
    let before = rig.state.spelling().spelling().cloned().expect("loaded");
    let missing = rig.data.path().join("nope.ssp");
    rig.send(SpellingMessage::Load(missing));
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::LoadFailed(SpellingLoadError::File(
            mychess_app::FileErrorKind::NotFound
        )))
    );
    let unclosed = rig.data.path().join("bad.ssp");
    std::fs::write(&unclosed, "@PLAYER \"abc\nNobody\n").unwrap();
    rig.send(SpellingMessage::Load(unclosed));
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::LoadFailed(
            SpellingLoadError::UnclosedQuote { line: 1 }
        ))
    );
    let after = rig
        .state
        .spelling()
        .spelling()
        .cloned()
        .expect("still loaded");
    assert_eq!(after, before, "the set in force is the same one (rule 12)");
    assert_eq!(
        rig.state.prefs.player_tools.spelling_file,
        rig.data.path().join("spelling.ssp").to_string_lossy(),
        "a failed file is not remembered"
    );
}

#[test]
fn reload_reads_the_file_again() {
    let mut rig = Rig::new();
    let path = rig.load_ssp(SSP);
    let first = rig.state.spelling_status_view().counts.expect("counts");
    std::fs::write(&path, format!("{SSP}Zeta, Z #- BRA [1500]\n")).unwrap();
    // The Zeta line falls in the last section (rounds), so add a player.
    std::fs::write(
        &path,
        SSP.replace("Gama, G #-", "Zeta, Z #- BRA [1500]\nGama, G #-"),
    )
    .unwrap();
    rig.send(SpellingMessage::Reload);
    let second = rig.state.spelling_status_view().counts.expect("counts");
    assert_eq!(second.players, first.players + 1);
}

#[test]
fn reload_and_unload_without_a_file() {
    let mut rig = Rig::new();
    rig.send(SpellingMessage::Reload);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NotLoaded));
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::Unload);
    assert!(!rig.state.spelling_status_view().loaded);
}

#[test]
fn load_at_start_uses_the_preference_or_the_default_file_and_is_quiet_when_missing() {
    let mut rig = Rig::new();
    // No file anywhere: nothing happens and nothing is said.
    rig.send(SpellingMessage::LoadAtStart);
    assert!(!rig.state.spelling_status_view().loaded);
    assert!(rig.spelling_notice().is_none());
    // The default file of the data directory (D3335).
    std::fs::write(rig.data.path().join("spelling.ssp"), SSP).unwrap();
    rig.send(SpellingMessage::LoadAtStart);
    assert!(rig.state.spelling_status_view().loaded);
    // The preference wins and "load at start" can be turned off.
    let mut rig = Rig::new();
    let other = rig.data.path().join("other.ssp");
    std::fs::write(&other, ssp_without_elo()).unwrap();
    std::fs::write(rig.data.path().join("spelling.ssp"), SSP).unwrap();
    let mut tools = (*rig.state.prefs.player_tools).clone();
    tools.spelling_file = other.to_string_lossy().into_owned();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::PlayerTools(
        Box::new(tools.clone()),
    ))));
    rig.send(SpellingMessage::LoadAtStart);
    assert!(
        !rig.state
            .spelling_status_view()
            .counts
            .expect("counts")
            .has_elo
    );
    let mut rig2 = Rig::new();
    std::fs::write(rig2.data.path().join("spelling.ssp"), SSP).unwrap();
    tools.spelling_file.clear();
    tools.load_at_start = false;
    rig2.send(Message::Prefs(PrefsMessage::Set(PrefChange::PlayerTools(
        Box::new(tools),
    ))));
    assert!(rig2.raw(SpellingMessage::LoadAtStart).is_empty());
}

#[test]
fn a_stale_load_answer_is_dropped_and_cancel_stops_the_reading() {
    let mut rig = Rig::new();
    let path = rig.data.path().join("a.ssp");
    std::fs::write(&path, SSP).unwrap();
    let first = rig.raw(SpellingMessage::Load(path.clone()));
    let second = rig.raw(SpellingMessage::Load(path));
    let Effect::Spelling(mychess_app::SpellingEffect::Load { control, .. }) = &first[0] else {
        panic!("load effect");
    };
    assert!(control.is_cancelled(), "the new load stops the old one");
    let old = rig.perform(first[0].clone());
    rig.raw(old);
    assert!(
        !rig.state.spelling_status_view().loaded,
        "the old answer was dropped"
    );
    let new = rig.perform(second[0].clone());
    rig.raw(new);
    assert!(rig.state.spelling_status_view().loaded);
}

// ---- the name checker ----

#[test]
fn without_a_file_the_checker_asks_for_one() {
    let mut rig = Rig::new();
    let host = rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    assert!(
        matches!(host.first(), Some(Effect::PickFile(spec)) if spec.kind == PickKind::OpenSpelling)
    );
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NotLoaded));
    assert!(rig.state.spell_view().is_none());
}

#[test]
fn the_checker_lists_the_corrections_of_each_kind() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let view = rig.state.spell_view().expect("window");
    assert!(view.scanned && !view.interrupted && view.scanning.is_none());
    let pairs: Vec<(&str, &str, u32)> = view
        .rows
        .iter()
        .map(|r| (r.row.old.as_str(), r.row.new.as_str(), r.row.games))
        .collect();
    assert_eq!(
        pairs,
        [("Alfa, Alf", "Alfa, A", 2), ("Delt, D", "Delta, D", 1)],
        "the alias of Alfa and of Delta; Beta is not in the file"
    );
    assert!(view.rows.iter().all(|r| r.row.selected));
    assert_eq!(view.ticked, 2);
    // Events, sites: the other checkers of the same file.
    rig.send(SpellingMessage::OpenCheck(NameKind::Event));
    let events = rig.state.spell_view().expect("events");
    assert_eq!(events.kind, NameKind::Event);
    assert_eq!(
        events
            .rows
            .iter()
            .map(|r| r.row.new.as_str())
            .collect::<Vec<_>>(),
        ["Aberto Brasileiro"]
    );
    rig.send(SpellingMessage::OpenCheck(NameKind::Site));
    let sites = rig.state.spell_view().expect("sites");
    assert_eq!(sites.rows.len(), 1);
    assert_eq!(sites.rows[0].row.new, "Rio BRA");
}

#[test]
fn rows_can_be_ticked_edited_found_and_copied() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::ToggleRow(0));
    assert_eq!(rig.state.spell_view().unwrap().ticked, 1);
    rig.send(SpellingMessage::SelectAll(false));
    assert_eq!(rig.state.spell_view().unwrap().ticked, 0);
    rig.send(SpellingMessage::SelectAll(true));
    rig.send(SpellingMessage::EditNew(1, "Delta, Daniel".to_owned()));
    assert_eq!(
        rig.state.spell_view().unwrap().rows[1].row.new,
        "Delta, Daniel"
    );
    rig.send(SpellingMessage::SetFind("delt".to_owned()));
    let view = rig.state.spell_view().unwrap();
    assert_eq!(view.rows.len(), 1);
    assert_eq!(
        view.rows[0].index, 1,
        "the index is the place in the whole list"
    );
    assert_eq!(view.total, 2);
    let host = rig.send(SpellingMessage::CopyText);
    let Some(Effect::WriteClipboard(text)) = host.first() else {
        panic!("clipboard text expected: {host:?}");
    };
    assert!(text.starts_with("Scid found 2 player name corrections."));
    assert!(text.contains("\"Alfa, Alf\"\t>> \"Alfa, A\" (2)"), "{text}");
    // Indexes out of range are ignored.
    rig.send(SpellingMessage::ToggleRow(99));
    rig.send(SpellingMessage::EditNew(99, "x".to_owned()));
}

#[test]
fn surnames_and_ambiguous_scan_again_and_are_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let mut games = misspelled();
    games.push(g(
        "Aberto",
        "Rio BRA",
        "2020.07.01",
        "Gama",
        "Beta, B",
        "1-0",
        0,
        0,
    ));
    rig.open_si4(dir.path(), "m", &games);
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let plain = rig.state.spell_view().unwrap().rows.len();
    // "Gama" is a surname only: hidden until "Surnames" is on.
    rig.send(SpellingMessage::SetSurnames(true));
    let with = rig.state.spell_view().unwrap();
    assert_eq!(with.rows.len(), plain + 1);
    assert!(with.surnames);
    assert!(rig.state.prefs.player_tools.spellcheck.surnames);
    rig.send(SpellingMessage::SetAmbiguous(true));
    assert!(rig.state.prefs.player_tools.spellcheck.ambiguous);
    assert!(rig.state.spell_view().unwrap().ambiguous);
    // A new window starts with the remembered options.
    rig.send(SpellingMessage::CloseCheck);
    assert!(rig.state.spell_view().is_none());
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    assert!(rig.state.spell_view().unwrap().surnames);
}

#[test]
fn applying_asks_to_confirm_then_rewrites_the_names_and_reopens_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let old_source = rig.state.bases.slot(slot).unwrap().source.clone();
    let fx = rig.raw(SpellingMessage::Apply);
    assert!(fx.is_empty(), "the confirmation comes first: {fx:?}");
    assert!(rig.state.spell_view().unwrap().confirm);
    assert_eq!(disk_players(&path)[0].0, "Alfa, Alf", "nothing written yet");
    // "No" cancels.
    rig.send(SpellingMessage::ConfirmApply(false));
    assert!(!rig.state.spell_view().unwrap().confirm);
    assert_eq!(disk_players(&path)[0].0, "Alfa, Alf");
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    let players = disk_players(&path);
    assert_eq!(players[0], ("Alfa, A".to_owned(), "Beta, B".to_owned()));
    assert_eq!(players[1], ("Beta, B".to_owned(), "Alfa, A".to_owned()));
    assert_eq!(players[2].1, "Delta, D");
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 4);
    assert_ne!(
        rig.state.bases.slot(slot).unwrap().source,
        old_source,
        "the slot reads the database again"
    );
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::Applied {
            kind: NameKind::Player,
            names: 2,
            times: 3,
            skipped_by_date: 0,
            not_found: 0,
            interrupted: false,
        })
    );
    let view = rig.state.spell_view().unwrap();
    assert!(view.rows.is_empty() && view.result.is_some());
    // The old names stay in the .sn4 without games (rule 55).
    assert!(
        names_of(&rig, slot, mychess_db::si4::NameKind::Player).contains(&"Alfa, Alf".to_owned())
    );
    // Another scan finds nothing left.
    rig.send(SpellingMessage::Rescan);
    assert!(rig.state.spell_view().unwrap().rows.is_empty());
}

#[test]
fn the_filter_and_the_list_survive_a_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SelectNumbers {
            slot,
            numbers: vec![2, 3],
        },
    )));
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::KeepSelected { slot },
    )));
    assert_eq!(rig.filter_numbers(slot), [2, 3]);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(
        rig.filter_numbers(slot),
        [2, 3],
        "the filter stays (rule 58)"
    );
    assert_eq!(disk_players(&path)[1].1, "Alfa, A");
}

#[test]
fn a_changed_file_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    // Somebody touches the index behind the back of the slot.
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(file);
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(disk_players(&path)[0].0, "Alfa, Alf", "nothing was written");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Stale)))
    );
    // The slot was read again, so a second try goes through.
    assert!(rig.state.bases.slot(slot).is_some());
    rig.send(SpellingMessage::Rescan);
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(disk_players(&path)[0].0, "Alfa, A");
}

#[test]
fn a_pgn_shows_the_corrections_but_refuses_to_apply_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "m.pgn", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let view = rig.state.spell_view().unwrap();
    assert_eq!(view.rows.len(), 2);
    assert!(!view.writable);
    let fx = rig.raw(SpellingMessage::Apply);
    assert!(fx.is_empty());
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ReadOnly));
    assert!(!rig.state.spell_view().unwrap().confirm);
}

#[test]
fn a_read_only_si4_refuses_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SetReadOnly {
            slot,
            read_only: true,
        },
    )));
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ReadOnly));
    assert_eq!(disk_players(&path)[0].0, "Alfa, Alf");
}

#[test]
fn nothing_ticked_is_told_and_no_job_starts() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::SelectAll(false));
    let fx = rig.raw(SpellingMessage::Apply);
    assert!(fx.is_empty());
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NoneSelected));
    // An edit that makes the new name equal to the old one counts as nothing.
    rig.send(SpellingMessage::SelectAll(true));
    rig.send(SpellingMessage::EditNew(0, "Alfa, Alf".to_owned()));
    rig.send(SpellingMessage::ToggleRow(1));
    rig.raw(SpellingMessage::Apply);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NoneSelected));
}

#[test]
fn an_edited_new_name_is_the_one_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::EditNew(0, "Alfa, Alfredo".to_owned()));
    rig.send(SpellingMessage::ToggleRow(1));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    let players = disk_players(&path);
    assert_eq!(players[0].0, "Alfa, Alfredo");
    assert_eq!(players[2].1, "Delt, D", "the unticked row was left alone");
}

#[test]
fn the_clipbase_is_corrected_in_memory_without_a_question() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(Message::Bases(BasesMessage::List(ListMessage::CopyGames {
        from: slot,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    })));
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 4);
    rig.send(Message::Bases(BasesMessage::Select(SlotId::CLIPBASE)));
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let view = rig.state.spell_view().unwrap();
    assert!(view.in_clipbase && view.writable);
    assert_eq!(view.rows.len(), 2);
    rig.send(SpellingMessage::Apply);
    assert!(
        !rig.state.spell_view().unwrap().confirm,
        "no question for the Clipbase"
    );
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    assert_eq!(clip.total(), 4);
    let first = {
        use mychess_db::GameSource;
        clip.source.lock().game(1).unwrap().tags().white.clone()
    };
    assert_eq!(first, "Alfa, A");
    assert_eq!(
        disk_players(&path)[0].0,
        "Alfa, Alf",
        "the file is untouched"
    );
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Applied {
            names: 2,
            times: 3,
            ..
        })
    ));
    // Scanning again finds nothing: the table sees the new names.
    rig.send(SpellingMessage::Rescan);
    assert!(rig.state.spell_view().unwrap().rows.is_empty());
}

#[test]
fn the_life_span_of_a_player_limits_the_games_changed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    // Delta lived from 1960.01.01; a game of 1950 keeps the old name.
    let games = vec![
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.10",
            "Delt, D",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "1950.01.10",
            "Delt, D",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
    ];
    let (_, path) = rig.open_si4(dir.path(), "m", &games);
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    let players = disk_players(&path);
    assert_eq!(players[0].0, "Delta, D");
    assert_eq!(players[1].0, "Delt, D");
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Applied {
            times: 1,
            skipped_by_date: 1,
            ..
        })
    ));
}

#[test]
fn a_stale_scan_answer_is_dropped_and_cancel_returns_what_was_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let first = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let second = rig.raw(SpellingMessage::Rescan);
    let old = rig.perform(first[0].clone());
    rig.raw(old);
    assert!(
        !rig.state.spell_view().unwrap().scanned,
        "the old answer was dropped"
    );
    let new = rig.perform(second[0].clone());
    rig.raw(new);
    assert!(rig.state.spell_view().unwrap().scanned);
    // Stop before the effect runs: a partial (here empty) list, said so.
    let third = rig.raw(SpellingMessage::Rescan);
    rig.raw(SpellingMessage::CancelScan);
    let answer = rig.perform(third[0].clone());
    rig.raw(answer);
    let view = rig.state.spell_view().unwrap();
    assert!(view.interrupted);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ScanInterrupted));
}

#[test]
fn quitting_stops_the_scan_and_the_reading() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let scan = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let Effect::Spelling(mychess_app::SpellingEffect::Scan { control, .. }) = &scan[0] else {
        panic!("scan effect");
    };
    let load = rig.raw(SpellingMessage::Reload);
    let Effect::Spelling(mychess_app::SpellingEffect::Load {
        control: load_control,
        ..
    }) = &load[0]
    else {
        panic!("load effect");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled() && load_control.is_cancelled());
}

#[test]
fn closing_the_database_makes_the_applying_answer_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    assert_eq!(fx.len(), 1, "one write effect");
    // The user closes the slot while the write is on its way.
    rig.raw(BasesMessage::Close(slot));
    assert!(rig.state.bases.slot(slot).is_none());
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    assert!(rig.state.bases.slot(slot).is_none());
    // The file can be opened again once the job answered.
    rig.send(BasesMessage::Open(path));
    assert!(rig.state.bases.slot(rig.state.bases.current()).is_some());
}

// ---- Add Elo ----

#[test]
fn add_elo_needs_a_file_with_ratings() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &six());
    rig.send(SpellingMessage::OpenAddElo);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NotLoaded));
    assert!(rig.state.add_elo_view().is_none());
    rig.load_ssp(&ssp_without_elo());
    rig.send(SpellingMessage::OpenAddElo);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NoEloData));
    assert!(rig.state.add_elo_view().is_none());
}

fn unrated() -> Vec<G> {
    vec![
        g(
            "Aberto",
            "Rio BRA",
            "2020.01.10",
            "Alfa, A",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.05.20",
            "Beta, B",
            "Alfa, A",
            "0-1",
            0,
            2400,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2019.05.20",
            "Alfa, A",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
        g(
            "Aberto",
            "Rio BRA",
            "2020.03.20",
            "Alfa, Alf",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
    ]
}

#[test]
fn add_elo_plans_asks_and_writes_the_ratings_of_the_period() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    let view = rig.state.add_elo_view().expect("dialog");
    assert!(
        !view.overwrite && view.only_filter,
        "the defaults of the Scid"
    );
    assert!(view.planned.is_none() && view.writable);
    // Not planned yet: confirming does nothing.
    assert!(rig.raw(SpellingMessage::ConfirmElo).is_empty());
    rig.send(SpellingMessage::PlanElo);
    let view = rig.state.add_elo_view().unwrap();
    // 2020.01 (position 0) of Alfa; the black side of game 2 already has a
    // rating; the alias and 2019 get none.
    assert_eq!(view.planned, Some((1, 1)), "ratings and games");
    assert_eq!(disk_elos(&path)[0], (0, 0), "planning writes nothing");
    let old_source = rig.state.bases.slot(slot).unwrap().source.clone();
    rig.send(SpellingMessage::ConfirmElo);
    let elos = disk_elos(&path);
    assert_eq!(elos[0], (2500, 0));
    assert_eq!(elos[1], (0, 2400), "a rating that is there stays");
    assert_eq!(elos[2], (0, 0), "no table for 2019");
    assert_eq!(elos[3], (0, 0), "an alias has no ratings (rule 62)");
    assert_ne!(rig.state.bases.slot(slot).unwrap().source, old_source);
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::EloAdded {
            ratings: 1,
            games: 1
        })
    );
    // The type of rating is Elo.
    {
        let db = mychess_db::si4::Si4Database::open(&path).unwrap();
        assert_eq!(db.entry(1).unwrap().white_rating_type, 0);
    }
    // Running it again changes nothing (D3330).
    rig.send(SpellingMessage::PlanElo);
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::EloNothing));
    assert!(rig.state.add_elo_view().unwrap().planned.is_none());
}

#[test]
fn add_elo_options_change_what_is_planned_and_are_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    // Only the filter: keep the second game only, whose rating is there.
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SelectNumbers {
            slot,
            numbers: vec![2],
        },
    )));
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::KeepSelected { slot },
    )));
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    assert!(rig.state.add_elo_view().unwrap().planned.is_none());
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::EloNothing));
    // All the games, no overwrite.
    rig.send(SpellingMessage::SetEloOnlyFilter(false));
    assert!(!rig.state.prefs.player_tools.add_elo.only_filter);
    rig.send(SpellingMessage::PlanElo);
    assert_eq!(rig.state.add_elo_view().unwrap().planned, Some((1, 1)));
    // Changing an option drops the plan.
    rig.send(SpellingMessage::SetEloOverwrite(true));
    assert!(
        rig.state.add_elo_view().unwrap().planned.is_none(),
        "a plan is dropped when the options change"
    );
    assert!(rig.state.prefs.player_tools.add_elo.overwrite);
    // Overwrite: the 2400 of the second game is replaced too.
    rig.send(SpellingMessage::PlanElo);
    assert_eq!(rig.state.add_elo_view().unwrap().planned, Some((2, 2)));
    rig.send(SpellingMessage::ConfirmElo);
    let elos = disk_elos(&path);
    assert_eq!(elos[0].0, 2500);
    assert_eq!(elos[1].1, 2504, "May of 2020 is position 4");
}

#[test]
fn add_elo_in_a_pgn_refuses_to_plan_and_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "m.pgn", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    assert!(!rig.state.add_elo_view().unwrap().writable);
    // The application is the source of truth (D3495): no plan is made for a
    // base that cannot be written, whoever asks (the Enter key included).
    let fx = rig.raw(SpellingMessage::PlanElo);
    assert!(fx.iter().all(|e| !matches!(e, Effect::Spelling(_))));
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ReadOnly));
    assert!(rig.state.add_elo_view().unwrap().planned.is_none());
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert!(fx.is_empty());
}

#[test]
fn add_elo_in_the_clipbase_is_done_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(Message::Bases(BasesMessage::List(ListMessage::CopyGames {
        from: slot,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    })));
    rig.send(Message::Bases(BasesMessage::Select(SlotId::CLIPBASE)));
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    rig.send(SpellingMessage::ConfirmElo);
    let entry = {
        use mychess_db::GameSource;
        rig.state
            .bases
            .slot(SlotId::CLIPBASE)
            .unwrap()
            .source
            .read()
            .entry(1)
            .unwrap()
    };
    assert_eq!(entry.white_elo, 2500);
    assert_eq!(disk_elos(&path)[0], (0, 0), "the file is untouched");
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::EloAdded {
            ratings: 1,
            games: 1
        })
    );
}

#[test]
fn a_changed_file_stops_add_elo_and_closing_the_dialog_cancels_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(7200))
        .unwrap();
    drop(file);
    rig.send(SpellingMessage::ConfirmElo);
    assert_eq!(disk_elos(&path)[0], (0, 0));
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::Failed(BaseError::Stale)))
    );
    let fx = rig.raw(SpellingMessage::PlanElo);
    let Some(Effect::Spelling(mychess_app::SpellingEffect::PlanElo { control, .. })) = fx.first()
    else {
        panic!("plan effect: {fx:?}");
    };
    rig.raw(SpellingMessage::CloseAddElo);
    assert!(control.is_cancelled());
    assert!(rig.state.add_elo_view().is_none());
}

// ---- the PlayerStats cache ----

#[test]
fn the_estimates_of_the_spelling_file_feed_the_player_stats_of_each_database() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &six());
    assert!(
        rig.state.player_stats(slot).is_none(),
        "no spelling file, no cache"
    );
    rig.load_ssp(SSP);
    let stats = rig.state.player_stats(slot).expect("built after the load");
    let id = |name: &str| {
        let db = mychess_db::si4::Si4Database::open(&path).unwrap();
        (0..db.names().num_names(mychess_db::si4::NameKind::Player))
            .find(|&i| {
                db.names().get(mychess_db::si4::NameKind::Player, i) == Some(name.as_bytes())
            })
            .unwrap()
    };
    assert_eq!(stats.max_elo(id("Alfa, A")), 2520, "the games win");
    assert_eq!(stats.max_elo(id("Gama, G")), 2050, "the peak of the file");
    assert_eq!(stats.max_elo(id("Delta, D")), 2100);
    assert_eq!(stats.max_elo(id("Beta, B")), 2410, "not in the file");
    // Another database opened later gets its own.
    let (second, _) = rig.open_si4(dir.path(), "n", &six());
    assert!(rig.state.player_stats(second).is_some());
    // Changing the file rebuilds (the estimates change).
    let other = SSP.replace("[2050]", "[2222]");
    rig.load_ssp(&other);
    let stats = rig.state.player_stats(slot).expect("rebuilt");
    assert_eq!(stats.max_elo(id("Gama, G")), 2222);
    rig.send(SpellingMessage::Unload);
    assert!(rig.state.player_stats(slot).is_none());
}

#[test]
fn a_write_makes_the_stats_of_the_slot_new() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let before = rig.state.player_stats(slot).expect("stats");
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    let after = rig
        .state
        .player_stats(slot)
        .expect("rebuilt for the new database");
    assert_ne!(after, before, "a new value, not the old one");
}

#[test]
fn the_clipbase_has_no_cached_stats() {
    let mut rig = Rig::new();
    rig.load_ssp(SSP);
    assert!(rig.state.player_stats(SlotId::CLIPBASE).is_none());
}

// ---- photos ----

#[test]
fn photos_of_the_data_directory_are_found_after_the_load() {
    let mut rig = Rig::new();
    let photos = rig.data.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("Alfa, A.gif"), b"GIF89a\x01\x00").unwrap();
    std::fs::write(photos.join("broken.gif"), b"").unwrap();
    rig.load_ssp(SSP);
    let counts = rig
        .state
        .spelling_status_view()
        .photos
        .expect("photos looked for");
    assert_eq!(counts.photos, 1);
}

// ---- preferences ----

#[test]
fn the_spelling_preferences_round_trip_through_the_changes() {
    let mut rig = Rig::new();
    let mut tools = (*rig.state.prefs.player_tools).clone();
    tools.show_photos = true;
    tools.photos_dir = "/somewhere".to_owned();
    tools.history = (0..15).map(|i| format!("P{i}")).collect();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::PlayerTools(
        Box::new(tools),
    ))));
    let p = &rig.state.prefs.player_tools;
    assert!(p.show_photos);
    assert_eq!(p.history.len(), mychess_app::HISTORY_MAX);
    let text = mychess_app::render_preferences(&rig.state.prefs);
    let loaded = mychess_app::load_preferences(&text);
    assert_eq!(*loaded.prefs.player_tools, **p);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}
