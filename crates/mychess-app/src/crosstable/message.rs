//! Messages, effects and notices of the crosstable panel (spec M6-4, slice
//! a, D3352, D3354, D3357).

use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::PartialDate;
use mychess_db::progress::CancelToken;
use mychess_db::{
    Crosstable, CrosstableError, CrosstableLabels, CrosstableNotice as DbNotice, CrosstableOptions,
    TournamentReference, load_games_cancellable, render_html, render_latex, render_text,
    select_tournament,
};

use super::prefs::{CrossFlag, CrossFormat, CrossSort};
use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;
use crate::message::Message;
use crate::tree::LockingSource;

/// The file formats of the export (menu "File").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    /// Plain text, as the Scid prints it.
    Text,
    /// A complete HTML document.
    Html,
    /// A complete LaTeX document.
    Latex,
}

impl ExportFormat {
    /// The extension of the file, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Text => "txt",
            ExportFormat::Html => "html",
            ExportFormat::Latex => "tex",
        }
    }
}

/// The words of the table in the exported files and in the panel header.
/// The front-end translates them (from the Fluent keys of the panel) and
/// sends them with [`CrosstableMessage::SetLabels`]; the default is the
/// English of the Scid (D3364).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossLabels {
    /// Header of the name column (`Player`).
    pub player: String,
    /// Header of the rating column (`Rtng`).
    pub rating: String,
    /// Header of the title column (`Ti`).
    pub title: String,
    /// Header of the age column (`Age`).
    pub age: String,
    /// Header of the country column (`Nat`).
    pub nation: String,
    /// Header of the score column (`Score`).
    pub score: String,
    /// Header of the tie-break column (`Tie`).
    pub tie: String,
    /// Header of the performance column (`Perf`).
    pub performance: String,
    /// Header of the rating change column (`Chg`).
    pub change: String,
    /// Header of the tallies column in HTML (`+/-/=`).
    pub tallies: String,
    /// `Average Rating`.
    pub average_rating: String,
    /// `Category`.
    pub category: String,
    /// The word in `3 games: +2 -0 =1`.
    pub games: String,
}

impl Default for CrossLabels {
    fn default() -> CrossLabels {
        CrosstableLabels::default().into()
    }
}

impl From<CrosstableLabels> for CrossLabels {
    fn from(l: CrosstableLabels) -> CrossLabels {
        CrossLabels {
            player: l.player,
            rating: l.rating,
            title: l.title,
            age: l.age,
            nation: l.nation,
            score: l.score,
            tie: l.tie,
            performance: l.performance,
            change: l.change,
            tallies: l.tallies,
            average_rating: l.average_rating,
            category: l.category,
            games: l.games,
        }
    }
}

impl From<&CrossLabels> for CrosstableLabels {
    fn from(l: &CrossLabels) -> CrosstableLabels {
        CrosstableLabels {
            player: l.player.clone(),
            rating: l.rating.clone(),
            title: l.title.clone(),
            age: l.age.clone(),
            nation: l.nation.clone(),
            score: l.score.clone(),
            tie: l.tie.clone(),
            performance: l.performance.clone(),
            change: l.change.clone(),
            tallies: l.tallies.clone(),
            average_rating: l.average_rating.clone(),
            category: l.category.clone(),
            games: l.games.clone(),
        }
    }
}

/// A warning that goes with the table (`Notice` of the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrossWarning {
    /// More than 1000 players: the table is incomplete.
    PlayerLimit,
    /// All-play-all with more than 300 players: only the first are shown.
    TooManyPlayers,
    /// More games than the limit of a tournament: only the first were used.
    GameLimit,
    /// This many games could not be read and were left out.
    Unreadable(u32),
    /// The all-play-all grid is very large.
    LargeOutput,
}

impl CrossWarning {
    pub(crate) fn from_engine(notice: DbNotice) -> CrossWarning {
        match notice {
            DbNotice::PlayerLimit => CrossWarning::PlayerLimit,
            DbNotice::TooManyPlayers => CrossWarning::TooManyPlayers,
            DbNotice::GameLimit => CrossWarning::GameLimit,
            DbNotice::Unreadable(n) => CrossWarning::Unreadable(n),
            DbNotice::LargeOutput => CrossWarning::LargeOutput,
        }
    }

    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            CrossWarning::PlayerLimit => "crosstab-limit",
            CrossWarning::TooManyPlayers => "crosstab-too-many",
            CrossWarning::GameLimit => "crosstab-game-limit",
            CrossWarning::Unreadable(_) => "crosstab-unreadable",
            CrossWarning::LargeOutput => "crosstab-large",
        }
    }
}

/// A remark about the panel for the status bar, or why there is no table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrosstableNotice {
    /// No game from a database is in the editor (D3357).
    NoGame,
    /// The tournament has fewer than two players (`No crosstable for this
    /// game.`).
    NoTable,
    /// The game of the table does not exist (`Invalid game number`).
    InvalidGame,
    /// The table could not be computed.
    Failed,
    /// A job of the list holds the filter.
    Busy,
    /// The database changed since the table was computed.
    Stale,
    /// The filter holds the games of the tournament now.
    FilterSet,
    /// The games of the tournament were added to the filter.
    FilterAdded,
    /// The file was written.
    Exported,
    /// The file could not be written.
    ExportFailed,
    /// The text went to the clipboard.
    Copied,
    /// The user stopped the computation and no table was ready.
    Cancelled,
    /// The table changed while the file dialog was open: nothing was
    /// written (D3454).
    ExportStale,
}

impl CrosstableNotice {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            CrosstableNotice::NoGame => "crosstab-notice-no-game",
            CrosstableNotice::NoTable => "crosstab-no-table",
            CrosstableNotice::InvalidGame => "crosstab-notice-invalid-game",
            CrosstableNotice::Failed => "crosstab-notice-failed",
            CrosstableNotice::Busy => "crosstab-notice-busy",
            CrosstableNotice::Stale => "crosstab-notice-stale",
            CrosstableNotice::FilterSet => "crosstab-notice-filter-set",
            CrosstableNotice::FilterAdded => "crosstab-notice-filter-added",
            CrosstableNotice::Exported => "crosstab-notice-exported",
            CrosstableNotice::ExportFailed => "crosstab-notice-export-failed",
            CrosstableNotice::Copied => "crosstab-notice-copied",
            CrosstableNotice::Cancelled => "crosstab-notice-cancelled",
            CrosstableNotice::ExportStale => "crosstab-notice-export-stale",
        }
    }
}

/// Why a table could not be computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CrosstableFailure {
    /// Fewer than two players.
    NoTable,
    /// The game does not exist.
    InvalidGame,
    /// The index entry of the game is damaged.
    Unreadable,
    /// The computation was stopped.
    Cancelled,
    /// The computation died.
    Internal,
}

impl From<&CrosstableError> for CrosstableFailure {
    fn from(error: &CrosstableError) -> CrosstableFailure {
        match error {
            CrosstableError::InvalidGame(_) => CrosstableFailure::InvalidGame,
            CrosstableError::Read { .. } => CrosstableFailure::Unreadable,
            CrosstableError::NoTable => CrosstableFailure::NoTable,
            CrosstableError::Cancelled => CrosstableFailure::Cancelled,
            _ => CrosstableFailure::Internal,
        }
    }
}

/// The tags of the game in the editor that differ from the stored ones: the
/// table follows what the user sees (D3357).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagsRef {
    /// `Event`.
    pub event: String,
    /// `Site`.
    pub site: String,
    /// `Date`.
    pub date: PartialDate,
    /// `EventDate`.
    pub event_date: PartialDate,
}

/// What to compute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableSpec {
    /// The game the tournament is found from (1-based).
    pub game: u32,
    /// The tags of the editor, if the game on screen is that game.
    pub tags: Option<TagsRef>,
    /// The options.
    pub options: CrosstableOptions,
}

/// A computed table and the games of its tournament.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableDone {
    /// The table (shared: export and copy pass the same `Arc` to their
    /// effect instead of cloning the table inside `update`, D3451).
    pub table: Arc<Crosstable>,
    /// `(game number, round)` of every game of the tournament (round 0:
    /// not numeric), for "Set filter" and "Add to filter".
    pub games: Vec<(u32, u32)>,
}

/// Identifies the table that is on screen, so a front-end that asks for a
/// file name can tell, when the user answers, whether it is still the same
/// table (D3454). It keeps the table alive, so two tokens are equal only for
/// the very same computed table.
#[derive(Debug, Clone)]
pub struct CrosstableToken(pub(crate) Arc<Crosstable>);

impl PartialEq for CrosstableToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// An event of the crosstable panel.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CrosstableMessage {
    /// Opens the panel if closed, closes it if open.
    Toggle,
    /// Opens the panel: the table of the game on screen is computed.
    Open,
    /// Closes the panel and stops the computation.
    Close,
    /// "Refresh": follow the game on screen again and compute at once.
    Refresh,
    /// Shows the tournament of game `game` of `slot` and stops following the
    /// game on screen until [`Refresh`](CrosstableMessage::Refresh) (the
    /// tournament locator, the Event item of the game information).
    OpenGame {
        /// The database.
        slot: SlotId,
        /// The game number, from 1.
        game: u32,
    },
    /// Chooses the format.
    SetFormat(CrossFormat),
    /// Chooses the order (the click on a header).
    SetSort(CrossSort),
    /// Turns a checkbox of the options menu on or off.
    SetFlag(CrossFlag, bool),
    /// The colour of the alternate rows, `#rrggbb`.
    SetRowColor(String),
    /// The translated words of the table (D3364).
    SetLabels(Box<CrossLabels>),
    /// The click on a result: loads that game of the table's database.
    LoadGame(u32),
    /// "Set filter": the filter of the table's database holds the games of
    /// the round (`None`: all of them).
    SetFilter(Option<u32>),
    /// "Add to filter".
    AddToFilter(Option<u32>),
    /// Writes the table to `path` in `format`.
    Export {
        /// The format.
        format: ExportFormat,
        /// The file chosen by the user.
        path: PathBuf,
    },
    /// Like [`CrosstableMessage::Export`], for a file name that was proposed
    /// for the table of `token`: if another table is on screen now, nothing
    /// is written and [`CrosstableNotice::ExportStale`] is shown (D3454).
    ExportOf {
        /// The table the dialog was opened for.
        token: CrosstableToken,
        /// The format.
        format: ExportFormat,
        /// The file chosen by the user.
        path: PathBuf,
    },
    /// Copies the plain text of the table to the clipboard.
    Copy,
    /// Stops the computation in flight.
    Cancel,
    /// The answer of [`CrosstableEffect::Compute`].
    Computed {
        /// Correlates the answer.
        request: RequestId,
        /// The table, or why there is none.
        result: Box<Result<CrosstableDone, CrosstableFailure>>,
    },
    /// The debounce ended.
    DelayElapsed(RequestId),
    /// The answer of [`CrosstableEffect::Export`].
    Exported {
        /// Whether the file was written.
        ok: bool,
    },
    /// The text rendered for the clipboard.
    Rendered(String),
}

/// What `perform` does for the crosstable.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CrosstableEffect {
    /// Selects the games of the tournament and builds the table on a
    /// blocking thread; answers with [`CrosstableMessage::Computed`].
    Compute {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// What to compute.
        spec: Box<CrosstableSpec>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Waits `ms` milliseconds, then answers with
    /// [`CrosstableMessage::DelayElapsed`].
    Delay {
        /// Correlates the answer.
        request: RequestId,
        /// How long.
        ms: u32,
    },
    /// Renders the table and writes it to a file; answers with
    /// [`CrosstableMessage::Exported`].
    Export {
        /// The file.
        path: PathBuf,
        /// The table (the one on screen, shared).
        table: Arc<Crosstable>,
        /// The words.
        labels: Box<CrossLabels>,
        /// The format.
        format: ExportFormat,
    },
    /// Renders the plain text; answers with [`CrosstableMessage::Rendered`].
    Render {
        /// The table (the one on screen, shared).
        table: Arc<Crosstable>,
        /// The words.
        labels: Box<CrossLabels>,
    },
}

/// Milliseconds the panel waits for the tags of the game to stay still
/// before it computes the table again (D3354).
pub const DEBOUNCE_MS: u32 = 300;

fn answer(message: CrosstableMessage) -> Message {
    Message::Crosstable(message)
}

fn render(table: &Crosstable, labels: &CrossLabels, format: ExportFormat) -> String {
    let labels = CrosstableLabels::from(labels);
    match format {
        ExportFormat::Text => render_text(table, &labels),
        ExportFormat::Html => render_html(table, &labels),
        ExportFormat::Latex => render_latex(table, &labels),
    }
}

/// Selects, loads and builds (the work of [`CrosstableEffect::Compute`]).
pub(crate) fn run_compute(
    source: SharedSource,
    spec: &CrosstableSpec,
    control: &OpenControl,
) -> Result<CrosstableDone, CrosstableFailure> {
    let db = LockingSource::new(source);
    let mut progress = control.reporter();
    let cancel: CancelToken = control.token();
    let fail = |e: CrosstableError| CrosstableFailure::from(&e);
    let mut reference = TournamentReference::from_game(&db, spec.game).map_err(fail)?;
    if let Some(tags) = &spec.tags {
        // The editor shows these tags: use them. The stored bytes of a name
        // are kept only when the text did not change (the comparison is by
        // bytes, D3399.5).
        if reference.event != tags.event {
            reference.event.clone_from(&tags.event);
            reference.event_raw.clear();
        }
        if reference.site != tags.site {
            reference.site.clone_from(&tags.site);
            reference.site_raw.clear();
        }
        reference.date = tags.date;
        reference.event_date = tags.event_date;
    }
    let selection = select_tournament(
        &db,
        &reference,
        spec.options.include_deleted,
        &mut progress,
        &cancel,
    )
    .map_err(fail)?;
    let games = load_games_cancellable(&db, &selection, &cancel).map_err(fail)?;
    let table =
        Crosstable::from_games_cancellable(&games, &spec.options, None, &cancel).map_err(fail)?;
    let list = games.games.iter().map(|g| (g.number, g.round)).collect();
    Ok(CrosstableDone {
        table: Arc::new(table),
        games: list,
    })
}

/// Runs an effect of the crosstable. Never panics: a task that dies is a
/// failed answer.
pub(crate) async fn perform(effect: CrosstableEffect) -> Message {
    match effect {
        CrosstableEffect::Compute {
            request,
            source,
            spec,
            control,
        } => {
            let work_control = control.clone();
            let result =
                tokio::task::spawn_blocking(move || run_compute(source, &spec, &work_control))
                    .await
                    .unwrap_or(Err(CrosstableFailure::Internal));
            let result = match result {
                Ok(_) if control.is_cancelled() => Err(CrosstableFailure::Cancelled),
                other => other,
            };
            answer(CrosstableMessage::Computed {
                request,
                result: Box::new(result),
            })
        }
        CrosstableEffect::Delay { request, ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
            answer(CrosstableMessage::DelayElapsed(request))
        }
        CrosstableEffect::Export {
            path,
            table,
            labels,
            format,
        } => {
            let ok = tokio::task::spawn_blocking(move || {
                let text = render(&table, &labels, format);
                crate::fsutil::write_atomic(&path, |file| {
                    std::io::Write::write_all(file, text.as_bytes())
                })
                .map_err(|error| tracing::warn!(?error, "could not export the crosstable"))
                .is_ok()
            })
            .await
            .unwrap_or(false);
            answer(CrosstableMessage::Exported { ok })
        }
        CrosstableEffect::Render { table, labels } => {
            let text =
                tokio::task::spawn_blocking(move || render(&table, &labels, ExportFormat::Text))
                    .await
                    .unwrap_or_default();
            answer(CrosstableMessage::Rendered(text))
        }
    }
}
