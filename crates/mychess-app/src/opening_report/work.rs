//! The blocking work of the opening report: the search of the games, the
//! report, the documents and the files. `perform` runs it on
//! `spawn_blocking`; `update` never calls it (spec M7-2, D3602, D3616).

use std::io::{Read as _, Write as _};
use std::path::Path;

use mychess_db::report::{
    OpeningInputs, ReportError, ReportLabels, opening_report, parse_reports_dat, render_html,
    render_latex, render_text,
};
use mychess_db::tree::{FastMode, NoCache, TreeError, TreeOptions, TreeQuery, TreeState, search};
use mychess_db::{Filter, GameSource, PlayerStats};

use super::favorites::{load_file, save_file};
use super::message::{
    BatchFailure, BatchJob, ComputeJob, ComputeOutcome, OpeningReportEffect, OpeningReportMessage,
    RenderJob, ReportFailure, ReportsDat, SaveFailure, SaveJob, SharedReport, SharedTemp,
};
use super::prefs::ReportFormat;
use crate::error::{FileError, FileErrorKind};
use crate::fsutil::write_atomic;
use crate::message::Message;
use crate::tree::LockingSource;

/// The name of the file "View HTML" writes (the Scid's `TempOpeningReport`).
pub(crate) const VIEW_FILE_NAME: &str = "TempOpeningReport.html";

/// Largest `reports.dat` that is read.
const MAX_DAT_BYTES: u64 = 1 << 20;

fn answer(message: OpeningReportMessage) -> Message {
    Message::OpeningReport(message)
}

/// Searches the games and makes the report (the body of
/// [`OpeningReportEffect::Compute`]).
pub(crate) fn run_compute(job: &ComputeJob) -> ComputeOutcome {
    let mut position = job.reference.start.clone().unwrap_or_default();
    for &mv in &job.reference.stem {
        match position.after(mv) {
            Ok(next) => position = next,
            Err(_) => return ComputeOutcome::Failed(ReportFailure::BadLine),
        }
    }
    let mut db = LockingSource::new(job.source.clone());
    let len = db.len();
    // The main filter of the search is private to the report: the filter
    // of the slot is never touched (D3601).
    let main_filter = Filter::new(len);
    let mut state = TreeState::default();
    let options = TreeOptions {
        fast: FastMode::Slow,
        eco_book: job.eco.as_deref(),
        ..TreeOptions::default()
    };
    let ply = u32::try_from(job.reference.stem.len()).unwrap_or(u32::MAX);
    let query = TreeQuery {
        position: &position,
        ply,
        main_filter: &main_filter,
    };
    let mut search_progress = job.search.reporter();
    let tree = match search(
        &mut db,
        &query,
        &options,
        &mut state,
        &mut NoCache,
        &job.search.token(),
        &mut search_progress,
    ) {
        Ok(tree) => tree,
        Err(TreeError::Cancelled) => return ComputeOutcome::SearchInterrupted,
        Err(TreeError::UnsupportedPosition) => {
            return ComputeOutcome::Failed(ReportFailure::UnsupportedPosition);
        }
        Err(TreeError::TooManyMoves) => return ComputeOutcome::Failed(ReportFailure::TooManyMoves),
        Err(TreeError::Source(error)) => {
            tracing::warn!(%error, "the database could not be read for the report");
            return ComputeOutcome::Failed(ReportFailure::Read);
        }
        Err(_) => return ComputeOutcome::Failed(ReportFailure::Internal),
    };
    let tree_filter = state.tree_filter().cloned().unwrap_or_else(|| {
        let mut none = Filter::new(len);
        none.fill(0);
        none
    });
    let own_stats;
    let player_stats: &PlayerStats = if let Some(shared) = &job.player_stats {
        shared
    } else {
        own_stats = PlayerStats::from_database(&db);
        &own_stats
    };
    let today = job.today.unwrap_or_else(crate::effect::today_utc);
    let stats_config = job.stats.config(today.year().unwrap_or(0));
    let inputs = OpeningInputs {
        position: &position,
        start: job.reference.start.as_ref(),
        stem: &job.reference.stem,
        tree_filter: &tree_filter,
        tree,
        options: &job.options,
        eco_book: job.eco.as_deref(),
        player_stats,
        stats_config: Some(&stats_config),
        today,
    };
    let mut progress = job.generate.reporter();
    match opening_report(&mut db, inputs, &mut progress, &job.generate.token()) {
        Ok(report) => ComputeOutcome::Ready {
            report: SharedReport::new(report),
            generated: today,
        },
        Err(ReportError::InvalidStem { .. } | ReportError::StemMismatch) => {
            ComputeOutcome::Failed(ReportFailure::BadLine)
        }
        Err(_) => ComputeOutcome::Failed(ReportFailure::Internal),
    }
}

/// The words of the document with what only the report knows.
fn prepared_labels(job: &RenderJob) -> ReportLabels {
    let mut labels = (*job.labels).clone();
    labels.database_name.clone_from(&job.database_name);
    labels.date = job.generated.to_string();
    labels.generator = format!("MyChess {}", env!("CARGO_PKG_VERSION"));
    labels
}

/// The document of a report in UTF-8 (D3617).
pub(crate) fn render_document(job: &RenderJob) -> String {
    let labels = prepared_labels(job);
    let part = job.part.engine();
    match job.format {
        ReportFormat::Text => render_text(&job.report, &job.options, &labels, part),
        ReportFormat::Html => render_html(&job.report, &job.options, &labels, part),
        ReportFormat::Latex => render_latex(&job.report, &job.options, &labels, part),
    }
}

fn write_document(
    path: &Path,
    text: &str,
    protected: &[std::path::PathBuf],
) -> Result<(), SaveFailure> {
    if crate::games_export::is_protected(path, protected) {
        return Err(SaveFailure::ProtectedFile);
    }
    write_atomic(path, |file| file.write_all(text.as_bytes()))
        .map_err(|error| SaveFailure::File(FileError::from_io(&error, Some(path.to_path_buf()))))
}

fn run_save(job: &SaveJob) -> Result<std::path::PathBuf, SaveFailure> {
    let text = render_document(&job.render);
    write_document(&job.path, &text, &job.protected)?;
    Ok(job.path.clone())
}

fn run_batch(job: &BatchJob) -> Result<std::path::PathBuf, BatchFailure> {
    // One control stops both phases.
    let report = match run_compute(&job.compute) {
        ComputeOutcome::Ready { report, generated } => (report, generated),
        ComputeOutcome::SearchInterrupted => return Err(BatchFailure::Stopped),
        ComputeOutcome::Failed(failure) => return Err(BatchFailure::Report(failure)),
    };
    if report.0.core.interrupted {
        return Err(BatchFailure::Stopped);
    }
    let render = RenderJob {
        report: report.0,
        options: job.compute.options.clone(),
        labels: job.labels.clone(),
        part: job.part,
        format: job.format,
        database_name: job.database_name.clone(),
        generated: report.1,
    };
    let text = render_document(&render);
    write_document(&job.path, &text, &job.protected).map_err(BatchFailure::Save)?;
    Ok(job.path.clone())
}

fn read_reports_dat(path: &Path) -> Result<ReportsDat, FileError> {
    let io = |error: &std::io::Error| FileError::from_io(error, Some(path.to_path_buf()));
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| io(&e))?
        .take(MAX_DAT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(&e))?;
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_DAT_BYTES) {
        return Err(FileError::new(
            FileErrorKind::Io(std::io::ErrorKind::InvalidData),
            Some(path.to_path_buf()),
        ));
    }
    // The Scid writes the file in the encoding of the system; anything that
    // is not UTF-8 is read lossily (the names are only labels).
    let text = String::from_utf8_lossy(&bytes);
    let parsed = parse_reports_dat(text.strip_prefix('\u{feff}').unwrap_or(&text));
    Ok(ReportsDat {
        favorites: parsed
            .favorites
            .into_iter()
            .map(|f| (f.name, f.moves))
            .collect(),
        ignored: parsed.ignored,
    })
}

fn write_html(
    job: &RenderJob,
    dir: Option<SharedTemp>,
) -> Option<(std::path::PathBuf, SharedTemp)> {
    let dir = match dir {
        Some(dir) => dir,
        None => SharedTemp::new(
            tempfile::Builder::new()
                .prefix("mychess-report-")
                .tempdir()
                .ok()?,
        ),
    };
    let path = dir.path().join(VIEW_FILE_NAME);
    let text = render_document(job);
    write_atomic(&path, |file| file.write_all(text.as_bytes())).ok()?;
    Some((path, dir))
}

/// Runs an effect of the opening report. Never panics: a task that dies is
/// a failure of the request.
pub(crate) async fn perform(effect: OpeningReportEffect) -> Message {
    match effect {
        OpeningReportEffect::Compute(job) => {
            let request = job.request;
            let outcome = tokio::task::spawn_blocking(move || run_compute(&job))
                .await
                .unwrap_or(ComputeOutcome::Failed(ReportFailure::Internal));
            answer(OpeningReportMessage::Computed {
                request,
                outcome: Box::new(outcome),
            })
        }
        OpeningReportEffect::Save(job) => {
            let request = job.request;
            let result = tokio::task::spawn_blocking(move || run_save(&job))
                .await
                .unwrap_or(Err(SaveFailure::Internal));
            answer(OpeningReportMessage::Saved { request, result })
        }
        OpeningReportEffect::WriteHtml { request, html, dir } => {
            let result = tokio::task::spawn_blocking(move || write_html(&html, dir))
                .await
                .unwrap_or(None);
            answer(OpeningReportMessage::HtmlWritten { request, result })
        }
        OpeningReportEffect::OpenFile { path } => {
            let ok = tokio::task::spawn_blocking(move || crate::online::open_path(&path))
                .await
                .unwrap_or(false);
            answer(OpeningReportMessage::FileOpened { ok })
        }
        OpeningReportEffect::LoadFavorites { request, path } => {
            let result = tokio::task::spawn_blocking(move || load_file(&path))
                .await
                .unwrap_or_else(|_| {
                    Err(FileError::new(
                        FileErrorKind::Io(std::io::ErrorKind::Other),
                        None,
                    ))
                });
            answer(OpeningReportMessage::FavoritesLoaded { request, result })
        }
        OpeningReportEffect::SaveFavorites {
            request,
            path,
            favorites,
            backup,
        } => {
            let ok = tokio::task::spawn_blocking(move || {
                save_file(&path, &favorites, backup)
                    .map_err(|error| tracing::warn!(?error, "could not save the favorite reports"))
                    .is_ok()
            })
            .await
            .unwrap_or(false);
            answer(OpeningReportMessage::FavoritesSaved { request, ok })
        }
        OpeningReportEffect::ReadReportsDat { request, path } => {
            let result = tokio::task::spawn_blocking(move || read_reports_dat(&path))
                .await
                .unwrap_or_else(|_| {
                    Err(FileError::new(
                        FileErrorKind::Io(std::io::ErrorKind::Other),
                        None,
                    ))
                });
            answer(OpeningReportMessage::ReportsDatRead { request, result })
        }
        OpeningReportEffect::BatchItem(job) => {
            let request = job.request;
            let result = tokio::task::spawn_blocking(move || run_batch(&job))
                .await
                .unwrap_or(Err(BatchFailure::Report(ReportFailure::Internal)));
            answer(OpeningReportMessage::BatchItem { request, result })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bases::{BaseSource, Clipbase, OpenControl, SharedSource};
    use crate::error::RequestId;
    use crate::opening_report::Reference;
    use mychess_db::report::ReportOptions;

    fn job(path: std::path::PathBuf, protected: Vec<std::path::PathBuf>) -> SaveJob {
        let clip = Clipbase::new(100).expect("clipbase");
        let compute = ComputeJob {
            request: RequestId(1),
            source: SharedSource::new(BaseSource::Clipbase(Box::new(clip))),
            reference: Reference {
                start: None,
                stem: Vec::new(),
            },
            options: ReportOptions::default(),
            stats: crate::charts::StatsPrefs::default(),
            eco: None,
            player_stats: None,
            today: Some("2026.10.29".parse().expect("date")),
            search: OpenControl::new(),
            generate: OpenControl::new(),
        };
        let ComputeOutcome::Ready { report, generated } = run_compute(&compute) else {
            panic!("a report of an empty database");
        };
        SaveJob {
            request: RequestId(2),
            path,
            render: RenderJob {
                report,
                options: ReportOptions::default(),
                labels: Box::default(),
                part: ReportPart::Full,
                format: ReportFormat::Text,
                database_name: "Clipbase".to_owned(),
                generated,
            },
            protected,
        }
    }

    use super::super::prefs::ReportPart;

    #[test]
    fn an_open_database_is_never_overwritten() {
        let dir = tempfile::tempdir().expect("dir");
        let base = dir.path().join("games.txt");
        std::fs::write(&base, "the database").expect("write");
        let refused = run_save(&job(base.clone(), vec![base.clone()]));
        assert_eq!(refused, Err(SaveFailure::ProtectedFile));
        assert_eq!(
            std::fs::read_to_string(&base).expect("read"),
            "the database"
        );
        // The same name in another case counts as the same file.
        let upper = dir.path().join("GAMES.TXT");
        assert_eq!(
            run_save(&job(upper, vec![base.clone()])),
            Err(SaveFailure::ProtectedFile)
        );
        // Another file is written.
        let other = dir.path().join("other.txt");
        assert_eq!(run_save(&job(other.clone(), vec![base])), Ok(other.clone()));
        assert!(other.exists());
    }

    #[test]
    fn an_empty_database_gives_the_header_and_zero_games() {
        let dir = tempfile::tempdir().expect("dir");
        let out = dir.path().join("empty.txt");
        run_save(&job(out.clone(), Vec::new())).expect("saved");
        let text = std::fs::read_to_string(out).expect("read");
        assert!(text.contains("(0 games)"), "{text}");
        assert!(!text.contains('\0'));
    }
}
