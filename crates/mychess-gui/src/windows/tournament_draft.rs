//! The form of the tournament between engines, as data (spec M8-2b, D1263-
//! D1289, D1360-D1379).
//!
//! The application owns the tournament; what is here is the configuration
//! being edited: a [`TournamentConfig`] (the structure the `[tournament]`
//! preferences keep) with the numeric fields held as text, so a field can be
//! empty or half typed while it is edited, as in the game dialog. Nothing in
//! this module touches the application or the disk: [`TournamentDraft::to_config`]
//! reads the texts, [`TournamentDraft::refresh`] validates and estimates.

use mychess_app::AppState;
use mychess_app::engines::{EngineEntry, EngineId};
use mychess_app::tournament::{
    BookConfig, BookPick, CommentOptions, ConfigError, DrawRule, Estimate, ExtraDestination,
    MAX_PARTICIPANTS, OnTimeLoss, OpeningSource, Participant, ResignRule, Scheduling, ScoreComment,
    SuiteConfig, SuiteOrder, TimeControl, TournamentConfig,
};

/// Most digits typed in a numeric field.
const MAX_DIGITS: usize = 9;
/// Most digits of the fields that hold a 64-bit number (nodes, seed).
const MAX_DIGITS_WIDE: usize = 19;
/// Most characters of a participant name (the name of the tournament has
/// the same limit in the application).
const NAME_CHARS: usize = 64;

/// A numeric field of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TField {
    /// Games per pair.
    Cycles,
    /// Base time, in seconds.
    Base,
    /// Increment, in seconds.
    Increment,
    /// Time per move, in seconds.
    MoveTime,
    /// `go depth`.
    Depth,
    /// `go nodes`.
    Nodes,
    /// Tolerance of the time per move, in per cent.
    Leeway,
    /// Margin of the watchdog, in seconds.
    Margin,
    /// Ceiling per move of depth and nodes, in seconds.
    DepthLimit,
    /// Seed of the draws (0: a new one).
    Seed,
    /// Most plies of book (empty: no limit).
    BookLimit,
    /// `Hash` of every engine, in MB (empty: the saved one).
    Hash,
    /// `Threads` of every engine (empty: the saved one).
    Threads,
    /// Resign: centipawns.
    ResignCp,
    /// Resign: moves in a row.
    ResignMoves,
    /// Draw: after this move.
    DrawAfter,
    /// Draw: centipawns.
    DrawCp,
    /// Draw: moves in a row.
    DrawMoves,
    /// Draw at this many plies.
    MaxPlies,
    /// Largest tablebase position (pieces) the tablebase adjudicates (D2540).
    TbPieces,
    /// Times a broken game is played again.
    Retries,
    /// Failures of an engine before the tournament pauses.
    MaxFailures,
    /// Plies taken from each game of a PGN suite.
    SuitePlies,
    /// Games at once (D2500).
    Concurrency,
}

impl TField {
    const COUNT: usize = 24;

    fn slot(self) -> usize {
        self as usize
    }

    /// Whether the field holds seconds, which may have decimals.
    pub fn is_decimal(self) -> bool {
        matches!(
            self,
            TField::Base | TField::Increment | TField::MoveTime | TField::Margin
        )
    }

    fn digits(self) -> usize {
        if matches!(self, TField::Nodes | TField::Seed) {
            MAX_DIGITS_WIDE
        } else {
            MAX_DIGITS
        }
    }
}

/// The four ways the games are limited (D1266).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeKind {
    /// A clock per side with an increment.
    Clock,
    /// A time per move.
    MoveTime,
    /// A fixed depth.
    Depth,
    /// A fixed number of nodes.
    Nodes,
}

impl TimeKind {
    /// The kind of a time control.
    pub fn of(time: TimeControl) -> TimeKind {
        match time {
            TimeControl::Clock { .. } => TimeKind::Clock,
            TimeControl::MoveTime { .. } => TimeKind::MoveTime,
            TimeControl::Depth(_) => TimeKind::Depth,
            TimeControl::Nodes(_) => TimeKind::Nodes,
        }
    }
}

/// An edit of the form.
#[derive(Debug, Clone, PartialEq)]
pub enum DraftEdit {
    /// The name of the tournament.
    Name(String),
    /// The site.
    Site(String),
    /// A numeric field.
    Number(TField, String),
    /// Adds a participant that uses this engine.
    AddEngine(EngineId),
    /// Adds a copy of the participant (same engine, own options).
    Duplicate(usize),
    /// Removes the participant.
    Remove(usize),
    /// The display name of a participant.
    ParticipantName(usize, String),
    /// The UCI options of a participant, as `Name=Value; Name=Value`.
    ParticipantOptions(usize, String),
    /// The format.
    Scheduling(Scheduling),
    /// The tested participant of the gauntlet.
    Tested(usize),
    /// The way the games are limited.
    Time(TimeKind),
    /// Where the games start from.
    Opening(OpeningSource),
    /// How a book move is picked.
    BookPick(BookPick),
    /// The book file (`None`: no book).
    Book(Option<std::path::PathBuf>),
    /// The suite file (`None`: no suite).
    Suite(Option<std::path::PathBuf>),
    /// The order of the suite.
    SuiteOrder(SuiteOrder),
    /// Both games of a pair start from the same opening.
    PairOpenings(bool),
    /// Think on the time of the opponent.
    Ponder(bool),
    /// Start the engines again before every game.
    RestartEach(bool),
    /// Draw by repetition and 50 moves.
    ClaimDraws(bool),
    /// The rule that decides a won game.
    Resign(bool),
    /// The engine of the winner must agree.
    ResignBoth(bool),
    /// The rule that decides a drawn game.
    Draw(bool),
    /// A draw at a number of plies.
    MaxPlies(bool),
    /// The tablebase adjudication rule (D2540).
    Tablebase(bool),
    /// What to do with a game lost on time.
    OnTimeLoss(OnTimeLoss),
    /// How the note of the engine is written.
    Score(ScoreComment),
    /// `[%clk]` on each move.
    CommentClock(bool),
    /// `[%emt]` on each move.
    CommentTime(bool),
    /// An extra destination of the games.
    Destination(ExtraDestination),
}

/// The form being edited.
#[derive(Debug, Clone)]
pub struct TournamentDraft {
    config: TournamentConfig,
    kind: TimeKind,
    texts: [String; TField::COUNT],
    /// `Name=Value; ...` of each participant, parallel to the participants.
    options: Vec<String>,
    /// The position on the board when the form opened (D1272).
    board_fen: String,
    error: Option<ConfigError>,
    estimate: Estimate,
}

/// The engines of the list that can play: UCI and XBoard (D2521).
pub fn playable_engines(app: &AppState) -> impl Iterator<Item = &EngineEntry> {
    app.engines.engines().iter()
}

/// Seconds as text: `60`, `1.5`.
pub fn format_secs(ms: u64) -> String {
    let (whole, frac) = (ms / 1000, ms % 1000);
    if frac == 0 {
        whole.to_string()
    } else {
        let digits = format!("{frac:03}");
        format!("{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// Milliseconds from a text of seconds (`1.5`, `1,5`, `60`); `None` when
/// the text is not a number.
pub fn parse_secs(text: &str) -> Option<u64> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return None;
    }
    let (whole, frac) = text.split_once('.').unwrap_or((text.as_str(), ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut millis = 0u64;
    for (i, c) in frac.chars().take(3).enumerate() {
        let digit = u64::from(c.to_digit(10)?);
        millis += digit * [100, 10, 1][i];
    }
    whole.checked_mul(1000)?.checked_add(millis)
}

/// `Name=Value; Name=Value` as pairs; entries without a name are dropped.
pub fn parse_options(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn options_text(options: &[(String, String)]) -> String {
    options
        .iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

impl TournamentDraft {
    /// The form with what the preferences remember. Participants whose
    /// engine is gone (or is not UCI) are dropped; without any, the first
    /// two UCI engines are put in, so a first tournament is one click away.
    pub fn new(app: &AppState, board_fen: String) -> TournamentDraft {
        let mut config = (*app.prefs.tournament).clone();
        config
            .participants
            .retain(|p| playable_engines(app).any(|e| e.id.0 == p.engine));
        if config.participants.is_empty() {
            for entry in playable_engines(app).take(2) {
                let name = unique_name(&config.participants, &entry.name);
                config
                    .participants
                    .push(Participant::new(entry.id.0, &name));
            }
        }
        let options = config
            .participants
            .iter()
            .map(|p| options_text(&p.options))
            .collect();
        if config.tested >= config.participants.len() {
            config.tested = 0;
        }
        if !app.tournament_tablebase_available() {
            config.adjudication.tablebase = false;
        }
        let kind = TimeKind::of(config.time);
        let mut draft = TournamentDraft {
            texts: texts_of(&config),
            config,
            kind,
            options,
            board_fen,
            error: None,
            estimate: Estimate {
                games: 0,
                max_duration: std::time::Duration::ZERO,
                warn: false,
                large_state: false,
                state_bytes: 0,
                lanes: 1,
            },
        };
        draft.refresh();
        draft
    }

    /// The text of a numeric field.
    pub fn text(&self, field: TField) -> &str {
        &self.texts[field.slot()]
    }

    /// The way the games are limited.
    pub fn time_kind(&self) -> TimeKind {
        self.kind
    }

    /// The configuration as last read (the non-numeric parts are exact).
    pub fn config(&self) -> &TournamentConfig {
        &self.config
    }

    /// The participants.
    pub fn participants(&self) -> &[Participant] {
        &self.config.participants
    }

    /// The `Name=Value` text of a participant.
    pub fn options_of(&self, index: usize) -> &str {
        self.options.get(index).map_or("", String::as_str)
    }

    /// Why the form cannot start a tournament (`None`: it can).
    pub fn error(&self) -> Option<&ConfigError> {
        self.error.as_ref()
    }

    /// The games and the longest time of the tournament.
    pub fn estimate(&self) -> Estimate {
        self.estimate
    }

    /// The position on the board when the form opened.
    pub fn board_fen(&self) -> &str {
        &self.board_fen
    }

    fn read_u64(&self, field: TField, old: u64) -> u64 {
        self.text(field).trim().parse().unwrap_or(old)
    }

    fn read_u32(&self, field: TField, old: u32) -> u32 {
        u32::try_from(self.read_u64(field, u64::from(old))).unwrap_or(u32::MAX)
    }

    fn read_secs(&self, field: TField, old_ms: u64) -> u64 {
        parse_secs(self.text(field)).unwrap_or(old_ms)
    }

    fn read_optional(&self, field: TField) -> Option<u32> {
        let text = self.text(field).trim();
        if text.is_empty() {
            None
        } else {
            Some(self.read_u32(field, 0))
        }
    }

    /// The configuration the form stands for: the numbers are read from the
    /// text (an empty field keeps the old value, or means "none" for the
    /// optional ones). Nothing is limited here: the application validates.
    pub fn to_config(&self) -> TournamentConfig {
        let mut c = self.config.clone();
        c.cycles = self.read_u32(TField::Cycles, c.cycles);
        let (old_base, old_inc) = match c.time {
            TimeControl::Clock {
                base_ms,
                increment_ms,
            } => (base_ms, increment_ms),
            _ => (60_000, 1_000),
        };
        c.time = match self.kind {
            TimeKind::Clock => TimeControl::Clock {
                base_ms: self.read_secs(TField::Base, old_base),
                increment_ms: self.read_secs(TField::Increment, old_inc),
            },
            TimeKind::MoveTime => TimeControl::MoveTime {
                ms: self.read_secs(TField::MoveTime, 3_000),
            },
            TimeKind::Depth => TimeControl::Depth(self.read_u32(TField::Depth, 12)),
            TimeKind::Nodes => TimeControl::Nodes(self.read_u64(TField::Nodes, 100_000)),
        };
        c.leeway_percent = self.read_u32(TField::Leeway, c.leeway_percent);
        let margin = self.read_secs(TField::Margin, u64::from(c.margin_ms));
        c.margin_ms = u32::try_from(margin).unwrap_or(u32::MAX);
        c.depth_limit_secs = self.read_u32(TField::DepthLimit, c.depth_limit_secs);
        c.seed = self.read_u64(TField::Seed, 0);
        c.book_ply_limit = self.read_optional(TField::BookLimit);
        c.hash_mb = self.read_optional(TField::Hash);
        c.threads = self.read_optional(TField::Threads);
        c.concurrency = self.read_u32(TField::Concurrency, c.concurrency);
        c.adjudication.resign = c.adjudication.resign.map(|r| ResignRule {
            centipawns: self.read_u32(TField::ResignCp, r.centipawns),
            moves: self.read_u32(TField::ResignMoves, r.moves),
            require_both: r.require_both,
        });
        c.adjudication.draw = c.adjudication.draw.map(|d| DrawRule {
            after_move: self.read_u32(TField::DrawAfter, d.after_move),
            centipawns: self.read_u32(TField::DrawCp, d.centipawns),
            moves: self.read_u32(TField::DrawMoves, d.moves),
        });
        c.adjudication.max_plies = c
            .adjudication
            .max_plies
            .map(|old| self.read_u32(TField::MaxPlies, old));
        c.adjudication.tb_pieces = self.read_u32(TField::TbPieces, c.adjudication.tb_pieces);
        let retries = self.read_u32(TField::Retries, u32::from(c.failures.max_retries));
        c.failures.max_retries = u8::try_from(retries.min(u32::from(u8::MAX))).unwrap_or(u8::MAX);
        c.failures.max_failures = self.read_u32(TField::MaxFailures, c.failures.max_failures);
        c.suite = c.suite.map(|s| SuiteConfig {
            plies: self.read_u32(TField::SuitePlies, s.plies),
            ..s
        });
        // Only the openings that start from the board need its position.
        c.start_fen = matches!(
            c.opening,
            OpeningSource::CurrentPosition | OpeningSource::CurrentPositionFirstGame
        )
        .then(|| self.board_fen.clone());
        // The book and the suite matter only to the source that uses them.
        if c.opening != OpeningSource::Book {
            c.book = None;
        }
        if c.opening != OpeningSource::Suite {
            c.suite = None;
        }
        for (p, text) in c.participants.iter_mut().zip(&self.options) {
            p.options = parse_options(text);
        }
        c
    }

    /// Validates and estimates the form as it is now.
    pub fn refresh(&mut self) {
        let config = self.to_config();
        self.error = config.validate().err();
        self.estimate = config.estimate();
    }

    /// Applies an edit and refreshes the check.
    pub fn edit(&mut self, edit: DraftEdit, app: &AppState) {
        match edit {
            DraftEdit::Name(name) => self.config.name = name.chars().take(NAME_CHARS + 8).collect(),
            DraftEdit::Site(site) => self.config.site = site.chars().take(NAME_CHARS + 8).collect(),
            DraftEdit::Number(field, text) => self.set_text(field, &text),
            DraftEdit::AddEngine(id) => self.add_engine(id, app),
            DraftEdit::Duplicate(index) => self.duplicate(index),
            DraftEdit::Remove(index) => self.remove(index),
            DraftEdit::ParticipantName(index, name) => {
                if let Some(p) = self.config.participants.get_mut(index) {
                    p.name = name.chars().take(NAME_CHARS + 8).collect();
                }
            }
            DraftEdit::ParticipantOptions(index, text) => {
                if let Some(slot) = self.options.get_mut(index) {
                    *slot = text;
                }
            }
            DraftEdit::Scheduling(s) => self.config.scheduling = s,
            DraftEdit::Tested(index) => self.config.tested = index,
            DraftEdit::Time(kind) => self.kind = kind,
            DraftEdit::Opening(source) => self.config.opening = source,
            DraftEdit::BookPick(pick) => {
                if let Some(book) = self.config.book.as_mut() {
                    book.pick = pick;
                }
            }
            DraftEdit::Book(path) => {
                let pick = self
                    .config
                    .book
                    .as_ref()
                    .map_or(BookPick::default(), |b| b.pick);
                self.config.book = path.map(|path| BookConfig { path, pick });
            }
            DraftEdit::Suite(path) => {
                let old = self.config.suite.take();
                self.config.suite = path.map(|path| SuiteConfig {
                    path,
                    order: old.as_ref().map_or(SuiteOrder::default(), |s| s.order),
                    plies: old.as_ref().map_or(8, |s| s.plies),
                });
            }
            DraftEdit::SuiteOrder(order) => {
                if let Some(suite) = self.config.suite.as_mut() {
                    suite.order = order;
                }
            }
            DraftEdit::PairOpenings(on) => self.config.pair_openings = on,
            DraftEdit::Ponder(on) => self.config.ponder = on,
            DraftEdit::RestartEach(on) => self.config.restart_each_game = on,
            DraftEdit::ClaimDraws(on) => self.config.claim_draws = on,
            DraftEdit::Resign(on) => {
                self.config.adjudication.resign = on.then(ResignRule::default);
                if on {
                    let rule = ResignRule::default();
                    self.texts[TField::ResignCp.slot()] = rule.centipawns.to_string();
                    self.texts[TField::ResignMoves.slot()] = rule.moves.to_string();
                }
            }
            DraftEdit::ResignBoth(on) => {
                if let Some(rule) = self.config.adjudication.resign.as_mut() {
                    rule.require_both = on;
                }
            }
            DraftEdit::Draw(on) => {
                self.config.adjudication.draw = on.then(DrawRule::default);
                if on {
                    let rule = DrawRule::default();
                    self.texts[TField::DrawAfter.slot()] = rule.after_move.to_string();
                    self.texts[TField::DrawCp.slot()] = rule.centipawns.to_string();
                    self.texts[TField::DrawMoves.slot()] = rule.moves.to_string();
                }
            }
            DraftEdit::MaxPlies(on) => {
                let value = self.read_u32(TField::MaxPlies, 200);
                self.config.adjudication.max_plies = on.then_some(value.max(1));
                if on && self.text(TField::MaxPlies).is_empty() {
                    "200".clone_into(&mut self.texts[TField::MaxPlies.slot()]);
                }
            }
            DraftEdit::Tablebase(on) => {
                self.config.adjudication.tablebase = on;
                if on && self.text(TField::TbPieces).is_empty() {
                    "5".clone_into(&mut self.texts[TField::TbPieces.slot()]);
                }
            }
            DraftEdit::OnTimeLoss(policy) => self.config.failures.on_time_loss = policy,
            DraftEdit::Score(score) => self.config.comments.score = score,
            DraftEdit::CommentClock(on) => self.config.comments.clock = on,
            DraftEdit::CommentTime(on) => self.config.comments.move_time = on,
            DraftEdit::Destination(dest) => self.config.destination = dest,
        }
        self.refresh();
    }

    fn set_text(&mut self, field: TField, text: &str) {
        let mut seen_dot = false;
        let mut cleaned = String::new();
        for c in text.chars() {
            if c.is_ascii_digit() {
                cleaned.push(c);
            } else if field.is_decimal() && (c == '.' || c == ',') && !seen_dot {
                seen_dot = true;
                cleaned.push(c);
            }
        }
        // Digits, plus room for the decimals after the dot.
        let limit = field.digits() + if seen_dot { 4 } else { 0 };
        let cleaned: String = cleaned.chars().take(limit).collect();
        self.texts[field.slot()] = cleaned;
    }

    fn add_engine(&mut self, id: EngineId, app: &AppState) {
        if self.config.participants.len() >= MAX_PARTICIPANTS {
            return;
        }
        let Some(entry) = playable_engines(app).find(|e| e.id == id) else {
            return;
        };
        let name = unique_name(&self.config.participants, &entry.name);
        self.config.participants.push(Participant::new(id.0, &name));
        self.options.push(String::new());
    }

    fn duplicate(&mut self, index: usize) {
        if self.config.participants.len() >= MAX_PARTICIPANTS {
            return;
        }
        let Some(source) = self.config.participants.get(index).cloned() else {
            return;
        };
        let mut copy = source;
        copy.name = unique_name(&self.config.participants, &copy.name);
        let options = self.options.get(index).cloned().unwrap_or_default();
        self.config.participants.insert(index + 1, copy);
        self.options.insert(index + 1, options);
        if self.config.tested > index {
            self.config.tested += 1;
        }
    }

    fn remove(&mut self, index: usize) {
        if index >= self.config.participants.len() {
            return;
        }
        self.config.participants.remove(index);
        if index < self.options.len() {
            self.options.remove(index);
        }
        let count = self.config.participants.len();
        if self.config.tested > index || self.config.tested >= count {
            self.config.tested = self.config.tested.saturating_sub(1);
        }
    }

    /// Whether the engines of the participants were all found (they are
    /// checked when the form opens and when the list changes).
    pub fn comments(&self) -> CommentOptions {
        self.config.comments
    }
}

/// `Name`, or `Name 2`, `Name 3`... when taken (without regard to case).
fn unique_name(participants: &[Participant], base: &str) -> String {
    let taken = |name: &str| {
        participants
            .iter()
            .any(|p| p.name.trim().eq_ignore_ascii_case(name.trim()))
    };
    let base = base.trim();
    if !taken(base) {
        return base.to_owned();
    }
    (2..=u32::MAX)
        .map(|n| format!("{base} {n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| base.to_owned())
}

fn texts_of(config: &TournamentConfig) -> [String; TField::COUNT] {
    let mut texts: [String; TField::COUNT] = Default::default();
    let mut set = |field: TField, value: String| texts[field.slot()] = value;
    set(TField::Cycles, config.cycles.to_string());
    set(TField::Concurrency, config.concurrency.to_string());
    let (base, inc, movetime, depth, nodes) = match config.time {
        TimeControl::Clock {
            base_ms,
            increment_ms,
        } => (base_ms, increment_ms, 3_000, 12, 100_000),
        TimeControl::MoveTime { ms } => (60_000, 1_000, ms, 12, 100_000),
        TimeControl::Depth(d) => (60_000, 1_000, 3_000, u64::from(d), 100_000),
        TimeControl::Nodes(n) => (60_000, 1_000, 3_000, 12, n),
    };
    set(TField::Base, format_secs(base));
    set(TField::Increment, format_secs(inc));
    set(TField::MoveTime, format_secs(movetime));
    set(TField::Depth, depth.to_string());
    set(TField::Nodes, nodes.to_string());
    set(TField::Leeway, config.leeway_percent.to_string());
    set(TField::Margin, format_secs(u64::from(config.margin_ms)));
    set(TField::DepthLimit, config.depth_limit_secs.to_string());
    set(
        TField::Seed,
        if config.seed == 0 {
            String::new()
        } else {
            config.seed.to_string()
        },
    );
    set(
        TField::BookLimit,
        config
            .book_ply_limit
            .map_or(String::new(), |v| v.to_string()),
    );
    set(
        TField::Hash,
        config.hash_mb.map_or(String::new(), |v| v.to_string()),
    );
    set(
        TField::Threads,
        config.threads.map_or(String::new(), |v| v.to_string()),
    );
    let resign = config.adjudication.resign.unwrap_or_default();
    set(TField::ResignCp, resign.centipawns.to_string());
    set(TField::ResignMoves, resign.moves.to_string());
    let draw = config.adjudication.draw.unwrap_or_default();
    set(TField::DrawAfter, draw.after_move.to_string());
    set(TField::DrawCp, draw.centipawns.to_string());
    set(TField::DrawMoves, draw.moves.to_string());
    set(
        TField::MaxPlies,
        config
            .adjudication
            .max_plies
            .map_or("200".to_owned(), |v| v.to_string()),
    );
    set(TField::TbPieces, config.adjudication.tb_pieces.to_string());
    set(TField::Retries, config.failures.max_retries.to_string());
    set(
        TField::MaxFailures,
        config.failures.max_failures.to_string(),
    );
    set(
        TField::SuitePlies,
        config.suite.as_ref().map_or(8, |s| s.plies).to_string(),
    );
    texts
}

#[cfg(test)]
mod tests {
    use mychess_app::engines::EngineDraft;

    use super::*;

    fn app_with(names: &[&str]) -> AppState {
        let mut app = AppState::default();
        for name in names {
            app.engines
                .add(EngineDraft::new(*name, "fake-engine"))
                .expect("engine added");
        }
        app
    }

    #[test]
    fn seconds_round_trip_with_decimals_and_commas() {
        assert_eq!(parse_secs("60"), Some(60_000));
        assert_eq!(parse_secs("1.5"), Some(1_500));
        assert_eq!(parse_secs("0,25"), Some(250));
        assert_eq!(parse_secs(".5"), Some(500));
        assert_eq!(parse_secs(""), None);
        assert_eq!(parse_secs("."), None);
        assert_eq!(parse_secs("1.2.3"), None);
        assert_eq!(format_secs(60_000), "60");
        assert_eq!(format_secs(1_500), "1.5");
        assert_eq!(format_secs(250), "0.25");
        assert_eq!(parse_secs(&format_secs(1_050)), Some(1_050));
    }

    #[test]
    fn options_are_read_as_pairs_and_junk_is_dropped() {
        assert_eq!(
            parse_options("Hash=64; Threads = 2;;novalue; =x"),
            [
                ("Hash".to_owned(), "64".to_owned()),
                ("Threads".to_owned(), "2".to_owned())
            ]
        );
        assert_eq!(
            options_text(&parse_options("Hash=64; Skill Level=5")),
            "Hash=64; Skill Level=5"
        );
    }

    #[test]
    fn a_new_form_puts_in_the_first_two_playable_engines() {
        let app = app_with(&["Alpha", "Beta", "Gamma"]);
        let draft = TournamentDraft::new(&app, "fen".to_owned());
        let names: Vec<&str> = draft
            .participants()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["Alpha", "Beta"]);
        assert!(draft.error().is_none());
        assert_eq!(draft.estimate().games, 2);
    }

    #[test]
    fn the_same_engine_can_be_added_again_with_a_unique_name() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        let alpha = draft.participants()[0].engine;
        draft.edit(DraftEdit::AddEngine(EngineId(alpha)), &app);
        draft.edit(DraftEdit::AddEngine(EngineId(alpha)), &app);
        let names: Vec<&str> = draft
            .participants()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["Alpha", "Beta", "Alpha 2", "Alpha 3"]);
        assert!(draft.error().is_none());
        // 4 participants, 2 cycles: 6 pairs x 2.
        assert_eq!(draft.estimate().games, 12);
    }

    #[test]
    fn duplicate_names_are_reported_and_removing_fixes_them() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        draft.edit(DraftEdit::ParticipantName(1, "alpha".to_owned()), &app);
        assert!(matches!(draft.error(), Some(ConfigError::DuplicateName(_))));
        draft.edit(DraftEdit::Remove(1), &app);
        assert!(matches!(
            draft.error(),
            Some(ConfigError::TooFewParticipants)
        ));
    }

    #[test]
    fn duplicating_keeps_the_options_and_the_tested_index() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        draft.edit(DraftEdit::Tested(1), &app);
        draft.edit(DraftEdit::ParticipantOptions(0, "Hash=32".to_owned()), &app);
        draft.edit(DraftEdit::Duplicate(0), &app);
        assert_eq!(draft.participants().len(), 3);
        assert_eq!(draft.options_of(1), "Hash=32");
        assert_eq!(draft.to_config().participants[1].options.len(), 1);
        // The tested one (Beta) moved to index 2.
        assert_eq!(draft.config().tested, 2);
    }

    #[test]
    fn numeric_fields_keep_digits_and_one_decimal_point() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        draft.edit(DraftEdit::Number(TField::Cycles, "1a2b".to_owned()), &app);
        assert_eq!(draft.text(TField::Cycles), "12");
        draft.edit(
            DraftEdit::Number(TField::Increment, "1.5.2".to_owned()),
            &app,
        );
        assert_eq!(draft.text(TField::Increment), "1.52");
        draft.edit(DraftEdit::Number(TField::Depth, "1.5".to_owned()), &app);
        assert_eq!(draft.text(TField::Depth), "15");
    }

    #[test]
    fn the_time_kinds_build_the_four_time_controls() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        draft.edit(DraftEdit::Number(TField::Base, "180".to_owned()), &app);
        draft.edit(DraftEdit::Number(TField::Increment, "2.5".to_owned()), &app);
        assert_eq!(
            draft.to_config().time,
            TimeControl::Clock {
                base_ms: 180_000,
                increment_ms: 2_500
            }
        );
        draft.edit(DraftEdit::Time(TimeKind::MoveTime), &app);
        draft.edit(DraftEdit::Number(TField::MoveTime, "0.5".to_owned()), &app);
        assert_eq!(draft.to_config().time, TimeControl::MoveTime { ms: 500 });
        draft.edit(DraftEdit::Time(TimeKind::Depth), &app);
        draft.edit(DraftEdit::Number(TField::Depth, "9".to_owned()), &app);
        assert_eq!(draft.to_config().time, TimeControl::Depth(9));
        draft.edit(DraftEdit::Time(TimeKind::Nodes), &app);
        draft.edit(DraftEdit::Number(TField::Nodes, "5000".to_owned()), &app);
        assert_eq!(draft.to_config().time, TimeControl::Nodes(5000));
    }

    #[test]
    fn an_empty_numeric_field_keeps_the_old_value_and_optional_ones_mean_none() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        draft.edit(DraftEdit::Number(TField::Cycles, String::new()), &app);
        assert_eq!(draft.to_config().cycles, 2);
        draft.edit(DraftEdit::Number(TField::Hash, "128".to_owned()), &app);
        assert_eq!(draft.to_config().hash_mb, Some(128));
        draft.edit(DraftEdit::Number(TField::Hash, String::new()), &app);
        assert_eq!(draft.to_config().hash_mb, None);
    }

    #[test]
    fn tablebase_rule_needs_a_tablebase_and_reads_its_field() {
        use mychess_app::tablebase::TablebasePrefs;
        let mut app = app_with(&["Alpha", "Beta"]);
        app.prefs.tournament.adjudication.tablebase = true;
        assert!(!app.tournament_tablebase_available());
        let draft = TournamentDraft::new(&app, String::new());
        assert!(!draft.to_config().adjudication.tablebase);

        *app.prefs.tablebase = TablebasePrefs {
            local: true,
            directories: vec!["x".into()],
            online: false,
        };
        assert!(app.tournament_tablebase_available());
        let mut draft = TournamentDraft::new(&app, String::new());
        assert!(draft.to_config().adjudication.tablebase);
        draft.edit(DraftEdit::Number(TField::TbPieces, "6".to_owned()), &app);
        assert_eq!(draft.to_config().adjudication.tb_pieces, 6);
        draft.edit(DraftEdit::Number(TField::TbPieces, String::new()), &app);
        draft.edit(DraftEdit::Tablebase(false), &app);
        assert!(!draft.to_config().adjudication.tablebase);
        draft.edit(DraftEdit::Tablebase(true), &app);
        assert!(draft.to_config().adjudication.tablebase);
        assert_eq!(draft.text(TField::TbPieces), "5");
    }

    #[test]
    fn adjudication_rules_start_from_the_defaults_and_read_the_fields() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        assert!(draft.to_config().adjudication.resign.is_none());
        draft.edit(DraftEdit::Resign(true), &app);
        draft.edit(DraftEdit::Number(TField::ResignCp, "800".to_owned()), &app);
        draft.edit(DraftEdit::ResignBoth(false), &app);
        let rule = draft.to_config().adjudication.resign.expect("a rule");
        assert_eq!(
            (rule.centipawns, rule.moves, rule.require_both),
            (800, 4, false)
        );
        draft.edit(DraftEdit::Draw(true), &app);
        let rule = draft.to_config().adjudication.draw.expect("a rule");
        assert_eq!((rule.after_move, rule.centipawns, rule.moves), (40, 10, 8));
        draft.edit(DraftEdit::MaxPlies(true), &app);
        assert_eq!(draft.to_config().adjudication.max_plies, Some(200));
        draft.edit(DraftEdit::Resign(false), &app);
        assert!(draft.to_config().adjudication.resign.is_none());
    }

    #[test]
    fn openings_that_do_not_use_a_book_or_a_suite_drop_them() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, "the-fen".to_owned());
        draft.edit(DraftEdit::Book(Some("a.bin".into())), &app);
        draft.edit(DraftEdit::Suite(Some("s.epd".into())), &app);
        assert!(draft.to_config().book.is_none());
        assert!(draft.to_config().suite.is_none());
        draft.edit(DraftEdit::Opening(OpeningSource::Book), &app);
        assert!(draft.to_config().book.is_some());
        draft.edit(DraftEdit::Opening(OpeningSource::Suite), &app);
        assert!(draft.to_config().suite.is_some());
        assert!(draft.to_config().start_fen.is_none());
        draft.edit(DraftEdit::Opening(OpeningSource::CurrentPosition), &app);
        assert_eq!(draft.to_config().start_fen.as_deref(), Some("the-fen"));
    }

    #[test]
    fn a_book_opening_without_a_book_is_an_error() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        draft.edit(DraftEdit::Opening(OpeningSource::Book), &app);
        assert!(matches!(
            draft.error(),
            Some(ConfigError::MissingOpeningData(_))
        ));
        draft.edit(DraftEdit::Book(Some("book.bin".into())), &app);
        assert!(draft.error().is_none());
    }

    #[test]
    fn the_estimate_follows_the_format_and_the_cycles() {
        let app = app_with(&["Alpha", "Beta", "Gamma", "Delta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        for id in [2, 3] {
            let engine = app.engines.engines()[id].id;
            draft.edit(DraftEdit::AddEngine(engine), &app);
        }
        assert_eq!(draft.estimate().games, 12);
        draft.edit(DraftEdit::Scheduling(Scheduling::Gauntlet), &app);
        assert_eq!(draft.estimate().games, 6);
        draft.edit(DraftEdit::Number(TField::Cycles, "100".to_owned()), &app);
        assert_eq!(draft.estimate().games, 300);
        assert!(!draft.estimate().warn);
        draft.edit(DraftEdit::Scheduling(Scheduling::RoundRobin), &app);
        draft.edit(DraftEdit::Number(TField::Cycles, "200".to_owned()), &app);
        assert_eq!(draft.estimate().games, 1200);
        assert!(draft.estimate().warn);
    }

    #[test]
    fn more_than_thirty_two_participants_are_not_added() {
        let app = app_with(&["Alpha", "Beta"]);
        let mut draft = TournamentDraft::new(&app, String::new());
        let alpha = draft.participants()[0].engine;
        for _ in 0..40 {
            draft.edit(DraftEdit::AddEngine(EngineId(alpha)), &app);
        }
        assert_eq!(draft.participants().len(), MAX_PARTICIPANTS);
    }

    #[test]
    fn the_last_configuration_comes_back_without_the_engines_that_are_gone() {
        let mut app = app_with(&["Alpha", "Beta"]);
        let config = TournamentConfig {
            name: "Cup".to_owned(),
            participants: vec![
                Participant::new(app.engines.engines()[0].id.0, "A"),
                Participant::new(9_999, "Ghost"),
                Participant::new(app.engines.engines()[1].id.0, "B"),
            ],
            ..TournamentConfig::default()
        };
        *app.prefs.tournament = config;
        let draft = TournamentDraft::new(&app, String::new());
        let names: Vec<&str> = draft
            .participants()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["A", "B"]);
        assert_eq!(draft.config().name, "Cup");
    }
}
