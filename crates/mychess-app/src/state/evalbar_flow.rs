//! The evaluation bar, the arrows of the engine and the traffic lights in the
//! state (spec M4-5).
//!
//! The views are derived data, computed once per change at the end of
//! [`update`](super::update) and kept in [`EvalBarFlow`]: `view` only reads
//! them (D451, rule 12). A report of an engine that changes nothing they show
//! (same depth, scores and first moves) costs a hash of the lines and no
//! allocation.

use std::hash::{DefaultHasher, Hash, Hasher};

use mychess_core::{NodeId, Square, eval_of};
use mychess_engine::{Bound, Score};

use super::AppState;
use crate::analysis::{AnalysisView, Terminal};
use crate::effect::Effect;
use crate::engines::{EngineRunState, RunId};
use crate::evalbar::{
    ArrowKind, ArrowMode, ArrowPrefs, EngineArrow, EngineViewPrefs, EvalBarMessage, EvalBarPrefs,
    EvalBarView, EvalSource, EvalValue, TerminalResult, TrafficLight, TrafficLightPrefs,
    engine_arrows, traffic_lights,
};
use crate::prefs::{PrefChange, PrefsMessage};
use crate::session::{GameMessage, PlayOptions};

/// What was looked at for one execution when the views were built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Probe {
    run: RunId,
    /// A hash of the lines (scores, depths, first moves) and of the end of
    /// the game.
    fingerprint: u64,
    /// The lines are about the position at the cursor.
    follows: bool,
    /// The analysis is locked to a position (the bar ignores it once the
    /// cursor left that position, rule 6).
    locked: bool,
}

/// Everything the views depend on. `locked` rides in the [`Probe`] of each
/// run rather than in the key itself: it is a property of the run, and the
/// bar reads it only for the run it probed.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    changes: u64,
    cursor: NodeId,
    flipped: bool,
    prefs: EngineViewPrefs,
    main: Option<Probe>,
    arrows: Option<Probe>,
    lights: Option<Probe>,
}

/// What the sequence arrows last showed, for the redraw rule of the Scid
/// (`analysis.tcl` 3896-3900).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SequenceMemo {
    changes: u64,
    cursor: NodeId,
    run: RunId,
    prefs: ArrowPrefs,
    moves: usize,
    depth: u32,
}

/// The last value shown by the engine, kept to fade it while the engine
/// has nothing for the new position (D452).
#[derive(Debug, Clone, Copy, PartialEq)]
struct LastEngineValue {
    run: RunId,
    value: EvalValue,
    bound: Bound,
}

/// The state of the slice.
#[derive(Debug, Default)]
pub(crate) struct EvalBarFlow {
    /// The execution the user marked with "arrows on the board" (D456).
    /// Executions are not persisted; the preference `arrows.enabled` is.
    explicit: Option<RunId>,
    key: Option<Key>,
    bar: EvalBarView,
    arrows: Vec<EngineArrow>,
    lights: Vec<TrafficLight>,
    memo: Option<SequenceMemo>,
    last: Option<LastEngineValue>,
}

/// Whether the sequence arrows are drawn again (`analysis.tcl` 3896-3900):
/// the line grew past half of what was last shown, the depth went up by two
/// or more, or a mate was found.
fn sequence_redraw(
    seen_moves: usize,
    seen_depth: u32,
    moves: usize,
    depth: u32,
    mate: bool,
) -> bool {
    moves > seen_moves / 2 || depth > seen_depth.saturating_add(1) || mate
}

/// A hash of what the views read from `view`: the revision of its lines
/// (which moves on any change, the tail of a PV included) and the end of the
/// game.
fn fingerprint(view: &AnalysisView) -> u64 {
    let mut hasher = DefaultHasher::new();
    view.revision().hash(&mut hasher);
    view.lines.len().hash(&mut hasher);
    view.terminal
        .map(|t| t == Terminal::Checkmate)
        .hash(&mut hasher);
    hasher.finish()
}

/// The value the engine reports for a position, from White's side.
fn engine_value(view: &AnalysisView) -> Option<(EvalValue, Bound)> {
    let (score, bound) = view.main_score()?;
    let value = match (view.terminal, score) {
        (Some(Terminal::Stalemate), _) => EvalValue::Terminal(TerminalResult::Draw),
        (Some(Terminal::Checkmate), score) => EvalValue::Terminal(if score.as_centipawns() >= 0 {
            TerminalResult::WhiteWins
        } else {
            TerminalResult::BlackWins
        }),
        (None, Score::Mate(n)) => EvalValue::Mate(n),
        (None, Score::Cp(cp)) => {
            // `cp` is an i32 of at most 32 700 in practice; the f32 loses
            // nothing that matters for a bar.
            #[allow(clippy::cast_precision_loss)]
            let pawns = cp as f32 / 100.0;
            EvalValue::Pawns(pawns)
        }
    };
    Some((value, bound))
}

impl AppState {
    /// What the evaluation bar shows now (empty when the bar is hidden).
    pub fn eval_bar_view(&self) -> &EvalBarView {
        &self.flow.evalbar.bar
    }

    /// The arrows of the engine for the position at the cursor; empty when
    /// they are off or the lines are not about this position.
    pub fn engine_arrows(&self) -> &[EngineArrow] {
        &self.flow.evalbar.arrows
    }

    /// The strips of the MultiPV traffic lights, line 1 first; empty when
    /// they are hidden or there is nothing to show.
    pub fn traffic_lights(&self) -> &[TrafficLight] {
        &self.flow.evalbar.lights
    }

    /// The execution whose arrows are drawn: the one the user marked, or the
    /// main engine when arrows are wanted and none was marked (D821). The
    /// button "arrows on the board" of that section is pressed.
    pub fn arrows_engine(&self) -> Option<RunId> {
        if !self.prefs.engine_view.arrows.enabled || self.arrows_suppressed() {
            return None;
        }
        self.explicit_run().or_else(|| self.main_engine())
    }

    /// No arrows are drawn while something else owns the board (D825): an
    /// annotation run (D456; the engine of the run is never drawn either).
    /// The training, threat and shoot-out modes of M4-8 extend this single
    /// point.
    fn arrows_suppressed(&self) -> bool {
        // The review draws its own arrow (M4-11).
        self.annotation_running() || self.play_live() || self.report.review.is_some()
    }

    /// The marked execution, if it is still analysing.
    fn explicit_run(&self) -> Option<RunId> {
        self.flow.evalbar.explicit.filter(|run| {
            self.engine_runs
                .get(*run)
                .is_some_and(|r| r.state == EngineRunState::Running && r.analyzing)
        })
    }

    /// The execution that feeds the traffic lights: the one of the arrows
    /// when one was marked, else the main engine (D822).
    fn lights_run(&self) -> Option<RunId> {
        self.explicit_run().or_else(|| self.main_engine())
    }

    fn probe(&mut self, run: Option<RunId>) -> Option<Probe> {
        let run = run?;
        let follows = self.lines_follow_cursor(run);
        let fingerprint = fingerprint(self.analysis.get(run)?);
        let locked = self.engine_runs.get(run).is_some_and(|r| r.locked);
        Some(Probe {
            run,
            fingerprint,
            follows,
            locked,
        })
    }

    /// Rebuilds the views when something they depend on changed. Called at
    /// the end of every `update`; with the three features off it only
    /// forgets what was built.
    pub(crate) fn refresh_evalbar(&mut self) {
        let prefs = *self.prefs.engine_view;
        if !(prefs.eval_bar.show || prefs.arrows.enabled || prefs.traffic_lights.show) {
            let flow = &mut self.flow.evalbar;
            if flow.key.take().is_some() {
                flow.bar = EvalBarView::empty(false);
                flow.arrows.clear();
                flow.lights.clear();
                flow.last = None;
                flow.memo = None;
            }
            return;
        }
        // One probe per run, shared by the three features.
        let mut probes: Vec<Probe> = Vec::with_capacity(3);
        let mut probe_of = |state: &mut Self, run: Option<RunId>| {
            let run = run?;
            if let Some(found) = probes.iter().find(|p| p.run == run) {
                return Some(*found);
            }
            let probe = state.probe(Some(run))?;
            probes.push(probe);
            Some(probe)
        };
        // The engines belong to the game against an engine meanwhile
        // (D1241): the bar, the arrows and the lights show nothing.
        let playing = self.play_live();
        let main = if prefs.eval_bar.show && !playing {
            let run = self.main_engine();
            probe_of(self, run)
        } else {
            None
        };
        let arrows_run = self.arrows_engine();
        let arrows = probe_of(self, arrows_run);
        let lights = if prefs.traffic_lights.show && !playing {
            let run = self.lights_run();
            probe_of(self, run)
        } else {
            None
        };
        let key = Key {
            changes: self.session.changes(),
            cursor: self.session.cursor(),
            flipped: self.board_flipped,
            prefs,
            main,
            arrows,
            lights,
        };
        if self.flow.evalbar.key.as_ref() == Some(&key) {
            return;
        }
        self.build_bar(&key);
        self.build_arrows(&key);
        self.build_lights(&key);
        self.flow.evalbar.key = Some(key);
    }

    fn build_bar(&mut self, key: &Key) {
        if !key.prefs.eval_bar.show {
            self.flow.evalbar.bar = EvalBarView::empty(key.flipped);
            return;
        }
        let engine = key.main.filter(|probe| {
            // A locked analysis does not feed the bar once the cursor left
            // its position (rule 6); an unlocked one does, faded.
            probe.follows || !probe.locked
        });
        let mut source = EvalSource::None;
        let mut value = None;
        let mut stale = false;
        if let Some(probe) = engine {
            let fresh = self
                .analysis
                .get(probe.run)
                .and_then(engine_value)
                .map(|(v, b)| (v, b, !probe.follows));
            if let Some((v, b, old)) = fresh {
                self.flow.evalbar.last = Some(LastEngineValue {
                    run: probe.run,
                    value: v,
                    bound: b,
                });
                (source, value, stale) = (EvalSource::Engine(probe.run), Some((v, b)), old);
            } else if let Some(last) = self.flow.evalbar.last.filter(|l| l.run == probe.run) {
                // The engine has nothing for this position yet: keep the
                // last value, faded, so the bar does not blink.
                (source, value, stale) = (
                    EvalSource::Engine(probe.run),
                    Some((last.value, last.bound)),
                    true,
                );
            }
        } else {
            // A dead or stopped engine leaves nothing behind (rule 4).
            self.flow.evalbar.last = None;
        }
        if value.is_none() {
            let comment = self
                .session
                .game()
                .comment(self.session.cursor())
                .ok()
                .flatten()
                .and_then(eval_of);
            if let Some(pawns) = comment {
                (source, value) = (
                    EvalSource::Comment,
                    Some((EvalValue::Pawns(pawns), Bound::Exact)),
                );
            }
        }
        self.flow.evalbar.bar =
            EvalBarView::new(source, value, stale, key.prefs.eval_bar, key.flipped);
    }

    fn build_arrows(&mut self, key: &Key) {
        let flow = &mut self.flow.evalbar;
        let Some(probe) = key.arrows.filter(|p| p.follows) else {
            flow.arrows.clear();
            flow.memo = None;
            return;
        };
        let Some(view) = self.analysis.get(probe.run) else {
            flow.arrows.clear();
            return;
        };
        let first = view.lines.first();
        let moves = first.map_or(0, |l| l.pv.len());
        let depth = first.map_or(0, |l| l.depth);
        let mate = first.is_some_and(|l| matches!(l.score, Score::Mate(n) if n != 0));
        let same_screen = flow.memo.is_some_and(|m| {
            m.changes == key.changes
                && m.cursor == key.cursor
                && m.run == probe.run
                && m.prefs == key.prefs.arrows
        });
        if key.prefs.arrows.mode == ArrowMode::Sequence
            && let Some(memo) = flow.memo.filter(|_| same_screen)
            && !sequence_redraw(memo.moves, memo.depth, moves, depth, mate)
        {
            return;
        }
        flow.arrows = engine_arrows(view, key.prefs.arrows);
        flow.memo = Some(SequenceMemo {
            changes: key.changes,
            cursor: key.cursor,
            run: probe.run,
            prefs: key.prefs.arrows,
            moves,
            depth,
        });
    }

    fn build_lights(&mut self, key: &Key) {
        let lights = key
            .lights
            .filter(|p| p.follows)
            .and_then(|p| self.analysis.get(p.run))
            .map(traffic_lights)
            .unwrap_or_default();
        self.flow.evalbar.lights = lights;
    }

    // ---- the messages ----

    pub(crate) fn evalbar_message(&mut self, message: &EvalBarMessage, effects: &mut Vec<Effect>) {
        let prefs = *self.prefs.engine_view;
        match *message {
            EvalBarMessage::ToggleBar => self.set_pref(
                PrefChange::EvalBar(EvalBarPrefs {
                    show: !prefs.eval_bar.show,
                    ..prefs.eval_bar
                }),
                effects,
            ),
            EvalBarMessage::ToggleTrafficLights => self.set_pref(
                PrefChange::TrafficLights(TrafficLightPrefs {
                    show: !prefs.traffic_lights.show,
                }),
                effects,
            ),
            EvalBarMessage::ToggleArrowNumbers => self.set_pref(
                PrefChange::Arrows(ArrowPrefs {
                    number_lines: !prefs.arrows.number_lines,
                    ..prefs.arrows
                }),
                effects,
            ),
            EvalBarMessage::CycleArrowMode => self.set_pref(
                PrefChange::Arrows(ArrowPrefs {
                    mode: prefs.arrows.mode.next(),
                    ..prefs.arrows
                }),
                effects,
            ),
            EvalBarMessage::CycleSequenceSide => self.set_pref(
                PrefChange::Arrows(ArrowPrefs {
                    sequence_side: prefs.arrows.sequence_side.next(),
                    ..prefs.arrows
                }),
                effects,
            ),
            EvalBarMessage::ToggleArrows(run) => {
                let running = self
                    .engine_runs
                    .get(run)
                    .is_some_and(|r| r.state == EngineRunState::Running);
                if !running {
                    return;
                }
                let on = self.arrows_engine() != Some(run);
                self.flow.evalbar.explicit = on.then_some(run);
                self.set_pref(
                    PrefChange::Arrows(ArrowPrefs {
                        enabled: on,
                        ..prefs.arrows
                    }),
                    effects,
                );
            }
            EvalBarMessage::PlayArrow {
                run,
                line,
                from,
                to,
            } => self.play_arrow(run, line, (from, to), effects),
        }
    }

    fn set_pref(&mut self, change: PrefChange, effects: &mut Vec<Effect>) {
        self.prefs_message(PrefsMessage::Set(change), effects);
    }

    /// Plays the first move of a line, by the path of "Add move" (D457).
    ///
    /// The click is honoured only for an arrow that is on the board now
    /// (D826): the run is the one that draws, the arrows are on, the arrow
    /// is the first move of its line (a `Sequence(ply > 0)` arrow is not) and
    /// the first move of the line still has those squares.
    fn play_arrow(
        &mut self,
        run: RunId,
        line: u8,
        squares: (Square, Square),
        effects: &mut Vec<Effect>,
    ) {
        if !self.prefs.engine_view.arrows.click_to_play
            || self.arrows_engine() != Some(run)
            || !self.flow.evalbar.arrows.iter().any(|a| {
                a.rank == line
                    && (a.from, a.to) == squares
                    && matches!(
                        a.kind,
                        ArrowKind::Best | ArrowKind::Other | ArrowKind::Sequence(0)
                    )
            })
            || !self.edit_allowed(run)
            || !self.lines_usable(run)
        {
            return;
        }
        let first = self
            .analysis
            .get(run)
            .and_then(|v| v.lines.iter().find(|l| l.multipv == line))
            .and_then(|l| l.pv.first().copied());
        let Some(mv) = first
            .filter(|mv| (mv.from(), mv.to()) == (Some(squares.0), Some(squares.1)))
            .filter(|mv| self.session.position().is_legal(*mv))
        else {
            return;
        };
        self.apply_game(
            GameMessage::PlayMove {
                mv,
                options: PlayOptions::default(),
            },
            effects,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_redraw_rule_follows_the_scid() {
        // Grew, or shrank by less than half: redraw.
        assert!(sequence_redraw(10, 20, 6, 20, false));
        assert!(!sequence_redraw(10, 20, 5, 20, false));
        // Two plies deeper.
        assert!(sequence_redraw(10, 20, 5, 22, false));
        assert!(!sequence_redraw(10, 20, 5, 21, false));
        // A mate always redraws.
        assert!(sequence_redraw(10, 20, 0, 20, true));
        // First report.
        assert!(sequence_redraw(0, 0, 1, 1, false));
    }
}
