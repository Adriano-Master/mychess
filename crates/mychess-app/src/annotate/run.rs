//! The state machine of one annotation run (spec M4-6, D461-D465, D470).
//!
//! The run walks the moves of the game, asks the engine for one search per
//! position (`go depth` or `go movetime`, D462) and turns the answers into
//! [`Action`]s. It knows the engine only through [`EngineCommand`] and
//! [`EngineEvent`], and the game only through the `&Game` it is given at each
//! call, so it is tested with synthetic events.
//!
//! The run never edits the game: its outputs ([`RunOutput`]) say what to
//! send to the engine and which actions to apply, in order. Node ids stay
//! valid because the list of moves to annotate is fixed at the start and the
//! annotation only adds nodes (D464, rule 18).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use mychess_core::{Game, Move, Nag, NodeId, Position};
use mychess_engine::{
    BestMove, Bound, EngineCommand, EngineEvent, EngineWarning, OptionValue, Score, SearchId,
    SearchInfo, SearchLimits, SearchPosition, SearchRequest,
};
use thiserror::Error;

use super::book::{BookProbe, last_book_move_comment, out_of_book_comment};
use super::decision::{Action, Step, decide};
use super::exercise::{DIFFICULTY_TIMES_MS, OBVIOUS_DEPTHS, difficulty, exercise_move, is_obvious};
use super::options::{AnnotateOptions, Control, Informant, WithVars};
use super::score::{PositionEval, signed_pawns};
use crate::analysis::{NumberStyle, format_pv};

/// The high bits of the labels of the searches of the annotation, so they
/// never collide with those of the analysis window.
const TAG_BASE: u64 = 0x4E4E_0000_0000_0000;

/// The name of the option that empties the hash tables.
pub(crate) const CLEAR_HASH: &str = "Clear Hash";

/// Whether `tag` is a label of a search of an annotation run (never one of
/// the analysis window).
pub(crate) fn is_annotation_tag(tag: u64) -> bool {
    tag & 0xFFFF_0000_0000_0000 == TAG_BASE
}

/// Why a run could not start or stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AnnotationError {
    /// There is no move to annotate from the cursor.
    #[error("nothing to annotate")]
    NothingToAnnotate,
    /// The engine ended or failed during the run.
    #[error("the engine failed during the annotation")]
    EngineFailed,
    /// The engine answered a search without any score.
    #[error("the engine gave no score")]
    NoScore,
    /// A node of the plan is not in the game any more.
    #[error("the game changed during the annotation")]
    StaleGame,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Every move was annotated.
    Finished,
    /// The user cancelled; the edits stay.
    Cancelled,
    /// The game was replaced or altered by someone else.
    GameChanged,
    /// The run stopped on an error; the edits stay.
    Failed(AnnotationError),
}

/// What the status bar shows (rule 20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationProgress {
    /// Moves annotated so far.
    pub done_plies: u32,
    /// Moves to annotate.
    pub total_plies: u32,
    /// The game being annotated, from 1 (always 1 until the batch).
    pub game_index: u32,
    /// The number of games (always 1 until the batch).
    pub game_total: u32,
}

/// What the owner must do after a call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RunOutput {
    /// Send this to the engine.
    Send(Box<EngineCommand>),
    /// Apply these actions to the game, in order, as one edit.
    Apply(Vec<Action>),
    /// The run is over.
    Done(Outcome),
}

/// What the run needs to know about the engine and the choices.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunSetup {
    /// The options, already restricted to what the engine can do.
    pub(crate) options: AnnotateOptions,
    /// The thresholds.
    pub(crate) informant: Informant,
    /// The position that is evaluated first (the cursor, or the start).
    pub(crate) base: NodeId,
    /// Mark exercises: the engine searches with several lines and is
    /// probed for the difficulty.
    pub(crate) exercises: bool,
    /// The engine has a `Clear Hash` button.
    pub(crate) clear_hash: bool,
    /// The number of lines the run searches with. The probes of the
    /// exercises use one line and put this back after them (D893).
    pub(crate) lines: u8,
    /// Only collect the evaluation of every position of the line from
    /// `base`, never decide or edit anything (the game report, M4-10b).
    pub(crate) collect: bool,
}

/// The best line of a search so far.
#[derive(Debug, Clone)]
struct Line {
    score: Score,
    bound: Bound,
    pv: Vec<Move>,
    depth: u32,
}

/// The probes of a possible exercise (spec M4-6, D470, D893).
///
/// The Scid runs them in its internal analyzer (`sc_pos analyze -hashkb 1`,
/// `analysis.tcl` 1359-1379): one line and a hash of 1 KB, so nothing
/// carries from one probe to the next. Here the engine of the run answers
/// them, so each probe is a one-line search (`SetMultiPv(1)` before the first
/// probe, the run's lines back after the last) preceded by `Clear Hash` when
/// the engine has the button. Deviation: that **empties the hash table** of
/// the user's engine, which loses what it had learned for the position
/// shown; it is the price of a fair "time to find the move" test.
#[derive(Debug)]
struct Probe {
    node: NodeId,
    actions: Vec<Action>,
    best: Move,
    prev: f32,
    cur: f32,
    /// The answers so far: [`OBVIOUS_DEPTHS`] first, then
    /// [`DIFFICULTY_TIMES_MS`].
    answers: Vec<Option<Move>>,
}

#[derive(Debug)]
enum Purpose {
    Position(NodeId),
    Probe(Probe),
}

#[derive(Debug)]
struct InFlight {
    tag: u64,
    search: Option<SearchId>,
    purpose: Purpose,
    best: Option<Line>,
    second: Option<(Score, Bound)>,
}

impl InFlight {
    /// Keeps the report if it is better than what there is: an exact value
    /// always replaces, a bound only while nothing exact was seen.
    fn absorb(&mut self, info: &SearchInfo) {
        let Some((score, bound)) = info.score else {
            return;
        };
        match info.multipv {
            1 => {
                let keep = bound == Bound::Exact
                    || !self.best.as_ref().is_some_and(|l| l.bound == Bound::Exact);
                if keep {
                    let pv = if info.pv.is_empty() {
                        self.best.as_ref().map(|l| l.pv.clone()).unwrap_or_default()
                    } else {
                        info.pv.clone()
                    };
                    self.best = Some(Line {
                        score,
                        bound,
                        pv,
                        depth: info.depth.unwrap_or(0),
                    });
                }
            }
            2 => {
                let keep =
                    bound == Bound::Exact || !self.second.is_some_and(|(_, b)| b == Bound::Exact);
                if keep {
                    self.second = Some((score, bound));
                }
            }
            _ => {}
        }
    }
}

/// One run over one game.
#[derive(Debug)]
pub(crate) struct AnnotationRun {
    options: AnnotateOptions,
    informant: Informant,
    exercises: bool,
    clear_hash: bool,
    lines: u8,
    /// The engine is at one line for the probes (D893).
    lowered: bool,
    /// The nodes to evaluate: the base first, then the moves to annotate in
    /// depth-first pre-order (D464).
    order: Vec<NodeId>,
    next: usize,
    evals: HashMap<NodeId, PositionEval>,
    inflight: Option<InFlight>,
    cancelling: bool,
    finished: bool,
    tag_seq: u64,
    prev_nag: Option<Nag>,
    done: u32,
    non_standard_start: bool,
    zero_moves: bool,
    /// Report mode: the evaluations in walk order, no decisions.
    collect: bool,
    collected: Vec<PositionEval>,
    /// The opening book plan, if the run skips the moves of a book
    /// (spec M5-6, D1660).
    book: Option<BookPlan>,
}

/// What the run does with an opening book (spec M5-6, D1660-D1663; Scid
/// `bookAnnotation`, `analysis.tcl` 1243-1304).
///
/// The Scid walks the main line while the position still has book moves
/// and starts the analysis one move before the first position that has
/// none. Here the nodes that are not searched are `skip`; the move that
/// leaves the book is `out`, and it gets `comment` and no score once.
#[derive(Debug, Default)]
struct BookPlan {
    /// The nodes whose position is not searched: they are in the book and
    /// their children are too (or they hang from such a node).
    skip: HashSet<NodeId>,
    /// The variations that hang from a skipped node and all their moves:
    /// there is no evaluation of their parent to compare with.
    dead: HashSet<NodeId>,
    /// The first move out of the book (or the last of it at the end of the
    /// game).
    out: Option<NodeId>,
    /// The comment of `out`, in English like every comment of the run.
    comment: Option<String>,
}

/// The moves to annotate from `base`: depth-first pre-order, main
/// continuation first. Without `variations` only the main continuation of
/// `base` is followed. With an opening limit a move is kept while the
/// position before it is at or before that move number (D473).
pub(crate) fn plan(game: &Game, base: NodeId, options: &AnnotateOptions) -> Vec<NodeId> {
    let mut out = Vec::new();
    let Ok(children) = game.children(base) else {
        return out;
    };
    let mut stack: Vec<NodeId> = if options.variations {
        children.iter().rev().copied().collect()
    } else {
        children.first().copied().into_iter().collect()
    };
    while let Some(node) = stack.pop() {
        if let Some(limit) = options.opening_only {
            let before = game
                .parent(node)
                .ok()
                .flatten()
                .and_then(|p| game.position_at(p).ok())
                .map_or(0, Position::fullmove_number);
            if before > u32::from(limit) {
                continue;
            }
        }
        out.push(node);
        let Ok(children) = game.children(node) else {
            continue;
        };
        if options.variations {
            stack.extend(children.iter().rev().copied());
        } else if let Some(first) = children.first() {
            stack.push(*first);
        }
    }
    out
}

impl AnnotationRun {
    /// A run for `setup` over `game`.
    ///
    /// # Errors
    ///
    /// [`AnnotationError::NothingToAnnotate`] when there is no move from the
    /// base (a game without moves is annotated: its start position gets the
    /// initial comment, rule 11).
    pub(crate) fn new(game: &Game, setup: &RunSetup) -> Result<AnnotationRun, AnnotationError> {
        let moves = plan(game, setup.base, &setup.options);
        let zero_moves = !setup.collect
            && setup.base == game.root()
            && game.children(game.root()).is_ok_and(<[NodeId]>::is_empty);
        if moves.is_empty() && !zero_moves {
            return Err(AnnotationError::NothingToAnnotate);
        }
        let mut order = Vec::with_capacity(moves.len() + 1);
        order.push(setup.base);
        order.extend(moves);
        Ok(AnnotationRun {
            options: setup.options,
            informant: setup.informant,
            exercises: setup.exercises,
            clear_hash: setup.clear_hash,
            lines: setup.lines,
            lowered: false,
            order,
            next: 0,
            evals: HashMap::new(),
            inflight: None,
            cancelling: false,
            finished: false,
            tag_seq: 0,
            prev_nag: None,
            done: 0,
            non_standard_start: game.start_position() != &Position::new(),
            zero_moves,
            collect: setup.collect,
            collected: Vec::new(),
            book: None,
        })
    }

    /// The same run that does not search the positions of `book` (D1660).
    ///
    /// Only a run that starts at the beginning of the game and decides
    /// (never the report) uses it. The main line is walked while the
    /// position has book moves; the position before the first one without
    /// is searched as the "before" of the move that left the book, which
    /// gets the comment `Move out of book (name: moves)` or `Last book move
    /// (name)` and no score (rule 9).
    #[must_use]
    pub(crate) fn with_book(mut self, game: &Game, book: &dyn BookProbe) -> AnnotationRun {
        if self.collect || self.order.first() != Some(&game.root()) {
            return self;
        }
        // The positions of the main line: the root, then one per move.
        let mut line = vec![game.root()];
        line.extend(game.mainline());
        let mut k = 0;
        while k + 1 < line.len() {
            let Ok(position) = game.position_at(line[k]) else {
                return self;
            };
            if book.moves(position).is_empty() {
                break;
            }
            k += 1;
        }
        // `line[k]` is the first position without book moves, or the end.
        if k == 0 {
            return self;
        }
        let mut plan = BookPlan {
            skip: line[..k - 1].iter().copied().collect(),
            ..BookPlan::default()
        };
        if let (Ok(before), Ok(played)) = (game.position_at(line[k - 1]), game.mv(line[k])) {
            let moves = book.moves(before);
            plan.comment = Some(if moves.contains(&played) {
                last_book_move_comment(book.name())
            } else {
                let sans: Vec<String> = moves.iter().filter_map(|m| before.san(*m).ok()).collect();
                out_of_book_comment(book.name(), &sans)
            });
            plan.out = Some(line[k]);
        }
        self.book = Some(plan);
        self
    }

    /// Whether the book makes the run skip `node` without a search. A
    /// variation that hangs from a skipped node has nothing to compare with,
    /// so it is skipped too, with everything below it.
    fn skipped_by_book(&mut self, game: &Game, node: NodeId) -> bool {
        let Some(plan) = &mut self.book else {
            return false;
        };
        if plan.skip.contains(&node) || plan.dead.contains(&node) {
            return true;
        }
        let Some(parent) = game.parent(node).ok().flatten() else {
            return false;
        };
        let side_branch = plan.dead.contains(&parent)
            || (plan.skip.contains(&parent)
                && game.children(parent).ok().and_then(<[NodeId]>::first) != Some(&node));
        if side_branch {
            plan.dead.insert(node);
        }
        side_branch
    }

    /// The same run in report mode that goes on from a stopped one: `known`
    /// are the evaluations already collected, the base first, and the walk
    /// starts at the first position after them (D2110).
    #[must_use]
    pub(crate) fn with_known(mut self, known: Vec<PositionEval>) -> AnnotationRun {
        // `report_data` only gives a report of this very main line, so
        // `known` never outgrows the walk (D2197); a stray one is ignored
        // rather than trusted.
        debug_assert!(known.len() <= self.order.len(), "known outgrows the walk");
        if !self.collect || known.is_empty() || known.len() > self.order.len() {
            return self;
        }
        self.next = known.len();
        self.done = u32::try_from(known.len() - 1).unwrap_or(u32::MAX);
        self.collected = known;
        self
    }

    /// The progress for the status bar.
    pub(crate) fn progress(&self) -> AnnotationProgress {
        let moves = self.order.len().saturating_sub(1).max(1);
        AnnotationProgress {
            done_plies: self.done,
            total_plies: u32::try_from(moves).unwrap_or(u32::MAX),
            game_index: 1,
            game_total: 1,
        }
    }

    /// The evaluations collected in report mode, one per position walked
    /// (the base first), including a partial walk.
    pub(crate) fn take_collected(&mut self) -> Vec<PositionEval> {
        std::mem::take(&mut self.collected)
    }

    /// Whether the run is over.
    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    /// Starts the run: sends the first search.
    pub(crate) fn start(&mut self, game: &Game) -> Vec<RunOutput> {
        let mut out = Vec::new();
        self.advance(game, &mut out);
        out
    }

    /// The user cancelled. A search that started is stopped and the run ends
    /// when its `bestmove` arrives (the edits stay, D469). One that has not
    /// started (it waits behind an `isready` of the engine, or the engine
    /// never took it) cannot be waited for: the `stop` also drops the request
    /// the engine still holds, and the run ends at once; whatever that search
    /// sends late is ignored (D894).
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
                self.end(&mut out, Outcome::Cancelled);
            }
            None => self.end(&mut out, Outcome::Cancelled),
        }
        out
    }

    /// Whether the user asked to cancel and the run waits for the engine.
    pub(crate) fn is_cancelling(&self) -> bool {
        self.cancelling
    }

    /// The identifier of the search in flight, once the engine labelled or
    /// started it.
    pub(crate) fn pending_search(&self) -> Option<SearchId> {
        self.inflight.as_ref().and_then(|f| f.search)
    }

    /// Whether the search in flight was sent but the engine has not said it
    /// started (the request is refused, queued or lost).
    pub(crate) fn awaits_start(&self) -> bool {
        self.inflight.as_ref().is_some_and(|f| f.search.is_none())
    }

    /// Gives the run an event of its engine. `None`: the event is not about
    /// a search of the run and must go where it would have gone.
    pub(crate) fn on_event(&mut self, game: &Game, event: &EngineEvent) -> Option<Vec<RunOutput>> {
        let flight = self.inflight.as_mut()?;
        match event {
            // The engine refused the search itself: no `bestmove` will come.
            EngineEvent::Warning(EngineWarning::CommandRefused {
                command: "go" | "position",
                ..
            }) if flight.search.is_none() => Some(self.refused()),
            EngineEvent::SearchTagged { search, tag } if *tag == flight.tag => {
                flight.search = Some(*search);
                Some(Vec::new())
            }
            // An engine whose label was lost (a full event queue): the
            // search that starts next is ours.
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
                Some(self.search_done(game, best))
            }
            _ => None,
        }
    }

    /// The engine refused the search in flight (it never started): the run
    /// fails, or, when the user already cancelled, ends cancelled.
    pub(crate) fn refused(&mut self) -> Vec<RunOutput> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        let outcome = if self.cancelling {
            Outcome::Cancelled
        } else {
            Outcome::Failed(AnnotationError::EngineFailed)
        };
        self.end(&mut out, outcome);
        out
    }

    // ---- the walk ----

    /// Evaluates the next positions until a search must be waited for.
    fn advance(&mut self, game: &Game, out: &mut Vec<RunOutput>) {
        loop {
            if self.cancelling {
                self.end(out, Outcome::Cancelled);
                return;
            }
            let Some(&node) = self.order.get(self.next) else {
                self.end(out, Outcome::Finished);
                return;
            };
            if self.skipped_by_book(game, node) {
                if self.next > 0 {
                    self.done += 1;
                }
                self.next += 1;
                continue;
            }
            let Ok(position) = game.position_at(node) else {
                self.end(out, Outcome::Failed(AnnotationError::StaleGame));
                return;
            };
            if position.legal_moves().is_empty() {
                // Checkmate or stalemate: no engine is asked (D463).
                let eval = PositionEval::no_moves(position);
                if self.evaluated(game, node, eval, out) {
                    continue;
                }
                return;
            }
            self.send_position(game, node, out);
            return;
        }
    }

    fn end(&mut self, out: &mut Vec<RunOutput>, outcome: Outcome) {
        self.finished = true;
        self.inflight = None;
        out.push(RunOutput::Done(outcome));
    }

    fn next_tag(&mut self) -> u64 {
        self.tag_seq = self.tag_seq.wrapping_add(1);
        TAG_BASE | (self.tag_seq & 0x0000_FFFF_FFFF_FFFF)
    }

    fn request(
        &mut self,
        game: &Game,
        node: NodeId,
        limits: SearchLimits,
    ) -> Option<(SearchRequest, u64)> {
        let path = game.path_to(node).ok()?;
        let moves = path
            .iter()
            .map(|n| game.mv(*n))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let tag = self.next_tag();
        let mut request = SearchRequest::infinite(SearchPosition {
            start: game.start_position().clone(),
            moves,
        });
        request.limits = limits;
        request.tag = tag;
        Some((request, tag))
    }

    fn send_position(&mut self, game: &Game, node: NodeId, out: &mut Vec<RunOutput>) {
        let limits = match self.options.control {
            Control::Depth => SearchLimits {
                depth: Some(self.options.depth),
                ..SearchLimits::default()
            },
            Control::Time => SearchLimits {
                movetime: Some(self.options.movetime),
                ..SearchLimits::default()
            },
        };
        let Some((request, tag)) = self.request(game, node, limits) else {
            self.end(out, Outcome::Failed(AnnotationError::StaleGame));
            return;
        };
        out.push(RunOutput::Send(Box::new(EngineCommand::Search(request))));
        self.inflight = Some(InFlight {
            tag,
            search: None,
            purpose: Purpose::Position(node),
            best: None,
            second: None,
        });
    }

    /// A search ended.
    fn search_done(&mut self, game: &Game, best: &BestMove) -> Vec<RunOutput> {
        let mut out = Vec::new();
        let Some(flight) = self.inflight.take() else {
            return out;
        };
        if self.cancelling {
            self.end(&mut out, Outcome::Cancelled);
            return out;
        }
        match flight.purpose {
            Purpose::Position(node) => {
                let Some(line) = flight.best else {
                    self.end(&mut out, Outcome::Failed(AnnotationError::NoScore));
                    return out;
                };
                let mut pv = line.pv;
                if pv.is_empty() {
                    pv.extend(best.mv);
                }
                let mut eval = PositionEval::from_score(line.score, pv, line.depth);
                eval.second = flight.second.map(|(s, _)| s.pawns());
                if self.evaluated(game, node, eval, &mut out) {
                    self.advance(game, &mut out);
                }
            }
            Purpose::Probe(mut probe) => {
                probe.answers.push(best.mv);
                if self.probe_step(game, probe, &mut out) {
                    self.advance(game, &mut out);
                }
            }
        }
        out
    }

    /// Stores the evaluation of `node` and decides its move. Returns
    /// whether the walk goes on (`false`: probes were started, or the run
    /// ended).
    fn evaluated(
        &mut self,
        game: &Game,
        node: NodeId,
        eval: PositionEval,
        out: &mut Vec<RunOutput>,
    ) -> bool {
        if self.collect {
            if node != self.order[0] {
                self.done += 1;
            }
            self.collected.push(eval);
            self.next += 1;
            return true;
        }
        if node == self.order[0] {
            if self.zero_moves {
                // Rule 11: a game without moves.
                out.push(RunOutput::Apply(vec![Action::InitialComment {
                    text: format!("{}:{}", eval.depth, signed_pawns(eval.score)),
                }]));
            }
            self.evals.insert(node, eval);
            self.next += 1;
            return true;
        }
        self.evals.insert(node, eval);
        let step = self.step(game, node);
        // A leaf is nobody's parent: its evaluation is not needed again.
        if game.children(node).is_ok_and(<[NodeId]>::is_empty) {
            self.evals.remove(&node);
        }
        let Some(step) = step else {
            // Without the evaluation of the parent there is nothing to
            // compare: skip the move.
            self.done += 1;
            self.next += 1;
            return true;
        };
        let decision = decide(&self.options, &self.informant, &step);
        self.prev_nag = decision.prev_nag;
        let mut actions = decision.actions;
        // The move that left the book says so (the comment goes first: the
        // score is put in front of it).
        if let Some(plan) = &self.book
            && plan.out == Some(node)
            && let Some(text) = &plan.comment
        {
            actions.insert(
                0,
                Action::EndComment {
                    node,
                    text: text.clone(),
                },
            );
        }
        actions.extend(self.end_comment(game, &step));
        let candidate = actions
            .iter()
            .any(|a| matches!(a, Action::ExerciseCandidate { .. }));
        if candidate && self.exercises {
            let side_to_move = !step.mover;
            if let Some(best) = exercise_move(&self.informant, &step.prev, &step.cur, side_to_move)
            {
                let probe = Probe {
                    node,
                    actions,
                    best,
                    prev: step.prev.score,
                    cur: step.cur.score,
                    answers: Vec::new(),
                };
                return self.probe_step(game, probe, out);
            }
        }
        self.finish_step(actions, out);
        true
    }

    /// Puts the lines of the run back after the probes of an exercise.
    fn raise_lines(&mut self, out: &mut Vec<RunOutput>) {
        if std::mem::take(&mut self.lowered) {
            out.push(RunOutput::Send(Box::new(EngineCommand::SetMultiPv(
                self.lines,
            ))));
        }
    }

    fn finish_step(&mut self, actions: Vec<Action>, out: &mut Vec<RunOutput>) {
        let actions: Vec<Action> = actions
            .into_iter()
            .filter(|a| !matches!(a, Action::ExerciseCandidate { .. }))
            .collect();
        if !actions.is_empty() {
            out.push(RunOutput::Apply(actions));
        }
        self.done += 1;
        self.next += 1;
    }

    fn step(&self, game: &Game, node: NodeId) -> Option<Step> {
        let parent = game.parent(node).ok().flatten()?;
        let prev = self.evals.get(&parent)?.clone();
        let cur = self.evals.get(&node)?.clone();
        let before = game.position_at(parent).ok()?;
        let after = game.position_at(node).ok()?;
        Some(Step {
            node,
            parent,
            mover: before.turn(),
            prev,
            cur,
            played: game.mv(node).ok()?,
            is_last: game.children(node).ok()?.is_empty(),
            ply: game.ply(node).ok()?,
            non_standard_start: self.non_standard_start,
            cur_no_moves: after.legal_moves().is_empty(),
            prev_nag: self.prev_nag,
            suppress_score: self
                .book
                .as_ref()
                .is_some_and(|plan| plan.out == Some(node)),
            wanted_depth: self.options.depth,
        })
    }

    /// The comment of the last move of the main line with the line of the
    /// final position (D468, `VarAtEnd`).
    fn end_comment(&self, game: &Game, step: &Step) -> Option<Action> {
        if !self.options.var_at_end
            || matches!(self.options.with_vars, WithVars::No | WithVars::AllMoves)
            || !step.is_last
            || step.cur.pv.is_empty()
            || game.mainline().last() != Some(step.node)
        {
            return None;
        }
        let after = game.position_at(step.node).ok()?;
        if after.is_checkmate() {
            return None;
        }
        let line = format_pv(after, &step.cur.pv, NumberStyle::Tight);
        if line.is_empty() {
            return None;
        }
        Some(Action::EndComment {
            node: step.node,
            text: format!("{}:{} {line}", step.cur.depth, signed_pawns(step.cur.score)),
        })
    }

    // ---- exercises (D470) ----

    /// Sends the next probe of `probe`, or resolves it when it is done.
    /// Returns whether the walk goes on.
    fn probe_step(&mut self, game: &Game, probe: Probe, out: &mut Vec<RunOutput>) -> bool {
        let depths = OBVIOUS_DEPTHS.len();
        let total = depths + DIFFICULTY_TIMES_MS.len();
        let answered = probe.answers.len();
        // The move is obvious: not an exercise.
        if answered == depths && is_obvious(probe.best, &probe.answers) {
            self.raise_lines(out);
            self.finish_step(without_marks(probe.actions, None), out);
            return true;
        }
        if answered >= total {
            let level = difficulty(probe.best, &probe.answers[depths..]);
            let mark = Action::Exercise {
                node: probe.node,
                difficulty: level,
                prev: probe.prev,
                cur: probe.cur,
            };
            self.raise_lines(out);
            self.finish_step(without_marks(probe.actions, Some(mark)), out);
            return true;
        }
        let limits = if let Some(depth) = OBVIOUS_DEPTHS.get(answered) {
            SearchLimits {
                depth: Some(*depth),
                ..SearchLimits::default()
            }
        } else {
            let ms = DIFFICULTY_TIMES_MS
                .get(answered - depths)
                .copied()
                .unwrap_or(1);
            SearchLimits {
                movetime: Some(Duration::from_millis(ms)),
                ..SearchLimits::default()
            }
        };
        let node = probe.node;
        let Some((request, tag)) = self.request(game, node, limits) else {
            self.end(out, Outcome::Failed(AnnotationError::StaleGame));
            return false;
        };
        // Only the move of the probe matters: one line (D893).
        if !self.lowered && self.lines > 1 {
            self.lowered = true;
            out.push(RunOutput::Send(Box::new(EngineCommand::SetMultiPv(1))));
        }
        if self.clear_hash {
            out.push(RunOutput::Send(Box::new(EngineCommand::SetOption {
                name: CLEAR_HASH.to_owned(),
                value: OptionValue::Bool(true),
            })));
        }
        out.push(RunOutput::Send(Box::new(EngineCommand::Search(request))));
        self.inflight = Some(InFlight {
            tag,
            search: None,
            purpose: Purpose::Probe(probe),
            best: None,
            second: None,
        });
        false
    }
}

/// `actions` without the exercise candidates, with `mark` in the place of
/// the first one (the Scid marks before it writes the score).
fn without_marks(actions: Vec<Action>, mark: Option<Action>) -> Vec<Action> {
    let mut mark = mark;
    let mut out = Vec::with_capacity(actions.len());
    for action in actions {
        if matches!(action, Action::ExerciseCandidate { .. }) {
            out.extend(mark.take());
        } else {
            out.push(action);
        }
    }
    out
}
