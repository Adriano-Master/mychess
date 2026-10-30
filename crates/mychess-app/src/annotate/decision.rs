//! The decision for one move: the rules of `addAnnotation` of the Scid as a
//! pure function (spec M4-6, D461, "Regras e casos de borda", rules 1-12).
//!
//! `decide` knows nothing about the engine or the game: it gets the
//! evaluations before and after the move and says which edits to make. All
//! comparisons are made in whole centipawns (see
//! [`super::options::centipawns`]). Scores are in pawns from
//! White's point of view.

use mychess_core::{Color, Move, Nag, NodeId};

use super::options::{AnnotateOptions, Informant, Side, WithScore, WithVars, centipawns};
use super::score::{PositionEval, format_score_text, score_to_nag, signed_pawns};

/// What `decide` needs to know about one move (the "Step" of the spec).
#[derive(Debug, Clone)]
pub struct Step {
    /// The node of the move.
    pub node: NodeId,
    /// The node before the move.
    pub parent: NodeId,
    /// Who played the move.
    pub mover: Color,
    /// The evaluation of the position before the move.
    pub prev: PositionEval,
    /// The evaluation of the position after the move.
    pub cur: PositionEval,
    /// The move played.
    pub played: Move,
    /// The move ends its line (the Scid's `isAt vend`).
    pub is_last: bool,
    /// Plies from the start of the game: 1 for the first move.
    pub ply: u32,
    /// The game starts from a position that is not the standard one.
    pub non_standard_start: bool,
    /// The position after the move has no legal move (`getScoreNoMoves`).
    pub cur_no_moves: bool,
    /// The last evaluation NAG written (`prevNag`), for `WithVars = AllMoves`.
    pub prev_nag: Option<Nag>,
    /// The score of this move is not written once (the first move out of
    /// a book, rule 9). Always `false` without a book.
    pub suppress_score: bool,
    /// The depth the "missed mates" test assumes (`WantedDepth`).
    pub wanted_depth: u32,
}

/// One edit to the game, in the order of the Scid.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Puts `text` in front of the comment after the move of `node`
    /// (`addScore`).
    Score {
        /// The move.
        node: NodeId,
        /// The text.
        text: String,
    },
    /// Adds a NAG to the move of `node`.
    Nag {
        /// The move.
        node: NodeId,
        /// The NAG.
        nag: Nag,
    },
    /// Adds the engine's line as an alternative to the child of `parent`.
    Variation {
        /// The node the line starts from.
        parent: NodeId,
        /// The moves.
        pv: Vec<Move>,
        /// The comment before the first move (`Mate in N`).
        comment: Option<String>,
        /// The NAG for the last move of the line.
        end_nag: Option<Nag>,
    },
    /// The move may be a tactical exercise (`markExercise`): the run checks
    /// it with the engine and replaces it by [`Action::Exercise`] or drops it.
    ExerciseCandidate {
        /// The move.
        node: NodeId,
    },
    /// The move is a tactical exercise (D471).
    Exercise {
        /// The move.
        node: NodeId,
        /// The difficulty, 1 to 9.
        difficulty: u8,
        /// The score before the move.
        prev: f32,
        /// The score after the move.
        cur: f32,
    },
    /// Appends `text` to the comment after the move of `node` (the line of
    /// the final position, D468).
    EndComment {
        /// The move.
        node: NodeId,
        /// The text.
        text: String,
    },
    /// Appends `text` to the initial comment of the game (a game without
    /// moves, rule 11).
    InitialComment {
        /// The text.
        text: String,
    },
}

/// The edits for one move and the new `prevNag`.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// The edits, in the order to apply them.
    pub actions: Vec<Action>,
    /// The value of `prevNag` after this move.
    pub prev_nag: Option<Nag>,
}

/// What kind of mistake was found (the Scid's `isBlunder` 0, 1, 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Error {
    No,
    Loss,
    Mate,
}

impl Error {
    fn found(self) -> bool {
        self != Error::No
    }
}

/// The kind of text of a score (`addScore`'s `type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScoreKind {
    Single,
    Both,
}

fn nag(number: u8) -> Option<Nag> {
    Nag::new(u16::from(number)).ok()
}

/// The NAG of the loss `a` (centipawns) and whether it is a candidate for
/// an exercise (`?` and `??`).
fn error_nag(a: i64, inf: &Informant) -> Option<(Nag, bool)> {
    let (dubious, poor, blunder) = (
        centipawns(inf.dubious),
        centipawns(inf.poor),
        centipawns(inf.blunder),
    );
    if a > dubious && a <= poor {
        nag(6).map(|n| (n, false)) // ?!
    } else if a > poor && a <= blunder {
        nag(2).map(|n| (n, true)) // ?
    } else if a > blunder {
        nag(4).map(|n| (n, true)) // ??
    } else {
        None
    }
}

/// `addScore`: the action that writes the score of the move, if any.
/// `novar` is the Scid's argument of the same name.
fn score_action(
    opts: &AnnotateOptions,
    step: &Step,
    kind: ScoreKind,
    novar: bool,
) -> Option<Action> {
    let suppressed = match opts.with_score {
        WithScore::No => true,
        WithScore::Variations | WithScore::Blunders => novar,
        WithScore::AllMoves => false,
    };
    if suppressed || step.suppress_score {
        return None;
    }
    let text = if step.cur_no_moves || kind == ScoreKind::Single {
        // A position without moves is always written as a single score
        // (rule 10).
        format_score_text(step.cur.score, step.cur.mate, opts.score_format)
    } else {
        format!(
            "{} / {}",
            signed_pawns(step.cur.score),
            signed_pawns(step.prev.score)
        )
    };
    Some(Action::Score {
        node: step.node,
        text,
    })
}

/// Whether the side filter skips the move (rule 1).
pub fn skips_side(opts: &AnnotateOptions, mover: Color) -> bool {
    matches!(
        (opts.side, mover),
        (Side::White, Color::Black) | (Side::Black, Color::White)
    )
}

/// Decides what to write for one move (`addAnnotation`).
#[allow(clippy::too_many_lines)] // one flat decision table, as in the Scid
pub fn decide(opts: &AnnotateOptions, inf: &Informant, step: &Step) -> Decision {
    let mut decision = Decision {
        actions: Vec::new(),
        prev_nag: step.prev_nag,
    };
    // Rule 1: the other side's moves only advance `prev`.
    if skips_side(opts, step.mover) {
        return decision;
    }
    let out = &mut decision.actions;
    let white_moved = step.mover == Color::White;

    // Rule 2: the move is the one the engine expected.
    if let Some(first) = step.prev.pv.first()
        && !step.is_last
        && opts.with_vars != WithVars::AllMoves
        && *first == step.played
    {
        if opts.with_score == WithScore::AllMoves {
            out.extend(score_action(opts, step, ScoreKind::Single, true));
        }
        return decision;
    }

    let cur = centipawns(step.cur.score);
    let mut prev = centipawns(step.prev.score);
    let mut prev_mate = i64::from(step.prev.mate.unwrap_or(0));
    let cur_mate = i64::from(step.cur.mate.unwrap_or(0));
    // Rule 3: a game that starts from a set-up position has no real
    // "before" for its first move.
    let mut prev_score_for_nag = step.prev.score;
    if step.non_standard_start && step.ply == 1 {
        prev = cur;
        prev_mate = cur_mate;
        prev_score_for_nag = step.cur.score;
    }
    let delta = cur - prev;
    let thr = centipawns(opts.blunder);
    let cutoff = centipawns(opts.cutoff);
    let won = centipawns(inf.won);

    // Rules 4 and 5: the kind of mistake.
    let depth = i64::from(step.wanted_depth);
    let mut error = Error::No;
    let missed_mate = opts.missed_mates && {
        let (candidate, shorter) = if white_moved {
            (
                prev_mate > 0 && prev_mate < depth - 3,
                cur_mate > prev_mate - 1 || cur_mate <= 0,
            )
        } else {
            (
                prev_mate < 0 && -prev_mate < depth - 3,
                cur_mate < prev_mate + 1 || cur_mate >= 0,
            )
        };
        candidate && shorter
    };
    if missed_mate {
        error = Error::Mate;
    } else if prev.abs() < cutoff
        || cur.abs() < cutoff
        || (delta.abs() > cur.abs() && cur * prev < 0)
    {
        if opts.with_vars == WithVars::NotBest {
            if (delta < 0 && white_moved) || (delta > 0 && !white_moved) {
                error = Error::Loss;
            }
        } else {
            if (delta < -thr && white_moved) || (delta > thr && !white_moved) {
                error = Error::Loss;
            }
            // The game was decided before the move: no comment (rule 5).
            if (prev > won && !white_moved) || (prev < -won && white_moved) {
                error = Error::No;
            }
        }
    }

    let abs_delta = delta.abs();
    let eval_nag = |score: f32| score_to_nag(score, inf);

    match opts.with_vars {
        WithVars::No => {
            // Rule 6: scores only.
            if opts.with_score == WithScore::AllMoves {
                out.extend(score_action(opts, step, ScoreKind::Single, false));
            } else if opts.with_score == WithScore::Blunders && error.found() {
                out.extend(score_action(opts, step, ScoreKind::Both, false));
            }
        }
        WithVars::AllMoves => {
            // Rule 7.
            out.extend(score_action(opts, step, ScoreKind::Single, false));
            let lost = (delta < -thr && white_moved) || (delta > thr && !white_moved);
            if lost && let Some((n, exercise)) = error_nag(abs_delta, inf) {
                out.push(Action::Nag {
                    node: step.node,
                    nag: n,
                });
                if exercise {
                    out.push(Action::ExerciseCandidate { node: step.node });
                }
            }
            let new_nag = eval_nag(step.cur.score);
            if let Some(n) = new_nag
                && Some(n) != step.prev_nag
            {
                out.push(Action::Nag {
                    node: step.node,
                    nag: n,
                });
            }
            decision.prev_nag = new_nag;
            push_variation(out, step, error, false, eval_nag(prev_score_for_nag));
        }
        WithVars::Blunders | WithVars::NotBest => {
            if error.found() {
                // Rule 8.
                let decided_game_goes_on =
                    (cur > won && white_moved) || (cur < -won && !white_moved);
                if decided_game_goes_on {
                    out.extend(score_action(opts, step, ScoreKind::Both, false));
                } else {
                    if let Some((n, exercise)) = error_nag(abs_delta, inf) {
                        out.push(Action::Nag {
                            node: step.node,
                            nag: n,
                        });
                        if exercise {
                            out.push(Action::ExerciseCandidate { node: step.node });
                        }
                    }
                    out.extend(score_action(opts, step, ScoreKind::Both, false));
                }
                // The Scid writes the evaluation NAG, the diagram and the
                // line in both cases (the braces of `addAnnotation` close
                // the `if` before them).
                if let Some(n) = eval_nag(step.cur.score) {
                    out.push(Action::Nag {
                        node: step.node,
                        nag: n,
                    });
                }
                // The diagram goes on the parent node (the position before
                // the error): faithful to `addAnnotation`, `analysis.tcl`
                // 1646-1650 (`sc_move back`, `sc_pos addNag D`), not a slip.
                if let Some(d) = nag(201) {
                    out.push(Action::Nag {
                        node: step.parent,
                        nag: d,
                    });
                }
                push_variation(out, step, error, true, eval_nag(prev_score_for_nag));
            } else {
                // Without a mistake the score is written only for
                // "all moves" (`novar`).
                out.extend(score_action(opts, step, ScoreKind::Single, true));
            }
        }
    }
    decision
}

/// The variation with the engine's line before the move (`sc_var create`
/// ... `moveAdd`). A line that starts with the move played is not a
/// variation: the tree does not repeat moves, and the line would extend the
/// game itself.
fn push_variation(
    out: &mut Vec<Action>,
    step: &Step,
    error: Error,
    ellipsis: bool,
    end_nag: Option<Nag>,
) {
    let Some(first) = step.prev.pv.first() else {
        return;
    };
    if *first == step.played {
        return;
    }
    let comment = (error == Error::Mate).then(|| {
        let n = step.prev.mate.map_or(0, i32::unsigned_abs);
        if ellipsis {
            format!("Mate in {n}...")
        } else {
            format!("Mate in {n}")
        }
    });
    out.push(Action::Variation {
        parent: step.parent,
        pv: step.prev.pv.clone(),
        comment,
        end_nag,
    });
}

#[cfg(test)]
mod tests {
    use mychess_core::{Game, Position};

    use super::*;

    /// A step for a move played from the standard position; the node ids
    /// are real ones of a two-move game.
    struct Fixture {
        game: Game,
        parent: NodeId,
        node: NodeId,
        played: Move,
        alt: Move,
    }

    fn fixture() -> Fixture {
        let mut game = Game::new();
        let root = game.root();
        let start = Position::new();
        let played = start.parse_uci("a2a3").expect("uci");
        let alt = start.parse_uci("e2e4").expect("uci");
        let node = game.add_move(root, played).expect("move");
        Fixture {
            game,
            parent: root,
            node,
            played,
            alt,
        }
    }

    fn eval(score: f32, pv: &[Move]) -> PositionEval {
        PositionEval {
            score,
            mate: None,
            pv: pv.to_vec(),
            depth: 13,
            second: None,
        }
    }

    fn step(f: &Fixture, mover: Color, prev: PositionEval, cur: PositionEval) -> Step {
        let _ = &f.game;
        Step {
            node: f.node,
            parent: f.parent,
            mover,
            prev,
            cur,
            played: f.played,
            is_last: false,
            ply: 5,
            non_standard_start: false,
            cur_no_moves: false,
            prev_nag: None,
            suppress_score: false,
            wanted_depth: 13,
        }
    }

    fn run(opts: &AnnotateOptions, s: &Step) -> Decision {
        decide(opts, &Informant::default(), s)
    }

    fn nags_on(d: &Decision, node: NodeId) -> Vec<u8> {
        d.actions
            .iter()
            .filter_map(|a| match a {
                Action::Nag { node: n, nag } if *n == node => Some(nag.value()),
                _ => None,
            })
            .collect()
    }

    fn score_texts(d: &Decision) -> Vec<String> {
        d.actions
            .iter()
            .filter_map(|a| match a {
                Action::Score { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn plain() -> AnnotateOptions {
        AnnotateOptions {
            score_format: super::super::options::ScoreFormat::Plain,
            ..AnnotateOptions::default()
        }
    }

    #[test]
    fn a_blunder_gets_the_mark_the_exercise_the_score_the_diagram_and_the_line() {
        let f = fixture();
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(-1.80, &[]));
        let d = run(&plain(), &s);
        // Order of the Scid: `?`, exercise, both scores, evaluation NAG
        // on the move, `D` on the parent, then the variation.
        assert_eq!(nags_on(&d, f.node), vec![2, 17]);
        assert_eq!(nags_on(&d, f.parent), vec![201]);
        assert_eq!(score_texts(&d), vec!["-1.80 / +0.30"]);
        assert!(
            d.actions
                .contains(&Action::ExerciseCandidate { node: f.node })
        );
        let Some(Action::Variation {
            parent,
            pv,
            comment,
            end_nag,
        }) = d.actions.last()
        else {
            panic!("the variation comes last: {:?}", d.actions);
        };
        assert_eq!(*parent, f.parent);
        assert_eq!(pv, &vec![f.alt]);
        assert_eq!(*comment, None);
        assert_eq!(end_nag.map(Nag::value), Some(10), "+0.30 is `=`");
        assert!(matches!(d.actions[0], Action::Nag { .. }));
    }

    #[test]
    fn a_small_loss_only_writes_the_single_score() {
        let f = fixture();
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.10, &[]));
        let d = run(&plain(), &s);
        assert_eq!(d.actions.len(), 1);
        assert_eq!(score_texts(&d), vec!["+0.10"]);
    }

    #[test]
    fn black_losing_more_than_the_blunder_threshold_gets_two_question_marks() {
        let f = fixture();
        let s = step(&f, Color::Black, eval(-0.20, &[f.alt]), eval(3.40, &[]));
        let d = run(&plain(), &s);
        assert_eq!(nags_on(&d, f.node), vec![4, 18]);
        assert_eq!(score_texts(&d), vec!["+3.40 / -0.20"]);
    }

    #[test]
    fn a_loss_from_a_big_advantage_is_still_judged_below_the_cutoff() {
        let f = fixture();
        let s = step(&f, Color::White, eval(6.00, &[f.alt]), eval(4.00, &[]));
        let d = run(&plain(), &s);
        assert_eq!(
            nags_on(&d, f.node),
            vec![2, 18],
            "-2.00 is `?`, 4.00 is `+-`"
        );
    }

    #[test]
    fn a_decided_game_is_not_commented_even_when_the_score_climbs() {
        let f = fixture();
        let mut opts = plain();
        opts.cutoff = 8.0;
        let s = step(&f, Color::Black, eval(6.00, &[f.alt]), eval(9.00, &[]));
        let d = run(&opts, &s);
        assert_eq!(score_texts(&d), vec!["+9.00"], "single score only");
        assert!(nags_on(&d, f.node).is_empty());
        // With the default cut-off the move is not even judged.
        let d = run(&plain(), &s);
        assert_eq!(score_texts(&d), vec!["+9.00"]);
        assert!(
            d.actions
                .iter()
                .all(|a| !matches!(a, Action::Variation { .. }))
        );
    }

    #[test]
    fn a_missed_mate_is_a_mistake_with_a_mate_comment() {
        let f = fixture();
        let mut prev = eval(327.0, &[f.alt]);
        prev.mate = Some(3);
        let s = step(&f, Color::White, prev, eval(0.5, &[]));
        let d = run(&plain(), &s);
        let Some(Action::Variation { comment, .. }) = d.actions.last() else {
            panic!("variation expected: {:?}", d.actions);
        };
        assert_eq!(comment.as_deref(), Some("Mate in 3..."));
        // The loss is 326.5 pawns: `??`.
        assert_eq!(nags_on(&d, f.node)[0], 4);
    }

    #[test]
    fn a_mate_too_far_away_for_the_depth_is_not_a_missed_mate() {
        let f = fixture();
        let mut prev = eval(327.0, &[f.alt]);
        prev.mate = Some(10); // depth 13 - 3 = 10: not below
        let s = step(&f, Color::White, prev, eval(0.5, &[]));
        let d = run(&plain(), &s);
        // Not a mate error; the loss is judged by score (cutoff: |cur| < 5).
        let Some(Action::Variation { comment, .. }) = d.actions.last() else {
            panic!("variation expected");
        };
        assert_eq!(*comment, None);
    }

    #[test]
    fn a_shorter_mate_for_black_uses_the_mirrored_test() {
        let f = fixture();
        let mut prev = eval(-327.0, &[f.alt]);
        prev.mate = Some(-2);
        let mut cur = eval(-327.0, &[]);
        cur.mate = Some(-4); // still mated, but slower: missed the shorter
        let s = step(&f, Color::Black, prev, cur);
        let d = run(&plain(), &s);
        let Some(Action::Variation { comment, .. }) = d.actions.last() else {
            panic!("variation expected: {:?}", d.actions);
        };
        assert_eq!(comment.as_deref(), Some("Mate in 2..."));
    }

    #[test]
    fn the_other_sides_moves_are_skipped() {
        let f = fixture();
        let mut opts = plain();
        opts.side = Side::White;
        let s = step(&f, Color::Black, eval(0.3, &[f.alt]), eval(-3.0, &[]));
        assert!(run(&opts, &s).actions.is_empty());
        opts.side = Side::Black;
        let s = step(&f, Color::White, eval(0.3, &[f.alt]), eval(-3.0, &[]));
        assert!(run(&opts, &s).actions.is_empty());
    }

    #[test]
    fn the_expected_move_adds_at_most_the_score_and_not_at_the_end() {
        let f = fixture();
        let s = step(&f, Color::White, eval(0.30, &[f.played]), eval(-3.0, &[]));
        let d = run(&plain(), &s);
        assert_eq!(score_texts(&d), vec!["-3.00"], "even a big drop is ignored");
        assert_eq!(d.actions.len(), 1);
        let mut opts = plain();
        opts.with_score = WithScore::Blunders;
        assert!(run(&opts, &s).actions.is_empty());
        // At the last move of the line the shortcut does not apply.
        let mut last = s.clone();
        last.is_last = true;
        assert!(run(&plain(), &last).actions.len() > 1);
        // With every move annotated it does not apply either.
        let mut opts = plain();
        opts.with_vars = WithVars::AllMoves;
        assert!(run(&opts, &s).actions.len() > 1);
    }

    #[test]
    fn not_best_counts_any_loss() {
        let f = fixture();
        let mut opts = plain();
        opts.with_vars = WithVars::NotBest;
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.29, &[]));
        let d = run(&opts, &s);
        assert!(
            d.actions
                .iter()
                .any(|a| matches!(a, Action::Variation { .. }))
        );
        assert_eq!(score_texts(&d), vec!["+0.29 / +0.30"]);
        // The default needs more than the threshold.
        assert!(
            run(&plain(), &s)
                .actions
                .iter()
                .all(|a| !matches!(a, Action::Variation { .. }))
        );
    }

    #[test]
    fn scores_only_with_blunders_writes_two_scores_for_mistakes_only() {
        let f = fixture();
        let mut opts = plain();
        opts.with_vars = WithVars::No;
        opts.with_score = WithScore::Blunders;
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(-1.80, &[]));
        let d = run(&opts, &s);
        assert_eq!(d.actions.len(), 1);
        assert_eq!(score_texts(&d), vec!["-1.80 / +0.30"]);
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.20, &[]));
        assert!(run(&opts, &s).actions.is_empty());
        opts.with_score = WithScore::AllMoves;
        assert_eq!(score_texts(&run(&opts, &s)), vec!["+0.20"]);
    }

    #[test]
    fn every_move_mode_repeats_an_eval_nag_only_when_it_changes() {
        let f = fixture();
        let mut opts = plain();
        opts.with_vars = WithVars::AllMoves;
        let mut s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.20, &[]));
        let d = run(&opts, &s);
        assert_eq!(nags_on(&d, f.node), vec![10]);
        assert_eq!(d.prev_nag.map(Nag::value), Some(10));
        // The variation ends with the NAG of the score before the move.
        assert!(matches!(
            d.actions.last(),
            Some(Action::Variation { end_nag: Some(n), .. }) if n.value() == 10
        ));
        s.prev_nag = d.prev_nag;
        let again = run(&opts, &s);
        assert!(nags_on(&again, f.node).is_empty(), "same NAG as before");
        assert_eq!(again.prev_nag.map(Nag::value), Some(10));
        // A loss between the question mark thresholds adds `?!`.
        let s = step(&f, Color::White, eval(0.0, &[f.alt]), eval(-1.20, &[]));
        let d = run(&opts, &s);
        assert_eq!(nags_on(&d, f.node), vec![6, 15]);
    }

    #[test]
    fn a_position_without_moves_is_written_as_a_single_score() {
        let f = fixture();
        let mut s = step(&f, Color::Black, eval(0.0, &[f.alt]), eval(327.0, &[]));
        s.is_last = true;
        s.cur_no_moves = true;
        let mut opts = plain();
        opts.with_vars = WithVars::No;
        opts.with_score = WithScore::Blunders;
        let d = run(&opts, &s);
        assert_eq!(score_texts(&d), vec!["+327.00"]);
    }

    #[test]
    fn a_set_up_start_has_no_delta_for_its_first_move() {
        let f = fixture();
        let mut s = step(&f, Color::White, eval(0.0, &[f.alt]), eval(-2.0, &[]));
        s.non_standard_start = true;
        s.ply = 1;
        let d = run(&plain(), &s);
        assert!(nags_on(&d, f.node).is_empty());
        assert!(
            d.actions
                .iter()
                .all(|a| !matches!(a, Action::Variation { .. })),
            "no mistake on the first move of a set-up game"
        );
    }

    #[test]
    fn the_score_after_a_book_move_is_suppressed_once() {
        let f = fixture();
        let mut s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.10, &[]));
        s.suppress_score = true;
        assert!(run(&plain(), &s).actions.is_empty());
    }

    #[test]
    fn a_decided_game_that_goes_on_only_writes_both_scores_and_the_line() {
        let f = fixture();
        let mut opts = plain();
        opts.cutoff = 10.0;
        // White throws away a won position but stays above `won`.
        let s = step(&f, Color::White, eval(9.0, &[f.alt]), eval(6.0, &[]));
        let d = run(&opts, &s);
        assert!(
            nags_on(&d, f.node)
                .iter()
                .all(|n| *n != 2 && *n != 4 && *n != 6)
        );
        assert_eq!(score_texts(&d), vec!["+6.00 / +9.00"]);
        assert!(
            d.actions
                .iter()
                .any(|a| matches!(a, Action::Variation { .. }))
        );
    }

    #[test]
    fn a_line_that_starts_with_the_move_played_is_not_added() {
        let f = fixture();
        let mut opts = plain();
        opts.with_vars = WithVars::AllMoves;
        let s = step(&f, Color::White, eval(0.0, &[f.played]), eval(-2.0, &[]));
        let d = run(&opts, &s);
        assert!(
            d.actions
                .iter()
                .all(|a| !matches!(a, Action::Variation { .. }))
        );
    }

    #[test]
    fn a_blunder_written_as_eval_uses_the_chosen_format_for_single_scores_only() {
        let f = fixture();
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(0.10, &[]));
        let d = run(&AnnotateOptions::default(), &s);
        assert_eq!(score_texts(&d), vec!["[%eval +0.10]"]);
        let s = step(&f, Color::White, eval(0.30, &[f.alt]), eval(-1.80, &[]));
        let d = run(&AnnotateOptions::default(), &s);
        assert_eq!(
            score_texts(&d),
            vec!["-1.80 / +0.30"],
            "both scores are plain"
        );
    }
}
