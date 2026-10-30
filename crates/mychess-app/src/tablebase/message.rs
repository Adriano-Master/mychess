//! Messages and effects of the tablebase panel and of the training (spec
//! M8-8, D2563, D2567, D2583).

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mychess_core::Position;
use mychess_tablebase::{
    CachedOnline, LichessTablebase, LocalTablebase, OnlineProbe, OpenReport, PositionProbe,
    PositionReport, Source, TablebaseError,
};

use crate::effect::Effect;
use crate::error::RequestId;
use crate::message::Message;

/// The local tables, opened. Cheap to clone and compared by identity.
#[derive(Clone)]
pub struct LocalHandle(Arc<dyn PositionProbe>);

impl LocalHandle {
    /// Wraps a local source (the real files, or a stub in tests).
    #[must_use]
    pub fn new(probe: Arc<dyn PositionProbe>) -> LocalHandle {
        LocalHandle(probe)
    }

    /// The most pieces the tables cover.
    #[must_use]
    pub fn max_pieces(&self) -> u8 {
        self.0.max_pieces()
    }
}

impl std::fmt::Debug for LocalHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalHandle").field(&self.0).finish()
    }
}

impl PartialEq for LocalHandle {
    fn eq(&self, other: &LocalHandle) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug)]
struct OnlineCell {
    /// Set once the client was built; a failure to build it is not kept, so
    /// the next query tries again.
    probe: OnceLock<Arc<dyn OnlineProbe>>,
}

/// The online source. The client is built the first time it is needed (not in
/// `update`, D2596) and kept, with its cache of answers. Compared by identity.
#[derive(Debug, Clone)]
pub struct OnlineHandle(Arc<OnlineCell>);

impl OnlineHandle {
    /// A handle that builds the real Lichess client on first use.
    #[must_use]
    pub fn new() -> OnlineHandle {
        OnlineHandle(Arc::new(OnlineCell {
            probe: OnceLock::new(),
        }))
    }

    /// A handle that uses `probe` (a fake server's client, or a stub).
    #[must_use]
    pub fn from_probe(probe: Arc<dyn OnlineProbe>) -> OnlineHandle {
        let cell = OnlineCell {
            probe: OnceLock::new(),
        };
        drop(cell.probe.set(probe));
        OnlineHandle(Arc::new(cell))
    }

    async fn resolve(&self) -> Option<Arc<dyn OnlineProbe>> {
        if let Some(ready) = self.0.probe.get() {
            return Some(Arc::clone(ready));
        }
        let cell = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || {
            let built = LichessTablebase::new()
                .ok()
                .map(|tb| Arc::new(CachedOnline::new(tb)) as Arc<dyn OnlineProbe>)?;
            Some(Arc::clone(cell.probe.get_or_init(|| built)))
        })
        .await
        .unwrap_or(None)
    }
}

impl Default for OnlineHandle {
    fn default() -> OnlineHandle {
        OnlineHandle::new()
    }
}

impl PartialEq for OnlineHandle {
    fn eq(&self, other: &OnlineHandle) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The result of opening the folders.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedSet {
    /// The tables.
    pub handle: LocalHandle,
    /// What was found in each folder.
    pub report: OpenReport,
}

/// Why a delay was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DelayKind {
    /// The online query waits for the position to stay still (D2567).
    Debounce,
    /// The query is abandoned when it takes too long (D2563).
    Watchdog,
    /// The training waits before it plays its move (D2574).
    Reply,
    /// The reading of the folders is abandoned when it takes too long; the
    /// request is the one of the open.
    OpenWatchdog,
}

/// An event of the tablebase panel or of the training.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TablebaseMessage {
    /// Opens the panel, or closes it when it is open.
    Toggle,
    /// Opens or closes the panel.
    SetOpen(bool),
    /// Turns the local tables on or off.
    SetLocal(bool),
    /// Turns the online tablebase on or off.
    SetOnline(bool),
    /// Asks the front-end for a folder to add.
    PickDirectory,
    /// Adds a folder of tables.
    AddDirectory(PathBuf),
    /// Removes the folder at that place of the list.
    RemoveDirectory(usize),
    /// Reads the folders again.
    Reopen,
    /// Plays the move of that row of the list.
    PlayRow(usize),
    /// Starts the training from the position on the board.
    TrainStart,
    /// Ends the training.
    TrainStop,
    /// Hides the result of the last training.
    DismissOutcome,
    /// Replaces the game by a random position of that material and starts the
    /// training. `seed` comes from the clock of the front-end.
    NewPosition {
        /// The material, for instance `KRvKP`.
        material: String,
        /// Makes the position reproducible.
        seed: u64,
    },
    /// The answer of [`TablebaseEffect::Open`].
    Opened {
        /// Correlates the answer.
        request: RequestId,
        /// The tables, or why they could not be opened.
        result: Box<Result<OpenedSet, TablebaseError>>,
    },
    /// The answer of a probe.
    Probed {
        /// Correlates the answer.
        request: RequestId,
        /// The source that answered.
        source: Source,
        /// The report, or why there is none.
        result: Box<Result<PositionReport, TablebaseError>>,
    },
    /// A delay asked for ended.
    DelayElapsed {
        /// Correlates the answer.
        request: RequestId,
        /// Why it was asked for.
        kind: DelayKind,
    },
}

/// What `perform` does for the tablebase.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TablebaseEffect {
    /// Reads the folders (`spawn_blocking`); answers with
    /// [`TablebaseMessage::Opened`].
    Open {
        /// Correlates the answer.
        request: RequestId,
        /// The folders.
        dirs: Vec<PathBuf>,
    },
    /// Probes the local tables (`spawn_blocking`).
    ProbeLocal {
        /// Correlates the answer.
        request: RequestId,
        /// The tables.
        handle: LocalHandle,
        /// The position.
        position: Position,
    },
    /// Asks the online tablebase.
    ProbeOnline {
        /// Correlates the answer.
        request: RequestId,
        /// The client.
        handle: OnlineHandle,
        /// The position.
        position: Position,
    },
    /// Waits, then answers with [`TablebaseMessage::DelayElapsed`].
    Delay {
        /// Correlates the answer.
        request: RequestId,
        /// Why.
        kind: DelayKind,
        /// How long, in milliseconds.
        ms: u32,
    },
}

impl TablebaseEffect {
    /// Wraps the effect for the top-level enum.
    pub(crate) fn into_effect(self) -> Effect {
        Effect::Tablebase(self)
    }
}

fn answer(message: TablebaseMessage) -> Message {
    Message::Tablebase(message)
}

/// Runs an effect of the tablebase. Never panics: a task that dies is an
/// [`TablebaseError::Internal`] answer.
pub(crate) async fn perform(effect: TablebaseEffect) -> Message {
    match effect {
        TablebaseEffect::Open { request, dirs } => {
            let result = tokio::task::spawn_blocking(move || {
                LocalTablebase::open(&dirs).map(|(tables, report)| OpenedSet {
                    handle: LocalHandle::new(Arc::new(tables)),
                    report,
                })
            })
            .await
            .unwrap_or(Err(TablebaseError::Internal));
            answer(TablebaseMessage::Opened {
                request,
                result: Box::new(result),
            })
        }
        TablebaseEffect::ProbeLocal {
            request,
            handle,
            position,
        } => {
            let result = tokio::task::spawn_blocking(move || handle.0.probe(&position))
                .await
                .unwrap_or(Err(TablebaseError::Internal));
            answer(TablebaseMessage::Probed {
                request,
                source: Source::Local,
                result: Box::new(result),
            })
        }
        TablebaseEffect::ProbeOnline {
            request,
            handle,
            position,
        } => {
            let result = match handle.resolve().await {
                Some(probe) => probe.probe(&position).await,
                None => Err(TablebaseError::Network("no client".to_owned())),
            };
            answer(TablebaseMessage::Probed {
                request,
                source: Source::Online,
                result: Box::new(result),
            })
        }
        TablebaseEffect::Delay { request, kind, ms } => {
            tokio::time::sleep(Duration::from_millis(u64::from(ms))).await;
            answer(TablebaseMessage::DelayElapsed { request, kind })
        }
    }
}

/// A remark for the status bar (D2576, D2577).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TablebaseNotice {
    /// Something else holds the game (a game against an engine, a tournament,
    /// a trainer, the annotation).
    Busy,
    /// The training starts only at the end of a line.
    NotAtEndOfLine,
    /// The position is not worth training: the game is over, or the tables
    /// do not cover it.
    NothingToTrain,
    /// The text is not a material like `KRvKP`.
    BadMaterial,
    /// The folder cannot be kept in the preferences (not UTF-8).
    FolderNotSaved,
    /// The list of folders is full.
    TooManyFolders,
}

impl TablebaseNotice {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            TablebaseNotice::Busy => "tablebase-notice-busy",
            TablebaseNotice::NotAtEndOfLine => "tablebase-notice-not-at-end",
            TablebaseNotice::NothingToTrain => "tablebase-notice-nothing-to-train",
            TablebaseNotice::BadMaterial => "tablebase-notice-bad-material",
            TablebaseNotice::FolderNotSaved => "tablebase-notice-folder-not-saved",
            TablebaseNotice::TooManyFolders => "tablebase-notice-too-many-folders",
        }
    }
}
