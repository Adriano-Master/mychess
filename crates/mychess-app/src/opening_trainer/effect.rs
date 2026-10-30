//! Running the effects of the opening trainer (spec M8-5, D2235). Every
//! blocking piece goes to `spawn_blocking`.

use super::build::{BuildError, build};
use super::message::{OpeningEffect, OpeningMessage};
use super::store::{load_file, save_file};
use crate::message::Message;

/// Executes one effect and returns the message that reports its result.
pub(crate) async fn perform(effect: OpeningEffect) -> Message {
    let answer = |message| Message::OpeningTrainer(message);
    match effect {
        OpeningEffect::LoadStore { path } => {
            let loaded = tokio::task::spawn_blocking(move || load_file(&path))
                .await
                // A failed read protects the file instead of replacing it.
                .unwrap_or_else(|_| super::store::LoadedStore {
                    unreadable: true,
                    read_only: true,
                    ..super::store::LoadedStore::default()
                });
            answer(OpeningMessage::StoreLoaded(Box::new(loaded)))
        }
        OpeningEffect::SaveStore { path, store } => {
            let result = tokio::task::spawn_blocking(move || save_file(&path, &store))
                .await
                .unwrap_or_else(|_| {
                    Err(crate::error::FileError::new(
                        crate::error::FileErrorKind::Io(std::io::ErrorKind::Other),
                        None,
                    ))
                });
            answer(OpeningMessage::StoreSaved(result))
        }
        OpeningEffect::Build { request, job } => {
            // A panic inside the build becomes a cancelled build: the
            // watchdog (D2231) covers the rest.
            let result = tokio::task::spawn_blocking(move || build(&job))
                .await
                .unwrap_or(Err(BuildError::Failed));
            if let Err(error) = &result {
                tracing::warn!(%error, "the opening repertoire could not be built");
            }
            answer(OpeningMessage::Built {
                request,
                result: result.map(Box::new),
            })
        }
    }
}
