//! Preferences of the evaluation bar, the PV arrows and the MultiPV traffic
//! lights (spec M4-5, D455).
//!
//! They live in their own `[engine_view]` table (D820) so the `[analysis]`
//! table of the engines (D431) keeps its shape.

use serde::Serialize;

/// Smallest and largest bar scale, in pawns (Scid `evalScale`, 1 to 10).
pub const EVAL_SCALE_RANGE: (u8, u8) = (1, 10);
/// Smallest and largest number of moves of the sequence arrows (Scid
/// `boardSeqCount`; the Scid has no upper limit, 8 is the useful one).
pub const SEQUENCE_LEN_RANGE: (u8, u8) = (1, 8);

/// The evaluation bar next to the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EvalBarPrefs {
    /// Whether the bar is shown (Scid `gameInfo(showEval)`, off).
    pub show: bool,
    /// Pawns that fill the bar in the linear mapping, `1..=10` (`evalScale`).
    pub scale: u8,
    /// Logarithmic mapping instead of the linear one (`evalUseLog`).
    pub log: bool,
    /// Ticks along the bar (`evalTicks`).
    pub ticks: bool,
    /// The numeric label next to the bar (an extension, D454).
    pub label: bool,
}

impl Default for EvalBarPrefs {
    fn default() -> EvalBarPrefs {
        EvalBarPrefs {
            show: false,
            scale: 5,
            log: false,
            ticks: false,
            label: true,
        }
    }
}

impl EvalBarPrefs {
    /// The same preferences with the scale inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> EvalBarPrefs {
        self.scale = self.scale.clamp(EVAL_SCALE_RANGE.0, EVAL_SCALE_RANGE.1);
        self
    }
}

/// How the arrows of the engine are drawn (Scid `analysis(boardType)`; the
/// "Position" mode is not ported, D460).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[non_exhaustive]
pub enum ArrowMode {
    /// An arrow per MultiPV line, the first move of each ("Arrows").
    #[default]
    #[serde(rename = "arrows")]
    Arrows,
    /// The first moves of the main line, thin and numbered ("Sequential").
    #[serde(rename = "sequence")]
    Sequence,
}

impl ArrowMode {
    /// The other mode.
    #[must_use]
    pub fn next(self) -> ArrowMode {
        match self {
            ArrowMode::Arrows => ArrowMode::Sequence,
            ArrowMode::Sequence => ArrowMode::Arrows,
        }
    }

    /// Reads the spelling used in the preferences file.
    pub fn from_name(text: &str) -> Option<ArrowMode> {
        match text {
            "arrows" => Some(ArrowMode::Arrows),
            "sequence" => Some(ArrowMode::Sequence),
            _ => None,
        }
    }
}

/// Whose moves the sequence arrows show (Scid `boardSeqMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[non_exhaustive]
pub enum SequenceSide {
    /// Only the moves of White.
    #[serde(rename = "white")]
    White,
    /// Only the moves of Black.
    #[serde(rename = "black")]
    Black,
    /// Both sides.
    #[default]
    #[serde(rename = "both")]
    Both,
}

impl SequenceSide {
    /// The next submode (the mouse wheel of the Scid): both, white, black.
    #[must_use]
    pub fn next(self) -> SequenceSide {
        match self {
            SequenceSide::Both => SequenceSide::White,
            SequenceSide::White => SequenceSide::Black,
            SequenceSide::Black => SequenceSide::Both,
        }
    }

    /// Reads the spelling used in the preferences file.
    pub fn from_name(text: &str) -> Option<SequenceSide> {
        match text {
            "white" => Some(SequenceSide::White),
            "black" => Some(SequenceSide::Black),
            "both" => Some(SequenceSide::Both),
            _ => None,
        }
    }
}

/// The arrows of the engine on the main board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ArrowPrefs {
    /// Arrows are wanted; the engine that draws them is the one the user
    /// marked, or else the main engine (D821).
    pub enabled: bool,
    /// Which arrows.
    pub mode: ArrowMode,
    /// Number the secondary arrows (`numberPVArrows`).
    pub number_lines: bool,
    /// Moves of the sequence, `1..=8` (`boardSeqCount`).
    pub sequence_len: u8,
    /// Whose moves the sequence shows.
    pub sequence_side: SequenceSide,
    /// A click on an arrow plays its move (D457).
    pub click_to_play: bool,
}

impl Default for ArrowPrefs {
    fn default() -> ArrowPrefs {
        ArrowPrefs {
            enabled: false,
            mode: ArrowMode::Arrows,
            number_lines: true,
            sequence_len: 3,
            sequence_side: SequenceSide::Both,
            click_to_play: true,
        }
    }
}

impl ArrowPrefs {
    /// The same preferences with the length inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> ArrowPrefs {
        self.sequence_len = self
            .sequence_len
            .clamp(SEQUENCE_LEN_RANGE.0, SEQUENCE_LEN_RANGE.1);
        self
    }
}

/// The MultiPV traffic lights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct TrafficLightPrefs {
    /// Whether the strip is shown (Scid `enableMPVTls`, off).
    pub show: bool,
}

/// All the preferences of the slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct EngineViewPrefs {
    /// The evaluation bar.
    pub eval_bar: EvalBarPrefs,
    /// The arrows.
    pub arrows: ArrowPrefs,
    /// The traffic lights.
    pub traffic_lights: TrafficLightPrefs,
}

impl EngineViewPrefs {
    /// The same preferences with every number inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> EngineViewPrefs {
        self.eval_bar = self.eval_bar.sanitized();
        self.arrows = self.arrows.sanitized();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{PrefChange, Preferences, PrefsIssue, load_preferences, render_preferences};

    #[test]
    fn defaults_follow_the_scid() {
        let p = EngineViewPrefs::default();
        assert!(!p.eval_bar.show && !p.arrows.enabled && !p.traffic_lights.show);
        assert_eq!((p.eval_bar.scale, p.arrows.sequence_len), (5, 3));
        assert!(p.eval_bar.label && p.arrows.number_lines && p.arrows.click_to_play);
        assert_eq!(*Preferences::default().engine_view, p);
    }

    #[test]
    fn old_file_without_the_table_is_valid() {
        let loaded = load_preferences("[board]\nanimation_ms = 100\n");
        assert_eq!(*loaded.prefs.engine_view, EngineViewPrefs::default());
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn values_round_trip() {
        let prefs = Preferences {
            engine_view: Box::new(EngineViewPrefs {
                eval_bar: EvalBarPrefs {
                    show: true,
                    scale: 8,
                    log: true,
                    ticks: true,
                    label: false,
                },
                arrows: ArrowPrefs {
                    enabled: true,
                    mode: ArrowMode::Sequence,
                    number_lines: false,
                    sequence_len: 6,
                    sequence_side: SequenceSide::Black,
                    click_to_play: false,
                },
                traffic_lights: TrafficLightPrefs { show: true },
            }),
            ..Preferences::default()
        };
        let text = render_preferences(&prefs);
        assert!(text.contains("[engine_view.eval_bar]"), "{text}");
        let loaded = load_preferences(&text);
        assert_eq!(loaded.prefs.engine_view, prefs.engine_view);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    }

    #[test]
    fn bad_values_reset_or_clamp_one_by_one() {
        let loaded = load_preferences(
            "[engine_view.eval_bar]\nscale = 99\nlog = \"yes\"\nshow = true\n\
             [engine_view.arrows]\nmode = \"fancy\"\nsequence_len = 0\n\
             [engine_view]\ntraffic_lights = 3\n",
        );
        let v = *loaded.prefs.engine_view;
        assert_eq!(v.eval_bar.scale, 10);
        assert!(v.eval_bar.show && !v.eval_bar.log);
        assert_eq!(v.arrows.mode, ArrowMode::Arrows);
        assert_eq!(v.arrows.sequence_len, 1);
        assert!(loaded.issues.contains(&PrefsIssue::Clamped {
            key: "engine_view.eval_bar.scale".to_owned()
        }));
        assert!(loaded.issues.len() >= 4, "{:?}", loaded.issues);
    }

    #[test]
    fn changes_are_limited() {
        let mut prefs = Preferences::default();
        PrefChange::EvalBar(EvalBarPrefs {
            scale: 0,
            ..EvalBarPrefs::default()
        })
        .apply(&mut prefs);
        assert_eq!(prefs.engine_view.eval_bar.scale, 1);
        PrefChange::Arrows(ArrowPrefs {
            sequence_len: 200,
            ..ArrowPrefs::default()
        })
        .apply(&mut prefs);
        assert_eq!(prefs.engine_view.arrows.sequence_len, 8);
        PrefChange::TrafficLights(TrafficLightPrefs { show: true }).apply(&mut prefs);
        assert!(prefs.engine_view.traffic_lights.show);
    }

    #[test]
    fn cycles_visit_every_choice() {
        assert_eq!(ArrowMode::Arrows.next().next(), ArrowMode::Arrows);
        let s = SequenceSide::Both;
        assert_eq!(s.next().next().next(), s);
        assert_eq!(SequenceSide::from_name("black"), Some(SequenceSide::Black));
        assert_eq!(ArrowMode::from_name("x"), None);
    }
}
