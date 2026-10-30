//! The state machine of the analysis of the positions of an EPD file (spec
//! M7-5, regras 16-21, D3815-D3820; Scid `launchAnnotateEpd`, `epd.tcl`
//! 603-698).
//!
//! The run walks the records in file order, asks the engine for one search
//! per position (`go movetime`) and turns each answer into [`RunAction`]s on
//! the book. It knows the engine only through [`EngineCommand`] and
//! [`EngineEvent`], and the book only through the `&EpdBook` it is given at
//! each call, so it is tested with synthetic events. Searches carry a tag of
//! their own ([`TAG_BASE`]) so they never collide with those of the analysis
//! window or of the annotation of games.
//!
//! The run never edits the book: its outputs say what to send to the engine
//! and which actions to apply, in order.

use std::time::Duration;

use mychess_core::Move;
use mychess_engine::{
    BestMove, Bound, EngineCommand, EngineEvent, EngineWarning, Score, SearchId, SearchInfo,
    SearchLimits, SearchPosition, SearchRequest,
};
use mychess_pgn::epd::{EpdBook, EpdIndex, EpdOperation, EpdRecord};

use super::paste::{EngineLine, analysis_operations, engine_opcode};
use super::prefs::AnnotateMode;

/// The high bits of the labels of the searches of this run.
pub(crate) const TAG_BASE: u64 = 0x4E50_0000_0000_0000;

/// Whether `tag` is a label of a search of an EPD run.
pub(crate) fn is_epd_tag(tag: u64) -> bool {
    tag & 0xFFFF_0000_0000_0000 == TAG_BASE
}

/// Which positions the run analyses (D3819).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RunScope {
    /// Every record, from the first.
    #[default]
    All,
    /// From the selected record to the end of the file.
    FromSelected,
    /// The records that have no `ce` (to go on with an interrupted run).
    WithoutCe,
}

/// The choices of the "Analyze EPD" dialog (Scid `configAnnotateEpd`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotateConfig {
    /// The engine; `None` takes the first one that runs, or the one of F2.
    pub engine: Option<crate::engines::EngineId>,
    /// Seconds per position, 1-300 (out of range values are clamped).
    pub seconds: u32,
    /// What to do with each answer.
    pub mode: AnnotateMode,
    /// Which positions.
    pub scope: RunScope,
}

/// Why a run stopped on an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunError {
    /// The engine ended, refused the search or went silent.
    EngineFailed,
    /// The engine answered a search without any score.
    NoScore,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunOutcome {
    /// Every position of the scope was analysed.
    Finished,
    /// The user stopped it; what was written stays.
    Interrupted,
    /// It stopped on an error; what was written stays.
    Failed(RunError),
}

/// What the status bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunProgress {
    /// Positions analysed so far.
    pub done: u32,
    /// Positions in the scope.
    pub total: u32,
    /// Positions where the engine found the best move (`bm`) or avoided the
    /// bad one (`am`).
    pub found: u32,
    /// Positions that have a usable `bm` or `am`.
    pub counted: u32,
    /// Records whose `bm`/`am` has a move that is not legal in the position.
    pub illegal: u32,
}

/// An edit of the book that the owner applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunAction {
    /// Removes `acd acn ce dm pv` of the record and appends these.
    Annotate {
        index: EpdIndex,
        operations: Vec<EpdOperation>,
    },
    /// Replaces the operation of that opcode (the result of "Both").
    Result {
        index: EpdIndex,
        operation: EpdOperation,
    },
}

/// What the owner must do after a call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RunOutput {
    /// Send this to the engine.
    Send(Box<EngineCommand>),
    /// Apply these edits to the book, in order.
    Apply(Vec<RunAction>),
    /// The run is over.
    Done(RunOutcome),
}

/// The best line of the search in flight.
#[derive(Debug, Clone)]
struct Line {
    score: Score,
    exact: bool,
    depth: u32,
    nodes: u64,
    pv: Vec<Move>,
}

#[derive(Debug)]
struct InFlight {
    index: EpdIndex,
    tag: u64,
    search: Option<SearchId>,
    best: Option<Line>,
}

impl InFlight {
    fn absorb(&mut self, info: &SearchInfo) {
        if info.multipv != 1 {
            return;
        }
        let Some((score, bound)) = info.score else {
            return;
        };
        let depth = info.depth.unwrap_or(0);
        // A bound never replaces an exact line of the same or greater depth.
        if bound != Bound::Exact
            && self
                .best
                .as_ref()
                .is_some_and(|l| l.exact && l.depth >= depth)
        {
            return;
        }
        let previous = self.best.take();
        let pv = if info.pv.is_empty() {
            previous.as_ref().map(|l| l.pv.clone()).unwrap_or_default()
        } else {
            info.pv.clone()
        };
        let nodes = info
            .nodes
            .or_else(|| previous.as_ref().map(|l| l.nodes))
            .unwrap_or(0);
        self.best = Some(Line {
            score,
            exact: bound == Bound::Exact,
            depth: info
                .depth
                .unwrap_or_else(|| previous.map_or(0, |l| l.depth)),
            nodes,
            pv,
        });
    }
}

/// One run over one book.
#[derive(Debug)]
pub(crate) struct EpdRun {
    mode: AnnotateMode,
    movetime: Duration,
    opcode: String,
    scope: RunScope,
    selected: Option<EpdIndex>,
    cursor: Option<EpdIndex>,
    progress: RunProgress,
    inflight: Option<InFlight>,
    cancelling: bool,
    finished: bool,
    tag_seq: u64,
}

impl EpdRun {
    /// A run over the records of `scope`. `engine_name` names the result
    /// operation of the mode "Both"; `selected` is where `FromSelected`
    /// starts.
    pub(crate) fn new(
        book: &EpdBook,
        config: &AnnotateConfig,
        engine_name: &str,
        selected: Option<EpdIndex>,
    ) -> EpdRun {
        let seconds = config
            .seconds
            .clamp(*super::SECONDS_RANGE.start(), *super::SECONDS_RANGE.end());
        let total = match config.scope {
            RunScope::All => book.len(),
            RunScope::FromSelected => selected.map_or(book.len(), |s| {
                book.iter_indexed().filter(|(i, _)| *i >= s).count()
            }),
            RunScope::WithoutCe => book.iter().filter(|r| r.operation("ce").is_none()).count(),
        };
        EpdRun {
            mode: config.mode,
            movetime: Duration::from_secs(u64::from(seconds)),
            opcode: engine_opcode(engine_name),
            scope: config.scope,
            selected,
            cursor: None,
            progress: RunProgress {
                total: u32::try_from(total).unwrap_or(u32::MAX),
                ..RunProgress::default()
            },
            inflight: None,
            cancelling: false,
            finished: false,
            tag_seq: 0,
        }
    }

    /// The progress so far.
    pub(crate) fn progress(&self) -> RunProgress {
        self.progress
    }

    /// The record being analysed.
    pub(crate) fn current(&self) -> Option<EpdIndex> {
        self.inflight.as_ref().map(|f| f.index)
    }

    /// Whether the run is over.
    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    /// Whether the user asked to stop and the run waits for the engine.
    pub(crate) fn is_cancelling(&self) -> bool {
        self.cancelling
    }

    /// The identifier of the search in flight, once the engine labelled or
    /// started it.
    pub(crate) fn pending_search(&self) -> Option<SearchId> {
        self.inflight.as_ref().and_then(|f| f.search)
    }

    /// Whether a search was sent and the engine has not said it started.
    pub(crate) fn awaits_start(&self) -> bool {
        self.inflight.as_ref().is_some_and(|f| f.search.is_none())
    }

    /// Starts the run: sends the first search.
    pub(crate) fn start(&mut self, book: &EpdBook) -> Vec<RunOutput> {
        let mut out = Vec::new();
        self.advance(book, &mut out);
        out
    }

    /// The user stopped the run. A search that started is stopped and the
    /// run ends when its `bestmove` arrives; one that has not started
    /// cannot be waited for, so the run ends at once.
    pub(crate) fn cancel(&mut self) -> Vec<RunOutput> {
        if self.finished {
            return Vec::new();
        }
        self.cancelling = true;
        let mut out = Vec::new();
        match &self.inflight {
            Some(flight) if flight.search.is_some() => {
                out.push(RunOutput::Send(Box::new(EngineCommand::Stop)));
            }
            Some(_) => {
                out.push(RunOutput::Send(Box::new(EngineCommand::Stop)));
                self.end(&mut out, RunOutcome::Interrupted);
            }
            None => self.end(&mut out, RunOutcome::Interrupted),
        }
        out
    }

    /// The engine refused the search in flight (it never started).
    pub(crate) fn refused(&mut self) -> Vec<RunOutput> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        let outcome = if self.cancelling {
            RunOutcome::Interrupted
        } else {
            RunOutcome::Failed(RunError::EngineFailed)
        };
        self.end(&mut out, outcome);
        out
    }

    /// Gives the run an event of its engine. `None`: the event is not about
    /// a search of the run and must go where it would have gone.
    pub(crate) fn on_event(
        &mut self,
        book: &EpdBook,
        event: &EngineEvent,
    ) -> Option<Vec<RunOutput>> {
        let flight = self.inflight.as_mut()?;
        match event {
            EngineEvent::Warning(EngineWarning::CommandRefused {
                command: "go" | "position",
                ..
            }) if flight.search.is_none() => Some(self.refused()),
            EngineEvent::SearchTagged { search, tag } if *tag == flight.tag => {
                flight.search = Some(*search);
                Some(Vec::new())
            }
            // An engine whose label was lost: the search that starts next is
            // ours.
            EngineEvent::SearchStarted(id)
                if flight.search.is_none() || flight.search == Some(*id) =>
            {
                flight.search.get_or_insert(*id);
                Some(Vec::new())
            }
            EngineEvent::Info(info) if flight.search == Some(info.search) => {
                flight.absorb(info);
                Some(Vec::new())
            }
            EngineEvent::BestMove(best) if flight.search == Some(best.search) => {
                Some(self.search_done(book, best))
            }
            _ => None,
        }
    }

    // ---- the walk ----

    /// The next record of the scope after the cursor.
    fn next_target(&mut self, book: &EpdBook) -> Option<EpdIndex> {
        let mut candidate = if let Some(cursor) = self.cursor {
            let next = book.next(cursor)?;
            // The walk wrapped around: the file is over.
            if next <= cursor {
                return None;
            }
            next
        } else {
            let first = book.next(book.last_index()?)?;
            match (self.scope, self.selected) {
                (RunScope::FromSelected, Some(selected)) => {
                    if book.get_by_index(selected).is_some() {
                        selected
                    } else {
                        let next = book.next(selected)?;
                        if next < selected { first } else { next }
                    }
                }
                _ => first,
            }
        };
        loop {
            let record = book.get_by_index(candidate)?;
            if self.scope != RunScope::WithoutCe || record.operation("ce").is_none() {
                return Some(candidate);
            }
            let next = book.next(candidate)?;
            if next <= candidate {
                return None;
            }
            candidate = next;
        }
    }

    fn advance(&mut self, book: &EpdBook, out: &mut Vec<RunOutput>) {
        loop {
            if self.cancelling {
                self.end(out, RunOutcome::Interrupted);
                return;
            }
            let Some(index) = self.next_target(book) else {
                self.end(out, RunOutcome::Finished);
                return;
            };
            self.cursor = Some(index);
            let Some(record) = book.get_by_index(index) else {
                self.end(out, RunOutcome::Finished);
                return;
            };
            if record.position().legal_moves().is_empty() {
                // Checkmate or stalemate: no engine is asked.
                self.progress.done = self.progress.done.saturating_add(1);
                continue;
            }
            let tag = self.next_tag();
            let mut request =
                SearchRequest::infinite(SearchPosition::new(record.position().clone()));
            request.limits = SearchLimits {
                movetime: Some(self.movetime),
                ..SearchLimits::default()
            };
            request.tag = tag;
            out.push(RunOutput::Send(Box::new(EngineCommand::Search(request))));
            self.inflight = Some(InFlight {
                index,
                tag,
                search: None,
                best: None,
            });
            return;
        }
    }

    fn end(&mut self, out: &mut Vec<RunOutput>, outcome: RunOutcome) {
        self.finished = true;
        self.inflight = None;
        out.push(RunOutput::Done(outcome));
    }

    fn next_tag(&mut self) -> u64 {
        self.tag_seq = self.tag_seq.wrapping_add(1);
        TAG_BASE | (self.tag_seq & 0x0000_FFFF_FFFF_FFFF)
    }

    fn search_done(&mut self, book: &EpdBook, best: &BestMove) -> Vec<RunOutput> {
        let mut out = Vec::new();
        let Some(flight) = self.inflight.take() else {
            return out;
        };
        if self.cancelling {
            self.end(&mut out, RunOutcome::Interrupted);
            return out;
        }
        let Some(line) = flight.best else {
            self.end(&mut out, RunOutcome::Failed(RunError::NoScore));
            return out;
        };
        let Some(record) = book.get_by_index(flight.index) else {
            // The record went away meanwhile: nothing to write.
            self.progress.done = self.progress.done.saturating_add(1);
            self.advance(book, &mut out);
            return out;
        };
        let mut pv = line.pv;
        if pv.is_empty() {
            pv.extend(best.mv);
        }
        let first = pv.first().copied();
        let engine_line = EngineLine {
            score: line.score,
            depth: line.depth,
            nodes: line.nodes,
            pv,
        };
        let mut actions = Vec::new();
        if self.mode.annotates() {
            actions.push(RunAction::Annotate {
                index: flight.index,
                operations: analysis_operations(record.position(), &engine_line),
            });
        }
        if self.mode.counts() {
            let verdict = self.judge(record, first);
            if self.mode == AnnotateMode::Both {
                actions.push(RunAction::Result {
                    index: flight.index,
                    operation: EpdOperation::new(self.opcode.clone(), verdict.word()),
                });
            }
        }
        self.progress.done = self.progress.done.saturating_add(1);
        if !actions.is_empty() {
            out.push(RunOutput::Apply(actions));
        }
        self.advance(book, &mut out);
        out
    }

    /// Compares the first move of the engine with `bm` or `am` (rule 17).
    fn judge(&mut self, record: &EpdRecord, first: Option<Move>) -> Verdict {
        let (list, is_bm) = match record.bm() {
            Ok(list) if !list.is_empty() => (list, true),
            Ok(_) => match record.am() {
                Ok(list) if !list.is_empty() => (list, false),
                Ok(_) => return Verdict::NoResult,
                Err(_) => {
                    self.progress.illegal = self.progress.illegal.saturating_add(1);
                    return Verdict::NoResult;
                }
            },
            Err(_) => {
                self.progress.illegal = self.progress.illegal.saturating_add(1);
                return Verdict::NoResult;
            }
        };
        let Some(first) = first else {
            return Verdict::NoResult;
        };
        let listed = list.contains(&first);
        let success = if is_bm { listed } else { !listed };
        self.progress.counted = self.progress.counted.saturating_add(1);
        if success {
            self.progress.found = self.progress.found.saturating_add(1);
            Verdict::Success
        } else {
            Verdict::Fail
        }
    }
}

/// The result of one comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Success,
    Fail,
    NoResult,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Verdict::Success => "success",
            Verdict::Fail => "fail",
            Verdict::NoResult => "no-result",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::Position;

    fn pos(fen: &str) -> Position {
        Position::from_fen(fen).expect("fen")
    }

    fn mv(position: &Position, san: &str) -> Move {
        position
            .parse_san(san, mychess_core::SanMode::Tolerant)
            .expect("legal")
            .mv
    }

    /// Five distinct positions: the start, after e4, d4, c4, Nf3.
    fn five() -> (EpdBook, Vec<Position>) {
        let start = Position::new();
        let mut positions = vec![start.clone()];
        for san in ["e4", "d4", "c4", "Nf3"] {
            let m = mv(&start, san);
            positions.push(start.after(m).expect("after"));
        }
        let mut book = EpdBook::new();
        for p in &positions {
            book.insert(p.clone(), Vec::new());
        }
        (book, positions)
    }

    fn config(mode: AnnotateMode, scope: RunScope) -> AnnotateConfig {
        AnnotateConfig {
            engine: None,
            seconds: 2,
            mode,
            scope,
        }
    }

    /// Plays the engine's side: answers the search just sent with a score
    /// and the first legal move named by `san`.
    fn answer(
        run: &mut EpdRun,
        book: &EpdBook,
        sent: &EngineCommand,
        search: u64,
        cp: i32,
        san: &str,
    ) -> Vec<RunOutput> {
        let EngineCommand::Search(request) = sent else {
            panic!("not a search: {sent:?}");
        };
        let position = request.position.start.clone();
        let m = mv(&position, san);
        let id = SearchId(search);
        assert!(
            run.on_event(
                book,
                &EngineEvent::SearchTagged {
                    search: id,
                    tag: request.tag
                }
            )
            .is_some()
        );
        assert!(
            run.on_event(book, &EngineEvent::SearchStarted(id))
                .is_some()
        );
        let info = SearchInfo {
            search: id,
            multipv: 1,
            depth: Some(12),
            nodes: Some(5000),
            score: Some((Score::Cp(cp), Bound::Exact)),
            pv: vec![m],
            ..SearchInfo::default()
        };
        assert!(run.on_event(book, &EngineEvent::Info(info)).is_some());
        run.on_event(
            book,
            &EngineEvent::BestMove(BestMove {
                search: id,
                mv: Some(m),
                ponder: None,
            }),
        )
        .expect("ours")
    }

    fn sent(outputs: &[RunOutput]) -> Option<EngineCommand> {
        outputs.iter().find_map(|o| match o {
            RunOutput::Send(c) => Some((**c).clone()),
            _ => None,
        })
    }

    fn done(outputs: &[RunOutput]) -> Option<RunOutcome> {
        outputs.iter().find_map(|o| match o {
            RunOutput::Done(d) => Some(*d),
            _ => None,
        })
    }

    #[test]
    fn annotate_walks_the_five_positions_in_order() {
        let (book, positions) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        assert_eq!(run.progress().total, 5);
        let mut outputs = run.start(&book);
        let mut seen = Vec::new();
        let mut applied = 0;
        let mut n = 0;
        while let Some(command) = sent(&outputs) {
            let EngineCommand::Search(request) = &command else {
                panic!("search expected");
            };
            assert_eq!(request.limits.movetime, Some(Duration::from_secs(2)));
            assert!(is_epd_tag(request.tag));
            seen.push(request.position.start.clone());
            let legal = request.position.start.legal_moves()[0];
            let san = request.position.start.san(legal).expect("san");
            n += 1;
            outputs = answer(&mut run, &book, &command, n, 30, &san);
            applied += outputs
                .iter()
                .filter(|o| matches!(o, RunOutput::Apply(a) if matches!(a[0], RunAction::Annotate { .. })))
                .count();
        }
        assert_eq!(seen, positions);
        assert_eq!(applied, 5);
        assert_eq!(done(&outputs), Some(RunOutcome::Finished));
        assert_eq!(run.progress().done, 5);
    }

    #[test]
    fn annotate_actions_carry_the_five_operations() {
        let (book, _) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        let first = sent(&run.start(&book)).expect("search");
        let outputs = answer(&mut run, &book, &first, 1, 25, "e4");
        let Some(RunOutput::Apply(actions)) =
            outputs.iter().find(|o| matches!(o, RunOutput::Apply(_)))
        else {
            panic!("no apply");
        };
        let RunAction::Annotate { operations, .. } = &actions[0] else {
            panic!("annotate expected");
        };
        let text: Vec<String> = operations
            .iter()
            .map(|o| format!("{} {}", o.opcode, o.operands))
            .collect();
        assert_eq!(text, ["acd 12", "acn 5000", "ce 25", "dm 0", "pv e4"]);
    }

    #[test]
    fn count_compares_moves_not_text() {
        let start = Position::new();
        let mut book = EpdBook::new();
        // bm with decorations, am, none.
        book.insert(start.clone(), vec![EpdOperation::new("bm", "e4!")]);
        let p2 = start.after(mv(&start, "d4")).expect("after");
        book.insert(p2.clone(), vec![EpdOperation::new("am", "Nf6")]);
        let p3 = start.after(mv(&start, "c4")).expect("after");
        book.insert(p3, Vec::new());
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Count, RunScope::All),
            "SF",
            None,
        );
        let c1 = sent(&run.start(&book)).expect("search 1");
        let o1 = answer(&mut run, &book, &c1, 1, 0, "e4");
        let c2 = sent(&o1).expect("search 2");
        // The engine plays Nf6 where Nf6 must be avoided: fail.
        let o2 = answer(&mut run, &book, &c2, 2, 0, "Nf6");
        let c3 = sent(&o2).expect("search 3");
        let o3 = answer(&mut run, &book, &c3, 3, 0, "Nf6");
        assert_eq!(done(&o3), Some(RunOutcome::Finished));
        let progress = run.progress();
        assert_eq!((progress.found, progress.counted, progress.done), (1, 2, 3));
        // Count writes nothing.
        assert!(
            !o1.iter()
                .chain(&o2)
                .chain(&o3)
                .any(|o| matches!(o, RunOutput::Apply(_)))
        );
    }

    #[test]
    fn am_succeeds_when_the_engine_avoids_the_move() {
        let start = Position::new();
        let mut book = EpdBook::new();
        book.insert(start.clone(), vec![EpdOperation::new("am", "e4")]);
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Count, RunScope::All),
            "SF",
            None,
        );
        let c = sent(&run.start(&book)).expect("search");
        let out = answer(&mut run, &book, &c, 1, 0, "d4");
        assert_eq!(done(&out), Some(RunOutcome::Finished));
        assert_eq!((run.progress().found, run.progress().counted), (1, 1));
    }

    #[test]
    fn an_illegal_bm_is_no_result_and_counted_apart() {
        let start = Position::new();
        let mut book = EpdBook::new();
        book.insert(start.clone(), vec![EpdOperation::new("bm", "Qh5")]);
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Both, RunScope::All),
            "SF",
            None,
        );
        let c = sent(&run.start(&book)).expect("search");
        let out = answer(&mut run, &book, &c, 1, 0, "e4");
        let RunOutput::Apply(actions) = out
            .iter()
            .find(|o| matches!(o, RunOutput::Apply(_)))
            .expect("apply")
        else {
            panic!();
        };
        assert!(actions.iter().any(|a| matches!(a, RunAction::Result { operation, .. } if operation.operands == "no-result")));
        assert_eq!(run.progress().illegal, 1);
        assert_eq!(run.progress().counted, 0);
    }

    #[test]
    fn both_writes_the_engine_opcode_with_the_verdict() {
        let start = Position::new();
        let mut book = EpdBook::new();
        book.insert(start.clone(), vec![EpdOperation::new("bm", "e4")]);
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Both, RunScope::All),
            "Stockfish 17",
            None,
        );
        let c = sent(&run.start(&book)).expect("search");
        let out = answer(&mut run, &book, &c, 1, 10, "e4");
        let RunOutput::Apply(actions) = out
            .iter()
            .find(|o| matches!(o, RunOutput::Apply(_)))
            .expect("apply")
        else {
            panic!();
        };
        assert_eq!(actions.len(), 2);
        assert!(matches!(&actions[1], RunAction::Result { operation, .. }
            if operation.opcode == "Stockfish_17" && operation.operands == "success"));
    }

    #[test]
    fn stopping_in_the_middle_ends_interrupted_after_the_bestmove() {
        let (book, _) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        let c1 = sent(&run.start(&book)).expect("search");
        let o1 = answer(&mut run, &book, &c1, 1, 5, "e4");
        let c2 = sent(&o1).expect("search 2");
        let EngineCommand::Search(request) = &c2 else {
            panic!()
        };
        run.on_event(
            &book,
            &EngineEvent::SearchTagged {
                search: SearchId(2),
                tag: request.tag,
            },
        );
        let stop = run.cancel();
        assert_eq!(sent(&stop), Some(EngineCommand::Stop));
        assert_eq!(done(&stop), None, "waits for the bestmove");
        assert!(run.is_cancelling());
        let end = run
            .on_event(
                &book,
                &EngineEvent::BestMove(BestMove {
                    search: SearchId(2),
                    mv: None,
                    ponder: None,
                }),
            )
            .expect("ours");
        assert_eq!(done(&end), Some(RunOutcome::Interrupted));
        assert_eq!(run.progress().done, 1);
        assert!(run.is_finished());
    }

    #[test]
    fn a_search_that_never_started_is_cancelled_at_once() {
        let (book, _) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        run.start(&book);
        assert!(run.awaits_start());
        let out = run.cancel();
        assert_eq!(done(&out), Some(RunOutcome::Interrupted));
    }

    #[test]
    fn a_refused_search_fails_the_run() {
        let (book, _) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        run.start(&book);
        let out = run
            .on_event(
                &book,
                &EngineEvent::Warning(EngineWarning::CommandRefused {
                    command: "go",
                    reason: "x".into(),
                }),
            )
            .expect("ours");
        assert_eq!(done(&out), Some(RunOutcome::Failed(RunError::EngineFailed)));
    }

    #[test]
    fn a_bestmove_without_any_score_fails_the_run() {
        let (book, _) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        let c = sent(&run.start(&book)).expect("search");
        let EngineCommand::Search(request) = &c else {
            panic!()
        };
        run.on_event(
            &book,
            &EngineEvent::SearchTagged {
                search: SearchId(1),
                tag: request.tag,
            },
        );
        let out = run
            .on_event(
                &book,
                &EngineEvent::BestMove(BestMove {
                    search: SearchId(1),
                    mv: None,
                    ponder: None,
                }),
            )
            .expect("ours");
        assert_eq!(done(&out), Some(RunOutcome::Failed(RunError::NoScore)));
    }

    #[test]
    fn events_of_other_searches_are_not_taken() {
        let (book, _) = five();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        let c = sent(&run.start(&book)).expect("search");
        let EngineCommand::Search(request) = &c else {
            panic!()
        };
        run.on_event(
            &book,
            &EngineEvent::SearchTagged {
                search: SearchId(1),
                tag: request.tag,
            },
        );
        assert!(
            run.on_event(
                &book,
                &EngineEvent::Info(SearchInfo {
                    search: SearchId(9),
                    ..SearchInfo::default()
                })
            )
            .is_none()
        );
        assert!(
            run.on_event(
                &book,
                &EngineEvent::SearchTagged {
                    search: SearchId(9),
                    tag: 77
                }
            )
            .is_none()
        );
    }

    #[test]
    fn from_selected_starts_at_the_selection() {
        let (book, positions) = five();
        let third = book.index_of(&positions[2]).expect("index");
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::FromSelected),
            "SF",
            Some(third),
        );
        assert_eq!(run.progress().total, 3);
        let c = sent(&run.start(&book)).expect("search");
        let EngineCommand::Search(request) = &c else {
            panic!()
        };
        assert_eq!(request.position.start, positions[2]);
    }

    #[test]
    fn without_ce_skips_the_records_that_have_it() {
        let (mut book, positions) = five();
        for p in positions.iter().take(4) {
            if let Some(r) = book.get_mut(p) {
                r.set_operation("ce", "10");
            }
        }
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::WithoutCe),
            "SF",
            None,
        );
        assert_eq!(run.progress().total, 1);
        let c = sent(&run.start(&book)).expect("search");
        let EngineCommand::Search(request) = &c else {
            panic!()
        };
        assert_eq!(request.position.start, positions[4]);
        let out = answer(&mut run, &book, &c, 1, 3, "Nf6");
        assert_eq!(done(&out), Some(RunOutcome::Finished));
    }

    #[test]
    fn an_empty_book_finishes_at_once() {
        let book = EpdBook::new();
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        assert_eq!(done(&run.start(&book)), Some(RunOutcome::Finished));
    }

    #[test]
    fn terminal_positions_are_skipped_without_asking_the_engine() {
        let mate = pos("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1");
        let mut book = EpdBook::new();
        book.insert(mate, Vec::new());
        let mut run = EpdRun::new(
            &book,
            &config(AnnotateMode::Annotate, RunScope::All),
            "SF",
            None,
        );
        let out = run.start(&book);
        assert_eq!(sent(&out), None);
        assert_eq!(done(&out), Some(RunOutcome::Finished));
        assert_eq!(run.progress().done, 1);
    }
}
