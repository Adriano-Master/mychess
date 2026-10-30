//! The options of the automatic annotation and the informant thresholds
//! (spec M4-6, D461, D462, D466, D474).
//!
//! The defaults are the Scid's (`start.tcl` 177-189 and 1324-1331), except
//! the score format (D466).

use std::ops::RangeInclusive;
use std::time::Duration;

/// How the engine is limited at each position (D462).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Control {
    /// `go depth N` ([`AnnotateOptions::depth`]).
    #[default]
    Depth,
    /// `go movetime T` ([`AnnotateOptions::movetime`]).
    Time,
}

/// Which scores are written (`annotate(WithScore)`, "Add Score").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WithScore {
    /// Every move (the Scid's default).
    #[default]
    AllMoves,
    /// Only the moves that are mistakes or not the best.
    Blunders,
    /// Only on the variations (the score of the move that has one).
    Variations,
    /// No score at all.
    No,
}

/// When a variation (the engine's line) is added (`annotate(WithVars)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WithVars {
    /// When the move is a mistake (the Scid's default).
    #[default]
    Blunders,
    /// When the move is not the best: any loss.
    NotBest,
    /// On every move.
    AllMoves,
    /// Never: only scores.
    No,
}

/// Whose moves are annotated (`annotate(Moves)`, "Which Side").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    /// Both sides.
    #[default]
    All,
    /// Only White's moves.
    White,
    /// Only Black's moves.
    Black,
}

/// How a score is written in a comment (`annotate(scoreType)`, D466).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScoreFormat {
    /// `+1.50` (the Scid's default).
    Plain,
    /// `[% +1.50]`.
    Bracket,
    /// `[%eval +1.50]`, the command PGN tools understand (the MyChess
    /// default; a mate is written `#3`).
    #[default]
    Eval,
}

/// The range of the depth per move.
pub const DEPTH_RANGE: RangeInclusive<u32> = 10..=100;
/// The range of the time per move, in milliseconds.
pub const MOVETIME_RANGE_MS: RangeInclusive<u64> = 100..=300_000;
/// The range of the error threshold, in pawns.
pub const BLUNDER_RANGE: RangeInclusive<f32> = 0.1..=3.0;
/// The range of the cut-off, in pawns.
pub const CUTOFF_RANGE: RangeInclusive<f32> = 3.0..=10.0;
/// The range of the number of opening moves.
pub const OPENING_MOVES_RANGE: RangeInclusive<u8> = 5..=20;
/// The default number of opening moves (`main.tcl` 1706-1714).
pub const DEFAULT_OPENING_MOVES: u8 = 10;
/// The range of an informant threshold, in pawns.
pub const INFORMANT_RANGE: RangeInclusive<f32> = 0.0..=9.9;

/// What to annotate and how (`annotate(...)` of the Scid).
///
/// The opening book is chosen in the book window (spec M5-6), not here: only
/// the switch `use_book` is an option of the run (see
/// [`BookProbe`](super::BookProbe)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnnotateOptions {
    /// Depth or time limit of each search.
    pub control: Control,
    /// Depth per move when `control` is [`Control::Depth`]; also the depth
    /// the "missed mates" test assumes under a time limit (the Scid does
    /// the same, `analysis.tcl` 1534-1537).
    pub depth: u32,
    /// Time per move when `control` is [`Control::Time`].
    pub movetime: Duration,
    /// The loss, in pawns, that makes a move a mistake (`blunder`).
    pub blunder: f32,
    /// Positions evaluated beyond this many pawns (both before and after the
    /// move) are not judged (`cutoff`).
    pub cutoff: f32,
    /// Which scores are written.
    pub with_score: WithScore,
    /// When variations are added.
    pub with_vars: WithVars,
    /// Look for missed or shorter mates (UCI only).
    pub missed_mates: bool,
    /// Write the engine's line for the final position as a comment of the
    /// last move (D468).
    pub var_at_end: bool,
    /// Whose moves are annotated.
    pub side: Side,
    /// How a score is written.
    pub score_format: ScoreFormat,
    /// Stop after this many opening moves (D473).
    pub opening_only: Option<u8>,
    /// Add the `Annotator` (and `Depth`) tags (D467).
    pub add_tag: bool,
    /// Also annotate the variations that the game already has.
    pub variations: bool,
    /// Mark tactical exercises (UCI only, D470).
    pub mark_exercises: bool,
    /// Annotate every game of the filter of the current database and save
    /// each one in it (phase B, D1520).
    pub batch: bool,
    /// Do not search the moves of the opening book and comment the first
    /// one that leaves it (spec M5-6, D1664-D1665). Ignored when no opening
    /// book is loaded.
    pub use_book: bool,
}

impl Default for AnnotateOptions {
    fn default() -> AnnotateOptions {
        AnnotateOptions {
            control: Control::Depth,
            depth: 13,
            movetime: Duration::from_millis(5000),
            blunder: 1.0,
            cutoff: 5.0,
            with_score: WithScore::AllMoves,
            with_vars: WithVars::Blunders,
            missed_mates: true,
            var_at_end: true,
            side: Side::All,
            score_format: ScoreFormat::Eval,
            opening_only: None,
            add_tag: true,
            variations: false,
            mark_exercises: false,
            batch: false,
            use_book: false,
        }
    }
}

impl AnnotateOptions {
    /// The options with every number inside its range (a value read from a
    /// text field may be anything). NaN becomes the default.
    #[must_use]
    pub fn normalized(mut self) -> AnnotateOptions {
        let defaults = AnnotateOptions::default();
        self.depth = self.depth.clamp(*DEPTH_RANGE.start(), *DEPTH_RANGE.end());
        let ms = u64::try_from(self.movetime.as_millis()).unwrap_or(u64::MAX);
        self.movetime =
            Duration::from_millis(ms.clamp(*MOVETIME_RANGE_MS.start(), *MOVETIME_RANGE_MS.end()));
        self.blunder = clamp_f32(self.blunder, &BLUNDER_RANGE, defaults.blunder);
        self.cutoff = clamp_f32(self.cutoff, &CUTOFF_RANGE, defaults.cutoff);
        self.opening_only = self
            .opening_only
            .map(|n| n.clamp(*OPENING_MOVES_RANGE.start(), *OPENING_MOVES_RANGE.end()));
        self
    }

    /// The default options with the time per move taken from the preference
    /// "autoplay delay" (D462), in milliseconds.
    #[must_use]
    pub fn with_delay_ms(delay_ms: u32) -> AnnotateOptions {
        AnnotateOptions {
            movetime: Duration::from_millis(u64::from(delay_ms)),
            ..AnnotateOptions::default()
        }
        .normalized()
    }
}

/// The thresholds that classify a score or a loss, in pawns (D474,
/// `informant(...)` of `start.tcl` 1324-1331).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Informant {
    /// `!?`: an interesting move (not used by the annotation).
    pub interesting: f32,
    /// `?`: a poor move, the loss above which a move gets `?`.
    pub poor: f32,
    /// `??`: a blunder, the loss above which a move gets `??`.
    pub blunder: f32,
    /// `?!`: a dubious move, the loss above which a move gets `?!`.
    pub dubious: f32,
    /// `+=`: a slight advantage.
    pub slight: f32,
    /// `+/-`: a moderate advantage.
    pub moderate: f32,
    /// `+-`: a decisive advantage.
    pub decisive: f32,
    /// `++-`: a won game.
    pub won: f32,
}

impl Default for Informant {
    fn default() -> Informant {
        Informant {
            interesting: 0.5,
            poor: 1.5,
            blunder: 3.0,
            dubious: 0.5,
            slight: 0.5,
            moderate: 1.5,
            decisive: 3.0,
            won: 5.5,
        }
    }
}

impl Informant {
    /// The thresholds with every value inside [`INFORMANT_RANGE`].
    #[must_use]
    pub fn normalized(self) -> Informant {
        let d = Informant::default();
        let f = |v, default| clamp_f32(v, &INFORMANT_RANGE, default);
        Informant {
            interesting: f(self.interesting, d.interesting),
            poor: f(self.poor, d.poor),
            blunder: f(self.blunder, d.blunder),
            dubious: f(self.dubious, d.dubious),
            slight: f(self.slight, d.slight),
            moderate: f(self.moderate, d.moderate),
            decisive: f(self.decisive, d.decisive),
            won: f(self.won, d.won),
        }
    }
}

fn clamp_f32(value: f32, range: &RangeInclusive<f32>, default: f32) -> f32 {
    if value.is_nan() {
        default
    } else {
        value.clamp(*range.start(), *range.end())
    }
}

/// Pawns to centipawns, rounded: the decision compares whole centipawns so
/// that a loss of exactly the threshold is not pushed over it by float noise
/// (`1.30 - 0.30` is not `1.00` in floating point).
pub(crate) fn centipawns(pawns: f32) -> i64 {
    // Clamped to the range of an engine's `cp` (an `i32`): a product of two
    // such values still fits an i64, so the comparisons of the decision never
    // overflow, even for the infinities and `f32::MAX` of a public caller.
    // NaN rounds to 0. The clamped, rounded value is whole and fits an i64.
    const LIMIT: f64 = 2_147_483_647.0;
    #[allow(clippy::cast_possible_truncation)]
    let cp = (f64::from(pawns) * 100.0).clamp(-LIMIT, LIMIT).round() as i64;
    cp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_scids_except_the_score_format() {
        let o = AnnotateOptions::default();
        assert_eq!(o.control, Control::Depth);
        assert_eq!(o.depth, 13);
        assert_eq!(o.movetime, Duration::from_secs(5));
        assert!((o.blunder - 1.0).abs() < f32::EPSILON);
        assert!((o.cutoff - 5.0).abs() < f32::EPSILON);
        assert_eq!(o.with_vars, WithVars::Blunders);
        assert_eq!(o.with_score, WithScore::AllMoves);
        assert!(o.missed_mates && o.var_at_end && o.add_tag);
        assert!(!o.variations && !o.mark_exercises);
        assert_eq!(o.score_format, ScoreFormat::Eval);
        let i = Informant::default();
        assert!((i.won - 5.5).abs() < f32::EPSILON);
        assert!((i.blunder - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn normalizing_clamps_every_number_and_survives_nan() {
        let o = AnnotateOptions {
            depth: 1,
            movetime: Duration::from_secs(10_000),
            blunder: f32::NAN,
            cutoff: 99.0,
            opening_only: Some(1),
            ..AnnotateOptions::default()
        }
        .normalized();
        assert_eq!(o.depth, 10);
        assert_eq!(o.movetime, Duration::from_secs(300));
        assert!((o.blunder - 1.0).abs() < f32::EPSILON);
        assert!((o.cutoff - 10.0).abs() < f32::EPSILON);
        assert_eq!(o.opening_only, Some(5));
        let i = Informant {
            won: -4.0,
            slight: f32::INFINITY,
            ..Informant::default()
        }
        .normalized();
        assert!(i.won.abs() < f32::EPSILON);
        assert!((i.slight - 9.9).abs() < 1e-6);
    }

    #[test]
    fn the_delay_preference_becomes_the_time_per_move() {
        let o = AnnotateOptions::with_delay_ms(2500);
        assert_eq!(o.movetime, Duration::from_millis(2500));
        assert_eq!(
            AnnotateOptions::with_delay_ms(1).movetime,
            Duration::from_millis(100)
        );
    }

    #[test]
    fn centipawns_round_away_float_noise() {
        assert_eq!(centipawns(1.3 - 0.3), 100);
        assert_eq!(centipawns(-0.006), -1);
        assert_eq!(centipawns(327.47), 32747);
    }
}
