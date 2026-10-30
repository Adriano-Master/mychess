//! The glue of the player information window with [`AppState`] (spec M6-3,
//! slice b): it follows the player, the filter and the game on screen, and
//! answers the links that set the filter.

use super::AppState;
use crate::bases::OpenControl;
use crate::effect::Effect;
use crate::finder_ui::FinderMessage;
use crate::flow::AppNotice;
use crate::panel_calc::{FilterStamp, Step};
use crate::player_info_ui::{
    DEBOUNCE_MS, FilterJob, FilterRun, InfoJob, InfoKey, InfoShown, InfoStats, PlayerInfoEffect,
    PlayerInfoMessage, PlayerInfoNotice, PlayerInfoView, parse_link,
};

impl AppState {
    /// The state of the player information window.
    pub fn player_info(&self) -> &crate::player_info_ui::PlayerInfoUi {
        &self.player_info
    }

    fn info_notice(&mut self, notice: PlayerInfoNotice) {
        self.notice = Some(AppNotice::PlayerInfo(notice));
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn player_info_cancel(&mut self) {
        self.player_info.cancel();
    }

    /// The other name of the game on screen, when `name` is one of them.
    fn info_opponent(&self, name: &str) -> Option<String> {
        let tags = self.session.game().tags();
        let other = if tags.white == name {
            &tags.black
        } else if tags.black == name {
            &tags.white
        } else {
            return None;
        };
        (!other.is_empty() && other != "?").then(|| other.clone())
    }

    fn info_key(&self) -> Option<InfoKey> {
        let ui = &self.player_info;
        if !ui.open || ui.name.is_empty() {
            return None;
        }
        let id = self.bases.current();
        let slot = self.bases.slot(id).filter(|s| !s.needs_reload)?;
        Some(InfoKey {
            slot: id,
            source: slot.source.clone(),
            len: slot.total(),
            version: self.spelling.version,
            name: ui.name.clone(),
            opponent: self.info_opponent(&ui.name),
            filter: FilterStamp::of(slot),
            fingerprint: self.spelling.set.as_ref().map_or(0, |set| {
                set.checker(mychess_db::si4::NameKind::Player).fingerprint()
            }),
            photos: self.spelling.photos.clone(),
        })
    }

    /// The progress of the count of the open window (cheap: nothing is
    /// cloned); `None` when closed or idle.
    pub fn player_info_progress(&self) -> Option<(u64, u64)> {
        if !self.player_info.open {
            return None;
        }
        self.player_info.refresh.progress()
    }

    /// The window, when open.
    pub fn player_info_view(&self) -> Option<PlayerInfoView> {
        let ui = &self.player_info;
        if !ui.open {
            return None;
        }
        let shown = ui.shown.as_ref().filter(|s| s.key.name == ui.name);
        let stale = ui
            .shown
            .as_ref()
            .is_some_and(|s| s.key.name != ui.name || ui.refresh.last_key.as_ref() != Some(&s.key));
        Some(PlayerInfoView {
            name: ui.name.clone(),
            found: shown.is_some_and(|s| s.done.info.found),
            header: shown.and_then(|s| s.done.header.clone()),
            stats: shown
                .filter(|s| s.done.info.found)
                .map(|s| InfoStats::of(&s.done.info)),
            photo: shown.and_then(|s| s.done.photo.clone()),
            progress: ui.refresh.progress(),
            stale,
            history: self.prefs.player_tools.history.clone(),
            opponent: self.info_opponent(&ui.name),
            spelling_loaded: self.spelling.set.is_some(),
            no_base: self.bases.slot(self.bases.current()).is_none(),
        })
    }

    /// Starts the count the open window needs (called at the end of every
    /// `update`).
    pub(crate) fn refresh_player_info(&mut self, effects: &mut Vec<Effect>) {
        if !self.player_info.open {
            return;
        }
        // Not while the names or ratings of the slot are being written.
        if self.slot_rewriting(self.bases.current()) {
            return;
        }
        let key = self.info_key();
        let has = self.player_info.shown.is_some();
        match self.player_info.refresh.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key {
                    self.info_start(&key, effects);
                }
            }
            Step::Debounce => {
                let request = self.new_request();
                self.player_info.refresh.delay = Some(request);
                effects.push(Effect::PlayerInfo(PlayerInfoEffect::Delay {
                    request,
                    ms: DEBOUNCE_MS,
                }));
            }
        }
    }

    fn info_start(&mut self, key: &InfoKey, effects: &mut Vec<Effect>) {
        let Some(slot) = self.bases.slot(key.slot) else {
            return;
        };
        let job = InfoJob {
            source: key.source.clone(),
            name: key.name.clone(),
            filter: Some(Box::new(slot.filter.clone())),
            opponent: key.opponent.clone(),
            set: self.spelling.set.clone(),
            photos: self.spelling.photos.clone(),
        };
        let request = self.new_request();
        let control = OpenControl::new();
        self.player_info
            .refresh
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::PlayerInfo(PlayerInfoEffect::Compute {
            request,
            job: Box::new(job),
            control,
        }));
    }

    /// Handles a [`PlayerInfoMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn player_info_message(
        &mut self,
        message: PlayerInfoMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            PlayerInfoMessage::Open { name } => {
                let mut name = name.trim().to_owned();
                if name.is_empty() {
                    name.clone_from(&self.player_info.name);
                }
                if name.is_empty() {
                    let white = &self.session.game().tags().white;
                    if !white.is_empty() && white != "?" {
                        name.clone_from(white);
                    }
                }
                if name.is_empty() {
                    self.info_notice(PlayerInfoNotice::NoPlayer);
                    return;
                }
                let remembered = name.clone();
                self.tools_prefs(move |p| p.remember(&remembered), effects);
                let ui = &mut self.player_info;
                ui.name = name;
                ui.open = true;
                ui.refresh.force();
            }
            PlayerInfoMessage::Close => {
                self.player_info.open = false;
                self.player_info.cancel();
                self.player_info.refresh.last_key = None;
            }
            PlayerInfoMessage::Refresh => self.player_info.refresh.force(),
            PlayerInfoMessage::Cancel => self.player_info.refresh.stop(),
            PlayerInfoMessage::FilterLink(code) => self.info_link(&code, effects),
            PlayerInfoMessage::OpenFide => {
                let url = self
                    .player_info
                    .shown
                    .as_ref()
                    .filter(|s| s.key.name == self.player_info.name)
                    .and_then(|s| s.done.header.as_ref())
                    .and_then(|h| h.fide_url.clone());
                match url {
                    Some(url) => {
                        effects.push(Effect::PlayerInfo(PlayerInfoEffect::OpenUrl { url }));
                    }
                    None => self.info_notice(PlayerInfoNotice::NoFide),
                }
            }
            PlayerInfoMessage::UrlOpened { ok } => {
                if !ok {
                    self.info_notice(PlayerInfoNotice::FideFailed);
                }
            }
            PlayerInfoMessage::LookupName => {
                let prefix = self
                    .player_info
                    .name
                    .split(' ')
                    .next()
                    .map(str::to_owned)
                    .filter(|p| !p.is_empty());
                self.finder_message(FinderMessage::OpenPlayers { prefix }, effects);
            }
            PlayerInfoMessage::OpenTournaments => {
                let player = Some(self.player_info.name.clone()).filter(|p| !p.is_empty());
                self.finder_message(FinderMessage::OpenTournaments { player }, effects);
            }
            PlayerInfoMessage::DelayElapsed(request) => {
                if self.player_info.open && self.player_info.refresh.delay_elapsed(request) {
                    self.player_info.refresh.force();
                }
            }
            PlayerInfoMessage::Computed { request, result } => {
                let Some(calc) = self.player_info.refresh.finish(request) else {
                    return;
                };
                match *result {
                    Some(done) => {
                        self.player_info.shown = Some(InfoShown {
                            key: calc.key,
                            done,
                        });
                    }
                    // A cancelled count says nothing; any other end without
                    // figures is a failure the user is told about (D3429).
                    None if !calc.control.is_cancelled() => {
                        self.info_notice(PlayerInfoNotice::Failed);
                    }
                    None => {}
                }
            }
            PlayerInfoMessage::FilterReady { request, filter } => {
                let matches = self
                    .player_info
                    .filter_run
                    .as_ref()
                    .is_some_and(|r| r.request == request);
                if !matches {
                    return;
                }
                let Some(run) = self.player_info.filter_run.take() else {
                    return;
                };
                let Some(filter) = *filter else {
                    return;
                };
                let Some(slot) = self.bases.slot_mut(run.slot) else {
                    return;
                };
                if slot.source != run.source
                    || filter.len() != slot.filter.len()
                    || FilterStamp::of(slot) != run.filter_fingerprint
                {
                    self.info_notice(PlayerInfoNotice::Stale);
                    return;
                }
                let games = filter.count();
                slot.filter = filter;
                slot.filter_changed();
                slot.list.first = 1;
                self.info_notice(PlayerInfoNotice::FilterSet { games });
            }
        }
    }

    fn info_link(&mut self, code: &str, effects: &mut Vec<Effect>) {
        let Some(link) = parse_link(code) else {
            self.info_notice(PlayerInfoNotice::BadLink);
            return;
        };
        let name = self.player_info.name.clone();
        if name.is_empty() {
            self.info_notice(PlayerInfoNotice::NoPlayer);
            return;
        }
        let id = self.bases.current();
        if self.list_busy() {
            return;
        }
        let Some(slot) = self.bases.slot(id) else {
            self.info_notice(PlayerInfoNotice::NoBase);
            return;
        };
        let job = FilterJob {
            source: slot.source.clone(),
            name: name.clone(),
            selection: link.selection,
            mode: link.mode,
            opponent: self.info_opponent(&name),
            filter: Box::new(slot.filter.clone()),
        };
        let source = job.source.clone();
        let filter_fingerprint = FilterStamp::of(slot);
        if let Some(old) = self.player_info.filter_run.take() {
            old.control.cancel();
        }
        let request = self.new_request();
        let control = OpenControl::new();
        self.player_info.filter_run = Some(FilterRun {
            request,
            slot: id,
            source,
            filter_fingerprint,
            control: control.clone(),
        });
        effects.push(Effect::PlayerInfo(PlayerInfoEffect::SetFilter {
            request,
            job: Box::new(job),
            control,
        }));
    }
}
