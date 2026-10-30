//! The work of the mask effects, run by [`perform`](crate::perform) on
//! blocking threads (spec M5-4, D1862).

use mychess_db::tree::mask::{outline, search, stm};

use super::message::{
    MaskEffect, MaskFillAnswer, MaskFillIssue, MaskFillJob, MaskIssue, MaskLoaded, MaskMessage,
    SharedMask,
};
use crate::message::Message;
use crate::tree::{LockingSource, TreeMessage};

fn wrap(message: MaskMessage) -> Message {
    Message::Tree(TreeMessage::Mask(message))
}

/// Runs the effect and returns the message that reports its result.
pub(crate) async fn perform_mask(effect: MaskEffect) -> Message {
    match effect {
        MaskEffect::Read { request, path } => {
            let result = tokio::task::spawn_blocking(move || read_file(&path))
                .await
                .unwrap_or(Err(MaskIssue::Internal));
            wrap(MaskMessage::Opened {
                request,
                result: Box::new(result),
            })
        }
        MaskEffect::Write {
            request,
            path,
            mask,
        } => {
            let result = tokio::task::spawn_blocking(move || write_file(&path, &mask))
                .await
                .unwrap_or(Err(MaskIssue::Internal));
            wrap(MaskMessage::Saved { request, result })
        }
        MaskEffect::Fill { job } => {
            let id = job.id;
            let answer = tokio::task::spawn_blocking(move || run_fill(*job))
                .await
                .unwrap_or(MaskFillAnswer {
                    id,
                    // Not used: the state keeps its own mask on a failure.
                    mask: SharedMask::new(mychess_db::tree::mask::TreeMask::new()),
                    result: Err(MaskFillIssue::Internal),
                });
            wrap(MaskMessage::FillFinished(Box::new(answer)))
        }
        MaskEffect::Search {
            request,
            mask,
            query,
        } => {
            let hits = tokio::task::spawn_blocking(move || search::run(mask.mask(), &query))
                .await
                .unwrap_or_default();
            wrap(MaskMessage::Searched {
                request,
                hits: Box::new(hits),
            })
        }
        MaskEffect::Outline {
            request,
            mask,
            position,
            budget,
        } => {
            let outline =
                tokio::task::spawn_blocking(move || outline::build(mask.mask(), &position, budget))
                    .await
                    .unwrap_or_default();
            wrap(MaskMessage::Outlined {
                request,
                outline: Box::new(outline),
            })
        }
        MaskEffect::Deadline { delay, generation } => {
            tokio::time::sleep(delay).await;
            wrap(MaskMessage::Deadline(generation))
        }
    }
}

fn read_file(path: &std::path::Path) -> Result<MaskLoaded, MaskIssue> {
    match stm::read(path) {
        Ok((mask, report)) => Ok(MaskLoaded {
            mask: SharedMask::new(mask),
            report,
        }),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "the mask file was not read");
            Err(MaskIssue::from(&error))
        }
    }
}

fn write_file(path: &std::path::Path, mask: &SharedMask) -> Result<u64, MaskIssue> {
    match stm::write_report(path, mask.mask()) {
        Ok(written) => {
            if !written.permissions_kept {
                tracing::warn!(path = %path.display(), "the permissions of the mask file were not kept");
            }
            Ok(written.size)
        }
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "the mask file was not written");
            Err(MaskIssue::from(&error))
        }
    }
}

/// Runs a fill (D1826). The mask is copied here if the state still holds
/// it, so the state never waits for the job (D1863).
fn run_fill(job: MaskFillJob) -> MaskFillAnswer {
    let MaskFillJob {
        id,
        mask,
        source,
        filter,
        side,
        control,
    } = job;
    let mut work = std::sync::Arc::try_unwrap(mask.0).unwrap_or_else(|shared| (*shared).clone());
    let mut locking = LockingSource::new(source);
    let mut progress = control.reporter();
    let result = mychess_db::tree::mask::feed_filter(
        &mut work,
        &mut locking,
        &filter,
        side,
        &control.token(),
        &mut progress,
    )
    .map_err(|error| {
        tracing::warn!(%error, "the fill of the mask failed");
        MaskFillIssue::FilterSize
    });
    MaskFillAnswer {
        id,
        mask: SharedMask::new(work),
        result,
    }
}
