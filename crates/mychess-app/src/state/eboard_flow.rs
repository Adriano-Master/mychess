//! The glue of the electronic board with [`AppState`] (spec M8-10): the
//! connection, the moves of the board played in the game, the position, the
//! result, and the moves of the user sent to the board.
//!
//! The protocols and the sessions are in `mychess-eboard`; the state shown by
//! the dialog is [`EboardState`]. Nothing here blocks.

use mychess_core::{Color, GameResult, NodeId, Position};
use mychess_eboard::{
    BoardKind, BoardMove, EboardCommand, EboardEvent, Note, Reason, Warning, placement_matches,
};

use super::AppState;
use crate::eboard::{
    BoardChoice, ConsoleNote, EboardEffect, EboardMessage, EboardNotice, EboardPhase, EboardPrefs,
    EboardState, LastMove, MoveOutcome, UserCommand,
};
use crate::effect::Effect;
use crate::flow::{AppNotice, Replacement};
use crate::message::Message;
use crate::session::{GameMessage, Nav, PlayOptions};

impl AppState {
    /// The state of the electronic board, for the dialog.
    #[must_use]
    pub fn eboard(&self) -> &EboardState {
        &self.flow.eboard
    }

    fn eboard_notice(&mut self, notice: EboardNotice) {
        self.notice = Some(AppNotice::Eboard(notice));
    }

    fn eboard_send(&self, command: EboardCommand, effects: &mut Vec<Effect>) {
        effects.push(Effect::Eboard(EboardEffect::Send {
            command,
            hub: self.flow.eboard.hub.clone(),
        }));
    }

    /// Handles an [`EboardMessage`].
    pub(crate) fn eboard_message(&mut self, message: EboardMessage, effects: &mut Vec<Effect>) {
        match message {
            EboardMessage::Connect(prefs) => self.eboard_connect(*prefs, effects),
            EboardMessage::Disconnect => self.eboard_disconnect(effects),
            EboardMessage::Command(command) => self.eboard_user_command(command, effects),
            EboardMessage::ClearConsole => self.flow.eboard.console.clear(),
            EboardMessage::RefreshPorts => effects.push(Effect::Eboard(EboardEffect::ListPorts)),
            EboardMessage::Event { epoch, event } => {
                if epoch == self.flow.eboard.epoch {
                    self.eboard_event(event, effects);
                }
            }
            EboardMessage::Connected { epoch } => {
                // An attempt that was cancelled meanwhile: close what it opened.
                if epoch != self.flow.eboard.epoch || self.flow.eboard.phase == EboardPhase::Idle {
                    effects.push(Effect::Eboard(EboardEffect::Disconnect {
                        epoch,
                        hub: self.flow.eboard.hub.clone(),
                    }));
                }
            }
            EboardMessage::ConnectFailed { epoch, error } => {
                if epoch == self.flow.eboard.epoch && self.flow.eboard.phase != EboardPhase::Idle {
                    self.flow.eboard.phase = EboardPhase::Idle;
                    let notice = EboardNotice::ConnectFailed(error);
                    self.flow.eboard.issue = Some(notice.clone());
                    self.eboard_notice(notice);
                }
            }
            EboardMessage::PortsListed(result) => match result {
                Ok(ports) => {
                    self.flow.eboard.ports = ports;
                    self.flow.eboard.ports_error = None;
                }
                Err(error) => {
                    self.flow.eboard.ports.clear();
                    self.flow.eboard.ports_error = Some(error);
                }
            },
            EboardMessage::Ack => {}
        }
    }

    fn eboard_connect(&mut self, prefs: EboardPrefs, effects: &mut Vec<Effect>) {
        if self.flow.eboard.phase != EboardPhase::Idle {
            return;
        }
        let prefs = prefs.sanitized();
        let kind = match prefs.validate() {
            Ok(kind) => kind,
            Err(issue) => {
                let notice = EboardNotice::BadSettings(issue);
                self.flow.eboard.issue = Some(notice.clone());
                self.eboard_notice(notice);
                return;
            }
        };
        let choice = prefs.kind;
        if *self.prefs.eboard != prefs {
            *self.prefs.eboard = prefs;
            self.request_prefs_save(effects);
        }
        let state = &mut self.flow.eboard;
        state.reset_session();
        state.epoch += 1;
        state.phase = EboardPhase::Connecting;
        state.connected = choice;
        state.filter = self.prefs.eboard.send_filter;
        // A driver tells the position of the board first (D2730).
        state.awaiting_start = matches!(kind, BoardKind::InputEngine { .. });
        effects.push(Effect::Eboard(EboardEffect::Connect {
            epoch: state.epoch,
            kind,
            hub: state.hub.clone(),
        }));
    }

    fn eboard_disconnect(&mut self, effects: &mut Vec<Effect>) {
        let state = &mut self.flow.eboard;
        if state.phase == EboardPhase::Idle {
            return;
        }
        let epoch = state.epoch;
        // Events of this connection that are still on their way are dropped.
        state.epoch += 1;
        state.phase = EboardPhase::Idle;
        state.note(ConsoleNote::Disconnected, String::new());
        effects.push(Effect::Eboard(EboardEffect::Disconnect {
            epoch,
            hub: state.hub.clone(),
        }));
        self.eboard_notice(EboardNotice::Disconnected);
    }

    /// Ends the session when the program quits.
    pub(crate) fn eboard_quit(&mut self, effects: &mut Vec<Effect>) {
        self.eboard_disconnect(effects);
    }

    fn eboard_user_command(&mut self, command: UserCommand, effects: &mut Vec<Effect>) {
        if self.flow.eboard.phase != EboardPhase::Connected {
            return;
        }
        match command {
            UserCommand::Synchronise => {
                self.flow.eboard.awaiting_start = true;
                self.eboard_send(EboardCommand::Query, effects);
            }
            UserCommand::Rotate => {
                self.flow.eboard.awaiting_start = true;
                self.eboard_send(EboardCommand::Rotate, effects);
                self.eboard_send(EboardCommand::Query, effects);
            }
            UserCommand::Info => self.eboard_send(EboardCommand::SystemInfo, effects),
            UserCommand::Filter(filter) => {
                self.flow.eboard.filter = filter;
                if self.prefs.eboard.send_filter != filter {
                    self.prefs.eboard.send_filter = filter;
                    self.request_prefs_save(effects);
                }
                self.eboard_send(EboardCommand::SendFilter(filter.to_filter()), effects);
            }
            UserCommand::Raw(text) => self.eboard_send(EboardCommand::Raw(text), effects),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the events reads best
    fn eboard_event(&mut self, event: EboardEvent, effects: &mut Vec<Effect>) {
        match event {
            EboardEvent::Ready => {
                self.flow.eboard.phase = EboardPhase::Connected;
                self.flow.eboard.issue = None;
                self.eboard_notice(EboardNotice::Connected);
                // The driver starts with the filter of the user.
                if self.flow.eboard.connected == BoardChoice::InputEngine
                    && self.flow.eboard.filter != crate::eboard::FilterChoice::Both
                {
                    let filter = self.flow.eboard.filter.to_filter();
                    self.eboard_send(EboardCommand::SendFilter(filter), effects);
                }
            }
            EboardEvent::Log { direction, text } => self.flow.eboard.log(direction, text),
            EboardEvent::Warning(Warning::NoReply) => self.eboard_notice(EboardNotice::NoReply),
            EboardEvent::Warning(Warning::Refused(text)) => {
                self.flow.eboard.note(ConsoleNote::Refused, text);
            }
            EboardEvent::Note(note) => {
                let (note, text) = match note {
                    Note::UnreadableMove(text) => (ConsoleNote::UnreadableMove, text),
                    Note::InputEngineOnly => (ConsoleNote::InputEngineOnly, String::new()),
                    Note::DriverStderr(text) => (ConsoleNote::DriverStderr, text),
                    Note::LinesOmitted(count) => (ConsoleNote::LinesOmitted, count.to_string()),
                    _ => return,
                };
                self.flow.eboard.note(note, text);
            }
            EboardEvent::NewGame => {
                if self.flow.eboard.phase == EboardPhase::Connected {
                    let event = self.eboard_event_tag();
                    self.begin_replace(
                        Replacement::Board {
                            position: None,
                            event,
                        },
                        effects,
                    );
                }
            }
            EboardEvent::Move { mv, side } => self.eboard_board_move(mv, side, effects),
            // The board takes a move back only where the user could do it by
            // hand: not in an online or engine game, a training, a run (D2758).
            EboardEvent::TakeBack => {
                if self.flow.eboard.phase == EboardPhase::Connected && !self.edits_blocked() {
                    let mut played = super::update_body(
                        self,
                        Message::Game(GameMessage::Navigate(Nav::Back(1))),
                    );
                    effects.append(&mut played);
                }
            }
            EboardEvent::StartupFen => self.flow.eboard.awaiting_start = true,
            EboardEvent::Fen(fen) => self.eboard_fen(&fen, effects),
            EboardEvent::ToMove(color) => self.flow.eboard.to_move = Some(color),
            EboardEvent::NoClock => {
                self.flow.eboard.white_clock = None;
                self.flow.eboard.black_clock = None;
            }
            EboardEvent::Clock { color, seconds } => match color {
                Color::White => self.flow.eboard.white_clock = Some(seconds),
                Color::Black => self.flow.eboard.black_clock = Some(seconds),
            },
            EboardEvent::GameEnd(result) => self.eboard_game_end(result),
            EboardEvent::SetupMode(true) => {
                self.flow.eboard.note(ConsoleNote::SetupMode, String::new());
            }
            EboardEvent::SetupMode(false) => {
                // The player finished placing pieces: take the position.
                self.flow.eboard.awaiting_start = true;
                self.eboard_send(EboardCommand::Query, effects);
            }
            EboardEvent::BoardIdentified(text) => self.flow.eboard.board_name = Some(text),
            EboardEvent::DriverError(text) => {
                let notice = EboardNotice::DriverError(text);
                self.flow.eboard.issue = Some(notice.clone());
                self.eboard_notice(notice);
            }
            EboardEvent::Disconnected(reason) => self.eboard_disconnected(&reason),
            _ => {}
        }
    }

    fn eboard_disconnected(&mut self, reason: &Reason) {
        let state = &mut self.flow.eboard;
        if state.phase == EboardPhase::Idle {
            return;
        }
        state.phase = EboardPhase::Idle;
        let text = match reason {
            Reason::Requested => {
                self.eboard_notice(EboardNotice::Disconnected);
                return;
            }
            Reason::DeviceLost(message) | Reason::DriverError(message) => message.clone(),
            Reason::DriverExited => "the driver ended".to_owned(),
            Reason::HandshakeTimeout => "the driver did not answer in time".to_owned(),
            _ => "contact lost".to_owned(),
        };
        // A driver error already told its message.
        if matches!(reason, Reason::DriverError(_)) {
            return;
        }
        let notice = EboardNotice::ConnectionLost(text);
        state.issue = Some(notice.clone());
        self.eboard_notice(notice);
    }

    /// A move made on the board (D2714, D2728).
    fn eboard_board_move(&mut self, mv: BoardMove, side: Option<Color>, effects: &mut Vec<Effect>) {
        if self.flow.eboard.phase != EboardPhase::Connected {
            return;
        }
        let position = self.session.position().clone();
        // The echo of what we sent, or a move out of turn (D2714).
        if side.is_some_and(|side| side != position.turn()) {
            return;
        }
        let text = mv.to_string();
        let Ok(legal) = mv.resolve(&position) else {
            self.flow.eboard.last_move = Some(LastMove {
                text: text.clone(),
                outcome: MoveOutcome::Illegal,
            });
            self.flow
                .eboard
                .note(ConsoleNote::IllegalMove, text.clone());
            self.eboard_notice(EboardNotice::IllegalMove(text));
            return;
        };
        // The game belongs to someone else (the other side, the engine, a
        // run, a training): the board is not an entrance there (D2735,
        // D2758).
        if self.edits_blocked() {
            self.eboard_move_not_applied(text);
            return;
        }
        let before = self.session.cursor();
        self.flow.eboard.suppress_echo = true;
        let mut played = super::update_body(
            self,
            Message::Game(GameMessage::PlayMove {
                mv: legal,
                options: PlayOptions {
                    force_variation: true,
                },
            }),
        );
        self.flow.eboard.suppress_echo = false;
        effects.append(&mut played);
        if self.session.cursor() == before {
            self.eboard_move_not_applied(text);
            return;
        }
        self.flow.eboard.last_move = Some(LastMove {
            text,
            outcome: MoveOutcome::Played,
        });
        if self.flow.eboard.connected == BoardChoice::InputEngine {
            self.eboard_send(EboardCommand::Query, effects);
        }
    }

    /// A legal move of the board that the game did not take.
    fn eboard_move_not_applied(&mut self, text: String) {
        self.flow.eboard.last_move = Some(LastMove {
            text: text.clone(),
            outcome: MoveOutcome::NotApplied,
        });
        self.flow
            .eboard
            .note(ConsoleNote::MoveNotApplied, text.clone());
        self.eboard_notice(EboardNotice::MoveNotApplied(text));
    }

    /// The `Event` tag of a game the board starts (D2729).
    fn eboard_event_tag(&self) -> &'static str {
        match self.flow.eboard.connected {
            BoardChoice::InputEngine => "InputEngine Input",
            _ => "Novag Citrine",
        }
    }

    /// The game was replaced on the board's behalf: the start is taken
    /// (D2730). Called by the replacement itself, so a question that was
    /// cancelled or a refusal leaves the board waiting.
    pub(crate) fn eboard_game_started(&mut self, with_position: bool) {
        self.flow.eboard.awaiting_start = false;
        if with_position {
            self.flow.eboard.in_sync = Some(true);
        }
    }

    /// The position the driver sees (D2730).
    fn eboard_fen(&mut self, fen: &str, effects: &mut Vec<Effect>) {
        if self.flow.eboard.phase == EboardPhase::Idle {
            return;
        }
        if self.flow.eboard.awaiting_start {
            match Position::from_fen(fen) {
                // `awaiting_start` stays up until the game is replaced.
                Ok(position) => {
                    let event = self.eboard_event_tag();
                    self.begin_replace(
                        Replacement::Board {
                            position: Some(position),
                            event,
                        },
                        effects,
                    );
                }
                Err(_) => self.eboard_notice(EboardNotice::BadPosition),
            }
            return;
        }
        let same = placement_matches(fen, &self.session.position().fen());
        let was_in_sync = self.flow.eboard.in_sync != Some(false);
        self.flow.eboard.in_sync = Some(same);
        if !same && was_in_sync {
            self.eboard_notice(EboardNotice::OutOfSync);
        }
    }

    /// The board said the game ended (D2732): the result is written in the
    /// tags; the game is not added to any database.
    fn eboard_game_end(&mut self, result: GameResult) {
        // The result of a game someone else is playing, a run, a training
        // or a question in the way is not the board's to write (D2758).
        if self.refuse_edit() || self.modal_open() || self.eboard_trainer_holds_game() {
            return;
        }
        let edited = self.session.edit(|game, _| {
            game.tags_mut().result = result;
            Ok(())
        });
        if edited.is_ok() {
            self.eboard_notice(EboardNotice::ResultRecorded(result));
        }
    }

    /// Whether a tactics problem, an opening line, a calculation exercise or a review attempt holds
    /// the editor.
    fn eboard_trainer_holds_game(&self) -> bool {
        (self.flow.tactics.machine.locks_editor() && self.tactics_editor_is_ours())
            || (self.flow.opening.machine.locks_editor() && self.opening_editor_is_ours())
            || self.calvar_editor_locked()
            || self.review_trying_here()
    }

    /// Before a message of the game: the cursor, when the move the user
    /// makes on the screen has to be shown on the board (D2733).
    pub(crate) fn eboard_before_game_message(&self, message: &GameMessage) -> Option<NodeId> {
        let state = &self.flow.eboard;
        let wanted = state.phase == EboardPhase::Connected
            && state.connected == BoardChoice::Novag
            && !state.suppress_echo;
        (wanted && is_user_move_message(message)).then(|| self.session.cursor())
    }

    /// After a message of the game: if the user's move added or entered one
    /// move after `before`, it is sent to the board.
    pub(crate) fn eboard_after_game_message(
        &self,
        before: Option<NodeId>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(old) = before else {
            return;
        };
        let now = self.session.cursor();
        let game = self.session.game();
        if now == old || game.parent(now) != Ok(Some(old)) {
            return;
        }
        let (Ok(mv), Ok(position)) = (game.mv(now), game.position_at(old)) else {
            return;
        };
        if mv.is_null() {
            return;
        }
        self.eboard_send(
            EboardCommand::Move {
                uci: position.uci(mv),
                capture: mv.is_capture(),
            },
            effects,
        );
    }
}

/// The messages that make the user's move on the screen (the list of the
/// messages that play a move or answer a question about it).
fn is_user_move_message(message: &GameMessage) -> bool {
    matches!(
        message,
        GameMessage::PlayMove { .. }
            | GameMessage::PlaySquares { .. }
            | GameMessage::AnswerReplace(_)
            | GameMessage::AnswerPromotion(_)
            | GameMessage::MoveEntryCommit
            | GameMessage::EnterMoveText(_)
    )
}
