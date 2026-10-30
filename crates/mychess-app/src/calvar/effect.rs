//! Running the effects of the calculation exercise (spec M8-6, D2332). The
//! files go to `spawn_blocking`; the engine jobs are async and end by
//! themselves (budget and cancel flag, D2310).

use super::engine::{analyze_lines, analyze_root};
use super::message::{CalvarEffect, CalvarMessage};
use super::store::{LoadedStore, load_file, save_file};
use crate::bases::BaseError;
use crate::message::Message;
use crate::shared::SharedGame;

/// Executes one effect and returns the message that reports its result.
pub(crate) async fn perform(effect: CalvarEffect) -> Message {
    let answer = Message::Calvar;
    match effect {
        CalvarEffect::LoadStore { path } => {
            let loaded = tokio::task::spawn_blocking(move || load_file(&path))
                .await
                // A read that could not even run leaves the file alone: the
                // defaults must never be written over it (D2354).
                .unwrap_or_else(|_| LoadedStore {
                    unreadable: true,
                    read_only: true,
                    ..LoadedStore::default()
                });
            answer(CalvarMessage::StoreLoaded(Box::new(loaded)))
        }
        CalvarEffect::SaveStore { path, store } => {
            let result = tokio::task::spawn_blocking(move || save_file(&path, &store))
                .await
                .unwrap_or_else(|_| {
                    Err(crate::error::FileError::new(
                        crate::error::FileErrorKind::Io(std::io::ErrorKind::Other),
                        None,
                    ))
                });
            answer(CalvarMessage::StoreSaved(result))
        }
        CalvarEffect::ReadGame {
            request,
            number,
            source,
        } => {
            let result = tokio::task::spawn_blocking(move || {
                mychess_db::GameSource::game(&mut *source.lock(), number)
                    .map(SharedGame::new)
                    .map_err(|error| {
                        tracing::warn!(game = number, %error, "cannot read a game for the calculation");
                        BaseError::GameUnreadable { number }
                    })
            })
            .await
            .unwrap_or(Err(BaseError::GameUnreadable { number }));
            answer(CalvarMessage::GameRead { request, result })
        }
        CalvarEffect::AnalyzeRoot { request, job } => {
            let result = analyze_root(&job).await;
            answer(CalvarMessage::RootAnalyzed { request, result })
        }
        CalvarEffect::AnalyzeLines { request, job } => {
            let result = analyze_lines(&job).await;
            answer(CalvarMessage::LinesAnalyzed { request, result })
        }
    }
}
