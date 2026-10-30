//! The manual window (spec M9-5, slice c, decisions D4050-D4079).
//!
//! The application keeps where the reader is (`mychess_app::manual`); this
//! module draws it: a table of contents with a search field on the left and
//! the chapter on the right, rendered by the `markdown` widget of the
//! toolkit. The toolkit has no anchors, so each heading is wrapped in a
//! container with an id derived from its slug and the shell scrolls to it
//! (D4057, see `shell/manual_glue.rs`).
//!
//! Like the other windows of this front-end it is a layer of the main window.
//! While it is open the keys of the game behind are blocked
//! (`Shell::modal_layer_open`).
//!
//! - `texts`: the embedded chapters, the fallback of the language and the
//!   index of the headings;
//! - `render`: the parsed chapter, cached once per chapter and language,
//!   and the viewer that gives the headings their ids;
//! - `view`: the layer.

mod render;
mod texts;
mod view;

use mychess_app::AppState;
use mychess_app::manual::ManualMessage;

pub use render::{Rendered, heading_id, link_message, text_size};
pub use texts::{chapter_texts, shared_index};
pub use view::layer;

/// Id of the card of the window (for tests and automation).
pub const MANUAL_WINDOW_ID: &str = "manual-window";
/// Id of the search field.
pub const MANUAL_FIND_ID: &str = "manual-find";
/// Id of the scrollable that holds the chapter.
pub const MANUAL_SCROLL_ID: &str = "manual-scroll";
/// Id of the container that holds the chapter text.
pub const MANUAL_TEXT_ID: &str = "manual-text";

/// The id of the entry of a chapter in the table of contents.
#[must_use]
pub fn chapter_entry_id(chapter: usize) -> String {
    format!("manual-chapter-{chapter}")
}

/// The id of the entry of a result of the search.
#[must_use]
pub fn result_entry_id(index: usize) -> String {
    format!("manual-result-{index}")
}

/// An event of the window that only the front-end handles, or that needs
/// its state to become a message of the application.
#[derive(Debug, Clone)]
pub enum ManualUiMessage {
    /// "Manual" of the Help menu: opens the window (at the place where the
    /// reader was, or at the table of contents).
    Open,
    /// `F1`: opens the window at the chapter of the context.
    OpenContext(mychess_app::manual::HelpContext),
    /// A move of the reader (a link, a chapter, Back...): the scroll
    /// position is remembered first.
    Nav(ManualMessage),
    /// A message that does not leave the place (the search, the size of
    /// the text, the questions, closing).
    Plain(ManualMessage),
    /// The chapter was scrolled: how far, and the height of the view.
    Scrolled {
        /// Pixels from the top.
        offset: f32,
        /// The height of the visible part.
        height: f32,
    },
    /// The chapter on the screen is the one the reader moved to: scroll to
    /// the place.
    ScrollNow,
    /// The distance from the top of the text to the heading, if found.
    AnchorFound(Option<f32>),
    /// Ctrl/Cmd+F.
    FocusFind,
    /// Whether the search field has the focus (asked after each key).
    SearchFocus(bool),
    /// Nothing.
    Noop,
}

/// What the screen keeps about the manual.
#[derive(Debug, Default)]
pub struct ManualUi {
    /// The chapter on the screen, parsed once.
    pub(crate) rendered: Option<Rendered>,
    /// The last scroll position reported, in pixels from the top.
    pub(crate) scroll: f32,
    /// The height of the visible part of the chapter.
    pub(crate) viewport: f32,
    /// The `nav_serial` of the application already shown.
    pub(crate) served: u64,
    /// Whether the search field has the focus.
    pub(crate) search_focused: bool,
    /// Whether the window was open at the last update.
    pub(crate) was_open: bool,
}

impl ManualUi {
    /// Whether the window covers the main window. Cheap: it is asked many
    /// times per frame.
    #[must_use]
    pub fn layer_open(app: &AppState) -> bool {
        app.manual().is_open()
    }

    /// The chapter on the screen, if any.
    #[must_use]
    pub fn rendered(&self) -> Option<&Rendered> {
        self.rendered.as_ref()
    }

    /// Whether the search field is known to have the focus.
    #[must_use]
    pub fn search_focused(&self) -> bool {
        self.search_focused
    }

    /// Forgets the state of a window that closed.
    pub(crate) fn reset(&mut self) {
        self.rendered = None;
        self.scroll = 0.0;
        self.viewport = 0.0;
        self.search_focused = false;
    }
}
