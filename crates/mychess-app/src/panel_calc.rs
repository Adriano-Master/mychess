//! Bookkeeping shared by the panels of spec M6-4 that compute over a
//! database off the interface thread: the crosstable, the statistics and
//! graphs of the filter, the rating graph and the piece tracker.
//!
//! Each panel keeps one [`Refresh`]: the key of the last computation it was
//! asked for, the computation in flight (with the [`OpenControl`] that
//! reports its progress and cancels it) and the pending debounce. The
//! decision "start now, wait or do nothing" is pure ([`Refresh::decide`]) and
//! is tested here; the panels only turn it into effects (D3352).

use crate::bases::{BaseSlot, OpenControl};
use crate::error::RequestId;

/// A computation in flight.
#[derive(Debug)]
pub(crate) struct Calc<K> {
    /// Correlates the answer; a different request is a stale answer.
    pub(crate) request: RequestId,
    /// Progress and cancellation.
    pub(crate) control: OpenControl,
    /// What the computation was asked for.
    pub(crate) key: K,
}

/// What a panel does about a key it has just computed from the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The key is the one already asked for (or the panel is unusable).
    Idle,
    /// Start the computation now.
    Start,
    /// Wait for the burst of changes to end (the caller sends the delay).
    Debounce,
}

/// The state of the automatic refresh of one panel.
#[derive(Debug)]
pub(crate) struct Refresh<K> {
    /// The key of the last computation asked for.
    pub(crate) last_key: Option<K>,
    /// The computation in flight, if any.
    pub(crate) calc: Option<Calc<K>>,
    /// The debounce in flight: the request its delay will answer with.
    pub(crate) delay: Option<RequestId>,
    /// The next change starts at once (the panel was just opened or the user
    /// asked for "Refresh"), instead of waiting for the debounce.
    pub(crate) immediate: bool,
}

impl<K> Default for Refresh<K> {
    fn default() -> Refresh<K> {
        Refresh {
            last_key: None,
            calc: None,
            delay: None,
            immediate: true,
        }
    }
}

impl<K: PartialEq + Clone> Refresh<K> {
    /// Decides what to do about `key`, the key the state asks for now
    /// (`None`: there is nothing to compute from). A changed key cancels the
    /// computation in flight: its answer is dropped by the request.
    pub(crate) fn decide(&mut self, key: Option<K>, has_result: bool) -> Step {
        let Some(key) = key else {
            self.cancel();
            self.last_key = None;
            return Step::Idle;
        };
        if self.last_key.as_ref() == Some(&key) {
            return Step::Idle;
        }
        self.last_key = Some(key);
        self.cancel_calc();
        self.delay = None;
        if self.immediate || !has_result {
            self.immediate = false;
            Step::Start
        } else {
            Step::Debounce
        }
    }

    /// Whether the key asked for last is the one `same` recognises. Lets a
    /// panel skip building a key (allocations) when nothing relevant
    /// changed: [`Refresh::decide`] would answer [`Step::Idle`] (D3421).
    pub(crate) fn is_current(&self, same: impl FnOnce(&K) -> bool) -> bool {
        self.last_key.as_ref().is_some_and(same)
    }

    /// Forces the next [`Refresh::decide`] to start at once.
    pub(crate) fn force(&mut self) {
        self.last_key = None;
        self.immediate = true;
    }

    /// The user cancelled the computation in flight: it and the debounce
    /// stop, and `last_key` stays, so nothing starts again until the user
    /// asks (Refresh) or the key changes (D3422).
    pub(crate) fn user_cancel(&mut self) {
        self.cancel();
    }

    /// Stops the computation in flight and the pending debounce.
    pub(crate) fn cancel(&mut self) {
        self.cancel_calc();
        self.delay = None;
    }

    /// Asks the computation in flight to stop but keeps it, so that its
    /// (partial) answer is still the one in flight (the "Stop" button of the
    /// piece tracker, P12).
    pub(crate) fn stop(&self) {
        if let Some(calc) = &self.calc {
            calc.control.cancel();
        }
    }

    /// Stops the computation in flight and forgets it: its answer is dropped.
    pub(crate) fn cancel_calc(&mut self) {
        if let Some(calc) = self.calc.take() {
            calc.control.cancel();
        }
    }

    /// Records the computation just sent.
    pub(crate) fn begin(&mut self, request: RequestId, control: OpenControl, key: K) {
        self.delay = None;
        self.calc = Some(Calc {
            request,
            control,
            key,
        });
    }

    /// Takes the computation `request` answers, if it is the one in flight.
    pub(crate) fn finish(&mut self, request: RequestId) -> Option<Calc<K>> {
        if self.calc.as_ref().map(|c| c.request) == Some(request) {
            self.calc.take()
        } else {
            None
        }
    }

    /// `(done, total)` of the computation in flight.
    pub(crate) fn progress(&self) -> Option<(u64, u64)> {
        self.calc.as_ref().map(|c| c.control.progress())
    }

    /// Whether a computation is in flight.
    pub(crate) fn running(&self) -> bool {
        self.calc.is_some()
    }

    /// Whether the debounce `request` is the pending one (and ends it).
    pub(crate) fn delay_elapsed(&mut self, request: RequestId) -> bool {
        if self.delay == Some(request) {
            self.delay = None;
            true
        } else {
            false
        }
    }
}

/// What a panel keys on to know that the filter or the games of the slot
/// changed: two counters of the slot and the count of the filter as a
/// safety net. Constant time (D3421).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FilterStamp {
    count: u32,
    filter_rev: u64,
    data_rev: u64,
}

impl FilterStamp {
    /// The stamp of `slot` now.
    pub(crate) fn of(slot: &BaseSlot) -> FilterStamp {
        FilterStamp {
            count: slot.filtered(),
            filter_rev: slot.filter_rev(),
            data_rev: slot.data_rev(),
        }
    }

    /// The same stamp without the filter: for a computation that reads the
    /// whole database and only needs to know that the games changed.
    #[must_use]
    pub(crate) fn data_only(self) -> FilterStamp {
        FilterStamp {
            count: 0,
            filter_rev: 0,
            data_rev: self.data_rev,
        }
    }
}

/// Declares a plain enum with the spelling used in the preferences file:
/// `ALL`, `name()` and `from_name()`. Exactly one variant is `#[default]`.
macro_rules! named_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        $vis enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every value, in the order a list shows them.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The spelling used in the preferences file.
            pub fn name(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// Reads the spelling used in the preferences file.
            pub fn from_name(text: &str) -> Option<$name> {
                match text { $($text => Some($name::$variant),)+ _ => None }
            }
        }
    };
}
pub(crate) use named_enum;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_key_starts_at_once() {
        let mut r = Refresh::<u32>::default();
        assert_eq!(r.decide(Some(1), false), Step::Start);
        // The same key again does nothing.
        assert_eq!(r.decide(Some(1), true), Step::Idle);
    }

    #[test]
    fn a_change_after_a_result_waits_for_the_debounce() {
        let mut r = Refresh::<u32>::default();
        assert_eq!(r.decide(Some(1), false), Step::Start);
        assert_eq!(r.decide(Some(2), true), Step::Debounce);
        // No result yet: do not wait.
        let mut r = Refresh::<u32> {
            immediate: false,
            ..Refresh::default()
        };
        assert_eq!(r.decide(Some(2), false), Step::Start);
    }

    #[test]
    fn a_new_key_cancels_the_computation_in_flight() {
        let mut r = Refresh::<u32>::default();
        assert_eq!(r.decide(Some(1), false), Step::Start);
        let control = OpenControl::new();
        r.begin(RequestId(1), control.clone(), 1);
        assert!(r.running());
        assert_eq!(r.decide(Some(2), true), Step::Debounce);
        assert!(control.is_cancelled());
        assert!(!r.running());
        // The old answer is not the one in flight.
        assert!(r.finish(RequestId(1)).is_none());
    }

    #[test]
    fn force_starts_at_once_even_with_the_same_key() {
        let mut r = Refresh::<u32>::default();
        assert_eq!(r.decide(Some(1), false), Step::Start);
        r.force();
        assert_eq!(r.decide(Some(1), true), Step::Start);
    }

    #[test]
    fn no_key_cancels_and_forgets() {
        let mut r = Refresh::<u32>::default();
        assert_eq!(r.decide(Some(1), false), Step::Start);
        let control = OpenControl::new();
        r.begin(RequestId(1), control.clone(), 1);
        assert_eq!(r.decide(None, true), Step::Idle);
        assert!(control.is_cancelled());
        assert!(r.last_key.is_none());
    }

    #[test]
    fn only_the_pending_delay_counts() {
        let mut r = Refresh::<u32> {
            delay: Some(RequestId(7)),
            ..Refresh::default()
        };
        assert!(!r.delay_elapsed(RequestId(6)));
        assert!(r.delay_elapsed(RequestId(7)));
        assert!(!r.delay_elapsed(RequestId(7)));
    }

    #[test]
    fn the_stamp_follows_the_counters_of_the_slot() {
        let mut slot = BaseSlot::clipbase(10).expect("clipbase");
        let first = FilterStamp::of(&slot);
        assert_eq!(first, FilterStamp::of(&slot));
        slot.filter_changed();
        let second = FilterStamp::of(&slot);
        assert_ne!(first, second, "a change of the filter");
        assert_eq!(first.data_only(), second.data_only(), "same games");
        slot.clear_sort();
        let third = FilterStamp::of(&slot);
        assert_ne!(second.data_only(), third.data_only(), "other games");
    }
}
