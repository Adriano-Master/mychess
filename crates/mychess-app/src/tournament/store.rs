//! Saving and resuming a tournament (spec M8-2, D1279, D1285).
//!
//! The state goes to `<data>/torneios/<slug>.toml` after every game, with an
//! atomic write; the games themselves are in `<slug>.pgn`. Reading never
//! panics: a corrupt file or an unknown version is a [`StoreError`].

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use mychess_core::PartialDate;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::config::{ConfigError, TournamentConfig};
use super::schedule::build_schedule;
use super::timecheck::TimeStats;
use crate::files::sanitize_file_name;
use crate::fsutil::write_atomic;

/// The version of the state file.
pub const STORE_VERSION: u32 = 1;
/// The largest state file read, in bytes.
pub const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;

/// Why a state file could not be saved or read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    /// A file system failure.
    #[error("i/o error on {path}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        #[source]
        source: io::Error,
    },
    /// The file is not a valid tournament.
    #[error("invalid tournament file: {0}")]
    Invalid(String),
    /// The file is from another version of the program.
    #[error("unsupported tournament file version {0}")]
    UnsupportedVersion(u32),
    /// The file is larger than [`MAX_STORE_BYTES`].
    #[error("tournament file too large")]
    TooLarge,
    /// The saved configuration is not valid.
    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),
}

impl StoreError {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            StoreError::Io { .. } => "tournament-store-io",
            StoreError::Invalid(_) | StoreError::Config(_) => "tournament-store-invalid",
            StoreError::UnsupportedVersion(_) => "tournament-store-version",
            StoreError::TooLarge => "tournament-store-too-large",
        }
    }
}

/// What a saved game was doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedStatus {
    /// Waiting (also a game that was running when the app stopped).
    Pending,
    /// Played; the result is in the PGN too.
    Played,
    /// Skipped by the user.
    Skipped,
    /// Played, then replaced by a redo.
    Replaced,
    /// Dropped because a participant left.
    Withdrawn,
}

/// A game of the schedule, saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedGame {
    /// The number in the schedule.
    pub number: u32,
    /// The cycle.
    pub cycle: u32,
    /// The round.
    pub round: u32,
    /// The participant with White.
    pub white: usize,
    /// The participant with Black.
    pub black: usize,
    /// The group of the pair (D1273).
    pub group: u32,
    /// The state.
    pub status: SavedStatus,
    /// The PGN result.
    #[serde(default)]
    pub result: Option<String>,
    /// The `Termination` tag.
    #[serde(default)]
    pub termination: Option<String>,
    /// The plies played.
    #[serde(default)]
    pub plies: u32,
    /// The name of the opening.
    #[serde(default)]
    pub opening: Option<String>,
}

/// A book line kept for the second game of a pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedLine {
    /// The group of the pair.
    pub group: u32,
    /// The moves in UCI, from the standard position.
    pub moves: Vec<String>,
}

/// Everything needed to resume a tournament (D1285).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTournament {
    /// The version of the format.
    pub version: u32,
    /// The base of the file names; checked by [`is_safe_slug`] on reading
    /// because it becomes part of paths (D1304).
    pub slug: String,
    /// The name of the PGN, for people who read the file. It is **not** used
    /// to find the PGN (that is `<slug>.pgn` next to the state file) and is
    /// not validated (D1304).
    pub pgn_file: String,
    /// The day the tournament was made, `YYYY.MM.DD`.
    pub created: String,
    /// The configuration, seed included.
    pub config: TournamentConfig,
    /// The schedule and what happened to each game.
    pub games: Vec<SavedGame>,
    /// The order of the games still to play, as indexes of `games`.
    pub queue: Vec<usize>,
    /// Failures of each participant.
    pub failures: Vec<u32>,
    /// Participants that left.
    pub withdrawn: Vec<bool>,
    /// Book lines of the pairs.
    #[serde(default)]
    pub book_lines: Vec<SavedLine>,
    /// What the games measured about the time of each participant (D2514).
    #[serde(default)]
    pub time_stats: Vec<TimeStats>,
    /// The tournament ended.
    #[serde(default)]
    pub finished: bool,
    /// Grows with every state the tournament hands out. Writes may reach the
    /// disk out of order; the one with the lower number is dropped (D1306).
    #[serde(default)]
    pub revision: u64,
}

impl SavedTournament {
    /// The games played that count.
    pub fn played(&self) -> usize {
        self.games
            .iter()
            .filter(|g| g.status == SavedStatus::Played)
            .count()
    }

    /// Checks the references between the parts (D1304, D1312).
    pub(crate) fn check(&self) -> Result<(), StoreError> {
        let invalid = |what: &str| Err(StoreError::Invalid(what.to_owned()));
        if self.version != STORE_VERSION {
            return Err(StoreError::UnsupportedVersion(self.version));
        }
        if !is_safe_slug(&self.slug) {
            return invalid("unsafe slug");
        }
        self.config.validate()?;
        let n = self.config.participants.len();
        if self.failures.len() != n || self.withdrawn.len() != n {
            return invalid("participant lists do not match");
        }
        if self
            .games
            .iter()
            .any(|g| g.white >= n || g.black >= n || g.white == g.black)
        {
            return invalid("game with an unknown participant");
        }
        if self.queue.iter().any(|&i| i >= self.games.len()) {
            return invalid("queue refers to an unknown game");
        }
        self.check_games()
    }

    /// The games against the schedule of the configuration and against each
    /// other (D1312). Colours are not compared: they are decided when the
    /// schedule is built, and a file from an earlier build keeps its own.
    fn check_games(&self) -> Result<(), StoreError> {
        let invalid = |what: &str| Err(StoreError::Invalid(what.to_owned()));
        let config = &self.config;
        let schedule = build_schedule(
            config.participants.len(),
            config.scheduling,
            config.cycles,
            config.tested,
        );
        if self.games.len() < schedule.pairings.len() {
            return invalid("the schedule is incomplete");
        }
        let pair = |a: usize, b: usize| (a.min(b), a.max(b));
        for (game, expected) in self.games.iter().zip(&schedule.pairings) {
            if (game.number, game.cycle, game.round, game.group)
                != (
                    expected.number,
                    expected.cycle,
                    expected.round,
                    expected.group,
                )
                || pair(game.white, game.black) != pair(expected.white, expected.black)
            {
                return invalid("a game does not match the schedule");
            }
        }
        // The games added after the schedule are redos: they repeat the
        // number of a game that was played and replaced.
        let mut by_number: std::collections::HashMap<u32, Vec<&SavedGame>> =
            std::collections::HashMap::new();
        for game in &self.games {
            by_number.entry(game.number).or_default().push(game);
        }
        for (number, same) in &by_number {
            let live = same
                .iter()
                .filter(|g| g.status != SavedStatus::Replaced)
                .count();
            if live != 1 {
                return invalid(&format!("game {number} is not unique"));
            }
            let first = same
                .first()
                .map(|g| (pair(g.white, g.black), g.group, g.cycle, g.round));
            if same
                .iter()
                .any(|g| Some((pair(g.white, g.black), g.group, g.cycle, g.round)) != first)
            {
                return invalid(&format!("game {number} is inconsistent"));
            }
        }
        let mut group_pair: std::collections::HashMap<u32, (usize, usize)> =
            std::collections::HashMap::new();
        for game in &self.games {
            let this = pair(game.white, game.black);
            if *group_pair.entry(game.group).or_insert(this) != this {
                return invalid("a group has two different pairs");
            }
        }
        let has_pending = self.games.iter().any(|g| g.status == SavedStatus::Pending);
        if self.finished && has_pending {
            return invalid("finished tournament with games to play");
        }
        if self.games.iter().any(|g| {
            matches!(g.status, SavedStatus::Played | SavedStatus::Replaced) && g.result.is_none()
        }) {
            return invalid("a played game has no result");
        }
        Ok(())
    }
}

/// Whether `slug` can be the base of a file name inside the folder of the
/// tournaments: not empty, as `sanitize_file_name` leaves it (so no
/// separator, no drive prefix, no reserved name, no leading or trailing dot
/// or space, at most 100 characters) and without `..` (D1304).
pub fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty() && !slug.contains("..") && sanitize_file_name(slug) == slug
}

/// The text of the state file.
///
/// # Errors
///
/// [`StoreError::Invalid`] if the state cannot be written as TOML.
pub fn render_state(state: &SavedTournament) -> Result<String, StoreError> {
    let text = toml::to_string(state).map_err(|e| StoreError::Invalid(e.to_string()))?;
    // The serializer writes an integer above `i64::MAX` as it is; no reader
    // takes it back. Better to refuse than to write a file that never loads.
    text.parse::<toml::Table>()
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
    Ok(text)
}

/// Reads the text of a state file.
///
/// # Errors
///
/// A [`StoreError`] for a malformed file, an unknown version or references
/// that do not fit.
pub fn parse_state(text: &str) -> Result<SavedTournament, StoreError> {
    if u64::try_from(text.len()).map_or(true, |n| n > MAX_STORE_BYTES) {
        return Err(StoreError::TooLarge);
    }
    // Look at the version first: a file of another version may not fit the
    // current structure at all.
    let probe: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| StoreError::Invalid(e.to_string()))?;
    let version = probe
        .get("version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok());
    match version {
        Some(STORE_VERSION) => {}
        Some(other) => return Err(StoreError::UnsupportedVersion(other)),
        None => return Err(StoreError::Invalid("no version".to_owned())),
    }
    let state: SavedTournament =
        toml::from_str(text).map_err(|e| StoreError::Invalid(e.to_string()))?;
    state.check()?;
    Ok(state)
}

/// The directory of the tournaments: `<data>/torneios`, `None` when the
/// system has no data directory.
pub fn tournaments_dir() -> Option<PathBuf> {
    crate::start::data_dir().map(|dir| dir.join("torneios"))
}

/// The state file of `slug` in `dir`.
pub fn state_path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{slug}.toml"))
}

/// The PGN of `slug` in `dir`.
pub fn pgn_path(dir: &Path, slug: &str) -> PathBuf {
    dir.join(format!("{slug}.pgn"))
}

/// The base of the file names: the name of the tournament made safe for the
/// three platforms plus the date (D1279). `taken` tells whether a slug is
/// already used (a number is appended until it is not).
pub fn make_slug(name: &str, date: PartialDate, taken: impl Fn(&str) -> bool) -> String {
    let base = sanitize_file_name(name).replace(' ', "-");
    let stamp = date.to_string().replace('.', "-");
    // The whole slug goes through the filter: an unknown date writes `?`.
    let mut first = sanitize_file_name(&format!("{base}-{stamp}"));
    // `is_safe_slug` refuses `..`: a name such as `v1..2` must still resume.
    while first.contains("..") {
        first = first.replace("..", "_");
    }
    if !taken(&first) {
        return first;
    }
    (2u32..=u32::MAX)
        .map(|n| format!("{first}-{n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or(first)
}

/// Writes the state atomically (blocking: run it from `perform`).
///
/// # Errors
///
/// [`StoreError`] when the state cannot be rendered or written.
pub fn save_state(path: &Path, state: &SavedTournament) -> Result<(), StoreError> {
    let text = render_state(state)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| StoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    write_atomic(path, |file| file.write_all(text.as_bytes())).map_err(|source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads a state file (blocking: run it from `perform`).
///
/// # Errors
///
/// [`StoreError`] for a file that cannot be read or is not a valid state.
pub fn load_state(path: &Path) -> Result<SavedTournament, StoreError> {
    let io_error = |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    };
    let meta = fs::metadata(path).map_err(io_error)?;
    if meta.len() > MAX_STORE_BYTES {
        return Err(StoreError::TooLarge);
    }
    let bytes = fs::read(path).map_err(io_error)?;
    let text =
        String::from_utf8(bytes).map_err(|_| StoreError::Invalid("not valid UTF-8".to_owned()))?;
    parse_state(&text)
}

/// A tournament that can be resumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumableTournament {
    /// The state file.
    pub path: PathBuf,
    /// The name of the tournament.
    pub name: String,
    /// Games played.
    pub played: usize,
    /// Games in the schedule that still count.
    pub total: usize,
}

/// The unfinished tournaments of `dir`, by file name (blocking). Files that
/// cannot be read are left out.
pub fn list_unfinished(dir: &Path) -> Vec<ResumableTournament> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let state = load_state(&path).ok()?;
            if state.finished {
                return None;
            }
            Some(ResumableTournament {
                name: state.config.name.clone(),
                played: state.played(),
                total: state
                    .games
                    .iter()
                    .filter(|g| g.status != SavedStatus::Replaced)
                    .count(),
                path,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tournament::config::Participant;

    fn state() -> SavedTournament {
        let config = TournamentConfig {
            participants: vec![Participant::new(1, "A"), Participant::new(2, "B")],
            ..TournamentConfig::default()
        };
        SavedTournament {
            version: STORE_VERSION,
            slug: "t-2026-10-24".to_owned(),
            pgn_file: "t-2026-10-24.pgn".to_owned(),
            created: "2026.10.24".to_owned(),
            config,
            games: vec![
                SavedGame {
                    number: 1,
                    cycle: 1,
                    round: 1,
                    white: 0,
                    black: 1,
                    group: 0,
                    status: SavedStatus::Played,
                    result: Some("1-0".to_owned()),
                    termination: Some("normal".to_owned()),
                    plies: 61,
                    opening: Some("Ruy".to_owned()),
                },
                SavedGame {
                    number: 2,
                    cycle: 2,
                    round: 1,
                    white: 1,
                    black: 0,
                    group: 0,
                    status: SavedStatus::Pending,
                    result: None,
                    termination: None,
                    plies: 0,
                    opening: None,
                },
            ],
            queue: vec![1],
            failures: vec![0, 1],
            withdrawn: vec![false, false],
            book_lines: vec![SavedLine {
                group: 0,
                moves: vec!["e2e4".to_owned()],
            }],
            time_stats: Vec::new(),
            finished: false,
            revision: 0,
        }
    }

    #[test]
    fn the_state_round_trips_through_toml() {
        let original = state();
        let text = render_state(&original).expect("render");
        assert_eq!(parse_state(&text).expect("parse"), original);
    }

    #[test]
    fn rejects_garbage_versions_and_bad_references() {
        assert!(matches!(
            parse_state("not = [toml"),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(parse_state(""), Err(StoreError::Invalid(_))));
        let mut s = state();
        s.version = 99;
        let text = toml::to_string(&s).expect("toml");
        assert!(matches!(
            parse_state(&text),
            Err(StoreError::UnsupportedVersion(99))
        ));
        let mut s = state();
        s.queue = vec![7];
        let text = toml::to_string(&s).expect("toml");
        assert!(matches!(parse_state(&text), Err(StoreError::Invalid(_))));
        let mut s = state();
        s.games[0].white = 5;
        let text = toml::to_string(&s).expect("toml");
        assert!(matches!(parse_state(&text), Err(StoreError::Invalid(_))));
        let mut s = state();
        s.config.participants.pop();
        let text = toml::to_string(&s).expect("toml");
        assert!(parse_state(&text).is_err());
        // A file of another version that does not fit the structure.
        assert!(matches!(
            parse_state("version = 2\nwhatever = true\n"),
            Err(StoreError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn saves_atomically_and_loads_back() {
        let dir = tempfile::tempdir().expect("dir");
        let path = state_path(&dir.path().join("torneios"), "t");
        let s = state();
        save_state(&path, &s).expect("save");
        assert_eq!(load_state(&path).expect("load"), s);
        let mut next = s.clone();
        next.failures = vec![2, 2];
        save_state(&path, &next).expect("save again");
        assert_eq!(load_state(&path).expect("load").failures, [2, 2]);
        let leftovers: Vec<_> = fs::read_dir(path.parent().expect("dir"))
            .expect("read")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(leftovers.len(), 1, "no temporary file remains");
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("bad.toml");
        fs::write(&path, [0xff, 0xfe, 0x00]).expect("write");
        assert!(matches!(load_state(&path), Err(StoreError::Invalid(_))));
        assert!(matches!(
            load_state(&dir.path().join("missing.toml")),
            Err(StoreError::Io { .. })
        ));
    }

    #[test]
    fn lists_only_the_unfinished_tournaments() {
        let dir = tempfile::tempdir().expect("dir");
        let open = state();
        save_state(&state_path(dir.path(), "a"), &open).expect("save");
        let mut done = state();
        done.finished = true;
        save_state(&state_path(dir.path(), "b"), &done).expect("save");
        fs::write(dir.path().join("c.toml"), "junk").expect("write");
        let listed = list_unfinished(dir.path());
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].played, listed[0].total), (1, 2));
        assert_eq!(listed[0].name, "MyChess Tournament");
        assert!(list_unfinished(&dir.path().join("none")).is_empty());
    }

    #[test]
    fn slugs_are_safe_and_unique() {
        let date = PartialDate::new(Some(2026), Some(10), Some(24)).expect("date");
        assert_eq!(make_slug("My Cup", date, |_| false), "My-Cup-2026-10-24");
        let taken = |s: &str| s == "My-Cup-2026-10-24" || s == "My-Cup-2026-10-24-2";
        assert_eq!(make_slug("My Cup", date, taken), "My-Cup-2026-10-24-3");
        let weird = make_slug("a/b:c*?", date, |_| false);
        assert!(!weird.contains(['/', ':', '*', '?']), "{weird}");
        let reserved = make_slug("CON", date, |_| false);
        assert!(reserved.starts_with("_CON"), "{reserved}");
        assert!(pgn_path(Path::new("d"), "x").ends_with("x.pgn"));
    }

    fn invalid(s: &SavedTournament) -> bool {
        matches!(
            parse_state(&toml::to_string(s).expect("toml")),
            Err(StoreError::Invalid(_))
        )
    }

    #[test]
    fn an_unsafe_slug_makes_the_state_invalid() {
        // Regression (D1304): the slug becomes part of a path.
        for slug in [
            "../x",
            "..",
            "a/b",
            "a\\b",
            "",
            "C:evil",
            "/abs",
            "a..b",
            ".hidden",
            "CON",
            "trailing.",
        ] {
            let mut s = state();
            s.slug = slug.to_owned();
            assert!(invalid(&s), "{slug:?}");
        }
        let mut ok = state();
        ok.slug = "My-Cup-2026-10-24-2".to_owned();
        assert!(parse_state(&toml::to_string(&ok).expect("toml")).is_ok());
        assert!(is_safe_slug(&make_slug(
            "a/b:c*?",
            PartialDate::default(),
            |_| false
        )));
    }

    #[test]
    fn inconsistent_games_make_the_state_invalid() {
        // Regression (D1312).
        let mut s = state();
        s.games[1].number = 1;
        assert!(invalid(&s), "duplicate number");
        let mut s = state();
        s.games[1].group = 7;
        assert!(invalid(&s), "group does not match the schedule");
        let mut s = state();
        s.games[1].round = 9;
        assert!(invalid(&s), "round does not match the schedule");
        let mut s = state();
        s.games.pop();
        s.queue.clear();
        assert!(invalid(&s), "schedule incomplete");
        let mut s = state();
        s.finished = true;
        assert!(invalid(&s), "finished with a game to play");
        let mut s = state();
        s.games[0].result = None;
        assert!(invalid(&s), "played without result");
    }

    #[test]
    fn a_redo_may_repeat_the_number_of_a_replaced_game() {
        let mut s = state();
        s.games[0].status = SavedStatus::Replaced;
        let mut again = s.games[0].clone();
        again.status = SavedStatus::Played;
        s.games.push(again);
        assert!(parse_state(&toml::to_string(&s).expect("toml")).is_ok());
        // Two live games with one number are not.
        s.games[0].status = SavedStatus::Played;
        assert!(invalid(&s));
    }

    #[test]
    fn a_value_toml_cannot_hold_is_refused_on_writing() {
        let mut s = state();
        s.config.participants[0].engine = u64::MAX;
        assert!(matches!(render_state(&s), Err(StoreError::Invalid(_))));
    }
}
