//! The state of the online play and its pure transitions (spec M8-3, D1311,
//! D1318-D1325, D1330-D1333).
//!
//! [`OnlineState::apply`] is the whole state machine: it takes a message and
//! the little the machine must know of the rest of the program ([`Ctx`]),
//! changes the state and says what the program must do about it
//! ([`Out`]). Nothing here reads the clock (every message that needs an
//! instant carries one), touches the disk or the network.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mychess_core::{Color, Move, Square};
use mychess_online::models::{
    Challenge, Direction, Event, GameEvent, GameFull, GameInfo, GameState, WatchEvent, WatchMove,
};
use mychess_online::time_control::{validate_challenge, validate_seek};
use mychess_online::{SeekOutcome, StreamMessage};

use super::commands::{OnlineCommand, OnlineHub};
use super::game_builder::{
    Applied, MAX_LABEL_CHARS, MAX_NAME_CHARS, OnlineGame, Outcome, StartError, clean_text,
};
use super::message::{
    AccountView, ActionDone, ActionKind, ChallengeRequest, ConnectRequest, MarkerInfo, OnlineColor,
    OnlineError, OnlineMessage, OnlineNotice, OnlineVariant, RejectReason, RetryKind, SeekRequest,
    StoreChoice, StreamKind, StreamUpdate, TokenSource,
};
use super::prefs::{OnlinePrefs, base_url_is_acceptable, login_is_valid};
use super::premove::{Premove, PremoveSlot};
use super::session::{Phase, PlayingState};
use super::watch::{Followed, ParsedExport, WatchPhase, WatchedGame};
use crate::play::PlayClock;

/// How long the lock of a stale marker waits for the event stream to show
/// that the game is still going on (D1332).
pub const MARKER_GRACE: Duration = Duration::from_secs(5);

/// How many times the stream of a game is reopened for a divergence or an
/// end that was not announced (D1318).
const MAX_REOPENS: u8 = 2;

/// How long the stream of a lost game waits before it is opened again
/// (D1365).
pub const GAME_RETRY_DELAY: Duration = Duration::from_secs(10);

/// How many times the stream of a lost game is opened again before the game
/// is closed here (D1365).
const MAX_GAME_RETRIES: u8 = 6;

/// How long a search or a challenge waits for the `gameStart` after the
/// server said it ended (D1366).
pub const SEARCH_WATCHDOG: Duration = Duration::from_secs(15);

/// The most steps of a watched game kept while its PGN is on the way.
const MAX_BUFFERED_STEPS: usize = 4096;
/// The longest `fen` of a watched step that is kept (D3115).
const MAX_FEN_CHARS: usize = 100;

/// A login in the browser that waits (D3005, D3009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthWait {
    /// The authorization page, once it is ready.
    pub url: Option<String>,
    /// The program opened it in the browser.
    pub opened: bool,
}

/// A game of other players being watched (D3000).
#[derive(Debug)]
pub struct Watch {
    /// The id of the game.
    pub id: String,
    base_url: String,
    /// How far it got.
    pub phase: WatchPhase,
    /// The game, once the server described it.
    pub game: Option<WatchedGame>,
    /// The position of the description, for when the PGN does not come.
    first_fen: Option<String>,
    /// Steps that arrived before the PGN (they are applied after it).
    buffered: Vec<WatchMove>,
    pgn_done: bool,
    /// Steps were dropped because the buffer was full (said once).
    dropped_steps: bool,
}

/// What the machine needs to know of the rest of the program.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    /// A game against an engine, a tournament, an annotation or a training
    /// mode is on (D1312).
    pub busy: bool,
    /// The preferences.
    pub prefs: &'a OnlinePrefs,
}

/// The state of the connection of the event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Link {
    /// Not connected.
    Down,
    /// Receiving events.
    Up,
    /// The connection was lost and is being reopened.
    Reconnecting,
}

/// A challenge received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingChallenge {
    /// The id (also the id of the game).
    pub id: String,
    /// The name of the challenger.
    pub from: String,
    /// His title.
    pub title: Option<String>,
    /// His rating.
    pub rating: Option<u32>,
    /// Rated game.
    pub rated: bool,
    /// The time control as the server shows it (`10+5`).
    pub time: String,
    /// The app can play it: standard or Chess960, blitz or slower, with a clock.
    pub playable: bool,
    /// The reason sent when it is declined.
    pub decline_reason: &'static str,
    /// A Chess960 game (M9-6).
    pub chess960: bool,
}

/// The opponent left the game (D1325).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpponentGone {
    /// Seconds after `since` at which the victory can be claimed.
    pub claim_secs: Option<u32>,
    /// When the server said it.
    pub since: Instant,
}

impl OpponentGone {
    /// Whether the victory can be claimed at `now`.
    pub fn claimable(&self, now: Instant) -> bool {
        let wait = Duration::from_secs(u64::from(self.claim_secs.unwrap_or(0)));
        now.saturating_duration_since(self.since) >= wait
    }
}

#[derive(Debug, Clone)]
struct PendingMove {
    mv: Move,
    ply: usize,
    sent_at: Instant,
}

/// What the program must do after a message.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Out {
    /// Run a command.
    Command(OnlineCommand),
    /// Show a remark.
    Notice(OnlineNotice),
    /// The account that was used; the preferences remember its name.
    Login(String),
    /// A game starts (or resumes): stop engines and everything assistive
    /// (D1331).
    Starting {
        /// The game was already on (resumed).
        resumed: bool,
        /// When.
        at: Instant,
    },
    /// The game was described by the server: show it in the editor.
    Opened,
    /// The moves of the game changed: show them.
    Changed,
    /// The game is over: store it.
    Finished,
    /// Put the game that was in the editor before back.
    RestorePrevious,
    /// The result was dismissed.
    Dismissed,
    /// The game being watched was described: show it in the editor.
    WatchOpened,
    /// The watched game changed.
    WatchChanged,
    /// The watch ended: put the previous game back.
    WatchClosed,
}

/// The online play.
#[derive(Debug)]
pub struct OnlineState {
    /// The client, the streams and the stores.
    pub hub: OnlineHub,
    /// Where the session is.
    pub phase: Phase,
    /// The event stream.
    pub link: Link,
    /// The connected account.
    pub account: Option<AccountView>,
    /// The game, once the server described it.
    pub game: Option<OnlineGame>,
    /// The clock, fed by the server (D1319).
    pub clock: PlayClock,
    /// The premove waiting for the turn of the user (D1321).
    pub premove: PremoveSlot,
    /// The challenges received.
    pub incoming: Vec<IncomingChallenge>,
    /// The id of the challenge sent, once the server named it.
    pub outgoing: Option<String>,
    /// The opponent left (D1325).
    pub opponent_gone: Option<OpponentGone>,
    /// The latency of the last move, milliseconds (D1320).
    pub ping: Option<Duration>,
    /// How the last game ended.
    pub finished: Option<Outcome>,
    /// A login in the browser is waiting (D3005).
    pub oauth: Option<OAuthWait>,
    /// A game of other players is being watched (D3000).
    pub watch: Option<Watch>,
    /// The marker of a game that may be on (D1332).
    pub marker: Option<MarkerInfo>,
    /// The user released the lock by hand (D1332).
    pub released: bool,
    /// The last remark of the online play.
    pub notice: Option<OnlineNotice>,
    pub(crate) marker_dir: Option<PathBuf>,
    next_epoch: u64,
    account_epoch: u64,
    game_epoch: u64,
    seek_epoch: u64,
    challenge_epoch: u64,
    game_id: Option<String>,
    pending: Option<PendingMove>,
    reopens: u8,
    expecting: bool,
    grace: u64,
    retry_gen: u64,
    game_retries: u8,
    final_fetch: bool,
    search_gen: u64,
    search_ended: bool,
    marker_seq: u64,
    oauth_epoch: u64,
    watch_epoch: u64,
}

impl Default for OnlineState {
    fn default() -> OnlineState {
        OnlineState::new(OnlineHub::new())
    }
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

impl OnlineState {
    /// A disconnected state around `hub`.
    pub fn new(hub: OnlineHub) -> OnlineState {
        OnlineState {
            hub,
            phase: Phase::Disconnected,
            link: Link::Down,
            account: None,
            game: None,
            clock: PlayClock::authoritative(),
            premove: PremoveSlot::new(),
            incoming: Vec::new(),
            outgoing: None,
            opponent_gone: None,
            ping: None,
            finished: None,
            oauth: None,
            watch: None,
            marker: None,
            released: false,
            notice: None,
            marker_dir: None,
            next_epoch: 0,
            account_epoch: 0,
            game_epoch: 0,
            seek_epoch: 0,
            challenge_epoch: 0,
            game_id: None,
            pending: None,
            reopens: 0,
            expecting: false,
            grace: 0,
            retry_gen: 0,
            game_retries: 0,
            final_fetch: false,
            search_gen: 0,
            search_ended: false,
            marker_seq: 0,
            oauth_epoch: 0,
            watch_epoch: 0,
        }
    }

    /// Whether the assistance is locked (D1330): a game as a player is on,
    /// or a marker says one may be, and the user did not release it.
    pub fn assist_locked(&self) -> bool {
        (self.phase.is_live() && !self.released) || self.marker.is_some()
    }

    /// Whether the user may release the lock by hand (D1332, D1362): never
    /// while the event stream is up and a game or a marker is there (the
    /// server can tell, and the grace period of the marker settles it).
    pub fn can_remove_lock(&self) -> bool {
        self.assist_locked()
            && !(self.link == Link::Up && (self.phase.is_live() || self.marker.is_some()))
    }

    /// The side of the user in the game.
    pub fn my_color(&self) -> Option<Color> {
        self.game.as_ref().map(|g| g.my_color)
    }

    /// The remaining time of `side` at `now` (D1319).
    pub fn shown(&self, side: Color, now: Instant) -> Duration {
        self.clock.shown(side, now)
    }

    /// Whether the victory over an absent opponent can be claimed at `now`.
    pub fn can_claim_victory(&self, now: Instant) -> bool {
        self.phase.is_live() && self.opponent_gone.is_some_and(|g| g.claimable(now))
    }

    /// Whether the game can still be aborted (fewer than two plies).
    pub fn can_abort(&self) -> bool {
        self.phase.is_live()
            && self
                .game
                .as_ref()
                .is_some_and(|g| !g.is_over() && g.plies() < 2)
    }

    /// Whether the opponent offers a draw.
    pub fn draw_offered_to_me(&self) -> bool {
        self.game.as_ref().is_some_and(|g| match g.my_color {
            Color::White => g.bdraw,
            Color::Black => g.wdraw,
        })
    }

    /// Whether the opponent proposes a takeback.
    pub fn takeback_proposed_to_me(&self) -> bool {
        self.game.as_ref().is_some_and(|g| match g.my_color {
            Color::White => g.btakeback,
            Color::Black => g.wtakeback,
        })
    }

    /// Whether the user can propose a takeback now: his last move is on the
    /// board and no proposal is pending either way (D3011).
    pub fn can_propose_takeback(&self) -> bool {
        self.phase.is_live()
            && self.game.as_ref().is_some_and(|g| {
                let mine = match g.my_color {
                    Color::White => g.wtakeback,
                    Color::Black => g.btakeback,
                };
                let my_moves = usize::midpoint(g.plies(), usize::from(g.my_color == Color::White));
                !g.is_over() && my_moves >= 1 && !mine && !self.takeback_proposed_to_me()
            })
    }

    /// Whether a draw can be claimed over an absent opponent at `now`.
    pub fn can_claim_draw(&self, now: Instant) -> bool {
        self.can_claim_victory(now)
    }

    /// Whether the finished game can be played again against the same
    /// opponent (D3012).
    pub fn can_rematch(&self) -> bool {
        self.phase == Phase::Playing(PlayingState::Finished)
            && self.link == Link::Up
            && self.account.as_ref().is_some_and(|a| a.can_challenge)
            && self.game.as_ref().is_some_and(|g| {
                g.clock.is_some()
                    && g.variant == "standard"
                    && match g.my_color {
                        Color::White => g.black.id.is_some(),
                        Color::Black => g.white.id.is_some(),
                    }
            })
    }

    /// Whether a game of other players is being watched.
    pub fn is_watching(&self) -> bool {
        self.watch.is_some()
    }

    fn bump(&mut self) -> u64 {
        self.next_epoch = self.next_epoch.wrapping_add(1);
        self.next_epoch
    }

    fn reject(outs: &mut Vec<Out>, reason: RejectReason) {
        outs.push(Out::Notice(OnlineNotice::Rejected(reason)));
    }

    /// A marker command with the next number (D1372).
    fn marker_command(&mut self, make: impl FnOnce(u64, PathBuf) -> OnlineCommand) -> Option<Out> {
        let dir = self.marker_dir.clone()?;
        self.marker_seq = self.marker_seq.wrapping_add(1);
        Some(Out::Command(make(self.marker_seq, dir)))
    }

    /// Applies one message.
    pub fn apply(&mut self, message: OnlineMessage, ctx: &Ctx<'_>) -> Vec<Out> {
        let mut outs = Vec::new();
        match message {
            OnlineMessage::Connect(request) => self.connect(request, ctx, &mut outs),
            OnlineMessage::Disconnect => self.disconnect(ctx, &mut outs),
            OnlineMessage::Seek(request) => self.seek(&request, ctx, &mut outs),
            OnlineMessage::CancelSeek => self.cancel_seek(&mut outs),
            OnlineMessage::Challenge(request) => self.challenge(&request, ctx, &mut outs),
            OnlineMessage::CancelChallenge => self.cancel_challenge(&mut outs),
            OnlineMessage::AcceptChallenge(id) => self.accept_challenge(&id, ctx, &mut outs),
            OnlineMessage::DeclineChallenge(id) => self.decline_challenge(&id, &mut outs),
            OnlineMessage::PlayerMove {
                from,
                to,
                promotion,
                offer_draw,
                at,
            } => self.player_move(from, to, promotion, offer_draw, at, ctx, &mut outs),
            OnlineMessage::CancelPremove => {
                self.premove.cancel();
            }
            OnlineMessage::Resign => self.game_command(ActionKind::Resign, &mut outs),
            OnlineMessage::Abort => self.game_command(ActionKind::Abort, &mut outs),
            OnlineMessage::OfferDraw => self.game_command(ActionKind::DrawYes, &mut outs),
            OnlineMessage::AnswerDraw(yes) => {
                let kind = if yes {
                    ActionKind::DrawYes
                } else {
                    ActionKind::DrawNo
                };
                self.guarded_command(self.draw_offered_to_me(), kind, &mut outs);
            }
            OnlineMessage::AnswerTakeback(yes) => {
                let kind = if yes {
                    ActionKind::TakebackYes
                } else {
                    ActionKind::TakebackNo
                };
                self.guarded_command(self.takeback_proposed_to_me(), kind, &mut outs);
            }
            OnlineMessage::ClaimVictory { at } => {
                self.guarded_command(
                    self.can_claim_victory(at),
                    ActionKind::ClaimVictory,
                    &mut outs,
                );
            }
            OnlineMessage::ProposeTakeback => {
                self.guarded_command(
                    self.can_propose_takeback(),
                    ActionKind::TakebackPropose,
                    &mut outs,
                );
            }
            OnlineMessage::ClaimDraw { at } => {
                self.guarded_command(self.can_claim_draw(at), ActionKind::ClaimDraw, &mut outs);
            }
            OnlineMessage::Rematch => self.rematch(ctx, &mut outs),
            OnlineMessage::Watch(text) => self.start_watch(&text, ctx, &mut outs),
            OnlineMessage::StopWatching => self.stop_watch(&mut outs),
            OnlineMessage::StartOAuth { store } => self.start_oauth(store, ctx, &mut outs),
            OnlineMessage::CancelOAuth => self.cancel_oauth(&mut outs),
            OnlineMessage::OAuthUrl { epoch, url, opened } => {
                self.oauth_url(epoch, url, opened, &mut outs);
            }
            OnlineMessage::OAuthDone {
                epoch,
                result,
                store,
            } => self.oauth_done(epoch, result, store, ctx, &mut outs),
            OnlineMessage::WatchPgn { epoch, result } => {
                self.watch_pgn(epoch, result.ok(), &mut outs);
            }
            OnlineMessage::DismissResult => self.dismiss(&mut outs),
            OnlineMessage::RestorePrevious => self.restore_previous(&mut outs),
            OnlineMessage::RemoveLock { confirmed } => self.remove_lock(confirmed, &mut outs),
            OnlineMessage::LoadMarker => {
                if let Some(out) = self.marker_command(|_, dir| OnlineCommand::ReadMarker { dir }) {
                    outs.push(out);
                }
            }
            OnlineMessage::Ack => {}
            OnlineMessage::Connected { epoch, result } => self.connected(epoch, result, &mut outs),
            OnlineMessage::Stream(update) => self.stream(update, &mut outs),
            OnlineMessage::Done(done) => self.done(&done, &mut outs),
            OnlineMessage::MarkerRead(info) => self.marker_read(info, &mut outs),
            OnlineMessage::MarkerGrace(generation) => self.marker_grace(generation, &mut outs),
            OnlineMessage::Retry { kind, generation } => self.retry(kind, generation, &mut outs),
        }
        for out in &outs {
            if let Out::Notice(notice) = out {
                self.notice = Some(notice.clone());
            }
        }
        outs
    }

    fn oauth_url(&mut self, epoch: u64, url: String, opened: bool, outs: &mut Vec<Out>) {
        if epoch == self.oauth_epoch
            && let Some(wait) = self.oauth.as_mut()
        {
            wait.url = Some(url);
            wait.opened = opened;
            if !opened {
                outs.push(Out::Notice(OnlineNotice::BrowserNotOpened));
            }
        }
    }

    fn marker_read(&mut self, info: Option<MarkerInfo>, outs: &mut Vec<Out>) {
        self.marker = info;
        if self.link == Link::Up {
            self.arm_grace(outs);
        }
    }

    fn restore_previous(&mut self, outs: &mut Vec<Out>) {
        if self.phase.is_live() {
            Self::reject(outs, RejectReason::GameInProgress);
        } else {
            outs.push(Out::RestorePrevious);
        }
    }

    /// Runs a game command when `allowed`, otherwise rejects it.
    fn guarded_command(&mut self, allowed: bool, kind: ActionKind, outs: &mut Vec<Out>) {
        if allowed {
            self.game_command(kind, outs);
        } else {
            Self::reject(outs, RejectReason::NotAvailable);
        }
    }

    // ---- connection ----

    fn connect(&mut self, request: ConnectRequest, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if !self.phase.can_connect() {
            return Self::reject(outs, RejectReason::WrongState);
        }
        if !base_url_is_acceptable(&ctx.prefs.base_url) {
            outs.push(Out::Notice(OnlineNotice::Error(OnlineError::InsecureUrl)));
            return;
        }
        self.phase = Phase::Connecting;
        let epoch = self.bump();
        self.account_epoch = epoch;
        outs.push(Out::Command(OnlineCommand::Connect {
            epoch,
            base_url: ctx.prefs.base_url.clone(),
            source: request.source,
            store: request.store,
        }));
    }

    fn connected(
        &mut self,
        epoch: u64,
        result: Result<AccountView, OnlineError>,
        outs: &mut Vec<Out>,
    ) {
        if epoch != self.account_epoch || self.phase != Phase::Connecting {
            return;
        }
        match result {
            Ok(view) => {
                outs.push(Out::Login(view.login.clone()));
                self.account = Some(view);
                self.phase = Phase::Idle;
                outs.push(Out::Notice(OnlineNotice::Connected));
            }
            Err(error) => {
                self.phase = Phase::Disconnected;
                outs.push(Out::Notice(OnlineNotice::Error(error)));
            }
        }
    }

    fn reset_connection(&mut self) {
        self.phase = Phase::Disconnected;
        self.link = Link::Down;
        self.account = None;
        self.incoming.clear();
        self.outgoing = None;
        self.expecting = false;
        self.game = None;
        self.game_id = None;
        self.finished = None;
        self.premove.cancel();
        self.pending = None;
        self.opponent_gone = None;
        // Anything the old streams still say is dropped.
        let epoch = self.bump();
        self.account_epoch = epoch;
        self.game_epoch = epoch;
        self.seek_epoch = epoch;
        self.challenge_epoch = epoch;
    }

    fn disconnect(&mut self, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if self.phase.is_live() {
            return Self::reject(outs, RejectReason::GameInProgress);
        }
        let login = self
            .account
            .as_ref()
            .map_or_else(|| ctx.prefs.login.clone(), |a| a.login.clone());
        self.reset_connection();
        if login_is_valid(&login) {
            outs.push(Out::Command(OnlineCommand::Disconnect { login }));
        } else {
            outs.push(Out::Command(OnlineCommand::Shutdown));
        }
        outs.push(Out::Notice(OnlineNotice::SignedOut));
    }

    // ---- seeks and challenges ----

    fn can_begin(&self, ctx: &Ctx<'_>, outs: &mut Vec<Out>) -> bool {
        let refusal = if !self.phase.is_connected() {
            Some(RejectReason::NotConnected)
        } else if self.phase.is_live() {
            Some(RejectReason::GameInProgress)
        } else if !self.phase.can_start() {
            Some(RejectReason::WrongState)
        } else if ctx.busy {
            Some(RejectReason::Busy)
        } else if self.link != Link::Up {
            Some(RejectReason::EventsNotReady)
        } else {
            None
        };
        if let Some(reason) = refusal {
            Self::reject(outs, reason);
        }
        refusal.is_none()
    }

    fn seek(&mut self, request: &SeekRequest, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if !self.can_begin(ctx, outs) {
            return;
        }
        if validate_seek(request.minutes, request.increment).is_err() {
            return Self::reject(outs, RejectReason::TimeControl);
        }
        // A seek cannot fix the position of a Chess960 game.
        if matches!(request.variant, OnlineVariant::Chess960 { start: Some(_) }) {
            return Self::reject(outs, RejectReason::BadVariant);
        }
        self.phase = Phase::Seeking;
        self.expecting = true;
        self.search_ended = false;
        self.search_gen = self.search_gen.wrapping_add(1);
        let epoch = self.bump();
        self.seek_epoch = epoch;
        outs.push(Out::Notice(OnlineNotice::AssistanceWillBeDisabled));
        outs.push(Out::Command(OnlineCommand::Seek {
            epoch,
            request: request.clone(),
        }));
    }

    fn cancel_seek(&mut self, outs: &mut Vec<Out>) {
        if self.phase != Phase::Seeking {
            return;
        }
        self.phase = Phase::Idle;
        self.expecting = false;
        self.seek_epoch = self.bump();
        outs.push(Out::Command(OnlineCommand::CancelSeek));
    }

    fn challenge(&mut self, request: &ChallengeRequest, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if !self.can_begin(ctx, outs) {
            return;
        }
        if !login_is_valid(&request.username) {
            return Self::reject(outs, RejectReason::BadUser);
        }
        if !self.account.as_ref().is_some_and(|a| a.can_challenge) {
            return Self::reject(outs, RejectReason::NotAvailable);
        }
        if validate_challenge(request.limit_secs, request.increment).is_err() {
            return Self::reject(outs, RejectReason::TimeControl);
        }
        // A fixed Chess960 position is a number 0..=959 in a casual game.
        if let OnlineVariant::Chess960 { start: Some(n) } = request.variant
            && (n > 959 || request.rated)
        {
            return Self::reject(outs, RejectReason::BadVariant);
        }
        self.phase = Phase::Challenging;
        self.expecting = true;
        self.search_ended = false;
        self.search_gen = self.search_gen.wrapping_add(1);
        self.outgoing = None;
        let epoch = self.bump();
        self.challenge_epoch = epoch;
        outs.push(Out::Notice(OnlineNotice::AssistanceWillBeDisabled));
        outs.push(Out::Command(OnlineCommand::Challenge {
            epoch,
            request: request.clone(),
        }));
    }

    fn cancel_challenge(&mut self, outs: &mut Vec<Out>) {
        if self.phase != Phase::Challenging {
            return;
        }
        self.phase = Phase::Idle;
        self.expecting = false;
        self.challenge_epoch = self.bump();
        outs.push(Out::Command(OnlineCommand::CancelChallenge {
            id: self.outgoing.take(),
        }));
    }

    fn accept_challenge(&mut self, id: &str, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if !self.can_begin(ctx, outs) {
            return;
        }
        let playable = self.incoming.iter().any(|c| c.id == id && c.playable)
            && self.account.as_ref().is_some_and(|a| a.can_challenge);
        if !playable {
            return Self::reject(outs, RejectReason::ChallengeNotPlayable);
        }
        self.incoming.retain(|c| c.id != id);
        self.expecting = true;
        outs.push(Out::Notice(OnlineNotice::AssistanceWillBeDisabled));
        outs.push(Out::Command(OnlineCommand::AcceptChallenge {
            id: id.to_owned(),
        }));
    }

    fn decline_challenge(&mut self, id: &str, outs: &mut Vec<Out>) {
        let Some(found) = self.incoming.iter().find(|c| c.id == id) else {
            return Self::reject(outs, RejectReason::ChallengeNotPlayable);
        };
        let reason = found.decline_reason.to_owned();
        self.incoming.retain(|c| c.id != id);
        outs.push(Out::Command(OnlineCommand::DeclineChallenge {
            id: id.to_owned(),
            reason,
        }));
    }

    // ---- the streams ----

    fn stream(&mut self, update: StreamUpdate, outs: &mut Vec<Out>) {
        let StreamUpdate { epoch, at, kind } = update;
        match kind {
            StreamKind::Account(message) if epoch == self.account_epoch => {
                self.account_stream(message, at, outs);
            }
            StreamKind::Game { id, message }
                if epoch == self.game_epoch && self.game_id.as_deref() == Some(id.as_str()) =>
            {
                self.game_stream(message, at, outs);
            }
            StreamKind::Seek(outcome) if epoch == self.seek_epoch => {
                if self.phase != Phase::Seeking {
                    return;
                }
                if let SeekOutcome::Failed(error) = outcome {
                    self.phase = Phase::Idle;
                    self.expecting = false;
                    outs.push(Out::Notice(OnlineNotice::Error(OnlineError::from(&error))));
                } else {
                    // Paired: the `gameStart` must come; if it does not,
                    // the watchdog gives the session back (D1366).
                    self.arm_search_watchdog(outs);
                }
            }
            StreamKind::Challenge(message) if epoch == self.challenge_epoch => {
                self.challenge_stream(message, outs);
            }
            StreamKind::Watch { id, message }
                if epoch == self.watch_epoch && self.watch.as_ref().is_some_and(|w| w.id == id) =>
            {
                self.watch_stream(message, outs);
            }
            _ => {}
        }
    }

    fn account_stream(&mut self, message: StreamMessage<Event>, at: Instant, outs: &mut Vec<Out>) {
        match message {
            StreamMessage::Connected => {
                self.link = Link::Up;
                self.arm_grace(outs);
            }
            StreamMessage::Reconnecting { .. } => self.link = Link::Reconnecting,
            StreamMessage::Item(event) => self.account_event(event, at, outs),
            StreamMessage::Ended => self.event_stream_lost(None, outs),
            StreamMessage::Failed(error) => self.event_stream_lost(Some(&error), outs),
            _ => {}
        }
    }

    fn event_stream_lost(&mut self, error: Option<&mychess_online::Error>, outs: &mut Vec<Out>) {
        self.link = Link::Down;
        if self.phase.is_live() {
            // The game goes on through its own stream.
            return;
        }
        if let Some(error) = error {
            outs.push(Out::Notice(OnlineNotice::Error(OnlineError::from(error))));
        }
        self.reset_connection();
        outs.push(Out::Command(OnlineCommand::Shutdown));
        outs.push(Out::Notice(OnlineNotice::Disconnected));
    }

    fn account_event(&mut self, event: Event, at: Instant, outs: &mut Vec<Out>) {
        match event {
            Event::GameStart(info) => self.game_start(&info, at, outs),
            Event::GameFinish(info) => {
                if self.marker.as_ref().is_some_and(|m| m.game_id == info.id)
                    && !self.phase.is_live()
                {
                    self.clear_marker(outs);
                }
                // The game stream may have missed its end: ask once more
                // what the server says (D1365).
                let current = self.game_id.as_deref() == Some(info.id.as_str());
                let over = self.game.as_ref().is_some_and(OnlineGame::is_over);
                if self.phase.is_live() && current && !over && !self.final_fetch {
                    self.final_fetch = true;
                    self.open_game(outs);
                }
            }
            Event::Challenge(challenge) => self.challenge_received(&challenge, outs),
            Event::ChallengeCanceled(challenge) => {
                self.incoming.retain(|c| c.id != challenge.id);
            }
            Event::ChallengeDeclined(challenge) => {
                self.incoming.retain(|c| c.id != challenge.id);
                if self.phase == Phase::Challenging {
                    self.phase = Phase::Idle;
                    self.expecting = false;
                    self.outgoing = None;
                    outs.push(Out::Notice(OnlineNotice::ChallengeDeclined {
                        reason: challenge.decline_reason_key.clone(),
                    }));
                }
            }
            _ => {}
        }
    }

    fn challenge_received(&mut self, challenge: &Challenge, outs: &mut Vec<Out>) {
        if challenge.direction != Some(Direction::In) {
            return;
        }
        let bot = challenge
            .challenger
            .as_ref()
            .is_some_and(|p| p.title.as_deref() == Some("BOT"));
        if bot {
            outs.push(Out::Command(OnlineCommand::DeclineChallenge {
                id: challenge.id.clone(),
                reason: "noBot".to_owned(),
            }));
            return;
        }
        if self.incoming.iter().any(|c| c.id == challenge.id) {
            return;
        }
        let standard = challenge
            .variant
            .as_ref()
            .is_none_or(mychess_online::models::Variant::is_playable);
        let chess960 = challenge
            .variant
            .as_ref()
            .is_some_and(mychess_online::models::Variant::is_chess960);
        let playable = challenge.is_playable();
        let decline_reason = if !standard {
            "variant"
        } else if !playable {
            "timeControl"
        } else {
            "generic"
        };
        let who = challenge.challenger.as_ref();
        self.incoming.push(IncomingChallenge {
            id: challenge.id.clone(),
            from: who
                .and_then(|p| p.name.as_deref())
                .map(|n| clean_text(n, MAX_NAME_CHARS))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "?".to_owned()),
            title: who
                .and_then(|p| p.title.as_deref())
                .map(|t| clean_text(t, MAX_LABEL_CHARS))
                .filter(|t| !t.is_empty()),
            rating: who.and_then(|p| p.rating).filter(|r| *r > 0),
            rated: challenge.rated,
            time: challenge
                .time_control
                .as_ref()
                .and_then(|t| t.show.as_deref())
                .map(|t| clean_text(t, MAX_LABEL_CHARS))
                .unwrap_or_default(),
            playable,
            decline_reason,
            chess960,
        });
    }

    fn challenge_stream(
        &mut self,
        message: StreamMessage<mychess_online::models::ChallengeUpdate>,
        outs: &mut Vec<Out>,
    ) {
        use mychess_online::models::ChallengeUpdate;
        match message {
            StreamMessage::Item(ChallengeUpdate::Challenge(challenge)) => {
                if self.phase == Phase::Challenging {
                    self.outgoing = Some(challenge.id.clone());
                }
            }
            StreamMessage::Item(ChallengeUpdate::Done(reason)) => {
                if self.phase != Phase::Challenging {
                    return;
                }
                if reason == "accepted" {
                    // The game starts through the event stream; if it does
                    // not, the watchdog gives the session back (D1366).
                    self.arm_search_watchdog(outs);
                } else {
                    self.phase = Phase::Idle;
                    self.expecting = false;
                    self.outgoing = None;
                    outs.push(Out::Notice(OnlineNotice::ChallengeDeclined {
                        reason: None,
                    }));
                }
            }
            StreamMessage::Ended if self.phase == Phase::Challenging && !self.search_ended => {
                // The stream closed without saying why: a failure (D1366).
                self.phase = Phase::Idle;
                self.expecting = false;
                self.outgoing = None;
                outs.push(Out::Notice(OnlineNotice::Error(OnlineError::Network)));
            }
            StreamMessage::Failed(error) if self.phase == Phase::Challenging => {
                self.phase = Phase::Idle;
                self.expecting = false;
                self.outgoing = None;
                outs.push(Out::Notice(OnlineNotice::Error(OnlineError::from(&error))));
            }
            _ => {}
        }
    }

    fn arm_search_watchdog(&mut self, outs: &mut Vec<Out>) {
        self.search_ended = true;
        self.search_gen = self.search_gen.wrapping_add(1);
        outs.push(Out::Command(OnlineCommand::Retry {
            kind: RetryKind::Search,
            generation: self.search_gen,
            delay: SEARCH_WATCHDOG,
        }));
    }

    // ---- the marker (D1332) ----

    fn arm_grace(&mut self, outs: &mut Vec<Out>) {
        if self.marker.is_some() && !self.phase.is_live() {
            self.grace = self.grace.wrapping_add(1);
            outs.push(Out::Command(OnlineCommand::Grace {
                generation: self.grace,
                delay: MARKER_GRACE,
            }));
        }
    }

    fn marker_grace(&mut self, generation: u64, outs: &mut Vec<Out>) {
        if generation == self.grace && self.marker.is_some() && !self.phase.is_live() {
            self.clear_marker(outs);
            outs.push(Out::Notice(OnlineNotice::MarkerCleared));
        }
    }

    fn clear_marker(&mut self, outs: &mut Vec<Out>) {
        self.marker = None;
        if let Some(out) = self.marker_command(|seq, dir| OnlineCommand::RemoveMarker { seq, dir })
        {
            outs.push(out);
        }
    }

    fn remove_lock(&mut self, confirmed: bool, outs: &mut Vec<Out>) {
        if !confirmed {
            return;
        }
        if !self.can_remove_lock() {
            if self.assist_locked() {
                Self::reject(outs, RejectReason::NotAvailable);
            }
            return;
        }
        self.released = true;
        self.clear_marker(outs);
        outs.push(Out::Notice(OnlineNotice::LockReleased));
    }

    // ---- the game ----

    fn game_start(&mut self, info: &GameInfo, at: Instant, outs: &mut Vec<Out>) {
        let id = info.id.clone();
        let valid = (8..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric());
        if !valid {
            return;
        }
        if self.phase.is_live() {
            if self.game_id.as_deref() != Some(id.as_str()) {
                outs.push(Out::Notice(OnlineNotice::AnotherGame));
            }
            return;
        }
        if !self.phase.is_connected() {
            return;
        }
        let resumed = !self.expecting;
        self.expecting = false;
        // A game of the user starts: the watch makes way (D3001).
        self.stop_watch(outs);
        self.game_id = Some(id.clone());
        self.game = None;
        self.released = false;
        self.finished = None;
        self.premove.cancel();
        self.pending = None;
        self.opponent_gone = None;
        self.ping = None;
        self.outgoing = None;
        self.reopens = 0;
        self.game_retries = 0;
        self.final_fetch = false;
        self.retry_gen = self.retry_gen.wrapping_add(1);
        self.phase = Phase::Playing(PlayingState::Live);
        let epoch = self.bump();
        self.game_epoch = epoch;
        // The streams of the search are over.
        self.seek_epoch = self.bump();
        self.challenge_epoch = self.bump();
        outs.push(Out::Starting { resumed, at });
        outs.push(Out::Command(OnlineCommand::OpenGame {
            epoch,
            id: id.clone(),
        }));
        if let Some(out) = self.marker_command(|seq, dir| OnlineCommand::WriteMarker {
            seq,
            dir,
            game_id: id,
        }) {
            outs.push(out);
        }
        if resumed {
            outs.push(Out::Notice(OnlineNotice::Resumed));
        }
    }

    fn game_stream(&mut self, message: StreamMessage<GameEvent>, at: Instant, outs: &mut Vec<Out>) {
        match message {
            StreamMessage::Connected => {
                if self.phase == Phase::Playing(PlayingState::Reconnecting) {
                    self.phase = Phase::Playing(PlayingState::Live);
                }
            }
            StreamMessage::Reconnecting { .. } => {
                if self.phase == Phase::Playing(PlayingState::Live) {
                    self.phase = Phase::Playing(PlayingState::Reconnecting);
                }
            }
            StreamMessage::Item(GameEvent::Full(full)) => self.game_full(&full, at, outs),
            StreamMessage::Item(GameEvent::State(state)) => self.game_state(&state, at, outs),
            StreamMessage::Item(GameEvent::OpponentGone {
                gone,
                claim_win_in_seconds,
            }) => {
                self.opponent_gone = gone.then_some(OpponentGone {
                    claim_secs: claim_win_in_seconds,
                    since: at,
                });
            }
            StreamMessage::Ended => {
                let over = self.game.as_ref().is_some_and(OnlineGame::is_over);
                if !over && self.phase.is_live() {
                    self.reopen(outs);
                }
            }
            StreamMessage::Failed(error) if self.phase.is_live() => {
                // The server is the truth and cannot be reached: the lock
                // stays, the stream is opened again now and then, and when
                // that runs out the game is closed here (D1365).
                self.phase = Phase::Playing(PlayingState::Reconnecting);
                outs.push(Out::Notice(OnlineNotice::Error(OnlineError::from(&error))));
                self.schedule_game_retry(outs);
            }
            _ => {}
        }
    }

    /// Closes and reopens the stream of the game; the new `gameFull`
    /// resynchronises everything (D1318, D1333).
    fn reopen(&mut self, outs: &mut Vec<Out>) {
        if self.game_id.is_none() {
            return;
        }
        if self.reopens >= MAX_REOPENS {
            self.phase = Phase::Playing(PlayingState::Reconnecting);
            outs.push(Out::Notice(OnlineNotice::Desynced));
            self.schedule_game_retry(outs);
            return;
        }
        self.reopens += 1;
        self.phase = Phase::Playing(PlayingState::Reconnecting);
        self.open_game(outs);
    }

    /// Opens the stream of the current game again; whatever the old stream
    /// still says is dropped.
    fn open_game(&mut self, outs: &mut Vec<Out>) {
        let Some(id) = self.game_id.clone() else {
            return;
        };
        let epoch = self.bump();
        self.game_epoch = epoch;
        outs.push(Out::Command(OnlineCommand::OpenGame { epoch, id }));
    }

    /// Arms the timer that opens the game stream again, or closes the game
    /// here when the attempts ran out (D1365).
    fn schedule_game_retry(&mut self, outs: &mut Vec<Out>) {
        if self.game_retries >= MAX_GAME_RETRIES {
            self.abandon(OnlineNotice::GameLost, outs);
            return;
        }
        self.game_retries += 1;
        self.retry_gen = self.retry_gen.wrapping_add(1);
        outs.push(Out::Command(OnlineCommand::Retry {
            kind: RetryKind::Game,
            generation: self.retry_gen,
            delay: GAME_RETRY_DELAY,
        }));
    }

    fn retry(&mut self, kind: RetryKind, generation: u64, outs: &mut Vec<Out>) {
        match kind {
            RetryKind::Game => {
                if generation == self.retry_gen
                    && self.phase == Phase::Playing(PlayingState::Reconnecting)
                {
                    self.open_game(outs);
                    // A stream that never answers is tried again too.
                    self.schedule_game_retry(outs);
                }
            }
            RetryKind::Search => {
                if generation != self.search_gen {
                    return;
                }
                let command = match self.phase {
                    Phase::Seeking => OnlineCommand::CancelSeek,
                    Phase::Challenging => OnlineCommand::CancelChallenge { id: None },
                    _ => return,
                };
                self.phase = Phase::Idle;
                self.expecting = false;
                self.outgoing = None;
                self.search_ended = false;
                outs.push(Out::Command(command));
                outs.push(Out::Notice(OnlineNotice::SearchTimedOut));
            }
        }
    }

    /// After the game left the session: a dead event stream is closed
    /// like it would have been outside a game, keeping the token (D1367).
    fn leave_dead_link(&mut self, outs: &mut Vec<Out>) {
        if self.link == Link::Down && self.phase != Phase::Disconnected {
            self.reset_connection();
            outs.push(Out::Command(OnlineCommand::Shutdown));
            outs.push(Out::Notice(OnlineNotice::Disconnected));
        }
    }

    fn abandon(&mut self, notice: OnlineNotice, outs: &mut Vec<Out>) {
        self.phase = Phase::Idle;
        self.game = None;
        self.game_id = None;
        outs.push(Out::Command(OnlineCommand::CloseGame));
        if self.marker.is_some() || self.marker_dir.is_some() {
            self.clear_marker(outs);
        }
        outs.push(Out::Notice(notice));
        self.retry_gen = self.retry_gen.wrapping_add(1);
        self.leave_dead_link(outs);
    }

    fn game_full(&mut self, full: &GameFull, at: Instant, outs: &mut Vec<Out>) {
        let Some(me) = self.account.as_ref().map(|a| a.id.clone()) else {
            return;
        };
        if self.game.as_ref().is_some_and(|g| g.id == full.id) {
            return self.game_refull(full, at, outs);
        }
        match OnlineGame::from_full(full, &me) {
            Ok(game) => {
                self.game = Some(game);
                self.game_retries = 0;
                self.phase = Phase::Playing(PlayingState::Live);
                outs.push(Out::Opened);
                self.after_state(at, outs);
            }
            Err(StartError::UnsupportedVariant) => {
                self.abandon(OnlineNotice::UnsupportedVariant, outs);
            }
            Err(_) => self.abandon(OnlineNotice::Error(OnlineError::Protocol), outs),
        }
    }

    /// A second `gameFull` of the same game (a reconnection): everything is
    /// reconciled with what the server says; a move of the user the server
    /// has not seen is sent again if it is still his turn (D1333).
    fn game_refull(&mut self, full: &GameFull, at: Instant, outs: &mut Vec<Out>) {
        let pending = self.pending.take();
        let Some(game) = self.game.as_mut() else {
            return;
        };
        match game.apply_state(&full.state) {
            Ok(applied) => {
                self.game_retries = 0;
                self.phase = Phase::Playing(PlayingState::Live);
                if applied.rewritten {
                    self.premove.cancel();
                }
                outs.push(Out::Changed);
                self.after_state(at, outs);
                if let Some(p) = pending
                    && self
                        .game
                        .as_ref()
                        .is_some_and(|g| !g.is_over() && g.my_turn() && g.plies() + 1 == p.ply)
                {
                    self.send_move(p.mv, false, at, outs);
                }
            }
            Err(_) => self.reopen(outs),
        }
    }

    fn game_state(&mut self, state: &GameState, at: Instant, outs: &mut Vec<Out>) {
        let pending = self.pending.clone();
        let Some(game) = self.game.as_mut() else {
            return;
        };
        if let Ok(applied) = game.apply_state(state) {
            self.reopens = 0;
            self.game_retries = 0;
            self.reconcile_pending(pending, applied, at);
            outs.push(Out::Changed);
            self.after_state(at, outs);
        } else {
            outs.push(Out::Notice(OnlineNotice::Resynchronised));
            self.reopen(outs);
        }
    }

    /// The move of the user the server has not mentioned yet stays on the
    /// board; one it confirmed measures the latency; a takeback drops it.
    fn reconcile_pending(&mut self, pending: Option<PendingMove>, applied: Applied, at: Instant) {
        let Some(p) = pending else {
            if applied.rewritten {
                self.premove.cancel();
            }
            return;
        };
        if applied.rewritten && applied.before == p.ply && applied.after + 1 == p.ply {
            // The state predates the move: keep it on the board.
            if let Some(game) = self.game.as_mut() {
                let _ = game.push_local(p.mv);
            }
            return;
        }
        if applied.rewritten {
            self.premove.cancel();
            self.pending = None;
        } else if applied.after >= p.ply {
            self.ping = Some(at.saturating_duration_since(p.sent_at));
            self.pending = None;
        }
    }

    fn after_state(&mut self, at: Instant, outs: &mut Vec<Out>) {
        self.sync_clock(at);
        let over = self.game.as_ref().is_some_and(OnlineGame::is_over);
        if over {
            self.finish(outs);
        } else {
            self.try_premove(at, outs);
        }
    }

    fn sync_clock(&mut self, at: Instant) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        // The clocks run after the second ply (D: Lichess starts them then).
        // The confirmed position decides: a move of the user that waits for
        // the server must not flip the clock to the other side (D1364).
        let running = !game.is_over() && game.confirmed_plies() >= 2;
        self.clock.sync(
            ms(game.wtime),
            ms(game.btime),
            game.confirmed_turn(),
            running,
            at,
        );
    }

    fn try_premove(&mut self, at: Instant, outs: &mut Vec<Out>) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        if !self.premove.is_set() || !game.my_turn() || game.has_pending_move() || game.is_over() {
            return;
        }
        // Either way the slot ends empty: an illegal premove is dropped.
        if let Some(mv) = self.premove.take_if_legal(game.position()) {
            self.send_move(mv, false, at, outs);
        }
    }

    fn finish(&mut self, outs: &mut Vec<Out>) {
        if self.phase == Phase::Playing(PlayingState::Finished) {
            return;
        }
        self.phase = Phase::Playing(PlayingState::Finished);
        self.finished = self.game.as_ref().and_then(OnlineGame::outcome);
        self.premove.cancel();
        self.pending = None;
        self.opponent_gone = None;
        outs.push(Out::Command(OnlineCommand::CloseGame));
        if self.marker.is_some() || self.marker_dir.is_some() {
            self.clear_marker(outs);
        }
        outs.push(Out::Notice(OnlineNotice::GameOver));
        outs.push(Out::Finished);
    }

    fn dismiss(&mut self, outs: &mut Vec<Out>) {
        if self.phase != Phase::Playing(PlayingState::Finished) {
            return Self::reject(outs, RejectReason::WrongState);
        }
        self.phase = Phase::Idle;
        self.game = None;
        self.game_id = None;
        self.finished = None;
        outs.push(Out::Dismissed);
        self.leave_dead_link(outs);
    }

    // ---- moves and actions ----

    #[allow(clippy::too_many_arguments)]
    fn player_move(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<mychess_core::Role>,
        offer_draw: bool,
        at: Instant,
        ctx: &Ctx<'_>,
        outs: &mut Vec<Out>,
    ) {
        let live = self.phase.is_live();
        let Some(game) = self.game.as_ref().filter(|g| live && !g.is_over()) else {
            return Self::reject(outs, RejectReason::NoGame);
        };
        if !game.my_turn() {
            // `enter` also settles the promotion: none chosen means a queen
            // (D1369).
            let entered = if ctx.prefs.premove {
                Premove::enter(game.position(), game.my_color, from, to, promotion)
            } else {
                None
            };
            if let Some(premove) = entered {
                self.premove.set(premove);
                outs.push(Out::Changed);
            } else {
                Self::reject(outs, RejectReason::NotYourTurn);
            }
            return;
        }
        if game.has_pending_move() {
            return Self::reject(outs, RejectReason::MovePending);
        }
        let Some(mv) = game.position().find_move(from, to, promotion) else {
            return Self::reject(outs, RejectReason::IllegalMove);
        };
        self.premove.cancel();
        self.send_move(mv, offer_draw, at, outs);
    }

    /// Plays `mv` on the board and sends it (D1320).
    fn send_move(&mut self, mv: Move, offer_draw: bool, at: Instant, outs: &mut Vec<Out>) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        if game.push_local(mv).is_err() {
            return Self::reject(outs, RejectReason::IllegalMove);
        }
        let Some(uci) = game.last_uci() else {
            return;
        };
        let ply = game.plies();
        let id = game.id.clone();
        self.pending = Some(PendingMove {
            mv,
            ply,
            sent_at: at,
        });
        outs.push(Out::Command(OnlineCommand::Move {
            id,
            uci,
            offer_draw,
            ply,
        }));
        outs.push(Out::Changed);
    }

    fn game_command(&mut self, action: ActionKind, outs: &mut Vec<Out>) {
        let live = self.phase.is_live();
        let Some(game) = self.game.as_ref().filter(|g| live && !g.is_over()) else {
            return Self::reject(outs, RejectReason::NoGame);
        };
        let own_draw_pending = match game.my_color {
            Color::White => game.wdraw,
            Color::Black => game.bdraw,
        };
        let allowed = match action {
            ActionKind::Resign => game.plies() >= 2,
            ActionKind::Abort => game.plies() < 2,
            ActionKind::DrawYes if !self.draw_offered_to_me() => !own_draw_pending,
            ActionKind::TakebackPropose => self.can_propose_takeback(),
            _ => true,
        };
        if !allowed {
            return Self::reject(outs, RejectReason::NotAvailable);
        }
        outs.push(Out::Command(OnlineCommand::Game {
            id: game.id.clone(),
            action,
        }));
    }

    // ---- rematch (D3012) ----

    fn rematch(&mut self, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if !self.can_rematch() {
            return Self::reject(outs, RejectReason::NotAvailable);
        }
        let Some(game) = self.game.as_ref() else {
            return Self::reject(outs, RejectReason::NoGame);
        };
        let (opponent, color) = match game.my_color {
            Color::White => (&game.black, OnlineColor::Black),
            Color::Black => (&game.white, OnlineColor::White),
        };
        let (Some(username), Some((initial, increment))) = (opponent.id.clone(), game.clock) else {
            return Self::reject(outs, RejectReason::NotAvailable);
        };
        // The clocks of Lichess are whole seconds (the challenge API takes
        // seconds); a fraction would be lost, so it is refused instead.
        if initial % 1000 != 0 || increment % 1000 != 0 {
            return Self::reject(outs, RejectReason::TimeControl);
        }
        let (Ok(limit_secs), Ok(increment)) = (
            u32::try_from(initial / 1000),
            u32::try_from(increment / 1000),
        ) else {
            return Self::reject(outs, RejectReason::TimeControl);
        };
        let request = ChallengeRequest {
            username,
            limit_secs,
            increment,
            rated: game.rated,
            color,
            variant: OnlineVariant::Standard,
        };
        self.challenge(&request, ctx, outs);
        if self.phase == Phase::Challenging {
            outs.push(Out::Notice(OnlineNotice::RematchSent));
        }
    }

    // ---- login in the browser (D3005, D3009) ----

    fn start_oauth(&mut self, store: StoreChoice, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        if !self.phase.can_connect() {
            return Self::reject(outs, RejectReason::WrongState);
        }
        if !base_url_is_acceptable(&ctx.prefs.base_url) {
            outs.push(Out::Notice(OnlineNotice::Error(OnlineError::InsecureUrl)));
            return;
        }
        self.phase = Phase::Connecting;
        let epoch = self.bump();
        self.account_epoch = epoch;
        self.oauth_epoch = epoch;
        self.oauth = Some(OAuthWait {
            url: None,
            opened: false,
        });
        outs.push(Out::Command(OnlineCommand::OAuthLogin {
            epoch,
            base_url: ctx.prefs.base_url.clone(),
            store,
        }));
    }

    fn cancel_oauth(&mut self, outs: &mut Vec<Out>) {
        if self.oauth.take().is_none() {
            return;
        }
        self.phase = Phase::Disconnected;
        let epoch = self.bump();
        self.oauth_epoch = epoch;
        self.account_epoch = epoch;
        outs.push(Out::Command(OnlineCommand::CancelOAuth));
    }

    fn oauth_done(
        &mut self,
        epoch: u64,
        result: Result<(), OnlineError>,
        store: StoreChoice,
        ctx: &Ctx<'_>,
        outs: &mut Vec<Out>,
    ) {
        if epoch != self.oauth_epoch || self.oauth.take().is_none() {
            return;
        }
        self.phase = Phase::Disconnected;
        match result {
            // The token waits in the hub: the normal connection takes it.
            Ok(()) => self.connect(
                ConnectRequest {
                    source: TokenSource::Staged,
                    store,
                },
                ctx,
                outs,
            ),
            Err(error) => outs.push(Out::Notice(OnlineNotice::Error(error))),
        }
    }

    // ---- watching a game of others (D3000-D3004) ----

    fn start_watch(&mut self, text: &str, ctx: &Ctx<'_>, outs: &mut Vec<Out>) {
        let Some(id) = mychess_online::parse_game_ref(text) else {
            return Self::reject(outs, RejectReason::BadGameRef);
        };
        // The game of the user (live, or one the marker says may be on) is
        // never watched: the lock of D1330 is for exactly that (D3001).
        let own = self.game_id.as_deref() == Some(id.as_str())
            || self.marker.as_ref().is_some_and(|m| m.game_id == id);
        if own && (self.phase.is_live() || self.marker.is_some()) {
            return Self::reject(outs, RejectReason::OwnGame);
        }
        if self.assist_locked() || self.phase.is_busy() {
            return Self::reject(outs, RejectReason::GameInProgress);
        }
        if !matches!(self.phase, Phase::Disconnected | Phase::Idle) {
            return Self::reject(outs, RejectReason::WrongState);
        }
        if ctx.busy {
            return Self::reject(outs, RejectReason::Busy);
        }
        if !base_url_is_acceptable(&ctx.prefs.base_url) {
            outs.push(Out::Notice(OnlineNotice::Error(OnlineError::InsecureUrl)));
            return;
        }
        let epoch = self.bump();
        self.watch_epoch = epoch;
        self.watch = Some(Watch {
            id: id.clone(),
            base_url: ctx.prefs.base_url.clone(),
            phase: WatchPhase::Opening,
            game: None,
            first_fen: None,
            buffered: Vec::new(),
            pgn_done: false,
            dropped_steps: false,
        });
        outs.push(Out::Command(OnlineCommand::OpenWatch {
            epoch,
            base_url: ctx.prefs.base_url.clone(),
            id,
        }));
        outs.push(Out::Notice(OnlineNotice::Watching));
    }

    fn stop_watch(&mut self, outs: &mut Vec<Out>) {
        if self.watch.take().is_none() {
            return;
        }
        self.watch_epoch = self.bump();
        outs.push(Out::Command(OnlineCommand::CloseWatch));
        outs.push(Out::WatchClosed);
    }

    fn watch_stream(&mut self, message: StreamMessage<WatchEvent>, outs: &mut Vec<Out>) {
        match message {
            StreamMessage::Item(WatchEvent::Description(desc)) => {
                let Some(watch) = self.watch.as_mut() else {
                    return;
                };
                if let Some(game) = watch.game.as_mut() {
                    game.finish_with(&desc);
                    if desc.is_final() {
                        watch.phase = WatchPhase::Finished;
                        outs.push(Out::Notice(OnlineNotice::WatchEnded));
                    }
                    outs.push(Out::WatchChanged);
                    return;
                }
                match WatchedGame::from_description(&desc) {
                    Ok(game) => {
                        watch.first_fen = desc.fen.clone().filter(|fen| fen.len() <= MAX_FEN_CHARS);
                        watch.phase = if game.is_over() {
                            WatchPhase::Finished
                        } else {
                            WatchPhase::Live
                        };
                        let (id, base_url) = (watch.id.clone(), watch.base_url.clone());
                        watch.game = Some(game);
                        outs.push(Out::WatchOpened);
                        outs.push(Out::Command(OnlineCommand::FetchPgn {
                            epoch: self.watch_epoch,
                            base_url,
                            id,
                        }));
                    }
                    Err(StartError::UnsupportedVariant) => {
                        self.watch = None;
                        outs.push(Out::Command(OnlineCommand::CloseWatch));
                        outs.push(Out::Notice(OnlineNotice::UnsupportedVariant));
                    }
                    Err(_) => {
                        self.watch = None;
                        outs.push(Out::Command(OnlineCommand::CloseWatch));
                        outs.push(Out::Notice(OnlineNotice::Error(OnlineError::Protocol)));
                    }
                }
            }
            StreamMessage::Item(WatchEvent::Move(step)) => self.watch_step(step, outs),
            StreamMessage::Ended => {
                if let Some(watch) = self.watch.as_mut()
                    && watch.phase != WatchPhase::Finished
                {
                    watch.phase = WatchPhase::Finished;
                    outs.push(Out::Notice(OnlineNotice::WatchEnded));
                }
            }
            StreamMessage::Failed(error) => {
                if let Some(watch) = self.watch.as_mut() {
                    watch.phase = WatchPhase::Finished;
                }
                outs.push(Out::Command(OnlineCommand::CloseWatch));
                outs.push(Out::Notice(OnlineNotice::Error(OnlineError::from(&error))));
            }
            _ => {}
        }
    }

    fn watch_step(&mut self, step: WatchMove, outs: &mut Vec<Out>) {
        let Some(watch) = self.watch.as_mut() else {
            return;
        };
        if watch.game.is_none() {
            return;
        }
        // A position is at most ~90 characters; a longer line is not from
        // Lichess and is not kept (D3115).
        if step.fen.len() > MAX_FEN_CHARS {
            return;
        }
        if !watch.pgn_done {
            if watch.buffered.len() < MAX_BUFFERED_STEPS {
                watch.buffered.push(step);
            } else if !watch.dropped_steps {
                // Said once: the later moves will not follow and the game
                // restarts as incomplete (D3003).
                watch.dropped_steps = true;
                outs.push(Out::Notice(OnlineNotice::WatchStepsDropped));
            }
            return;
        }
        Self::follow_step(watch, &step, outs);
    }

    fn follow_step(watch: &mut Watch, step: &WatchMove, outs: &mut Vec<Out>) {
        let Some(game) = watch.game.as_mut() else {
            return;
        };
        match game.follow(step) {
            Followed::Same => {}
            Followed::Added => outs.push(Out::WatchChanged),
            Followed::Restarted => {
                outs.push(Out::Notice(OnlineNotice::WatchIncomplete));
                outs.push(Out::WatchChanged);
            }
        }
    }

    fn watch_pgn(&mut self, epoch: u64, export: Option<ParsedExport>, outs: &mut Vec<Out>) {
        if epoch != self.watch_epoch {
            return;
        }
        let Some(watch) = self.watch.as_mut() else {
            return;
        };
        if watch.pgn_done {
            return;
        }
        watch.pgn_done = true;
        let used = match (watch.game.as_mut(), export) {
            (Some(game), Some(export)) => game.apply_export(export),
            _ => false,
        };
        if !used && let Some(fen) = watch.first_fen.take() {
            // No history: start from where the stream began.
            Self::follow_step(
                watch,
                &WatchMove {
                    fen,
                    last_move: None,
                    white_clock: 0,
                    black_clock: 0,
                },
                outs,
            );
        }
        for step in std::mem::take(&mut watch.buffered) {
            Self::follow_step(watch, &step, outs);
        }
        outs.push(Out::WatchChanged);
    }

    fn done(&mut self, done: &ActionDone, outs: &mut Vec<Out>) {
        let Err(failure) = &done.result else {
            return;
        };
        match done.action {
            ActionKind::Move => {
                let matches_pending = self
                    .pending
                    .as_ref()
                    .is_some_and(|p| Some(p.ply) == done.ply);
                if !matches_pending {
                    return;
                }
                self.pending = None;
                if failure.ambiguous {
                    outs.push(Out::Notice(OnlineNotice::MoveUncertain));
                    // A fresh `gameFull` settles whether the server took it
                    // (D1368).
                    self.reopen(outs);
                    return;
                }
                self.premove.cancel();
                if let Some(game) = self.game.as_mut() {
                    game.undo_local();
                }
                outs.push(Out::Changed);
                outs.push(Out::Notice(match &failure.error {
                    OnlineError::Rejected { reason } => OnlineNotice::MoveRejected {
                        reason: reason.clone(),
                    },
                    other => OnlineNotice::Error(other.clone()),
                }));
            }
            ActionKind::AcceptChallenge => {
                self.expecting = false;
                outs.push(Out::Notice(OnlineNotice::Error(failure.error.clone())));
            }
            _ => outs.push(Out::Notice(OnlineNotice::Error(failure.error.clone()))),
        }
    }
}
