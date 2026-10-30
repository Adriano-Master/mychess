//! The FICS play in the state (spec M8-9b).
//!
//! The rules are in [`crate::fics`] (a pure state machine); this is the
//! glue, the twin of `online_flow`: it runs what the machine asks for
//! (effects, stopping everything assistive, putting the game in the editor
//! and storing it at the end). The lock itself is
//! [`AppState::assist_locked`], shared with the Lichess play. Nothing here
//! blocks.

use mychess_core::{Game, NodeId};

use super::AppState;
use crate::bases::SlotId;
use crate::effect::Effect;
use crate::fics::{FicsCommand, FicsGame, FicsMessage, FicsNotice, FicsState, Out, Role};
use crate::files::GameOrigin;
use crate::flow::AppNotice;
use crate::online::last_node;
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, Nav};
use crate::sound::SoundCue;

/// The game the editor held before the FICS game took its place.
#[derive(Debug, Clone)]
struct Parked {
    game: Game,
    origin: GameOrigin,
    cursor: NodeId,
    dirty: bool,
}

/// What the glue remembers between messages.
#[derive(Debug, Default)]
pub(crate) struct FicsFlow {
    parked: Option<Parked>,
    /// The generation of the session game that holds the FICS game: any
    /// other generation means the user put something else in the editor,
    /// which must not be overwritten.
    generation: Option<u64>,
    /// The keys of the games stored in this session (D2624).
    imported: Vec<String>,
}

impl AppState {
    /// The FICS play.
    pub fn fics(&self) -> &FicsState {
        &self.fics
    }

    /// Whether a game that was in the editor before the FICS one can be
    /// put back.
    pub fn fics_has_parked_game(&self) -> bool {
        self.flow.fics.parked.is_some() && !self.fics.playing()
    }

    fn fics_editor_is_ours(&self) -> bool {
        self.flow.fics.generation == Some(self.session.generation())
    }

    /// Whether another play (engine, tournament, annotation, training or a
    /// Lichess game) keeps the user from starting a game here (D2613).
    fn fics_blocked(&self) -> bool {
        self.annotation_running()
            || self.play_live()
            || self.tournament_live()
            || self.training_playing()
            || self.online.phase.is_live()
    }

    pub(crate) fn fics_message(&mut self, message: FicsMessage, effects: &mut Vec<Effect>) {
        let starts_game = matches!(
            message,
            FicsMessage::Seek(_)
                | FicsMessage::Challenge { .. }
                | FicsMessage::Accept(_)
                | FicsMessage::PlaySeek(_)
                | FicsMessage::Examine(_)
                // It swaps the game of the editor (D2697).
                | FicsMessage::Show(_)
        );
        if starts_game && self.fics_blocked() {
            self.notice = Some(AppNotice::Fics(FicsNotice::WrongState));
            return;
        }
        let was_locked = self.assist_locked();
        let connecting = matches!(message, FicsMessage::Connect(_));
        let outs = self.fics.apply(message);
        if connecting
            && !outs
                .iter()
                .any(|o| matches!(o, Out::Command(FicsCommand::Connect { .. })))
        {
            // A refused connect must not leave a password staged (D2698).
            self.fics.hub.clear_staged();
        }
        for out in outs {
            self.fics_out(out, effects);
        }
        if !was_locked && self.assist_locked() {
            self.online_close_assist(effects);
        }
        // The settings the machine changed (a seek, a connection) are kept.
        if self.fics.prefs != *self.prefs.fics {
            let prefs = Box::new(self.fics.prefs.clone());
            self.prefs_message(PrefsMessage::Set(PrefChange::Fics(prefs)), effects);
        }
    }

    /// The preferences were read: the machine uses the `[fics]` table.
    pub(crate) fn fics_prefs_loaded(&mut self) {
        self.fics.prefs = (*self.prefs.fics).clone();
    }

    /// The program quits: the connection is closed (a game of the user
    /// goes on at the server, the way the server decides).
    pub(crate) fn fics_quit(&mut self, effects: &mut Vec<Effect>) {
        if self.fics.is_connected() {
            effects.push(Effect::Fics {
                command: FicsCommand::Disconnect {
                    quit: true,
                    epoch: self.fics.epoch(),
                },
                hub: self.fics.hub.clone(),
            });
        }
    }

    fn fics_out(&mut self, out: Out, effects: &mut Vec<Effect>) {
        match out {
            Out::Command(command) => effects.push(Effect::Fics {
                command,
                hub: self.fics.hub.clone(),
            }),
            Out::Notice(notice) => self.notice = Some(AppNotice::Fics(notice)),
            Out::Starting(at) => self.online_stop_everything(at, effects),
            Out::Opened => {
                self.online_entries = self.online_entries.wrapping_add(1);
                self.fics_open_game();
                if self
                    .fics
                    .game
                    .as_ref()
                    .is_some_and(|g| g.role == Role::Playing)
                {
                    self.sound_game_event(SoundCue::GameStart, effects);
                }
            }
            Out::Changed => {
                self.fics_refresh_game(effects);
            }
            Out::Finished => self.fics_finish(effects),
            Out::Closed => self.fics_restore_previous(),
        }
    }

    fn fics_built(&self, game: &FicsGame) -> Game {
        game.to_game(self.fics.prefs.store_emt, self.fics.prefs.store_clk)
    }

    /// Puts the game of the server in the editor (a dedicated session game,
    /// D2622); the previous one is parked.
    fn fics_open_game(&mut self) {
        let Some(game) = self.fics.editor_game() else {
            return;
        };
        let built = self.fics_built(game);
        let flipped = game
            .my_color
            .map_or(game.flipped, |c| c == mychess_core::Color::Black);
        if !self.fics_editor_is_ours() {
            self.flow.fics.parked = Some(Parked {
                game: self.session.game().clone(),
                origin: self.origin.clone(),
                cursor: self.session.cursor(),
                dirty: self.session.is_dirty(),
            });
        }
        self.session.replace_game(built);
        self.flow.fics.generation = Some(self.session.generation());
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
    fn fics_refresh_game(&mut self, effects: &mut Vec<Effect>) -> bool {
        if !self.fics_editor_is_ours() {
            return false;
        }
        let Some(game) = self.fics.editor_game() else {
            return false;
        };
        let before = self.sound_mainline_len();
        let built = self.fics_built(game);
        let cursor = self.session.cursor();
        let at_end = cursor == last_node(self.session.game());
        let ply = self
            .session
            .game()
            .mainline()
            .position(|node| node == cursor)
            .map_or(0, |i| i + 1);
        self.session.replace_game(built);
        self.flow.fics.generation = Some(self.session.generation());
        let nav = if at_end {
            Nav::GameEnd
        } else {
            Nav::GoToMainlinePly(u32::try_from(ply).unwrap_or(u32::MAX))
        };
        drop(self.session.update(GameMessage::Navigate(nav)));
        self.sound_after_refresh(before, effects)
    }

    /// The game of the user is over: shows the final game and stores it in
    /// the Clipbase (D2624).
    fn fics_finish(&mut self, effects: &mut Vec<Effect>) {
        if !self.fics_refresh_game(effects) {
            self.sound_game_event(SoundCue::GameEnd, effects);
        }
        let Some(game) = self.fics.game.as_ref() else {
            return;
        };
        if game.role != Role::Playing {
            return;
        }
        if !game.is_standard() {
            // A variant is not stored: the core does not know its rules.
            self.notice = Some(AppNotice::Fics(FicsNotice::NotImported));
            return;
        }
        let unfinished = game
            .result
            .as_ref()
            .is_none_or(|(r, _)| *r == mychess_online::fics::FicsResult::Unfinished);
        if unfinished {
            self.notice = Some(AppNotice::Fics(FicsNotice::NotImported));
            return;
        }
        let key = game.import_key();
        if self.flow.fics.imported.contains(&key) {
            self.notice = Some(AppNotice::Fics(FicsNotice::AlreadyImported));
            return;
        }
        let built = self.fics_built(game);
        let pushed = self.bases.slot_mut(SlotId::CLIPBASE).and_then(|slot| {
            let pushed = slot
                .source
                .lock()
                .clipbase_mut()
                .map(|clip| clip.push_game(&built));
            if matches!(pushed, Some(Ok(_))) {
                slot.filter.append(1);
                slot.clear_sort();
            }
            pushed
        });
        match pushed {
            Some(Ok(_)) => {
                self.flow.fics.imported.push(key);
                self.notice = Some(AppNotice::Fics(FicsNotice::Imported));
            }
            _ => self.notice = Some(AppNotice::Fics(FicsNotice::ImportFailed)),
        }
    }

    fn fics_restore_previous(&mut self) {
        if !self.fics_editor_is_ours() {
            self.flow.fics.generation = None;
            self.flow.fics.parked = None;
            return;
        }
        let Some(parked) = self.flow.fics.parked.take() else {
            // Nothing to put back: the game of the server must not stay
            // in the editor (D2695).
            self.flow.fics.generation = None;
            self.session.replace_game(Game::new());
            self.origin = GameOrigin::New;
            self.board_flipped = false;
            return;
        };
        self.flow.fics.generation = None;
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
    }
}
