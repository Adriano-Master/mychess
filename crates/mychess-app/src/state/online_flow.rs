//! The online play in the state (spec M8-3, phase b).
//!
//! The rules are in [`crate::online`] (a pure state machine); this is the
//! glue: it hands the machine the little it needs, runs what it asks for
//! (effects, stopping everything assistive, putting the game in the editor
//! and storing it at the end) and enforces the lock of D1330 together with
//! [`update`](super::update). Nothing here blocks.

use std::path::Path;
use std::time::Instant;

use mychess_core::{Color, Game, NodeId};

use super::AppState;
use crate::bases::SlotId;
use crate::effect::Effect;
use crate::engines::RunId;
use crate::files::GameOrigin;
use crate::flow::AppNotice;
use crate::online::{
    Ctx, EndKind, OnlineMessage, OnlineNotice, OnlineState, Out, SaveTo, last_node,
};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};
use crate::sound::SoundCue;

/// The game the editor held before the online game took its place (D1330:
/// a dedicated session game; the old one comes back only on request).
#[derive(Debug, Clone)]
struct Parked {
    game: Game,
    origin: GameOrigin,
    cursor: NodeId,
    dirty: bool,
}

/// What the glue remembers between messages.
#[derive(Debug, Default)]
pub(crate) struct OnlineFlow {
    parked: Option<Parked>,
    /// The generation of the session game that holds the online game: any
    /// other generation means the user put something else in the editor,
    /// which must not be overwritten.
    generation: Option<u64>,
    /// The `Site` of the games stored in this session (D1335).
    imported: Vec<String>,
    /// The watched game as last put in the editor: a change by the user
    /// stops the updates, so his notes are never overwritten (D3004).
    watch_shown: Option<Game>,
}

impl AppState {
    /// The online play.
    pub fn online(&self) -> &OnlineState {
        &self.online
    }

    /// Whether the assistance is locked (spec M8-3, D1330): engines, the
    /// opening tree, evaluations and the editing of the game are refused
    /// by [`update`](crate::update) and the opening name is hidden.
    pub fn assist_locked(&self) -> bool {
        self.online.assist_locked() || self.fics.assist_locked()
    }

    /// Whether the user may release the lock by hand: only without a
    /// network (spec D1332, D1362).
    pub fn can_remove_lock(&self) -> bool {
        self.online.can_remove_lock()
    }

    /// Whether a game that was in the editor before the online one can be
    /// put back.
    pub fn online_has_parked_game(&self) -> bool {
        self.flow.online.parked.is_some() && !self.online.phase.is_live()
    }

    fn editor_is_online(&self) -> bool {
        self.flow.online.generation == Some(self.session.generation())
    }

    /// The modes that refuse a seek or a challenge (`RejectReason::Busy`).
    /// The training modes (tactics, opening trainer, calculation exercise)
    /// are not among them: an online game ends them instead
    /// (`online_stop_everything`, D1931, D1962, D2309, D2322).
    fn online_busy(&self) -> bool {
        self.annotation_running()
            || self.play_live()
            || self.tournament_live()
            || self.training_playing()
            || self.fics.playing()
    }

    pub(crate) fn online_message(&mut self, message: OnlineMessage, effects: &mut Vec<Effect>) {
        let was_locked = self.assist_locked();
        self.online.marker_dir = self
            .flow
            .prefs_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let busy = self.online_busy();
        let outs = {
            let ctx = Ctx {
                busy,
                prefs: &self.prefs.online,
            };
            self.online.apply(message, &ctx)
        };
        for out in outs {
            self.online_out(out, effects);
        }
        if !was_locked && self.assist_locked() {
            self.online_close_assist(effects);
        }
    }

    /// The program quits: the streams are closed (the game, if any, goes on
    /// at the server and the marker stays, D1332).
    pub(crate) fn online_quit(&mut self, effects: &mut Vec<Effect>) {
        if self.online.phase != crate::online::OnlinePhase::Disconnected {
            effects.push(Effect::Online {
                command: crate::online::OnlineCommand::Shutdown,
                hub: self.online.hub.clone(),
            });
        }
    }

    fn online_out(&mut self, out: Out, effects: &mut Vec<Effect>) {
        match out {
            Out::Command(command) => effects.push(Effect::Online {
                command,
                hub: self.online.hub.clone(),
            }),
            Out::Notice(notice) => self.notice = Some(AppNotice::Online(notice)),
            Out::Login(login) => {
                if self.prefs.online.login != login {
                    let mut prefs = (*self.prefs.online).clone();
                    prefs.login = login;
                    self.prefs_message(
                        PrefsMessage::Set(PrefChange::Online(Box::new(prefs))),
                        effects,
                    );
                }
            }
            Out::Starting { at, .. } => self.online_stop_everything(at, effects),
            Out::Opened => {
                self.online_entries = self.online_entries.wrapping_add(1);
                self.online_open_game();
                self.sound_game_event(SoundCue::GameStart, effects);
            }
            Out::Changed => {
                self.online_refresh_game(effects);
            }
            Out::Finished => self.online_finish(effects),
            Out::RestorePrevious => self.online_restore_previous(),
            Out::Dismissed => {}
            Out::WatchOpened => {
                self.online_entries = self.online_entries.wrapping_add(1);
                self.online_open_watch();
            }
            Out::WatchChanged => self.online_refresh_watch(effects),
            Out::WatchClosed => self.online_close_watch(),
        }
    }

    /// Ends the game against an engine, the tournament, the annotation and
    /// the training modes, and every engine (D1331).
    pub(super) fn online_stop_everything(&mut self, at: Instant, effects: &mut Vec<Effect>) {
        if self.play_live() {
            self.play_message(crate::play::PlayMessage::Quit(at), effects);
        }
        if self.tournament_live() {
            self.tournament_message(crate::tournament::TournamentMessage::Abort(at), effects);
        }
        if self.annotation_running() {
            self.annotate_finish(crate::annotate::Outcome::Cancelled, true, effects);
        }
        self.training_stop_by_user(effects);
        // The training would otherwise park its problem as the game of the
        // user, and show the answers (D1944).
        self.tactics_stop(effects);
        self.online_close_assist(effects);
    }

    /// Stops the engines and closes the opening tree.
    pub(super) fn online_close_assist(&mut self, effects: &mut Vec<Effect>) {
        let runs: Vec<RunId> = self.engine_runs().iter().map(|r| r.id).collect();
        for run in runs {
            self.stop_run(run, effects);
        }
        self.tree_message(crate::tree::TreeMessage::Close, effects);
        self.eco_close_all();
    }

    /// Puts the game of the server in the editor (a dedicated session game,
    /// D1330); the previous one is parked.
    fn online_open_game(&mut self) {
        let Some(game) = self.online.game.as_ref() else {
            return;
        };
        let built = game.to_game();
        let flipped = game.my_color == Color::Black;
        if !self.editor_is_online() {
            self.flow.online.parked = Some(Parked {
                game: self.session.game().clone(),
                origin: self.origin.clone(),
                cursor: self.session.cursor(),
                dirty: self.session.is_dirty(),
            });
        }
        self.session.replace_game(built);
        self.flow.online.generation = Some(self.session.generation());
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        self.origin = GameOrigin::New;
        self.collection = None;
        self.board_flipped = flipped;
        self.setup = None;
        self.eco_close_all();
        self.info_dialog = None;
        self.chooser = None;
        self.discard_prompt = None;
    }

    /// Shows the moves the server confirmed (and the user's own), following
    /// the end of the game unless the user is looking at an earlier move.
    fn online_refresh_game(&mut self, effects: &mut Vec<Effect>) -> bool {
        let Some(game) = self.online.game.as_ref() else {
            return false;
        };
        if !self.editor_is_online() {
            return false;
        }
        let before = self.sound_mainline_len();
        let built = game.to_game();
        let cursor = self.session.cursor();
        let at_end = cursor == last_node(self.session.game());
        let ply = self
            .session
            .game()
            .mainline()
            .position(|node| node == cursor)
            .map_or(0, |i| i + 1);
        self.session.replace_game(built);
        self.flow.online.generation = Some(self.session.generation());
        let nav = if at_end {
            Nav::GameEnd
        } else {
            Nav::GoToMainlinePly(u32::try_from(ply).unwrap_or(u32::MAX))
        };
        drop(self.session.update(GameMessage::Navigate(nav)));
        self.sound_after_refresh(before, effects)
    }

    /// The game is over: shows the final game and stores it (D1335).
    fn online_finish(&mut self, effects: &mut Vec<Effect>) {
        if !self.online_refresh_game(effects) {
            self.sound_game_event(SoundCue::GameEnd, effects);
        }
        let Some(game) = self.online.game.as_ref() else {
            return;
        };
        let aborted = game
            .outcome()
            .is_some_and(|outcome| outcome.kind == EndKind::Aborted);
        if aborted {
            self.notice = Some(AppNotice::Online(OnlineNotice::NotImported));
            return;
        }
        let site = game.site();
        if self.flow.online.imported.contains(&site) {
            self.notice = Some(AppNotice::Online(OnlineNotice::AlreadyImported));
            return;
        }
        if self.prefs.online.save_to == SaveTo::Nowhere {
            return;
        }
        let built = game.to_game();
        let pushed = self.bases.slot_mut(SlotId::CLIPBASE).and_then(|slot| {
            let pushed = slot
                .source
                .lock()
                .clipbase_mut()
                .map(|clip| clip.push_game(&built));
            if matches!(pushed, Some(Ok(_))) {
                // The game enters the filter, like the "copy" of the
                // Clipbase does.
                slot.filter.append(1);
                slot.clear_sort();
            }
            pushed
        });
        match pushed {
            Some(Ok(_)) => {
                self.flow.online.imported.push(site);
                self.notice = Some(AppNotice::Online(OnlineNotice::Imported));
            }
            _ => self.notice = Some(AppNotice::Online(OnlineNotice::ImportFailed)),
        }
    }

    /// Puts the watched game in the editor (the previous one is parked, like
    /// for a game of the user, D3004).
    fn online_open_watch(&mut self) {
        let Some(built) = self
            .online
            .watch
            .as_ref()
            .and_then(|w| w.game.as_ref())
            .map(|w| w.game.clone())
        else {
            return;
        };
        if !self.editor_is_online() {
            self.flow.online.parked = Some(Parked {
                game: self.session.game().clone(),
                origin: self.origin.clone(),
                cursor: self.session.cursor(),
                dirty: self.session.is_dirty(),
            });
        }
        self.flow.online.watch_shown = Some(built.clone());
        self.session.replace_game(built);
        self.flow.online.generation = Some(self.session.generation());
        drop(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        self.origin = GameOrigin::New;
        self.collection = None;
        self.board_flipped = false;
        self.setup = None;
        self.info_dialog = None;
        self.chooser = None;
        self.discard_prompt = None;
    }

    /// Shows the new moves of the watched game, following the end unless
    /// the user looks at an earlier move. If the user changed the game in
    /// the editor (a note, a variation) the updates stop for that game.
    fn online_refresh_watch(&mut self, effects: &mut Vec<Effect>) {
        let Some(built) = self
            .online
            .watch
            .as_ref()
            .and_then(|w| w.game.as_ref())
            .map(|w| w.game.clone())
        else {
            return;
        };
        if !self.editor_is_online() {
            return;
        }
        let untouched = self
            .flow
            .online
            .watch_shown
            .as_ref()
            .is_some_and(|shown| self.session.game().same_content(shown));
        if !untouched {
            return;
        }
        let before = self.sound_mainline_len();
        let cursor = self.session.cursor();
        let at_end = cursor == last_node(self.session.game());
        let ply = self
            .session
            .game()
            .mainline()
            .position(|node| node == cursor)
            .map_or(0, |i| i + 1);
        self.flow.online.watch_shown = Some(built.clone());
        self.session.replace_game(built);
        self.flow.online.generation = Some(self.session.generation());
        let nav = if at_end {
            Nav::GameEnd
        } else {
            Nav::GoToMainlinePly(u32::try_from(ply).unwrap_or(u32::MAX))
        };
        drop(self.session.update(GameMessage::Navigate(nav)));
        self.sound_after_refresh(before, effects);
    }

    /// The watch ended: the game of before comes back, unless the user put
    /// something else in the editor meanwhile.
    fn online_close_watch(&mut self) {
        self.flow.online.watch_shown = None;
        if self.editor_is_online() && self.flow.online.parked.is_some() {
            self.online_restore_previous();
        }
    }

    fn online_restore_previous(&mut self) {
        let Some(parked) = self.flow.online.parked.take() else {
            return;
        };
        self.flow.online.generation = None;
        self.session.replace_game(parked.game);
        drop(
            self.session
                .update(GameMessage::Navigate(Nav::GoToNode(parked.cursor))),
        );
        if parked.dirty {
            self.session.mark_altered();
        }
        self.origin = parked.origin;
        self.board_flipped = false;
        self.notice = Some(AppNotice::Online(OnlineNotice::PreviousRestored));
    }
}
