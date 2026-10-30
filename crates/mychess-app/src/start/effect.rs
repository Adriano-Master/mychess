//! The effect of the start-up: the blocking disk work of the first run.

use std::path::PathBuf;

use super::folders::{FolderReport, prepare_user_folder};
use super::message::StartMessage;
use crate::error::RequestId;
use crate::message::Message;

/// Disk work of the start-up, run by [`perform`](crate::perform).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartEffect {
    /// Creates the `MyChess` folder and looks for engines in it; answers
    /// [`StartMessage::Prepared`].
    PrepareFolder {
        /// The request.
        request: RequestId,
        /// The `MyChess` folder.
        root: PathBuf,
    },
}

/// Runs an effect on a blocking thread.
pub(crate) async fn perform(effect: StartEffect) -> Message {
    match effect {
        StartEffect::PrepareFolder { request, root } => {
            let fallback = root.clone();
            let report = tokio::task::spawn_blocking(move || prepare_user_folder(&root))
                .await
                .unwrap_or_else(|_| FolderReport {
                    root: fallback.clone(),
                    created: Vec::new(),
                    existing: Vec::new(),
                    problems: vec![super::FolderProblem::CreateFailed {
                        path: fallback,
                        kind: std::io::ErrorKind::Other,
                    }],
                    engines: Vec::new(),
                });
            Message::Start(StartMessage::Prepared {
                request,
                report: Box::new(report),
            })
        }
    }
}
