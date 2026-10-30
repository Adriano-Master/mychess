//! The part of the shell that serves the play on FICS: the events of its
//! window, the moves and premoves of the board and the tick of the clocks
//! (spec M8-9c, D2613-D2615).

use std::time::Instant;

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::fics::FicsMessage;
use mychess_app::online::Premove;

use super::{Message, Shell};
use crate::widgets::board::interaction::BoardEvent;
use crate::widgets::board::premove::PremoveEvent;
use crate::windows::fics_dialog::FicsUiMessage;
use crate::windows::online_ui::shown_is_current;

impl Shell {
    /// An event of the FICS window or of the board during a FICS game.
    pub(super) fn update_fics(&mut self, message: FicsUiMessage) -> Task<Message> {
        match message {
            FicsUiMessage::Open => return self.open_fics(),
            FicsUiMessage::Tick(at) => return self.on_fics_tick(at),
            FicsUiMessage::Premove(event) => return self.on_fics_premove(event),
            _ => {}
        }
        let out = self.ui.dialogs.fics.update(message, &self.app);
        self.run_outcome(out, None)
    }

    /// Opens the window with the forms filled from the settings.
    pub(super) fn open_fics(&mut self) -> Task<Message> {
        self.ui.dialogs.fics.load(&self.app.fics().prefs);
        self.dispatch(AppMessage::Fics(FicsMessage::OpenWindow))
    }

    /// A tick of the clocks of the game: only the strips change.
    pub(super) fn on_fics_tick(&mut self, at: Instant) -> Task<Message> {
        self.ui.dialogs.fics.now = at;
        self.ui.dialogs.fics.refresh_clocks(&self.app);
        // The watchdogs of the move and of the connection (D2692).
        if self.app.fics().needs_ticks() {
            return self.dispatch(AppMessage::Fics(FicsMessage::Tick(at)));
        }
        Task::none()
    }

    /// A board event during a game of the user on FICS: the move goes to
    /// the server, nothing else is edited (D2622).
    pub(super) fn on_fics_board_event(&mut self, event: &BoardEvent) -> Task<Message> {
        let BoardEvent::PlayMove { mv, dragged } = event else {
            return Task::none();
        };
        let (Some(from), Some(to)) = (mv.from(), mv.to()) else {
            return Task::none();
        };
        let current = self
            .app
            .fics()
            .game
            .as_ref()
            .is_some_and(|g| shown_is_current(&self.app, g.position().zobrist()));
        if !current {
            return Task::none();
        }
        if *dragged {
            self.ui.suppress_next_animation();
        }
        self.dispatch(AppMessage::Fics(FicsMessage::PlayerMove {
            from,
            to,
            promotion: mv.promotion(),
        }))
    }

    /// The premove entry of the board published something (D2615). A pawn
    /// that reaches the last rank is promoted as the settings say (a queen
    /// when they ask).
    pub(super) fn on_fics_premove(&mut self, event: PremoveEvent) -> Task<Message> {
        match event {
            PremoveEvent::Set { from, to } => {
                let Some(game) = self.app.fics().game.as_ref() else {
                    return Task::none();
                };
                let Some(color) = game.my_color else {
                    return Task::none();
                };
                if Premove::enter(game.position(), color, from, to, None).is_none() {
                    return Task::none();
                }
                self.dispatch(AppMessage::Fics(FicsMessage::PlayerMove {
                    from,
                    to,
                    promotion: None,
                }))
            }
            PremoveEvent::Cancel => self.dispatch(AppMessage::Fics(FicsMessage::CancelPremove)),
            PremoveEvent::Redraw => Task::none(),
        }
    }
}
