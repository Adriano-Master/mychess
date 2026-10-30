//! The engine of the exercise (spec M8-6, D2307, D2308, D2310).
//!
//! The engine is started for the exercise and ended after each stage: it is
//! not one of the runs of the engine list, so the rule that stops those
//! during a training does not touch it. Two jobs: [`analyze_root`] (MultiPV
//! of the start position, while the user calculates) and [`analyze_lines`]
//! (the position after the first move of each line of the user and the one
//! at its end). Both obey a [`Cancel`] flag, never wait longer than their
//! budget and never panic, whatever the engine prints.

use std::collections::BTreeMap;
use std::time::Duration;

use mychess_core::{Move, Position};
use mychess_engine::{
    Bound, Engine, EngineCommand, EngineConfig, EngineEvent, EngineEvents, SearchInfo,
    SearchLimits, SearchPosition, SearchRequest,
};

use super::eval::{Eval, LineProbes, Probe, RootLine};
use crate::tactics::Cancel;

/// How often a running job looks at its cancel flag (D2310).
const CANCEL_POLL: Duration = Duration::from_millis(50);
/// Extra time on top of the searches for starting and ending the engine.
const OVERHEAD: Duration = Duration::from_secs(30);

/// The engine to start.
#[derive(Debug, Clone)]
pub struct CalvarEngine(pub EngineConfig);

impl PartialEq for CalvarEngine {
    fn eq(&self, other: &CalvarEngine) -> bool {
        self.0.command == other.0.command && self.0.args == other.0.args
    }
}

/// Why a job gave nothing (D2310).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EngineFailure {
    /// The engine did not start.
    NotStarted,
    /// The engine died, closed the channel or did not answer in time.
    Failed,
    /// The job was cancelled.
    Cancelled,
    /// The engine finished without a usable line.
    NoLines,
}

/// The first stage: the lines of the start position (D2308).
#[derive(Debug, Clone, PartialEq)]
pub struct RootJob {
    /// The engine.
    pub engine: CalvarEngine,
    /// The start position.
    pub position: Position,
    /// Lines to ask for.
    pub multipv: u8,
    /// How long to think.
    pub think: Duration,
    /// Raised when the exercise ends or the job is given up.
    pub cancel: Cancel,
}

impl RootJob {
    /// The longest the whole job may take.
    pub fn budget(&self) -> Duration {
        self.think + OVERHEAD
    }
}

/// The second stage: the lines of the user (D2308).
#[derive(Debug, Clone, PartialEq)]
pub struct LinesJob {
    /// The engine.
    pub engine: CalvarEngine,
    /// The start position.
    pub start: Position,
    /// The moves of each line.
    pub lines: Vec<Vec<Move>>,
    /// How long each search thinks.
    pub per_search: Duration,
    /// Raised when the exercise ends or the job is given up.
    pub cancel: Cancel,
}

impl LinesJob {
    /// Searches the job may run: two per line of more than one move.
    pub fn searches(&self) -> u32 {
        self.lines
            .iter()
            .map(|l| match l.len() {
                0 => 0,
                1 => 1,
                _ => 2,
            })
            .sum()
    }

    /// The longest the whole job may take.
    pub fn budget(&self) -> Duration {
        self.per_search
            .saturating_mul(self.searches().max(1))
            .saturating_add(OVERHEAD)
    }
}

async fn start_engine(
    config: &EngineConfig,
    cancel: &Cancel,
) -> Result<(Engine, EngineEvents), EngineFailure> {
    let start = Engine::start(config.clone());
    tokio::pin!(start);
    let started = loop {
        tokio::select! {
            result = &mut start => break result,
            () = tokio::time::sleep(CANCEL_POLL) => {
                if cancel.is_cancelled() {
                    return Err(EngineFailure::Cancelled);
                }
            }
        }
    };
    let (engine, events) = started.map_err(|_| EngineFailure::NotStarted)?;
    if cancel.is_cancelled() {
        engine.shutdown().await;
        return Err(EngineFailure::Cancelled);
    }
    Ok((engine, events))
}

/// Keeps the report of a MultiPV line (D2356). A bound ("at least", "at
/// most") is not the evaluation of the line: only an exact report counts, and
/// it replaces an exact one of the same depth or deeper. A report without
/// score or without variation is ignored.
fn record(lines: &mut BTreeMap<u8, RootLine>, info: SearchInfo) {
    let Some((score, Bound::Exact)) = info.score else {
        return;
    };
    if info.pv.is_empty() {
        return;
    }
    let number = info.multipv.max(1);
    let depth = info.depth.unwrap_or(0);
    if lines.get(&number).is_some_and(|old| old.depth > depth) {
        return;
    }
    lines.insert(
        number,
        RootLine {
            multipv: number,
            depth,
            eval: Eval::from_score(score),
            pv: info.pv,
        },
    );
}

/// Searches `position` for `think` and returns the lines of the last report
/// of each MultiPV number, best first. A report without score or without
/// variation is ignored.
async fn search(
    engine: &Engine,
    events: &mut EngineEvents,
    position: SearchPosition,
    think: Duration,
    cancel: &Cancel,
) -> Result<Vec<RootLine>, EngineFailure> {
    let request = SearchRequest {
        position,
        limits: SearchLimits {
            movetime: Some(think),
            ..SearchLimits::default()
        },
        search_moves: Vec::new(),
        ponder: false,
        tag: 0,
    };
    engine
        .send(EngineCommand::Search(request))
        .map_err(|_| EngineFailure::Failed)?;
    let mut lines: BTreeMap<u8, RootLine> = BTreeMap::new();
    loop {
        if cancel.is_cancelled() {
            engine.send(EngineCommand::Stop).ok();
            return Err(EngineFailure::Cancelled);
        }
        let event = match tokio::time::timeout(CANCEL_POLL, events.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => return Err(EngineFailure::Failed),
            Err(_) => continue,
        };
        match event {
            EngineEvent::Info(info) => record(&mut lines, info),
            EngineEvent::BestMove(_) => break,
            EngineEvent::Exited(_) => return Err(EngineFailure::Failed),
            _ => {}
        }
    }
    if lines.is_empty() {
        return Err(EngineFailure::NoLines);
    }
    Ok(lines.into_values().collect())
}

/// The position after `moves`, if they are all legal.
fn after_all(start: &Position, moves: &[Move]) -> Option<Position> {
    let mut pos = start.clone();
    for mv in moves {
        pos = pos.after(*mv).ok()?;
    }
    Some(pos)
}

/// A probe of the position after `moves`: the engine's best line there. A
/// position that ends the game needs no search; one the engine could not
/// search gives `None`.
async fn probe(
    engine: &Engine,
    events: &mut EngineEvents,
    start: &Position,
    moves: &[Move],
    think: Duration,
    cancel: &Cancel,
) -> Result<Option<Probe>, EngineFailure> {
    let Some(pos) = after_all(start, moves) else {
        return Ok(None);
    };
    if let Some(eval) = Eval::terminal(&pos) {
        return Ok(Some(Probe {
            eval,
            depth: 0,
            pv: Vec::new(),
        }));
    }
    let position = SearchPosition {
        start: start.clone(),
        moves: moves.to_vec(),
    };
    match search(engine, events, position, think, cancel).await {
        Ok(lines) => Ok(lines.into_iter().next().map(|l| Probe {
            eval: l.eval,
            depth: l.depth,
            pv: l.pv,
        })),
        Err(EngineFailure::NoLines) => Ok(None),
        Err(other) => Err(other),
    }
}

async fn root_inner(job: &RootJob) -> Result<Vec<RootLine>, EngineFailure> {
    let (engine, mut events) = start_engine(&job.engine.0, &job.cancel).await?;
    engine.send(EngineCommand::SetMultiPv(job.multipv)).ok();
    let result = search(
        &engine,
        &mut events,
        SearchPosition::new(job.position.clone()),
        job.think,
        &job.cancel,
    )
    .await;
    engine.shutdown().await;
    result
}

/// The first stage (D2308): the MultiPV lines of the start position.
pub async fn analyze_root(job: &RootJob) -> Result<Vec<RootLine>, EngineFailure> {
    tokio::time::timeout(job.budget(), root_inner(job))
        .await
        .unwrap_or(Err(EngineFailure::Failed))
}

async fn lines_inner(job: &LinesJob) -> Result<Vec<LineProbes>, EngineFailure> {
    let (engine, mut events) = start_engine(&job.engine.0, &job.cancel).await?;
    let mut out = Vec::with_capacity(job.lines.len());
    let mut failure = None;
    for line in &job.lines {
        let mut probes = LineProbes::default();
        let result: Result<(), EngineFailure> = async {
            if let Some(first) = line.get(..1) {
                probes.first = probe(
                    &engine,
                    &mut events,
                    &job.start,
                    first,
                    job.per_search,
                    &job.cancel,
                )
                .await?;
            }
            if line.len() > 1 {
                probes.end = probe(
                    &engine,
                    &mut events,
                    &job.start,
                    line,
                    job.per_search,
                    &job.cancel,
                )
                .await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            failure = Some(error);
            break;
        }
        out.push(probes);
    }
    engine.shutdown().await;
    match failure {
        Some(error) => Err(error),
        None => Ok(out),
    }
}

/// The second stage (D2308): one entry per line of the job, in order.
pub async fn analyze_lines(job: &LinesJob) -> Result<Vec<LineProbes>, EngineFailure> {
    tokio::time::timeout(job.budget(), lines_inner(job))
        .await
        .unwrap_or(Err(EngineFailure::Failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_engine_that_cannot_start_is_a_failure_not_a_panic() {
        let job = RootJob {
            engine: CalvarEngine(EngineConfig::new("/nonexistent/engine/for/the/test")),
            position: Position::new(),
            multipv: 3,
            think: Duration::from_millis(200),
            cancel: Cancel::new(),
        };
        assert_eq!(analyze_root(&job).await, Err(EngineFailure::NotStarted));
        let lines = LinesJob {
            engine: job.engine.clone(),
            start: Position::new(),
            lines: vec![vec![]],
            per_search: Duration::from_millis(200),
            cancel: Cancel::new(),
        };
        assert_eq!(analyze_lines(&lines).await, Err(EngineFailure::NotStarted));
    }

    fn info(multipv: u8, depth: u32, cp: i32, bound: Bound) -> SearchInfo {
        let start = Position::new();
        SearchInfo {
            multipv,
            depth: Some(depth),
            score: Some((mychess_engine::Score::Cp(cp), bound)),
            pv: vec![start.parse_uci("e2e4").expect("uci")],
            ..SearchInfo::default()
        }
    }

    #[test]
    fn a_bound_is_not_an_evaluation_and_an_exact_report_replaces_an_exact_one() {
        let mut lines = BTreeMap::new();
        record(&mut lines, info(1, 10, 30, Bound::Lower));
        record(&mut lines, info(1, 11, 400, Bound::Upper));
        assert!(lines.is_empty(), "bounds alone give nothing");
        record(&mut lines, info(1, 12, 30, Bound::Exact));
        record(&mut lines, info(1, 13, 999, Bound::Lower));
        record(&mut lines, info(1, 14, -999, Bound::Upper));
        assert_eq!(lines[&1].eval.cp, 30, "a bound never replaces an exact one");
        assert_eq!(lines[&1].depth, 12);
        record(&mut lines, info(1, 13, 35, Bound::Exact));
        assert_eq!(lines[&1].eval.cp, 35);
        record(&mut lines, info(1, 9, 0, Bound::Exact));
        assert_eq!(lines[&1].depth, 13, "an older depth does not come back");
    }

    #[test]
    fn a_report_without_score_or_variation_is_ignored() {
        let mut lines = BTreeMap::new();
        let mut no_score = info(1, 5, 0, Bound::Exact);
        no_score.score = None;
        let mut no_pv = info(2, 5, 0, Bound::Exact);
        no_pv.pv.clear();
        record(&mut lines, no_score);
        record(&mut lines, no_pv);
        assert!(lines.is_empty());
    }

    #[test]
    fn the_budget_counts_two_searches_for_a_long_line() {
        let start = Position::new();
        let one = start.parse_uci("e2e4").expect("uci");
        let job = LinesJob {
            engine: CalvarEngine(EngineConfig::new("x")),
            start,
            lines: vec![vec![one], vec![one, one]],
            per_search: Duration::from_secs(10),
            cancel: Cancel::new(),
        };
        assert_eq!(job.searches(), 3);
        assert_eq!(job.budget(), Duration::from_secs(60));
    }
}
