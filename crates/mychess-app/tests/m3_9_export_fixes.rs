//! M3-9 review fixes (spec `docs/specs/m3-9-exportar-partidas.md`,
//! D2013, D2021-D2025).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m3_9_common;

use m3_9_common::*;
use mychess_app::{
    ChooserMessage, Command, ExportList, GamesExportJob, GamesExportOutcome, OpenControl, RequestId,
};
use tempfile::tempdir;

fn job_with(rig: &Rig, list: ExportList, dest: &std::path::Path) -> GamesExportJob {
    let id = rig.state.bases.current();
    GamesExportJob {
        source: rig.state.bases.slot(id).unwrap().source.clone(),
        list,
        only_tactics: false,
        dest: dest.to_path_buf(),
        confirmed: true,
        append: false,
        write: mychess_pgn::write::WriteOptions::default(),
        protected: Vec::new(),
    }
}

fn run_job(rig: &Rig, job: GamesExportJob) -> Result<Box<GamesExportOutcome>, BaseError> {
    let answer = rig.perform(Effect::ExportGames {
        request: RequestId(1),
        job: Box::new(job),
        control: OpenControl::new(),
    });
    match answer {
        Message::Bases(BasesMessage::List(ListMessage::GamesExported { result, .. })) => result,
        other => panic!("{other:?}"),
    }
}

#[test]
fn d2021_when_every_game_is_skipped_the_old_destination_stays() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    let dest = dir.path().join("keep.pgn");
    std::fs::write(&dest, "precious").unwrap();
    // Numbers that are not in the database: every game is unreadable.
    let job = job_with(&rig, ExportList::Numbers(vec![40, 41]), &dest);
    let outcome = run_job(&rig, job).unwrap();
    assert_eq!(outcome.report.written, 0);
    assert_eq!(outcome.report.skipped, 2);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "precious");
    assert!(!has_temp(dir.path()));
    // And no empty file appears where there was none.
    let fresh = dir.path().join("fresh.pgn");
    let job = job_with(&rig, ExportList::Numbers(vec![40]), &fresh);
    run_job(&rig, job).unwrap();
    assert!(!fresh.exists());
}

#[test]
fn d2021_a_partly_skipped_export_still_writes_the_good_games() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    let dest = dir.path().join("mix.pgn");
    let job = job_with(&rig, ExportList::Numbers(vec![2, 40]), &dest);
    let outcome = run_job(&rig, job).unwrap();
    assert_eq!((outcome.report.written, outcome.report.skipped), (1, 1));
    assert_eq!(outcome.report.lines.len(), 1);
    assert_eq!(whites_in(&text_of(&dest)), ["P2"]);
}

#[test]
fn d2013_the_pgn_file_on_the_board_is_protected() {
    let dir = tempdir().unwrap();
    for games in [1, 3] {
        let mut rig = Rig::new();
        rig.state.open_pgn_in_slots = false;
        let path = write(dir.path(), &format!("one{games}.pgn"), &numbered(games));
        let before = std::fs::read(&path).unwrap();
        let effects = rig.raw(Command::Open);
        let spec = pick_of(&effects, PickKind::OpenPgn);
        rig.send(Message::FilePicked(spec.request, Some(path.clone())));
        if games > 1 {
            rig.send(ChooserMessage::Confirm);
        }
        rig.gx_all(&path);
        assert_eq!(std::fs::read(&path).unwrap(), before, "{games}");
        match rig.state.games_export_view().expect("report").step {
            GamesExportStepView::Done(result) => {
                assert_eq!(result.error, Some(BaseError::ExportOverBase), "{games}");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn d2013_a_link_to_the_open_base_is_the_same_file() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let src = write(dir.path(), "src.pgn", &numbered(2));
    rig.open_pgn(dir.path(), "src.pgn", &numbered(2));
    let link = dir.path().join("link.pgn");
    std::os::unix::fs::symlink(&src, &link).unwrap();
    let before = std::fs::read(&src).unwrap();
    rig.gx_all(&link);
    assert_eq!(std::fs::read(&src).unwrap(), before);
    assert!(link.is_symlink(), "the link was not replaced");
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => {
            assert_eq!(result.error, Some(BaseError::ExportOverBase));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn d2012_a_changed_name_is_never_appended_to_either() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    std::fs::write(dir.path().join("a.pgn"), "mine").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, _) = rig.gx_choose(&dir.path().join("a"));
    rig.gx_run(effects);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.pgn")).unwrap(),
        "mine"
    );
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => assert!(matches!(
            result.error,
            Some(BaseError::File(ref e)) if e.kind == FileErrorKind::AlreadyExists
        )),
        other => panic!("{other:?}"),
    }
}

#[test]
fn d2010_a_failed_rename_leaves_no_temporary_and_the_old_destination() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    // A directory where the file should go: the rename at the end fails.
    let dest = dir.path().join("x.pgn");
    std::fs::create_dir(&dest).unwrap();
    rig.gx_all(&dest);
    assert!(dest.is_dir());
    assert!(!has_temp(dir.path()));
    match rig.state.games_export_view().unwrap().step {
        GamesExportStepView::Done(result) => {
            assert!(matches!(result.error, Some(BaseError::File(_))));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn d2010_a_file_named_like_the_temporary_is_left_alone() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(2));
    std::fs::write(dir.path().join("t.pgn.tmp"), "mine").unwrap();
    rig.gx_all(&dir.path().join("t.pgn"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("t.pgn.tmp")).unwrap(),
        "mine"
    );
    assert_eq!(whites_in(&text_of(&dir.path().join("t.pgn"))), ["P1", "P2"]);
}

#[test]
fn d2012_a_reserved_name_without_extension_goes_through_end_to_end() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(1));
    rig.gx_all(&dir.path().join("nul"));
    assert!(dir.path().join("_nul.pgn").exists());
}

#[test]
fn d2023_deleted_games_are_exported_with_all_and_the_filter() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let path = make_si4(dir.path(), "d", &numbered(3));
    {
        let mut w = mychess_db::si4::Si4Writer::open(&path).unwrap();
        w.set_flag(
            2,
            mychess_db::si4::GameFlags::DELETE,
            mychess_db::si4::FlagOp::Set,
        )
        .unwrap();
    }
    rig.send(BasesMessage::Open(path));
    let dest = dir.path().join("d.pgn");
    rig.gx_all(&dest);
    assert_eq!(whites_in(&text_of(&dest)), ["P1", "P2", "P3"]);
}

#[test]
fn d2024_quitting_during_the_export_cancels_it_and_waits_at_most_five_seconds() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    rig.gx(GamesExportMessage::Open);
    let dest = dir.path().join("q.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    let control = effects
        .iter()
        .find_map(|e| match e {
            Effect::ExportGames { control, .. } => Some(control.clone()),
            _ => None,
        })
        .unwrap();
    let quit = rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    let delay = quit
        .iter()
        .find_map(|e| match e {
            Effect::QuitDeadline { delay } => Some(*delay),
            _ => None,
        })
        .expect("a deadline");
    assert!(delay <= std::time::Duration::from_secs(5), "{delay:?}");
    assert!(!quit.iter().any(|e| matches!(e, Effect::Exit)));
    // The answer of the cancelled job lets the quit go on.
    let mut exits = false;
    for effect in effects {
        let answer = rig.perform(effect);
        exits |= rig.raw(answer).iter().any(|e| matches!(e, Effect::Exit));
    }
    assert!(exits, "the quit goes on after the job answered");
    assert!(!dest.exists());
}
