//! The strength of the engine in a tactical game (spec M8-7, D2452-D2456):
//! from one Elo to what the engine can actually be told.
//!
//! Everything here is pure. The Scid's Phalanx took a single number, the
//! "easy level", computed from an Elo between 1400 and 2400
//! (`tacgame.tcl` `launchPhalanx`, lines 605-611); any UCI engine declares
//! its own options instead, so the plan depends on what the engine says it
//! has.

use mychess_engine::{EngineOption, OptionKind};

/// The Elo a tactical game accepts (D2456).
pub const TACTICAL_ELO_RANGE: (u32, u32) = (600, 3200);
/// The Elo of a first game (D2451).
pub const DEFAULT_TACTICAL_ELO: u32 = 1500;
/// The steps of the dynamic adjustment the dialog offers (D2456).
pub const ADJUST_STEPS: [u32; 3] = [25, 50, 100];
/// The step of the adjustment by default.
pub const DEFAULT_ADJUST_STEP: u32 = 50;

/// The Elo that `Skill Level` starts at (D2453).
pub const SKILL_ELO_FLOOR: u32 = 800;
/// The Elo that `Skill Level` tops out at (D2453).
pub const SKILL_ELO_CEIL: u32 = 2800;

/// The deepest search allowed for an Elo when the engine has no option of
/// strength (D2454): `(Elo up to, depth)`, in rising order.
pub const DEPTH_BY_ELO: [(u32, u32); 10] = [
    (800, 1),
    (1000, 2),
    (1200, 3),
    (1400, 4),
    (1600, 5),
    (1800, 6),
    (2000, 8),
    (2200, 10),
    (2400, 12),
    (2600, 14),
];
/// The depth for an Elo above the last row of [`DEPTH_BY_ELO`].
pub const MAX_DEPTH_CAP: u32 = 16;

/// What an engine says it can do about its strength (D2452).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineCaps {
    /// `UCI_LimitStrength` with the range of `UCI_Elo`. `None` unless the
    /// engine declares both.
    pub limit_elo: Option<(i64, i64)>,
    /// The range of `Skill Level`.
    pub skill: Option<(i64, i64)>,
}

impl EngineCaps {
    /// Reads the options an engine declared at its handshake.
    #[must_use]
    pub fn from_options(options: &[EngineOption]) -> EngineCaps {
        let spin = |name: &str| {
            options
                .iter()
                .find_map(|o| match (&o.kind, o.name.eq_ignore_ascii_case(name)) {
                    (OptionKind::Spin { min, max, .. }, true) if min <= max => Some((*min, *max)),
                    _ => None,
                })
        };
        let limits = options.iter().any(|o| {
            o.name.eq_ignore_ascii_case("UCI_LimitStrength")
                && matches!(o.kind, OptionKind::Check { .. })
        });
        EngineCaps {
            limit_elo: spin("UCI_Elo").filter(|_| limits),
            skill: spin("Skill Level"),
        }
    }
}

/// What the engine will be told, from a wanted Elo (D2452).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StrengthPlan {
    /// `UCI_LimitStrength` on, `UCI_Elo` = `elo`.
    Elo {
        /// The value sent, inside the range the engine declared.
        elo: i64,
        /// Whether the wanted Elo had to be moved into the range (D2455).
        clamped: bool,
    },
    /// `Skill Level` = `level` (approximate, D2453).
    Skill {
        /// The value sent, inside the range the engine declared.
        level: i64,
    },
    /// No option of strength: every search is cut at `depth` (D2454).
    Depth {
        /// The deepest search allowed.
        depth: u32,
    },
}

impl StrengthPlan {
    /// The text of the tag `TacticalLimit` (D2461).
    #[must_use]
    pub fn limit_tag(self) -> String {
        match self {
            StrengthPlan::Elo { elo, .. } => format!("UCI_Elo {elo}"),
            StrengthPlan::Skill { level } => format!("Skill Level {level}"),
            StrengthPlan::Depth { depth } => format!("Depth {depth}"),
        }
    }

    /// The Elo to write for the engine in the tags: the one it was told, or
    /// the wanted one when the plan is not an Elo.
    #[must_use]
    pub fn effective_elo(self, wanted: u32) -> u32 {
        match self {
            StrengthPlan::Elo { elo, .. } => u32::try_from(elo).unwrap_or(wanted),
            _ => wanted,
        }
    }

    /// The options of strength to put in `PlayConfig.strength` (D2452).
    #[must_use]
    pub fn as_strength(self) -> crate::play::Strength {
        match self {
            StrengthPlan::Elo { elo, .. } => crate::play::Strength {
                elo: Some(elo),
                skill: None,
            },
            StrengthPlan::Skill { level } => crate::play::Strength {
                elo: None,
                skill: Some(level),
            },
            StrengthPlan::Depth { .. } => crate::play::Strength::default(),
        }
    }

    /// The deepest search allowed, for the plan that has no option.
    #[must_use]
    pub fn depth_cap(self) -> Option<u32> {
        match self {
            StrengthPlan::Depth { depth } => Some(depth),
            _ => None,
        }
    }
}

/// `elo` inside [`TACTICAL_ELO_RANGE`].
#[must_use]
pub fn clamp_elo(elo: u32) -> u32 {
    elo.clamp(TACTICAL_ELO_RANGE.0, TACTICAL_ELO_RANGE.1)
}

/// The `Skill Level` for `elo` in the range an engine declared (D2453):
/// linear between [`SKILL_ELO_FLOOR`] and [`SKILL_ELO_CEIL`].
#[must_use]
pub fn skill_for_elo(elo: u32, (min, max): (i64, i64)) -> i64 {
    let span = i64::from(SKILL_ELO_CEIL - SKILL_ELO_FLOOR);
    let above = i64::from(elo.clamp(SKILL_ELO_FLOOR, SKILL_ELO_CEIL) - SKILL_ELO_FLOOR);
    let width = max.saturating_sub(min);
    // Rounded half up, in integers.
    let step = width
        .saturating_mul(above)
        .saturating_mul(2)
        .saturating_add(span)
        / span.saturating_mul(2);
    min.saturating_add(step).clamp(min, max)
}

/// The deepest search for `elo` when the engine has no option (D2454).
#[must_use]
pub fn depth_for_elo(elo: u32) -> u32 {
    DEPTH_BY_ELO
        .iter()
        .find(|(up_to, _)| elo <= *up_to)
        .map_or(MAX_DEPTH_CAP, |(_, depth)| *depth)
}

/// What to tell an engine with the abilities `caps` to play at about `elo`
/// (D2452). Deterministic.
#[must_use]
pub fn plan_strength(caps: EngineCaps, elo: u32) -> StrengthPlan {
    let elo = clamp_elo(elo);
    if let Some((min, max)) = caps.limit_elo {
        let wanted = i64::from(elo);
        let sent = wanted.clamp(min, max);
        return StrengthPlan::Elo {
            elo: sent,
            clamped: sent != wanted,
        };
    }
    if let Some(range) = caps.skill {
        return StrengthPlan::Skill {
            level: skill_for_elo(elo, range),
        };
    }
    StrengthPlan::Depth {
        depth: depth_for_elo(elo),
    }
}

/// How a game went, from the point of view of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The user won.
    Win,
    /// A draw.
    Draw,
    /// The user lost (resigning counts).
    Loss,
}

/// The Elo of the next game after `outcome` (D2456): `step` more for a
/// win, `step` less for a loss, the same for a draw, inside
/// [`TACTICAL_ELO_RANGE`].
#[must_use]
pub fn adjust_elo(elo: u32, outcome: Outcome, step: u32) -> u32 {
    let elo = clamp_elo(elo);
    clamp_elo(match outcome {
        Outcome::Win => elo.saturating_add(step),
        Outcome::Draw => elo,
        Outcome::Loss => elo.saturating_sub(step),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin(name: &str, min: i64, max: i64) -> EngineOption {
        EngineOption {
            name: name.to_owned(),
            kind: OptionKind::Spin {
                default: min,
                min,
                max,
            },
        }
    }

    fn check(name: &str) -> EngineOption {
        EngineOption {
            name: name.to_owned(),
            kind: OptionKind::Check { default: false },
        }
    }

    #[test]
    fn caps_need_both_limit_strength_and_elo() {
        let both = [check("UCI_LimitStrength"), spin("UCI_Elo", 1320, 3190)];
        assert_eq!(
            EngineCaps::from_options(&both).limit_elo,
            Some((1320, 3190))
        );
        assert_eq!(
            EngineCaps::from_options(&[spin("UCI_Elo", 1320, 3190)]).limit_elo,
            None
        );
        assert_eq!(
            EngineCaps::from_options(&[check("UCI_LimitStrength")]).limit_elo,
            None
        );
        assert_eq!(
            EngineCaps::from_options(&[spin("Skill Level", 0, 20)]).skill,
            Some((0, 20))
        );
        assert_eq!(EngineCaps::from_options(&[]), EngineCaps::default());
    }

    #[test]
    fn option_names_are_matched_ignoring_case() {
        // Engines differ in the spelling (`UCI_ELO`, `Skill level`).
        let caps = EngineCaps::from_options(&[
            check("uci_limitstrength"),
            spin("UCI_ELO", 1320, 3190),
            spin("skill level", 0, 20),
        ]);
        assert_eq!(caps.limit_elo, Some((1320, 3190)));
        assert_eq!(caps.skill, Some((0, 20)));
    }

    #[test]
    fn a_backwards_range_is_not_an_option() {
        let caps = EngineCaps::from_options(&[spin("Skill Level", 20, 0)]);
        assert_eq!(caps.skill, None);
    }

    #[test]
    fn elo_inside_the_range_is_sent_as_it_is() {
        let caps = EngineCaps {
            limit_elo: Some((1320, 3190)),
            skill: Some((0, 20)),
        };
        assert_eq!(
            plan_strength(caps, 1500),
            StrengthPlan::Elo {
                elo: 1500,
                clamped: false
            }
        );
    }

    #[test]
    fn elo_outside_the_range_is_moved_in_and_reported() {
        let caps = EngineCaps {
            limit_elo: Some((1320, 3190)),
            skill: None,
        };
        assert_eq!(
            plan_strength(caps, 900),
            StrengthPlan::Elo {
                elo: 1320,
                clamped: true
            }
        );
        assert_eq!(
            plan_strength(caps, 3200),
            StrengthPlan::Elo {
                elo: 3190,
                clamped: true
            }
        );
    }

    #[test]
    fn without_uci_elo_the_skill_level_is_used_and_it_is_monotonic() {
        let caps = EngineCaps {
            limit_elo: None,
            skill: Some((0, 20)),
        };
        let levels: Vec<i64> = (600..=3200)
            .step_by(100)
            .map(|elo| match plan_strength(caps, elo) {
                StrengthPlan::Skill { level } => level,
                other => panic!("not a skill plan: {other:?}"),
            })
            .collect();
        assert!(levels.windows(2).all(|w| w[0] <= w[1]), "{levels:?}");
        assert_eq!(levels.first(), Some(&0));
        assert_eq!(levels.last(), Some(&20));
    }

    #[test]
    fn skill_extremes_and_the_middle() {
        assert_eq!(skill_for_elo(800, (0, 20)), 0);
        assert_eq!(skill_for_elo(2800, (0, 20)), 20);
        assert_eq!(skill_for_elo(1800, (0, 20)), 10);
        assert_eq!(skill_for_elo(100, (0, 20)), 0);
        assert_eq!(skill_for_elo(9000, (5, 15)), 15);
        assert_eq!(skill_for_elo(1800, (7, 7)), 7);
    }

    #[test]
    fn with_no_option_the_depth_is_capped_and_it_is_monotonic() {
        let caps = EngineCaps::default();
        let depths: Vec<u32> = (600..=3200)
            .step_by(50)
            .map(|elo| match plan_strength(caps, elo) {
                StrengthPlan::Depth { depth } => depth,
                other => panic!("not a depth plan: {other:?}"),
            })
            .collect();
        assert!(depths.windows(2).all(|w| w[0] <= w[1]), "{depths:?}");
        assert_eq!(depths.first(), Some(&1));
        assert_eq!(depths.last(), Some(&MAX_DEPTH_CAP));
        assert_eq!(depth_for_elo(1500), 5);
    }

    #[test]
    fn the_order_of_choice_is_elo_then_skill_then_depth() {
        let all = EngineCaps {
            limit_elo: Some((1000, 3000)),
            skill: Some((0, 20)),
        };
        assert!(matches!(plan_strength(all, 1500), StrengthPlan::Elo { .. }));
        let skill = EngineCaps {
            limit_elo: None,
            ..all
        };
        assert!(matches!(
            plan_strength(skill, 1500),
            StrengthPlan::Skill { .. }
        ));
        assert!(matches!(
            plan_strength(EngineCaps::default(), 1500),
            StrengthPlan::Depth { .. }
        ));
    }

    #[test]
    fn the_wanted_elo_is_limited_to_the_tactical_range_first() {
        let caps = EngineCaps {
            limit_elo: Some((100, 9000)),
            skill: None,
        };
        assert_eq!(
            plan_strength(caps, 10),
            StrengthPlan::Elo {
                elo: 600,
                clamped: false
            }
        );
    }

    #[test]
    fn the_adjustment_follows_the_result_and_stays_in_range() {
        assert_eq!(adjust_elo(1500, Outcome::Win, 50), 1550);
        assert_eq!(adjust_elo(1500, Outcome::Loss, 50), 1450);
        assert_eq!(adjust_elo(1500, Outcome::Draw, 50), 1500);
        assert_eq!(adjust_elo(3190, Outcome::Win, 100), 3200);
        assert_eq!(adjust_elo(620, Outcome::Loss, 100), 600);
        assert_eq!(adjust_elo(u32::MAX, Outcome::Win, u32::MAX), 3200);
        assert_eq!(adjust_elo(0, Outcome::Loss, u32::MAX), 600);
    }

    #[test]
    fn tags_and_options_follow_the_plan() {
        let elo = StrengthPlan::Elo {
            elo: 1500,
            clamped: false,
        };
        assert_eq!(elo.limit_tag(), "UCI_Elo 1500");
        assert_eq!(elo.as_strength().elo, Some(1500));
        assert_eq!(elo.effective_elo(1400), 1500);
        let skill = StrengthPlan::Skill { level: 8 };
        assert_eq!(skill.limit_tag(), "Skill Level 8");
        assert_eq!(skill.as_strength().skill, Some(8));
        assert_eq!(skill.effective_elo(1400), 1400);
        let depth = StrengthPlan::Depth { depth: 5 };
        assert_eq!(depth.limit_tag(), "Depth 5");
        assert_eq!(depth.depth_cap(), Some(5));
        assert_eq!(depth.as_strength(), crate::play::Strength::default());
        assert_eq!(elo.depth_cap(), None);
    }
}
