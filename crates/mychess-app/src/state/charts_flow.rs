//! The glue of the statistics panel and of the rating graph with
//! [`AppState`] (spec M6-4, slice b): the panels follow the filter and the
//! database of the current slot, compute off the interface thread
//! (debounced) and drop the answers that are not the last asked for (D3352).

use std::sync::Arc;

use mychess_db::RatingPlayer;

use super::AppState;
use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::charts::{
    ChartsEffect, ChartsFailure, ChartsMessage, ChartsNotice, ChartsPart, DEBOUNCE_MS,
    FilterGraphPrefs, GraphKey, GraphShown, RatingKey, RatingPanelView, RatingPrefs, RatingShown,
    StatsKey, StatsLineView, StatsPanelView, StatsPrefs, StatsTab, graph_view, rating_view,
};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::panel_calc::{FilterStamp, Step};
use crate::prefs::{PrefChange, PrefsMessage};
use mychess_db::MAX_RATING_PLAYERS;

/// The database the panels read: the current slot.
struct Target {
    slot: SlotId,
    source: SharedSource,
    len: u32,
    /// Stamp of the filter and of the games.
    filter: FilterStamp,
}

impl AppState {
    /// The state of the statistics panel and of the rating graph.
    pub fn charts(&self) -> &crate::charts::ChartsUi {
        &self.charts
    }

    fn charts_target(&self) -> Option<Target> {
        let id = self.bases.current();
        let slot = self.bases.slot(id).filter(|s| !s.needs_reload)?;
        Some(Target {
            slot: id,
            source: slot.source.clone(),
            len: slot.total(),
            filter: FilterStamp::of(slot),
        })
    }

    /// The statistics panel, ready to draw.
    pub fn stats_view(&self) -> StatsPanelView {
        let ui = &self.charts;
        let prefs = &self.prefs.filter_graph;
        let slot = self.bases.current_slot();
        let mut graph_prefs = (**prefs).clone();
        if let Some(year) = ui.year {
            graph_prefs.year_max = prefs.config(year).year_max;
        }
        let stale_stats = ui
            .stats_shown
            .as_ref()
            .is_some_and(|(k, _)| ui.stats.last_key.as_ref() != Some(k));
        let graph = if prefs.tab == StatsTab::Table {
            None
        } else {
            ui.graph_shown.as_ref().map(|g| Arc::clone(&g.view))
        };
        let stale_graph = prefs.tab != StatsTab::Table
            && ui
                .graph_shown
                .as_ref()
                .is_none_or(|g| ui.graph.last_key.as_ref() != Some(&g.key));
        StatsPanelView {
            open: ui.stats_open,
            tab: prefs.tab,
            lines: ui
                .stats_shown
                .as_ref()
                .map(|(_, lines)| lines.iter().map(StatsLineView::from).collect())
                .unwrap_or_default(),
            options: ui.year.map(|y| self.prefs.stats.options_view(y)),
            graph,
            graph_prefs,
            stale: stale_stats || stale_graph,
            progress: ui.stats.progress().or_else(|| ui.graph.progress()),
            filter_games: slot.map_or(0, crate::bases::BaseSlot::filtered),
            base_games: slot.map_or(0, crate::bases::BaseSlot::total),
            unavailable: slot.is_none_or(|s| s.needs_reload),
            issue: ui.stats_issue,
        }
    }

    /// The rating graph, ready to draw.
    pub fn rating_view(&self) -> RatingPanelView {
        let ui = &self.charts;
        let graph = ui.rating_shown.as_ref().map(|r| Arc::clone(&r.view));
        let stale = ui.rating_shown.as_ref().map(|r| &r.key) != ui.rating.last_key.as_ref()
            || (ui.rating_shown.is_none() && !ui.players.is_empty());
        RatingPanelView {
            open: ui.rating_open,
            players: ui.players.clone(),
            can_add: ui.players.len() < MAX_RATING_PLAYERS,
            graph,
            prefs: self.prefs.rating_graph,
            stale,
            progress: ui.rating.progress(),
            unavailable: self.bases.current_slot().is_none_or(|s| s.needs_reload),
            issue: ui.rating_issue,
        }
    }

    /// Stops every computation (the program is ending).
    pub(crate) fn charts_cancel(&mut self) {
        self.charts.cancel();
    }

    // ---- the refresh at the end of every update ----

    /// Starts the computations of the open panels whose answer is not for
    /// the state now. Called at the end of every `update`.
    pub(crate) fn refresh_charts(&mut self, effects: &mut Vec<Effect>) {
        if !self.charts.stats_open && !self.charts.rating_open {
            return;
        }
        self.charts_refresh_graph_view();
        if self.charts.year.is_none() {
            if !self.charts.year_asked {
                self.charts.year_asked = true;
                effects.push(Effect::Charts(ChartsEffect::QueryYear));
            }
            // The rating graph does not need the year.
            if !self.charts.rating_open {
                return;
            }
        }
        let target = self.charts_target();
        self.refresh_stats(target.as_ref(), effects);
        self.refresh_graph_of_filter(target.as_ref(), effects);
        self.refresh_rating(target.as_ref(), effects);
    }

    /// Converts the graph of the filter again when the user switched
    /// between the absolute and the relative tab (D3425).
    fn charts_refresh_graph_view(&mut self) {
        let relative = self.prefs.filter_graph.tab == StatsTab::Relative;
        if let Some(shown) = &mut self.charts.graph_shown
            && shown.relative != relative
        {
            shown.view = Arc::new(graph_view(&shown.histogram, relative));
            shown.relative = relative;
        }
    }

    fn debounce(&mut self, part: ChartsPart, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        match part {
            ChartsPart::Stats => self.charts.stats.delay = Some(request),
            ChartsPart::Graph => self.charts.graph.delay = Some(request),
            ChartsPart::Rating => self.charts.rating.delay = Some(request),
        }
        effects.push(Effect::Charts(ChartsEffect::Delay {
            request,
            part,
            ms: DEBOUNCE_MS,
        }));
    }

    fn stats_key(&self, target: Option<&Target>) -> Option<StatsKey> {
        let year = self.charts.year?;
        let t = target?;
        Some(StatsKey {
            slot: t.slot,
            source: t.source.clone(),
            len: t.len,
            filter: t.filter,
            year,
            prefs: (*self.prefs.stats).clone(),
        })
    }

    /// Whether `last` is the key the state asks for: [`AppState::stats_key`]
    /// compared in place, without cloning the choices (D3421).
    fn stats_unchanged(&self, t: &Target, last: &StatsKey) -> bool {
        self.charts.year == Some(last.year)
            && last.slot == t.slot
            && last.source == t.source
            && last.len == t.len
            && last.filter == t.filter
            && last.prefs == *self.prefs.stats
    }

    /// The same for the rating graph.
    fn rating_unchanged(&self, t: &Target, last: &RatingKey) -> bool {
        let prefs = self.prefs.rating_graph;
        last.slot == t.slot
            && last.source == t.source
            && last.len == t.len
            && last.filter == Self::rating_filter(t, prefs)
            && last.players == self.charts.players
            && last.options == prefs.options()
    }

    /// The same for the graph of the filter.
    fn graph_unchanged(&self, t: &Target, last: &GraphKey) -> bool {
        let prefs = &self.prefs.filter_graph;
        self.charts
            .year
            .is_some_and(|year| last.config == prefs.config(year))
            && last.slot == t.slot
            && last.source == t.source
            && last.len == t.len
            && last.filter == t.filter
            && last.kind == prefs.kind
    }

    /// The filter counts only when the graph is limited to it.
    fn rating_filter(t: &Target, prefs: RatingPrefs) -> FilterStamp {
        if prefs.only_filter {
            t.filter
        } else {
            t.filter.data_only()
        }
    }

    fn graph_key(&self, target: Option<&Target>) -> Option<GraphKey> {
        let year = self.charts.year?;
        let t = target?;
        let prefs = &self.prefs.filter_graph;
        Some(GraphKey {
            slot: t.slot,
            source: t.source.clone(),
            len: t.len,
            filter: t.filter,
            kind: prefs.kind,
            config: prefs.config(year),
        })
    }

    fn rating_key(&self, target: Option<&Target>) -> Option<RatingKey> {
        let t = target?;
        if self.charts.players.is_empty() {
            return None;
        }
        let prefs = self.prefs.rating_graph;
        Some(RatingKey {
            slot: t.slot,
            source: t.source.clone(),
            len: t.len,
            filter: Self::rating_filter(t, prefs),
            players: self.charts.players.clone(),
            options: prefs.options(),
        })
    }

    fn refresh_stats(&mut self, target: Option<&Target>, effects: &mut Vec<Effect>) {
        if !self.charts.stats_open {
            return;
        }
        if let Some(t) = target
            && self
                .charts
                .stats
                .is_current(|last| self.stats_unchanged(t, last))
        {
            return;
        }
        let key = self.stats_key(target);
        let has = self.charts.stats_shown.is_some();
        match self.charts.stats.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key.clone() {
                    self.stats_start(key, effects);
                }
            }
            Step::Debounce => self.debounce(ChartsPart::Stats, effects),
        }
        if key.is_none() {
            self.charts.stats_shown = None;
            self.charts.stats_issue = None;
        }
    }

    fn refresh_graph_of_filter(&mut self, target: Option<&Target>, effects: &mut Vec<Effect>) {
        let wanted = self.charts.stats_open && self.prefs.filter_graph.tab != StatsTab::Table;
        if wanted
            && let Some(t) = target
            && self.charts.year.is_some()
            && self
                .charts
                .graph
                .is_current(|last| self.graph_unchanged(t, last))
        {
            return;
        }
        let key = if wanted { self.graph_key(target) } else { None };
        let has = self.charts.graph_shown.is_some();
        match self.charts.graph.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key {
                    self.graph_start(key, effects);
                }
            }
            Step::Debounce => self.debounce(ChartsPart::Graph, effects),
        }
        if !wanted {
            // Re-entering the tab starts at once.
            self.charts.graph.immediate = true;
        }
    }

    fn refresh_rating(&mut self, target: Option<&Target>, effects: &mut Vec<Effect>) {
        if !self.charts.rating_open {
            return;
        }
        if let Some(t) = target
            && !self.charts.players.is_empty()
            && self
                .charts
                .rating
                .is_current(|last| self.rating_unchanged(t, last))
        {
            return;
        }
        let key = self.rating_key(target);
        let has = self.charts.rating_shown.is_some();
        match self.charts.rating.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key.clone() {
                    self.rating_start(key, effects);
                }
            }
            Step::Debounce => self.debounce(ChartsPart::Rating, effects),
        }
        if key.is_none() {
            self.charts.rating_shown = None;
            // An empty list of players has its own remark (NoPlayers).
            if target.is_none() {
                self.charts.rating_issue = None;
            }
        }
    }

    fn filter_of_current(&self) -> Option<Box<mychess_db::Filter>> {
        self.bases
            .current_slot()
            .map(|s| Box::new(s.filter().clone()))
    }

    fn stats_start(&mut self, key: StatsKey, effects: &mut Vec<Effect>) {
        let Some(filter) = self.filter_of_current() else {
            return;
        };
        let request = self.new_request();
        let control = OpenControl::new();
        let config = Box::new(key.config());
        self.charts.stats_issue = None;
        self.charts
            .stats
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::Charts(ChartsEffect::Stats {
            request,
            source: key.source,
            filter,
            config,
            control,
        }));
    }

    fn graph_start(&mut self, key: GraphKey, effects: &mut Vec<Effect>) {
        let Some(filter) = self.filter_of_current() else {
            return;
        };
        let request = self.new_request();
        let control = OpenControl::new();
        self.charts.stats_issue = None;
        self.charts
            .graph
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::Charts(ChartsEffect::Graph {
            request,
            source: key.source,
            filter,
            kind: key.kind,
            config: key.config,
            control,
        }));
    }

    fn rating_start(&mut self, key: RatingKey, effects: &mut Vec<Effect>) {
        let filter = if key.options.only_filter {
            self.filter_of_current()
        } else {
            None
        };
        let request = self.new_request();
        let control = OpenControl::new();
        self.charts.rating_issue = None;
        self.charts
            .rating
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::Charts(ChartsEffect::Rating {
            request,
            source: key.source,
            filter,
            players: key
                .players
                .iter()
                .cloned()
                .map(RatingPlayer::Name)
                .collect(),
            options: key.options,
            control,
        }));
    }

    // ---- messages ----

    /// Handles a [`ChartsMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn charts_message(&mut self, message: ChartsMessage, effects: &mut Vec<Effect>) {
        match message {
            ChartsMessage::ToggleStats => {
                let open = !self.charts.stats_open;
                self.charts_set_stats_open(open);
            }
            ChartsMessage::OpenStats => self.charts_set_stats_open(true),
            ChartsMessage::CloseStats => self.charts_set_stats_open(false),
            ChartsMessage::SelectTab(tab) => {
                let changed = self.prefs.filter_graph.tab != tab;
                self.graph_prefs(|p| p.tab = tab, effects);
                self.charts.graph.immediate = true;
                // The remark belongs to the tab where it was made (D3508): a
                // click on the tab already active keeps it (D3512).
                if changed {
                    self.charts.stats_issue = None;
                }
            }
            ChartsMessage::SetStatsElo(value, on) => {
                // The menu is offered once the year is known (D3428).
                let Some(year) = self.charts.year else {
                    return;
                };
                let next = self.prefs.stats.with_elo(year, value, on);
                self.stats_prefs(next, effects);
            }
            ChartsMessage::SetStatsYear(value, on) => {
                let Some(year) = self.charts.year else {
                    return;
                };
                let next = self.prefs.stats.with_year(year, value, on);
                self.stats_prefs(next, effects);
            }
            ChartsMessage::ResetStats => self.stats_prefs(StatsPrefs::default(), effects),
            ChartsMessage::SetGraphKind(kind) => {
                self.graph_prefs(|p| p.kind = kind, effects);
                self.charts.graph.immediate = true;
            }
            ChartsMessage::SetGraphField { kind, field, value } => {
                self.graph_prefs(|p| p.set_field(kind, field, value), effects);
                self.charts.graph.immediate = true;
            }
            ChartsMessage::SetEstimate(on) => {
                self.graph_prefs(|p| p.estimate = on, effects);
                self.charts.graph.immediate = true;
            }
            ChartsMessage::SetGraphDots(on) => self.graph_prefs(|p| p.show_dots = on, effects),
            ChartsMessage::ResetGraphAxes => {
                let year = self.charts.year.unwrap_or(2000);
                let next = self.prefs.filter_graph.reset_axes(year);
                if next != *self.prefs.filter_graph {
                    self.prefs_message(PrefsMessage::Set(PrefChange::FilterGraph(next)), effects);
                }
                self.charts.graph.immediate = true;
            }
            ChartsMessage::RefreshStats => {
                self.charts.stats.force();
                self.charts.graph.force();
            }
            ChartsMessage::CancelStats => {
                let ui = &mut self.charts;
                let stats = ui.stats.running() || ui.stats.delay.is_some();
                let graph = ui.graph.running() || ui.graph.delay.is_some();
                ui.stats.user_cancel();
                ui.graph.user_cancel();
                if (stats && ui.stats_shown.is_none()) || (graph && ui.graph_shown.is_none()) {
                    ui.stats_issue = Some(ChartsNotice::Cancelled);
                }
            }
            ChartsMessage::SetYear(year) => {
                // A year the dates of the Scid can hold (D3428).
                let year = year.clamp(1000, 2999);
                if self.charts.year != Some(year) {
                    self.charts.year = Some(year);
                }
            }
            ChartsMessage::ToggleRating => {
                let open = !self.charts.rating_open;
                self.charts_set_rating_open(open);
            }
            ChartsMessage::OpenRating => self.charts_set_rating_open(true),
            ChartsMessage::CloseRating => self.charts_set_rating_open(false),
            ChartsMessage::AddPlayer(name) => self.rating_add(&name),
            ChartsMessage::RemovePlayer(name) => self.charts.players.retain(|p| *p != name),
            ChartsMessage::TogglePlayer(name) => {
                if self.charts.players.contains(&name) {
                    self.charts.players.retain(|p| *p != name);
                } else {
                    self.rating_add(&name);
                }
            }
            ChartsMessage::UseGamePlayers => self.rating_use_game_players(),
            ChartsMessage::ClearPlayers => self.charts.players.clear(),
            ChartsMessage::SetRatingStartYear(year) => {
                self.rating_prefs(|p| p.start_year = year, effects);
            }
            ChartsMessage::SetRatingStartElo(elo) => {
                self.rating_prefs(|p| p.start_elo = elo, effects);
            }
            ChartsMessage::SetRatingOnlyFilter(on) => {
                self.rating_prefs(|p| p.only_filter = on, effects);
            }
            ChartsMessage::SetRatingDots(on) => self.rating_prefs(|p| p.show_dots = on, effects),
            ChartsMessage::RefreshRating => self.charts.rating.force(),
            ChartsMessage::CancelRating => {
                let ui = &mut self.charts;
                if ui.rating.running() || ui.rating.delay.is_some() {
                    ui.rating.user_cancel();
                    if ui.rating_shown.is_none() {
                        ui.rating_issue = Some(ChartsNotice::Cancelled);
                    }
                }
            }
            ChartsMessage::StatsComputed { request, result } => {
                if let Some(calc) = self.charts.stats.finish(request) {
                    match *result {
                        Ok(lines) => self.charts.stats_shown = Some((calc.key, lines)),
                        Err(ChartsFailure::Cancelled) => {
                            self.charts_stopped_from_outside(ChartsPart::Stats);
                        }
                        Err(_) => {
                            self.charts.stats_shown = None;
                            self.charts.stats_issue = Some(ChartsNotice::StatsFailed);
                        }
                    }
                }
            }
            ChartsMessage::GraphComputed { request, result } => {
                if let Some(calc) = self.charts.graph.finish(request) {
                    match *result {
                        Ok(histogram) => {
                            let relative = self.prefs.filter_graph.tab == StatsTab::Relative;
                            self.charts.graph_shown = Some(GraphShown {
                                view: Arc::new(graph_view(&histogram, relative)),
                                relative,
                                histogram,
                                key: calc.key,
                            });
                        }
                        Err(ChartsFailure::Cancelled) => {
                            self.charts_stopped_from_outside(ChartsPart::Graph);
                        }
                        Err(_) => {
                            self.charts.graph_shown = None;
                            self.charts.stats_issue = Some(ChartsNotice::GraphFailed);
                        }
                    }
                }
            }
            ChartsMessage::RatingComputed { request, result } => {
                if let Some(calc) = self.charts.rating.finish(request) {
                    match *result {
                        Ok(series) => {
                            self.charts.rating_shown = Some(RatingShown {
                                view: Arc::new(rating_view(&calc.key.players, &series)),
                                key: calc.key,
                            });
                        }
                        Err(ChartsFailure::Cancelled) => {
                            if self.charts.rating_shown.is_none() {
                                self.charts.rating_issue = Some(ChartsNotice::Cancelled);
                            }
                        }
                        Err(_) => {
                            self.charts.rating_shown = None;
                            self.charts.rating_issue = Some(ChartsNotice::RatingFailed);
                        }
                    }
                }
            }
            ChartsMessage::DelayElapsed { request, part } => {
                self.charts_delay_elapsed(request, part, effects);
            }
        }
    }

    /// A computation of the statistics table or of the graph ended as
    /// cancelled while it was still the one in flight: nothing is left to
    /// compute (D3422). The remark is shown when the part that stopped has
    /// no result and is the one on view: the graph is judged by
    /// `graph_shown` (not by the table) and only on a graph tab (D3453).
    fn charts_stopped_from_outside(&mut self, part: ChartsPart) {
        let graph_tab = self.prefs.filter_graph.tab != StatsTab::Table;
        let ui = &mut self.charts;
        let none_shown = match part {
            ChartsPart::Graph => graph_tab && ui.graph_shown.is_none(),
            _ => !graph_tab && ui.stats_shown.is_none(),
        };
        if none_shown {
            ui.stats_issue = Some(ChartsNotice::Cancelled);
        }
    }

    fn charts_delay_elapsed(
        &mut self,
        request: RequestId,
        part: ChartsPart,
        effects: &mut Vec<Effect>,
    ) {
        let target = self.charts_target();
        match part {
            ChartsPart::Stats => {
                if self.charts.stats_open
                    && self.charts.stats.delay_elapsed(request)
                    && let Some(key) = self.stats_key(target.as_ref())
                {
                    self.charts.stats.last_key = Some(key.clone());
                    self.stats_start(key, effects);
                }
            }
            ChartsPart::Graph => {
                if self.charts.stats_open
                    && self.charts.graph.delay_elapsed(request)
                    && let Some(key) = self.graph_key(target.as_ref())
                {
                    self.charts.graph.last_key = Some(key.clone());
                    self.graph_start(key, effects);
                }
            }
            ChartsPart::Rating => {
                if self.charts.rating_open
                    && self.charts.rating.delay_elapsed(request)
                    && let Some(key) = self.rating_key(target.as_ref())
                {
                    self.charts.rating.last_key = Some(key.clone());
                    self.rating_start(key, effects);
                }
            }
        }
    }

    fn charts_set_stats_open(&mut self, open: bool) {
        self.charts.stats_open = open;
        if open {
            self.charts.stats.force();
            self.charts.graph.force();
        } else {
            self.charts.stats.cancel();
            self.charts.graph.cancel();
            self.charts.stats.last_key = None;
            self.charts.graph.last_key = None;
        }
    }

    fn charts_set_rating_open(&mut self, open: bool) {
        self.charts.rating_open = open;
        if open {
            self.charts.rating.force();
            if self.charts.players.is_empty() {
                self.rating_use_game_players();
            }
        } else {
            self.charts.rating.cancel();
            self.charts.rating.last_key = None;
        }
    }

    fn stats_prefs(&mut self, next: StatsPrefs, effects: &mut Vec<Effect>) {
        let next = next.sanitized();
        if next != *self.prefs.stats {
            self.prefs_message(PrefsMessage::Set(PrefChange::Stats(next)), effects);
            // A click on an option starts at once (D3410, D3423).
            self.charts.stats.immediate = true;
        }
    }

    fn graph_prefs(
        &mut self,
        change: impl FnOnce(&mut FilterGraphPrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut next = (*self.prefs.filter_graph).clone();
        change(&mut next);
        let next = next.sanitized();
        if next != *self.prefs.filter_graph {
            self.prefs_message(PrefsMessage::Set(PrefChange::FilterGraph(next)), effects);
        }
    }

    fn rating_prefs(&mut self, change: impl FnOnce(&mut RatingPrefs), effects: &mut Vec<Effect>) {
        let mut next = self.prefs.rating_graph;
        change(&mut next);
        let next = next.sanitized();
        if next != self.prefs.rating_graph {
            self.prefs_message(PrefsMessage::Set(PrefChange::RatingGraph(next)), effects);
            self.charts.rating.immediate = true;
        }
    }

    /// A name worth a line: not empty and not the `?` of an unknown player.
    fn rating_name(name: &str) -> Option<String> {
        let name = name.trim();
        (!name.is_empty() && name != "?").then(|| name.to_owned())
    }

    fn rating_add(&mut self, name: &str) {
        let Some(name) = Self::rating_name(name) else {
            return;
        };
        if self.charts.players.contains(&name) {
            return;
        }
        if self.charts.players.len() >= MAX_RATING_PLAYERS {
            self.charts.rating_issue = Some(ChartsNotice::TooManyPlayers);
            return;
        }
        self.charts.players.push(name);
        self.charts.rating.immediate = true;
    }

    fn rating_use_game_players(&mut self) {
        let tags = self.session.game().tags();
        let names: Vec<String> = [&tags.white, &tags.black]
            .into_iter()
            .filter_map(|n| Self::rating_name(n))
            .collect();
        if names.is_empty() {
            self.charts.rating_issue = Some(ChartsNotice::NoPlayers);
            return;
        }
        self.charts.players.clear();
        for name in names {
            if !self.charts.players.contains(&name) {
                self.charts.players.push(name);
            }
        }
        self.charts.rating.immediate = true;
    }
}
