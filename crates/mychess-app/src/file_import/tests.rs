//! Tests of the import of PGN files that need the internals of the
//! application (a slot closed under a running job) or a job stopped at a
//! known point (M3-8 review, D1990-D1993).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use mychess_db::GameSource;
use mychess_db::si4::{Si4Database, Si4Writer};

use super::work::run;
use super::{FileImportJob, FileImportMessage, FileImportNotice};
use crate::bases::{BaseError, BasesMessage, BasesNotice, ListMessage, OpenControl, SlotId};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::host::PickKind;
use crate::state::update;
use crate::{AppState, Message};

fn game(n: usize) -> String {
    format!(
        "[Event \"E{n}\"]\n[White \"W{n}\"]\n[Black \"B{n}\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n"
    )
}

fn games(count: usize) -> String {
    (1..=count).map(game).collect()
}

fn make_si4(dir: &Path, name: &str, count: usize) -> PathBuf {
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).unwrap();
    import_pgn(
        games(count).as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    drop(writer);
    base.with_extension("si4")
}

fn on_disk(path: &Path) -> u32 {
    GameSource::len(&Si4Database::open(path).unwrap())
}

struct Rig {
    state: AppState,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    fn new() -> Rig {
        let mut state = AppState::default();
        state.open_pgn_in_slots = true;
        Rig {
            state,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
        }
    }

    fn drive(&mut self, message: impl Into<Message>) {
        let mut queue = vec![message.into()];
        while let Some(message) = queue.pop() {
            for effect in update(&mut self.state, message) {
                if !effect.runs_on_host() {
                    queue.push(self.rt.block_on(crate::perform(effect)));
                }
            }
        }
    }

    fn open(&mut self, path: &Path) -> SlotId {
        self.drive(BasesMessage::Open(path.to_owned()));
        self.state.bases.current()
    }

    /// Starts an import into the current slot; returns the pieces of the job.
    fn start(&mut self, files: &[PathBuf]) -> (RequestId, FileImportJob, OpenControl) {
        let effects = update(&mut self.state, FileImportMessage::Open.into());
        let spec = effects
            .into_iter()
            .find_map(|e| match e {
                Effect::PickFile(spec) if spec.kind == PickKind::ImportPgn => Some(spec),
                _ => None,
            })
            .unwrap();
        let effects = update(
            &mut self.state,
            Message::FilesPicked(spec.request, files.to_vec()),
        );
        effects
            .into_iter()
            .find_map(|e| match e {
                Effect::ImportPgnFiles {
                    request,
                    job,
                    control,
                } => Some((request, *job, control)),
                _ => None,
            })
            .expect("a job")
    }

    fn answer(&mut self, request: RequestId, result: Result<super::FileImportOutcome, BaseError>) {
        self.drive(Message::Bases(BasesMessage::List(
            ListMessage::PgnImported {
                request,
                result: result.map(Box::new),
            },
        )));
    }
}

fn pgn(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_slot_closed_under_the_job_does_not_update_another_slot_and_the_report_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a_path = make_si4(dir.path(), "a", 2);
    let c_path = make_si4(dir.path(), "c", 4);
    let a = rig.open(&a_path);
    let a_canonical = rig.state.bases.slot(a).unwrap().path.clone().unwrap();
    let source = pgn(dir.path(), "x.pgn", &games(3));
    let second = pgn(dir.path(), "y.pgn", &games(0));
    let (request, job, control) = rig.start(&[source, second]);
    // The job runs to the end before the slot is closed under it.
    let outcome = run(&job, &control, &mut |_| {});
    // D1988: closing the slot cancels the job, which stays until it answers.
    rig.state.close_slot(a);
    assert!(control.is_cancelled());
    assert!(rig.state.flow.list_job.as_ref().is_some_and(|j| j.closing));
    assert!(rig.state.list_job().is_none(), "a closing job is not shown");
    // The file is not opened again while the job winds down.
    let mut effects = Vec::new();
    rig.state.start_open_base(a_canonical, &mut effects);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::ListBusy))
    ));
    // Another database takes the free slot (maybe the same number).
    rig.state.start_open_base(c_path.clone(), &mut effects);
    for effect in effects {
        let answer = rig.rt.block_on(crate::perform(effect));
        rig.drive(answer);
    }
    let c = rig.state.bases.find_by_path(&c_path).expect("C is open");
    assert_eq!(rig.state.bases.slot(c).unwrap().total(), 4);
    // The answer arrives: the other slot is not touched.
    rig.answer(request, outcome);
    assert_eq!(rig.state.bases.slot(c).unwrap().total(), 4);
    assert_eq!(rig.state.bases.slot(c).unwrap().filtered(), 4);
    assert_eq!(on_disk(&c_path), 4);
    assert_eq!(on_disk(&a_path), 5, "the games were written");
    // The window shows the report and can be closed.
    assert!(rig.state.flow.list_job.is_none());
    let result = rig
        .state
        .file_import_view()
        .and_then(|v| v.result)
        .expect("the window shows the report");
    assert_eq!(result.report.games_added, 3);
    assert!(rig.state.modal_open());
    rig.drive(FileImportMessage::Close);
    assert!(rig.state.file_import_view().is_none());
}

#[test]
fn a_slot_that_holds_another_database_when_the_answer_arrives_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a_path = make_si4(dir.path(), "a", 2);
    let a = rig.open(&a_path);
    let other = make_si4(dir.path(), "o", 7);
    let b = rig.open(&other);
    rig.drive(BasesMessage::Select(a));
    let source = pgn(dir.path(), "x.pgn", &games(1));
    let (request, job, control) = rig.start(&[source]);
    let outcome = run(&job, &control, &mut |_| {});
    // The slot is given another database behind the job.
    let replacement = rig.state.bases.slot(b).unwrap().source.clone();
    {
        let slot = rig.state.bases.slot_mut(a).unwrap();
        slot.source = replacement;
        slot.filter = mychess_db::Filter::new(7);
    }
    rig.answer(request, outcome);
    let slot = rig.state.bases.slot(a).unwrap();
    assert_eq!(slot.filter.len(), 7);
    assert_eq!(on_disk(&a_path), 3, "the games were written to the file");
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::FileImport(FileImportNotice::Imported {
            count: 1
        }))
    ));
}

#[test]
fn stopping_between_files_keeps_the_first_file_and_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a_path = make_si4(dir.path(), "a", 1);
    let a = rig.open(&a_path);
    let first = pgn(dir.path(), "1.pgn", &games(2));
    let second = pgn(dir.path(), "2.pgn", &games(3));
    let (request, job, control) = rig.start(&[first, second]);
    let outcome = run(&job, &control, &mut |index| {
        if index == 0 {
            control.cancel();
        }
    });
    rig.answer(request, outcome);
    assert_eq!(on_disk(&a_path), 3);
    let slot = rig.state.bases.slot(a).unwrap();
    assert_eq!((slot.total(), slot.filtered()), (3, 3));
    let result = rig.state.file_import_view().and_then(|v| v.result).unwrap();
    assert!(result.report.interrupted);
    assert_eq!(result.report.files_read, 1);
    assert_eq!(result.error, None, "stopping is not an error");
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::FileImport(FileImportNotice::Interrupted {
            count: 2
        }))
    ));
}

#[test]
fn stopping_before_the_first_game_is_stored_is_a_quiet_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a_path = make_si4(dir.path(), "a", 2);
    rig.open(&a_path);
    let empty = pgn(dir.path(), "empty.pgn", "");
    let second = pgn(dir.path(), "2.pgn", &games(3));
    let (request, job, control) = rig.start(&[empty, second]);
    let outcome = run(&job, &control, &mut |_| control.cancel());
    assert_eq!(outcome.as_ref().err(), Some(&BaseError::Cancelled));
    rig.answer(request, outcome);
    assert_eq!(on_disk(&a_path), 2);
    assert!(rig.state.file_import_view().is_none());
}

#[test]
fn stopping_before_the_first_game_of_a_new_base_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let empty = pgn(dir.path(), "empty.pgn", "");
    let second = pgn(dir.path(), "2.pgn", &games(3));
    // The Clipbase is current: the name of a new database is asked for.
    let effects = update(&mut rig.state, FileImportMessage::Open.into());
    let spec = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec),
            _ => None,
        })
        .unwrap();
    let effects = update(
        &mut rig.state,
        Message::FilesPicked(spec.request, vec![empty, second]),
    );
    let name = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) if spec.kind == PickKind::NewBase => Some(spec),
            _ => None,
        })
        .unwrap();
    let target = dir.path().join("new.si4");
    let effects = update(
        &mut rig.state,
        Message::FilePicked(name.request, Some(target.clone())),
    );
    let (request, job, control) = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::ImportPgnFiles {
                request,
                job,
                control,
            } => Some((request, *job, control)),
            _ => None,
        })
        .unwrap();
    let outcome = run(&job, &control, &mut |_| control.cancel());
    assert_eq!(outcome.as_ref().err(), Some(&BaseError::Cancelled));
    assert!(!target.exists());
    rig.answer(request, outcome);
    assert!(rig.state.file_import_view().is_none());
}

#[test]
fn a_source_that_is_a_file_of_the_database_is_skipped_with_an_error_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a_path = make_si4(dir.path(), "a", 2);
    rig.open(&a_path);
    let good = pgn(dir.path(), "good.pgn", &games(2));
    // The names file of the database itself, and the same file by a
    // different spelling of the path.
    let own = dir.path().join("a.sn4");
    let own_again = dir.path().join(".").join("a.sg4");
    let (request, job, control) = rig.start(&[own, good, own_again]);
    let outcome = run(&job, &control, &mut |_| {});
    let report = &outcome.as_ref().unwrap().report;
    assert_eq!(report.games_added, 2);
    assert_eq!(
        report.file_errors,
        [
            (0, BaseError::SourceIsTarget),
            (2, BaseError::SourceIsTarget)
        ]
    );
    rig.answer(request, outcome);
    assert_eq!(on_disk(&a_path), 4);
}
