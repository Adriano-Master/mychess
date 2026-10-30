//! Messages and notices of the analysis window (spec M4-4).

use std::path::PathBuf;

use mychess_engine::LogEntry;

use super::view::Display;
use crate::engines::RunId;
use crate::error::FileError;

/// Which lines "Add all lines" adds (`addAllVariations`, `analysis.tcl`
/// 1841-1922).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AllLinesMode {
    /// Every line, in order (the left click).
    #[default]
    All,
    /// Only the second line (`multipv[1]`; needs two lines).
    SecondOnly,
    /// Every line reduced to its first move (`rightclick == 2`).
    FirstMovesOnly,
}

/// An event of the analysis window: the buttons of a section and the
/// answers to its effects. `RunId` names the execution (the section).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AnalysisMessage {
    /// Turns the analysis of the execution on or off.
    Toggle(RunId),
    /// "Add move": plays the first move of line 1 in the game.
    AddMove(RunId),
    /// "Add score": puts `depth:score` in front of the comment of the
    /// current move.
    AddScore(RunId),
    /// "Add variation": adds the first line (the same as
    /// `AddLineNumber(run, 1)`).
    AddLine(RunId),
    /// Adds line `n` (1-based, as numbered by the window: the right click on
    /// a line, "Add this line"; the right click on the "Add variation"
    /// button is `n == 2`).
    AddLineNumber(RunId, u8),
    /// "Add all variations".
    AddAllLines(RunId, AllLinesMode),
    /// Sets the number of lines (the field and the keys `1` to `5`).
    SetMultiPv(RunId, u8),
    /// Locks the analysis on the current position, or unlocks it.
    ToggleLock(RunId),
    /// Opens the dialog of the excluded moves.
    OpenExclude(RunId),
    /// Closes it without changing anything.
    CloseExclude(RunId),
    /// Excludes the moves of the text (SAN, spaces or commas); empty text
    /// clears the exclusion.
    SetExclusion(RunId, String),
    /// Switches the low CPU priority and restarts the engine (D446).
    ToggleLowPriority(RunId),
    /// Shows or hides the extra information line.
    ToggleInfo(RunId),
    /// Sets how the lines are shown.
    SetDisplay(RunId, Display),
    /// Goes to the next display mode (the middle click).
    CycleDisplay(RunId),
    /// Asks an XBoard engine for its statistics (the `.` command).
    RequestStats(RunId),
    /// Opens the log of the engine.
    OpenLog(RunId),
    /// Closes it (and forgets what it held).
    CloseLog(RunId),
    /// Fetches the entries written since the last read (the front-end sends
    /// it every 250 ms while the log is open and "Auto" is on; "Update"
    /// sends it once).
    LogTick(RunId),
    /// Sets the text the log is filtered with.
    SetLogFilter(RunId, String),
    /// Writes the whole log to a file.
    SaveLog(RunId, PathBuf),
    /// Ends the execution (the section's close button).
    Close(RunId),

    // ---- answers to effects ----
    /// The entries of the log of an execution.
    LogRead {
        /// The execution.
        run: RunId,
        /// The first entry that was asked for.
        since: u64,
        /// The entries from `since` on.
        entries: Vec<LogEntry>,
        /// Where the next read starts.
        next: u64,
    },
    /// The log was written (or not).
    LogSaved {
        /// The execution.
        run: RunId,
        /// The outcome.
        result: Result<(), FileError>,
    },
}

/// A remark of the analysis window, as data (the front-end translates it
/// with the key of [`AnalysisNotice::message_key`]).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AnalysisNotice {
    /// The position is the end of the main line: "Add variation" and "Add
    /// all" are refused (D442); "Add move" and "Add score" still work.
    AtGameEnd,
    /// The line had a move that is not playable; the moves before it were
    /// added (how many).
    LineTruncated(usize),
    /// A token of the exclusion text is not a legal move.
    ExcludeInvalid(String),
    /// The exclusion would leave no move to search.
    ExcludeAll,
    /// The engine cannot restrict the moves it searches (XBoard).
    ExcludeUnsupported,
    /// The engine has no `MultiPV` option.
    MultiPvUnsupported,
    /// The action is not available while the analysis is locked (D444).
    Locked,
    /// The log was written to a file.
    LogSaved,
}

impl AnalysisNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            AnalysisNotice::AtGameEnd => "analysis-notice-at-game-end",
            AnalysisNotice::LineTruncated(_) => "analysis-notice-line-truncated",
            AnalysisNotice::ExcludeInvalid(_) => "analysis-notice-exclude-invalid",
            AnalysisNotice::ExcludeAll => "analysis-notice-exclude-all",
            AnalysisNotice::ExcludeUnsupported => "analysis-notice-exclude-unsupported",
            AnalysisNotice::MultiPvUnsupported => "analysis-notice-multipv-unsupported",
            AnalysisNotice::Locked => "analysis-notice-locked",
            AnalysisNotice::LogSaved => "analysis-notice-log-saved",
        }
    }
}
