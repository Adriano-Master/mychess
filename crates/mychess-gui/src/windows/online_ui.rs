//! The visual state of the online play on Lichess and the events of its
//! dialogs and panel (spec M8-3c, D1470-D1499).
//!
//! The application owns the session (`mychess_app::online`); what is here is
//! what the screen needs: the forms being edited, the box that asks for a
//! confirmation, the strips with the clocks and the rule that tells the
//! board which premoves it may take. [`OnlineUi::update`] never blocks and
//! never touches the network: it returns the messages to send in an
//! [`Outcome`]. The clipboard, the premove entry and the quit are the shell's
//! business (`shell/online_glue.rs`).
//!
//! **The token** is typed into a [`Secret`], whose `Debug` is redacted, so a
//! message, a log line or a failed assertion never shows it (D1471). On
//! submit it is handed to the hub of the application (`stage_token`) and the
//! field is emptied: the text lives in no message and no state.

use std::fmt;
use std::time::{Duration, Instant};

use mychess_app::online::{
    ChallengeRequest, ConnectRequest, OnlineColor, OnlineError, OnlineMessage, OnlineNotice,
    OnlinePhase, OnlinePrefs, OnlineVariant, PlayingState, SeekRequest, StoreChoice, TokenSource,
    is_pseudo_legal,
};
use mychess_app::{AppState, Message as AppMessage, PrefChange, PrefsMessage};
use mychess_core::{Role, Square, ZobristHash};

use crate::app_api;
use crate::widgets::board::premove::PremoveEvent;
use crate::windows::dialog_ui::Outcome;
use crate::windows::play_dialog::shown_seconds;

/// Id of the token field.
pub const ONLINE_TOKEN_ID: &str = "online-token";
/// Id of the user field of the challenge form.
pub const ONLINE_USER_ID: &str = "online-challenge-user";
/// Id of the minutes field of the seek form.
pub const ONLINE_MINUTES_ID: &str = "online-seek-minutes";
/// Id of the field of the game to watch.
pub const ONLINE_WATCH_ID: &str = "online-watch-input";

/// The page where a personal token with the scopes the app needs is created
/// (spec D1305). The token itself is never part of an address.
pub const TOKEN_PAGE: &str = "https://lichess.org/account/oauth/token/create?scopes[]=board:play&scopes[]=challenge:read&scopes[]=challenge:write&description=MyChess";

/// The time controls offered as buttons (minutes, increment in seconds).
pub const PRESETS: [(u32, u32); 5] = [(10, 0), (10, 5), (15, 10), (30, 0), (30, 20)];

/// The longest user name the form takes.
const MAX_USER_CHARS: usize = 30;
/// The longest text taken for the game to watch (a link).
const MAX_WATCH_CHARS: usize = 200;

/// A piece of text that must not leak: `Debug` shows nothing of it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps `text`.
    pub fn new(text: impl Into<String>) -> Secret {
        Secret(text.into())
    }

    /// The text (only to give it to the hub or to the field).
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether nothing was typed (spaces do not count).
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// Forgets the text.
    pub fn clear(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl Drop for Secret {
    /// Overwrites the text before the memory goes back to the allocator
    /// (best effort).
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// The window the user opened, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineDialog {
    /// "Connect to Lichess": the token.
    Connect,
    /// "Find a game": the seek.
    Seek,
    /// "Challenge a user".
    Challenge,
    /// The challenges received.
    Incoming,
    /// "Watch a game": a link or an id (spec D3000).
    Watch,
}

/// The questions the online play asks before it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineConfirm {
    /// "Resign the game?"
    Resign,
    /// "Remove the lock of the assistance?" (D1332).
    RemoveLock,
    /// "A game is on: quit anyway?"
    Quit,
}

/// What is on top, for the dialog layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineActive {
    /// The connection window.
    Connect,
    /// The seek window.
    Seek,
    /// The challenge window.
    Challenge,
    /// The list of challenges received.
    Incoming,
    /// The window of the game to watch.
    Watch,
    /// The piece a premove promotes to (D3013).
    Promotion,
    /// A confirmation.
    Confirm,
}

/// The numbers and the switch of a time control form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeForm {
    /// Minutes of each side, as typed.
    pub minutes: String,
    /// Increment in seconds, as typed.
    pub increment: String,
    /// A game that counts for the rating.
    pub rated: bool,
    /// Chess960 instead of standard chess (M9-6).
    pub chess960: bool,
}

impl TimeForm {
    fn from_prefs(prefs: &OnlinePrefs) -> TimeForm {
        TimeForm {
            minutes: prefs.time.to_string(),
            increment: prefs.increment.to_string(),
            rated: prefs.rated,
            chess960: false,
        }
    }

    /// The numbers, when both are numbers.
    pub fn parse(&self) -> Option<(u32, u32)> {
        let minutes = self.minutes.trim().parse().ok()?;
        let increment = self.increment.trim().parse().ok()?;
        Some((minutes, increment))
    }
}

/// What the Chess960 position field of the challenge says (M9-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartField {
    /// Empty: the server draws the position.
    Drawn,
    /// A number from 0 to 959.
    Fixed(u16),
    /// Text that is not a number from 0 to 959.
    Invalid,
}

/// The form of the challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeForm {
    /// The name of the user.
    pub user: String,
    /// The clock.
    pub time: TimeForm,
    /// The colour asked for.
    pub color: OnlineColor,
    /// The Chess960 start position number as typed; empty lets the server
    /// draw one (M9-6).
    pub start: String,
}

impl ChallengeForm {
    /// The Chess960 start position typed.
    pub fn chess960_start(&self) -> StartField {
        let text = self.start.trim();
        if text.is_empty() {
            return StartField::Drawn;
        }
        text.parse::<u16>()
            .ok()
            .filter(|n| *n <= 959)
            .map_or(StartField::Invalid, StartField::Fixed)
    }
}

/// What tells the board which premoves it takes: the rule is bound to the
/// position shown, so the board stays ignorant of the online module.
#[derive(Default)]
pub struct PremoveRule(Option<Box<dyn Fn(Square, Square) -> bool + Send + Sync>>);

impl PremoveRule {
    /// A rule (or none).
    pub fn new(rule: Option<Box<dyn Fn(Square, Square) -> bool + Send + Sync>>) -> PremoveRule {
        PremoveRule(rule)
    }

    /// The predicate, when a premove may be entered now.
    pub fn get(&self) -> Option<&(dyn Fn(Square, Square) -> bool + Send + Sync)> {
        self.0.as_deref()
    }
}

impl fmt::Debug for PremoveRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "PremoveRule(on)"
        } else {
            "PremoveRule(off)"
        })
    }
}

/// An event of the online dialogs and panel.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OnlineUiMessage {
    /// Opens a window (the connection window when nobody is connected).
    Open(OnlineDialog),
    /// Closes the window; a seek or challenge in progress is withdrawn.
    Close,
    /// The token field changed.
    Token(Secret),
    /// Where to keep the token.
    Store(StoreChoice),
    /// Copies the address of the page that creates the token.
    CopyLink,
    /// Logs in through the browser (OAuth + PKCE, D3005).
    OAuthLogin,
    /// Gives up the login in the browser.
    CancelOAuth,
    /// Copies the address of the authorization page (D3009).
    CopyOAuthLink,
    /// The text of the game to watch changed.
    WatchInput(String),
    /// Stops watching.
    StopWatch,
    /// Proposes a takeback.
    ProposeTakeback,
    /// Claims a draw over an absent opponent.
    ClaimDraw,
    /// Challenges the opponent again.
    Rematch,
    /// The piece chosen for a premove that promotes (`None` cancels).
    PromotionChoice(Option<Role>),
    /// Connects with the token kept for the last account.
    UseSaved,
    /// Enter or the main button of the open window.
    Submit,
    /// Withdraws the seek or the challenge in progress.
    CancelWait,
    /// A time control button.
    Preset(u32, u32),
    /// The minutes field changed.
    Minutes(String),
    /// The increment field changed.
    Increment(String),
    /// The "rated" box.
    Rated(bool),
    /// The user of the challenge changed.
    ChallengeUser(String),
    /// The colour of the challenge.
    Color(OnlineColor),
    /// The variant selector: Chess960 or standard (M9-6).
    Chess960(bool),
    /// The Chess960 start number of the challenge changed (M9-6).
    ChallengeStart(String),
    /// Accepts a challenge received.
    Accept(String),
    /// Declines a challenge received.
    Decline(String),
    /// The Resign button.
    Resign,
    /// The Abort button.
    Abort,
    /// The Offer draw button.
    OfferDraw,
    /// Accepts (`true`) or declines the draw offered.
    AnswerDraw(bool),
    /// Accepts (`true`) or declines the takeback proposed.
    AnswerTakeback(bool),
    /// Claims the victory over an absent opponent.
    Claim,
    /// Leaves the finished game.
    Dismiss,
    /// Puts back the game of the editor that was there before.
    Restore,
    /// Signs out.
    Disconnect,
    /// Asks to release the lock of the assistance.
    AskRemoveLock,
    /// The answer of a confirmation.
    Confirm(bool),
    /// The premove entry of the board published something.
    Premove(PremoveEvent),
    /// A tick of the clocks (while a game is on).
    Tick(Instant),
}

/// The visual state of the online play.
#[derive(Debug)]
pub struct OnlineUi {
    /// The window open, if any.
    pub dialog: Option<OnlineDialog>,
    /// The question waiting for an answer.
    pub confirm: Option<OnlineConfirm>,
    /// The token being typed (redacted in `Debug`).
    pub token: Secret,
    /// A token is staged in the hub (it can be used again without typing).
    pub staged: bool,
    /// The text typed is not shaped like a token.
    pub token_rejected: bool,
    /// Where the token is kept.
    pub store: StoreChoice,
    /// The seek form.
    pub seek: TimeForm,
    /// The challenge form.
    pub challenge: ChallengeForm,
    /// The instant of the last event (views never read the clock).
    pub now: Instant,
    /// The strips of the board with the clocks of the game.
    pub strips: Option<mychess_app::PlayerStrips>,
    /// The premove rule for the board.
    pub premove_rule: PremoveRule,
    /// What is typed in the "Watch a game" window.
    pub watch_input: String,
    /// A premove that promotes waits for the piece (D3013).
    pub premove_promotion: Option<(Square, Square)>,
}

impl Default for OnlineUi {
    fn default() -> OnlineUi {
        let prefs = OnlinePrefs::default();
        OnlineUi {
            dialog: None,
            confirm: None,
            token: Secret::default(),
            staged: false,
            token_rejected: false,
            store: StoreChoice::Keyring,
            seek: TimeForm::from_prefs(&prefs),
            challenge: ChallengeForm {
                user: String::new(),
                time: TimeForm {
                    minutes: "10".to_owned(),
                    increment: "0".to_owned(),
                    rated: false,
                    chess960: false,
                },
                color: OnlineColor::Random,
                start: String::new(),
            },
            now: Instant::now(),
            strips: None,
            premove_rule: PremoveRule::default(),
            watch_input: String::new(),
            premove_promotion: None,
        }
    }
}

fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(3).collect()
}

fn online(message: OnlineMessage) -> AppMessage {
    AppMessage::Online(message)
}

fn send(message: OnlineMessage) -> Outcome {
    Outcome::send(online(message))
}

/// A time control in seconds of the clock, for the rules of the challenge.
fn limit_secs(minutes: u32) -> u32 {
    minutes.saturating_mul(60)
}

impl OnlineUi {
    /// What is on top for the dialog layer.
    pub fn active(&self) -> Option<OnlineActive> {
        if self.confirm.is_some() {
            return Some(OnlineActive::Confirm);
        }
        if self.premove_promotion.is_some() {
            return Some(OnlineActive::Promotion);
        }
        self.dialog.map(|dialog| match dialog {
            OnlineDialog::Connect => OnlineActive::Connect,
            OnlineDialog::Seek => OnlineActive::Seek,
            OnlineDialog::Challenge => OnlineActive::Challenge,
            OnlineDialog::Incoming => OnlineActive::Incoming,
            OnlineDialog::Watch => OnlineActive::Watch,
        })
    }

    /// Whether the seek form is acceptable to the Board API.
    pub fn seek_is_valid(&self) -> bool {
        self.seek
            .parse()
            .is_some_and(|(m, i)| mychess_app::online::seek_time_is_valid(m, i))
    }

    /// Whether the challenge form is acceptable.
    pub fn challenge_is_valid(&self) -> bool {
        mychess_app::online::login_is_valid(self.challenge.user.trim())
            && self.challenge.time.parse().is_some_and(|(m, i)| {
                mychess_app::online::challenge_time_is_valid(limit_secs(m), i)
            })
            && self.challenge_start_is_valid()
    }

    /// Whether the Chess960 position of the challenge is acceptable: a
    /// number from 0 to 959 and a casual game (M9-6). Always true for
    /// standard chess and for a position drawn by the server.
    pub fn challenge_start_is_valid(&self) -> bool {
        if !self.challenge.time.chess960 {
            return true;
        }
        match self.challenge.chess960_start() {
            StartField::Drawn => true,
            StartField::Fixed(_) => !self.challenge.time.rated,
            StartField::Invalid => false,
        }
    }

    /// The variant of the seek form.
    fn seek_variant(&self) -> OnlineVariant {
        if self.seek.chess960 {
            OnlineVariant::Chess960 { start: None }
        } else {
            OnlineVariant::Standard
        }
    }

    /// The variant of the challenge form.
    fn challenge_variant(&self) -> OnlineVariant {
        if self.challenge.time.chess960 {
            OnlineVariant::Chess960 {
                start: match self.challenge.chess960_start() {
                    StartField::Fixed(n) => Some(n),
                    _ => None,
                },
            }
        } else {
            OnlineVariant::Standard
        }
    }

    /// The widget to focus when a window has just opened.
    pub fn focus_of(active: OnlineActive) -> Option<&'static str> {
        match active {
            OnlineActive::Connect => Some(ONLINE_TOKEN_ID),
            OnlineActive::Seek => Some(ONLINE_MINUTES_ID),
            OnlineActive::Challenge => Some(ONLINE_USER_ID),
            OnlineActive::Watch => Some(ONLINE_WATCH_ID),
            OnlineActive::Incoming | OnlineActive::Promotion | OnlineActive::Confirm => None,
        }
    }

    /// Brings the visual state up to date with the application: windows
    /// that no longer make sense close, the strips follow the clocks.
    pub fn sync(&mut self, app: &AppState) {
        let phase = app.online.phase;
        match self.dialog {
            Some(OnlineDialog::Connect) if phase.is_connected() => self.close_connect(),
            Some(OnlineDialog::Watch) if phase.is_live() || app.online.watch.is_some() => {
                self.dialog = None;
                self.watch_input.clear();
            }
            Some(OnlineDialog::Seek | OnlineDialog::Challenge | OnlineDialog::Incoming)
                if !phase.is_connected() || phase.is_live() =>
            {
                self.dialog = None;
            }
            _ => {}
        }
        if self.dialog != Some(OnlineDialog::Connect) && !phase.can_connect() && self.staged {
            // The hub keeps the token for a retry only while it is useful.
            self.staged = false;
        }
        if self.staged && phase.can_connect() {
            // Back to disconnected: only a refused place for the token (the
            // user may pick another) keeps it for a retry (D1485).
            let refused_place = self.dialog == Some(OnlineDialog::Connect)
                && matches!(
                    app.online.notice,
                    Some(OnlineNotice::Error(OnlineError::TokenStore))
                );
            if !refused_place {
                app.online.hub.clear_staged();
                self.staged = false;
            }
        }
        match self.confirm {
            Some(OnlineConfirm::Resign) if !phase.is_live() => self.confirm = None,
            Some(OnlineConfirm::RemoveLock) if !app.can_remove_lock() => self.confirm = None,
            _ => {}
        }
        // The premove that waited for a piece is moot once it cannot be set.
        if self.premove_promotion.is_some() && self.premove_rule.get().is_none() {
            self.premove_promotion = None;
        }
        self.rebuild_strips(app);
        self.rebuild_premove_rule(app);
    }

    fn close_connect(&mut self) {
        self.dialog = None;
        self.token.clear();
        self.token_rejected = false;
    }

    fn game_has_clock(app: &AppState) -> bool {
        matches!(app.online.phase, OnlinePhase::Playing(_))
            && app.online.game.as_ref().is_some_and(|g| g.clock.is_some())
    }

    /// The strips of the board with the clocks of the online game.
    fn rebuild_strips(&mut self, app: &AppState) {
        if !Self::game_has_clock(app) {
            self.strips = None;
            return;
        }
        let mut strips = app_api::player_strips(app).clone();
        strips.clock_slot = true;
        strips.white.clock = Some(shown_seconds(
            app.online.shown(mychess_core::Color::White, self.now),
            true,
        ));
        strips.black.clock = Some(shown_seconds(
            app.online.shown(mychess_core::Color::Black, self.now),
            true,
        ));
        self.strips = Some(strips);
    }

    /// Redraws the clocks of the strips at `now`; whether anything changed.
    pub fn refresh_clocks(&mut self, app: &AppState) -> bool {
        let before = self.strips.clone();
        self.rebuild_strips(app);
        self.strips != before
    }

    fn rebuild_premove_rule(&mut self, app: &AppState) {
        let online = &app.online;
        let rule = online
            .game
            .as_ref()
            .filter(|g| online.phase.is_live() && !g.is_over() && !g.my_turn())
            .filter(|_| app.prefs.online.premove)
            .filter(|g| shown_is_current(app, g.position().zobrist()))
            .map(|g| {
                let position = g.position().clone();
                let color = g.my_color;
                Box::new(move |from, to| is_pseudo_legal(&position, color, from, to))
                    as Box<dyn Fn(Square, Square) -> bool + Send + Sync>
            });
        self.premove_rule = PremoveRule(rule);
    }

    fn open(&mut self, wanted: OnlineDialog, app: &AppState) -> Outcome {
        let prefs = &app.prefs.online;
        let phase = app.online.phase;
        // Asking for a game before signing in leads to the sign-in window.
        let dialog = if wanted != OnlineDialog::Connect
            && wanted != OnlineDialog::Watch
            && !phase.is_connected()
        {
            OnlineDialog::Connect
        } else {
            wanted
        };
        match dialog {
            OnlineDialog::Connect => {
                if phase.is_connected() {
                    return Outcome::default();
                }
                self.token.clear();
                self.token_rejected = false;
                // Writing the token to a file is chosen each time, never
                // inherited from the preferences (D1488).
                self.store = StoreChoice::Keyring;
            }
            OnlineDialog::Seek => {
                if !(phase.can_start() || phase == OnlinePhase::Seeking) {
                    return Outcome::default();
                }
                if phase != OnlinePhase::Seeking {
                    self.seek = TimeForm::from_prefs(prefs);
                }
            }
            OnlineDialog::Challenge => {
                if !(phase.can_start() || phase == OnlinePhase::Challenging) {
                    return Outcome::default();
                }
            }
            OnlineDialog::Incoming => {
                if phase.is_live() {
                    return Outcome::default();
                }
            }
            OnlineDialog::Watch => {
                if phase.is_busy() || app.assist_locked() {
                    return Outcome::default();
                }
                self.watch_input.clear();
            }
        }
        self.dialog = Some(dialog);
        Outcome {
            focus: Self::focus_of(self.active().unwrap_or(OnlineActive::Incoming)),
            ..Outcome::default()
        }
    }

    fn close(&mut self, app: &AppState) -> Outcome {
        let phase = app.online.phase;
        let dialog = self.dialog.take();
        if dialog == Some(OnlineDialog::Connect) {
            self.token.clear();
            self.token_rejected = false;
            // A token staged for a retry does not outlive the window
            // (D1485); an attempt in flight has taken its own copy.
            if self.staged && phase.can_connect() {
                app.online.hub.clear_staged();
                self.staged = false;
            }
        }
        match (dialog, phase) {
            (Some(OnlineDialog::Seek), OnlinePhase::Seeking) => send(OnlineMessage::CancelSeek),
            (Some(OnlineDialog::Challenge), OnlinePhase::Challenging) => {
                send(OnlineMessage::CancelChallenge)
            }
            (Some(OnlineDialog::Connect), _) if app.online.oauth.is_some() => {
                send(OnlineMessage::CancelOAuth)
            }
            (Some(OnlineDialog::Watch), _) => {
                self.watch_input.clear();
                Outcome::default()
            }
            _ => Outcome::default(),
        }
    }

    fn connect(&mut self, app: &AppState) -> Outcome {
        if !app.online.phase.can_connect() {
            return Outcome::default();
        }
        if !self.token.is_blank() {
            if app
                .online
                .hub
                .stage_token(self.token.expose().trim())
                .is_err()
            {
                self.token_rejected = true;
                return Outcome::default();
            }
            self.staged = true;
            self.token_rejected = false;
            self.token.clear();
        } else if !self.staged {
            return Outcome::default();
        }
        send(OnlineMessage::Connect(ConnectRequest {
            source: TokenSource::Staged,
            store: self.store,
        }))
    }

    fn submit_watch(&self) -> Outcome {
        if self.watch_input.trim().is_empty() {
            return Outcome::default();
        }
        send(OnlineMessage::Watch(self.watch_input.trim().to_owned()))
    }

    fn promotion_choice(&mut self, role: Option<Role>) -> Outcome {
        let Some((from, to)) = self.premove_promotion.take() else {
            return Outcome::default();
        };
        let Some(role) = role else {
            return Outcome::default();
        };
        Outcome::send(online(OnlineMessage::PlayerMove {
            from,
            to,
            promotion: Some(role),
            offer_draw: false,
            at: Instant::now(),
        }))
    }

    fn use_saved(&mut self, app: &AppState) -> Outcome {
        let login = app.prefs.online.login.clone();
        if !app.online.phase.can_connect() || login.is_empty() {
            return Outcome::default();
        }
        send(OnlineMessage::Connect(ConnectRequest {
            source: TokenSource::Stored { login },
            store: self.store,
        }))
    }

    fn remember(
        app: &AppState,
        form: &TimeForm,
        minutes: u32,
        increment: u32,
    ) -> Option<AppMessage> {
        let mut prefs = app.prefs.online.as_ref().clone();
        if prefs.time == minutes && prefs.increment == increment && prefs.rated == form.rated {
            return None;
        }
        prefs.time = minutes;
        prefs.increment = increment;
        prefs.rated = form.rated;
        Some(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Online(
            Box::new(prefs),
        ))))
    }

    fn submit_seek(&mut self, app: &AppState) -> Outcome {
        if app.online.phase == OnlinePhase::Seeking || !self.seek_is_valid() {
            return Outcome::default();
        }
        let Some((minutes, increment)) = self.seek.parse() else {
            return Outcome::default();
        };
        let mut messages = Vec::new();
        messages.extend(Self::remember(app, &self.seek, minutes, increment));
        messages.push(online(OnlineMessage::Seek(SeekRequest {
            minutes,
            increment,
            rated: self.seek.rated,
            color: OnlineColor::Random,
            rating_range: None,
            variant: self.seek_variant(),
        })));
        Outcome {
            messages,
            ..Outcome::default()
        }
    }

    fn submit_challenge(&mut self, app: &AppState) -> Outcome {
        if app.online.phase == OnlinePhase::Challenging || !self.challenge_is_valid() {
            return Outcome::default();
        }
        let Some((minutes, increment)) = self.challenge.time.parse() else {
            return Outcome::default();
        };
        send(OnlineMessage::Challenge(ChallengeRequest {
            username: self.challenge.user.trim().to_owned(),
            limit_secs: limit_secs(minutes),
            increment,
            rated: self.challenge.time.rated,
            color: self.challenge.color,
            variant: self.challenge_variant(),
        }))
    }

    fn cancel_wait(app: &AppState) -> Outcome {
        match app.online.phase {
            OnlinePhase::Seeking => send(OnlineMessage::CancelSeek),
            OnlinePhase::Challenging => send(OnlineMessage::CancelChallenge),
            _ => Outcome::default(),
        }
    }

    fn resign(&mut self, app: &AppState) -> Outcome {
        if app.prefs.online.confirm_resign {
            self.confirm = Some(OnlineConfirm::Resign);
            Outcome::default()
        } else {
            send(OnlineMessage::Resign)
        }
    }

    fn answer(&mut self, yes: bool) -> Outcome {
        let Some(kind) = self.confirm.take() else {
            return Outcome::default();
        };
        if !yes {
            return Outcome::default();
        }
        match kind {
            OnlineConfirm::Resign => send(OnlineMessage::Resign),
            OnlineConfirm::RemoveLock => send(OnlineMessage::RemoveLock { confirmed: true }),
            // The shell quits.
            OnlineConfirm::Quit => Outcome::default(),
        }
    }

    /// Applies an event. The clipboard, the premove entry and the quit are
    /// the shell's: it never forwards those here.
    pub fn update(&mut self, message: OnlineUiMessage, app: &AppState) -> Outcome {
        match message {
            OnlineUiMessage::Open(dialog) => self.open(dialog, app),
            OnlineUiMessage::Close => self.close(app),
            OnlineUiMessage::Token(secret) => {
                self.token = secret;
                self.token_rejected = false;
                Outcome::default()
            }
            OnlineUiMessage::Store(choice) => {
                self.store = choice;
                Outcome::default()
            }
            OnlineUiMessage::UseSaved => self.use_saved(app),
            OnlineUiMessage::Submit => match self.dialog {
                Some(OnlineDialog::Connect) => self.connect(app),
                Some(OnlineDialog::Seek) => self.submit_seek(app),
                Some(OnlineDialog::Challenge) => self.submit_challenge(app),
                Some(OnlineDialog::Incoming) => self.close(app),
                Some(OnlineDialog::Watch) => self.submit_watch(),
                None => Outcome::default(),
            },
            OnlineUiMessage::CancelWait => Self::cancel_wait(app),
            OnlineUiMessage::Preset(minutes, increment) => {
                self.seek.minutes = minutes.to_string();
                self.seek.increment = increment.to_string();
                Outcome::default()
            }
            OnlineUiMessage::Minutes(text) => {
                self.set_minutes(digits(&text));
                Outcome::default()
            }
            OnlineUiMessage::Increment(text) => {
                self.set_increment(digits(&text));
                Outcome::default()
            }
            OnlineUiMessage::Rated(on) => {
                self.form_mut().rated = on;
                Outcome::default()
            }
            OnlineUiMessage::ChallengeUser(text) => {
                self.challenge.user = text.chars().take(MAX_USER_CHARS).collect();
                Outcome::default()
            }
            OnlineUiMessage::Color(color) => {
                self.challenge.color = color;
                Outcome::default()
            }
            OnlineUiMessage::Chess960(on) => {
                self.form_mut().chess960 = on;
                Outcome::default()
            }
            OnlineUiMessage::ChallengeStart(text) => {
                self.challenge.start = digits(&text);
                Outcome::default()
            }
            OnlineUiMessage::Accept(id) => {
                self.dialog = None;
                send(OnlineMessage::AcceptChallenge(id))
            }
            OnlineUiMessage::Decline(id) => send(OnlineMessage::DeclineChallenge(id)),
            OnlineUiMessage::Resign => self.resign(app),
            OnlineUiMessage::Abort => send(OnlineMessage::Abort),
            OnlineUiMessage::OfferDraw => send(OnlineMessage::OfferDraw),
            OnlineUiMessage::AnswerDraw(yes) => send(OnlineMessage::AnswerDraw(yes)),
            OnlineUiMessage::AnswerTakeback(yes) => send(OnlineMessage::AnswerTakeback(yes)),
            OnlineUiMessage::Claim => send(OnlineMessage::ClaimVictory { at: Instant::now() }),
            OnlineUiMessage::Dismiss => send(OnlineMessage::DismissResult),
            OnlineUiMessage::Restore => send(OnlineMessage::RestorePrevious),
            OnlineUiMessage::Disconnect => send(OnlineMessage::Disconnect),
            OnlineUiMessage::OAuthLogin => {
                if !app.online.phase.can_connect() {
                    return Outcome::default();
                }
                self.token.clear();
                self.token_rejected = false;
                send(OnlineMessage::StartOAuth { store: self.store })
            }
            OnlineUiMessage::CancelOAuth => send(OnlineMessage::CancelOAuth),
            OnlineUiMessage::WatchInput(text) => {
                self.watch_input = text.chars().take(MAX_WATCH_CHARS).collect();
                Outcome::default()
            }
            OnlineUiMessage::StopWatch => send(OnlineMessage::StopWatching),
            OnlineUiMessage::ProposeTakeback => send(OnlineMessage::ProposeTakeback),
            OnlineUiMessage::ClaimDraw => send(OnlineMessage::ClaimDraw { at: Instant::now() }),
            OnlineUiMessage::Rematch => send(OnlineMessage::Rematch),
            OnlineUiMessage::PromotionChoice(role) => self.promotion_choice(role),
            OnlineUiMessage::AskRemoveLock => {
                self.confirm = Some(OnlineConfirm::RemoveLock);
                Outcome::default()
            }
            OnlineUiMessage::Confirm(yes) => self.answer(yes),
            // The shell handles these (clipboard, board, clocks).
            OnlineUiMessage::CopyLink
            | OnlineUiMessage::CopyOAuthLink
            | OnlineUiMessage::Premove(_)
            | OnlineUiMessage::Tick(_) => Outcome::default(),
        }
    }

    fn form_mut(&mut self) -> &mut TimeForm {
        if self.dialog == Some(OnlineDialog::Challenge) {
            &mut self.challenge.time
        } else {
            &mut self.seek
        }
    }

    fn set_minutes(&mut self, text: String) {
        self.form_mut().minutes = text;
    }

    fn set_increment(&mut self, text: String) {
        self.form_mut().increment = text;
    }

    /// Escape in the window on top.
    pub fn escape(&mut self, active: OnlineActive, app: &AppState) -> Outcome {
        match active {
            // Esc is "No": the game goes on.
            OnlineActive::Confirm => self.answer(false),
            OnlineActive::Promotion => self.promotion_choice(None),
            _ => self.close(app),
        }
    }

    /// Enter in the window on top (unless a text field used it).
    pub fn enter(&mut self, active: OnlineActive, app: &AppState) -> Outcome {
        match active {
            // Resigning, quitting and removing the lock are deliberate: the
            // safe answer is the default (D1478).
            OnlineActive::Confirm => self.answer(false),
            // A deliberate pick: Enter does not choose a piece.
            OnlineActive::Promotion => Outcome::default(),
            _ => self.update(OnlineUiMessage::Submit, app),
        }
    }

    /// The time shown by the panel for the claim of the victory.
    pub fn claim_wait(app: &AppState, now: Instant) -> Option<Duration> {
        let gone = app.online.opponent_gone?;
        let wait = Duration::from_secs(u64::from(gone.claim_secs.unwrap_or(0)));
        Some(wait.saturating_sub(now.saturating_duration_since(gone.since)))
    }
}

/// Whether the position on the board is the one of the end of the online
/// game (a move or premove only makes sense there).
pub fn shown_is_current(app: &AppState, current: ZobristHash) -> bool {
    app.session.position().zobrist() == current
}

/// Whether the board shows the end of the online game.
pub fn viewing_live_position(app: &AppState) -> bool {
    app.online
        .game
        .as_ref()
        .is_some_and(|g| shown_is_current(app, g.position().zobrist()))
}

/// Whether the clocks of a game are running (a tick is useful).
pub fn wants_ticks(app: &AppState) -> bool {
    app.online.phase == OnlinePhase::Playing(PlayingState::Live)
        || app.online.phase == OnlinePhase::Playing(PlayingState::Reconnecting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secret_never_shows_its_text() {
        let secret = Secret::new("lip_hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(
            format!("{:?}", OnlineUiMessage::Token(secret)),
            "Token(Secret(<redacted>))"
        );
    }

    #[test]
    fn a_form_needs_two_numbers() {
        let mut form = TimeForm {
            minutes: "10".to_owned(),
            increment: String::new(),
            rated: false,
            chess960: false,
        };
        assert_eq!(form.parse(), None);
        form.increment = "5".to_owned();
        assert_eq!(form.parse(), Some((10, 5)));
    }

    #[test]
    fn digits_keep_three_numbers_at_most() {
        assert_eq!(digits("a1b2c3d4"), "123");
    }
}
