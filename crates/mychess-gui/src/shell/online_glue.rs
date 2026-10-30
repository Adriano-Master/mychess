//! The part of the shell that serves the online play on Lichess: the
//! events of its windows and panel, the moves and premoves of the board,
//! the clipboard and the tick of the clocks (spec M8-3c, D1470-D1499).

use std::time::Instant;

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::online::{OnlineMessage, Premove};

use super::{Message, Shell};
use crate::host;
use crate::widgets::board::interaction::BoardEvent;
use crate::widgets::board::premove::PremoveEvent;
use crate::windows::online_ui::{
    OnlineConfirm, OnlineUiMessage, TOKEN_PAGE, viewing_live_position,
};

impl Shell {
    /// An event of the windows or the panel of the online play.
    pub(super) fn update_online(&mut self, message: OnlineUiMessage) -> Task<Message> {
        match message {
            OnlineUiMessage::CopyLink => {
                return host::task(host::HostWork::WriteClipboard(TOKEN_PAGE.to_owned()));
            }
            OnlineUiMessage::CopyOAuthLink => {
                if let Some(url) = self.app.online.oauth.as_ref().and_then(|w| w.url.clone()) {
                    return host::task(host::HostWork::WriteClipboard(url));
                }
                return Task::none();
            }
            OnlineUiMessage::Premove(event) => return self.on_premove(event),
            OnlineUiMessage::Tick(at) => return self.on_online_tick(at),
            OnlineUiMessage::Confirm(true)
                if self.ui.dialogs.online.confirm == Some(OnlineConfirm::Quit) =>
            {
                self.ui.dialogs.online.confirm = None;
                return self.quit_after_books();
            }
            _ => {}
        }
        let out = self.ui.dialogs.online.update(message, &self.app);
        self.run_outcome(out, None)
    }

    /// A tick of the clocks of the online game: only the strips change.
    pub(super) fn on_online_tick(&mut self, at: Instant) -> Task<Message> {
        self.ui.dialogs.online.now = at;
        self.ui.dialogs.online.refresh_clocks(&self.app);
        Task::none()
    }

    /// A board event during an online game: the moves go to the game of the
    /// server, nothing else is edited (D1330).
    pub(super) fn on_online_board_event(&mut self, event: &BoardEvent) -> Task<Message> {
        let BoardEvent::PlayMove { mv, dragged } = event else {
            return Task::none();
        };
        let (Some(from), Some(to)) = (mv.from(), mv.to()) else {
            return Task::none();
        };
        // A move made while looking at an earlier position of the game
        // would not be a move of the game.
        if !viewing_live_position(&self.app) {
            return Task::none();
        }
        if *dragged {
            self.ui.suppress_next_animation();
        }
        self.dispatch(AppMessage::Online(OnlineMessage::PlayerMove {
            from,
            to,
            promotion: mv.promotion(),
            offer_draw: false,
            at: Instant::now(),
        }))
    }

    /// The premove entry of the board published something (D1392).
    fn on_premove(&mut self, event: PremoveEvent) -> Task<Message> {
        if self.app.fics().playing() {
            return self.on_fics_premove(event);
        }
        // Against an engine the premove is the game's own (D4452).
        if !self.app.online.phase.is_live() && self.app.play_live() {
            return self.on_play_premove(event);
        }
        match event {
            PremoveEvent::Set { from, to } => {
                let Some(game) = self.app.online.game.as_ref() else {
                    return Task::none();
                };
                let Some(premove) = Premove::enter(game.position(), game.my_color, from, to, None)
                else {
                    return Task::none();
                };
                // A pawn on the last rank asks for the piece (D3013).
                if premove.promotion.is_some() {
                    self.ui.dialogs.online.premove_promotion = Some((from, to));
                    return Task::none();
                }
                self.dispatch(AppMessage::Online(OnlineMessage::PlayerMove {
                    from,
                    to,
                    promotion: premove.promotion,
                    offer_draw: false,
                    at: Instant::now(),
                }))
            }
            PremoveEvent::Cancel => self.dispatch(AppMessage::Online(OnlineMessage::CancelPremove)),
            PremoveEvent::Redraw => Task::none(),
        }
    }

    /// The premove entry of the board during a game against an engine
    /// (D4452): the application keeps it and plays it when the engine
    /// answers. A pawn that reaches the last rank promotes to a queen.
    fn on_play_premove(&mut self, event: PremoveEvent) -> Task<Message> {
        match event {
            PremoveEvent::Set { from, to } => {
                self.dispatch(AppMessage::Play(mychess_app::PlayMessage::SetPremove {
                    from,
                    to,
                    promotion: None,
                }))
            }
            PremoveEvent::Cancel => {
                self.dispatch(AppMessage::Play(mychess_app::PlayMessage::CancelPremove))
            }
            PremoveEvent::Redraw => Task::none(),
        }
    }
}
