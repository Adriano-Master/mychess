//! The built-in manual (spec `docs/specs/m9-5-manual-integrado.md`, slices a
//! and b, decisions D4050-D4079): the part of the viewer that is not
//! drawing.
//!
//! The text of the chapters is data owned by the front-end (it embeds the
//! files of `assets/manual/<language>/`); this crate knows their names
//! ([`CHAPTER_FILES`]), reads their titles and headings into a
//! [`ManualIndex`] and keeps where the reader is. Nothing here reads a file:
//! the flow (`state/manual_flow.rs`) only moves the position and the history.
//!
//! - `index`: [`ManualIndex`], the titles, the headings and the search;
//! - `link`: [`slug`], [`resolve_link`] and [`help_target`];
//! - `message`: [`ManualMessage`] and [`ManualNotice`];
//! - `prefs`: [`ManualPrefs`] (`[manual]`).

mod index;
mod link;
mod message;
mod prefs;

pub use index::{Heading, ManualIndex, SearchHit, SharedIndex};
pub use link::{HelpContext, LinkTarget, help_target, resolve_link, slug};
pub use message::{ManualMessage, ManualNotice};
pub use prefs::{DEFAULT_TEXT_SCALE, MAX_TEXT_SCALE, ManualPrefs};

/// The files of the chapters, in reading order. The name is the same in
/// every language.
pub const CHAPTER_FILES: [&str; 15] = [
    "00-indice.md",
    "01-primeiros-passos.md",
    "02-tabuleiro-e-partida.md",
    "03-bancos-de-dados.md",
    "04-buscas.md",
    "05-aberturas.md",
    "06-engines-e-analise.md",
    "07-jogar.md",
    "08-treinar.md",
    "09-jogadores-e-torneios.md",
    "10-manutencao-e-relatorios.md",
    "11-aparencia-e-preferencias.md",
    "12-atalhos-de-teclado.md",
    "13-perguntas-frequentes.md",
    "14-glossario.md",
];

/// The most entries the history keeps (spec 4.3).
pub const HISTORY_LIMIT: usize = 100;

/// Where the reader is: a chapter and, maybe, a heading of it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Place {
    /// The index in [`CHAPTER_FILES`].
    pub chapter: usize,
    /// The slug of the heading the view scrolls to; `None` is the top.
    pub anchor: Option<String>,
    /// The scroll position (pixels from the top) the front-end reported when
    /// the reader left the place; used by Back and Forward when there is no
    /// anchor.
    pub scroll: f32,
}

/// The state of the manual window, kept by the application. It holds only
/// the navigation; the rendered chapter is a cache of the front-end.
#[derive(Debug, Clone, Default)]
pub struct ManualState {
    pub(crate) open: bool,
    pub(crate) index: Option<SharedIndex>,
    pub(crate) history: Vec<Place>,
    pub(crate) cursor: usize,
    pub(crate) query: String,
    pub(crate) results: Vec<SearchHit>,
    pub(crate) external: Option<String>,
    pub(crate) external_confirmed: bool,
    pub(crate) notice: Option<ManualNotice>,
    pub(crate) nav_serial: u64,
}

impl ManualState {
    /// Whether the window is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The titles and headings, once the front-end has given them.
    #[must_use]
    pub fn index(&self) -> Option<&ManualIndex> {
        self.index.as_ref().map(SharedIndex::get)
    }

    /// The handle of the index in use, to tell whether the front-end has
    /// to give another (the language changed).
    #[must_use]
    pub fn shared_index(&self) -> Option<&SharedIndex> {
        self.index.as_ref()
    }

    /// The current place (the top of the first chapter before any
    /// navigation).
    #[must_use]
    pub fn place(&self) -> Place {
        self.history.get(self.cursor).cloned().unwrap_or_default()
    }

    /// The current chapter.
    #[must_use]
    pub fn chapter(&self) -> usize {
        self.history.get(self.cursor).map_or(0, |p| p.chapter)
    }

    /// Whether Back has somewhere to go.
    #[must_use]
    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    /// Whether Forward has somewhere to go.
    #[must_use]
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }

    /// The text of the search field.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The results of the search by title (empty while the field is).
    #[must_use]
    pub fn results(&self) -> &[SearchHit] {
        &self.results
    }

    /// The address waiting for the answer "open it in the browser?".
    #[must_use]
    pub fn pending_external(&self) -> Option<&str> {
        self.external.as_deref()
    }

    /// The remark of the bar of the manual, if any.
    #[must_use]
    pub fn notice(&self) -> Option<ManualNotice> {
        self.notice
    }

    /// Counts every move of the reader (a link, a result, Back...). The
    /// front-end scrolls when the number changed.
    #[must_use]
    pub fn nav_serial(&self) -> u64 {
        self.nav_serial
    }

    /// Whether the manual in use is not in the language of the interface
    /// (spec 4.7).
    #[must_use]
    pub fn is_fallback(&self) -> bool {
        self.index().is_some_and(ManualIndex::is_fallback)
    }
}
