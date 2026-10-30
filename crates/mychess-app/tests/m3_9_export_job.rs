//! M3-9 contract: the job, the destination and the report (spec
//! `docs/specs/m3-9-exportar-partidas.md`, D2002-D2015, D2017-D2020).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m3_9_common;

use m3_9_common::*;
use tempfile::tempdir;

fn job_of(effects: &[Effect]) -> mychess_app::OpenControl {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::ExportGames { control, .. } => Some(control.clone()),
            _ => None,
        })
        .expect("an export job")
}

#[test]
fn d2002_the_dialog_kind_follows_the_append_option_and_proposes_the_base_name() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "mybase", &numbered(2));
    rig.gx(GamesExportMessage::Open);
    let effects = rig.raw(GamesExportMessage::Choose);
    let spec = pick_of(&effects, PickKind::SavePgn);
    assert_eq!(spec.file_name.as_deref(), Some("mybase.pgn"));
    // Answering "cancelled" goes back to the options.
    rig.raw(Message::FilePicked(spec.request, None));
    assert!(matches!(
        rig.state.games_export_view().unwrap().step,
        GamesExportStepView::Options
    ));
    rig.gx(GamesExportMessage::SetAppend(true));
    let effects = rig.raw(GamesExportMessage::Choose);
    pick_of(&effects, PickKind::AppendPgn);
}

#[test]
fn d2018_an_old_dialog_answer_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    rig.gx(GamesExportMessage::Open);
    let effects = rig.raw(GamesExportMessage::Choose);
    let spec = pick_of(&effects, PickKind::SavePgn);
    let wrong = mychess_app::RequestId(spec.request.0 + 100);
    let out = rig.raw(Message::FilePicked(wrong, Some(dir.path().join("x.pgn"))));
    assert!(out.is_empty());
    // Closing while the dialog is open drops the window; the late answer
    // starts nothing.
    rig.gx(GamesExportMessage::Close);
    let out = rig.raw(Message::FilePicked(
        spec.request,
        Some(dir.path().join("x.pgn")),
    ));
    assert!(out.is_empty());
    assert!(!dir.path().join("x.pgn").exists());
}

#[test]
fn d2005_one_job_at_a_time_and_the_window_is_modal() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    rig.gx(GamesExportMessage::Open);
    assert!(rig.state.modal_open());
    let dest = dir.path().join("o.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    let (kind, _) = rig.state.list_job().expect("job in flight");
    assert_eq!(kind, ListJobKind::ExportGames);
    // While it runs: Close does nothing, a second Open says "busy".
    rig.gx(GamesExportMessage::Close);
    assert!(rig.state.games_export_view().is_some());
    rig.gx(GamesExportMessage::Open);
    assert!(matches!(
        rig.state.games_export_view().unwrap().step,
        GamesExportStepView::Running { .. }
    ));
    rig.gx_run(effects);
    assert!(rig.state.list_job().is_none());
}

#[test]
fn d2010_cancelling_discards_the_temporary_and_keeps_the_old_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    let dest = dir.path().join("keep.pgn");
    std::fs::write(&dest, "old content").unwrap();
    rig.gx(GamesExportMessage::Open);
    let (effects, _) = rig.gx_choose(&dest);
    let control = job_of(&effects);
    control.cancel();
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old content");
    assert!(!has_temp(dir.path()));
    assert_eq!(notice(&rig), Some(GamesExportNotice::Stopped));
    assert!(rig.state.games_export_view().is_none());
}

#[test]
fn d2010_cancelling_in_the_middle_leaves_no_partial_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "big", &numbered(40_000));
    rig.gx(GamesExportMessage::Open);
    let dest = dir.path().join("big.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    let control = job_of(&effects);
    let watcher = {
        let control = control.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while control.progress().0 == 0 && started.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            control.cancel();
        })
    };
    rig.gx_run(effects);
    watcher.join().unwrap();
    assert!(!dest.exists(), "no partial file");
    assert!(!has_temp(dir.path()));
    assert_eq!(notice(&rig), Some(GamesExportNotice::Stopped));
}

#[test]
fn d2011_a_cancelled_append_keeps_what_was_added() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "big", &numbered(40_000));
    let dest = dir.path().join("log.pgn");
    std::fs::write(&dest, "% old\n").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, kind) = rig.gx_choose(&dest);
    assert_eq!(kind, PickKind::AppendPgn);
    let control = job_of(&effects);
    let watcher = {
        let control = control.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while control.progress().0 == 0 && started.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            control.cancel();
        })
    };
    rig.gx_run(effects);
    watcher.join().unwrap();
    let text = text_of(&dest);
    assert!(text.starts_with("% old\n[Event"), "{}", &text[..40]);
    assert!(matches!(
        notice(&rig),
        Some(GamesExportNotice::StoppedAppended { count }) if count > 0
    ));
}

#[test]
fn d2011_append_adds_to_the_end_and_a_missing_file_is_created() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    let dest = dir.path().join("a.pgn");
    std::fs::write(&dest, "% old\n").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    let text = text_of(&dest);
    assert!(text.starts_with("% old\n"), "{text}");
    assert_eq!(whites_in(&text), ["P1", "P2"]);
    let fresh = dir.path().join("fresh.pgn");
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, _) = rig.gx_choose(&fresh);
    rig.gx_run(effects);
    assert_eq!(whites_in(&text_of(&fresh)), ["P1", "P2"]);
}

#[test]
fn d2010_an_existing_file_confirmed_by_the_dialog_is_replaced() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    let dest = dir.path().join("r.pgn");
    std::fs::write(&dest, "old").unwrap();
    rig.gx_all(&dest);
    assert_eq!(whites_in(&text_of(&dest)), ["P1", "P2"]);
    assert!(!text_of(&dest).contains("old"));
}

#[test]
fn d2012_the_extension_is_added_but_an_existing_file_of_that_name_is_not_replaced() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    // The dialog confirmed `r`, not `r.pgn`.
    std::fs::write(dir.path().join("r.pgn"), "mine").unwrap();
    rig.gx_all(&dir.path().join("r"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("r.pgn")).unwrap(),
        "mine"
    );
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => assert!(matches!(
            result.error,
            Some(BaseError::File(ref e)) if e.kind == FileErrorKind::AlreadyExists
        )),
        other => panic!("{other:?}"),
    }
    rig.gx(GamesExportMessage::Close);
    rig.gx_all(&dir.path().join("fresh"));
    assert_eq!(whites_in(&text_of(&dir.path().join("fresh.pgn"))).len(), 2);
}

#[test]
fn d2012_invalid_windows_characters_in_the_name_are_replaced() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(1));
    rig.gx_all(&dir.path().join("a:b?.pgn"));
    assert!(dir.path().join("a_b_.pgn").exists());
    rig.gx_all(&dir.path().join("CON.pgn"));
    assert!(dir.path().join("_CON.pgn").exists());
}

#[test]
fn d2013_the_file_of_an_open_pgn_base_is_never_overwritten() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "src.pgn", &numbered(3));
    let src = dir.path().join("src.pgn");
    let before = std::fs::read(&src).unwrap();
    rig.gx_all(&src);
    assert_eq!(std::fs::read(&src).unwrap(), before);
    assert!(!has_temp(dir.path()));
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => {
            assert_eq!(result.error, Some(BaseError::ExportOverBase));
        }
        other => panic!("{other:?}"),
    }
    assert!(rig.state.modal_open());
    rig.gx(GamesExportMessage::Close);
    assert!(!rig.state.modal_open());
    // The same goes for appending, and for another spelling of the path.
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, _) = rig.gx_choose(&dir.path().join(".").join("SRC.pgn"));
    rig.gx_run(effects);
    assert_eq!(std::fs::read(&src).unwrap(), before);
}

#[test]
fn d2014_a_missing_directory_is_reported_and_leaves_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    rig.gx_all(&dir.path().join("nowhere").join("x.pgn"));
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => {
            assert!(matches!(result.error, Some(BaseError::File(_))));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        notice(&rig),
        Some(GamesExportNotice::Failed(BaseError::File(_)))
    ));
}

#[test]
fn d2014_a_read_only_destination_file_is_refused_and_left_alone() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    let dest = dir.path().join("ro.pgn");
    std::fs::write(&dest, "locked").unwrap();
    let mut perms = std::fs::metadata(&dest).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&dest, perms).unwrap();
    rig.gx_all(&dest);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "locked");
    assert!(!has_temp(dir.path()));
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => assert!(matches!(
            result.error,
            Some(BaseError::File(ref e)) if e.kind == FileErrorKind::PermissionDenied
        )),
        other => panic!("{other:?}"),
    }
    let mut perms = std::fs::metadata(&dest).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&dest, perms).unwrap();
}

#[cfg(unix)]
#[test]
fn d2014_a_read_only_directory_is_reported() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    let out = dir.path().join("ro");
    std::fs::create_dir(&out).unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root ignores the mode: skip then.
    let writable = std::fs::write(out.join("probe"), "x").is_ok();
    if !writable {
        rig.gx_all(&out.join("x.pgn"));
        match rig.state.games_export_view().unwrap().step {
            GamesExportStepView::Done(result) => assert!(matches!(
                result.error,
                Some(BaseError::File(ref e)) if e.kind == FileErrorKind::PermissionDenied
            )),
            other => panic!("{other:?}"),
        }
    }
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn d2009_the_file_is_utf8_without_bom_and_uses_line_feeds() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(
        dir.path(),
        "u.pgn",
        "[Event \"\u{e9}t\u{e9}\"]\n[White \"Mu\u{f1}oz\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 *\n\n",
    );
    let dest = dir.path().join("o.pgn");
    rig.gx_all(&dest);
    let bytes = std::fs::read(&dest).unwrap();
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(!bytes.contains(&b'\r'));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("Mu\u{f1}oz"));
}

#[test]
fn d2009_comments_variations_and_marks_follow_the_options() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(
        dir.path(),
        "v.pgn",
        "[Event \"V\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n\
         1. e4 {[%eval 0.3] nice} e5 (1... c5 2. Nf3) 2. Nf3 *\n\n",
    );
    let run = |rig: &mut Rig, name: &str, set: &[GamesExportMessage]| {
        rig.gx(GamesExportMessage::Open);
        for m in set {
            rig.gx(*m);
        }
        let dest = dir.path().join(name);
        let (effects, _) = rig.gx_choose(&dest);
        rig.gx_run(effects);
        text_of(&dest)
    };
    let full = run(&mut rig, "full.pgn", &[]);
    assert!(full.contains("nice") && full.contains("(1... c5") && full.contains("[%eval"));
    let plain = run(
        &mut rig,
        "plain.pgn",
        &[
            GamesExportMessage::SetComments(false),
            GamesExportMessage::SetVariations(false),
        ],
    );
    assert!(!plain.contains("nice") && !plain.contains("c5"));
    let no_marks = run(
        &mut rig,
        "nomarks.pgn",
        &[GamesExportMessage::SetMarks(false)],
    );
    assert!(no_marks.contains("nice") && !no_marks.contains("[%eval"));
}

#[test]
fn d2020_round_trip_gives_the_same_games_with_variations_and_comments() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let text = "[Event \"V\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n\
                1. e4 {a comment} e5 (1... c5 2. Nf3) 2. Nf3 *\n\n";
    let all = format!("{text}{}", numbered(3));
    rig.open_pgn(dir.path(), "src.pgn", &all);
    let dest = dir.path().join("rt.pgn");
    rig.gx_all(&dest);
    // Reimport the exported file into a new base.
    let mut other = Rig::new();
    let (_, path) = other.open_si4(dir.path(), "re", &games(1));
    other.import(&[dest]);
    assert_eq!(games_on_disk(&path), 1 + 4);
    let db = mychess_db::si4::Si4Database::open(&path).unwrap();
    let entry = mychess_db::GameSource::entry(&db, 2).unwrap();
    assert_eq!(entry.variation_count, 1);
    assert_eq!(entry.comment_count, 1);
    assert_eq!(white_of(&path, 3), "P1");
}

#[test]
fn d2019_the_job_is_an_effect_and_update_does_not_touch_the_disk() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    rig.gx(GamesExportMessage::Open);
    let dest = dir.path().join("lazy.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ExportGames { .. }))
    );
    assert!(!dest.exists());
}
