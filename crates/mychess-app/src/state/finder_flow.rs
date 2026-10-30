//! The glue of the player finder and the tournament finder with
//! [`AppState`] (spec M6-3, slice b).
//!
//! Everything here is bookkeeping, which `update` may do. The table of
//! players, the query of the table and the search of tournaments are the
//! effects of [`FinderEffect`]. Both windows follow the database of the
//! active slot: when it changes (another slot, a database read again, the
//! Clipbase growing, a new spelling file) the work is done again, after a
//! short wait if there was a result on show (spec rules 35 and 80).

use std::sync::Arc;

use mychess_db::finder::PlayerQuery;

use super::AppState;
use crate::bases::{BasesMessage, OpenControl};
use crate::effect::Effect;
use crate::finder_ui::{
    FinderEffect, FinderMessage, FinderNotice, Found, Panel, PlayerFinderView, PlayerForm,
    TableKey, TableShown, TourKey, TournamentAction, TournamentFinderView, TournamentForm,
    TournamentRowView,
};
use crate::flow::AppNotice;
use crate::panel_calc::Step;
use crate::player_info_ui::PlayerInfoMessage;
use crate::spelling_ui::{FinderPrefs, PlayerSortKey, Shared, TournamentPrefs};

/// Milliseconds the finders wait for a burst of changes to end.
const DEBOUNCE_MS: u32 = crate::finder_ui::DEBOUNCE_MS;

/// The direction of the order of a column when the user did not choose.
fn natural_descending(sort: PlayerSortKey) -> bool {
    matches!(
        sort,
        PlayerSortKey::Games | PlayerSortKey::Newest | PlayerSortKey::Elo
    )
}

impl AppState {
    /// The state of the finder windows.
    pub fn finder(&self) -> &crate::finder_ui::FinderUi {
        &self.finder
    }

    fn finder_notice(&mut self, notice: FinderNotice) {
        self.notice = Some(AppNotice::Finder(notice));
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn finder_cancel(&mut self) {
        self.finder.cancel();
    }

    fn slot_file_name(&self, slot: crate::bases::SlotId) -> Option<String> {
        self.bases
            .slot(slot)
            .and_then(|s| s.path.as_deref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    fn player_checker_fingerprint(&self) -> u64 {
        self.spelling.set.as_ref().map_or(0, |set| {
            set.checker(mychess_db::si4::NameKind::Player).fingerprint()
        })
    }

    /// What a table of players would be made from now.
    fn table_key(&self) -> Option<TableKey> {
        let slot = self.bases.current();
        let base = self.bases.slot(slot).filter(|s| !s.needs_reload)?;
        Some(TableKey {
            slot,
            source: base.source.clone(),
            len: base.total(),
            version: self.spelling.version,
            fingerprint: self.player_checker_fingerprint(),
        })
    }

    // ---- views ----

    /// The player finder, when open.
    pub fn player_finder_view(&self) -> Option<PlayerFinderView> {
        let ui = &self.finder.players;
        if !ui.open {
            return None;
        }
        let slot = self.bases.current();
        Some(PlayerFinderView {
            file_name: self.slot_file_name(slot),
            players: ui.table.as_ref().map_or(0, |t| t.table.len()),
            form: ui.form.clone(),
            rows: ui.rows.clone(),
            country_column: ui.with_country,
            sort: ui.sort,
            descending: ui.descending.unwrap_or_else(|| natural_descending(ui.sort)),
            building: ui.refresh.progress(),
            estimated: ui.table.as_ref().is_some_and(|t| t.table.has_estimates()),
            no_base: self.table_key().is_none(),
        })
    }

    /// The tournament finder, when open.
    pub fn tournament_finder_view(&self) -> Option<TournamentFinderView> {
        let ui = &self.finder.tournaments;
        if !ui.open {
            return None;
        }
        let player = ui
            .applied
            .as_ref()
            .map(|q| q.player.clone())
            .filter(|p| !p.is_empty());
        Some(TournamentFinderView {
            file_name: self.slot_file_name(self.bases.current()),
            player,
            form: ui.form.clone(),
            rows: ui.found.as_ref().map(|f| f.rows.clone()),
            sort: ui.sort,
            running: ui.refresh.progress(),
            no_base: self.bases.slot(self.bases.current()).is_none(),
        })
    }

    // ---- automatic work ----

    /// Starts the work the open windows need (called at the end of every
    /// `update`).
    pub(crate) fn refresh_finder(&mut self, effects: &mut Vec<Effect>) {
        self.refresh_players(effects);
        self.refresh_tournaments(effects);
    }

    fn refresh_players(&mut self, effects: &mut Vec<Effect>) {
        if !self.finder.players.open || self.slot_rewriting(self.bases.current()) {
            return;
        }
        let key = self.table_key();
        if key.is_none() {
            self.finder.players.table = None;
            self.finder.players.rows = Shared::new(Vec::new());
        }
        let has = self.finder.players.table.is_some();
        match self.finder.players.refresh.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key {
                    self.players_build(key, effects);
                }
            }
            Step::Debounce => {
                let request = self.new_request();
                self.finder.players.refresh.delay = Some(request);
                effects.push(Effect::Finder(FinderEffect::Delay {
                    panel: Panel::Players,
                    request,
                    ms: DEBOUNCE_MS,
                }));
            }
        }
    }

    fn players_build(&mut self, key: TableKey, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        let control = OpenControl::new();
        self.finder
            .players
            .refresh
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::Finder(FinderEffect::Build {
            request,
            source: key.source,
            set: self.spelling.set.clone(),
            control,
        }));
    }

    /// Asks the table on show for the rows of the applied query.
    fn players_query(&mut self, effects: &mut Vec<Effect>) {
        let (Some(shown), Some(query)) = (
            self.finder.players.table.as_ref(),
            self.finder.players.applied.clone(),
        ) else {
            return;
        };
        let table = shown.table.clone();
        let request = self.new_request();
        self.finder.players.querying = Some(request);
        effects.push(Effect::Finder(FinderEffect::Query {
            request,
            table,
            set: self.spelling.set.clone(),
            query: Box::new(query),
        }));
    }

    /// The query of the form as it stands.
    fn players_apply(&mut self, persist: bool, effects: &mut Vec<Effect>) {
        let prefs = self.prefs.player_tools.finder.clone();
        let ui = &mut self.finder.players;
        ui.form.normalize(&prefs);
        let query: PlayerQuery = ui.form.query(ui.sort, ui.descending);
        ui.applied = Some(query.clone());
        if persist {
            let (sort, descending) = (ui.sort, ui.descending);
            let (games, elo) = (query.games.clone(), query.elo.clone());
            let limit = u32::try_from(query.limit).unwrap_or(0);
            self.tools_prefs(
                move |p| {
                    p.finder.games_min = *games.start();
                    p.finder.games_max = *games.end();
                    p.finder.elo_min = *elo.start();
                    p.finder.elo_max = *elo.end();
                    p.finder.limit = limit;
                    p.finder.sort = sort;
                    p.finder.direction = match descending {
                        None => String::new(),
                        Some(true) => "desc".to_owned(),
                        Some(false) => "asc".to_owned(),
                    };
                },
                effects,
            );
        }
        let current = self.table_key();
        let valid = self
            .finder
            .players
            .table
            .as_ref()
            .is_some_and(|t| Some(&t.key) == current.as_ref());
        if valid {
            self.players_query(effects);
        }
    }

    fn players_open(&mut self, prefix: Option<String>, effects: &mut Vec<Effect>) {
        let prefs = self.prefs.player_tools.finder.clone();
        let ui = &mut self.finder.players;
        if !ui.started || prefix.is_some() {
            ui.form = PlayerForm::from_prefs(&if prefix.is_some() {
                FinderPrefs::default()
            } else {
                prefs.clone()
            });
            if let Some(prefix) = prefix {
                ui.form.prefix = prefix;
            }
            ui.sort = prefs.sort;
            ui.descending = match prefs.direction.as_str() {
                "desc" => Some(true),
                "asc" => Some(false),
                _ => None,
            };
            ui.started = true;
        }
        ui.open = true;
        ui.refresh.force();
        self.players_apply(false, effects);
    }

    fn refresh_tournaments(&mut self, effects: &mut Vec<Effect>) {
        if !self.finder.tournaments.open || self.slot_rewriting(self.bases.current()) {
            return;
        }
        let current = self.bases.current();
        let key = self
            .bases
            .slot(current)
            .filter(|s| !s.needs_reload)
            .and_then(|slot| {
                Some(TourKey {
                    slot: current,
                    source: slot.source.clone(),
                    len: slot.total(),
                    version: self.spelling.version,
                    query: self.finder.tournaments.applied.clone()?,
                })
            });
        let has = self.finder.tournaments.found.is_some();
        match self.finder.tournaments.refresh.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key {
                    let request = self.new_request();
                    let control = OpenControl::new();
                    self.finder
                        .tournaments
                        .refresh
                        .begin(request, control.clone(), key.clone());
                    self.finder.tournaments.running_request = Some(request);
                    effects.push(Effect::Finder(FinderEffect::Tournaments {
                        request,
                        source: key.source,
                        query: Box::new(key.query),
                        control,
                    }));
                }
            }
            Step::Debounce => {
                let request = self.new_request();
                self.finder.tournaments.refresh.delay = Some(request);
                effects.push(Effect::Finder(FinderEffect::Delay {
                    panel: Panel::Tournaments,
                    request,
                    ms: DEBOUNCE_MS,
                }));
            }
        }
    }

    fn tournaments_apply(&mut self, persist: bool, effects: &mut Vec<Effect>) {
        let ui = &mut self.finder.tournaments;
        ui.form.normalize();
        let query = ui.form.query(ui.sort);
        ui.applied = Some(query.clone());
        ui.refresh.force();
        if persist {
            let (start, sort) = (ui.form.start.clone(), ui.sort);
            let (players, games) = (query.players.clone(), query.games.clone());
            let limit = u32::try_from(query.limit).unwrap_or(0);
            self.tools_prefs(
                move |p| {
                    p.tournaments.start.clone_from(&start);
                    p.tournaments.min_players = *players.start();
                    p.tournaments.min_games = *games.start();
                    p.tournaments.limit = limit;
                    p.tournaments.sort = sort;
                },
                effects,
            );
        }
    }

    // ---- the messages ----

    /// Handles a [`FinderMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn finder_message(&mut self, message: FinderMessage, effects: &mut Vec<Effect>) {
        match message {
            FinderMessage::OpenPlayers { prefix } => self.players_open(prefix, effects),
            FinderMessage::TogglePlayers => {
                if self.finder.players.open {
                    self.finder_message(FinderMessage::ClosePlayers, effects);
                } else {
                    self.players_open(None, effects);
                }
            }
            FinderMessage::ClosePlayers => {
                let ui = &mut self.finder.players;
                ui.open = false;
                ui.refresh.cancel();
                ui.refresh.last_key = None;
                ui.querying = None;
            }
            FinderMessage::SetPlayerField(field, text) => self.finder.players.form.set(field, text),
            FinderMessage::UpdatePlayers => self.players_apply(true, effects),
            FinderMessage::DefaultsPlayers => {
                let ui = &mut self.finder.players;
                ui.form = PlayerForm::from_prefs(&FinderPrefs::default());
                ui.sort = PlayerSortKey::Name;
                ui.descending = None;
            }
            FinderMessage::SortPlayers(sort) => {
                let ui = &mut self.finder.players;
                if ui.sort == sort {
                    let now = ui.descending.unwrap_or_else(|| natural_descending(sort));
                    ui.descending = Some(!now);
                } else {
                    ui.sort = sort;
                    ui.descending = None;
                }
                self.players_apply(true, effects);
            }
            FinderMessage::ClickPlayer(index) => {
                let name = self.finder.players.rows.get(index).map(|r| r.name.clone());
                if let Some(name) = name {
                    self.player_info_message(PlayerInfoMessage::Open { name }, effects);
                }
            }
            FinderMessage::CancelPlayers => self.finder.players.refresh.stop(),
            FinderMessage::OpenTournaments { player } => {
                let prefs = self.prefs.player_tools.tournaments.clone();
                let ui = &mut self.finder.tournaments;
                if !ui.started || player.is_some() {
                    ui.form = match &player {
                        Some(name) => TournamentForm::for_player(name, &prefs),
                        None => TournamentForm::from_prefs(&prefs),
                    };
                    ui.sort = prefs.sort;
                    ui.started = true;
                }
                ui.open = true;
                self.tournaments_apply(false, effects);
            }
            FinderMessage::ToggleTournaments => {
                if self.finder.tournaments.open {
                    self.finder_message(FinderMessage::CloseTournaments, effects);
                } else {
                    self.finder_message(FinderMessage::OpenTournaments { player: None }, effects);
                }
            }
            FinderMessage::CloseTournaments => {
                let ui = &mut self.finder.tournaments;
                ui.open = false;
                ui.refresh.cancel();
                ui.refresh.last_key = None;
                ui.running_request = None;
            }
            FinderMessage::SetTournamentField(field, text) => {
                self.finder.tournaments.form.set(field, text);
            }
            FinderMessage::UpdateTournaments => self.tournaments_apply(true, effects),
            FinderMessage::DefaultsTournaments => {
                let ui = &mut self.finder.tournaments;
                ui.form = TournamentForm::from_prefs(&TournamentPrefs::default());
                ui.sort = crate::spelling_ui::TournamentSortKey::Date;
            }
            FinderMessage::SortTournaments(sort) => {
                self.finder.tournaments.sort = sort;
                self.tournaments_apply(true, effects);
            }
            FinderMessage::ClickTournament { index, action } => {
                self.tournament_click(index, action, effects);
            }
            FinderMessage::CancelTournaments => self.finder.tournaments.refresh.stop(),
            FinderMessage::TableBuilt { request, table } => {
                let Some(calc) = self.finder.players.refresh.finish(request) else {
                    return;
                };
                if let Some(table) = table {
                    self.finder.players.table = Some(TableShown {
                        key: calc.key,
                        table,
                    });
                    if self.finder.players.applied.is_none() {
                        self.players_apply(false, effects);
                    } else {
                        self.players_query(effects);
                    }
                }
            }
            FinderMessage::PlayersQueried {
                request,
                rows,
                with_country,
            } => {
                let ui = &mut self.finder.players;
                if ui.querying == Some(request) {
                    ui.querying = None;
                    ui.rows = rows;
                    ui.with_country = with_country;
                }
            }
            FinderMessage::TournamentsFound { request, rows } => {
                let Some(calc) = self.finder.tournaments.refresh.finish(request) else {
                    return;
                };
                self.finder.tournaments.running_request = None;
                if let Some(rows) = rows {
                    let views: Vec<TournamentRowView> = rows
                        .iter()
                        .enumerate()
                        .map(|(i, r)| TournamentRowView::of(i, r))
                        .collect();
                    self.finder.tournaments.found = Some(Found {
                        key: calc.key,
                        rows: Shared::new(views),
                        games: rows.iter().map(|r| Arc::clone(&r.game_numbers)).collect(),
                        first: rows.iter().map(|r| r.first_game).collect(),
                    });
                }
            }
            FinderMessage::Failed { panel, request } => match panel {
                Panel::Players => {
                    let ui = &mut self.finder.players;
                    let built = ui.refresh.finish(request).is_some();
                    let queried = ui.querying == Some(request);
                    if queried {
                        ui.querying = None;
                    }
                    if built || queried {
                        self.finder_notice(FinderNotice::Failed);
                    }
                }
                Panel::Tournaments => {
                    if self.finder.tournaments.refresh.finish(request).is_some() {
                        self.finder.tournaments.running_request = None;
                        self.finder_notice(FinderNotice::Failed);
                    }
                }
            },
            FinderMessage::DelayElapsed { panel, request } => match panel {
                Panel::Players => {
                    if self.finder.players.open
                        && self.finder.players.refresh.delay_elapsed(request)
                        && let Some(key) = self.table_key()
                    {
                        self.finder.players.refresh.last_key = Some(key.clone());
                        self.players_build(key, effects);
                    }
                }
                Panel::Tournaments => {
                    if self.finder.tournaments.open
                        && self.finder.tournaments.refresh.delay_elapsed(request)
                    {
                        self.finder.tournaments.refresh.force();
                    }
                }
            },
        }
    }

    fn tournament_click(
        &mut self,
        index: usize,
        action: TournamentAction,
        effects: &mut Vec<Effect>,
    ) {
        let Some(found) = self.finder.tournaments.found.as_ref() else {
            return;
        };
        let (Some(games), Some(&first)) = (found.games.get(index).cloned(), found.first.get(index))
        else {
            return;
        };
        let slot = found.key.slot;
        let same = self
            .bases
            .slot(slot)
            .is_some_and(|s| s.source == found.key.source && s.total() == found.key.len);
        if !same {
            self.finder_notice(FinderNotice::Stale);
            return;
        }
        match action {
            TournamentAction::OpenCrosstable => self.crosstable_message(
                crate::crosstable::CrosstableMessage::OpenGame { slot, game: first },
                effects,
            ),
            TournamentAction::LoadFirstGame => self.bases_message(
                BasesMessage::LoadGame {
                    slot,
                    number: first,
                },
                effects,
            ),
            TournamentAction::ShowInFilter => {
                if self.list_busy() {
                    return;
                }
                if let Some(base) = self.bases.slot_mut(slot) {
                    base.filter.fill(0);
                    for &n in games.iter() {
                        base.filter.set(n, 1);
                    }
                    base.filter_changed();
                    base.list.first = 1;
                    let count = base.filter.count();
                    self.finder_notice(FinderNotice::Filtered { games: count });
                }
            }
        }
    }
}
