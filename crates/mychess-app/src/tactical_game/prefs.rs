//! The `[play.tactical]` table of the preferences (spec M8-7, D2464): what
//! the tactical dialog remembers, and the score.

use std::path::PathBuf;

use serde::Serialize;
use toml::Value;

use super::TacticalSetup;
use super::start::{PickSource, StartProblem, parse_start_fen};
use super::stats::TacticalStats;
use super::strength::{ADJUST_STEPS, DEFAULT_ADJUST_STEP, DEFAULT_TACTICAL_ELO, clamp_elo};
use crate::engines::EngineId;
use crate::play::{PlayConfig, StartFrom};
use crate::prefs::{PlayPrefs, PrefsIssue};
use mychess_core::Position;

/// Longest FEN text kept in the preferences.
pub const MAX_FEN_PREF_CHARS: usize = 200;

/// Where the tactical game starts (D2457).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TacticalStartPref {
    /// The standard position.
    #[default]
    New,
    /// The position on the board.
    Current,
    /// A line of the list of openings.
    Opening,
    /// The problem on the board of the tactics training.
    Problem,
    /// A FEN typed in the dialog.
    Fen,
    /// A position drawn from a file.
    RandomFile,
    /// A position drawn from the current database.
    RandomBase,
}

impl TacticalStartPref {
    /// The spelling used in the TOML file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TacticalStartPref::New => "new",
            TacticalStartPref::Current => "current",
            TacticalStartPref::Opening => "opening",
            TacticalStartPref::Problem => "problem",
            TacticalStartPref::Fen => "fen",
            TacticalStartPref::RandomFile => "random_file",
            TacticalStartPref::RandomBase => "random_base",
        }
    }

    /// Reads the TOML spelling.
    #[must_use]
    pub fn from_name(text: &str) -> Option<TacticalStartPref> {
        Some(match text {
            "new" => TacticalStartPref::New,
            "current" => TacticalStartPref::Current,
            "opening" => TacticalStartPref::Opening,
            "problem" => TacticalStartPref::Problem,
            "fen" => TacticalStartPref::Fen,
            "random_file" => TacticalStartPref::RandomFile,
            "random_base" => TacticalStartPref::RandomBase,
            _ => return None,
        })
    }
}

/// What the tactical game remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TacticalPrefs {
    /// The Elo of the engine for the next game.
    pub elo: u32,
    /// Change the Elo after each game (D2456).
    pub adjust: bool,
    /// The step of the change.
    pub adjust_step: u32,
    /// Where the game starts.
    pub start: TacticalStartPref,
    /// The FEN typed for [`TacticalStartPref::Fen`].
    pub fen: String,
    /// The file for [`TacticalStartPref::RandomFile`].
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub random_file: Option<PathBuf>,
    /// The score.
    pub stats: TacticalStats,
}

#[allow(clippy::ref_option)] // the signature serde's `skip_serializing_if` wants
fn path_is_unsavable(path: &Option<PathBuf>) -> bool {
    path.as_ref().is_none_or(|p| p.to_str().is_none())
}

impl Default for TacticalPrefs {
    fn default() -> TacticalPrefs {
        TacticalPrefs {
            elo: DEFAULT_TACTICAL_ELO,
            adjust: false,
            adjust_step: DEFAULT_ADJUST_STEP,
            start: TacticalStartPref::New,
            fen: String::new(),
            random_file: None,
            stats: TacticalStats::default(),
        }
    }
}

impl TacticalPrefs {
    /// The same preferences with every value inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> TacticalPrefs {
        self.elo = clamp_elo(self.elo);
        if !ADJUST_STEPS.contains(&self.adjust_step) {
            // The closest step the dialog offers.
            self.adjust_step = ADJUST_STEPS
                .iter()
                .copied()
                .min_by_key(|s| s.abs_diff(self.adjust_step))
                .unwrap_or(DEFAULT_ADJUST_STEP);
        }
        self.fen = self.fen.chars().take(MAX_FEN_PREF_CHARS).collect();
        self.stats = self.stats.sanitized();
        self
    }

    /// The setup of the game this stands for (D2451, D2456).
    #[must_use]
    pub fn setup(&self) -> TacticalSetup {
        TacticalSetup {
            elo: clamp_elo(self.elo),
            adjust_step: self.adjust.then_some(self.adjust_step),
            plan: None,
        }
    }

    /// Where the game starts, for the position of the board `problem`
    /// (D2457-D2459). The drawing of a position is left to the application
    /// ([`StartFrom::Pick`]).
    ///
    /// # Errors
    ///
    /// A [`StartProblem`]: bad FEN, no problem on the board, no file, no
    /// opening in the list.
    pub fn start_from(
        &self,
        play: &PlayPrefs,
        problem: Option<Position>,
    ) -> Result<StartFrom, StartProblem> {
        Ok(match self.start {
            TacticalStartPref::New => StartFrom::NewGame,
            TacticalStartPref::Current => StartFrom::CurrentPosition,
            TacticalStartPref::Opening => play
                .opening_lines()
                .into_iter()
                .find(|l| l.name == play.opening)
                .map_or(Err(StartProblem::NoOpening), |line| {
                    Ok(StartFrom::Opening(line))
                })?,
            TacticalStartPref::Problem => {
                StartFrom::Position(Box::new(problem.ok_or(StartProblem::NoProblem)?))
            }
            TacticalStartPref::Fen => StartFrom::Position(Box::new(parse_start_fen(&self.fen)?)),
            TacticalStartPref::RandomFile => StartFrom::Pick(PickSource::File(
                self.random_file.clone().ok_or(StartProblem::NoFile)?,
            )),
            TacticalStartPref::RandomBase => StartFrom::Pick(PickSource::CurrentBase),
        })
    }

    /// The configuration of a tactical game with `engine` (D2451): the
    /// shared settings of the game against an engine, the force of the
    /// preset instead of the force of the dialog, and `start`.
    #[must_use]
    pub fn to_config(&self, play: &PlayPrefs, engine: EngineId, start: StartFrom) -> PlayConfig {
        let mut config = play.to_config(engine);
        config.strength = crate::play::Strength::default();
        config.start = start;
        config.tactical = Some(self.setup());
        config.normalized()
    }

    /// Reads `[play.tactical]` field by field (D2464). A value of the wrong
    /// type or spelling goes back to the default and is reported.
    pub fn read(value: Option<&Value>, issues: &mut Vec<PrefsIssue>) -> TacticalPrefs {
        let d = TacticalPrefs::default();
        let table = match value {
            None => return d,
            Some(Value::Table(table)) => table,
            Some(_) => {
                issues.push(PrefsIssue::InvalidValue {
                    key: "play.tactical".to_owned(),
                });
                return d;
            }
        };
        let mut invalid = |key: &str| {
            issues.push(PrefsIssue::InvalidValue {
                key: format!("play.tactical.{key}"),
            });
        };
        let int = |key: &str, default: u32, invalid: &mut dyn FnMut(&str)| match table.get(key) {
            None => default,
            Some(Value::Integer(n)) => u32::try_from((*n).max(0)).unwrap_or(u32::MAX),
            Some(_) => {
                invalid(key);
                default
            }
        };
        let elo = int("elo", d.elo, &mut invalid);
        let adjust_step = int("adjust_step", d.adjust_step, &mut invalid);
        let adjust = match table.get("adjust") {
            None => d.adjust,
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                invalid("adjust");
                d.adjust
            }
        };
        let start = match table.get("start") {
            None => d.start,
            Some(Value::String(s)) => TacticalStartPref::from_name(s).unwrap_or_else(|| {
                invalid("start");
                d.start
            }),
            Some(_) => {
                invalid("start");
                d.start
            }
        };
        let fen = match table.get("fen") {
            None => d.fen.clone(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                invalid("fen");
                d.fen.clone()
            }
        };
        let random_file = match table.get("random_file") {
            None => None,
            Some(Value::String(s)) if !s.is_empty() => Some(PathBuf::from(s)),
            Some(_) => {
                invalid("random_file");
                None
            }
        };
        let stats = read_stats(table.get("stats"), &mut invalid);
        TacticalPrefs {
            elo,
            adjust,
            adjust_step,
            start,
            fen,
            random_file,
            stats,
        }
        .sanitized()
    }
}

fn read_stats(value: Option<&Value>, invalid: &mut dyn FnMut(&str)) -> TacticalStats {
    let d = TacticalStats::default();
    let table = match value {
        None => return d,
        Some(Value::Table(table)) => table,
        Some(_) => {
            invalid("stats");
            return d;
        }
    };
    let mut int = |key: &str| match table.get(key) {
        None => 0,
        Some(Value::Integer(n)) => u32::try_from((*n).max(0)).unwrap_or(u32::MAX),
        Some(_) => {
            invalid(&format!("stats.{key}"));
            0
        }
    };
    TacticalStats {
        games: int("games"),
        wins: int("wins"),
        draws: int("draws"),
        losses: int("losses"),
        streak: int("streak"),
        best_streak: int("best_streak"),
        best_win_elo: int("best_win_elo"),
    }
    .sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{Preferences, load_preferences, render_preferences};

    #[test]
    fn defaults_are_the_documented_ones() {
        let p = TacticalPrefs::default();
        assert_eq!(p.elo, 1500);
        assert!(!p.adjust);
        assert_eq!(p.adjust_step, 50);
        assert_eq!(p.start, TacticalStartPref::New);
        assert_eq!(Preferences::default().play.tactical, p);
    }

    #[test]
    fn an_old_file_without_the_table_opens_silently() {
        let loaded = load_preferences("[play]\nmovetime = 9\n");
        assert_eq!(loaded.prefs.play.tactical, TacticalPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn every_field_round_trips() {
        let mut prefs = Preferences::default();
        prefs.play.tactical = TacticalPrefs {
            elo: 2100,
            adjust: true,
            adjust_step: 100,
            start: TacticalStartPref::RandomFile,
            fen: "8/8/8/4k3/8/3K4/4P3/8 w - - 0 1".to_owned(),
            random_file: Some(PathBuf::from("puzzles.pgn")),
            stats: TacticalStats {
                games: 9,
                wins: 4,
                draws: 2,
                losses: 3,
                streak: 1,
                best_streak: 3,
                best_win_elo: 1900,
            },
        };
        let text = render_preferences(&prefs);
        let loaded = load_preferences(&text);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(loaded.prefs.play.tactical, prefs.play.tactical);
    }

    #[test]
    fn a_bad_field_only_resets_itself() {
        let loaded = load_preferences(
            "[play.tactical]\nelo = \"high\"\nadjust = true\nstart = \"nowhere\"\nadjust_step = 40\n",
        );
        let t = &loaded.prefs.play.tactical;
        assert_eq!(t.elo, 1500);
        assert!(t.adjust);
        assert_eq!(t.start, TacticalStartPref::New);
        assert_eq!(t.adjust_step, 50, "the nearest step offered");
        let keys: Vec<String> = loaded
            .issues
            .iter()
            .filter_map(|i| match i {
                PrefsIssue::InvalidValue { key } => Some(key.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(keys, ["play.tactical.elo", "play.tactical.start"]);
    }

    #[test]
    fn numbers_are_limited_and_the_table_must_be_a_table() {
        let loaded = load_preferences(
            "[play.tactical]\nelo = 99999\n[play.tactical.stats]\nwins = 7\ngames = 2\n",
        );
        assert_eq!(loaded.prefs.play.tactical.elo, 3200);
        assert_eq!(loaded.prefs.play.tactical.stats.games, 7);
        let loaded = load_preferences("[play]\ntactical = 3\n");
        assert_eq!(loaded.prefs.play.tactical, TacticalPrefs::default());
        assert!(!loaded.issues.is_empty());
    }

    #[test]
    fn the_start_follows_the_preference() {
        let play = PlayPrefs::default();
        let mut t = TacticalPrefs::default();
        assert_eq!(t.start_from(&play, None), Ok(StartFrom::NewGame));
        t.start = TacticalStartPref::Current;
        assert_eq!(t.start_from(&play, None), Ok(StartFrom::CurrentPosition));
        t.start = TacticalStartPref::Problem;
        assert_eq!(t.start_from(&play, None), Err(StartProblem::NoProblem));
        assert_eq!(
            t.start_from(&play, Some(Position::new())),
            Ok(StartFrom::Position(Box::new(Position::new())))
        );
        t.start = TacticalStartPref::Fen;
        assert_eq!(t.start_from(&play, None), Err(StartProblem::BadFen));
        t.fen = "8/8/8/4k3/8/3K4/4P3/8 w - - 0 1".to_owned();
        assert!(matches!(
            t.start_from(&play, None),
            Ok(StartFrom::Position(_))
        ));
        t.start = TacticalStartPref::RandomFile;
        assert_eq!(t.start_from(&play, None), Err(StartProblem::NoFile));
        t.random_file = Some(PathBuf::from("x.pgn"));
        assert_eq!(
            t.start_from(&play, None),
            Ok(StartFrom::Pick(PickSource::File(PathBuf::from("x.pgn"))))
        );
        t.start = TacticalStartPref::RandomBase;
        assert_eq!(
            t.start_from(&play, None),
            Ok(StartFrom::Pick(PickSource::CurrentBase))
        );
        t.start = TacticalStartPref::Opening;
        assert_eq!(t.start_from(&play, None), Err(StartProblem::NoOpening));
        let play = PlayPrefs {
            openings: vec!["Sicilian: e4 c5".to_owned()],
            opening: "Sicilian".to_owned(),
            ..PlayPrefs::default()
        };
        assert!(matches!(
            t.start_from(&play, None),
            Ok(StartFrom::Opening(_))
        ));
    }

    #[test]
    fn the_config_ignores_the_strength_of_the_normal_dialog() {
        let play = PlayPrefs {
            limit_strength: true,
            use_skill: true,
            ..PlayPrefs::default()
        };
        let t = TacticalPrefs {
            adjust: true,
            ..TacticalPrefs::default()
        };
        let config = t.to_config(&play, EngineId(3), StartFrom::NewGame);
        assert_eq!(config.strength, crate::play::Strength::default());
        let setup = config.tactical.expect("tactical");
        assert_eq!(setup.elo, 1500);
        assert_eq!(setup.adjust_step, Some(50));
        assert_eq!(setup.plan, None);
        assert_eq!(config.engine, EngineId(3));
    }
}
