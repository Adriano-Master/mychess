//! Messages, effects and notices of the piece tracker (spec M6-4, slice c,
//! D3352, D3389, D3408).

use mychess_core::Square;
use mychess_db::{Filter, PieceTrackQuery, PieceTrackResult, StatsError, piece_track};

use super::{TrackerMode, TrackerScheme};
use crate::bases::{OpenControl, SharedSource};
use crate::error::RequestId;
use crate::message::Message;
use crate::tree::LockingSource;

/// Milliseconds the panel waits for the filter to stay still before it
/// counts again.
pub const DEBOUNCE_MS: u32 = 400;

/// Why a count gave nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PieceTrackerFailure {
    /// The filter does not cover the database.
    FilterSize,
    /// The count died.
    Internal,
}

/// A remark for the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PieceTrackerNotice {
    /// The count failed.
    Failed,
    /// The square is not the starting square of a piece.
    NotAPiece,
}

impl PieceTrackerNotice {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            PieceTrackerNotice::Failed => "ptracker-notice-failed",
            PieceTrackerNotice::NotAPiece => "ptracker-notice-not-a-piece",
        }
    }
}

/// An event of the piece tracker.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PieceTrackerMessage {
    /// Opens the panel, or closes it when it is open.
    Toggle,
    /// Opens the panel.
    Open,
    /// Closes the panel and stops the count.
    Close,
    /// A click on one of the 32 drawings of the pieces: the piece that
    /// starts on `square` alone, or with its sisters (right button).
    SelectPiece {
        /// The starting square of the piece.
        square: Square,
        /// Also the sister pieces.
        sister: bool,
    },
    /// The first move of the window (0 to 999).
    SetStart(u16),
    /// The last move of the window (0 to 999).
    SetEnd(u16),
    /// Games or time.
    SetMode(TrackerMode),
    /// The colour of the map.
    SetScheme(TrackerScheme),
    /// "Update": counts again at once.
    Refresh,
    /// "Stop": ends the count and keeps what was counted, marked
    /// interrupted (P12).
    Stop,
    /// The answer of [`PieceTrackerEffect::Track`].
    Computed {
        /// Correlates the answer.
        request: RequestId,
        /// The counts (possibly partial, see `interrupted`), or why none.
        result: Box<Result<PieceTrackResult, PieceTrackerFailure>>,
    },
    /// The debounce ended.
    DelayElapsed(RequestId),
}

/// What `perform` does for the piece tracker.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PieceTrackerEffect {
    /// Follows the pieces through the games of the filter on a blocking
    /// thread.
    Track {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// A copy of the filter.
        filter: Box<Filter>,
        /// What to follow.
        query: Box<PieceTrackQuery>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Waits, then answers with [`PieceTrackerMessage::DelayElapsed`].
    Delay {
        /// Correlates the answer.
        request: RequestId,
        /// How long, in milliseconds.
        ms: u32,
    },
}

/// Runs an effect. Never panics: a task that dies is an internal failure.
pub(crate) async fn perform(effect: PieceTrackerEffect) -> Message {
    let message = match effect {
        PieceTrackerEffect::Track {
            request,
            source,
            filter,
            query,
            control,
        } => {
            let result = tokio::task::spawn_blocking(move || {
                let mut db = LockingSource::new(source);
                let mut progress = control.reporter();
                piece_track(&mut db, &filter, &query, &mut progress, &control.token()).map_err(
                    |e| match e {
                        StatsError::FilterSize { .. } => PieceTrackerFailure::FilterSize,
                        _ => PieceTrackerFailure::Internal,
                    },
                )
            })
            .await
            .unwrap_or(Err(PieceTrackerFailure::Internal));
            PieceTrackerMessage::Computed {
                request,
                result: Box::new(result),
            }
        }
        PieceTrackerEffect::Delay { request, ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
            PieceTrackerMessage::DelayElapsed(request)
        }
    };
    Message::PieceTracker(message)
}
