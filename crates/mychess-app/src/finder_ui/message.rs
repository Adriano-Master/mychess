//! Messages, effects and notices of the player and tournament finders.

use mychess_db::finder::{
    PlayerQuery, PlayerTable, TournamentQuery, TournamentRow, find_tournaments,
};

use super::players::{PlayerField, PlayerRowView, rows_of};
use super::tournaments::{TournamentAction, TournamentField};
use crate::Message;
use crate::bases::{OpenControl, SharedSource};
use crate::error::RequestId;
use crate::spelling_ui::{PlayerSortKey, Shared, SharedSpelling, TournamentSortKey};
use crate::tree::LockingSource;

/// The two windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// The player finder.
    Players,
    /// The tournament finder.
    Tournaments,
}

/// An event of the player finder or the tournament finder.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FinderMessage {
    /// Opens the player finder, with this start of a name when given (the
    /// "Lookup name" button; the other fields go back to the defaults).
    OpenPlayers {
        /// The start of the name.
        prefix: Option<String>,
    },
    /// Opens the player finder if closed, closes it if open.
    TogglePlayers,
    /// Closes the player finder and stops its work.
    ClosePlayers,
    /// Types in a field of the form.
    SetPlayerField(PlayerField, String),
    /// "Update": checks the form and asks again.
    UpdatePlayers,
    /// "Defaults".
    DefaultsPlayers,
    /// The click on a header: orders by that column (a second click turns
    /// the order round, D3313).
    SortPlayers(PlayerSortKey),
    /// The click on a row of the list: opens the information of the player.
    ClickPlayer(usize),
    /// Stops the building of the table.
    CancelPlayers,

    /// Opens the tournament finder, for a player when given (the
    /// "Tournaments" button of the player information; rule 4).
    OpenTournaments {
        /// The player.
        player: Option<String>,
    },
    /// Opens the tournament finder if closed, closes it if open.
    ToggleTournaments,
    /// Closes the tournament finder and stops its search.
    CloseTournaments,
    /// Types in a field of the form.
    SetTournamentField(TournamentField, String),
    /// "Update": checks the form and searches again.
    UpdateTournaments,
    /// "Defaults".
    DefaultsTournaments,
    /// The click on a header: orders by that column and searches again.
    SortTournaments(TournamentSortKey),
    /// The click on a row (the action is the front-end's choice: left click,
    /// right click, menu).
    ClickTournament {
        /// The row, from 0.
        index: usize,
        /// What to do.
        action: TournamentAction,
    },
    /// Stops the search of tournaments.
    CancelTournaments,

    /// The answer of [`FinderEffect::Build`].
    TableBuilt {
        /// Correlates the answer.
        request: RequestId,
        /// The table, `None` when stopped.
        table: Option<Shared<PlayerTable>>,
    },
    /// The answer of [`FinderEffect::Query`].
    PlayersQueried {
        /// Correlates the answer.
        request: RequestId,
        /// The rows.
        rows: Shared<Vec<PlayerRowView>>,
        /// The rows have the country column.
        with_country: bool,
    },
    /// The answer of [`FinderEffect::Tournaments`].
    TournamentsFound {
        /// Correlates the answer.
        request: RequestId,
        /// The tournaments, `None` when stopped.
        rows: Option<Vec<TournamentRow>>,
    },
    /// A task of the finder died (a panic in a worker): the previous rows
    /// stay on show and the user is told.
    Failed {
        /// The window.
        panel: Panel,
        /// Correlates the answer.
        request: RequestId,
    },
    /// The wait of [`FinderEffect::Delay`] ended.
    DelayElapsed {
        /// The window.
        panel: Panel,
        /// Correlates the answer.
        request: RequestId,
    },
}

/// What `perform` does for the finders.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FinderEffect {
    /// Builds the table of players; answers [`FinderMessage::TableBuilt`].
    Build {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The spelling file (estimated Elo).
        set: Option<SharedSpelling>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Queries the table; answers [`FinderMessage::PlayersQueried`].
    Query {
        /// Correlates the answer.
        request: RequestId,
        /// The table.
        table: Shared<PlayerTable>,
        /// The spelling file (countries).
        set: Option<SharedSpelling>,
        /// The query.
        query: Box<PlayerQuery>,
    },
    /// Finds the tournaments; answers [`FinderMessage::TournamentsFound`].
    Tournaments {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The query.
        query: Box<TournamentQuery>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Waits `ms` milliseconds; answers [`FinderMessage::DelayElapsed`].
    Delay {
        /// The window.
        panel: Panel,
        /// Correlates the answer.
        request: RequestId,
        /// How long.
        ms: u32,
    },
}

/// Milliseconds the finders wait for a burst of changes of the database to
/// end before they build again.
pub(crate) const DEBOUNCE_MS: u32 = 300;

/// A remark about the finders (Fluent keys in
/// [`FinderNotice::message_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FinderNotice {
    /// There is no database (`finder-no-base`).
    NoBase,
    /// The database changed since the list was made (`finder-stale`).
    Stale,
    /// The filter holds the games of the tournament (`tmt-filtered`).
    Filtered {
        /// The games in the filter.
        games: u32,
    },
    /// The work of the window failed and the rows on show are the previous
    /// ones (`finder-failed`).
    Failed,
}

impl FinderNotice {
    /// The Fluent key of the message.
    pub fn message_key(self) -> &'static str {
        match self {
            FinderNotice::NoBase => "finder-no-base",
            FinderNotice::Stale => "finder-stale",
            FinderNotice::Filtered { .. } => "tmt-filtered",
            FinderNotice::Failed => "finder-failed",
        }
    }
}

/// Runs an effect of the finders. Never panics: a task that dies is a
/// stopped answer.
pub(crate) async fn perform(effect: FinderEffect) -> Message {
    let message = match effect {
        FinderEffect::Build {
            request,
            source,
            set,
            control,
        } => {
            let table = tokio::task::spawn_blocking(move || {
                let db = LockingSource::new(source);
                let mut progress = control.reporter();
                // `Err` is only the cancellation: an answer without table.
                PlayerTable::build(
                    &db,
                    set.as_ref()
                        .map(|s| s.checker(mychess_db::si4::NameKind::Player)),
                    &control.token(),
                    &mut progress,
                )
                .ok()
                .map(Shared::new)
            })
            .await;
            match table {
                Ok(table) => FinderMessage::TableBuilt { request, table },
                Err(_) => FinderMessage::Failed {
                    panel: Panel::Players,
                    request,
                },
            }
        }
        FinderEffect::Query {
            request,
            table,
            set,
            query,
        } => {
            let with_country = query.country != mychess_db::finder::CountryFilter::No;
            let rows = tokio::task::spawn_blocking(move || {
                let found = table.query(
                    &query,
                    set.as_ref()
                        .map(|s| s.checker(mychess_db::si4::NameKind::Player)),
                );
                rows_of(&found)
            })
            .await;
            match rows {
                Ok(rows) => FinderMessage::PlayersQueried {
                    request,
                    rows: Shared::new(rows),
                    with_country,
                },
                Err(_) => FinderMessage::Failed {
                    panel: Panel::Players,
                    request,
                },
            }
        }
        FinderEffect::Tournaments {
            request,
            source,
            query,
            control,
        } => {
            let rows = tokio::task::spawn_blocking(move || {
                let db = LockingSource::new(source);
                let mut progress = control.reporter();
                find_tournaments(&db, &query, &control.token(), &mut progress).ok()
            })
            .await;
            match rows {
                Ok(rows) => FinderMessage::TournamentsFound { request, rows },
                Err(_) => FinderMessage::Failed {
                    panel: Panel::Tournaments,
                    request,
                },
            }
        }
        FinderEffect::Delay { panel, request, ms } => {
            tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
            FinderMessage::DelayElapsed { panel, request }
        }
    };
    Message::Finder(message)
}
