//! Replacing the game being edited, with the "save changes?" question
//! (spec M2-2, D103-D105, D113).

use mychess_core::{Game, Position, Tags};

use super::{AppState, PendingPick, PendingSave, Protected, SaveCheck};
use crate::effect::{Effect, SaveGuard};
use crate::files::{GameOrigin, SaveMode, ensure_pgn_extension, suggest_file_name};
use crate::flow::{
    AppNotice, DiscardChoice, DiscardPrompt, OverwritePrompt, OverwriteReason, Pending, Replacement,
};
use crate::host::{PickKind, PickSpec};
use crate::shared::SharedGame;

impl AppState {
    /// Whether replacing the game must ask first: it was altered, is not a
    /// trial, and is not just an empty game with default tags (D103).
    pub(crate) fn needs_confirmation(&self) -> bool {
        self.session.is_dirty()
            && !self.session.in_trial()
            && (self.session.game().mainline().next().is_some()
                || *self.session.game().tags() != Tags::default())
    }

    /// Starts a replacement: asks first when changes would be lost.
    pub(crate) fn begin_replace(&mut self, replacement: Replacement, effects: &mut Vec<Effect>) {
        self.proceed_or_ask(Pending::Replace(replacement), effects);
    }

    pub(crate) fn proceed_or_ask(&mut self, pending: Pending, effects: &mut Vec<Effect>) {
        // Replacing the game or closing its base while the annotation runs
        // would pull the game from under it (D897); a quit cancels the run
        // first instead.
        if !matches!(pending, Pending::Quit) && self.refuse_edit() {
            return;
        }
        if self.discard_prompt.is_some() {
            // A question is open (for instance "save before quitting?"): a
            // late file, parse or clipboard answer must not replace what it
            // will do. The user can ask again afterwards.
            return;
        }
        // A session of the tactics training holds the game of the user
        // aside, perhaps with unsaved changes (D2145), and shows a problem
        // that is marked as saved: the question below would not see what is
        // at stake. The session ends first and the game of the user comes
        // back as it was, so the normal question protects it (D2158).
        if !matches!(pending, Pending::TacticsStart(_)) {
            self.tactics_halt();
        }
        if self.needs_confirmation() {
            self.discard_prompt = Some(DiscardPrompt { pending });
        } else {
            self.proceed(pending, effects);
        }
    }

    /// Carries out what the user confirmed.
    pub(crate) fn proceed(&mut self, pending: Pending, effects: &mut Vec<Effect>) {
        if !matches!(pending, Pending::Quit) && self.refuse_edit() {
            return;
        }
        match pending {
            Pending::Replace(replacement) => self.apply_replacement(replacement),
            Pending::Quit => self.continue_quit(effects),
            Pending::CloseBase(slot) => self.close_slot_in_flow(slot, effects),
            Pending::TacticsStart(request) => {
                self.tactics_start_confirmed(request.config, request.seed, effects);
            }
            Pending::CalvarStart(request) => {
                self.calvar_start_confirmed(*request, effects);
            }
            Pending::OpeningStart(request) => {
                self.opening_start_confirmed(request.config, request.seed, request.today, effects);
            }
            Pending::Maintenance => self.maintenance_confirmed(effects),
        }
    }

    /// Puts a new game in place of the current one and closes the dialogs
    /// that led to it.
    pub(crate) fn apply_replacement(&mut self, replacement: Replacement) {
        let mut from_base = None;
        let mut landing = None;
        // Set only when the replacement really happens: a refused or a
        // cancelled one leaves the board waiting for its start.
        let board_start = match &replacement {
            Replacement::Board { position, .. } => Some(position.is_some()),
            _ => None,
        };
        // "Go to this position" of the search or the outline of a mask: the
        // dialog it came from closes only now, when the game was replaced
        // (a cancelled question or a refusal leaves it open).
        if matches!(replacement, Replacement::FromPosition(_)) {
            let masks = &mut self.tree.mask;
            masks.search = None;
            masks.searching = None;
            masks.outline = None;
            masks.outlining = None;
        }
        let (game, origin, dirty, collection) = match replacement {
            Replacement::New => (Game::new(), GameOrigin::New, false, None),
            Replacement::FromPosition(position) => {
                (game_from_position(position), GameOrigin::New, false, None)
            }
            Replacement::Board { position, event } => {
                let mut game = position.map_or_else(Game::new, game_from_position);
                event.clone_into(&mut game.tags_mut().event);
                (game, GameOrigin::New, false, None)
            }
            Replacement::Loaded {
                game,
                origin,
                dirty,
                collection,
            } => (game.to_game(), origin, dirty, collection),
            Replacement::FromBase {
                slot,
                number,
                game,
                origin,
                landing: browser_landing,
            } => {
                from_base = Some((slot, number));
                landing = browser_landing;
                (game.to_game(), origin, false, None)
            }
        };
        let old_tags = self.session.game().tags();
        if *old_tags != Tags::default() {
            self.last_tags = Some(old_tags.clone());
        }
        self.names.observe_tags(game.tags());
        self.session.replace_game(game);
        if dirty {
            self.session.mark_altered();
        }
        self.collection = match origin {
            GameOrigin::PgnFile { .. } => collection,
            _ => None,
        };
        self.origin = origin;
        // A record of an EPD file: the side to move is at the bottom (spec
        // M7-5, regra 8, D3808).
        if matches!(self.origin, GameOrigin::EpdRecord { .. })
            && self.prefs.epd.flip_to_side_to_move
        {
            self.board_flipped = self.session.position().turn() == mychess_core::Color::Black;
        }
        self.note_current_game(from_base);
        if let Some(landing) = landing {
            self.land_browser_game(landing);
        }
        if let Some(with_position) = board_start {
            self.eboard_game_started(with_position);
        }
        self.pasted_moves = None;
        self.setup = None;
        self.info_dialog = None;
        self.chooser = None;
        self.discard_prompt = None;
        let clean = self
            .import
            .as_ref()
            .and_then(|d| d.report.as_ref())
            .is_some_and(|r| !r.has_problems());
        if clean {
            self.import = None;
        }
    }

    pub(crate) fn answer_discard(&mut self, choice: DiscardChoice, effects: &mut Vec<Effect>) {
        let Some(prompt) = self.discard_prompt.take() else {
            return;
        };
        match choice {
            DiscardChoice::Cancel => {
                self.flow.quit_asked.clear();
                if matches!(prompt.pending, Pending::Maintenance) {
                    self.maintenance_give_up();
                }
                if matches!(prompt.pending, Pending::Quit) {
                    self.tournament_quit_undone(effects);
                    self.spelling_quit_undone(effects);
                }
            }
            DiscardChoice::DontSave => self.proceed(prompt.pending, effects),
            DiscardChoice::Save => {
                if let Some(save) = self.flow.save.as_mut() {
                    // A save is already being written: let it carry on with
                    // this when it ends, if nothing else waits for it.
                    if save.then.is_none() {
                        save.then = Some(prompt.pending);
                    } else {
                        self.notice = Some(AppNotice::SaveInProgress);
                        self.discard_prompt = Some(prompt);
                    }
                    return;
                }
                self.save_then(prompt.pending, effects);
            }
        }
    }

    /// "Save", then carry on with `pending` (nothing happens if saving
    /// fails or is cancelled).
    fn save_then(&mut self, pending: Pending, effects: &mut Vec<Effect>) {
        if let GameOrigin::SavedFile { path } = &self.origin {
            let path = path.clone();
            self.start_save(
                path,
                SaveMode::Overwrite,
                Some(pending),
                SaveCheck::default(),
                effects,
            );
        } else {
            self.ask_for_file(PickKind::SavePgn, Some(pending), effects);
        }
    }

    /// Shows the file dialog of `kind`.
    pub(crate) fn ask_for_file(
        &mut self,
        kind: PickKind,
        then: Option<Pending>,
        effects: &mut Vec<Effect>,
    ) {
        let request = self.new_request();
        self.flow.pick = Some(PendingPick {
            request,
            kind,
            then,
        });
        let file_name = matches!(kind, PickKind::SavePgn | PickKind::AppendPgn)
            .then(|| suggest_file_name(self.session.game().tags()));
        let initial_dir = match kind {
            PickKind::SavePgn | PickKind::AppendPgn => self.prefs.files.last_save_dir.clone(),
            _ => self.prefs.files.last_open_dir.clone(),
        };
        effects.push(Effect::PickFile(PickSpec {
            request,
            kind,
            initial_dir,
            file_name,
        }));
    }

    /// Asks the effect runner to write the game. Only one write is in
    /// flight: a second request is refused with
    /// [`AppNotice::SaveInProgress`], so the answer of the first can never
    /// be mistaken for the second (and keeps its error and its `then`).
    pub(crate) fn start_save(
        &mut self,
        path: std::path::PathBuf,
        mode: SaveMode,
        then: Option<Pending>,
        check: SaveCheck,
        effects: &mut Vec<Effect>,
    ) {
        if self.flow.save.is_some() {
            self.notice = Some(AppNotice::SaveInProgress);
            return;
        }
        // The file must not hold trial moves (see `GameSession::mark_saved`).
        if self.session.in_trial() {
            drop(self.session.update(crate::session::GameMessage::ExitTrial));
        }
        let request = self.new_request();
        self.flow.save = Some(PendingSave {
            request,
            revision: self.session.revision(),
            then,
            path: path.clone(),
            check: check.clone(),
        });
        effects.push(Effect::SavePgn {
            request,
            path,
            mode,
            game: SharedGame::new(self.session.game().clone()),
            options: self.prefs.pgn_export.clone(),
            guard: SaveGuard {
                create_only: check.create_only,
                protect: check.protected.map(|p| p.path),
            },
        });
    }

    pub(crate) fn answer_overwrite(&mut self, yes: bool, effects: &mut Vec<Effect>) {
        let Some(prompt) = self.overwrite_prompt.take() else {
            return;
        };
        if !yes && prompt.then == Some(Pending::Quit) {
            // The quit is given up (D3505).
            self.spelling_quit_undone(effects);
        }
        if !yes && prompt.then == Some(Pending::Maintenance) {
            // The operation that waited for the save is dropped (D4125).
            self.maintenance_give_up();
        }
        if yes {
            let check = SaveCheck::default();
            self.start_save(
                prompt.path,
                SaveMode::Overwrite,
                prompt.then,
                check,
                effects,
            );
        }
    }

    /// A save destination was chosen.
    pub(crate) fn save_destination(
        &mut self,
        path: std::path::PathBuf,
        mode: SaveMode,
        then: Option<Pending>,
        effects: &mut Vec<Effect>,
    ) {
        let chosen = path;
        let path = ensure_pgn_extension(chosen.clone());
        // The dialog confirmed `chosen`, not the name with the extension
        // added: `perform` checks that nothing is replaced silently.
        let mut check = SaveCheck {
            create_only: mode == SaveMode::Overwrite && path != chosen,
            protected: None,
        };
        if mode == SaveMode::Overwrite
            && let GameOrigin::PgnFile {
                path: open,
                total,
                partial,
                ..
            } = &self.origin
            && (*total > 1 || *partial)
        {
            let reason = if *partial {
                OverwriteReason::PartiallyRead
            } else {
                OverwriteReason::MultiGame
            };
            if *open == path {
                self.overwrite_prompt = Some(OverwritePrompt {
                    path,
                    games: *total,
                    reason,
                    then,
                });
                return;
            }
            // Another spelling of the same file (links, case, `..`) is
            // caught by `perform`, which can look at the file system.
            check.protected = Some(Protected {
                path: open.clone(),
                games: *total,
                reason,
            });
        }
        self.start_save(path, mode, then, check, effects);
    }

    /// The setup dialog: opens it on the position at the cursor.
    pub(crate) fn open_setup(&mut self) {
        // A Chess960 game opens the editor in Chess960 mode (M9-6).
        let position = self.session.position();
        let mut board = crate::setup::SetupBoard::from_position(position);
        board.fen_history.clone_from(&self.setup_history);
        self.setup = Some(board);
    }
}

/// A game that starts from `position`; the standard start is a plain new
/// game, without a `FEN` tag (Scid: `exitSetupBoard`).
pub(super) fn game_from_position(position: Position) -> Game {
    // Position 518 equals the standard start but plays in Chess960 mode and
    // must keep its `Variant` (M9-6, D4502).
    let standard = position == Position::new()
        && position.castling_mode() == mychess_core::CastlingMode::Standard
        && position.halfmove_clock() == 0
        && position.fullmove_number() == 1;
    if standard {
        Game::new()
    } else {
        Game::from_position(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_start_is_a_plain_game() {
        let game = game_from_position(Position::new());
        assert_eq!(game.start_position(), &Position::new());
        let moved = Position::from_fen("8/8/8/8/8/8/8/K6k w - - 0 1").expect("fen");
        let game = game_from_position(moved.clone());
        assert_eq!(game.start_position(), &moved);
    }
}
