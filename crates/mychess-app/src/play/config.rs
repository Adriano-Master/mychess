//! The configuration of a game against an engine (spec M8-1, D1222-D1224,
//! D1234-D1237).
//!
//! Plain data with the ranges of the Scid's dialog (`sergame.tcl` `config`,
//! lines 31-324). Everything is validated by [`PlayConfig::normalized`]; a
//! value outside its range is clamped, never rejected, because the values
//! come from text fields and from the preferences file.

use std::path::PathBuf;
use std::time::Duration;

use mychess_core::{Color, Move, Position, SanMode};

use super::clock::{PlayClock, SideBudget};
use super::coach::CoachConfig;
use crate::engines::EngineId;
use crate::tactical_game::{PickSource, TacticalSetup};

/// Minutes of a `timebonus` side, as in the Scid's spin boxes.
pub const MINUTES_RANGE: (u32, u32) = (1, 120);
/// Increment in seconds of a `timebonus` side.
pub const BONUS_RANGE: (u32, u32) = (0, 60);
/// Fixed depth of the `depth` mode.
pub const PLAY_DEPTH_RANGE: (u32, u32) = (1, 20);
/// Fixed node count of the `nodes` mode, exactly as the Scid sends it.
pub const NODES_RANGE: (u32, u32) = (100, 2000);
/// Seconds per move of the `movetime` mode.
pub const MOVETIME_RANGE: (u32, u32) = (1, 120);

/// How the engine's thinking and the clock are limited (D1224).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeMode {
    /// Clock with time and increment per side (`go wtime ... winc ...`).
    TimeBonus {
        /// Minutes of White.
        white_minutes: u32,
        /// Increment of White in seconds.
        white_bonus: u32,
        /// Minutes of Black.
        black_minutes: u32,
        /// Increment of Black in seconds.
        black_bonus: u32,
    },
    /// `go depth N`.
    Depth(u32),
    /// `go nodes N`.
    Nodes(u32),
    /// `go movetime N*1000`, N in seconds.
    MoveTime(u32),
}

impl Default for TimeMode {
    /// `movetime`, 6 seconds (D1224).
    fn default() -> TimeMode {
        TimeMode::MoveTime(6)
    }
}

fn clamp((low, high): (u32, u32), value: u32) -> u32 {
    value.clamp(low, high)
}

impl TimeMode {
    /// The mode with every number inside its range.
    #[must_use]
    pub fn normalized(self) -> TimeMode {
        match self {
            TimeMode::TimeBonus {
                white_minutes,
                white_bonus,
                black_minutes,
                black_bonus,
            } => TimeMode::TimeBonus {
                white_minutes: clamp(MINUTES_RANGE, white_minutes),
                white_bonus: clamp(BONUS_RANGE, white_bonus),
                black_minutes: clamp(MINUTES_RANGE, black_minutes),
                black_bonus: clamp(BONUS_RANGE, black_bonus),
            },
            TimeMode::Depth(n) => TimeMode::Depth(clamp(PLAY_DEPTH_RANGE, n)),
            TimeMode::Nodes(n) => TimeMode::Nodes(clamp(NODES_RANGE, n)),
            TimeMode::MoveTime(n) => TimeMode::MoveTime(clamp(MOVETIME_RANGE, n)),
        }
    }

    /// The clock of the game: a countdown for `timebonus`, otherwise a
    /// clock that only counts (D1226).
    pub fn clock(self) -> PlayClock {
        match self.normalized() {
            TimeMode::TimeBonus {
                white_minutes,
                white_bonus,
                black_minutes,
                black_bonus,
            } => {
                let side = |minutes: u32, bonus: u32| SideBudget {
                    time: Duration::from_secs(u64::from(minutes) * 60),
                    increment: Duration::from_secs(u64::from(bonus)),
                };
                PlayClock::countdown(
                    side(white_minutes, white_bonus),
                    side(black_minutes, black_bonus),
                )
            }
            _ => PlayClock::count_up(),
        }
    }

    /// The tag `Timemode` the Scid writes (`sergame.tcl` 486-499).
    pub fn timemode_tag(self) -> String {
        match self.normalized() {
            TimeMode::TimeBonus {
                white_minutes,
                white_bonus,
                black_minutes,
                black_bonus,
            } => format!("Time+Bonus {white_minutes}+{white_bonus}/{black_minutes}+{black_bonus}"),
            TimeMode::Depth(n) => format!("Depth {n}"),
            TimeMode::Nodes(n) => format!("Nodes {n}"),
            TimeMode::MoveTime(n) => format!("Movetime {n}"),
        }
    }

    /// The PGN tag `TimeControl` (D1232), when the mode has one. Sides with
    /// different budgets have no single value, so the tag is left out.
    pub fn time_control_tag(self) -> Option<String> {
        match self.normalized() {
            TimeMode::TimeBonus {
                white_minutes,
                white_bonus,
                black_minutes,
                black_bonus,
            } if white_minutes == black_minutes && white_bonus == black_bonus => {
                let seconds = u64::from(white_minutes) * 60;
                Some(if white_bonus == 0 {
                    seconds.to_string()
                } else {
                    format!("{seconds}+{white_bonus}")
                })
            }
            TimeMode::MoveTime(n) => Some(format!("1/{n}")),
            _ => None,
        }
    }
}

/// The colour the player takes (D1222).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    /// White.
    #[default]
    White,
    /// Black.
    Black,
    /// Decided by a roll when the game starts.
    Random,
}

impl ColorChoice {
    /// The colour of the player; `roll` decides `Random` (even: White).
    pub fn resolve(self, roll: u64) -> Color {
        match self {
            ColorChoice::White => Color::White,
            ColorChoice::Black => Color::Black,
            ColorChoice::Random => {
                if roll.is_multiple_of(2) {
                    Color::White
                } else {
                    Color::Black
                }
            }
        }
    }
}

/// A named line of moves, `Name: e4 e5 Nf3` (D1235).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningLine {
    /// The name, written to the `Opening` tag.
    pub name: String,
    /// The moves in SAN, from the standard start position.
    pub moves: Vec<String>,
}

/// Removes a leading move number (`1.`, `12...`) from `token`. A number
/// without a dot is not one (it is left alone).
fn strip_move_number(token: &str) -> &str {
    let digits = token.trim_start_matches(|c: char| c.is_ascii_digit());
    if digits.len() == token.len() || !digits.starts_with('.') {
        return token;
    }
    digits.trim_start_matches('.')
}

impl OpeningLine {
    /// Reads `Name: moves`. Move numbers (`1.`, `1...`) and results are
    /// ignored. `None` for a line without a name or without moves.
    pub fn parse(line: &str) -> Option<OpeningLine> {
        let (name, rest) = line.split_once(':')?;
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let moves: Vec<String> = rest
            .split_whitespace()
            .filter(|token| !matches!(*token, "1-0" | "0-1" | "1/2-1/2" | "*"))
            .filter_map(|token| {
                let bare = strip_move_number(token);
                // Zeros for castling (`0-0`) are tolerated and normalised.
                let san = match bare {
                    "0-0" => "O-O",
                    "0-0-0" => "O-O-O",
                    other => other,
                };
                (!san.is_empty()).then(|| san.to_owned())
            })
            .collect();
        if moves.is_empty() {
            return None;
        }
        Some(OpeningLine {
            name: name.to_owned(),
            moves,
        })
    }

    /// Reads a list of lines, one per text line; invalid lines are skipped.
    pub fn parse_list(text: &str) -> Vec<OpeningLine> {
        text.lines().filter_map(OpeningLine::parse).collect()
    }

    /// The moves as legal moves from the standard start position. `None`
    /// when a move is not legal (the line is wrong).
    pub fn to_moves(&self) -> Option<Vec<Move>> {
        let mut position = Position::new();
        let mut out = Vec::with_capacity(self.moves.len());
        for san in &self.moves {
            let parsed = position.parse_san(san, SanMode::Tolerant).ok()?;
            position.play(parsed.mv).ok()?;
            out.push(parsed.mv);
        }
        Some(out)
    }
}

/// Where the game starts from (D1236).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StartFrom {
    /// The standard start position.
    #[default]
    NewGame,
    /// The position shown on the board when the game starts. The glue
    /// fills `fen` from the session.
    CurrentPosition,
    /// A specific opening (D1235).
    Opening(OpeningLine),
    /// A position that is already known: a typed FEN or the problem of
    /// the tactics training (M8-7, D2457).
    Position(Box<Position>),
    /// A position still to be drawn from a file or the current database;
    /// the application replaces it with a [`StartFrom::Position`] (M8-7,
    /// D2460).
    Pick(PickSource),
}

/// How a move is chosen in the book (D1234).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookSelection {
    /// The move of greatest weight.
    Best,
    /// A random move, with probability proportional to the weight.
    #[default]
    Weighted,
}

/// The book the engine plays from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookChoice {
    /// The Polyglot file.
    pub path: PathBuf,
    /// How the move is picked.
    pub selection: BookSelection,
}

/// The limited strength of the engine (D1237). `None` leaves the option
/// alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Strength {
    /// `UCI_LimitStrength` is turned on with this `UCI_Elo`.
    pub elo: Option<i64>,
    /// `Skill Level`.
    pub skill: Option<i64>,
}

/// The whole configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayConfig {
    /// The engine of the list.
    pub engine: EngineId,
    /// The colour of the player.
    pub color: ColorChoice,
    /// The limits and the clock.
    pub mode: TimeMode,
    /// The start position.
    pub start: StartFrom,
    /// The book, if any.
    pub book: Option<BookChoice>,
    /// The limited strength.
    pub strength: Strength,
    /// Write `[%emt]` (and, in `timebonus`, `[%clk]`) in the comment of
    /// every move that cost time (M8-1c, D1506).
    pub store_clock: bool,
    /// The engine thinks on the player's time (M8-1d, D2400).
    pub ponder: bool,
    /// The coach, an auxiliary engine that judges the moves of the player
    /// (M8-1d, D2418).
    pub coach: Option<CoachConfig>,
    /// Makes the game a tactical one: the strength comes from an Elo
    /// (M8-7, D2450).
    pub tactical: Option<TacticalSetup>,
}

impl PlayConfig {
    /// A configuration with the defaults for `engine`.
    pub fn new(engine: EngineId) -> PlayConfig {
        PlayConfig {
            engine,
            color: ColorChoice::default(),
            mode: TimeMode::default(),
            start: StartFrom::default(),
            book: None,
            strength: Strength::default(),
            store_clock: false,
            ponder: false,
            coach: None,
            tactical: None,
        }
    }

    /// The configuration with every number inside its range.
    #[must_use]
    pub fn normalized(mut self) -> PlayConfig {
        self.mode = self.mode.normalized();
        self.coach = self.coach.map(CoachConfig::normalized);
        self.tactical = self.tactical.map(TacticalSetup::normalized);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mode_is_six_seconds_per_move() {
        assert_eq!(TimeMode::default(), TimeMode::MoveTime(6));
    }

    #[test]
    fn numbers_are_clamped_to_the_scid_ranges() {
        assert_eq!(TimeMode::Depth(0).normalized(), TimeMode::Depth(1));
        assert_eq!(TimeMode::Depth(99).normalized(), TimeMode::Depth(20));
        assert_eq!(TimeMode::Nodes(5).normalized(), TimeMode::Nodes(100));
        assert_eq!(TimeMode::Nodes(9999).normalized(), TimeMode::Nodes(2000));
        assert_eq!(
            TimeMode::MoveTime(500).normalized(),
            TimeMode::MoveTime(120)
        );
        assert_eq!(
            TimeMode::TimeBonus {
                white_minutes: 0,
                white_bonus: 100,
                black_minutes: 999,
                black_bonus: 3
            }
            .normalized(),
            TimeMode::TimeBonus {
                white_minutes: 1,
                white_bonus: 60,
                black_minutes: 120,
                black_bonus: 3
            }
        );
    }

    #[test]
    fn only_timebonus_has_a_countdown() {
        let bonus = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 3,
        };
        assert!(bonus.clock().is_countdown());
        for mode in [
            TimeMode::Depth(3),
            TimeMode::Nodes(1000),
            TimeMode::MoveTime(6),
        ] {
            assert!(!mode.clock().is_countdown(), "{mode:?}");
        }
    }

    #[test]
    fn tags_follow_d1231_and_d1232() {
        let bonus = TimeMode::TimeBonus {
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 3,
        };
        assert_eq!(bonus.timemode_tag(), "Time+Bonus 5+3/5+3");
        assert_eq!(bonus.time_control_tag().as_deref(), Some("300+3"));
        assert_eq!(TimeMode::MoveTime(6).timemode_tag(), "Movetime 6");
        assert_eq!(
            TimeMode::MoveTime(6).time_control_tag().as_deref(),
            Some("1/6")
        );
        assert_eq!(TimeMode::Nodes(1000).timemode_tag(), "Nodes 1000");
        assert_eq!(TimeMode::Depth(3).timemode_tag(), "Depth 3");
        assert_eq!(TimeMode::Depth(3).time_control_tag(), None);
        let no_bonus = TimeMode::TimeBonus {
            white_minutes: 10,
            white_bonus: 0,
            black_minutes: 10,
            black_bonus: 0,
        };
        assert_eq!(no_bonus.time_control_tag().as_deref(), Some("600"));
        let uneven = TimeMode::TimeBonus {
            white_minutes: 10,
            white_bonus: 0,
            black_minutes: 5,
            black_bonus: 0,
        };
        assert_eq!(uneven.time_control_tag(), None);
    }

    #[test]
    fn random_colour_follows_the_roll() {
        assert_eq!(ColorChoice::Random.resolve(0), Color::White);
        assert_eq!(ColorChoice::Random.resolve(7), Color::Black);
        assert_eq!(ColorChoice::Black.resolve(0), Color::Black);
    }

    #[test]
    fn parses_opening_lines() {
        let line = OpeningLine::parse("Sicilian: 1.e4 c5 2. Nf3 d6").expect("line");
        assert_eq!(line.name, "Sicilian");
        assert_eq!(line.moves, ["e4", "c5", "Nf3", "d6"]);
        assert_eq!(line.to_moves().expect("legal").len(), 4);
        assert!(OpeningLine::parse("no colon e4").is_none());
        assert!(OpeningLine::parse(": e4").is_none());
        assert!(OpeningLine::parse("Empty:").is_none());
        let list = OpeningLine::parse_list("A: e4\nbad\nB: d4 d5\n");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn opening_line_drops_results_and_keeps_castling() {
        let line = OpeningLine::parse("A: 1.e4 e5 1-0 * 0-1 1/2-1/2").expect("line");
        assert_eq!(line.moves, ["e4", "e5"]);
        let line =
            OpeningLine::parse("B: 1. e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.0-0 Nf6 5.0-0-0").expect("line");
        assert_eq!(
            line.moves,
            [
                "e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O", "Nf6", "O-O-O"
            ]
        );
        assert!(OpeningLine::parse("C: 1-0").is_none());
        let line = OpeningLine::parse("D: 1...e5 2...Nc6").expect("line");
        assert_eq!(line.moves, ["e5", "Nc6"]);
    }

    #[test]
    fn an_illegal_line_has_no_moves() {
        let line = OpeningLine::parse("Bad: e4 e4").expect("line");
        assert!(line.to_moves().is_none());
    }
}
