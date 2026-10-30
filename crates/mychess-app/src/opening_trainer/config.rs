//! The configuration of an opening-trainer session: what the dialog edits
//! (spec M8-5, D2200-D2227).

use std::path::PathBuf;

/// The side that the user trains (D2213).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TrainSide {
    /// The user plays White in every line.
    #[default]
    White,
    /// The user plays Black; the machine moves first.
    Black,
    /// The colour alternates from line to line.
    Both,
}

impl TrainSide {
    /// The spelling used in `opening_trainer.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            TrainSide::White => "white",
            TrainSide::Black => "black",
            TrainSide::Both => "both",
        }
    }

    /// Reads the spelling of [`TrainSide::as_str`].
    pub fn from_name(text: &str) -> Option<TrainSide> {
        match text {
            "white" => Some(TrainSide::White),
            "black" => Some(TrainSide::Black),
            "both" => Some(TrainSide::Both),
            _ => None,
        }
    }
}

/// Where the repertoire comes from (D2202).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RepSourceKind {
    /// The open database whose type says "Openings for ..." (D2203).
    #[default]
    TypedBase,
    /// The filter of the current database.
    CurrentBase,
    /// A file: a Polyglot book, a tree mask or games (by extension).
    File,
}

impl RepSourceKind {
    /// The spelling used in `opening_trainer.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            RepSourceKind::TypedBase => "typed",
            RepSourceKind::CurrentBase => "base",
            RepSourceKind::File => "file",
        }
    }

    /// Reads the spelling of [`RepSourceKind::as_str`].
    pub fn from_name(text: &str) -> Option<RepSourceKind> {
        match text {
            "typed" => Some(RepSourceKind::TypedBase),
            "base" => Some(RepSourceKind::CurrentBase),
            "file" => Some(RepSourceKind::File),
            _ => None,
        }
    }
}

/// Range of the maximum depth, in plies (D2209).
pub const DEPTH_RANGE: (u8, u8) = (2, 60);
/// The default maximum depth, in plies (Scid: 10; D2209).
pub const DEFAULT_DEPTH: u8 = 16;

/// What the dialog edits. [`OpeningConfig::sanitized`] brings every field
/// into its range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningConfig {
    /// The side to train.
    pub side: TrainSide,
    /// Where the repertoire comes from.
    pub source: RepSourceKind,
    /// The file, when the source is a file.
    pub file: Option<PathBuf>,
    /// Maximum depth in plies (D2209).
    pub max_depth: u8,
    /// A dubious move of the user is an error (Scid `playerBestMove`, D2216).
    pub strict: bool,
    /// The machine never plays a dubious move (Scid `opBestMove`, D2218).
    pub opponent_best: bool,
    /// Do not repeat a line within the session (Scid `removeGames`, D2220).
    pub no_repeat: bool,
    /// Only games flagged as opening lines (Scid `onlyFlaggedLines`, D2206).
    pub only_flagged: bool,
    /// Show the candidate moves all the time (Scid `displayCM`, D2217).
    pub show_candidates: bool,
    /// From a mask: only the moves marked "to train" or green, on the turns
    /// of the user, when the mask has any (D2208).
    pub only_marked: bool,
}

impl Default for OpeningConfig {
    fn default() -> OpeningConfig {
        OpeningConfig {
            side: TrainSide::White,
            source: RepSourceKind::TypedBase,
            file: None,
            max_depth: DEFAULT_DEPTH,
            strict: true,
            opponent_best: true,
            no_repeat: false,
            only_flagged: false,
            show_candidates: false,
            only_marked: true,
        }
    }
}

impl OpeningConfig {
    /// The configuration with every number in its range.
    #[must_use]
    pub fn sanitized(mut self) -> OpeningConfig {
        self.max_depth = self.max_depth.clamp(DEPTH_RANGE.0, DEPTH_RANGE.1);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_depth_is_clamped_into_its_range() {
        let low = OpeningConfig {
            max_depth: 0,
            ..OpeningConfig::default()
        };
        assert_eq!(low.sanitized().max_depth, DEPTH_RANGE.0);
        let high = OpeningConfig {
            max_depth: 200,
            ..OpeningConfig::default()
        };
        assert_eq!(high.sanitized().max_depth, DEPTH_RANGE.1);
    }

    #[test]
    fn the_spellings_round_trip() {
        for side in [TrainSide::White, TrainSide::Black, TrainSide::Both] {
            assert_eq!(TrainSide::from_name(side.as_str()), Some(side));
        }
        for kind in [
            RepSourceKind::TypedBase,
            RepSourceKind::CurrentBase,
            RepSourceKind::File,
        ] {
            assert_eq!(RepSourceKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(TrainSide::from_name("x"), None);
    }
}
