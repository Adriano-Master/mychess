//! The messages, notices and state of the start-up (spec M9-4).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::folders::FolderReport;
use super::launch::LaunchFile;
use super::layout::{LayoutMode, LayoutWarning};
use crate::error::RequestId;

/// What `main` found out before the window opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartInfo {
    /// The `MyChess` folder, `None` when there is no place for it.
    pub root: Option<PathBuf>,
    /// Which rule chose it.
    pub mode: LayoutMode,
    /// What went wrong while choosing.
    pub warnings: Vec<LayoutWarning>,
    /// The files named on the command line, in order.
    pub files: Vec<LaunchFile>,
    /// Do not show the tip of the day this time (`--no-tip`).
    pub skip_tip: bool,
}

/// An event of the start-up.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StartMessage {
    /// The front-end hands over what it found out at launch. What it asks
    /// for happens once the preferences have been read.
    Configure(Box<StartInfo>),
    /// The user folder was prepared ([`StartEffect::PrepareFolder`](super::StartEffect)).
    Prepared {
        /// The request.
        request: RequestId,
        /// What was done.
        report: Box<FolderReport>,
    },
    /// The welcome was closed (button, Enter or Esc): it will not come back,
    /// and the engines found in `Engine/` are added to the list.
    CloseWelcome,
    /// Opens the tip of the day (Help menu).
    ShowTip,
    /// The next tip.
    NextTip,
    /// The previous tip.
    PreviousTip,
    /// Closes the tip.
    CloseTip,
    /// The "show tips at start" box.
    SetShowTip(bool),
}

/// A remark about the start-up, for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartNotice {
    /// The portable folder cannot be written: the normal mode is in use.
    PortableNotWritable(PathBuf),
    /// The portable mode was asked for but the program's folder is unknown.
    PortableUnavailable,
    /// The user's home folder is unknown.
    NoHomeFolder,
    /// The `MyChess` folder (or one of its folders) could not be created.
    FolderProblem(PathBuf),
}

impl StartNotice {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            StartNotice::PortableNotWritable(_) => "start-notice-portable-not-writable",
            StartNotice::PortableUnavailable => "start-notice-portable-unavailable",
            StartNotice::NoHomeFolder => "start-notice-no-home",
            StartNotice::FolderProblem(_) => "start-notice-folder-problem",
        }
    }

    /// The path the remark is about, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            StartNotice::PortableNotWritable(path) | StartNotice::FolderProblem(path) => Some(path),
            StartNotice::PortableUnavailable | StartNotice::NoHomeFolder => None,
        }
    }
}

impl From<&LayoutWarning> for StartNotice {
    fn from(warning: &LayoutWarning) -> StartNotice {
        match warning {
            LayoutWarning::PortableNotWritable(path) => {
                StartNotice::PortableNotWritable(path.clone())
            }
            LayoutWarning::PortableUnavailable => StartNotice::PortableUnavailable,
            LayoutWarning::NoHomeFolder => StartNotice::NoHomeFolder,
        }
    }
}

/// The first-run welcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeState {
    /// The `MyChess` folder.
    pub root: Option<PathBuf>,
    /// Which rule chose it.
    pub mode: LayoutMode,
    /// What the preparation of the folders did; `None` while it runs.
    pub report: Option<FolderReport>,
    pub(crate) request: Option<RequestId>,
}

/// The state of the start-up flow.
#[derive(Debug, Default)]
pub struct StartState {
    pub(crate) pending: Option<StartInfo>,
    pub(crate) welcome: Option<WelcomeState>,
    pub(crate) tip: Option<u32>,
    pub(crate) queue: VecDeque<LaunchFile>,
}

impl StartState {
    /// The welcome, while it is open.
    #[must_use]
    pub fn welcome(&self) -> Option<&WelcomeState> {
        self.welcome.as_ref()
    }

    /// The tip being shown (an index below [`TIP_COUNT`](super::TIP_COUNT)).
    #[must_use]
    pub fn tip(&self) -> Option<u32> {
        self.tip
    }

    /// Whether the welcome or the tip covers the window.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.welcome.is_some() || self.tip.is_some()
    }

    /// How many command-line files still wait their turn.
    #[must_use]
    pub fn queued_files(&self) -> usize {
        self.queue.len()
    }
}
