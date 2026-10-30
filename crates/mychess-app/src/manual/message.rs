//! The messages of the manual window.

use super::SharedIndex;

/// An event of the manual window (spec M9-5, section 5.2).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ManualMessage {
    /// Opens the window (or brings it up to date) at a chapter and, maybe, a
    /// heading. The front-end gives the index of the language it shows.
    Open {
        /// The titles and headings of the manual in use.
        index: SharedIndex,
        /// The chapter to show.
        chapter: usize,
        /// The slug of the heading to scroll to.
        anchor: Option<String>,
    },
    /// The language of the interface changed: the same chapter and heading
    /// are shown from the new index.
    Reindex(SharedIndex),
    /// Closes the window.
    Close,
    /// A link of the text was clicked.
    Link(String),
    /// A chapter (or a heading of it) was chosen in the table of contents.
    Goto {
        /// The chapter.
        chapter: usize,
        /// The slug of the heading, if any.
        anchor: Option<String>,
    },
    /// The reader leaves the place: the front-end reports how far the text
    /// was scrolled, so Back can return to the same spot.
    RememberScroll(f32),
    /// The "Back" button.
    Back,
    /// The "Forward" button.
    Forward,
    /// The "Home" button: the first chapter.
    Home,
    /// The text of the search field changed.
    Query(String),
    /// A result of the search was chosen.
    PickResult(usize),
    /// The size of the text changed by this many steps.
    Scale(i8),
    /// The answer of "open this address in the browser?".
    ConfirmExternal(bool),
    /// The browser was asked (`false`: the system refused the address).
    UrlOpened(bool),
}

/// A remark of the bar of the manual; the front-end translates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManualNotice {
    /// The link points to a file or heading that does not exist.
    LinkNotFound,
    /// The link uses a scheme the manual does not follow.
    LinkIgnored,
    /// The system refused to open the address.
    UrlRefused,
}

impl ManualNotice {
    /// The Fluent key of the text.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            ManualNotice::LinkNotFound => "manual-link-not-found",
            ManualNotice::LinkIgnored => "manual-link-ignored",
            ManualNotice::UrlRefused => "manual-url-refused",
        }
    }
}
