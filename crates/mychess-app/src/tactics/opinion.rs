//! The second opinion of an engine on a move of "Best move" (spec M8-4d,
//! D2130-D2139).
//!
//! In "Best move" the move of the game is the answer, and a different move
//! is wrong unless a mate as short as the game's is shown (D1913). With the
//! option on, a different move that is not wrong by that rule is put to an
//! engine: both moves are searched for a short time and the move of the user
//! is right if its evaluation is within [`OPINION_TOLERANCE_CP`] of the
//! move of the game. The engine is started for the question and ended
//! after it: it is not one of the runs of the engine list, so the rule that
//! stops the engines during a problem (D1959) does not touch it.

use std::time::Duration;

use mychess_core::{Color, Move, Position};
use mychess_engine::{
    Engine, EngineCommand, EngineConfig, EngineEvent, SearchLimits, SearchPosition, SearchRequest,
};

use super::source::Cancel;

/// How long each of the two positions is searched (D2132).
pub const OPINION_MS: u64 = 1_000;

/// How much worse than the move of the game the move of the user may be and
/// still be accepted, in centipawns (D2132).
pub const OPINION_TOLERANCE_CP: i32 = 30;

/// The whole question is given up after this long (D2133): the start of the
/// engine and two searches.
const OPINION_DEADLINE: Duration = Duration::from_secs(25);

/// How often a running question looks at its cancel flag (D2133).
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// A value that stands for "I was mated" or "I mate" in the centipawns of
/// the mover.
const MATE_CP: i32 = 100_000;

/// The engine that is asked: how to start it.
#[derive(Debug, Clone)]
pub struct OpinionEngine(pub EngineConfig);

impl PartialEq for OpinionEngine {
    fn eq(&self, other: &OpinionEngine) -> bool {
        self.0.command == other.0.command && self.0.args == other.0.args
    }
}

/// A question for the engine (D2131).
#[derive(Debug, Clone, PartialEq)]
pub struct OpinionJob {
    /// The engine to start.
    pub engine: OpinionEngine,
    /// The position of the problem, with the user to move.
    pub position: Position,
    /// The move of the user.
    pub user: Move,
    /// The move of the game.
    pub best: Move,
    /// Raised when the session ends or the question is given up.
    pub cancel: Cancel,
}

/// What the engine thinks of the move of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpinionVerdict {
    /// As good as the move of the game, within the tolerance.
    Equivalent,
    /// Clearly worse.
    Worse,
    /// The engine did not answer (it did not start, it failed, the time ran
    /// out or the question was cancelled).
    Unavailable,
}

/// The verdict for the evaluations (in centipawns, from the point of view of
/// the side that moved) of the move of the user and of the move of the game
/// (D2132).
pub fn decide(user_cp: i32, best_cp: i32) -> OpinionVerdict {
    if user_cp.saturating_add(OPINION_TOLERANCE_CP) >= best_cp {
        OpinionVerdict::Equivalent
    } else {
        OpinionVerdict::Worse
    }
}

/// The evaluation, for `mover`, of the position that follows `mv`; `None`
/// when the engine could not say.
async fn evaluate(
    engine: &Engine,
    events: &mut mychess_engine::EngineEvents,
    position: &Position,
    mv: Move,
    mover: Color,
    cancel: &Cancel,
) -> Option<i32> {
    let after = position.after(mv).ok()?;
    // A position that ends the game needs no engine.
    if after.is_checkmate() {
        return Some(MATE_CP);
    }
    if after.legal_moves().is_empty() {
        return Some(0);
    }
    let request = SearchRequest {
        position: SearchPosition::new(after),
        limits: SearchLimits {
            movetime: Some(Duration::from_millis(OPINION_MS)),
            ..SearchLimits::default()
        },
        search_moves: Vec::new(),
        ponder: false,
        tag: 0,
    };
    engine.send(EngineCommand::Search(request)).ok()?;
    let mut last = None;
    loop {
        if cancel.is_cancelled() {
            engine.send(EngineCommand::Stop).ok();
            return None;
        }
        let event = match tokio::time::timeout(CANCEL_POLL, events.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => return None,
            Err(_) => continue,
        };
        match event {
            EngineEvent::Info(info) if info.multipv <= 1 => {
                if let Some((score, _)) = info.score {
                    last = Some(score.as_centipawns());
                }
            }
            EngineEvent::BestMove(_) => break,
            EngineEvent::Exited(_) => return None,
            _ => {}
        }
    }
    // The score is from the point of view of White.
    let white = last?;
    Some(if mover == Color::White { white } else { -white })
}

async fn ask_inner(job: &OpinionJob) -> OpinionVerdict {
    // The start of the engine can take a while: the cancel flag is looked
    // at while it starts (D2167), and the engine of a cancelled question is
    // ended at once.
    let start = Engine::start(job.engine.0.clone());
    tokio::pin!(start);
    let started = loop {
        tokio::select! {
            result = &mut start => break result,
            () = tokio::time::sleep(CANCEL_POLL) => {
                if job.cancel.is_cancelled() {
                    return OpinionVerdict::Unavailable;
                }
            }
        }
    };
    let Ok((engine, mut events)) = started else {
        return OpinionVerdict::Unavailable;
    };
    if job.cancel.is_cancelled() {
        engine.shutdown().await;
        return OpinionVerdict::Unavailable;
    }
    let mover = job.position.turn();
    let user = evaluate(
        &engine,
        &mut events,
        &job.position,
        job.user,
        mover,
        &job.cancel,
    )
    .await;
    let best = match user {
        Some(_) => {
            evaluate(
                &engine,
                &mut events,
                &job.position,
                job.best,
                mover,
                &job.cancel,
            )
            .await
        }
        None => None,
    };
    engine.shutdown().await;
    match (user, best) {
        (Some(user), Some(best)) => decide(user, best),
        _ => OpinionVerdict::Unavailable,
    }
}

/// Asks the engine (D2131): starts it, searches both moves and ends it.
/// Never panics and never waits longer than `OPINION_DEADLINE`.
pub async fn ask(job: &OpinionJob) -> OpinionVerdict {
    tokio::time::timeout(OPINION_DEADLINE, ask_inner(job))
        .await
        .unwrap_or(OpinionVerdict::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_within_the_tolerance_is_equivalent_and_a_clearly_worse_one_is_not() {
        assert_eq!(decide(50, 50), OpinionVerdict::Equivalent);
        assert_eq!(
            decide(50 - OPINION_TOLERANCE_CP, 50),
            OpinionVerdict::Equivalent
        );
        assert_eq!(decide(10, 50), OpinionVerdict::Worse);
        assert_eq!(decide(-300, 20), OpinionVerdict::Worse);
        assert_eq!(decide(MATE_CP, 900), OpinionVerdict::Equivalent);
        assert_eq!(decide(i32::MIN, 0), OpinionVerdict::Worse);
    }

    #[tokio::test]
    async fn an_engine_that_cannot_start_is_unavailable_not_an_error() {
        let position = Position::new();
        let user = position.parse_uci("e2e4").expect("uci");
        let best = position.parse_uci("d2d4").expect("uci");
        let job = OpinionJob {
            engine: OpinionEngine(EngineConfig::new("/nonexistent/engine/for/the/test")),
            position,
            user,
            best,
            cancel: Cancel::new(),
        };
        assert_eq!(ask(&job).await, OpinionVerdict::Unavailable);
    }
}
