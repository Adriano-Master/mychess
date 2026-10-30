//! The configuration of a tournament between engines and its validation
//! (spec M8-2, D1263-D1266, D1269-D1272, D1276, D1281, D1288, D1289).
//!
//! Plain data. [`TournamentConfig::validate`] checks everything that needs no
//! file system; [`TournamentConfig::check_files`] (blocking, for `perform`)
//! checks the book and the opening suite. The same type is saved with the
//! tournament (`store`) and, in part, in the preferences.
//!
//! Scid: `comp.tcl` `compInit` (19-212), `checkTimeControl` (214-232) and
//! `compOk` validations (248-292).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::resources::MAX_CONCURRENCY;

/// Fewest participants.
pub const MIN_PARTICIPANTS: usize = 2;
/// Most participants (D1263).
pub const MAX_PARTICIPANTS: usize = 32;
/// Cycles (games per pair), as the Scid's "rounds" (D1264).
pub const CYCLES_RANGE: (u32, u32) = (1, 100);
/// Longest base time of a clock, in milliseconds (the Scid allows 7200 s).
pub const MAX_BASE_MS: u64 = 7_200_000;
/// Longest increment of a clock, in milliseconds (the Scid allows 60 s).
pub const MAX_INCREMENT_MS: u64 = 60_000;
/// Longest time per move, in milliseconds (the Scid allows 3600 s).
pub const MAX_MOVETIME_MS: u64 = 3_600_000;
/// Deepest fixed depth.
pub const MAX_DEPTH: u32 = 100;
/// The leeway of `MoveTime` in per cent of the time (D1267).
pub const LEEWAY_RANGE: (u32, u32) = (100, 300);
/// The margin of the watchdog in milliseconds (D1268).
pub const MARGIN_RANGE_MS: (u32, u32) = (1_000, 30_000);
/// The ceiling per move of `Depth`/`Nodes`, in seconds (D1268).
pub const DEPTH_LIMIT_RANGE_SECS: (u32, u32) = (10, 3_600);
/// Retries of a broken game (D1269).
pub const RETRIES_MAX: u8 = 3;
/// The plies of the book limit (D1271).
pub const BOOK_PLY_RANGE: (u32, u32) = (1, 40);
/// Games above which the estimate warns (D1289).
pub const WARN_GAMES: usize = 1000;
/// Hours above which the estimate warns (D1289).
pub const WARN_HOURS: u64 = 24;
/// The largest integer a TOML file keeps (`i64::MAX`).
const MAX_TOML_INT: u64 = u64::MAX >> 1;
/// Longest `name` and `site` of a tournament, in characters (D1309): the name
/// becomes part of the file names.
pub const MAX_TITLE_CHARS: usize = 64;
/// Games above which the state file is reported as large (D1315).
pub const LARGE_STATE_GAMES: usize = 10_000;
/// A generous estimate of the bytes one game takes in the state file (its
/// `[[games]]` table plus its entry in the queue), redo games included
/// (D1315).
pub const STATE_BYTES_PER_GAME: u64 = 384;
/// Largest value of the rule numbers of the adjudication (centipawns, moves).
pub const MAX_RULE_VALUE: u32 = 100_000;
/// Most pieces a tablebase can know.
pub const MAX_TB_PIECES: u32 = 7;

/// One engine of the tournament (D1263).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    /// The engine, by its stable identity in `engines.toml`
    /// (`EngineId`).
    pub engine: u64,
    /// The name shown and written to the tags; unique in the tournament.
    pub name: String,
    /// The name of the engine in the list when the tournament was made, to
    /// find it again if the identity is gone (D1285).
    #[serde(default)]
    pub engine_name: String,
    /// UCI options that override the saved ones (Hash, Threads, level...).
    #[serde(default)]
    pub options: Vec<(String, String)>,
    /// The rating written to `WhiteElo`/`BlackElo`.
    #[serde(default)]
    pub elo: Option<u16>,
}

impl Participant {
    /// A participant with no option of its own.
    pub fn new(engine: u64, name: &str) -> Participant {
        Participant {
            engine,
            name: name.to_owned(),
            engine_name: String::new(),
            options: Vec::new(),
            elo: None,
        }
    }
}

/// The format of the tournament (D1264).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Scheduling {
    /// Everybody against everybody; the games of a pair in sequence.
    #[default]
    RoundRobin,
    /// Everybody plays in every round (circle method).
    Carousel,
    /// The tested participant against every other.
    Gauntlet,
}

/// How the games are limited (D1266). Both engines have the same limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeControl {
    /// A clock per side with an increment.
    Clock {
        /// The initial time, in milliseconds.
        base_ms: u64,
        /// Added after each move, in milliseconds.
        increment_ms: u64,
    },
    /// A fixed time per move, in milliseconds.
    MoveTime {
        /// The time.
        ms: u64,
    },
    /// `go depth`.
    Depth(u32),
    /// `go nodes`.
    Nodes(u64),
}

impl Default for TimeControl {
    /// 60 s + 1 s, as the Scid (`start.tcl` 193-208).
    fn default() -> TimeControl {
        TimeControl::Clock {
            base_ms: 60_000,
            increment_ms: 1_000,
        }
    }
}

/// What to do with a game lost on time (D1269).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OnTimeLoss {
    /// The engine loses the game.
    #[default]
    Loss,
    /// The game is played again (as many times as `max_retries`).
    Replay,
}

/// How the note of the engine is written in the comments (D1281).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScoreComment {
    /// No note.
    None,
    /// `+1.50`.
    Text,
    /// `[% +1.50]`.
    Pct,
    /// `[%eval 1.50,18]` (the format of the graphs, M4-7).
    #[default]
    Eval,
}

/// The comments written on each move (D1281).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentOptions {
    /// The note of the engine.
    pub score: ScoreComment,
    /// `[%clk h:mm:ss]`.
    pub clock: bool,
    /// `[%emt h:mm:ss]`.
    pub move_time: bool,
}

impl Default for CommentOptions {
    fn default() -> CommentOptions {
        CommentOptions {
            score: ScoreComment::Eval,
            clock: true,
            move_time: true,
        }
    }
}

/// Where the games start from (D1272).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OpeningSource {
    /// The standard position.
    #[default]
    Initial,
    /// Every game from `start_fen`.
    CurrentPosition,
    /// Only the first game from `start_fen`; the others from the standard
    /// position.
    CurrentPositionFirstGame,
    /// Moves of a Polyglot book (D1271).
    Book,
    /// A suite of openings in an EPD or PGN file.
    Suite,
}

/// How a book move is picked (D1271).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BookPick {
    /// The heaviest move.
    Best,
    /// Random, in proportion to the weight (as the Scid).
    #[default]
    Weighted,
}

/// The book of the tournament.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookConfig {
    /// The `.bin` file.
    pub path: PathBuf,
    /// How a move is picked.
    #[serde(default)]
    pub pick: BookPick,
}

/// The order of the openings of a suite (D1273).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SuiteOrder {
    /// As in the file.
    #[default]
    Sequential,
    /// Shuffled with the seed of the tournament.
    Random,
}

/// A suite of openings (D1272).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteConfig {
    /// The EPD or PGN file.
    pub path: PathBuf,
    /// The order.
    #[serde(default)]
    pub order: SuiteOrder,
    /// The plies taken from each game of a PGN suite.
    #[serde(default = "default_suite_plies")]
    pub plies: u32,
}

fn default_suite_plies() -> u32 {
    8
}

/// The automatic adjudication of a won game (D1276).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResignRule {
    /// The score, in centipawns, that counts as lost.
    pub centipawns: u32,
    /// The moves in a row.
    pub moves: u32,
    /// The engine of the winner must agree.
    pub require_both: bool,
}

impl Default for ResignRule {
    fn default() -> ResignRule {
        ResignRule {
            centipawns: 600,
            moves: 4,
            require_both: true,
        }
    }
}

/// The automatic adjudication of a draw (D1276).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawRule {
    /// Only after this move number (full moves).
    pub after_move: u32,
    /// The scores must stay within this many centipawns of zero.
    pub centipawns: u32,
    /// The moves in a row, of each side.
    pub moves: u32,
}

impl Default for DrawRule {
    fn default() -> DrawRule {
        DrawRule {
            after_move: 40,
            centipawns: 10,
            moves: 8,
        }
    }
}

/// The adjudication rules, all off by default (D1276, D1277).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjudication {
    /// Decide a game that is clearly won.
    #[serde(default)]
    pub resign: Option<ResignRule>,
    /// Decide a game that is clearly drawn.
    #[serde(default)]
    pub draw: Option<DrawRule>,
    /// A draw at this many plies.
    #[serde(default)]
    pub max_plies: Option<u32>,
    /// Decide with a tablebase when there is a probe.
    #[serde(default)]
    pub tablebase: bool,
    /// The most pieces the tablebase decides.
    #[serde(default = "default_tb_pieces")]
    pub tb_pieces: u32,
}

fn default_tb_pieces() -> u32 {
    5
}

impl Default for Adjudication {
    fn default() -> Adjudication {
        Adjudication {
            resign: None,
            draw: None,
            max_plies: None,
            tablebase: false,
            tb_pieces: default_tb_pieces(),
        }
    }
}

/// The policy for broken games (D1269).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailurePolicy {
    /// A game lost on time.
    pub on_time_loss: OnTimeLoss,
    /// Times a game is played again after the engine broke (0 to 3).
    pub max_retries: u8,
    /// Failures of one participant before the tournament pauses.
    pub max_failures: u32,
}

impl Default for FailurePolicy {
    fn default() -> FailurePolicy {
        FailurePolicy {
            on_time_loss: OnTimeLoss::Loss,
            max_retries: 1,
            max_failures: 3,
        }
    }
}

/// An extra destination of the games, besides the PGN of the tournament
/// (D1279).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExtraDestination {
    /// Only the PGN of the tournament.
    #[default]
    None,
    /// The current database slot.
    CurrentSlot,
    /// The Clipbase.
    Clipbase,
}

/// Everything the dialog of the tournament asks (D1263-D1289).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TournamentConfig {
    /// The `Event` tag.
    pub name: String,
    /// The `Site` tag.
    pub site: String,
    /// The participants, in order of registration.
    pub participants: Vec<Participant>,
    /// The format.
    pub scheduling: Scheduling,
    /// The tested participant of `Gauntlet` (index in `participants`).
    pub tested: usize,
    /// Games per pair.
    pub cycles: u32,
    /// The limits of the games.
    pub time: TimeControl,
    /// `MoveTime`: how much past the time is tolerated, per cent (D1267).
    pub leeway_percent: u32,
    /// The margin of the watchdog, in milliseconds (D1268).
    pub margin_ms: u32,
    /// The ceiling per move of `Depth`/`Nodes`, in seconds (D1268).
    pub depth_limit_secs: u32,
    /// Where the games start from.
    pub opening: OpeningSource,
    /// The position of `CurrentPosition*` (a FEN), copied on OK (D1272).
    #[serde(default)]
    pub start_fen: Option<String>,
    /// The book, used by `OpeningSource::Book`.
    #[serde(default)]
    pub book: Option<BookConfig>,
    /// The suite, used by `OpeningSource::Suite`.
    #[serde(default)]
    pub suite: Option<SuiteConfig>,
    /// Both games of a pair start from the same opening (D1273).
    pub pair_openings: bool,
    /// The most plies of book (D1271).
    #[serde(default)]
    pub book_ply_limit: Option<u32>,
    /// The seed of the draws: openings and book moves (D1273).
    pub seed: u64,
    /// Think on the time of the opponent (D1270).
    pub ponder: bool,
    /// `Hash` for every engine that has it, in MB (D1288).
    #[serde(default)]
    pub hash_mb: Option<u32>,
    /// `Threads` for every engine that has it (D1288).
    #[serde(default)]
    pub threads: Option<u32>,
    /// `SyzygyPath` for every engine that has it.
    #[serde(default)]
    pub syzygy_path: Option<String>,
    /// Start the engines again before every game (D1288).
    pub restart_each_game: bool,
    /// The automatic draws by repetition and 50 moves (D1274).
    pub claim_draws: bool,
    /// The adjudication.
    #[serde(default)]
    pub adjudication: Adjudication,
    /// The broken games.
    #[serde(default)]
    pub failures: FailurePolicy,
    /// The comments of each move.
    #[serde(default)]
    pub comments: CommentOptions,
    /// An extra destination of the games.
    #[serde(default)]
    pub destination: ExtraDestination,
    /// Games at once, one per lane, each with its own engines (D2500).
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

fn default_concurrency() -> u32 {
    1
}

impl Default for TournamentConfig {
    fn default() -> TournamentConfig {
        TournamentConfig {
            name: "MyChess Tournament".to_owned(),
            site: "MyChess".to_owned(),
            participants: Vec::new(),
            scheduling: Scheduling::RoundRobin,
            tested: 0,
            cycles: 2,
            time: TimeControl::default(),
            leeway_percent: 175,
            margin_ms: 2_000,
            depth_limit_secs: 600,
            opening: OpeningSource::Initial,
            start_fen: None,
            book: None,
            suite: None,
            pair_openings: true,
            book_ply_limit: None,
            seed: 0,
            ponder: false,
            hash_mb: None,
            threads: None,
            syzygy_path: None,
            restart_each_game: false,
            claim_draws: true,
            adjudication: Adjudication::default(),
            failures: FailurePolicy::default(),
            comments: CommentOptions::default(),
            destination: ExtraDestination::None,
            concurrency: 1,
        }
    }
}

/// Why a configuration is refused (D1289).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Fewer than two participants.
    #[error("a tournament needs at least {MIN_PARTICIPANTS} participants")]
    TooFewParticipants,
    /// More than 32 participants.
    #[error("a tournament has at most {MAX_PARTICIPANTS} participants")]
    TooManyParticipants,
    /// A participant has no name.
    #[error("participant {0} has no name")]
    EmptyName(usize),
    /// Two participants have the same name.
    #[error("duplicate participant name `{0}`")]
    DuplicateName(String),
    /// The cycles are out of range.
    #[error("cycles must be between {} and {}", CYCLES_RANGE.0, CYCLES_RANGE.1)]
    BadCycles,
    /// The tested participant of the gauntlet does not exist.
    #[error("the tested participant does not exist")]
    BadTested,
    /// The time control is incoherent.
    #[error("incoherent time control: {0}")]
    BadTime(&'static str),
    /// The leeway, the margin or the ceiling are out of range.
    #[error("watchdog setting out of range: {0}")]
    BadWatchdog(&'static str),
    /// A source of openings is chosen but its data is missing.
    #[error("the opening source needs {0}")]
    MissingOpeningData(&'static str),
    /// The FEN of the start position is not valid.
    #[error("invalid start position")]
    BadStartFen,
    /// The book limit is out of range.
    #[error("book ply limit out of range")]
    BadBookLimit,
    /// The seed does not fit a signed 64-bit number (the files keep it so).
    #[error("seed out of range")]
    BadSeed,
    /// `name` or `site` is longer than [`MAX_TITLE_CHARS`] characters.
    #[error("name and site have at most {MAX_TITLE_CHARS} characters")]
    TitleTooLong,
    /// A number of the adjudication rules is zero or out of range.
    #[error("adjudication setting out of range: {0}")]
    BadAdjudication(&'static str),
    /// `max_failures` is zero or out of range.
    #[error("failure limit out of range")]
    BadFailureLimit,
    /// The schedule would make a state file larger than the limit.
    #[error("the schedule is too large to be saved")]
    TooManyGames,
    /// The games at once are zero or above [`MAX_CONCURRENCY`] (D2503).
    #[error("games at once must be between 1 and {MAX_CONCURRENCY}")]
    BadConcurrency,
    /// The book file does not exist.
    #[error("book file not found: {0}")]
    BookNotFound(PathBuf),
    /// The suite file cannot be read.
    #[error("opening suite not readable: {0}")]
    SuiteUnreadable(PathBuf),
}

impl ConfigError {
    /// The Fluent key of the message (the GUI translates from the variant).
    pub fn message_key(&self) -> &'static str {
        match self {
            ConfigError::TooFewParticipants => "tournament-error-too-few",
            ConfigError::TooManyParticipants => "tournament-error-too-many",
            ConfigError::EmptyName(_) => "tournament-error-empty-name",
            ConfigError::DuplicateName(_) => "tournament-error-duplicate-name",
            ConfigError::BadCycles => "tournament-error-cycles",
            ConfigError::BadTested => "tournament-error-tested",
            ConfigError::BadTime(_) => "tournament-error-time",
            ConfigError::BadWatchdog(_) => "tournament-error-watchdog",
            ConfigError::MissingOpeningData(_) => "tournament-error-opening-data",
            ConfigError::BadStartFen => "tournament-error-start-fen",
            ConfigError::BadBookLimit => "tournament-error-book-limit",
            ConfigError::BadSeed => "tournament-error-seed",
            ConfigError::TitleTooLong => "tournament-error-title-long",
            ConfigError::BadAdjudication(_) => "tournament-error-adjudication",
            ConfigError::BadFailureLimit => "tournament-error-failure-limit",
            ConfigError::TooManyGames => "tournament-error-too-many-games",
            ConfigError::BadConcurrency => "tournament-error-concurrency",
            ConfigError::BookNotFound(_) => "tournament-error-book-missing",
            ConfigError::SuiteUnreadable(_) => "tournament-error-suite",
        }
    }
}

/// The estimate shown before the start (D1289).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    /// Number of games.
    pub games: usize,
    /// The longest the whole tournament can take.
    pub max_duration: Duration,
    /// More than [`WARN_GAMES`] games or [`WARN_HOURS`] hours.
    pub warn: bool,
    /// More than [`LARGE_STATE_GAMES`] games: the state file is rewritten
    /// after every game and its size grows with the schedule (D1315).
    pub large_state: bool,
    /// The estimated bytes of the state file.
    pub state_bytes: u64,
    /// The games at once the estimate divides the duration by (D2503).
    pub lanes: u32,
}

impl TournamentConfig {
    /// The number of games of the schedule (D1289).
    pub fn game_count(&self) -> usize {
        let n = self.participants.len();
        let cycles = usize::try_from(self.cycles).unwrap_or(usize::MAX);
        let per_cycle = match self.scheduling {
            Scheduling::RoundRobin | Scheduling::Carousel => n * n.saturating_sub(1) / 2,
            Scheduling::Gauntlet => n.saturating_sub(1),
        };
        per_cycle.saturating_mul(cycles)
    }

    /// The games and the longest duration (D1289): `games * 2 * (base +
    /// increment * 60 moves)` with a clock, `2 * 60 moves * time` per game
    /// with a time per move. Depth and nodes have no estimate (zero).
    pub fn estimate(&self) -> Estimate {
        let games = self.game_count();
        let per_game_ms: u64 = match self.time {
            TimeControl::Clock {
                base_ms,
                increment_ms,
            } => 2u64.saturating_mul(base_ms.saturating_add(increment_ms.saturating_mul(60))),
            TimeControl::MoveTime { ms } => 2u64.saturating_mul(60).saturating_mul(ms),
            TimeControl::Depth(_) | TimeControl::Nodes(_) => 0,
        };
        let total_ms = per_game_ms.saturating_mul(u64::try_from(games).unwrap_or(u64::MAX));
        let lanes = self.concurrency.clamp(1, MAX_CONCURRENCY);
        let total_ms = total_ms.div_ceil(u64::from(lanes));
        let max_duration = Duration::from_millis(total_ms);
        Estimate {
            games,
            max_duration,
            warn: games > WARN_GAMES || max_duration.as_secs() > WARN_HOURS * 3600,
            large_state: games > LARGE_STATE_GAMES,
            state_bytes: self.state_bytes(),
            lanes,
        }
    }

    /// The bytes the state file is expected to take for the whole schedule
    /// (D1315). The file is rewritten as a whole after every game, and the
    /// PGN is appended by rewriting it too (`save_pgn` with `Append` is an
    /// atomic copy), so the cost of a game grows with the schedule.
    pub fn state_bytes(&self) -> u64 {
        u64::try_from(self.game_count())
            .unwrap_or(u64::MAX)
            .saturating_mul(STATE_BYTES_PER_GAME)
    }

    /// Checks everything that needs no file (D1289).
    ///
    /// # Errors
    ///
    /// The first problem found, as a [`ConfigError`].
    pub fn validate(&self) -> Result<(), ConfigError> {
        let n = self.participants.len();
        if n < MIN_PARTICIPANTS {
            return Err(ConfigError::TooFewParticipants);
        }
        if n > MAX_PARTICIPANTS {
            return Err(ConfigError::TooManyParticipants);
        }
        let mut seen = HashSet::new();
        for (i, p) in self.participants.iter().enumerate() {
            let name = p.name.trim();
            if name.is_empty() {
                return Err(ConfigError::EmptyName(i));
            }
            // Names are unique without regard to case: they become file and
            // table labels (multiplataforma.md).
            if !seen.insert(name.to_lowercase()) {
                return Err(ConfigError::DuplicateName(name.to_owned()));
            }
        }
        if self.cycles < CYCLES_RANGE.0 || self.cycles > CYCLES_RANGE.1 {
            return Err(ConfigError::BadCycles);
        }
        if self.concurrency == 0 || self.concurrency > MAX_CONCURRENCY {
            return Err(ConfigError::BadConcurrency);
        }
        if self.name.chars().count() > MAX_TITLE_CHARS
            || self.site.chars().count() > MAX_TITLE_CHARS
        {
            return Err(ConfigError::TitleTooLong);
        }
        if self.state_bytes() > super::store::MAX_STORE_BYTES {
            return Err(ConfigError::TooManyGames);
        }
        if i64::try_from(self.seed).is_err() {
            return Err(ConfigError::BadSeed);
        }
        if self.scheduling == Scheduling::Gauntlet && self.tested >= n {
            return Err(ConfigError::BadTested);
        }
        if self
            .participants
            .iter()
            .any(|p| i64::try_from(p.engine).is_err())
        {
            return Err(ConfigError::BadTime("engine identity out of range"));
        }
        if self.failures.max_failures == 0
            || self.failures.max_failures > 100
            || self.failures.max_retries > RETRIES_MAX
        {
            return Err(ConfigError::BadFailureLimit);
        }
        self.validate_adjudication()?;
        self.validate_time()?;
        self.validate_openings()?;
        Ok(())
    }

    fn validate_adjudication(&self) -> Result<(), ConfigError> {
        let rules = &self.adjudication;
        let bad = |what: &'static str| Err(ConfigError::BadAdjudication(what));
        let in_range = |v: u32| (1..=MAX_RULE_VALUE).contains(&v);
        if let Some(rule) = rules.resign
            && !(in_range(rule.centipawns) && in_range(rule.moves))
        {
            return bad("resign");
        }
        if let Some(rule) = rules.draw
            && !(rule.after_move <= MAX_RULE_VALUE
                && rule.centipawns <= MAX_RULE_VALUE
                && in_range(rule.moves))
        {
            return bad("draw");
        }
        if rules.max_plies.is_some_and(|plies| !in_range(plies)) {
            return bad("max plies");
        }
        if !(1..=MAX_TB_PIECES).contains(&rules.tb_pieces) {
            return bad("tablebase pieces");
        }
        Ok(())
    }

    fn validate_time(&self) -> Result<(), ConfigError> {
        match self.time {
            TimeControl::Clock {
                base_ms,
                increment_ms,
            } => {
                if base_ms > MAX_BASE_MS || increment_ms > MAX_INCREMENT_MS {
                    return Err(ConfigError::BadTime("clock too long"));
                }
                if base_ms == 0 && increment_ms == 0 {
                    return Err(ConfigError::BadTime("zero base needs an increment"));
                }
            }
            TimeControl::MoveTime { ms } => {
                if ms == 0 || ms > MAX_MOVETIME_MS {
                    return Err(ConfigError::BadTime("time per move out of range"));
                }
            }
            TimeControl::Depth(d) => {
                if d == 0 || d > MAX_DEPTH {
                    return Err(ConfigError::BadTime("depth out of range"));
                }
            }
            TimeControl::Nodes(n) => {
                // TOML integers are signed 64-bit numbers.
                if n == 0 || i64::try_from(n).is_err() {
                    return Err(ConfigError::BadTime("nodes out of range"));
                }
            }
        }
        let in_range = |v: u32, (low, high): (u32, u32)| (low..=high).contains(&v);
        if !in_range(self.leeway_percent, LEEWAY_RANGE) {
            return Err(ConfigError::BadWatchdog("leeway"));
        }
        if !in_range(self.margin_ms, MARGIN_RANGE_MS) {
            return Err(ConfigError::BadWatchdog("margin"));
        }
        if !in_range(self.depth_limit_secs, DEPTH_LIMIT_RANGE_SECS) {
            return Err(ConfigError::BadWatchdog("ceiling"));
        }
        Ok(())
    }

    fn validate_openings(&self) -> Result<(), ConfigError> {
        match self.opening {
            OpeningSource::Initial => {}
            OpeningSource::CurrentPosition | OpeningSource::CurrentPositionFirstGame => {
                let Some(fen) = &self.start_fen else {
                    return Err(ConfigError::MissingOpeningData("a position"));
                };
                if mychess_core::Position::from_fen(fen).is_err() {
                    return Err(ConfigError::BadStartFen);
                }
            }
            OpeningSource::Book => {
                if self.book.is_none() {
                    return Err(ConfigError::MissingOpeningData("a book"));
                }
            }
            OpeningSource::Suite => {
                if self.suite.is_none() {
                    return Err(ConfigError::MissingOpeningData("a suite file"));
                }
            }
        }
        if let Some(limit) = self.book_ply_limit
            && (limit < BOOK_PLY_RANGE.0 || limit > BOOK_PLY_RANGE.1)
        {
            return Err(ConfigError::BadBookLimit);
        }
        Ok(())
    }

    /// Checks the book and the suite on disk (blocking: run it from
    /// `perform`).
    ///
    /// # Errors
    ///
    /// [`ConfigError::BookNotFound`] or [`ConfigError::SuiteUnreadable`].
    pub fn check_files(&self) -> Result<(), ConfigError> {
        if self.opening == OpeningSource::Book
            && let Some(book) = &self.book
            && !book.path.is_file()
        {
            return Err(ConfigError::BookNotFound(book.path.clone()));
        }
        if self.opening == OpeningSource::Suite
            && let Some(suite) = &self.suite
            && std::fs::File::open(&suite.path).is_err()
        {
            return Err(ConfigError::SuiteUnreadable(suite.path.clone()));
        }
        Ok(())
    }

    /// Brings every number into its range (the dialog and the preferences
    /// hold text, so a value can be wild) and cuts `name` and `site`. The
    /// list of participants is not touched, except for the identity of each
    /// engine, which is cut to a signed 64-bit number.
    #[must_use]
    pub fn clamped(mut self) -> TournamentConfig {
        self.cycles = self.cycles.clamp(CYCLES_RANGE.0, CYCLES_RANGE.1);
        self.leeway_percent = self.leeway_percent.clamp(LEEWAY_RANGE.0, LEEWAY_RANGE.1);
        self.margin_ms = self.margin_ms.clamp(MARGIN_RANGE_MS.0, MARGIN_RANGE_MS.1);
        self.depth_limit_secs = self
            .depth_limit_secs
            .clamp(DEPTH_LIMIT_RANGE_SECS.0, DEPTH_LIMIT_RANGE_SECS.1);
        self.seed &= u64::MAX >> 1;
        self.concurrency = self.concurrency.clamp(1, MAX_CONCURRENCY);
        self.failures.max_retries = self.failures.max_retries.min(RETRIES_MAX);
        self.failures.max_failures = self.failures.max_failures.clamp(1, 100);
        if let Some(limit) = &mut self.book_ply_limit {
            *limit = (*limit).clamp(BOOK_PLY_RANGE.0, BOOK_PLY_RANGE.1);
        }
        // Every number that ends in a TOML file fits a signed 64-bit integer
        // (D1310), and the times keep to the limits of the dialog.
        self.time = match self.time {
            TimeControl::Clock {
                base_ms,
                increment_ms,
            } => TimeControl::Clock {
                base_ms: base_ms.min(MAX_BASE_MS),
                increment_ms: increment_ms.min(MAX_INCREMENT_MS),
            },
            TimeControl::MoveTime { ms } => TimeControl::MoveTime {
                ms: ms.clamp(1, MAX_MOVETIME_MS),
            },
            TimeControl::Depth(depth) => TimeControl::Depth(depth.clamp(1, MAX_DEPTH)),
            TimeControl::Nodes(nodes) => TimeControl::Nodes(nodes.clamp(1, MAX_TOML_INT)),
        };
        for participant in &mut self.participants {
            participant.engine = participant.engine.min(MAX_TOML_INT);
        }
        let rules = &mut self.adjudication;
        let rule_value = |v: u32| v.clamp(1, MAX_RULE_VALUE);
        if let Some(rule) = &mut rules.resign {
            rule.centipawns = rule_value(rule.centipawns);
            rule.moves = rule_value(rule.moves);
        }
        if let Some(rule) = &mut rules.draw {
            rule.after_move = rule.after_move.min(MAX_RULE_VALUE);
            rule.centipawns = rule.centipawns.min(MAX_RULE_VALUE);
            rule.moves = rule_value(rule.moves);
        }
        if let Some(plies) = &mut rules.max_plies {
            *plies = rule_value(*plies);
        }
        rules.tb_pieces = rules.tb_pieces.clamp(1, MAX_TB_PIECES);
        self.name = self.name.chars().take(MAX_TITLE_CHARS).collect();
        self.site = self.site.chars().take(MAX_TITLE_CHARS).collect();
        self
    }

    /// The names of the participants, in order.
    pub fn names(&self) -> Vec<String> {
        self.participants.iter().map(|p| p.name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(n: usize) -> TournamentConfig {
        TournamentConfig {
            participants: (0..n)
                .map(|i| Participant::new(1, &format!("Engine {i}")))
                .collect(),
            ..TournamentConfig::default()
        }
    }

    #[test]
    fn a_plain_configuration_is_valid() {
        assert_eq!(config(3).validate(), Ok(()));
    }

    #[test]
    fn the_same_engine_twice_with_distinct_names_is_accepted() {
        let mut c = config(2);
        c.participants[0].engine = 7;
        c.participants[1].engine = 7;
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn rejects_too_few_and_too_many_participants() {
        assert_eq!(config(1).validate(), Err(ConfigError::TooFewParticipants));
        assert_eq!(config(33).validate(), Err(ConfigError::TooManyParticipants));
        assert_eq!(config(32).validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicate_and_empty_names_ignoring_case() {
        let mut c = config(3);
        c.participants[2].name = " engine 0 ".to_owned();
        assert_eq!(
            c.validate(),
            Err(ConfigError::DuplicateName("engine 0".to_owned()))
        );
        c.participants[2].name = "  ".to_owned();
        assert_eq!(c.validate(), Err(ConfigError::EmptyName(2)));
    }

    #[test]
    fn rejects_bad_cycles_and_tested() {
        let mut c = config(3);
        c.cycles = 0;
        assert_eq!(c.validate(), Err(ConfigError::BadCycles));
        c.cycles = 101;
        assert_eq!(c.validate(), Err(ConfigError::BadCycles));
        c.cycles = 2;
        c.scheduling = Scheduling::Gauntlet;
        c.tested = 3;
        assert_eq!(c.validate(), Err(ConfigError::BadTested));
        c.tested = 2;
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn checks_the_time_control() {
        let mut c = config(2);
        for (time, ok) in [
            (
                TimeControl::Clock {
                    base_ms: 0,
                    increment_ms: 0,
                },
                false,
            ),
            (
                TimeControl::Clock {
                    base_ms: 0,
                    increment_ms: 500,
                },
                true,
            ),
            (
                TimeControl::Clock {
                    base_ms: 7_200_001,
                    increment_ms: 0,
                },
                false,
            ),
            (TimeControl::MoveTime { ms: 0 }, false),
            (TimeControl::MoveTime { ms: 3_600_000 }, true),
            (TimeControl::Depth(0), false),
            (TimeControl::Depth(12), true),
            (TimeControl::Nodes(0), false),
            (TimeControl::Nodes(10_000), true),
        ] {
            c.time = time;
            assert_eq!(c.validate().is_ok(), ok, "{time:?}");
        }
    }

    #[test]
    fn checks_the_watchdog_settings() {
        let mut c = config(2);
        c.leeway_percent = 99;
        assert_eq!(c.validate(), Err(ConfigError::BadWatchdog("leeway")));
        c.leeway_percent = 175;
        c.margin_ms = 500;
        assert_eq!(c.validate(), Err(ConfigError::BadWatchdog("margin")));
        assert_eq!(c.clamped().margin_ms, 1_000);
    }

    #[test]
    fn opening_sources_need_their_data() {
        let mut c = config(2);
        c.opening = OpeningSource::Book;
        assert!(matches!(
            c.validate(),
            Err(ConfigError::MissingOpeningData(_))
        ));
        c.opening = OpeningSource::CurrentPosition;
        c.start_fen = Some("not a fen".to_owned());
        assert_eq!(c.validate(), Err(ConfigError::BadStartFen));
        c.start_fen = Some("8/8/4k3/8/8/3K4/8/8 w - - 0 1".to_owned());
        assert_eq!(c.validate(), Ok(()));
        c.book_ply_limit = Some(41);
        assert_eq!(c.validate(), Err(ConfigError::BadBookLimit));
    }

    #[test]
    fn the_seed_and_the_nodes_must_fit_a_toml_integer() {
        let mut c = config(2);
        c.seed = u64::MAX;
        assert_eq!(c.validate(), Err(ConfigError::BadSeed));
        assert_eq!(c.clone().clamped().seed, MAX_TOML_INT);
        c.seed = 5;
        c.time = TimeControl::Nodes(u64::MAX);
        assert!(matches!(c.validate(), Err(ConfigError::BadTime(_))));
    }

    #[test]
    fn counts_games_per_format() {
        let mut c = config(4);
        c.cycles = 2;
        assert_eq!(c.game_count(), 12);
        c.scheduling = Scheduling::Carousel;
        assert_eq!(c.game_count(), 12);
        c.scheduling = Scheduling::Gauntlet;
        assert_eq!(c.game_count(), 6);
    }

    #[test]
    fn estimates_the_longest_duration_and_warns() {
        let mut c = config(2);
        c.cycles = 1;
        c.time = TimeControl::Clock {
            base_ms: 60_000,
            increment_ms: 1_000,
        };
        let e = c.estimate();
        assert_eq!(e.games, 1);
        assert_eq!(e.max_duration, Duration::from_mins(4));
        assert!(!e.warn);
        c.time = TimeControl::MoveTime { ms: 3_600_000 };
        assert!(c.estimate().warn);
        let mut big = config(32);
        big.cycles = 4;
        big.time = TimeControl::Depth(8);
        assert!(big.estimate().warn, "more than 1000 games");
    }

    #[test]
    fn check_files_finds_missing_book_and_suite() {
        let mut c = config(2);
        c.opening = OpeningSource::Book;
        c.book = Some(BookConfig {
            path: PathBuf::from("definitely-missing.bin"),
            pick: BookPick::Best,
        });
        assert!(matches!(c.check_files(), Err(ConfigError::BookNotFound(_))));
        c.opening = OpeningSource::Suite;
        c.suite = Some(SuiteConfig {
            path: PathBuf::from("definitely-missing.epd"),
            order: SuiteOrder::Sequential,
            plies: 8,
        });
        assert!(matches!(
            c.check_files(),
            Err(ConfigError::SuiteUnreadable(_))
        ));
    }

    #[test]
    fn zero_adjudication_rules_and_failure_limits_are_refused() {
        // Regression (D1309): a zero rule fired at once.
        let mut c = config(2);
        c.adjudication.max_plies = Some(0);
        assert_eq!(c.validate(), Err(ConfigError::BadAdjudication("max plies")));
        c.adjudication.max_plies = Some(200);
        assert_eq!(c.validate(), Ok(()));
        c.adjudication.resign = Some(ResignRule {
            centipawns: 0,
            ..ResignRule::default()
        });
        assert_eq!(c.validate(), Err(ConfigError::BadAdjudication("resign")));
        c.adjudication.resign = Some(ResignRule {
            moves: 0,
            ..ResignRule::default()
        });
        assert_eq!(c.validate(), Err(ConfigError::BadAdjudication("resign")));
        c.adjudication.resign = None;
        c.adjudication.draw = Some(DrawRule {
            moves: 0,
            ..DrawRule::default()
        });
        assert_eq!(c.validate(), Err(ConfigError::BadAdjudication("draw")));
        c.adjudication.draw = None;
        c.adjudication.tb_pieces = 0;
        assert_eq!(
            c.validate(),
            Err(ConfigError::BadAdjudication("tablebase pieces"))
        );
        c.adjudication.tb_pieces = 5;
        c.failures.max_failures = 0;
        assert_eq!(c.validate(), Err(ConfigError::BadFailureLimit));
    }

    #[test]
    fn the_name_and_the_site_are_limited() {
        let mut c = config(2);
        c.name = "x".repeat(MAX_TITLE_CHARS);
        assert_eq!(c.validate(), Ok(()));
        c.name.push('x');
        assert_eq!(c.validate(), Err(ConfigError::TitleTooLong));
        c.name = "ok".to_owned();
        c.site = "s".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(c.validate(), Err(ConfigError::TitleTooLong));
        assert_eq!(c.clamped().site.chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn clamped_keeps_every_number_inside_a_toml_integer() {
        // Regression (D1310): a `u64::MAX` wiped the preferences file.
        let mut c = config(2);
        c.participants[0].engine = u64::MAX;
        c.time = TimeControl::Nodes(u64::MAX);
        let clamped = c.clone().clamped();
        assert_eq!(clamped.participants[0].engine, MAX_TOML_INT);
        assert_eq!(clamped.time, TimeControl::Nodes(MAX_TOML_INT));
        assert!(toml::to_string(&clamped).is_ok());
        c.time = TimeControl::Clock {
            base_ms: u64::MAX,
            increment_ms: u64::MAX,
        };
        assert_eq!(
            c.clone().clamped().time,
            TimeControl::Clock {
                base_ms: MAX_BASE_MS,
                increment_ms: MAX_INCREMENT_MS
            }
        );
        c.time = TimeControl::MoveTime { ms: u64::MAX };
        assert_eq!(
            c.clone().clamped().time,
            TimeControl::MoveTime {
                ms: MAX_MOVETIME_MS
            }
        );
        c.adjudication.max_plies = Some(0);
        c.adjudication.resign = Some(ResignRule {
            centipawns: 0,
            moves: 0,
            require_both: true,
        });
        c.failures.max_failures = 0;
        let fixed = c.clamped();
        assert_eq!(fixed.validate(), Ok(()));
    }

    #[test]
    fn a_schedule_too_big_for_the_state_file_is_refused() {
        let mut c = config(32);
        c.cycles = 100;
        assert!(c.state_bytes() > super::super::store::MAX_STORE_BYTES);
        assert_eq!(c.validate(), Err(ConfigError::TooManyGames));
        c.cycles = 30;
        assert_eq!(c.validate(), Ok(()));
        let e = c.estimate();
        assert!(e.large_state, "{} games", e.games);
        assert!(e.warn);
        assert!(!config(4).estimate().large_state);
    }
}
