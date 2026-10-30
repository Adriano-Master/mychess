//! Messages, commands and notices of the FICS play (spec m8-9).

use std::time::Instant;

use mychess_core::{Role, Square};
use mychess_online::fics::{ColorChoice, Command, Disconnect, FicsMessage as Net, SeekSpec};
use serde::Serialize;

/// The piece a pawn becomes (D2614).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AutoPromote {
    /// The front-end asks.
    #[default]
    Ask,
    /// Always a queen.
    Queen,
    /// Always a rook.
    Rook,
    /// Always a bishop.
    Bishop,
    /// Always a knight.
    Knight,
}

impl AutoPromote {
    /// Reads the TOML spelling (`ask`, `queen`, `rook`, `bishop`, `knight`).
    #[must_use]
    pub fn from_name(text: &str) -> Option<AutoPromote> {
        match text {
            "ask" => Some(AutoPromote::Ask),
            "queen" => Some(AutoPromote::Queen),
            "rook" => Some(AutoPromote::Rook),
            "bishop" => Some(AutoPromote::Bishop),
            "knight" => Some(AutoPromote::Knight),
            _ => None,
        }
    }

    /// The piece, `None` when the user is asked.
    #[must_use]
    pub fn role(self) -> Option<Role> {
        match self {
            AutoPromote::Ask => None,
            AutoPromote::Queen => Some(Role::Queen),
            AutoPromote::Rook => Some(Role::Rook),
            AutoPromote::Bishop => Some(Role::Bishop),
            AutoPromote::Knight => Some(Role::Knight),
        }
    }
}

/// Settings of the FICS play (D2628), the `[fics]` table. The password is
/// not here (D2602).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FicsPrefs {
    /// The server.
    pub host: String,
    /// The port.
    pub port: u16,
    /// The last handle used; empty is a guest (D2601).
    pub login: String,
    /// Rated seeks and challenges.
    pub rated: bool,
    /// Minutes of the last seek.
    pub minutes: u32,
    /// Increment of the last seek.
    pub increment: u32,
    /// Promotion without asking (D2614).
    pub auto_promote: AutoPromote,
    /// Premove (D2615).
    pub premove: bool,
    /// Write `[%emt]` (D2623).
    pub store_emt: bool,
    /// Write `[%clk]` (D2623).
    pub store_clk: bool,
    /// Hear the shouts (`set shout`, D2628).
    pub shouts: bool,
    /// Commands sent after login (D2609); each is checked.
    pub init_commands: Vec<String>,
}

impl FicsPrefs {
    /// The same settings made safe: a host the connection would refuse
    /// falls back to the default, the times stay in their domain, the
    /// handle is valid or empty and the commands are checked.
    #[must_use]
    pub fn sanitized(mut self) -> FicsPrefs {
        let defaults = FicsPrefs::default();
        let host_ok = !self.host.is_empty()
            && self.host.len() <= 253
            && self
                .host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
        if !host_ok {
            self.host = defaults.host;
        }
        if self.port == 0 {
            self.port = defaults.port;
        }
        let login_ok = (2..=17).contains(&self.login.len())
            && self.login.bytes().all(|b| b.is_ascii_alphanumeric());
        if !login_ok {
            self.login.clear();
        }
        self.minutes = self.minutes.min(600);
        self.increment = self.increment.min(600);
        self.init_commands.retain(|c| Command::raw(c).is_ok());
        self.init_commands.truncate(32);
        self
    }
}

impl Default for FicsPrefs {
    fn default() -> FicsPrefs {
        FicsPrefs {
            host: mychess_online::fics::DEFAULT_HOST.to_owned(),
            port: mychess_online::fics::DEFAULT_PORT,
            login: String::new(),
            rated: false,
            minutes: 5,
            increment: 0,
            auto_promote: AutoPromote::Ask,
            premove: true,
            store_emt: true,
            store_clk: true,
            shouts: false,
            init_commands: Vec::new(),
        }
    }
}

/// A request to connect. The password of a registered account is staged in
/// the hub ([`FicsHub::stage_password`](super::FicsHub::stage_password)) and
/// never travels in a message (D2602).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectRequest {
    /// The handle; `None` is a guest (D2601).
    pub name: Option<String>,
}

/// What the stream of the connection says, stamped with the epoch of the
/// connection.
#[derive(Debug, Clone, PartialEq)]
pub struct NetUpdate {
    pub(crate) epoch: u64,
    pub(crate) at: Instant,
    pub(crate) message: Net,
}

/// Something the FICS play asks the outside world to do.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FicsCommand {
    /// Open the connection.
    Connect {
        /// The epoch of the attempt.
        epoch: u64,
        /// The server.
        host: String,
        /// The port.
        port: u16,
        /// The handle; `None` is a guest.
        name: Option<String>,
        /// The commands of the user to send after login.
        init: Vec<Command>,
    },
    /// Write a command.
    Send(Command),
    /// Close the connection (politely: `quit` first when `quit`).
    Disconnect {
        /// Say `quit` before closing.
        quit: bool,
        /// The epoch of the connection to close: a newer connection is left
        /// alone (D2691).
        epoch: u64,
    },
}

/// A remark for the status bar, as data (D2625).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FicsNotice {
    /// Logged in.
    Connected {
        /// The handle.
        name: String,
    },
    /// The connection ended.
    Disconnected(Disconnect),
    /// A command or a name was refused before it left.
    BadInput,
    /// The command is not possible in this state.
    WrongState,
    /// The message of the families of D1330 was dropped.
    AssistanceDisabled,
    /// The server did not accept the move of the user.
    MoveRejected,
    /// A game started: engines and queries are off.
    GameStarted,
    /// The game of the user ended.
    GameEnded,
    /// The game was stored in the Clipbase.
    Imported,
    /// The game was stored before.
    AlreadyImported,
    /// The game ended without a result and was not stored.
    NotImported,
    /// The game could not be stored.
    ImportFailed,
    /// The history before the first board is missing (D2613).
    Incomplete,
    /// More games are watched than the limit allows (D2604).
    TooManyObserved,
    /// Following is not possible while playing (D2618).
    FollowWhilePlaying,
    /// The lock was released by the user without a game at the server.
    LockReleased,
    /// A command could not be queued (D2694).
    SendFailed,
    /// Another game started while one is on; it is not shown (D2613).
    AnotherGame,
}

impl FicsNotice {
    /// The Fluent key (`fics-notice-*`).
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            FicsNotice::Connected { .. } => "fics-notice-connected",
            FicsNotice::Disconnected(d) => match d {
                Disconnect::LoginFailed(_) => "fics-notice-login-failed",
                Disconnect::ConnectFailed => "fics-notice-connect-failed",
                Disconnect::Timeout => "fics-notice-timeout",
                _ => "fics-notice-disconnected",
            },
            FicsNotice::BadInput => "fics-notice-bad-input",
            FicsNotice::WrongState => "fics-notice-wrong-state",
            FicsNotice::AssistanceDisabled => "online-notice-assistance-disabled",
            FicsNotice::MoveRejected => "fics-notice-move-rejected",
            FicsNotice::GameStarted => "fics-notice-game-started",
            FicsNotice::GameEnded => "fics-notice-game-ended",
            FicsNotice::Imported => "online-notice-imported",
            FicsNotice::AlreadyImported => "online-notice-already-imported",
            FicsNotice::NotImported => "online-notice-not-imported",
            FicsNotice::ImportFailed => "online-notice-import-failed",
            FicsNotice::Incomplete => "fics-notice-incomplete",
            FicsNotice::TooManyObserved => "fics-notice-too-many-observed",
            FicsNotice::FollowWhilePlaying => "fics-notice-follow-while-playing",
            FicsNotice::LockReleased => "online-notice-lock-released",
            FicsNotice::SendFailed => "fics-notice-send-failed",
            FicsNotice::AnotherGame => "fics-notice-another-game",
        }
    }
}

/// What the user can answer to an offer of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Offer {
    /// A match request to the user.
    Match {
        /// Who asks.
        from: String,
        /// His rating.
        rating: Option<u32>,
        /// Rated game.
        rated: bool,
        /// The kind.
        kind: String,
        /// Minutes of each side.
        minutes: u32,
        /// Increment in seconds.
        increment: u32,
    },
    /// A draw offer.
    Draw {
        /// Who offers.
        from: String,
    },
    /// A takeback request.
    Takeback {
        /// Who asks.
        from: String,
        /// How many plies.
        plies: u32,
    },
    /// An abort request.
    Abort {
        /// Who asks.
        from: String,
    },
}

impl Offer {
    /// Who made the offer.
    #[must_use]
    pub fn from(&self) -> &str {
        match self {
            Offer::Match { from, .. }
            | Offer::Draw { from }
            | Offer::Takeback { from, .. }
            | Offer::Abort { from } => from,
        }
    }
}

/// A message of the FICS play. Created by the front-end (the user acts) and
/// by the hub (the connection speaks).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FicsMessage {
    /// Show the FICS window.
    OpenWindow,
    /// Hide the FICS window.
    CloseWindow,
    /// Connect (the password, if any, is staged in the hub).
    Connect(ConnectRequest),
    /// Close the connection.
    Disconnect,
    /// A line typed in the console.
    Console(String),
    /// Look for an opponent (D2612).
    Seek(SeekSpec),
    /// Withdraw the seeks.
    Unseek,
    /// Challenge a player.
    Challenge {
        /// The handle.
        player: String,
        /// Minutes of each side.
        minutes: u32,
        /// Increment in seconds.
        increment: u32,
        /// Rated game.
        rated: bool,
        /// The colour.
        color: ColorChoice,
    },
    /// Accept the offer of a player (the only one when `None`).
    Accept(Option<String>),
    /// Decline the offer of a player.
    Decline(Option<String>),
    /// Answer an advertisement of the graph.
    PlaySeek(u32),
    /// A move of the user; a premove when it is not his turn.
    PlayerMove {
        /// Origin.
        from: Square,
        /// Destination.
        to: Square,
        /// Promotion piece chosen.
        promotion: Option<Role>,
    },
    /// Drop the premove.
    CancelPremove,
    /// Resign.
    Resign,
    /// Offer or accept a draw.
    OfferDraw,
    /// Abort.
    Abort,
    /// Ask to take back plies.
    Takeback(u32),
    /// Observe a game by number or a player by handle.
    Observe(String),
    /// Stop observing.
    Unobserve(u32),
    /// Show an observed game in the editor.
    Show(u32),
    /// Follow a player.
    Follow(String),
    /// Stop following.
    Unfollow,
    /// Examine (an empty board, or the game of a player).
    Examine(Option<String>),
    /// Stop examining.
    Unexamine,
    /// Examined game: forward.
    Forward(u32),
    /// Examined game: backward.
    Backward(u32),
    /// Join or leave the channel of the lectures (D2616).
    Lectures(bool),
    /// New settings.
    SetPrefs(Box<FicsPrefs>),
    /// Clear the console.
    ClearConsole,
    /// Forget a game the server may still keep, releasing the lock of the
    /// assistance (only without a connection).
    ForgetGame,
    /// Put the game that was in the editor before the FICS one back (when
    /// no game of the user is on).
    RestorePrevious,
    /// What the connection said.
    Net(NetUpdate),
    /// A command could not be queued.
    SendFailed {
        /// The epoch of the connection the hub held.
        epoch: u64,
        /// The line that did not go out.
        line: String,
    },
    /// A tick of the clock of the front-end: the watchdogs of the move and
    /// of the connection run on it (D2692).
    Tick(Instant),
    /// A command ran and has nothing to say.
    Ack,
}
