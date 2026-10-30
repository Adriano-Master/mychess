//! M8-7: the plan of the strength against the options that the real process
//! of `mychess-fake-engine` declares with each flag (spec D2452). Skips when
//! the binary is not built, except with `MYCHESS_REQUIRE_FAKE_ENGINE`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m4_3_common;

use mychess_app::tactical_game::{EngineCaps, StrengthPlan, plan_strength};
use mychess_engine::{EngineConfig, probe_uci};

async fn caps(flags: &[&str]) -> Option<EngineCaps> {
    let binary = m4_3_common::fake_binary()?;
    let config = EngineConfig::new(binary).with_args(flags.iter().copied());
    let identity = tokio::time::timeout(std::time::Duration::from_secs(30), probe_uci(config))
        .await
        .expect("the handshake ends")
        .expect("the fake answers");
    Some(EngineCaps::from_options(&identity.options))
}

#[tokio::test]
async fn the_default_fake_is_planned_with_uci_elo() {
    let Some(caps) = caps(&[]).await else {
        return;
    };
    assert_eq!(caps.limit_elo, Some((1320, 3190)));
    assert_eq!(
        plan_strength(caps, 1500),
        StrengthPlan::Elo {
            elo: 1500,
            clamped: false
        }
    );
    assert_eq!(
        plan_strength(caps, 1000),
        StrengthPlan::Elo {
            elo: 1320,
            clamped: true
        }
    );
}

#[tokio::test]
async fn the_fake_without_limit_strength_is_planned_with_skill_level() {
    let Some(caps) = caps(&["--no-limit-strength"]).await else {
        return;
    };
    assert_eq!(caps.limit_elo, None);
    assert!(matches!(
        plan_strength(caps, 1800),
        StrengthPlan::Skill { level: 10 }
    ));
}

#[tokio::test]
async fn the_fake_without_strength_options_gets_a_depth_cap() {
    let Some(caps) = caps(&["--no-strength"]).await else {
        return;
    };
    assert_eq!(caps, EngineCaps::default());
    assert_eq!(plan_strength(caps, 1500), StrengthPlan::Depth { depth: 5 });
}
