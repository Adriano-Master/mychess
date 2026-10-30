//! The coach of the game against an engine (spec M8-1d, D2416-D2430): a
//! second, **auxiliary** engine that judges every move of the player and
//! says it in a short remark ("good move", "inaccuracy (-0.8)").
//!
//! This module is pure: no engine, no clock, no I/O. [`CoachTracker`] is the
//! state machine. The glue (`state::play_flow::coach`) turns its
//! [`CoachOutput::Search`] into engine commands and hands the answers back
//! with [`CoachTracker::search_done`]; every answer carries the token of the
//! search it belongs to, so a late one is dropped (D2422).
//!
//! The judgement of a move needs two searches of the position **before** it,
//! at the same depth (D2420): the first gives the best move and its score;
//! if the player played something else, the second restricts the engine to
//! the move played (`searchmoves`) to get its score. The loss is the
//! difference, from the point of view of the player.

use std::collections::VecDeque;

use mychess_core::{Color, Move, Position};
use mychess_engine::Score;

use crate::engines::EngineId;

/// Depth of each search of the coach (D2418).
pub const COACH_DEPTH_RANGE: (u32, u32) = (1, 30);
/// Longest a search of the coach may last, in seconds (D2418).
pub const COACH_SECONDS_RANGE: (u32, u32) = (1, 30);
/// The default depth.
pub const DEFAULT_COACH_DEPTH: u32 = 12;
/// The default time limit of a search, in seconds.
pub const DEFAULT_COACH_SECONDS: u32 = 3;

/// Loss, in centipawns, from which a move is an inaccuracy (`?!`; the
/// Informant of the Scid, 0.5).
pub const INACCURACY_CP: i32 = 50;
/// Loss from which a move is a mistake (`?`, 1.5).
pub const MISTAKE_CP: i32 = 150;
/// Loss from which a move is a blunder (`??`, 3.0).
pub const BLUNDER_CP: i32 = 300;
/// A score beyond this (either side) is a decided game: the coach does not
/// warn (the `++-` of the Informant, 5.5).
pub const DECIDED_CP: i32 = 550;
/// A score beyond this is a mate (`Score::as_centipawns`).
const MATE_CP: i32 = 30_000;
/// The two searches of a judgement must have reached depths this close;
/// otherwise the difference of their scores is horizon noise, and the move
/// goes unjudged (D2433).
pub const DEPTH_TOLERANCE: u32 = 1;
/// Moves waiting for the coach; the oldest one that has not started is
/// dropped when more arrive (D2422).
pub const QUEUE_LIMIT: usize = 4;

/// What the coach is asked to do (D2418).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoachConfig {
    /// The auxiliary engine (it must not be the opponent's, D2416).
    pub engine: EngineId,
    /// Depth of each search.
    pub depth: u32,
    /// Seconds a search may last at most.
    pub seconds: u32,
    /// Add the best move to the remark when the move was worse than good.
    pub hint: bool,
    /// Show the loss in pawns.
    pub numbers: bool,
}

impl CoachConfig {
    /// The defaults for `engine`.
    pub fn new(engine: EngineId) -> CoachConfig {
        CoachConfig {
            engine,
            depth: DEFAULT_COACH_DEPTH,
            seconds: DEFAULT_COACH_SECONDS,
            hint: false,
            numbers: true,
        }
    }

    /// The configuration with every number inside its range.
    #[must_use]
    pub fn normalized(mut self) -> CoachConfig {
        self.depth = self.depth.clamp(COACH_DEPTH_RANGE.0, COACH_DEPTH_RANGE.1);
        self.seconds = self
            .seconds
            .clamp(COACH_SECONDS_RANGE.0, COACH_SECONDS_RANGE.1);
        self
    }
}

/// How good a move of the player was (D2421).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum CoachGrade {
    /// The move the coach would have played.
    Best,
    /// Loses less than [`INACCURACY_CP`].
    Good,
    /// `?!`
    Inaccuracy,
    /// `?`
    Mistake,
    /// `??`
    Blunder,
}

impl CoachGrade {
    /// The grade of a loss, in centipawns, for a move that was not the
    /// coach's best.
    pub fn from_loss(loss: i32) -> CoachGrade {
        if loss >= BLUNDER_CP {
            CoachGrade::Blunder
        } else if loss >= MISTAKE_CP {
            CoachGrade::Mistake
        } else if loss >= INACCURACY_CP {
            CoachGrade::Inaccuracy
        } else {
            CoachGrade::Good
        }
    }

    /// Whether the grade deserves a warning (and so the hint).
    pub fn is_weak(self) -> bool {
        self >= CoachGrade::Inaccuracy
    }

    /// The Fluent key of the word that names the grade.
    pub fn message_key(self) -> &'static str {
        match self {
            CoachGrade::Best => "play-coach-best",
            CoachGrade::Good => "play-coach-good",
            CoachGrade::Inaccuracy => "play-coach-inaccuracy",
            CoachGrade::Mistake => "play-coach-mistake",
            CoachGrade::Blunder => "play-coach-blunder",
        }
    }
}

/// The remark about one move of the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoachVerdict {
    /// The ply of the move in the game, from 1.
    pub ply: u32,
    /// The move, in SAN.
    pub played_san: String,
    /// The best move in SAN, only when the configuration asks for hints and
    /// the grade is weak (D2425).
    pub best_san: Option<String>,
    /// The grade.
    pub grade: CoachGrade,
    /// What the move lost, in centipawns (0 for the best move).
    pub loss_cp: i32,
    /// A mate was involved: the loss means nothing in pawns and is not
    /// shown (D2421).
    pub decisive: bool,
}

/// What the front-end draws of the coach (D2425).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoachView {
    /// The name of the coach engine.
    pub engine: String,
    /// A move is being judged.
    pub busy: bool,
    /// The coach engine is gone; the game goes on without it (D2423).
    pub lost: bool,
    /// Show the loss in pawns.
    pub numbers: bool,
    /// The remark about the last move judged.
    pub last: Option<CoachVerdict>,
}

/// A search the glue must run on the coach engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoachSearch {
    /// Answer with [`CoachTracker::search_done`] using this token.
    pub token: u64,
    /// The position the game started from.
    pub start: Position,
    /// The moves played from `start` to the position to judge (the one
    /// **before** the move of the player).
    pub moves: Vec<Move>,
    /// Restrict the search to this move (`searchmoves`).
    pub only: Option<Move>,
}

/// What the owner of the tracker must do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CoachOutput {
    /// Run a search on the coach engine; it replaces any other.
    Search(CoachSearch),
    /// Stop the search that is running; its answer is dropped.
    Stop,
    /// A move was judged.
    Verdict(CoachVerdict),
}

#[derive(Debug, Clone, Copy)]
enum Stage {
    /// Waiting for the best move of the position.
    Best,
    /// Waiting for the score of the move played.
    Played {
        best: Move,
        best_cp: i32,
        best_depth: Option<u32>,
    },
}

#[derive(Debug, Clone)]
struct Job {
    start: Position,
    moves: Vec<Move>,
    before: Position,
    played: Move,
    ply: u32,
    stage: Stage,
}

/// The state machine of the coach (see the module docs).
#[derive(Debug, Clone)]
pub struct CoachTracker {
    config: CoachConfig,
    player: Color,
    ready: bool,
    queue: VecDeque<Job>,
    /// The token of the search in flight, for the job in front.
    running: Option<u64>,
    next_token: u64,
    last: Option<CoachVerdict>,
}

impl CoachTracker {
    /// A coach for the player of `player`, not ready yet: moves are queued
    /// until [`CoachTracker::set_ready`].
    pub fn new(config: CoachConfig, player: Color) -> CoachTracker {
        CoachTracker {
            config: config.normalized(),
            player,
            ready: false,
            queue: VecDeque::new(),
            running: None,
            next_token: 1,
            last: None,
        }
    }

    /// The configuration.
    pub fn config(&self) -> &CoachConfig {
        &self.config
    }

    /// The last remark.
    pub fn last(&self) -> Option<&CoachVerdict> {
        self.last.as_ref()
    }

    /// Whether a move is waiting for a verdict.
    pub fn busy(&self) -> bool {
        !self.queue.is_empty()
    }

    /// The token of the search in flight.
    pub fn running(&self) -> Option<u64> {
        self.running
    }

    /// The coach engine is ready (`true`) or gone (`false`: everything
    /// waiting is dropped).
    pub fn set_ready(&mut self, ready: bool) -> Vec<CoachOutput> {
        self.ready = ready;
        if ready {
            let mut out = Vec::new();
            self.pump(&mut out);
            out
        } else {
            self.cancel()
        }
    }

    /// The player made `played` in the position reached from `start` by
    /// `moves_before`; it is the `ply`-th ply of the game.
    pub fn player_moved(
        &mut self,
        start: &Position,
        moves_before: &[Move],
        played: Move,
        ply: u32,
    ) -> Vec<CoachOutput> {
        let mut out = Vec::new();
        let mut before = start.clone();
        for mv in moves_before {
            if before.play(*mv).is_err() {
                return out;
            }
        }
        if before.turn() != self.player || !before.is_legal(played) {
            return out;
        }
        if self.queue.len() >= QUEUE_LIMIT {
            // The one in front may be in flight: it is kept.
            let victim = usize::from(self.running.is_some());
            drop(self.queue.remove(victim));
        }
        self.queue.push_back(Job {
            start: start.clone(),
            moves: moves_before.to_vec(),
            before,
            played,
            ply,
            stage: Stage::Best,
        });
        self.pump(&mut out);
        out
    }

    /// The search `token` ended with `best` and `score` (White's point of
    /// view) reached at `depth`; `None` for a search that failed (silent
    /// engine, no move). A token that is not the one in flight is a late
    /// answer and is dropped. If the two searches of a judgement ended at
    /// depths more than [`DEPTH_TOLERANCE`] apart, the move goes unjudged
    /// (D2433); a depth the engine did not report is not compared.
    pub fn search_done(
        &mut self,
        token: u64,
        best: Option<Move>,
        score: Option<Score>,
        depth: Option<u32>,
    ) -> Vec<CoachOutput> {
        let mut out = Vec::new();
        if self.running != Some(token) {
            return out;
        }
        self.running = None;
        let Some(job) = self.queue.front_mut() else {
            return out;
        };
        let points = |s: Score| {
            let cp = s.as_centipawns();
            if self.player == Color::White {
                cp
            } else {
                cp.saturating_neg()
            }
        };
        match (job.stage, best, score) {
            (Stage::Best, Some(best), Some(score)) if job.before.is_legal(best) => {
                let best_cp = points(score);
                if best == job.played {
                    let verdict = self.verdict(&Judged {
                        best,
                        best_cp,
                        played_cp: best_cp,
                    });
                    self.finish(verdict, &mut out);
                } else {
                    job.stage = Stage::Played {
                        best,
                        best_cp,
                        best_depth: depth,
                    };
                    let only = Some(job.played);
                    self.issue(only, &mut out);
                }
            }
            (
                Stage::Played {
                    best,
                    best_cp,
                    best_depth,
                },
                _,
                Some(score),
            ) => {
                let apart = best_depth
                    .zip(depth)
                    .is_some_and(|(a, b)| a.abs_diff(b) > DEPTH_TOLERANCE);
                let verdict = if apart {
                    None
                } else {
                    self.verdict(&Judged {
                        best,
                        best_cp,
                        played_cp: points(score),
                    })
                };
                self.finish(verdict, &mut out);
            }
            _ => {
                // A failed search: the move goes unjudged (D2422).
                drop(self.queue.pop_front());
                self.pump(&mut out);
            }
        }
        out
    }

    /// Drops everything: the game ended, was left, or the coach engine is
    /// gone. A search in flight is stopped.
    pub fn cancel(&mut self) -> Vec<CoachOutput> {
        self.queue.clear();
        if self.running.take().is_some() {
            vec![CoachOutput::Stop]
        } else {
            Vec::new()
        }
    }

    // ---- internals ----

    /// Starts the search of the job in front, when idle and ready.
    fn pump(&mut self, out: &mut Vec<CoachOutput>) {
        if !self.ready || self.running.is_some() || self.queue.is_empty() {
            return;
        }
        self.issue(None, out);
    }

    fn issue(&mut self, only: Option<Move>, out: &mut Vec<CoachOutput>) {
        let Some(job) = self.queue.front() else {
            return;
        };
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1).max(1);
        self.running = Some(token);
        out.push(CoachOutput::Search(CoachSearch {
            token,
            start: job.start.clone(),
            moves: job.moves.clone(),
            only,
        }));
    }

    fn finish(&mut self, verdict: Option<CoachVerdict>, out: &mut Vec<CoachOutput>) {
        drop(self.queue.pop_front());
        if let Some(verdict) = verdict {
            self.last = Some(verdict.clone());
            out.push(CoachOutput::Verdict(verdict));
        }
        self.pump(out);
    }

    /// The verdict of the job in front (D2420-D2421).
    fn verdict(&self, judged: &Judged) -> Option<CoachVerdict> {
        let job = self.queue.front()?;
        let Judged {
            best,
            best_cp,
            played_cp,
        } = *judged;
        let loss = if best == job.played {
            0
        } else {
            best_cp.saturating_sub(played_cp).max(0)
        };
        let decided = (best_cp >= DECIDED_CP && played_cp >= DECIDED_CP)
            || (best_cp <= -DECIDED_CP && played_cp <= -DECIDED_CP);
        let grade = if best == job.played {
            CoachGrade::Best
        } else if decided {
            CoachGrade::Good
        } else {
            CoachGrade::from_loss(loss)
        };
        let decisive = best_cp.abs() >= MATE_CP || played_cp.abs() >= MATE_CP;
        let best_san = (self.config.hint && grade.is_weak())
            .then(|| job.before.san(best).ok())
            .flatten();
        Some(CoachVerdict {
            ply: job.ply,
            played_san: job.before.san(job.played).ok()?,
            best_san,
            grade,
            loss_cp: loss,
            decisive,
        })
    }
}

/// The two numbers a verdict is made of.
#[derive(Debug, Clone, Copy)]
struct Judged {
    best: Move,
    best_cp: i32,
    played_cp: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(hint: bool) -> CoachTracker {
        let mut config = CoachConfig::new(EngineId(2));
        config.hint = hint;
        let mut t = CoachTracker::new(config, Color::White);
        drop(t.set_ready(true));
        t
    }

    fn mv(text: &str) -> Move {
        Position::new().parse_uci(text).expect("uci")
    }

    fn only_search(out: &[CoachOutput]) -> CoachSearch {
        match out {
            [CoachOutput::Search(s)] => s.clone(),
            other => panic!("one search expected, got {other:?}"),
        }
    }

    #[test]
    fn the_best_move_is_judged_with_one_search() {
        let mut t = tracker(false);
        let out = t.player_moved(&Position::new(), &[], mv("e2e4"), 1);
        let s = only_search(&out);
        assert_eq!(s.only, None);
        let out = t.search_done(s.token, Some(mv("e2e4")), Some(Score::Cp(30)), None);
        assert!(matches!(
            out.as_slice(),
            [CoachOutput::Verdict(v)] if v.grade == CoachGrade::Best && v.loss_cp == 0
        ));
        assert!(!t.busy());
    }

    #[test]
    fn another_move_needs_a_search_restricted_to_it_and_loss_is_the_difference() {
        let mut t = tracker(true);
        let out = t.player_moved(&Position::new(), &[], mv("a2a3"), 1);
        let s = only_search(&out);
        let out = t.search_done(s.token, Some(mv("e2e4")), Some(Score::Cp(40)), None);
        let second = only_search(&out);
        assert_eq!(second.only, Some(mv("a2a3")));
        assert_ne!(second.token, s.token);
        let out = t.search_done(second.token, Some(mv("a2a3")), Some(Score::Cp(-45)), None);
        let [CoachOutput::Verdict(v)] = out.as_slice() else {
            panic!("a verdict: {out:?}");
        };
        assert_eq!(v.loss_cp, 85);
        assert_eq!(v.grade, CoachGrade::Inaccuracy);
        assert_eq!(v.best_san.as_deref(), Some("e4"));
        assert_eq!(v.played_san, "a3");
    }

    #[test]
    fn a_late_answer_is_dropped() {
        let mut t = tracker(false);
        let s = only_search(&t.player_moved(&Position::new(), &[], mv("e2e4"), 1));
        assert!(
            t.search_done(s.token + 7, Some(mv("e2e4")), Some(Score::Cp(0)), None)
                .is_empty()
        );
        assert_eq!(t.running(), Some(s.token));
    }

    #[test]
    fn grades_follow_the_informant_thresholds() {
        assert_eq!(CoachGrade::from_loss(49), CoachGrade::Good);
        assert_eq!(CoachGrade::from_loss(50), CoachGrade::Inaccuracy);
        assert_eq!(CoachGrade::from_loss(150), CoachGrade::Mistake);
        assert_eq!(CoachGrade::from_loss(300), CoachGrade::Blunder);
    }

    #[test]
    fn cancel_stops_the_search_in_flight() {
        let mut t = tracker(false);
        drop(t.player_moved(&Position::new(), &[], mv("e2e4"), 1));
        assert_eq!(t.cancel(), [CoachOutput::Stop]);
        assert!(!t.busy() && t.running().is_none());
        assert!(t.cancel().is_empty());
    }
}
