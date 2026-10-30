//! The state machine of the FICS play (spec m8-9, D2601-D2619): pure, no
//! I/O. [`FicsState::apply`] takes a message and says what to do in
//! [`Out`]; the glue in `state/fics_flow.rs` runs it.

use std::time::{Duration, Instant};

use mychess_core::{Move, Position};
use mychess_online::fics::{
    Command, Disconnect, FicsEvent, FicsMessage as Net, FicsResult, GameSummary, Relation, Style12,
};

use super::console::{Console, LineKind};
use super::game::{Applied, FicsGame, Role};
use super::hub::FicsHub;
use super::message::{
    ConnectRequest, FicsCommand, FicsMessage, FicsNotice, FicsPrefs, NetUpdate, Offer,
};
use super::seeks::{PLAYABLE_KINDS, SeekGraph, is_playable};
use crate::online::{Premove, PremoveSlot};

/// Most observed games at the same time (D2604).
pub const MAX_OBSERVED: usize = 8;
/// Most offers kept.
const MAX_OFFERS: usize = 16;
/// Most game summaries kept for the next `<12>`.
const MAX_META: usize = 16;
/// The channel of the lectures (D2616).
pub const LECTURE_CHANNEL: u32 = 24;
/// After this long without an answer to a move the board is asked again
/// (D2692).
pub(crate) const MOVE_REFRESH_AFTER: Duration = Duration::from_secs(3);
/// After this long the move is taken out (D2692).
pub(crate) const MOVE_UNDO_AFTER: Duration = Duration::from_secs(5);
/// A connection attempt that takes longer is given up (D2692, D2630). It
/// is a backstop for a stuck client, so it must outlast the client's own
/// connect and login timeouts (15 s + 60 s) and leave a margin.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);

/// Where the connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// No connection.
    Disconnected,
    /// Connecting or logging in.
    Connecting,
    /// Logged in.
    LoggedIn,
}

/// What the glue has to do after a message.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Out {
    /// Run a command against the hub.
    Command(FicsCommand),
    /// Show a remark.
    Notice(FicsNotice),
    /// A game of the user is about to be shown: stop the assistance (D2622).
    Starting(Instant),
    /// A game was opened: put it in the editor.
    Opened,
    /// The game shown changed.
    Changed,
    /// The game of the user is over: store it (D2624).
    Finished,
    /// The game shown is gone (examination ended, game forgotten).
    Closed,
}

/// The state of the FICS play.
#[derive(Debug)]
pub struct FicsState {
    /// Where the connection is.
    pub phase: Phase,
    /// The connection.
    pub hub: FicsHub,
    /// The handle the server gave.
    pub me: Option<String>,
    /// The console.
    pub console: Console,
    /// The seek graph (D2626).
    pub seeks: SeekGraph,
    /// Offers waiting for an answer.
    pub offers: Vec<Offer>,
    /// The game of the user (played or examined).
    pub game: Option<FicsGame>,
    /// Games the user watches, at most [`MAX_OBSERVED`].
    pub observed: Vec<FicsGame>,
    /// The observed game shown in the editor.
    pub shown: Option<u32>,
    /// The round trip of the last `ping`.
    pub latency: Option<Duration>,
    /// The ping the server reported.
    pub ping_ms: Option<u32>,
    /// The player followed.
    pub following: Option<String>,
    /// The channel of the lectures is joined.
    pub lectures: bool,
    /// The premove (D2615).
    pub premove: PremoveSlot,
    /// The settings.
    pub prefs: FicsPrefs,
    /// The FICS window is open.
    pub window_open: bool,
    pub(crate) epoch: u64,
    meta: Vec<GameSummary>,
    /// When the watchdog first saw the move of the user waiting.
    pending_since: Option<Instant>,
    /// The board was asked again for the move waiting.
    pending_refreshed: bool,
    /// The line of the move waiting, to tell its send failure from others.
    pending_line: Option<String>,
    /// When the watchdog first saw the connection opening.
    connecting_since: Option<Instant>,
}

impl Default for FicsState {
    fn default() -> FicsState {
        FicsState::new()
    }
}

impl FicsState {
    /// A state with no connection.
    #[must_use]
    pub fn new() -> FicsState {
        FicsState {
            phase: Phase::Disconnected,
            hub: FicsHub::new(),
            me: None,
            console: Console::new(),
            seeks: SeekGraph::new(),
            offers: Vec::new(),
            game: None,
            observed: Vec::new(),
            shown: None,
            latency: None,
            ping_ms: None,
            following: None,
            lectures: false,
            premove: PremoveSlot::new(),
            prefs: FicsPrefs::default(),
            window_open: false,
            epoch: 0,
            meta: Vec::new(),
            pending_since: None,
            pending_refreshed: false,
            pending_line: None,
            connecting_since: None,
        }
    }

    /// Whether a watchdog is running (a connection opening, a move waiting
    /// for the server): the front-end owes the state a tick (D2692).
    #[must_use]
    pub fn needs_ticks(&self) -> bool {
        self.phase == Phase::Connecting
            || self.game.as_ref().is_some_and(FicsGame::has_pending_move)
    }

    /// The epoch of the connection asked for last.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Whether the assistance is locked (D2622): the user plays a game that
    /// is not over.
    #[must_use]
    pub fn assist_locked(&self) -> bool {
        self.game.as_ref().is_some_and(FicsGame::is_live_player)
    }

    /// Whether the user plays a game that is not over.
    #[must_use]
    pub fn playing(&self) -> bool {
        self.assist_locked()
    }

    /// The game the editor shows: the game of the user, else the observed
    /// one he chose.
    #[must_use]
    pub fn editor_game(&self) -> Option<&FicsGame> {
        self.game.as_ref().or_else(|| {
            let n = self.shown?;
            self.observed.iter().find(|g| g.number == n)
        })
    }

    /// Whether a connection is open or opening.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.phase != Phase::Disconnected
    }

    fn logged_in(&self) -> bool {
        self.phase == Phase::LoggedIn
    }

    fn send(outs: &mut Vec<Out>, command: Result<Command, mychess_online::fics::CommandError>) {
        match command {
            Ok(c) => outs.push(Out::Command(FicsCommand::Send(c))),
            Err(_) => outs.push(Out::Notice(FicsNotice::BadInput)),
        }
    }

    /// Takes a message.
    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    pub fn apply(&mut self, message: FicsMessage) -> Vec<Out> {
        let mut outs = Vec::new();
        match message {
            FicsMessage::Net(update) => self.on_net(update, &mut outs),
            FicsMessage::Connect(request) => self.connect(&request, &mut outs),
            FicsMessage::Disconnect => self.disconnect(&mut outs),
            FicsMessage::Ack => {}
            FicsMessage::OpenWindow => self.window_open = true,
            FicsMessage::CloseWindow => self.window_open = false,
            FicsMessage::RestorePrevious => {
                if self.playing() {
                    outs.push(Out::Notice(FicsNotice::WrongState));
                } else {
                    self.drop_finished_game();
                    outs.push(Out::Closed);
                }
            }
            FicsMessage::SendFailed { epoch, line } => {
                if epoch == self.epoch {
                    outs.push(Out::Notice(FicsNotice::SendFailed));
                    if self.pending_line.as_deref() == Some(line.as_str()) {
                        self.reject_pending(&mut outs);
                    }
                }
            }
            FicsMessage::Tick(at) => self.on_tick(at, &mut outs),
            FicsMessage::SetPrefs(prefs) => self.prefs = prefs.sanitized(),
            FicsMessage::ClearConsole => self.console.clear(),
            FicsMessage::ForgetGame => self.forget_game(&mut outs),
            FicsMessage::CancelPremove => {
                self.premove.cancel();
            }
            other if !self.logged_in() => {
                drop(other);
                outs.push(Out::Notice(FicsNotice::WrongState));
            }
            other => self.on_user(other, &mut outs),
        }
        outs
    }

    fn connect(&mut self, request: &ConnectRequest, outs: &mut Vec<Out>) {
        if self.phase != Phase::Disconnected {
            outs.push(Out::Notice(FicsNotice::WrongState));
            return;
        }
        let name_ok = request.name.as_ref().is_none_or(|n| {
            (2..=17).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric())
        });
        if !name_ok {
            outs.push(Out::Notice(FicsNotice::BadInput));
            return;
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.phase = Phase::Connecting;
        self.prefs.login = request.name.clone().unwrap_or_default();
        let init: Vec<Command> = self
            .prefs
            .init_commands
            .iter()
            .filter_map(|c| Command::raw(c).ok())
            .collect();
        self.console.push(
            LineKind::System,
            &format!("Connecting to {}:{}...", self.prefs.host, self.prefs.port),
        );
        outs.push(Out::Command(FicsCommand::Connect {
            epoch: self.epoch,
            host: self.prefs.host.clone(),
            port: self.prefs.port,
            name: request.name.clone(),
            init,
        }));
    }

    fn disconnect(&mut self, outs: &mut Vec<Out>) {
        if self.phase == Phase::Disconnected {
            return;
        }
        let quit = self.logged_in();
        let closing = self.epoch;
        self.epoch = self.epoch.wrapping_add(1);
        self.reset_connection(outs);
        outs.push(Out::Command(FicsCommand::Disconnect {
            quit,
            epoch: closing,
        }));
    }

    /// The watchdogs (D2692): a move that the server does not answer, a
    /// connection that does not open.
    fn on_tick(&mut self, at: Instant, outs: &mut Vec<Out>) {
        if self.phase == Phase::Connecting {
            let since = *self.connecting_since.get_or_insert(at);
            if at.saturating_duration_since(since) >= CONNECT_TIMEOUT {
                self.disconnect(outs);
                outs.push(Out::Notice(FicsNotice::Disconnected(Disconnect::Timeout)));
            }
        } else {
            self.connecting_since = None;
        }
        if !self.game.as_ref().is_some_and(FicsGame::has_pending_move) {
            self.clear_pending_watch();
            return;
        }
        let since = *self.pending_since.get_or_insert(at);
        let waited = at.saturating_duration_since(since);
        if waited >= MOVE_UNDO_AFTER {
            self.reject_pending(outs);
        } else if waited >= MOVE_REFRESH_AFTER && !self.pending_refreshed && self.logged_in() {
            self.pending_refreshed = true;
            let number = self.game.as_ref().map(|g| g.number);
            outs.push(Out::Command(FicsCommand::Send(Command::refresh(number))));
        }
    }

    fn clear_pending_watch(&mut self) {
        self.pending_since = None;
        self.pending_refreshed = false;
        self.pending_line = None;
    }

    /// Takes the move that waits for the server out and says so.
    fn reject_pending(&mut self, outs: &mut Vec<Out>) {
        self.clear_pending_watch();
        if self.game.as_mut().is_some_and(FicsGame::undo_local) {
            outs.push(Out::Notice(FicsNotice::MoveRejected));
            outs.push(Out::Changed);
        }
    }

    /// Forgets the game of the user that is over (it was stored already).
    fn drop_finished_game(&mut self) {
        if self.game.as_ref().is_some_and(|g| g.result.is_some()) {
            self.game = None;
            self.clear_pending_watch();
        }
    }

    /// Forgets what belongs to a connection. A game of the user that is
    /// not over stays (the server may keep it) with the lock (D2622).
    fn reset_connection(&mut self, outs: &mut Vec<Out>) {
        self.phase = Phase::Disconnected;
        self.me = None;
        self.seeks.clear();
        self.offers.clear();
        self.observed.clear();
        self.latency = None;
        self.ping_ms = None;
        self.following = None;
        self.lectures = false;
        self.premove.cancel();
        self.meta.clear();
        self.connecting_since = None;
        // A move the connection fell on is not known to the server: the
        // game waits for the next board without it.
        self.reject_pending(outs);
        if self.shown.take().is_some() && self.game.is_none() {
            outs.push(Out::Closed);
        }
        if self.game.as_ref().is_some_and(|g| g.role != Role::Playing) {
            self.game = None;
            outs.push(Out::Closed);
        }
        self.console.push(LineKind::System, "Disconnected.");
    }

    /// Releases the lock (D2699-d): without a connection, or when the game
    /// is over. Whatever the editor held of the game goes with it (D2695);
    /// a game still alive at the server stays there, with no help from the
    /// program here.
    fn forget_game(&mut self, outs: &mut Vec<Out>) {
        let over = self.game.as_ref().is_some_and(|g| g.result.is_some());
        if self.phase != Phase::Disconnected && !over {
            outs.push(Out::Notice(FicsNotice::WrongState));
            return;
        }
        if self.game.take().is_some() {
            self.clear_pending_watch();
            self.premove.cancel();
            outs.push(Out::Notice(FicsNotice::LockReleased));
            outs.push(Out::Closed);
        }
    }

    // ---- what the user does -------------------------------------------

    #[allow(clippy::too_many_lines)] // one flat match over the messages reads best
    fn on_user(&mut self, message: FicsMessage, outs: &mut Vec<Out>) {
        match message {
            FicsMessage::Console(text) => match Command::raw(&text) {
                Ok(command) => {
                    self.console.push(LineKind::Input, &command.echo());
                    outs.push(Out::Command(FicsCommand::Send(command)));
                }
                Err(_) => outs.push(Out::Notice(FicsNotice::BadInput)),
            },
            FicsMessage::Seek(spec) => {
                if self.playing() {
                    outs.push(Out::Notice(FicsNotice::WrongState));
                    return;
                }
                self.prefs.minutes = spec.minutes;
                self.prefs.increment = spec.increment;
                self.prefs.rated = spec.rated;
                Self::send(outs, Command::seek(&spec));
            }
            FicsMessage::Unseek => outs.push(Out::Command(FicsCommand::Send(Command::unseek()))),
            FicsMessage::Challenge {
                player,
                minutes,
                increment,
                rated,
                color,
            } => {
                if self.playing() {
                    outs.push(Out::Notice(FicsNotice::WrongState));
                    return;
                }
                Self::send(
                    outs,
                    Command::challenge(&player, minutes, increment, rated, color),
                );
            }
            FicsMessage::Accept(from) => {
                // `accept` answers the offer of any kind (D2696).
                self.offers.retain(|o| {
                    from.as_deref()
                        .is_some_and(|f| !o.from().eq_ignore_ascii_case(f))
                });
                Self::send(outs, Command::accept(from.as_deref()));
            }
            FicsMessage::Decline(from) => {
                self.offers.retain(|o| {
                    from.as_deref()
                        .is_some_and(|f| !o.from().eq_ignore_ascii_case(f))
                });
                Self::send(outs, Command::decline(from.as_deref()));
            }
            FicsMessage::PlaySeek(index) => {
                if self.playing() || !self.seeks.get(index).is_some_and(is_playable) {
                    outs.push(Out::Notice(FicsNotice::WrongState));
                } else {
                    outs.push(Out::Command(FicsCommand::Send(Command::play_seek(index))));
                }
            }
            FicsMessage::PlayerMove {
                from,
                to,
                promotion,
            } => self.player_move(from, to, promotion, outs),
            FicsMessage::Resign => self.game_command(Command::resign(), outs),
            FicsMessage::OfferDraw => {
                self.offers.retain(|o| !matches!(o, Offer::Draw { .. }));
                self.game_command(Command::draw(), outs);
            }
            FicsMessage::Abort => {
                self.offers.retain(|o| !matches!(o, Offer::Abort { .. }));
                self.game_command(Command::abort(), outs);
            }
            FicsMessage::Takeback(plies) => match Command::takeback(plies) {
                Ok(c) => self.game_command(c, outs),
                Err(_) => outs.push(Out::Notice(FicsNotice::BadInput)),
            },
            FicsMessage::Observe(target) => {
                if self.observed.len() >= MAX_OBSERVED {
                    outs.push(Out::Notice(FicsNotice::TooManyObserved));
                    return;
                }
                let command = match target.trim().parse::<u32>() {
                    Ok(n) => Ok(Command::observe_game(n)),
                    Err(_) => Command::observe_player(target.trim()),
                };
                Self::send(outs, command);
            }
            FicsMessage::Unobserve(n) => {
                self.observed.retain(|g| g.number != n);
                if self.shown == Some(n) {
                    self.shown = None;
                    outs.push(Out::Closed);
                }
                outs.push(Out::Command(FicsCommand::Send(Command::unobserve(n))));
            }
            FicsMessage::Show(n) => {
                // A game that is over does not hold the editor (D2697).
                let busy = self.game.as_ref().is_some_and(|g| g.result.is_none());
                if busy || !self.observed.iter().any(|g| g.number == n) {
                    outs.push(Out::Notice(FicsNotice::WrongState));
                } else {
                    self.drop_finished_game();
                    self.shown = Some(n);
                    outs.push(Out::Opened);
                }
            }
            FicsMessage::Follow(player) => {
                if self.playing() {
                    outs.push(Out::Notice(FicsNotice::FollowWhilePlaying));
                    return;
                }
                match Command::follow(&player) {
                    Ok(c) => {
                        self.following = Some(player);
                        outs.push(Out::Command(FicsCommand::Send(c)));
                    }
                    Err(_) => outs.push(Out::Notice(FicsNotice::BadInput)),
                }
            }
            FicsMessage::Unfollow => {
                self.following = None;
                outs.push(Out::Command(FicsCommand::Send(Command::unfollow())));
            }
            FicsMessage::Examine(from) => {
                if self.playing() {
                    outs.push(Out::Notice(FicsNotice::WrongState));
                } else {
                    Self::send(outs, Command::examine(from.as_deref()));
                }
            }
            FicsMessage::Unexamine => {
                outs.push(Out::Command(FicsCommand::Send(Command::unexamine())));
            }
            FicsMessage::Forward(n) => {
                outs.push(Out::Command(FicsCommand::Send(Command::forward(n))));
            }
            FicsMessage::Backward(n) => {
                outs.push(Out::Command(FicsCommand::Send(Command::backward(n))));
            }
            FicsMessage::Lectures(on) => {
                let command = if on {
                    Command::join_channel(LECTURE_CHANNEL)
                } else {
                    Command::leave_channel(LECTURE_CHANNEL)
                };
                self.lectures = on;
                Self::send(outs, command);
            }
            // The others never reach here (`apply` handled them).
            _ => {}
        }
    }

    fn game_command(&mut self, command: Command, outs: &mut Vec<Out>) {
        if self.playing() {
            outs.push(Out::Command(FicsCommand::Send(command)));
        } else {
            outs.push(Out::Notice(FicsNotice::WrongState));
        }
    }

    fn player_move(
        &mut self,
        from: mychess_core::Square,
        to: mychess_core::Square,
        promotion: Option<mychess_core::Role>,
        outs: &mut Vec<Out>,
    ) {
        let auto = self.prefs.auto_promote.role();
        let premove_on = self.prefs.premove;
        let Some(game) = self.game.as_mut().filter(|g| g.is_live_player()) else {
            outs.push(Out::Notice(FicsNotice::WrongState));
            return;
        };
        let Some(me) = game.my_color else {
            return;
        };
        let promotion = auto.or(promotion);
        if !game.my_turn() {
            // D2615: kept on the client until it is his turn.
            if premove_on
                && let Some(premove) = Premove::enter(game.position(), me, from, to, promotion)
            {
                self.premove.set(premove);
            }
            return;
        }
        let position = game.position().clone();
        let chosen = position.find_move(from, to, promotion.or(Some(mychess_core::Role::Queen)));
        let mv = chosen.or_else(|| position.find_move(from, to, None));
        let Some(mv) = mv else {
            return;
        };
        self.premove.cancel();
        if game.push_local(mv).is_err() {
            return;
        }
        let commands = move_commands(&position, mv);
        self.pending_since = None;
        self.pending_refreshed = false;
        self.pending_line = commands.last().map(|c| c.line().to_owned());
        for command in commands {
            outs.push(Out::Command(FicsCommand::Send(command)));
        }
        outs.push(Out::Changed);
    }

    // ---- what the connection says -------------------------------------

    fn on_net(&mut self, update: NetUpdate, outs: &mut Vec<Out>) {
        if update.epoch != self.epoch {
            return;
        }
        match update.message {
            Net::Connected => self
                .console
                .push(LineKind::System, "Connected. Logging in..."),
            Net::LoggedIn { name } => {
                self.phase = Phase::LoggedIn;
                self.console
                    .push(LineKind::System, &format!("Logged in as {name}."));
                self.me = Some(name.clone());
                outs.push(Out::Notice(FicsNotice::Connected { name }));
            }
            Net::Latency(d) => self.latency = Some(d),
            Net::Disconnected(end) => {
                self.reset_connection(outs);
                outs.push(Out::Notice(FicsNotice::Disconnected(end)));
            }
            Net::Event(event) => self.on_event(event, update.at, outs),
            // `Net` is non-exhaustive.
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the events reads best
    fn on_event(&mut self, event: FicsEvent, at: Instant, outs: &mut Vec<Out>) {
        match event {
            FicsEvent::Style12(line) => self.on_board(&line, at, outs),
            FicsEvent::SeekAdd(seek) => self.seeks.add(*seek),
            FicsEvent::SeekRemove(ids) => self.seeks.remove(&ids),
            FicsEvent::SeekClear => self.seeks.clear(),
            FicsEvent::Creating(summary) | FicsEvent::Listed(summary) => {
                self.console.push(
                    LineKind::Server,
                    &format!(
                        "{} ({}) vs {} ({}) {} {} {} {}",
                        summary.white,
                        summary
                            .white_rating
                            .map_or_else(|| "-".into(), |r| r.to_string()),
                        summary.black,
                        summary
                            .black_rating
                            .map_or_else(|| "-".into(), |r| r.to_string()),
                        if summary.rated { "rated" } else { "unrated" },
                        summary.kind,
                        summary.minutes,
                        summary.increment,
                    ),
                );
                self.remember(*summary);
            }
            FicsEvent::GameEnded {
                game,
                reason,
                result,
                white,
                black,
            } => {
                self.console.push(
                    LineKind::Server,
                    &format!("Game {game} ({white} vs. {black}): {reason}"),
                );
                self.on_game_end(game, (&white, &black), result, &reason, outs);
            }
            FicsEvent::Challenge(offer) => {
                let mine = self.me.as_deref();
                let to_me = mine.is_some_and(|m| offer.opponent.eq_ignore_ascii_case(m));
                self.console.push(
                    LineKind::Server,
                    &format!(
                        "Challenge: {} vs {} {} {} {}+{}",
                        offer.challenger,
                        offer.opponent,
                        if offer.rated { "rated" } else { "unrated" },
                        offer.kind,
                        offer.minutes,
                        offer.increment
                    ),
                );
                if to_me {
                    self.add_offer(Offer::Match {
                        from: offer.challenger,
                        rating: offer.challenger_rating,
                        rated: offer.rated,
                        kind: offer.kind,
                        minutes: offer.minutes,
                        increment: offer.increment,
                    });
                }
            }
            FicsEvent::DrawOffer { from } => {
                self.console
                    .push(LineKind::Server, &format!("{from} offers a draw."));
                self.add_offer(Offer::Draw { from });
            }
            FicsEvent::TakebackOffer { from, plies } => {
                self.console.push(
                    LineKind::Server,
                    &format!("{from} asks to take back {plies} ply."),
                );
                self.add_offer(Offer::Takeback { from, plies });
            }
            FicsEvent::AbortOffer { from } => {
                self.console
                    .push(LineKind::Server, &format!("{from} asks to abort."));
                self.add_offer(Offer::Abort { from });
            }
            FicsEvent::Unobserved { game } => {
                self.observed.retain(|g| g.number != game);
                if self.shown == Some(game) {
                    self.shown = None;
                    outs.push(Out::Closed);
                }
            }
            FicsEvent::ExamineEnded => {
                if self
                    .game
                    .as_ref()
                    .is_some_and(|g| g.role == Role::Examining)
                {
                    self.game = None;
                    outs.push(Out::Closed);
                }
            }
            FicsEvent::Tell { from, text } => {
                self.console
                    .push(LineKind::Chat, &format!("{from} tells you: {text}"));
            }
            FicsEvent::Shout { from, text } => {
                self.console
                    .push(LineKind::Chat, &format!("{from} shouts: {text}"));
            }
            FicsEvent::ChannelTell {
                channel,
                from,
                text,
            } => {
                self.console
                    .push(LineKind::Chat, &format!("[{channel}] {from}: {text}"));
            }
            FicsEvent::Ping { ms } => self.ping_ms = Some(ms),
            FicsEvent::Text(text) => {
                if MOVE_REFUSALS.iter().any(|r| text.starts_with(r)) {
                    self.reject_pending(outs);
                }
                self.console.push(LineKind::Server, &text);
            }
            // Login events are the business of the connection; the rest
            // (observing, examining announcements, guest name) is console
            // text only.
            other => self.console.push(LineKind::Server, &describe(&other)),
        }
    }

    fn add_offer(&mut self, offer: Offer) {
        self.offers.retain(|o| {
            std::mem::discriminant(o) != std::mem::discriminant(&offer) || o.from() != offer.from()
        });
        if self.offers.len() >= MAX_OFFERS {
            self.offers.remove(0);
        }
        self.offers.push(offer);
    }

    fn remember(&mut self, summary: GameSummary) {
        self.meta
            .retain(|m| !(m.white == summary.white && m.black == summary.black));
        if self.meta.len() >= MAX_META {
            self.meta.remove(0);
        }
        self.meta.push(summary);
    }

    fn meta_for(&self, line: &Style12) -> Option<GameSummary> {
        self.meta
            .iter()
            .find(|m| m.game == Some(line.game) || (m.white == line.white && m.black == line.black))
            .cloned()
    }

    fn on_game_end(
        &mut self,
        number: u32,
        names: (&str, &str),
        result: FicsResult,
        reason: &str,
        outs: &mut Vec<Out>,
    ) {
        // The number alone is not the game: the server reuses it (D2690).
        let same = |g: &FicsGame| {
            g.number == number
                && g.white.eq_ignore_ascii_case(names.0)
                && g.black.eq_ignore_ascii_case(names.1)
        };
        if let Some(game) = self.game.as_mut().filter(|g| same(g) && g.result.is_none()) {
            game.finish(result, reason);
            let played = game.role == Role::Playing;
            self.premove.cancel();
            if played {
                // Offers belong to the game that is over (D2696).
                self.offers.retain(|o| matches!(o, Offer::Match { .. }));
                outs.push(Out::Notice(FicsNotice::GameEnded));
                outs.push(Out::Finished);
            }
            outs.push(Out::Changed);
        }
        if let Some(game) = self.observed.iter_mut().find(|g| same(g)) {
            game.finish(result, reason);
            if self.shown == Some(number) {
                outs.push(Out::Changed);
            }
        }
    }

    fn on_board(&mut self, line: &Style12, at: Instant, outs: &mut Vec<Out>) {
        match line.relation {
            Relation::PlayingMyMove | Relation::PlayingOpponentToMove => {
                self.on_playing_board(line, at, outs);
            }
            Relation::Examining | Relation::ObservingExamined => {
                self.on_examined_board(line, at, outs);
            }
            Relation::Observing => self.on_observed_board(line, at, outs),
            // Isolated positions (`ref`) and unknown relations are not games.
            _ => {}
        }
    }

    fn on_playing_board(&mut self, line: &Style12, at: Instant, outs: &mut Vec<Out>) {
        // A game that is over, or another one under the same number, is
        // not this game (D2690).
        if let Some(game) = self.game.as_mut().filter(|g| g.continues(line)) {
            let applied = game.apply(line, at);
            if applied == Applied::Resynced {
                outs.push(Out::Notice(FicsNotice::Incomplete));
            }
            if game.my_turn()
                && let Some(mv) = self.premove.take_if_legal(game.position())
            {
                // D2615: the premove goes out as soon as it is his turn.
                let position = game.position().clone();
                if game.push_local(mv).is_ok() {
                    let commands = move_commands(&position, mv);
                    self.pending_since = None;
                    self.pending_refreshed = false;
                    self.pending_line = commands.last().map(|c| c.line().to_owned());
                    for command in commands {
                        outs.push(Out::Command(FicsCommand::Send(command)));
                    }
                }
            } else if game.my_turn() {
                self.premove.cancel();
            }
            outs.push(Out::Changed);
            return;
        }
        if self.game.as_ref().is_some_and(FicsGame::is_live_player) {
            // Another game while one is on: not played here (D2613).
            outs.push(Out::Notice(FicsNotice::AnotherGame));
            return;
        }
        let meta = self.meta_for(line);
        let Some(game) = FicsGame::from_style12(line, meta.as_ref(), at) else {
            return;
        };
        // The old game of the same number (if any) is over: this is a new
        // one, with its own lock and its own editor game (D2690).
        self.clear_pending_watch();
        self.premove.cancel();
        // The game leaves the list of the observed.
        self.observed.retain(|g| g.number != line.game);
        self.shown = None;
        self.game = Some(game);
        outs.push(Out::Starting(at));
        outs.push(Out::Notice(FicsNotice::GameStarted));
        outs.push(Out::Opened);
    }

    fn on_examined_board(&mut self, line: &Style12, at: Instant, outs: &mut Vec<Out>) {
        if let Some(game) = self.game.as_mut().filter(|g| g.continues(line)) {
            game.apply(line, at);
            outs.push(Out::Changed);
            return;
        }
        if self.game.as_ref().is_some_and(FicsGame::is_live_player) {
            return;
        }
        let meta = self.meta_for(line);
        if meta.as_ref().is_some_and(|m| !is_standard_kind(&m.kind)) {
            return;
        }
        if let Some(game) = FicsGame::from_style12(line, meta.as_ref(), at) {
            self.shown = None;
            self.game = Some(game);
            outs.push(Out::Opened);
        }
    }

    fn on_observed_board(&mut self, line: &Style12, at: Instant, outs: &mut Vec<Out>) {
        if let Some(game) = self.observed.iter_mut().find(|g| g.number == line.game) {
            game.apply(line, at);
            if self.shown == Some(line.game) {
                outs.push(Out::Changed);
            }
            return;
        }
        if self.observed.len() >= MAX_OBSERVED {
            // D2604: beyond the limit the server is told to stop.
            outs.push(Out::Command(FicsCommand::Send(Command::unobserve(
                line.game,
            ))));
            outs.push(Out::Notice(FicsNotice::TooManyObserved));
            return;
        }
        let meta = self.meta_for(line);
        if meta.as_ref().is_some_and(|m| !is_standard_kind(&m.kind)) {
            // A variant is not shown (D2693): the server is told to stop.
            outs.push(Out::Command(FicsCommand::Send(Command::unobserve(
                line.game,
            ))));
            return;
        }
        if let Some(game) = FicsGame::from_style12(line, meta.as_ref(), at) {
            self.observed.push(game);
        }
    }
}

fn is_standard_kind(kind: &str) -> bool {
    kind.is_empty() || PLAYABLE_KINDS.contains(&kind)
}

/// How the server refuses a move (D2692).
const MOVE_REFUSALS: [&str; 4] = [
    "Illegal move",
    "It is not your move",
    "You are not playing",
    "Game is over",
];

/// The commands that play `mv` in `position`: the promotion piece first
/// (D2614), then the move in coordinates.
fn move_commands(position: &Position, mv: Move) -> Vec<Command> {
    let uci = position.uci(mv);
    let mut out = Vec::new();
    if let Some(piece) = uci.chars().nth(4)
        && let Ok(c) = Command::promote(piece)
    {
        out.push(c);
    }
    if let Ok(c) = Command::play_move(&uci) {
        out.push(c);
    }
    out
}

fn describe(event: &FicsEvent) -> String {
    match event {
        FicsEvent::Observing { game } => format!("You are now observing game {game}."),
        FicsEvent::Examining { game } => format!("You are now examining game {game}."),
        FicsEvent::GameBegan { game, white, black } => {
            format!("Game {game} ({white} vs. {black}) begins.")
        }
        FicsEvent::GuestName(name) => format!("Guest account: {name}"),
        FicsEvent::SessionStart { name } => format!("Session of {name} started."),
        FicsEvent::LoginFailed(_) => "Login refused.".to_owned(),
        _ => String::new(),
    }
}
