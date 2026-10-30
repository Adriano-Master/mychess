//! Autoplay: steps forward on a timer (spec rules 26-31).
//!
//! The timer is an [`Effect`]; stopping is done by bumping `generation`, so a
//! tick already in flight is ignored when it arrives.

use std::time::Duration;

use crate::effect::Effect;

use super::GameSession;

/// Shortest autoplay delay (Scid: 100 ms).
pub const MIN_AUTOPLAY_DELAY: Duration = Duration::from_millis(100);
/// Longest autoplay delay (Scid: 300 s).
pub const MAX_AUTOPLAY_DELAY: Duration = Duration::from_secs(300);

/// The autoplay state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autoplay {
    /// Autoplay is running.
    pub running: bool,
    /// Wait between steps.
    pub delay: Duration,
    /// Identifies the run a scheduled tick belongs to.
    pub generation: u64,
}

impl Autoplay {
    pub(super) fn new(delay: Duration) -> Autoplay {
        Autoplay {
            running: false,
            delay: delay.clamp(MIN_AUTOPLAY_DELAY, MAX_AUTOPLAY_DELAY),
            generation: 0,
        }
    }
}

impl GameSession {
    pub(super) fn toggle_autoplay(&mut self, effects: &mut Vec<Effect>) {
        if self.autoplay.running {
            self.stop_autoplay();
        } else {
            self.start_autoplay(effects);
        }
    }

    pub(super) fn start_autoplay(&mut self, effects: &mut Vec<Effect>) {
        if self.autoplay.running || self.prompt.is_some() {
            return;
        }
        self.autoplay.running = true;
        self.autoplay_step();
        self.schedule_or_finish(effects);
    }

    pub(super) fn stop_autoplay(&mut self) {
        self.autoplay.running = false;
        self.autoplay.generation = self.autoplay.generation.wrapping_add(1);
    }

    pub(super) fn set_autoplay_delay(&mut self, delay: Duration) {
        self.autoplay.delay = delay.clamp(MIN_AUTOPLAY_DELAY, MAX_AUTOPLAY_DELAY);
    }

    pub(super) fn autoplay_tick(&mut self, generation: u64, effects: &mut Vec<Effect>) {
        if !self.autoplay.running || generation != self.autoplay.generation || self.prompt.is_some()
        {
            return;
        }
        self.autoplay_step();
        self.schedule_or_finish(effects);
    }

    /// One step: the first continuation, never opening the chooser.
    fn autoplay_step(&mut self) {
        if let Some(&next) = self.children_of(self.cursor).first() {
            self.move_to(next);
        }
    }

    /// Stops at a leaf; otherwise schedules the next tick.
    fn schedule_or_finish(&mut self, effects: &mut Vec<Effect>) {
        if self.children_of(self.cursor).is_empty() {
            self.autoplay.running = false;
        } else {
            self.autoplay.generation = self.autoplay.generation.wrapping_add(1);
            effects.push(Effect::ScheduleAutoplayTick {
                generation: self.autoplay.generation,
                delay: self.autoplay.delay,
            });
        }
    }
}
