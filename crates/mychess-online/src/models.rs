//! Tolerant models of the Lichess API (spec m8-3, D1304).
//!
//! No struct denies unknown fields, optional fields default, enums that the
//! server may grow have an `Unknown` variant, and a line that does not decode
//! becomes `Unparsed` instead of ending the stream. The decoders
//! ([`Event::parse_line`], [`GameEvent::parse_line`]) never panic.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

pub use crate::time_control::Speed;
use crate::{
    error::Result,
    time_control::{validate_challenge, validate_seek},
};

/// Deserializes `T`, turning any decoding error into `None` (lenient enums).
fn lenient<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let value = serde_json::Value::deserialize(de)?;
    Ok(T::deserialize(value).ok())
}

/// Numeric field that must never sink the whole object: negative,
/// fractional, out-of-range or non-numeric values become the default (0).
fn lenient_num<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(lenient::<D, T>(de)?.unwrap_or_default())
}

/// Side of the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    /// White.
    White,
    /// Black.
    Black,
}

impl Color {
    /// The other side.
    #[must_use]
    pub fn opposite(self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

/// Variant of the game (`key` is `standard`, `chess960`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Variant {
    /// Machine name.
    #[serde(default)]
    pub key: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
}

impl Variant {
    /// Whether the key is `standard`.
    #[must_use]
    pub fn is_standard(&self) -> bool {
        self.key == "standard"
    }

    /// Whether the key is `chess960`.
    #[must_use]
    pub fn is_chess960(&self) -> bool {
        self.key == "chess960"
    }

    /// Whether this client plays the variant: standard or Chess960 (M9-6).
    #[must_use]
    pub fn is_playable(&self) -> bool {
        self.is_standard() || self.is_chess960()
    }
}

/// Rating of one performance category in the account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Perf {
    /// Rating (0 when unknown).
    #[serde(default, deserialize_with = "lenient_num")]
    pub rating: u32,
    /// Games played.
    #[serde(default, deserialize_with = "lenient_num")]
    pub games: u32,
    /// Provisional rating.
    #[serde(default)]
    pub prov: bool,
}

/// `GET /api/account` (the fields MyChess uses).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Account {
    /// Lowercase user id.
    #[serde(default)]
    pub id: String,
    /// User name as displayed.
    #[serde(default)]
    pub username: String,
    /// Title (`GM`, `BOT`...).
    #[serde(default)]
    pub title: Option<String>,
    /// Ratings by category (`blitz`, `rapid`, ...).
    #[serde(default)]
    pub perfs: HashMap<String, Perf>,
}

impl Account {
    /// Whether the account is a Bot account (D1301).
    #[must_use]
    pub fn is_bot(&self) -> bool {
        self.title.as_deref() == Some("BOT")
    }
}

/// Player data in `gameFull` and `challenge`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Player {
    /// Lowercase user id (absent for the AI and anonymous users).
    #[serde(default)]
    pub id: Option<String>,
    /// Display name.
    #[serde(default, alias = "username")]
    pub name: Option<String>,
    /// Title.
    #[serde(default)]
    pub title: Option<String>,
    /// Rating.
    #[serde(default, deserialize_with = "lenient")]
    pub rating: Option<u32>,
    /// Provisional rating.
    #[serde(default)]
    pub provisional: bool,
    /// Level when the opponent is the Lichess AI.
    #[serde(default, rename = "aiLevel", deserialize_with = "lenient")]
    pub ai_level: Option<u32>,
}

/// Initial clock of a game, in milliseconds (absent in correspondence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Clock {
    /// Initial time.
    #[serde(default, deserialize_with = "lenient_num")]
    pub initial: u64,
    /// Increment per move.
    #[serde(default, deserialize_with = "lenient_num")]
    pub increment: u64,
}

/// Game status. Everything but `Created` and `Started` ends the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum Status {
    /// Not started yet.
    Created,
    /// In progress.
    Started,
    /// Aborted.
    Aborted,
    /// Checkmate.
    Mate,
    /// Resignation.
    Resign,
    /// Stalemate.
    Stalemate,
    /// Flag fell with the opponent gone (`timeout`).
    Timeout,
    /// Draw.
    Draw,
    /// Flag fell.
    Outoftime,
    /// Cheat detected.
    Cheat,
    /// The first move was never made.
    NoStart,
    /// Unknown ending.
    UnknownFinish,
    /// Insufficient material claim.
    InsufficientMaterialClaim,
    /// Variant-specific ending.
    VariantEnd,
    /// A status this client does not know; treated as final.
    #[serde(other)]
    Unknown,
}

impl Status {
    /// Whether the game is over (an unknown status counts as over).
    #[must_use]
    pub fn is_final(self) -> bool {
        !matches!(self, Status::Created | Status::Started)
    }
}

/// Time left for a move in correspondence-like expirations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct Expiration {
    /// Idle time so far.
    #[serde(default, rename = "idleMillis", deserialize_with = "lenient_num")]
    pub idle_millis: u64,
    /// Time left to move.
    #[serde(default, rename = "millisToMove", deserialize_with = "lenient_num")]
    pub millis_to_move: u64,
}

/// `gameState` (also the `state` of `gameFull`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GameState {
    /// UCI moves separated by spaces.
    #[serde(default)]
    pub moves: String,
    /// White's remaining time (ms).
    #[serde(default, deserialize_with = "lenient_num")]
    pub wtime: u64,
    /// Black's remaining time (ms).
    #[serde(default, deserialize_with = "lenient_num")]
    pub btime: u64,
    /// White's increment (ms).
    #[serde(default, deserialize_with = "lenient_num")]
    pub winc: u64,
    /// Black's increment (ms).
    #[serde(default, deserialize_with = "lenient_num")]
    pub binc: u64,
    /// Status; `None` when the field is missing or malformed.
    #[serde(default, deserialize_with = "lenient")]
    pub status: Option<Status>,
    /// Winner, absent in draws and while playing.
    #[serde(default, deserialize_with = "lenient")]
    pub winner: Option<Color>,
    /// White offers a draw.
    #[serde(default)]
    pub wdraw: bool,
    /// Black offers a draw.
    #[serde(default)]
    pub bdraw: bool,
    /// White proposes a takeback.
    #[serde(default)]
    pub wtakeback: bool,
    /// Black proposes a takeback.
    #[serde(default)]
    pub btakeback: bool,
    /// Expiration data, if any.
    #[serde(default)]
    pub expiration: Option<Expiration>,
}

impl GameState {
    /// The moves as a list of UCI strings (empty when none).
    #[must_use]
    pub fn move_list(&self) -> Vec<&str> {
        self.moves.split_whitespace().collect()
    }

    /// Whether the game is over according to `status`.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.status.is_some_and(Status::is_final)
    }
}

/// Name of the rating category.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PerfName {
    /// Display name (`Rapid`, `Classical`...).
    #[serde(default)]
    pub name: String,
}

fn startpos() -> String {
    "startpos".to_owned()
}

/// `gameFull`: first line of the game stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GameFull {
    /// Game id.
    #[serde(default)]
    pub id: String,
    /// Variant.
    #[serde(default = "default_variant")]
    pub variant: Variant,
    /// Initial clock (absent in correspondence).
    #[serde(default)]
    pub clock: Option<Clock>,
    /// Speed.
    #[serde(default, deserialize_with = "lenient")]
    pub speed: Option<Speed>,
    /// Rating category.
    #[serde(default)]
    pub perf: Option<PerfName>,
    /// Rated game.
    #[serde(default)]
    pub rated: bool,
    /// Creation time (ms since the Unix epoch).
    #[serde(default, rename = "createdAt", deserialize_with = "lenient")]
    pub created_at: Option<u64>,
    /// White player.
    #[serde(default)]
    pub white: Player,
    /// Black player.
    #[serde(default)]
    pub black: Player,
    /// Initial FEN; `"startpos"` by default.
    #[serde(default = "startpos", rename = "initialFen")]
    pub initial_fen: String,
    /// State at the time of the connection.
    #[serde(default)]
    pub state: GameState,
    /// Days per move in correspondence.
    #[serde(default, rename = "daysPerTurn", deserialize_with = "lenient")]
    pub days_per_turn: Option<u32>,
    /// Tournament id, if any.
    #[serde(default, rename = "tournamentId")]
    pub tournament_id: Option<String>,
}

fn default_variant() -> Variant {
    Variant {
        key: "standard".to_owned(),
        name: "Standard".to_owned(),
    }
}

/// An event of the game stream (`/api/board/game/stream/{id}`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GameEvent {
    /// Full game description (first line, and after every reconnection).
    Full(Box<GameFull>),
    /// State update.
    State(GameState),
    /// Chat line; the text is dropped on purpose (D1326).
    ChatLine,
    /// The opponent left or came back.
    OpponentGone {
        /// Whether the opponent is gone.
        gone: bool,
        /// Seconds until the victory can be claimed, if known.
        claim_win_in_seconds: Option<u32>,
    },
    /// A known-shape JSON object with an unknown `type`.
    Unknown,
    /// A line that is not a valid event; the stream goes on (D1304).
    Unparsed {
        /// The offending line, truncated for logging.
        line: String,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum RawGameEvent {
    #[serde(rename = "gameFull")]
    Full(Box<GameFull>),
    #[serde(rename = "gameState")]
    State(GameState),
    #[serde(rename = "chatLine")]
    ChatLine {},
    #[serde(rename = "opponentGone")]
    OpponentGone {
        #[serde(default)]
        gone: bool,
        #[serde(default, rename = "claimWinInSeconds", deserialize_with = "lenient")]
        claim_win_in_seconds: Option<u32>,
    },
    #[serde(other)]
    Unknown,
}

/// Truncates a line for diagnostics without splitting a character.
fn clip(line: &str) -> String {
    line.chars().take(200).collect()
}

impl GameEvent {
    /// Decodes one NDJSON line. Never fails: bad input is `Unparsed`.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        match serde_json::from_str::<RawGameEvent>(line) {
            Ok(RawGameEvent::Full(full)) => GameEvent::Full(full),
            Ok(RawGameEvent::State(state)) => GameEvent::State(state),
            Ok(RawGameEvent::ChatLine {}) => GameEvent::ChatLine,
            Ok(RawGameEvent::OpponentGone {
                gone,
                claim_win_in_seconds,
            }) => GameEvent::OpponentGone {
                gone,
                claim_win_in_seconds,
            },
            Ok(RawGameEvent::Unknown) => GameEvent::Unknown,
            Err(_) => GameEvent::Unparsed { line: clip(line) },
        }
    }
}

/// Game summary inside `gameStart` and `gameFinish`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GameInfo {
    /// Game id. Current payloads send `id` and `gameId` together, older ones
    /// only `gameId`: a serde `alias` would reject the pair as a duplicate
    /// field, so `gameId` has its own field and `GameInfo::normalized`
    /// fills this one from it.
    #[serde(default)]
    pub id: String,
    /// The `gameId` key of the payload (see [`id`](Self::id)).
    #[serde(default, rename = "gameId")]
    pub game_id: Option<String>,
    /// Id of the game from the user's point of view (id plus 4 characters).
    #[serde(default, rename = "fullId")]
    pub full_id: Option<String>,
    /// The user's color.
    #[serde(default, deserialize_with = "lenient")]
    pub color: Option<Color>,
    /// Current FEN.
    #[serde(default)]
    pub fen: Option<String>,
    /// Whether it is the user's turn.
    #[serde(default, rename = "isMyTurn")]
    pub is_my_turn: bool,
    /// Rating category key.
    #[serde(default)]
    pub perf: Option<String>,
    /// Rated game.
    #[serde(default)]
    pub rated: bool,
    /// Speed.
    #[serde(default, deserialize_with = "lenient")]
    pub speed: Option<Speed>,
    /// Variant.
    #[serde(default)]
    pub variant: Option<Variant>,
    /// Opponent summary.
    #[serde(default)]
    pub opponent: Option<Player>,
    /// Winner (in `gameFinish`).
    #[serde(default, deserialize_with = "lenient")]
    pub winner: Option<Color>,
}

impl GameInfo {
    /// Makes `id` the game id whichever key the payload used.
    fn normalized(mut self) -> Self {
        if self.id.is_empty()
            && let Some(game_id) = self.game_id.clone()
        {
            self.id = game_id;
        }
        self
    }
}

/// Direction of a challenge relative to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Direction {
    /// Received.
    In,
    /// Sent.
    Out,
}

/// Time control inside a challenge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ChallengeTimeControl {
    /// `clock`, `correspondence` or `unlimited`.
    #[serde(default, rename = "type")]
    pub kind: String,
    /// Seconds.
    #[serde(default, deserialize_with = "lenient")]
    pub limit: Option<u32>,
    /// Seconds.
    #[serde(default, deserialize_with = "lenient")]
    pub increment: Option<u32>,
    /// Display string (`10+5`).
    #[serde(default)]
    pub show: Option<String>,
    /// Days per move.
    #[serde(default, deserialize_with = "lenient")]
    pub days: Option<u32>,
}

/// A challenge (event `challenge` and friends, and the challenge stream).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Challenge {
    /// Challenge id (also the id of the game once accepted).
    #[serde(default)]
    pub id: String,
    /// `created`, `offline`, `canceled`, `declined`, `accepted`.
    #[serde(default)]
    pub status: String,
    /// Who sent it.
    #[serde(default)]
    pub challenger: Option<Player>,
    /// Who receives it.
    #[serde(default, rename = "destUser")]
    pub dest_user: Option<Player>,
    /// Variant.
    #[serde(default)]
    pub variant: Option<Variant>,
    /// Rated.
    #[serde(default)]
    pub rated: bool,
    /// Speed.
    #[serde(default, deserialize_with = "lenient")]
    pub speed: Option<Speed>,
    /// Time control.
    #[serde(default, rename = "timeControl")]
    pub time_control: Option<ChallengeTimeControl>,
    /// Requested color (`white`, `black`, `random`).
    #[serde(default)]
    pub color: Option<String>,
    /// Resulting color.
    #[serde(default, rename = "finalColor", deserialize_with = "lenient")]
    pub final_color: Option<Color>,
    /// Direction relative to the user.
    #[serde(default, deserialize_with = "lenient")]
    pub direction: Option<Direction>,
    /// Initial FEN, if not the standard one.
    #[serde(default, rename = "initialFen")]
    pub initial_fen: Option<String>,
    /// Decline reason key, when declined.
    #[serde(default, rename = "declineReasonKey")]
    pub decline_reason_key: Option<String>,
}

impl Challenge {
    /// Whether this client can play the challenge: standard or Chess960
    /// variant (M9-6) and a real-time clock of blitz or slower (D1313,
    /// D1314).
    #[must_use]
    pub fn is_playable(&self) -> bool {
        let standard = self.variant.as_ref().is_none_or(Variant::is_playable);
        let clock_ok = self.time_control.as_ref().is_some_and(|tc| {
            tc.kind == "clock"
                && validate_challenge(tc.limit.unwrap_or(0), tc.increment.unwrap_or(0)).is_ok()
        });
        standard && clock_ok
    }
}

/// An event of the account stream (`/api/stream/event`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A game started.
    GameStart(GameInfo),
    /// A game ended.
    GameFinish(GameInfo),
    /// A challenge was received or created.
    Challenge(Box<Challenge>),
    /// A challenge was canceled.
    ChallengeCanceled(Box<Challenge>),
    /// A challenge was declined.
    ChallengeDeclined(Box<Challenge>),
    /// A JSON object with an unknown `type`.
    Unknown,
    /// A line that is not a valid event; the stream goes on (D1304).
    Unparsed {
        /// The offending line, truncated for logging.
        line: String,
    },
}

#[derive(Deserialize)]
struct GameRef {
    #[serde(default)]
    game: GameInfo,
}

#[derive(Deserialize)]
struct ChallengeRef {
    #[serde(default)]
    challenge: Challenge,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum RawEvent {
    #[serde(rename = "gameStart")]
    GameStart(GameRef),
    #[serde(rename = "gameFinish")]
    GameFinish(GameRef),
    #[serde(rename = "challenge")]
    Challenge(ChallengeRef),
    #[serde(rename = "challengeCanceled")]
    ChallengeCanceled(ChallengeRef),
    #[serde(rename = "challengeDeclined")]
    ChallengeDeclined(ChallengeRef),
    #[serde(other)]
    Unknown,
}

impl Event {
    /// Decodes one NDJSON line. Never fails: bad input is `Unparsed`.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        match serde_json::from_str::<RawEvent>(line) {
            Ok(RawEvent::GameStart(r)) => Event::GameStart(r.game.normalized()),
            Ok(RawEvent::GameFinish(r)) => Event::GameFinish(r.game.normalized()),
            Ok(RawEvent::Challenge(r)) => Event::Challenge(Box::new(r.challenge)),
            Ok(RawEvent::ChallengeCanceled(r)) => Event::ChallengeCanceled(Box::new(r.challenge)),
            Ok(RawEvent::ChallengeDeclined(r)) => Event::ChallengeDeclined(Box::new(r.challenge)),
            Ok(RawEvent::Unknown) => Event::Unknown,
            Err(_) => Event::Unparsed { line: clip(line) },
        }
    }
}

/// How a challenge sent with `keepAliveStream` ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChallengeUpdate {
    /// The challenge object (first line, and updates).
    Challenge(Box<Challenge>),
    /// `{"done": "..."}`: `accepted`, `declined`, `canceled` or other.
    Done(String),
    /// A line that is not valid; the stream goes on.
    Unparsed {
        /// The offending line, truncated for logging.
        line: String,
    },
}

impl ChallengeUpdate {
    /// Decodes one NDJSON line of the challenge stream. Never fails.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        #[derive(Deserialize)]
        struct Done {
            done: String,
        }
        if let Ok(done) = serde_json::from_str::<Done>(line) {
            return ChallengeUpdate::Done(done.done);
        }
        match serde_json::from_str::<Challenge>(line) {
            Ok(c) if !c.id.is_empty() => ChallengeUpdate::Challenge(Box::new(c)),
            _ => ChallengeUpdate::Unparsed { line: clip(line) },
        }
    }
}

/// Color choice of a seek or challenge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorChoice {
    /// Random (the server default).
    #[default]
    Random,
    /// White.
    White,
    /// Black.
    Black,
}

impl ColorChoice {
    fn as_str(self) -> &'static str {
        match self {
            ColorChoice::Random => "random",
            ColorChoice::White => "white",
            ColorChoice::Black => "black",
        }
    }
}

/// Parameters of a real-time seek (D1315).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeekParams {
    /// Minutes on the clock (0-180).
    pub minutes: u32,
    /// Increment in seconds (0-180).
    pub increment: u32,
    /// Rated game (default casual).
    pub rated: bool,
    /// Color.
    pub color: ColorChoice,
    /// Opponent rating range, `min..=max`.
    pub rating_range: Option<(u32, u32)>,
    /// Chess960 instead of standard chess (M9-6); the server draws the
    /// starting position.
    pub chess960: bool,
}

impl SeekParams {
    /// Casual seek without color or rating range.
    #[must_use]
    pub fn new(minutes: u32, increment: u32) -> Self {
        Self {
            minutes,
            increment,
            rated: false,
            color: ColorChoice::Random,
            rating_range: None,
            chess960: false,
        }
    }

    /// Validates the time control (D1314) and builds the form fields.
    ///
    /// # Errors
    /// [`crate::Error::TimeControl`] for a time control the Board API refuses.
    pub fn form_fields(&self) -> Result<Vec<(&'static str, String)>> {
        validate_seek(self.minutes, self.increment)?;
        if self.rating_range.is_some_and(|(lo, hi)| lo > hi) {
            return Err(crate::Error::InvalidArgument {
                what: "rating range",
            });
        }
        let mut f = vec![
            ("rated", self.rated.to_string()),
            ("time", self.minutes.to_string()),
            ("increment", self.increment.to_string()),
            ("variant", variant_key(self.chess960).to_owned()),
        ];
        if self.color != ColorChoice::Random {
            f.push(("color", self.color.as_str().to_owned()));
        }
        if let Some((lo, hi)) = self.rating_range {
            f.push(("ratingRange", format!("{lo}-{hi}")));
        }
        Ok(f)
    }
}

/// Parameters of a direct challenge (D1316).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeParams {
    /// Clock limit in seconds.
    pub limit_secs: u32,
    /// Increment in seconds (0-60).
    pub increment: u32,
    /// Rated game.
    pub rated: bool,
    /// Color.
    pub color: ColorChoice,
    /// Chess960 instead of standard chess (M9-6).
    pub chess960: bool,
    /// Fixed starting position (X-FEN/Shredder-FEN) of a Chess960 challenge;
    /// `None` lets the server draw one. Needs a casual game.
    pub initial_fen: Option<String>,
}

/// The `variant` form value.
fn variant_key(chess960: bool) -> &'static str {
    if chess960 { "chess960" } else { "standard" }
}

impl ChallengeParams {
    /// Casual challenge with a random color.
    #[must_use]
    pub fn new(limit_secs: u32, increment: u32) -> Self {
        Self {
            limit_secs,
            increment,
            rated: false,
            color: ColorChoice::Random,
            chess960: false,
            initial_fen: None,
        }
    }

    /// Validates the time control and builds the form fields, including
    /// `keepAliveStream=true` so the challenge does not expire in 20 s.
    ///
    /// # Errors
    /// [`crate::Error::TimeControl`] for a time control the API refuses;
    /// [`crate::Error::InvalidArgument`] for a fixed position outside a
    /// casual Chess960 challenge or with a blank or multi-line FEN.
    pub fn form_fields(&self) -> Result<Vec<(&'static str, String)>> {
        validate_challenge(self.limit_secs, self.increment)?;
        let mut fields = vec![
            ("rated", self.rated.to_string()),
            ("clock.limit", self.limit_secs.to_string()),
            ("clock.increment", self.increment.to_string()),
            ("color", self.color.as_str().to_owned()),
            ("variant", variant_key(self.chess960).to_owned()),
        ];
        if let Some(fen) = &self.initial_fen {
            let fen = fen.trim();
            if !self.chess960 || self.rated || fen.is_empty() || fen.contains(['\n', '\r']) {
                return Err(crate::Error::InvalidArgument {
                    what: "initial position",
                });
            }
            fields.push(("fen", fen.to_owned()));
        }
        fields.push(("keepAliveStream", "true".to_owned()));
        Ok(fields)
    }
}

/// A player in the description of an observed game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchPlayer {
    /// User name (empty for an anonymous or AI player).
    pub name: String,
    /// Title, if any.
    pub title: Option<String>,
    /// Rating, if known.
    pub rating: Option<u32>,
}

/// First line (and last line) of `/api/stream/game/{id}`: the description of
/// the game, without its move list (D3000).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchGame {
    /// Game id.
    pub id: String,
    /// Variant.
    pub variant: Variant,
    /// Rated game.
    pub rated: bool,
    /// Speed.
    pub speed: Option<Speed>,
    /// Status; `None` when absent or malformed.
    pub status: Option<Status>,
    /// Winner, absent in draws and while playing.
    pub winner: Option<Color>,
    /// Initial FEN, if the game did not start from the standard position.
    pub initial_fen: Option<String>,
    /// Current FEN, if the description carries it.
    pub fen: Option<String>,
    /// Creation time (ms since the Unix epoch).
    pub created_at: Option<u64>,
    /// White player.
    pub white: WatchPlayer,
    /// Black player.
    pub black: WatchPlayer,
}

impl WatchGame {
    /// Whether the description says the game is over.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.status.is_some_and(Status::is_final)
    }
}

/// A move line of an observed game: `{fen, lm, wc, bc}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchMove {
    /// Position after the move.
    pub fen: String,
    /// Last move in UCI (absent in the first position).
    pub last_move: Option<String>,
    /// White's clock (seconds; the unit is to be confirmed, spec D3001).
    pub white_clock: u64,
    /// Black's clock (same unit).
    pub black_clock: u64,
}

/// A line of the stream of an observed game (D3000).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatchEvent {
    /// Description of the game.
    Description(Box<WatchGame>),
    /// A move.
    Move(WatchMove),
    /// A well-formed object of an unknown shape.
    Unknown,
    /// A line that is not a JSON object; the stream goes on.
    Unparsed {
        /// The offending line, truncated for logging.
        line: String,
    },
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_owned)
}

fn watch_player(v: Option<&serde_json::Value>) -> WatchPlayer {
    let Some(v) = v else {
        return WatchPlayer::default();
    };
    let user = v.get("user");
    let name = user
        .and_then(|u| json_str(u, "name"))
        .or_else(|| user.and_then(|u| json_str(u, "id")))
        .unwrap_or_default();
    WatchPlayer {
        name,
        title: user
            .and_then(|u| json_str(u, "title"))
            .filter(|t| !t.is_empty()),
        rating: v
            .get("rating")
            .and_then(serde_json::Value::as_u64)
            .and_then(|r| u32::try_from(r).ok()),
    }
}

impl WatchEvent {
    /// Decodes one NDJSON line. Never fails: bad input is `Unparsed`.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return WatchEvent::Unparsed { line: clip(line) };
        };
        if !v.is_object() {
            return WatchEvent::Unparsed { line: clip(line) };
        }
        // A move line has `fen` plus the clocks and no `id`.
        if v.get("id").is_none() && v.get("fen").is_some_and(serde_json::Value::is_string) {
            let clock = |k: &str| v.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
            return WatchEvent::Move(WatchMove {
                fen: json_str(&v, "fen").unwrap_or_default(),
                last_move: json_str(&v, "lm").filter(|m| !m.is_empty()),
                white_clock: clock("wc"),
                black_clock: clock("bc"),
            });
        }
        let Some(id) = json_str(&v, "id") else {
            return WatchEvent::Unknown;
        };
        // The stream names the variant by key (`"standard"`); an object is
        // accepted too.
        let variant = match v.get("variant") {
            Some(serde_json::Value::String(key)) => Variant {
                key: key.clone(),
                name: key.clone(),
            },
            Some(x) => {
                serde_json::from_value::<Variant>(x.clone()).unwrap_or_else(|_| default_variant())
            }
            None => default_variant(),
        };
        let players = v.get("players");
        let status = v
            .get("status")
            .and_then(|x| serde_json::from_value::<Status>(x.clone()).ok());
        WatchEvent::Description(Box::new(WatchGame {
            id,
            variant,
            rated: v.get("rated").and_then(serde_json::Value::as_bool) == Some(true),
            speed: v
                .get("speed")
                .and_then(|x| serde_json::from_value::<Speed>(x.clone()).ok()),
            status,
            winner: v
                .get("winner")
                .and_then(|x| serde_json::from_value::<Color>(x.clone()).ok()),
            initial_fen: json_str(&v, "initialFen").filter(|f| f != "startpos"),
            fen: json_str(&v, "fen"),
            created_at: v.get("createdAt").and_then(serde_json::Value::as_u64),
            white: watch_player(players.and_then(|p| p.get("white"))),
            black: watch_player(players.and_then(|p| p.get("black"))),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    const GAME_FULL: &str = include_str!("../tests/fixtures/game_full.json");
    const GAME_STATE: &str = include_str!("../tests/fixtures/game_state.json");
    const GAME_START: &str = include_str!("../tests/fixtures/game_start.json");
    const GAME_FINISH: &str = include_str!("../tests/fixtures/game_finish.json");
    const CHALLENGE: &str = include_str!("../tests/fixtures/challenge.json");
    const ACCOUNT: &str = include_str!("../tests/fixtures/account.json");

    #[test]
    fn game_full_fixture_decodes() {
        let GameEvent::Full(full) = GameEvent::parse_line(GAME_FULL) else {
            panic!("expected gameFull");
        };
        assert_eq!(full.id, "abcd1234");
        assert!(full.variant.is_standard());
        assert_eq!(
            full.clock,
            Some(Clock {
                initial: 600_000,
                increment: 5000
            })
        );
        assert_eq!(full.speed, Some(Speed::Rapid));
        assert_eq!(full.white.name.as_deref(), Some("alice"));
        assert_eq!(full.white.title.as_deref(), Some("NM"));
        assert_eq!(full.black.rating, Some(1500));
        assert!(full.black.provisional);
        assert_eq!(full.initial_fen, "startpos");
        assert_eq!(full.created_at, Some(1_700_000_000_000));
        assert_eq!(full.state.move_list(), ["e2e4", "e7e5"]);
        assert_eq!(full.state.status, Some(Status::Started));
        assert!(!full.state.is_final());
    }

    #[test]
    fn game_state_fixture_decodes_final_state() {
        let GameEvent::State(s) = GameEvent::parse_line(GAME_STATE) else {
            panic!("expected gameState");
        };
        assert_eq!(s.status, Some(Status::Resign));
        assert_eq!(s.winner, Some(Color::White));
        assert!(s.is_final());
        assert!(s.bdraw && !s.wdraw);
        assert_eq!(
            (s.wtime, s.btime, s.winc, s.binc),
            (580_000, 570_000, 5000, 5000)
        );
    }

    #[test]
    fn opponent_gone_and_chat() {
        assert_eq!(
            GameEvent::parse_line(r#"{"type":"opponentGone","gone":true,"claimWinInSeconds":8}"#),
            GameEvent::OpponentGone {
                gone: true,
                claim_win_in_seconds: Some(8)
            }
        );
        assert_eq!(
            GameEvent::parse_line(r#"{"type":"opponentGone","gone":false}"#),
            GameEvent::OpponentGone {
                gone: false,
                claim_win_in_seconds: None
            }
        );
        assert_eq!(
            GameEvent::parse_line(
                r#"{"type":"chatLine","username":"x","text":"hi","room":"player"}"#
            ),
            GameEvent::ChatLine
        );
    }

    #[test]
    fn unknown_and_garbage_do_not_break() {
        assert_eq!(
            GameEvent::parse_line(r#"{"type":"somethingNew","a":1}"#),
            GameEvent::Unknown
        );
        assert!(matches!(
            GameEvent::parse_line("not json"),
            GameEvent::Unparsed { .. }
        ));
        assert!(matches!(
            GameEvent::parse_line("[1,2]"),
            GameEvent::Unparsed { .. }
        ));
        // A malformed number no longer sinks the object (D1347).
        assert!(matches!(
            GameEvent::parse_line(r#"{"type":"gameState","wtime":"x"}"#),
            GameEvent::State(_)
        ));
        assert!(matches!(
            GameEvent::parse_line(r#"{"type":"gameState","moves":5}"#),
            GameEvent::Unparsed { .. }
        ));
        assert_eq!(Event::parse_line(r#"{"type":"future"}"#), Event::Unknown);
        assert!(matches!(Event::parse_line("{"), Event::Unparsed { .. }));
    }

    #[test]
    fn new_status_and_speed_values_are_tolerated() {
        let s = GameEvent::parse_line(r#"{"type":"gameState","moves":"","status":"brandNew"}"#);
        let GameEvent::State(state) = s else {
            panic!("state")
        };
        assert_eq!(state.status, Some(Status::Unknown));
        assert!(state.is_final());
        let s = GameEvent::parse_line(
            r#"{"type":"gameState","moves":"","status":42,"winner":"purple"}"#,
        );
        let GameEvent::State(state) = s else {
            panic!("state")
        };
        assert_eq!(state.status, None);
        assert_eq!(state.winner, None);
    }

    #[test]
    fn unparsed_line_is_clipped_on_char_boundary() {
        let long = "é".repeat(500);
        let Event::Unparsed { line } = Event::parse_line(&long) else {
            panic!("unparsed")
        };
        assert_eq!(line.chars().count(), 200);
    }

    #[test]
    fn event_fixtures_decode() {
        let Event::GameStart(g) = Event::parse_line(GAME_START) else {
            panic!("start")
        };
        assert_eq!(g.id, "abcd1234");
        assert_eq!(g.color, Some(Color::Black));
        assert!(!g.is_my_turn);
        assert_eq!(g.opponent.and_then(|o| o.name).as_deref(), Some("alice"));
        let Event::GameFinish(g) = Event::parse_line(GAME_FINISH) else {
            panic!("finish")
        };
        assert_eq!(g.id, "abcd1234");
        assert_eq!(g.winner, Some(Color::White));
        // Older payloads use `gameId`.
        let Event::GameStart(g) =
            Event::parse_line(r#"{"type":"gameStart","game":{"gameId":"zzzz9999"}}"#)
        else {
            panic!("start")
        };
        assert_eq!(g.id, "zzzz9999");
    }

    /// Regression: the current Lichess `gameStart` carries both `gameId` and
    /// `id`; with a serde `alias` that is a duplicate field and the whole
    /// event used to be dropped as `Unparsed`, so no game ever started.
    #[test]
    fn game_start_with_both_game_id_and_id_decodes() {
        let line = r#"{"type":"gameStart","game":{"fullId":"rCRw1AuvqIhL","gameId":"rCRw1AuV","fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","color":"white","lastMove":"","source":"friend","status":{"id":20,"name":"started"},"variant":{"key":"standard","name":"Standard"},"speed":"blitz","perf":"blitz","rated":false,"hasMoved":false,"opponent":{"id":"philippe","username":"Philippe","rating":1790,"ratingDiff":-12},"isMyTurn":true,"secondsLeft":300,"compat":{"bot":false,"board":true},"id":"rCRw1AuV"}}"#;
        let Event::GameStart(g) = Event::parse_line(line) else {
            panic!("the gameStart with both ids was not decoded");
        };
        assert_eq!(g.id, "rCRw1AuV");
        assert_eq!(g.full_id.as_deref(), Some("rCRw1AuvqIhL"));
        assert_eq!(g.color, Some(Color::White));
        assert!(g.is_my_turn);
    }

    /// Realistic challenge payloads (event and `keepAliveStream` first line)
    /// decode, so a challenge sent from here is followed to its `done`.
    #[test]
    fn live_challenge_payloads_decode() {
        let challenge = r##"{"id":"7pGLxJ4F","url":"https://lichess.org/7pGLxJ4F","status":"created","compat":{"bot":false,"board":true},"challenger":{"id":"lovlas","name":"Lovlas","title":"IM","rating":2506,"provisional":true,"online":true,"lag":4},"destUser":{"id":"thibot","name":"thibot","title":null,"rating":1940,"online":true,"lag":4},"variant":{"key":"standard","name":"Standard","short":"Std"},"rated":false,"speed":"rapid","timeControl":{"type":"clock","limit":600,"increment":0,"show":"10+0"},"color":"random","finalColor":"black","perf":{"icon":"#","name":"Rapid"},"direction":"out"}"##;
        let ChallengeUpdate::Challenge(c) = ChallengeUpdate::parse_line(challenge) else {
            panic!("the first line of the challenge stream was not decoded");
        };
        assert_eq!(c.id, "7pGLxJ4F");
        assert_eq!(c.direction, Some(Direction::Out));
        let event = format!(r#"{{"type":"challenge","challenge":{challenge}}}"#);
        assert!(matches!(Event::parse_line(&event), Event::Challenge(_)));
        assert_eq!(
            ChallengeUpdate::parse_line(r#"{"done":"accepted"}"#),
            ChallengeUpdate::Done("accepted".to_owned())
        );
    }

    /// The game stream right after a `gameStart`: a realistic `gameFull`
    /// (with `type` after the other keys) and a `gameState` with the draw
    /// flags.
    #[test]
    fn live_game_stream_payloads_decode() {
        let full = r#"{"id":"rCRw1AuV","variant":{"key":"standard","name":"Standard","short":"Std"},"speed":"blitz","perf":{"name":"Blitz"},"rated":false,"createdAt":1523825103932,"white":{"id":"philippe","name":"Philippe","title":null,"rating":1790,"provisional":true},"black":{"id":"mario","name":"Mario","title":null,"rating":1805},"initialFen":"startpos","clock":{"initial":300000,"increment":0},"type":"gameFull","state":{"type":"gameState","moves":"","wtime":300000,"btime":300000,"winc":0,"binc":0,"status":"started"}}"#;
        assert!(matches!(GameEvent::parse_line(full), GameEvent::Full(_)));
        let state = r#"{"type":"gameState","moves":"e2e4 e7e5","wtime":299000,"btime":300000,"winc":0,"binc":0,"status":"started","wdraw":true}"#;
        assert!(matches!(GameEvent::parse_line(state), GameEvent::State(_)));
    }

    #[test]
    fn challenge_fixture_and_playability() {
        let Event::Challenge(c) = Event::parse_line(CHALLENGE) else {
            panic!("challenge")
        };
        assert_eq!(c.id, "chal0001");
        assert_eq!(c.direction, Some(Direction::In));
        assert_eq!(c.speed, Some(Speed::Rapid));
        assert!(c.is_playable());
        let mut bullet = (*c).clone();
        bullet.time_control = Some(ChallengeTimeControl {
            kind: "clock".into(),
            limit: Some(60),
            increment: Some(0),
            ..Default::default()
        });
        assert!(!bullet.is_playable());
        let mut variant = (*c).clone();
        variant.variant = Some(Variant {
            key: "crazyhouse".into(),
            name: "Crazyhouse".into(),
        });
        assert!(!variant.is_playable());
        let mut unlimited = (*c).clone();
        unlimited.time_control = Some(ChallengeTimeControl {
            kind: "unlimited".into(),
            ..Default::default()
        });
        assert!(!unlimited.is_playable());
        assert!(matches!(
            Event::parse_line(r#"{"type":"challengeDeclined","challenge":{"id":"x","declineReasonKey":"later"}}"#),
            Event::ChallengeDeclined(c) if c.decline_reason_key.as_deref() == Some("later")
        ));
        assert!(matches!(
            Event::parse_line(r#"{"type":"challengeCanceled","challenge":{"id":"x"}}"#),
            Event::ChallengeCanceled(_)
        ));
    }

    #[test]
    fn challenge_stream_lines() {
        assert!(
            matches!(ChallengeUpdate::parse_line(CHALLENGE_BODY), ChallengeUpdate::Challenge(c) if c.id == "chal0001")
        );
        assert_eq!(
            ChallengeUpdate::parse_line(r#"{"done":"declined"}"#),
            ChallengeUpdate::Done("declined".into())
        );
        assert!(matches!(
            ChallengeUpdate::parse_line("{}"),
            ChallengeUpdate::Unparsed { .. }
        ));
        assert!(matches!(
            ChallengeUpdate::parse_line("oops"),
            ChallengeUpdate::Unparsed { .. }
        ));
    }

    const CHALLENGE_BODY: &str = r#"{"id":"chal0001","status":"created"}"#;

    #[test]
    fn account_fixture_decodes() {
        let account: Account = serde_json::from_str(ACCOUNT).unwrap_or_default();
        assert_eq!(account.username, "Bob");
        assert_eq!(account.id, "bob");
        assert!(!account.is_bot());
        assert_eq!(account.perfs.get("rapid").map(|p| p.rating), Some(1612));
        let bot: Account = serde_json::from_str(r#"{"id":"b","title":"BOT"}"#).unwrap_or_default();
        assert!(bot.is_bot());
    }

    #[test]
    fn seek_form_fields_validate_time_control() {
        let fields = SeekParams::new(10, 5).form_fields().unwrap_or_default();
        assert!(fields.contains(&("time", "10".to_owned())));
        assert!(fields.contains(&("increment", "5".to_owned())));
        assert!(fields.contains(&("rated", "false".to_owned())));
        assert!(fields.contains(&("variant", "standard".to_owned())));
        assert!(
            !fields
                .iter()
                .any(|(k, _)| *k == "color" || *k == "ratingRange")
        );
        let mut p = SeekParams::new(15, 10);
        p.color = ColorChoice::White;
        p.rating_range = Some((1400, 1800));
        let fields = p.form_fields().unwrap_or_default();
        assert!(fields.contains(&("color", "white".to_owned())));
        assert!(fields.contains(&("ratingRange", "1400-1800".to_owned())));
        assert!(matches!(
            SeekParams::new(3, 0).form_fields(),
            Err(Error::TimeControl(_))
        ));
    }

    #[test]
    fn inverted_rating_range_is_refused() {
        let mut p = SeekParams::new(10, 5);
        p.rating_range = Some((1800, 1400));
        assert!(matches!(
            p.form_fields(),
            Err(Error::InvalidArgument { .. })
        ));
        p.rating_range = Some((1500, 1500));
        assert!(p.form_fields().is_ok());
    }

    #[test]
    fn negative_or_fractional_numbers_do_not_break_the_object() {
        let line = r#"{"type":"gameState","moves":"e2e4 e7e5","wtime":-5,"btime":1.5,
            "winc":99999999999999999999999,"binc":"x","status":"started",
            "expiration":{"idleMillis":-1,"millisToMove":2.5}}"#;
        let GameEvent::State(state) = GameEvent::parse_line(line) else {
            panic!("expected gameState");
        };
        assert_eq!(state.move_list(), ["e2e4", "e7e5"]);
        assert_eq!(
            (state.wtime, state.btime, state.winc, state.binc),
            (0, 0, 0, 0)
        );
        assert_eq!(state.expiration.map(|e| e.idle_millis), Some(0));
        let p: Player =
            serde_json::from_str(r#"{"id":"a","rating":-3,"aiLevel":2.5}"#).unwrap_or_default();
        assert_eq!(
            (p.id.as_deref(), p.rating, p.ai_level),
            (Some("a"), None, None)
        );
        let c: Clock = serde_json::from_str(r#"{"initial":-1,"increment":3}"#).unwrap_or(Clock {
            initial: 9,
            increment: 9,
        });
        assert_eq!((c.initial, c.increment), (0, 3));
    }

    #[test]
    fn challenge_form_fields_keep_the_stream_alive() {
        let fields = ChallengeParams::new(600, 5)
            .form_fields()
            .unwrap_or_default();
        assert!(fields.contains(&("clock.limit", "600".to_owned())));
        assert!(fields.contains(&("keepAliveStream", "true".to_owned())));
        assert!(matches!(
            ChallengeParams::new(60, 0).form_fields(),
            Err(Error::TimeControl(_))
        ));
    }

    #[test]
    fn watch_lines_decode_and_never_panic() {
        let desc = r#"{"id":"abcd1234","rated":true,"variant":"standard","speed":"blitz","status":"started","players":{"white":{"user":{"name":"Alice","id":"alice","title":"GM"},"rating":2500},"black":{"user":{"name":"Bob","id":"bob"},"rating":1500}},"fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}"#;
        let WatchEvent::Description(g) = WatchEvent::parse_line(desc) else {
            panic!("expected a description");
        };
        assert_eq!(g.id, "abcd1234");
        assert!(g.rated && !g.is_final());
        assert_eq!(g.white.name, "Alice");
        assert_eq!(g.white.title.as_deref(), Some("GM"));
        assert_eq!(g.black.rating, Some(1500));
        assert_eq!(g.speed, Some(Speed::Blitz));
        let mv = WatchEvent::parse_line(
            r#"{"fen":"8/8/8/8/8/8/8/K6k b - - 0 1","lm":"e2e4","wc":180,"bc":179}"#,
        );
        assert_eq!(
            mv,
            WatchEvent::Move(WatchMove {
                fen: "8/8/8/8/8/8/8/K6k b - - 0 1".to_owned(),
                last_move: Some("e2e4".to_owned()),
                white_clock: 180,
                black_clock: 179,
            })
        );
        for junk in [
            "",
            "nope",
            "[1,2]",
            "42",
            r#"{"fen":7}"#,
            r#"{"x":1}"#,
            "{\"id\":5}",
        ] {
            let e = WatchEvent::parse_line(junk);
            assert!(
                matches!(e, WatchEvent::Unparsed { .. } | WatchEvent::Unknown),
                "{junk}: {e:?}"
            );
        }
        let fin = WatchEvent::parse_line(r#"{"id":"abcd1234","status":"resign","winner":"black"}"#);
        assert!(
            matches!(&fin, WatchEvent::Description(g) if g.is_final() && g.winner == Some(Color::Black))
        );
    }
}
