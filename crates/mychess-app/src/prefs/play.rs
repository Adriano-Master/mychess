//! Preferences of the game against an engine: the `[play]` table (spec
//! M8-1, D1242). The dialog of M8-1b edits them; the engine, the colour and
//! the clocks are remembered between runs. `Store clock` acts since M8-1c
//! (D1506); ponder and the coach act since M8-1d (D2400, D2419).

use std::path::PathBuf;

use serde::Serialize;

use crate::engines::EngineId;
use crate::play::{
    BONUS_RANGE, BookChoice, BookSelection, COACH_DEPTH_RANGE, COACH_SECONDS_RANGE, CoachConfig,
    ColorChoice, DEFAULT_COACH_DEPTH, DEFAULT_COACH_SECONDS, MINUTES_RANGE, MOVETIME_RANGE,
    NODES_RANGE, OpeningLine, PLAY_DEPTH_RANGE, PlayConfig, StartFrom, Strength, TimeMode,
};
use crate::tactical_game::TacticalPrefs;

/// Largest number of opening lines kept.
pub const MAX_OPENING_LINES: usize = 500;
/// Longest player name kept, in characters.
pub const MAX_PLAYER_NAME_CHARS: usize = 100;

macro_rules! pref_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[non_exhaustive]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            /// The spelling used in the TOML file.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// Reads the TOML spelling.
            pub fn from_name(text: &str) -> Option<$name> {
                match text { $($text => Some($name::$variant),)+ _ => None }
            }
        }
    };
}

pref_enum! {
    /// The colour of the player.
    PlayColorPref {
        /// White.
        White => "white",
        /// Black.
        Black => "black",
        /// A roll decides.
        Random => "random",
    }
}

pref_enum! {
    /// The mode of time.
    PlayModePref {
        /// Clock with increment.
        TimeBonus => "timebonus",
        /// Fixed depth.
        Depth => "depth",
        /// Fixed nodes.
        Nodes => "nodes",
        /// Seconds per move.
        MoveTime => "movetime",
    }
}

pref_enum! {
    /// Where the game starts.
    PlayStartPref {
        /// The standard position.
        NewGame => "new",
        /// The position on the board.
        Current => "current",
        /// A specific opening.
        Opening => "opening",
    }
}

pref_enum! {
    /// How the book move is chosen.
    PlayBookPref {
        /// The heaviest move.
        Best => "best",
        /// Random, weighted.
        Weighted => "weighted",
    }
}

/// Everything the dialog remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlayPrefs {
    /// The engine, by its stable identity in `engines.toml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<u64>,
    /// The colour.
    pub color: PlayColorPref,
    /// The mode.
    pub mode: PlayModePref,
    /// Minutes of White (`timebonus`).
    pub white_minutes: u32,
    /// Increment of White in seconds.
    pub white_bonus: u32,
    /// Minutes of Black.
    pub black_minutes: u32,
    /// Increment of Black in seconds.
    pub black_bonus: u32,
    /// Depth of the `depth` mode.
    pub depth: u32,
    /// Nodes of the `nodes` mode.
    pub nodes: u32,
    /// Seconds of the `movetime` mode.
    pub movetime: u32,
    /// Where the game starts.
    pub start: PlayStartPref,
    /// Name of the last specific opening used.
    pub opening: String,
    /// The list of openings, `Name: moves` per entry.
    pub openings: Vec<String>,
    /// Play from a book.
    pub use_book: bool,
    /// The book file.
    #[serde(skip_serializing_if = "path_is_unsavable")]
    pub book_path: Option<PathBuf>,
    /// How the book move is chosen.
    pub book_selection: PlayBookPref,
    /// Limit the engine's strength (`UCI_Elo`).
    pub limit_strength: bool,
    /// The `UCI_Elo` value.
    pub elo: u32,
    /// Use `Skill Level`.
    pub use_skill: bool,
    /// The `Skill Level` value.
    pub skill: u32,
    /// The name of the player for the tags; empty is `?`.
    pub player_name: String,
    /// Think on the player's time (M8-1c).
    pub ponder: bool,
    /// Write `[%emt]` in the comments (M8-1c).
    pub store_clock: bool,
    /// The coach watches the moves of the player (M8-1d, D2419).
    pub coach: bool,
    /// The coach engine, by its stable identity in `engines.toml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coach_engine: Option<u64>,
    /// Depth of each search of the coach.
    pub coach_depth: u32,
    /// Seconds each search of the coach may last.
    pub coach_seconds: u32,
    /// The remark names the best move.
    pub coach_hint: bool,
    /// The remark shows the loss in pawns.
    pub coach_numbers: bool,
    /// The tactical game (M8-7, D2464), in `[play.tactical]`.
    pub tactical: TacticalPrefs,
}

#[allow(clippy::ref_option)] // the signature serde's `skip_serializing_if` wants
fn path_is_unsavable(path: &Option<PathBuf>) -> bool {
    path.as_ref().is_none_or(|p| p.to_str().is_none())
}

impl Default for PlayPrefs {
    fn default() -> PlayPrefs {
        PlayPrefs {
            engine: None,
            color: PlayColorPref::White,
            mode: PlayModePref::MoveTime,
            white_minutes: 5,
            white_bonus: 3,
            black_minutes: 5,
            black_bonus: 3,
            depth: 3,
            nodes: 1000,
            movetime: 6,
            start: PlayStartPref::NewGame,
            opening: String::new(),
            openings: Vec::new(),
            use_book: false,
            book_path: None,
            book_selection: PlayBookPref::Weighted,
            limit_strength: false,
            elo: 1500,
            use_skill: false,
            skill: 10,
            player_name: String::new(),
            ponder: false,
            store_clock: false,
            coach: false,
            coach_engine: None,
            coach_depth: DEFAULT_COACH_DEPTH,
            coach_seconds: DEFAULT_COACH_SECONDS,
            coach_hint: false,
            coach_numbers: true,
            tactical: TacticalPrefs::default(),
        }
    }
}

impl PlayPrefs {
    /// The same preferences with every number inside its range and the
    /// lists bounded.
    #[must_use]
    pub fn sanitized(mut self) -> PlayPrefs {
        let clamp = |range: (u32, u32), v: u32| v.clamp(range.0, range.1);
        self.white_minutes = clamp(MINUTES_RANGE, self.white_minutes);
        self.black_minutes = clamp(MINUTES_RANGE, self.black_minutes);
        self.white_bonus = clamp(BONUS_RANGE, self.white_bonus);
        self.black_bonus = clamp(BONUS_RANGE, self.black_bonus);
        self.depth = clamp(PLAY_DEPTH_RANGE, self.depth);
        self.nodes = clamp(NODES_RANGE, self.nodes);
        self.movetime = clamp(MOVETIME_RANGE, self.movetime);
        self.elo = self.elo.min(10_000);
        self.skill = self.skill.min(100);
        self.coach_depth = clamp(COACH_DEPTH_RANGE, self.coach_depth);
        self.coach_seconds = clamp(COACH_SECONDS_RANGE, self.coach_seconds);
        self.tactical = self.tactical.sanitized();
        self.openings.truncate(MAX_OPENING_LINES);
        self.player_name = self
            .player_name
            .chars()
            .take(MAX_PLAYER_NAME_CHARS)
            .collect();
        self
    }

    /// The opening lines that parse.
    pub fn opening_lines(&self) -> Vec<OpeningLine> {
        self.openings
            .iter()
            .filter_map(|line| OpeningLine::parse(line))
            .collect()
    }

    /// The time mode.
    pub fn time_mode(&self) -> TimeMode {
        match self.mode {
            PlayModePref::TimeBonus => TimeMode::TimeBonus {
                white_minutes: self.white_minutes,
                white_bonus: self.white_bonus,
                black_minutes: self.black_minutes,
                black_bonus: self.black_bonus,
            },
            PlayModePref::Depth => TimeMode::Depth(self.depth),
            PlayModePref::Nodes => TimeMode::Nodes(self.nodes),
            _ => TimeMode::MoveTime(self.movetime),
        }
    }

    /// The coach, when it is on, has an engine and that engine is not
    /// `opponent` (D2416, D2419).
    pub fn coach_config(&self, opponent: EngineId) -> Option<CoachConfig> {
        let id = EngineId(self.coach_engine?);
        (self.coach && id != opponent).then_some(CoachConfig {
            engine: id,
            depth: self.coach_depth,
            seconds: self.coach_seconds,
            hint: self.coach_hint,
            numbers: self.coach_numbers,
        })
    }

    /// The configuration for `engine` (the remembered one, or any, is the
    /// caller's choice). An opening that is no longer in the list falls back
    /// to a new game.
    pub fn to_config(&self, engine: EngineId) -> PlayConfig {
        let start = match self.start {
            PlayStartPref::Current => StartFrom::CurrentPosition,
            PlayStartPref::Opening => self
                .opening_lines()
                .into_iter()
                .find(|l| l.name == self.opening)
                .map_or(StartFrom::NewGame, StartFrom::Opening),
            _ => StartFrom::NewGame,
        };
        let book = if self.use_book {
            self.book_path.clone().map(|path| BookChoice {
                path,
                selection: match self.book_selection {
                    PlayBookPref::Best => BookSelection::Best,
                    _ => BookSelection::Weighted,
                },
            })
        } else {
            None
        };
        PlayConfig {
            engine,
            color: match self.color {
                PlayColorPref::White => ColorChoice::White,
                PlayColorPref::Black => ColorChoice::Black,
                _ => ColorChoice::Random,
            },
            mode: self.time_mode(),
            start,
            book,
            strength: Strength {
                elo: self.limit_strength.then_some(i64::from(self.elo)),
                skill: self.use_skill.then_some(i64::from(self.skill)),
            },
            store_clock: self.store_clock,
            ponder: self.ponder,
            coach: self.coach_config(engine),
            tactical: None,
        }
        .normalized()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{Preferences, load_preferences, render_preferences};

    #[test]
    fn defaults_follow_the_scid_dialog() {
        let p = PlayPrefs::default();
        assert_eq!(p.mode, PlayModePref::MoveTime);
        assert_eq!((p.depth, p.nodes, p.movetime), (3, 1000, 6));
        assert_eq!(Preferences::default().play, Box::new(p));
    }

    #[test]
    fn an_old_file_without_the_section_opens_silently() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(*loaded.prefs.play, PlayPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn every_field_round_trips() {
        let play = PlayPrefs {
            engine: Some(4),
            color: PlayColorPref::Random,
            mode: PlayModePref::TimeBonus,
            white_minutes: 10,
            white_bonus: 5,
            black_minutes: 7,
            black_bonus: 2,
            depth: 8,
            nodes: 500,
            movetime: 12,
            start: PlayStartPref::Opening,
            opening: "Sicilian".to_owned(),
            openings: vec!["Sicilian: e4 c5".to_owned(), "QGD: d4 d5 c4 e6".to_owned()],
            use_book: true,
            book_path: Some(PathBuf::from("books").join("main.bin")),
            book_selection: PlayBookPref::Best,
            limit_strength: true,
            elo: 1800,
            use_skill: true,
            skill: 4,
            player_name: "Ana".to_owned(),
            ponder: true,
            store_clock: true,
            coach: true,
            coach_engine: Some(9),
            coach_depth: 8,
            coach_seconds: 5,
            coach_hint: true,
            coach_numbers: false,
            tactical: TacticalPrefs::default(),
        };
        let prefs = Preferences {
            play: Box::new(play.clone()),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[play]"), "{text}");
        let loaded = load_preferences(&text);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(*loaded.prefs.play, play);
    }

    #[test]
    fn bad_values_reset_only_their_field_and_numbers_are_clamped() {
        let loaded = load_preferences(
            "[play]\ncolor = \"green\"\nmode = \"depth\"\ndepth = 99\nnodes = 1\nmovetime = \"x\"\nwhite_minutes = 0\n",
        );
        let p = &loaded.prefs.play;
        assert_eq!(p.color, PlayColorPref::White);
        assert_eq!(p.mode, PlayModePref::Depth);
        assert_eq!((p.depth, p.nodes, p.white_minutes), (20, 100, 1));
        assert_eq!(p.movetime, 6);
        assert!(loaded.issues.len() >= 2, "{:?}", loaded.issues);
    }

    #[test]
    fn the_config_follows_the_preferences() {
        let play = PlayPrefs {
            mode: PlayModePref::Nodes,
            nodes: 300,
            color: PlayColorPref::Black,
            start: PlayStartPref::Opening,
            opening: "A".to_owned(),
            openings: vec!["A: e4 e5".to_owned()],
            use_book: true,
            book_path: Some(PathBuf::from("b.bin")),
            limit_strength: true,
            elo: 1400,
            ..PlayPrefs::default()
        };
        let config = play.to_config(EngineId(3));
        assert_eq!(config.engine, EngineId(3));
        assert_eq!(config.mode, TimeMode::Nodes(300));
        assert_eq!(config.color, ColorChoice::Black);
        assert!(matches!(config.start, StartFrom::Opening(ref l) if l.name == "A"));
        assert!(config.book.is_some());
        assert_eq!(config.strength.elo, Some(1400));
        assert_eq!(config.strength.skill, None);
        // The opening vanished from the list: a new game.
        let gone = PlayPrefs {
            openings: Vec::new(),
            ..play
        };
        assert_eq!(gone.to_config(EngineId(3)).start, StartFrom::NewGame);
    }
}
