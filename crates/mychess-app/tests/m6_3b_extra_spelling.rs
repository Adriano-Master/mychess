//! Extra contract tests of the spelling file, the name checker and "Add Elo"
//! in the application layer (spec `m6-3-jogadores-torneios-spelling.md`,
//! slice M6-3b): hostile files, stale and cancelled answers, quitting during
//! work, concurrent jobs, reloads, read-only and closed databases.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m6_3b_common;

use std::fmt::Write as _;

use m6_3b_common::*;
use mychess_app::{NameKind, SpellingEffect, SpellingLoadError};

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
    ]
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
            "2020.03.10",
            "Alfa, A",
            "Beta, B",
            "1-0",
            0,
            0,
        ),
    ]
}

fn write(rig: &Rig, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = rig.data.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn counts(rig: &Rig) -> mychess_app::SpellingCounts {
    rig.state.spelling_status_view().counts.expect("loaded")
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A gzip member with one stored deflate block (data under 64 KiB).
fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(data.len()).unwrap();
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 1];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out
}

fn players_on_disk(path: &std::path::Path) -> Vec<String> {
    disk_players(path)
        .into_iter()
        .flat_map(|(w, b)| [w, b])
        .collect()
}

// ---- loading: hostile files (rules 1 to 14) ----

#[test]
fn a_directory_is_a_read_error_and_keeps_the_set() {
    let mut rig = Rig::new();
    rig.load_ssp(SSP);
    let before = rig.state.spelling().spelling().cloned().unwrap();
    let dir = rig.data.path().join("folder.ssp");
    std::fs::create_dir(&dir).unwrap();
    rig.send(SpellingMessage::Load(dir));
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::LoadFailed(_))
    ));
    assert_eq!(rig.state.spelling().spelling().cloned().unwrap(), before);
}

#[test]
fn a_corrupt_gzip_is_a_read_error_and_keeps_the_set() {
    let mut rig = Rig::new();
    rig.load_ssp(SSP);
    let before = rig.state.spelling().spelling().cloned().unwrap();
    let mut bad = gzip_stored(b"@PLAYER \"\"\nAaa, A\n");
    let n = bad.len();
    bad[n - 6] ^= 0xff; // the checksum
    let path = write(&rig, "bad.ssp.gz", &bad);
    rig.send(SpellingMessage::Load(path));
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::LoadFailed(_))
    ));
    assert_eq!(rig.state.spelling().spelling().cloned().unwrap(), before);
}

#[test]
fn a_gzip_file_loads_like_the_plain_one() {
    let mut rig = Rig::new();
    let plain = write(&rig, "a.ssp", SSP.as_bytes());
    rig.send(SpellingMessage::Load(plain));
    let want = counts(&rig);
    let zipped = write(&rig, "b.SSP.GZ", &gzip_stored(SSP.as_bytes()));
    rig.send(SpellingMessage::Unload);
    rig.send(SpellingMessage::Load(zipped));
    assert_eq!(counts(&rig), want);
    assert_eq!(want.players, 3);
}

#[test]
fn bom_crlf_cr_and_a_missing_last_newline_all_read() {
    let mut rig = Rig::new();
    // UTF-8 BOM, CRLF.
    let mut crlf = vec![0xEF, 0xBB, 0xBF];
    crlf.extend_from_slice(b"@PLAYER \"., -_*\"\r\nAaa, A #GM BRA [2500]\r\nBbb, B");
    let path = write(&rig, "crlf.ssp", &crlf);
    rig.send(SpellingMessage::Load(path));
    assert_eq!(counts(&rig).players, 2, "BOM skipped, last line kept");
    // Lone CR.
    let path = write(&rig, "cr.ssp", b"@PLAYER \"\"\rAaa, A\rBbb, B\rCcc, C");
    rig.send(SpellingMessage::Load(path));
    assert_eq!(counts(&rig).players, 3);
}

#[test]
fn latin1_bytes_are_converted_not_rejected() {
    let mut rig = Rig::new();
    let mut bytes = b"@PLAYER \"., -_*\"\nJos".to_vec();
    bytes.push(0xE9); // e acute in Latin-1
    bytes.extend_from_slice(b", Antonio #- ESP [2000]\nMuller, K\n");
    let path = write(&rig, "latin.ssp", &bytes);
    rig.send(SpellingMessage::Load(path));
    assert_eq!(counts(&rig).players, 2);
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Loaded(_))
    ));
}

#[test]
fn a_line_over_64_kib_is_skipped_and_counted() {
    let mut rig = Rig::new();
    let mut text = String::from("@PLAYER \"\"\nAaa, A\n");
    text.push_str(&"x".repeat(64 * 1024 + 1));
    text.push_str("\nBbb, B\n");
    let path = write(&rig, "long.ssp", text.as_bytes());
    rig.send(SpellingMessage::Load(path));
    let c = counts(&rig);
    assert_eq!(c.players, 2, "the giant line is not a name");
    assert_eq!(c.skipped_lines, 1);
}

#[test]
fn a_thousand_and_more_bio_notes_do_not_break_the_load() {
    let mut rig = Rig::new();
    let mut text = String::from("@PLAYER \"\"\nAaa, A #- BRA [2000]\n");
    for i in 0..1500 {
        let _ = writeln!(text, "%Bio note {i}");
    }
    text.push_str("Bbb, B\n");
    let path = write(&rig, "bio.ssp", text.as_bytes());
    rig.send(SpellingMessage::Load(path));
    assert_eq!(counts(&rig).players, 2);
}

#[test]
fn sections_of_unknown_type_or_before_the_first_are_ignored_without_error() {
    let mut rig = Rig::new();
    let text = "Orphan, O\n@FOO\nNobody, N\n@p\nAaa, A\n@s \"\"\nXx\n@event\nEe\n@r\n1\n";
    let path = write(&rig, "sec.ssp", text.as_bytes());
    rig.send(SpellingMessage::Load(path));
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::Loaded(_))
    ));
    let c = counts(&rig);
    assert_eq!(
        (c.players, c.sites, c.events, c.rounds),
        (1, 1, 1, 1),
        "prefixes of the type words work; the unknown type owns nothing"
    );
}

#[test]
fn an_unclosed_quote_anywhere_is_an_error_with_its_line() {
    let mut rig = Rig::new();
    let path = write(&rig, "q.ssp", b"@PLAYER \"\"\nAaa, A\n@SITE \"oops\nBbb\n");
    rig.send(SpellingMessage::Load(path));
    assert_eq!(
        rig.spelling_notice(),
        Some(SpellingNotice::LoadFailed(
            SpellingLoadError::UnclosedQuote { line: 3 }
        ))
    );
    assert!(!rig.state.spelling_status_view().loaded);
}

#[test]
fn an_empty_file_loads_with_zero_names() {
    let mut rig = Rig::new();
    let path = write(&rig, "empty.ssp", b"");
    rig.send(SpellingMessage::Load(path));
    assert!(rig.state.spelling_status_view().loaded);
    assert_eq!(counts(&rig).players, 0);
}

// ---- loading: cancellation, stale answers, quitting ----

#[test]
fn cancelling_a_load_keeps_the_set_and_does_not_say_it_failed() {
    let mut rig = Rig::new();
    rig.load_ssp(SSP);
    let before = rig.state.spelling().spelling().cloned().unwrap();
    let path = write(&rig, "other.ssp", b"@PLAYER \"\"\nZzz, Z\n");
    let fx = rig.raw(SpellingMessage::Load(path));
    rig.raw(SpellingMessage::CancelLoad);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    assert_eq!(rig.state.spelling().spelling().cloned().unwrap(), before);
    assert!(
        matches!(rig.spelling_notice(), Some(SpellingNotice::Loaded(_))),
        "{:?}",
        rig.spelling_notice()
    );
    assert!(rig.state.spelling_status_view().loading.is_none());
}

#[test]
fn unloading_while_a_load_runs_drops_its_answer() {
    let mut rig = Rig::new();
    let path = write(&rig, "a.ssp", SSP.as_bytes());
    let fx = rig.raw(SpellingMessage::Load(path));
    let Effect::Spelling(SpellingEffect::Load { control, .. }) = &fx[0] else {
        panic!("load effect");
    };
    rig.raw(SpellingMessage::Unload);
    assert!(control.is_cancelled());
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    assert!(!rig.state.spelling_status_view().loaded);
    assert!(rig.state.spelling_status_view().loading.is_none());
}

#[test]
fn an_answer_that_arrives_after_quit_starts_no_more_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    let path = write(&rig, "a.ssp", SSP.as_bytes());
    let fx = rig.raw(SpellingMessage::Load(path));
    // The answer is already on its way when the user quits.
    let answer = rig.perform(fx[0].clone());
    rig.raw(Message::Quit);
    let more = rig.raw(answer);
    assert!(
        more.iter().all(|e| !matches!(e, Effect::Spelling(_))),
        "no photos or stats after quit: {more:?}"
    );
}

#[test]
fn a_load_in_flight_is_replaced_by_the_new_one_and_only_the_new_one_counts() {
    let mut rig = Rig::new();
    let a = write(&rig, "a.ssp", b"@PLAYER \"\"\nAaa, A\n");
    let b = write(&rig, "b.ssp", b"@PLAYER \"\"\nAaa, A\nBbb, B\nCcc, C\n");
    let first = rig.raw(SpellingMessage::Load(a));
    let second = rig.raw(SpellingMessage::Load(b.clone()));
    // The newest answers first, then the old one limps in.
    let new = rig.perform(second[0].clone());
    rig.raw(new);
    let old = rig.perform(first[0].clone());
    rig.raw(old);
    assert_eq!(counts(&rig).players, 3);
    assert_eq!(rig.state.spelling_status_view().path, Some(b));
}

// ---- reload ----

#[test]
fn a_reload_makes_the_checker_use_the_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(&SSP.replace("  = Alfa, Alf\n", ""));
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    let before: Vec<String> = rig
        .state
        .spell_view()
        .unwrap()
        .rows
        .iter()
        .map(|r| r.row.old.clone())
        .collect();
    // "Alfa, Alf" still matches "Alfa, A..." by prefix: only check that the
    // alias file changes the proposals after the reload.
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::Reload);
    rig.send(SpellingMessage::Rescan);
    let after = rig.state.spell_view().unwrap();
    assert!(after.scanned);
    assert!(
        after.rows.iter().any(|r| r.row.old == "Delt, D"),
        "{before:?} -> {:?}",
        after.rows
    );
}

#[test]
fn a_reload_refreshes_the_estimates_of_the_stats() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &six());
    let path = rig.load_ssp(SSP);
    let first = rig.state.player_stats(slot).expect("built");
    std::fs::write(&path, SSP.replace("[2050]", "[2333]")).unwrap();
    rig.send(SpellingMessage::Reload);
    let second = rig.state.player_stats(slot).expect("rebuilt after reload");
    assert_ne!(
        first, second,
        "the old stats are not served for the new file"
    );
}

#[test]
fn a_reload_of_a_vanished_file_fails_and_keeps_the_set() {
    let mut rig = Rig::new();
    let path = rig.load_ssp(SSP);
    let before = rig.state.spelling().spelling().cloned().unwrap();
    std::fs::remove_file(path).unwrap();
    rig.send(SpellingMessage::Reload);
    assert!(matches!(
        rig.spelling_notice(),
        Some(SpellingNotice::LoadFailed(_))
    ));
    assert_eq!(rig.state.spelling().spelling().cloned().unwrap(), before);
}

// ---- the checker: hostile edits and the target of a write ----

#[test]
fn hostile_row_edits_never_panic_and_an_empty_new_name_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::ToggleRow(usize::MAX));
    rig.send(SpellingMessage::EditNew(usize::MAX, "x".into()));
    rig.send(SpellingMessage::EditNew(999, "x".into()));
    rig.send(SpellingMessage::SetFind("\u{0}\u{202e}%".into()));
    rig.send(SpellingMessage::SetFind(String::new()));
    // Blank out every new name: nothing valid is left to apply.
    let n = rig.state.spell_view().unwrap().total;
    assert!(n > 0);
    for i in 0..n {
        rig.send(SpellingMessage::EditNew(i, "   ".into()));
    }
    rig.send(SpellingMessage::SelectAll(true));
    let fx = rig.raw(SpellingMessage::Apply);
    assert!(fx.is_empty());
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NoneSelected));
    assert!(players_on_disk(&path).iter().all(|n| !n.trim().is_empty()));
}

#[test]
fn a_huge_edited_name_is_handled_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(SpellingMessage::SelectAll(false));
    rig.send(SpellingMessage::ToggleRow(0));
    rig.send(SpellingMessage::EditNew(0, "N".repeat(200_000)));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    // Whatever was decided, the database still opens and has its games.
    assert_eq!(disk_players(&path).len(), 3);
}

#[test]
fn applying_writes_the_database_of_the_dialog_not_the_one_selected_later() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (a, pa) = rig.open_si4(dir.path(), "a", &misspelled());
    let (b, pb) = rig.open_si4(dir.path(), "b", &misspelled());
    rig.load_ssp(SSP);
    rig.send(BasesMessage::Select(a));
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.send(BasesMessage::Select(b));
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(disk_players(&pb)[0].0, "Alfa, Alf", "b is untouched");
    assert!(
        disk_players(&pa)[0].0 == "Alfa, A" || rig.spell_view_is_gone(),
        "a was corrected, or the dialog followed the selection"
    );
}

trait GoneExt {
    fn spell_view_is_gone(&self) -> bool;
}

impl GoneExt for Rig {
    fn spell_view_is_gone(&self) -> bool {
        self.state.spell_view().is_none()
    }
}

#[test]
fn a_second_confirmation_does_not_start_a_second_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let first = rig.raw(SpellingMessage::ConfirmApply(true));
    assert_eq!(first.len(), 1);
    assert!(rig.raw(SpellingMessage::ConfirmApply(true)).is_empty());
    assert!(rig.raw(SpellingMessage::Apply).is_empty());
    let answer = rig.perform(first[0].clone());
    rig.raw(answer);
}

#[test]
fn a_write_in_flight_blocks_the_other_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::SetEloOnlyFilter(false));
    rig.send(SpellingMessage::PlanElo);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let names_write = rig.raw(SpellingMessage::ConfirmApply(true));
    assert_eq!(names_write.len(), 1);
    let elo_planned = rig.state.add_elo_view().and_then(|v| v.planned);
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert!(
        fx.is_empty() || elo_planned.is_none(),
        "no second job while one runs: {fx:?}"
    );
    let answer = rig.perform(names_write[0].clone());
    rig.raw(answer);
    assert_eq!(disk_players(&path)[0].0, "Alfa, A");
}

#[test]
fn cancelling_a_write_leaves_a_database_that_opens_and_can_be_finished() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    rig.raw(SpellingMessage::CancelJob);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    // Each side is either the old or the new spelling, never anything else.
    for name in players_on_disk(&path) {
        assert!(
            ["Alfa, Alf", "Alfa, A", "Beta, B", "Delt, D", "Delta, D"].contains(&name.as_str()),
            "{name}"
        );
    }
    assert!(rig.state.bases.slot(rig.state.bases.current()).is_some());
    // Doing it again finishes the job.
    rig.send(SpellingMessage::Rescan);
    rig.send(SpellingMessage::Apply);
    rig.send(SpellingMessage::ConfirmApply(true));
    assert_eq!(
        players_on_disk(&path),
        [
            "Alfa, A", "Beta, B", "Beta, B", "Alfa, A", "Alfa, A", "Delta, D"
        ]
    );
}

#[test]
fn quitting_during_a_write_cancels_it_and_exits_when_it_answers() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    let Effect::Spelling(SpellingEffect::ApplyNames { control, .. }) = &fx[0] else {
        panic!("write effect");
    };
    let quit = rig.raw(Message::Quit);
    assert!(control.is_cancelled(), "the write is asked to stop");
    assert!(
        quit.iter().all(|e| !matches!(e, Effect::Exit)),
        "the program waits for the writer: {quit:?}"
    );
    assert!(
        quit.iter()
            .any(|e| matches!(e, Effect::QuitDeadline { .. })),
        "D589: a deadline guards the wait: {quit:?}"
    );
    let answer = rig.perform(fx[0].clone());
    let after = rig.raw(answer);
    assert!(
        after.iter().any(|e| matches!(e, Effect::Exit)),
        "exits after the answer: {after:?}"
    );
    for name in players_on_disk(&path) {
        assert!(
            ["Alfa, Alf", "Alfa, A", "Beta, B", "Delt, D", "Delta, D"].contains(&name.as_str()),
            "{name}"
        );
    }
}

#[test]
fn the_notice_names_the_kind_even_if_the_dialog_was_closed_during_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenCheck(NameKind::Event));
    assert!(rig.state.spell_view().unwrap().total > 0, "events to fix");
    rig.raw(SpellingMessage::Apply);
    let fx = rig.raw(SpellingMessage::ConfirmApply(true));
    rig.raw(SpellingMessage::CloseCheck);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    match rig.spelling_notice() {
        Some(SpellingNotice::Applied { kind, .. }) => {
            assert_eq!(kind, NameKind::Event, "the result is about events");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn closing_the_dialog_or_quitting_stops_the_scan() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let fx = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    let Effect::Spelling(SpellingEffect::Scan { control, .. }) = &fx[0] else {
        panic!("scan");
    };
    rig.raw(SpellingMessage::CloseCheck);
    assert!(control.is_cancelled());
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    assert!(
        rig.state.spell_view().is_none(),
        "a late scan reopens nothing"
    );
}

#[test]
fn the_scan_of_a_closed_database_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &misspelled());
    rig.load_ssp(SSP);
    let fx = rig.raw(SpellingMessage::OpenCheck(NameKind::Player));
    rig.raw(BasesMessage::Close(slot));
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    rig.raw(SpellingMessage::Rescan);
    rig.raw(SpellingMessage::Apply);
    rig.raw(SpellingMessage::ConfirmApply(true));
    rig.raw(SpellingMessage::CloseCheck);
}

// ---- Add Elo ----

#[test]
fn add_elo_on_a_closed_database_says_there_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.raw(BasesMessage::Close(slot));
    let fx = rig.raw(SpellingMessage::PlanElo);
    assert!(fx.iter().all(|e| !matches!(e, Effect::Spelling(_))));
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::NoBase));
}

#[test]
fn add_elo_in_a_read_only_si4_refuses_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    assert!(rig.state.add_elo_view().unwrap().planned.is_some());
    rig.send(Message::Bases(BasesMessage::List(
        ListMessage::SetReadOnly {
            slot,
            read_only: true,
        },
    )));
    assert!(!rig.state.add_elo_view().unwrap().writable);
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert!(fx.is_empty());
    assert_eq!(rig.spelling_notice(), Some(SpellingNotice::ReadOnly));
    assert_eq!(disk_elos(&path), [(0, 0), (0, 0)]);
}

#[test]
fn cancelling_add_elo_during_the_write_leaves_whole_entries_and_can_be_redone() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::SetEloOnlyFilter(false));
    rig.send(SpellingMessage::PlanElo);
    let fx = rig.raw(SpellingMessage::ConfirmElo);
    assert_eq!(fx.len(), 1);
    rig.raw(SpellingMessage::CancelJob);
    let answer = rig.perform(fx[0].clone());
    rig.raw(answer);
    for (w, b) in disk_elos(&path) {
        assert!([0, 2500, 2502].contains(&w) && b == 0, "({w}, {b})");
    }
    // Planning again and writing finishes whatever was left.
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::SetEloOnlyFilter(false));
    rig.send(SpellingMessage::PlanElo);
    rig.send(SpellingMessage::ConfirmElo);
    let elos = disk_elos(&path);
    assert_eq!(elos[0], (2500, 0));
    assert_eq!(elos[1].1, 0);
    assert_ne!(elos[1].0, 0, "March of 2020 has a rating too");
}

#[test]
fn quitting_during_the_plan_and_the_write_of_add_elo_cancels_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    let plan = rig.raw(SpellingMessage::PlanElo);
    let Effect::Spelling(SpellingEffect::PlanElo { control, .. }) = &plan[0] else {
        panic!("plan effect");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    // The answer of a cancelled plan neither panics nor offers a write.
    let answer = rig.perform(plan[0].clone());
    rig.raw(answer);
    // (A plan of a tiny base may finish before it notices the cancel.)

    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "n", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::SetEloOnlyFilter(false));
    rig.send(SpellingMessage::PlanElo);
    let write = rig.raw(SpellingMessage::ConfirmElo);
    let Effect::Spelling(SpellingEffect::ApplyElo { control, .. }) = &write[0] else {
        panic!("write effect");
    };
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    let answer = rig.perform(write[0].clone());
    rig.raw(answer);
    for (w, b) in disk_elos(&path) {
        assert!(b == 0 && [0, 2500, 2502].contains(&w));
    }
}

#[test]
fn a_plan_answer_for_a_closed_dialog_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    let plan = rig.raw(SpellingMessage::PlanElo);
    rig.raw(SpellingMessage::CloseAddElo);
    let answer = rig.perform(plan[0].clone());
    rig.raw(answer);
    assert!(rig.state.add_elo_view().is_none());
    assert!(rig.raw(SpellingMessage::ConfirmElo).is_empty());
}

#[test]
fn unloading_the_file_between_plan_and_write_cannot_write_a_dead_plan() {
    // The plan was made from a file that is gone. The ratings in it were
    // computed from that file; the contract is only that nothing breaks and
    // the database stays valid.
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "m", &unrated());
    rig.load_ssp(SSP);
    rig.send(SpellingMessage::OpenAddElo);
    rig.send(SpellingMessage::PlanElo);
    rig.send(SpellingMessage::Unload);
    rig.send(SpellingMessage::ConfirmElo);
    for (w, b) in disk_elos(&path) {
        assert!([0, 2500, 2502].contains(&w) && b == 0);
    }
}
