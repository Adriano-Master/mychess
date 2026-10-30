//! Messages, effects and notices of the statistics and graphs of the filter
//! and of the rating graph (spec M6-4, slice b, D3352, D3354).

use mychess_db::{
    CancelToken, Filter, FilterGraphConfig, GraphKind, Histogram, RatingOptions, RatingPlayer,
    RatingSeries, StatsConfig, StatsError, StatsLine, filter_histogram, filter_stats_table,
    rating_histories,
};

use super::histogram::{FilterGraphKind, GraphField, StatsTab};
use crate::bases::{OpenControl, SharedSource};
use crate::error::RequestId;
use crate::message::Message;
use crate::tree::LockingSource;

/// Milliseconds the panels wait for the filter to stay still before they
/// compute again (D3354).
pub const DEBOUNCE_MS: u32 = 300;

/// Which computation of the panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartsPart {
    /// The table of the statistics.
    Stats,
    /// The graph of the filter.
    Graph,
    /// The rating graph.
    Rating,
}

/// Why a computation gave nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChartsFailure {
    /// The filter does not cover the database.
    FilterSize,
    /// Stopped.
    Cancelled,
    /// The computation died.
    Internal,
}

impl From<&StatsError> for ChartsFailure {
    fn from(error: &StatsError) -> ChartsFailure {
        match error {
            StatsError::FilterSize { .. } => ChartsFailure::FilterSize,
            StatsError::Cancelled => ChartsFailure::Cancelled,
            _ => ChartsFailure::Internal,
        }
    }
}

/// A remark of the panels for the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChartsNotice {
    /// The statistics could not be computed.
    StatsFailed,
    /// The graph of the filter could not be computed.
    GraphFailed,
    /// The rating graph could not be computed.
    RatingFailed,
    /// The user stopped the computation and nothing was ready.
    Cancelled,
    /// The graph holds nine players already.
    TooManyPlayers,
    /// The game on screen names no player.
    NoPlayers,
}

impl ChartsNotice {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            ChartsNotice::StatsFailed => "charts-notice-stats-failed",
            ChartsNotice::GraphFailed => "charts-notice-graph-failed",
            ChartsNotice::RatingFailed => "charts-notice-rating-failed",
            ChartsNotice::Cancelled => "charts-notice-cancelled",
            ChartsNotice::TooManyPlayers => "charts-notice-too-many-players",
            ChartsNotice::NoPlayers => "charts-notice-no-players",
        }
    }
}

/// An event of the statistics panel or of the rating graph.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ChartsMessage {
    /// Opens the statistics panel, or closes it when it is open.
    ToggleStats,
    /// Opens the statistics panel.
    OpenStats,
    /// Closes the statistics panel.
    CloseStats,
    /// Chooses the tab: table, relative graph or absolute graph.
    SelectTab(StatsTab),
    /// Checks or unchecks a rating threshold of the table.
    SetStatsElo(u16, bool),
    /// Checks or unchecks a year of the table.
    SetStatsYear(u16, bool),
    /// The lines of the table back to the defaults.
    ResetStats,
    /// Chooses the graph (decade, year, rating, moves).
    SetGraphKind(FilterGraphKind),
    /// Sets one value of the X axis of a graph.
    SetGraphField {
        /// The graph the value belongs to.
        kind: FilterGraphKind,
        /// Which value.
        field: GraphField,
        /// The value (limited to four digits).
        value: i32,
    },
    /// "Estimate" of the rating graph.
    SetEstimate(bool),
    /// "Show dots" of the graphs of the filter.
    SetGraphDots(bool),
    /// The X axes back to the defaults.
    ResetGraphAxes,
    /// Computes the statistics and the graph again.
    RefreshStats,
    /// Stops the computations of the statistics panel.
    CancelStats,
    /// The current year, for the default years and the graph (the
    /// front-end sends it, or the answer of [`ChartsEffect::QueryYear`]).
    SetYear(u16),
    /// Opens the rating graph, or closes it when it is open.
    ToggleRating,
    /// Opens the rating graph; with no players it takes those of the game on
    /// screen.
    OpenRating,
    /// Closes the rating graph.
    CloseRating,
    /// Adds a player by name (at most nine).
    AddPlayer(String),
    /// Takes a player out.
    RemovePlayer(String),
    /// Adds the player, or takes them out when they are in the graph (the
    /// menu of recent players).
    TogglePlayer(String),
    /// The players are the White and Black of the game on screen.
    UseGamePlayers,
    /// Takes every player out; the graph is empty.
    ClearPlayers,
    /// "Start year".
    SetRatingStartYear(u16),
    /// "Start rating".
    SetRatingStartElo(u16),
    /// Count only the games in the filter.
    SetRatingOnlyFilter(bool),
    /// "Show dots" of the rating graph.
    SetRatingDots(bool),
    /// Computes the rating graph again.
    RefreshRating,
    /// Stops the computation of the rating graph.
    CancelRating,
    /// The answer of [`ChartsEffect::Stats`].
    StatsComputed {
        /// Correlates the answer.
        request: RequestId,
        /// The lines, or why there are none.
        result: Box<Result<Vec<StatsLine>, ChartsFailure>>,
    },
    /// The answer of [`ChartsEffect::Graph`].
    GraphComputed {
        /// Correlates the answer.
        request: RequestId,
        /// The counts, or why there are none.
        result: Box<Result<Histogram, ChartsFailure>>,
    },
    /// The answer of [`ChartsEffect::Rating`].
    RatingComputed {
        /// Correlates the answer.
        request: RequestId,
        /// One series per player, or why there are none.
        result: Box<Result<Vec<RatingSeries>, ChartsFailure>>,
    },
    /// A debounce ended.
    DelayElapsed {
        /// Correlates the answer.
        request: RequestId,
        /// Which computation waited.
        part: ChartsPart,
    },
}

/// What `perform` does for the statistics and the graphs.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ChartsEffect {
    /// Counts the lines of the table in one pass over the index.
    Stats {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// A copy of the filter.
        filter: Box<Filter>,
        /// The lines.
        config: Box<StatsConfig>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Counts the bins of a graph of the filter.
    Graph {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// A copy of the filter.
        filter: Box<Filter>,
        /// The graph.
        kind: FilterGraphKind,
        /// The axis, already normalised.
        config: FilterGraphConfig,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// The histories of the players in one pass.
    Rating {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// A copy of the filter, when only its games count.
        filter: Option<Box<Filter>>,
        /// The players by name.
        players: Vec<RatingPlayer>,
        /// The options.
        options: RatingOptions,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Waits, then answers with [`ChartsMessage::DelayElapsed`].
    Delay {
        /// Correlates the answer.
        request: RequestId,
        /// Which computation waits.
        part: ChartsPart,
        /// How long, in milliseconds.
        ms: u32,
    },
    /// Asks for the current year (UTC); answers with
    /// [`ChartsMessage::SetYear`].
    QueryYear,
}

fn answer(message: ChartsMessage) -> Message {
    Message::Charts(message)
}

fn year_of_today() -> u16 {
    crate::effect::today_utc().year().unwrap_or(2000)
}

/// Runs an effect. Never panics: a task that dies is an internal failure.
pub(crate) async fn perform(effect: ChartsEffect) -> Message {
    match effect {
        ChartsEffect::Stats {
            request,
            source,
            filter,
            config,
            control,
        } => {
            let work = control.clone();
            let result = tokio::task::spawn_blocking(move || {
                let db = LockingSource::new(source);
                let mut progress = work.reporter();
                filter_stats_table(&db, &filter, &config, &mut progress, &work.token())
                    .map_err(|e| ChartsFailure::from(&e))
            })
            .await
            .unwrap_or(Err(ChartsFailure::Internal));
            answer(ChartsMessage::StatsComputed {
                request,
                result: Box::new(result),
            })
        }
        ChartsEffect::Graph {
            request,
            source,
            filter,
            kind,
            config,
            control,
        } => {
            let work = control.clone();
            let result = tokio::task::spawn_blocking(move || {
                let db = LockingSource::new(source);
                let mut progress = work.reporter();
                let kind: GraphKind = kind.engine();
                filter_histogram(&db, &filter, kind, &config, &mut progress, &work.token())
                    .map_err(|e| ChartsFailure::from(&e))
            })
            .await
            .unwrap_or(Err(ChartsFailure::Internal));
            answer(ChartsMessage::GraphComputed {
                request,
                result: Box::new(result),
            })
        }
        ChartsEffect::Rating {
            request,
            source,
            filter,
            players,
            options,
            control,
        } => {
            let work = control.clone();
            let result = tokio::task::spawn_blocking(move || {
                let db = LockingSource::new(source);
                let mut progress = work.reporter();
                let cancel: CancelToken = work.token();
                rating_histories(
                    &db,
                    &players,
                    options,
                    filter.as_deref(),
                    &mut progress,
                    &cancel,
                )
                .map_err(|e| ChartsFailure::from(&e))
            })
            .await
            .unwrap_or(Err(ChartsFailure::Internal));
            answer(ChartsMessage::RatingComputed {
                request,
                result: Box::new(result),
            })
        }
        ChartsEffect::Delay { request, part, ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
            answer(ChartsMessage::DelayElapsed { request, part })
        }
        ChartsEffect::QueryYear => answer(ChartsMessage::SetYear(year_of_today())),
    }
}
