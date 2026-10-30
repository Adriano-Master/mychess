//! Shared helpers of the `m8_7_*` contract tests of the tactical game (spec
//! `docs/specs/m8-7-jogo-tatico.md`): a game against the in-memory engine of
//! `m8_1_common` with the options of strength the test wants, and a loop that
//! runs the effects that read files.
//!
//! The including test file declares `mod m8_1_common;` too.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use mychess_app::tactical_game::TacticalSetup;
use mychess_app::{
    ColorChoice, Effect, Message, PlayConfig, PlayMessage, StartFrom, TimeMode, perform, update,
};
use mychess_core::Position;
use mychess_engine::EngineOption;

use crate::m8_1_common::{Rig, button, check, spin};

pub const MATE_IN_ONE: &str = "6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - 0 1";
pub const STALEMATE_IN_ONE: &str = "k7/8/1K6/8/8/8/8/2Q5 w - - 0 1";

/// The options of Stockfish that matter here.
pub fn full_options() -> Vec<EngineOption> {
    vec![
        check("UCI_LimitStrength"),
        spin("UCI_Elo", 1320, 3190),
        spin("Skill Level", 0, 20),
        check("Ponder"),
        button("Clear Hash"),
    ]
}

pub fn skill_only() -> Vec<EngineOption> {
    vec![spin("Skill Level", 0, 20), button("Clear Hash")]
}

/// A tactical configuration with the settings of the preset.
pub fn tactical(rig: &Rig, color: ColorChoice, elo: u32) -> PlayConfig {
    let mut config = rig.config(color, TimeMode::MoveTime(3));
    config.tactical = Some(TacticalSetup {
        elo,
        adjust_step: None,
        plan: None,
    });
    config
}

pub fn with_adjust(mut config: PlayConfig, step: u32) -> PlayConfig {
    if let Some(setup) = config.tactical.as_mut() {
        setup.adjust_step = Some(step);
    }
    config
}

pub fn from_fen(mut config: PlayConfig, fen: &str) -> PlayConfig {
    config.start = StartFrom::Position(Box::new(Position::from_fen(fen).expect("fen")));
    config
}

pub fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write");
    path
}

/// A long ordinary game, for the drawing of a position in the middle.
pub const RUY_LOPEZ: &str = "[Event \"Ruy\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 \
10. d4 Nbd7 1-0\n";

/// A runtime and the loop that carries out the effects that read files and
/// feeds the answers back (the engine effects stay unanswered).
pub struct Runner {
    rt: tokio::runtime::Runtime,
}

impl Runner {
    pub fn new() -> Runner {
        Runner {
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        }
    }

    pub fn perform(&self, effect: Effect) -> Message {
        self.rt.block_on(perform(effect))
    }

    /// Runs the effects of file work in `effects`, and those their answers
    /// produce; returns everything else in order.
    pub fn settle(&self, rig: &mut Rig, effects: Vec<Effect>) -> Vec<Effect> {
        self.settle_state(&mut rig.state, effects)
    }

    /// The same, on an application state alone.
    pub fn settle_state(
        &self,
        state: &mut mychess_app::AppState,
        effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let mut rest = Vec::new();
        let mut queue: Vec<Effect> = effects;
        while let Some(effect) = queue.pop() {
            let ours = matches!(
                effect,
                Effect::OpenBase { .. }
                    | Effect::PlayPick { .. }
                    | Effect::Tactics(_)
                    | Effect::ScanPgn { .. }
                    | Effect::LoadBaseGame { .. }
                    | Effect::ParseCollectionGame { .. }
                    | Effect::ParseMovetext { .. }
            );
            if ours {
                let message = self.perform(effect);
                queue.extend(update(state, message));
            } else {
                rest.push(effect);
            }
        }
        rest
    }
}
