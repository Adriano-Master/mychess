//! The seam of the training after a move of the user (D494, `::tree::doTraining`,
//! `tcl/windows/tree.tcl` 432-452).

use crate::effect::Effect;
use crate::state::AppState;

/// What a hook did with the move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingOutcome {
    /// Not mine: ask the next hook.
    Continue,
    /// Answered: the hooks after this one are skipped.
    Handled,
}

/// Something that answers the move of the user: the engines with automove
/// first, then the tablebase and then the opening tree (the order of the
/// Scid; the last two arrive in M8 and M5).
pub trait TrainingHook {
    /// Called after the user played a move, before anything else reacts.
    /// May change the state and ask for effects.
    fn after_user_move(
        &mut self,
        state: &mut AppState,
        effects: &mut Vec<Effect>,
    ) -> TrainingOutcome;
}

/// Runs `hooks` in order until one handles the move; returns whether one did.
pub fn run_hooks(
    hooks: &mut [&mut dyn TrainingHook],
    state: &mut AppState,
    effects: &mut Vec<Effect>,
) -> bool {
    for hook in hooks {
        if hook.after_user_move(state, effects) == TrainingOutcome::Handled {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorder<'a> {
        name: &'static str,
        outcome: TrainingOutcome,
        log: &'a std::cell::RefCell<Vec<&'static str>>,
    }

    impl TrainingHook for Recorder<'_> {
        fn after_user_move(&mut self, _: &mut AppState, _: &mut Vec<Effect>) -> TrainingOutcome {
            self.log.borrow_mut().push(self.name);
            self.outcome
        }
    }

    #[test]
    fn hooks_run_in_order_until_one_handles_the_move() {
        let log = std::cell::RefCell::new(Vec::new());
        let mut automove = Recorder {
            name: "automove",
            outcome: TrainingOutcome::Continue,
            log: &log,
        };
        let mut tablebase = Recorder {
            name: "tablebase",
            outcome: TrainingOutcome::Handled,
            log: &log,
        };
        let mut tree = Recorder {
            name: "tree",
            outcome: TrainingOutcome::Continue,
            log: &log,
        };
        let mut state = AppState::default();
        let mut effects = Vec::new();
        let handled = run_hooks(
            &mut [&mut automove, &mut tablebase, &mut tree],
            &mut state,
            &mut effects,
        );
        assert!(handled);
        assert_eq!(*log.borrow(), ["automove", "tablebase"]);
    }

    #[test]
    fn nobody_handling_it_is_reported() {
        let log = std::cell::RefCell::new(Vec::new());
        let mut only = Recorder {
            name: "tree",
            outcome: TrainingOutcome::Continue,
            log: &log,
        };
        let mut state = AppState::default();
        assert!(!run_hooks(&mut [&mut only], &mut state, &mut Vec::new()));
    }
}
