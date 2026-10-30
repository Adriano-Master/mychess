//! M3-8 contract: importing PGN files into the `.si4` of the current slot
//! (spec `docs/specs/m3-8-importar-arquivo-pgn.md`, D1970-D1984).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m3_8_common;

use m3_8_common::*;
use tempfile::tempdir;

#[test]
fn d1972_one_clean_file_is_appended_and_the_window_closes_by_itself() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "base", &games(2));
    let pgn = write(dir.path(), "more.pgn", &games(3));
    let host = rig.import(&[pgn]);
    assert!(host.is_empty(), "{host:?}");
    assert_eq!(rig.state.bases.current(), slot);
    assert_eq!(games_on_disk(&path), 5);
    assert_eq!(rig.current_total(), 5);
    // D1981: the new games are in the filter.
    assert_eq!(rig.current_filtered(), 5);
    // D1984: no warning, one file: no window.
    assert!(rig.state.file_import_view().is_none());
    assert!(!rig.state.modal_open());
    assert_eq!(rig.notice(), Some(FileImportNotice::Imported { count: 3 }));
    assert_eq!(white_of(&path, 3), "White 1");
}

#[test]
fn d1971_several_files_keep_the_window_with_the_report() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let a = write(dir.path(), "a.pgn", &games(2));
    let b = write(dir.path(), "b.pgn", &games(4));
    rig.import(&[a, b]);
    assert_eq!(games_on_disk(&path), 7);
    let view = rig.state.file_import_view().expect("window stays");
    assert!(!view.running);
    let result = view.result.expect("result");
    assert_eq!(result.report.games_added, 6);
    assert_eq!(result.report.files_read, 2);
    assert!(rig.state.modal_open(), "D1985: the window is modal");
    rig.send(FileImportMessage::Close);
    assert!(rig.state.file_import_view().is_none());
    assert!(!rig.state.modal_open());
}

#[test]
fn d1982_importing_the_same_file_twice_duplicates_the_games() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let pgn = write(dir.path(), "x.pgn", &games(2));
    rig.import(std::slice::from_ref(&pgn));
    rig.import(&[pgn]);
    assert_eq!(games_on_disk(&path), 5);
    assert_eq!(rig.current_filtered(), 5);
}

#[test]
fn variations_and_comments_are_stored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let pgn = write(
        dir.path(),
        "v.pgn",
        "[Event \"V\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n\
         1. e4 {a comment} e5 (1... c5 2. Nf3) 2. Nf3 *\n\n",
    );
    rig.import(&[pgn]);
    let db = mychess_db::si4::Si4Database::open(&path).unwrap();
    let entry = db.entry(2).unwrap();
    assert_eq!(entry.variation_count, 1);
    assert_eq!(entry.comment_count, 1);
}

#[test]
fn latin1_names_are_decoded() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let mut bytes = b"[Event \"E\"]\n[White \"Jos".to_vec();
    bytes.push(0xE9);
    bytes.extend_from_slice(b"\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n");
    let pgn = write_bytes(dir.path(), "latin1.pgn", &bytes);
    rig.import(&[pgn]);
    assert_eq!(white_of(&path, 2), "Jos\u{e9}");
}

#[test]
fn gzip_files_are_accepted() {
    use std::io::Write as _;
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(games(3).as_bytes()).unwrap();
    let pgn = write_bytes(dir.path(), "z.pgn.gz", &encoder.finish().unwrap());
    rig.import(&[pgn]);
    assert_eq!(games_on_disk(&path), 4);
}

#[test]
fn d1983_a_corrupt_game_in_the_middle_is_reported_and_the_rest_is_stored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let mut text = game(1);
    text.push_str(
        "[Event \"Bad\"]\n[White \"X\"]\n[Black \"Y\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Qh8 Nf6 3. Zz9 1-0\n\n",
    );
    text.push_str(&game(3));
    let pgn = write(dir.path(), "bad.pgn", &text);
    rig.import(&[pgn]);
    let view = rig
        .state
        .file_import_view()
        .expect("problems keep the window");
    let result = view.result.expect("result");
    assert!(result.report.has_problems(), "{result:?}");
    assert!(!result.report.lines.is_empty());
    assert!(result.report.lines.iter().all(|l| l.game > 0));
    // The good games are in, whatever happened to the bad one.
    assert!(games_on_disk(&path) >= 3, "{}", games_on_disk(&path));
    assert_eq!(result.report.games_read, 3);
}

#[test]
fn an_empty_file_stores_nothing_and_says_so() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(2));
    let pgn = write(dir.path(), "empty.pgn", "");
    rig.import(&[pgn]);
    assert_eq!(games_on_disk(&path), 2);
    assert_eq!(rig.notice(), Some(FileImportNotice::NothingImported));
}

#[test]
fn d1983_a_missing_file_is_an_error_line_and_the_others_go_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let missing = dir.path().join("missing.pgn");
    let good = write(dir.path(), "good.pgn", &games(2));
    rig.import(&[missing, good]);
    assert_eq!(games_on_disk(&path), 3);
    let result = rig
        .state
        .file_import_view()
        .and_then(|v| v.result)
        .expect("result");
    assert_eq!(result.report.file_errors.len(), 1);
    assert_eq!(result.report.file_errors[0].0, 0);
    assert!(matches!(result.report.file_errors[0].1, BaseError::File(_)));
}

#[test]
fn d1980_a_base_changed_behind_the_slot_is_refused() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(2));
    let pgn = write(dir.path(), "x.pgn", &games(2));
    let effects = rig.start_import(&[pgn]);
    assert!(effects.iter().any(is_import_job));
    // Somebody else adds a game before the job runs.
    {
        use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
        let mut writer = mychess_db::si4::Si4Writer::open(&path).unwrap();
        import_pgn(
            game(9).as_bytes(),
            &mut writer,
            &ImportOptions::default(),
            None,
            &mut NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
    }
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert_eq!(games_on_disk(&path), 3, "nothing was written");
    let result = rig
        .state
        .file_import_view()
        .and_then(|v| v.result)
        .expect("result");
    assert_eq!(result.error, Some(BaseError::Stale));
    assert_eq!(
        rig.notice(),
        Some(FileImportNotice::NotStarted(BaseError::Stale))
    );
}

#[test]
fn d1978_stopping_before_the_job_starts_writes_nothing_and_closes_quietly() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(2));
    let pgn = write(dir.path(), "x.pgn", &games(5));
    let effects = rig.start_import(&[pgn]);
    assert!(rig.state.modal_open());
    rig.send(BasesMessage::List(ListMessage::CancelJob));
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert_eq!(games_on_disk(&path), 2);
    assert!(rig.state.file_import_view().is_none());
}

#[test]
fn d1976_one_job_at_a_time_and_old_answers_are_dropped() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let pgn = write(dir.path(), "x.pgn", &games(2));
    let effects = rig.start_import(std::slice::from_ref(&pgn));
    // The window is modal: a second "Import" does nothing.
    assert!(rig.raw(FileImportMessage::Open).is_empty());
    // An answer with another request is dropped: the job stays.
    let stale = Message::Bases(BasesMessage::List(ListMessage::PgnImported {
        request: RequestId(999_999),
        result: Err(BaseError::Internal),
    }));
    rig.raw(stale);
    assert!(rig.state.list_job().is_some());
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert!(rig.state.list_job().is_none());
    assert_eq!(games_on_disk(&path), 3);
}

#[test]
fn a_busy_list_refuses_the_import_with_a_notice() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "base", &games(3));
    // A sort job in flight (not performed).
    let effects = rig.raw(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: mychess_app::ListColumn::White,
    }));
    assert!(!effects.is_empty());
    assert!(rig.raw(FileImportMessage::Open).is_empty());
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::ListBusy))
    ));
}

#[test]
fn d1987_quitting_during_the_import_cancels_it_and_waits_for_the_answer() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(1));
    let pgn = write(dir.path(), "x.pgn", &games(5));
    let effects = rig.start_import(&[pgn]);
    let control = effects
        .iter()
        .find_map(|e| match e {
            Effect::ImportPgnFiles { control, .. } => Some(control.clone()),
            _ => None,
        })
        .expect("job");
    let quit = rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    assert!(
        quit.iter()
            .any(|e| matches!(e, Effect::QuitDeadline { .. })),
        "{quit:?}"
    );
    assert!(!quit.iter().any(|e| matches!(e, Effect::Exit)));
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert_eq!(games_on_disk(&path), 1, "cancelled before anything");
}

#[test]
fn d1985_the_window_blocks_the_other_commands_and_close_waits_for_the_end() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "base", &games(1));
    let pgn = write(dir.path(), "x.pgn", &games(2));
    let effects = rig.start_import(&[pgn]);
    assert!(rig.state.modal_open());
    // Close does nothing while the job runs.
    rig.raw(FileImportMessage::Close);
    assert!(rig.state.file_import_view().is_some());
    let view = rig.state.file_import_view().unwrap();
    assert!(view.running);
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert!(!rig.state.modal_open());
}

#[test]
fn d1981_the_game_being_edited_is_not_lost_by_the_import() {
    use mychess_app::{GameMessage, PlayOptions};
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "base", &games(2));
    rig.send(Message::Command(mychess_app::Command::NewGame));
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    rig.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(rig.state.is_dirty());
    let before = rig.state.session.position().fen();
    let pgn = write(dir.path(), "x.pgn", &games(3));
    rig.import(&[pgn]);
    assert!(rig.state.is_dirty(), "the edit is still pending");
    assert_eq!(rig.state.session.position().fen(), before);
    assert_eq!(rig.current_total(), 5);
}
