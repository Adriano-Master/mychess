//! The blocking work of the player report: the selection of the games, the
//! report, the documents and the files. `perform` runs it on
//! `spawn_blocking`; `update` never calls it (spec M7-3, M7-3/D3669).

use std::io::Write as _;
use std::path::{Path, PathBuf};

use mychess_core::Position;
use mychess_db::PlayerStats;
use mychess_db::report::{
    PlayerLabels, PlayerReportRequest, ReportError, ReportStart, player_report, render_player_html,
    render_player_latex, render_player_text,
};

use super::message::{
    PlayerComputeJob, PlayerFailure, PlayerOutcome, PlayerRenderJob, PlayerReportEffect,
    PlayerReportMessage, PlayerSaveJob, SharedPlayerReport,
};
use crate::error::FileError;
use crate::fsutil::write_atomic;
use crate::message::Message;
use crate::opening_report::{ReportFormat, SaveFailure, SharedTemp};
use crate::tree::LockingSource;

/// The name of the file "View HTML" writes.
pub(crate) const VIEW_FILE_NAME: &str = "TempPlayerReport.html";

fn answer(message: PlayerReportMessage) -> Message {
    Message::PlayerReport(message)
}

/// Selects the games and makes the report (the body of
/// [`PlayerReportEffect::Compute`]).
pub(crate) fn run_compute(job: &PlayerComputeJob) -> PlayerOutcome {
    // The position of the board; the report walks the line from the
    // standard start only.
    let position: Option<Position> = match &job.reference {
        None => None,
        Some(reference) => {
            if reference.start.is_some() {
                return PlayerOutcome::Failed(PlayerFailure::BadLine);
            }
            let mut position = Position::new();
            for &mv in &reference.stem {
                match position.after(mv) {
                    Ok(next) => position = next,
                    Err(_) => return PlayerOutcome::Failed(PlayerFailure::BadLine),
                }
            }
            Some(position)
        }
    };
    let stem: &[mychess_core::Move] = job.reference.as_ref().map_or(&[], |r| &r.stem);
    let start = match &position {
        Some(position) => ReportStart::Position(position, stem),
        None => ReportStart::Beginning,
    };
    let mut db = LockingSource::new(job.source.clone());
    let own_stats;
    let player_stats: &PlayerStats = if let Some(shared) = &job.player_stats {
        shared
    } else {
        own_stats = PlayerStats::from_database(&db);
        &own_stats
    };
    let today = job.today.unwrap_or_else(crate::effect::today_utc);
    let stats_config = job.stats.config(today.year().unwrap_or(0));
    let request = PlayerReportRequest {
        name: &job.name,
        color: job.color.color(),
        matching: job.matching.engine(),
        start,
        options: &job.options,
        eco_book: job.eco.as_deref(),
        player_stats,
        stats_config: Some(&stats_config),
        today,
    };
    let mut progress = job.control.reporter();
    match player_report(&mut db, &request, &mut progress, &job.control.token()) {
        Ok(report) => PlayerOutcome::Ready {
            report: SharedPlayerReport::new(report),
            generated: today,
        },
        Err(ReportError::InvalidName) => PlayerOutcome::Failed(PlayerFailure::InvalidName),
        Err(ReportError::InvalidStem { .. } | ReportError::StemMismatch) => {
            PlayerOutcome::Failed(PlayerFailure::BadLine)
        }
        Err(ReportError::UnsupportedPosition) => {
            PlayerOutcome::Failed(PlayerFailure::UnsupportedPosition)
        }
        Err(ReportError::Search(error)) => {
            tracing::warn!(%error, "the database could not be searched for the player report");
            PlayerOutcome::Failed(PlayerFailure::Read)
        }
        Err(error) => {
            tracing::warn!(%error, "the player report failed");
            PlayerOutcome::Failed(PlayerFailure::Internal)
        }
    }
}

/// The words of the document with what only the report knows.
fn prepared_labels(job: &PlayerRenderJob) -> PlayerLabels {
    let mut labels = (*job.labels).clone();
    labels.base.database_name.clone_from(&job.database_name);
    labels.base.date = job.generated.to_string();
    labels.base.generator = format!("MyChess {}", env!("CARGO_PKG_VERSION"));
    labels
}

/// The document of a report in UTF-8.
pub(crate) fn render_document(job: &PlayerRenderJob) -> String {
    let labels = prepared_labels(job);
    let part = job.part.engine();
    match job.format {
        ReportFormat::Text => render_player_text(&job.report, &job.options, &labels, part),
        ReportFormat::Html => render_player_html(&job.report, &job.options, &labels, part),
        ReportFormat::Latex => render_player_latex(&job.report, &job.options, &labels, part),
    }
}

fn write_document(
    path: &Path,
    text: &str,
    protected: &[PathBuf],
    confirmed: bool,
) -> Result<(), SaveFailure> {
    if crate::games_export::is_protected(path, protected) {
        return Err(SaveFailure::ProtectedFile);
    }
    if !confirmed && std::fs::symlink_metadata(path).is_ok() {
        return Err(SaveFailure::NeedsConfirm(path.to_path_buf()));
    }
    write_atomic(path, |file| file.write_all(text.as_bytes()))
        .map_err(|error| SaveFailure::File(FileError::from_io(&error, Some(path.to_path_buf()))))
}

pub(crate) fn run_save(job: &PlayerSaveJob) -> Result<PathBuf, SaveFailure> {
    let text = render_document(&job.render);
    write_document(&job.path, &text, &job.protected, job.confirmed)?;
    Ok(job.path.clone())
}

fn write_html(job: &PlayerRenderJob, dir: Option<SharedTemp>) -> Option<(PathBuf, SharedTemp)> {
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

/// Runs an effect of the player report. Never panics: a task that dies is a
/// failure of the request.
pub(crate) async fn perform(effect: PlayerReportEffect) -> Message {
    match effect {
        PlayerReportEffect::Compute(job) => {
            let request = job.request;
            let outcome = tokio::task::spawn_blocking(move || run_compute(&job))
                .await
                .unwrap_or(PlayerOutcome::Failed(PlayerFailure::Internal));
            answer(PlayerReportMessage::Computed {
                request,
                outcome: Box::new(outcome),
            })
        }
        PlayerReportEffect::Save(job) => {
            let request = job.request;
            let result = tokio::task::spawn_blocking(move || run_save(&job))
                .await
                .unwrap_or(Err(SaveFailure::Internal));
            answer(PlayerReportMessage::Saved { request, result })
        }
        PlayerReportEffect::WriteHtml { request, html, dir } => {
            let result = tokio::task::spawn_blocking(move || write_html(&html, dir))
                .await
                .unwrap_or(None);
            answer(PlayerReportMessage::HtmlWritten { request, result })
        }
        PlayerReportEffect::OpenFile { path } => {
            let ok = tokio::task::spawn_blocking(move || crate::online::open_path(&path))
                .await
                .unwrap_or(false);
            answer(PlayerReportMessage::FileOpened { ok })
        }
    }
}
