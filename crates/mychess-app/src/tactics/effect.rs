//! Running the effects of the tactics training (spec M8-4, D1936). Every
//! blocking piece goes to `spawn_blocking`.

use super::judge::judge;
use super::message::{TacticsEffect, TacticsMessage};
use super::source::{SourceError, find_next, open_file};
use super::store::{load_file, save_file};
use crate::message::Message;

/// Executes one effect and returns the message that reports its result.
pub(crate) async fn perform(effect: TacticsEffect) -> Message {
    let answer = |message| Message::Tactics(message);
    match effect {
        TacticsEffect::LoadStore { path } => {
            let loaded = tokio::task::spawn_blocking(move || load_file(&path))
                .await
                .unwrap_or_default();
            answer(TacticsMessage::StoreLoaded(Box::new(loaded)))
        }
        TacticsEffect::SaveStore { path, store } => {
            let result = tokio::task::spawn_blocking(move || save_file(&path, &store))
                .await
                .unwrap_or_else(|_| {
                    Err(crate::error::FileError::new(
                        crate::error::FileErrorKind::Io(std::io::ErrorKind::Other),
                        None,
                    ))
                });
            answer(TacticsMessage::StoreSaved(result))
        }
        TacticsEffect::OpenFile { request, path } => {
            let failed = path.clone();
            let result = tokio::task::spawn_blocking(move || open_file(&path))
                .await
                .unwrap_or_else(|_| {
                    Err(SourceError::File(crate::error::FileError::new(
                        crate::error::FileErrorKind::Io(std::io::ErrorKind::Other),
                        Some(failed),
                    )))
                });
            answer(TacticsMessage::Opened {
                request,
                result: result.map(Box::new),
            })
        }
        TacticsEffect::Find {
            request,
            source,
            query,
        } => {
            // A panic inside the solver becomes "nothing found": the search
            // goes on with the next request and the watchdog (D1934) covers
            // the rest.
            let outcome = tokio::task::spawn_blocking(move || find_next(&source, &query))
                .await
                .unwrap_or_default();
            answer(TacticsMessage::Found {
                request,
                outcome: Box::new(outcome),
            })
        }
        TacticsEffect::Opinion { request, job } => {
            let verdict = super::opinion::ask(&job).await;
            answer(TacticsMessage::Opinion {
                request,
                verdict: Box::new(verdict),
            })
        }
        TacticsEffect::Judge { request, job } => {
            let verdict = tokio::task::spawn_blocking(move || judge(&job))
                .await
                .unwrap_or(super::judge::Verdict::Inconclusive);
            answer(TacticsMessage::Judged {
                request,
                verdict: Box::new(verdict),
            })
        }
    }
}
