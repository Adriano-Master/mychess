//! The ready-to-draw evaluation bar (spec M4-5, D451-D454).

use mychess_engine::Bound;

use super::mapping::{EvalValue, bar_fill, bar_ticks};
use super::prefs::EvalBarPrefs;
use crate::engines::RunId;

/// Where the value of the bar comes from (D452).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvalSource {
    /// The main engine, line 1 of its search.
    Engine(RunId),
    /// The score written in the comment of the current move.
    Comment,
    /// There is nothing to show: the bar is an empty outline (Scid `clear`).
    #[default]
    None,
}

/// What the bar shows; the front-end draws it without interpreting the
/// score.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EvalBarView {
    /// The origin of the value.
    pub source: EvalSource,
    /// The value, from White's point of view; `None` when the bar is empty.
    pub value: Option<EvalValue>,
    /// Whether the score is exact or a bound.
    pub bound: Bound,
    /// The value belongs to the previous position: draw it faded until the
    /// engine reports on the new one (D452).
    pub stale: bool,
    /// White's share of the bar, `0.0..=1.0`, measured from the bottom edge
    /// when White is at the bottom; `None` when the bar is empty.
    pub fill: Option<f32>,
    /// The board is turned: White's share is at the top.
    pub flipped: bool,
    /// The text next to the bar (`+0.35`, `M3`); `None` when empty or when
    /// the label is turned off.
    pub label: Option<String>,
    /// The label sits over the white part of the bar (dark text), else over
    /// the black part (light text): it goes where the side that is ahead is.
    pub label_on_white: bool,
    /// Positions of the ticks, `0.0..=1.0` as `fill`; empty when off.
    pub ticks: Vec<f32>,
}

impl EvalBarView {
    /// The view of a bar with nothing to show.
    pub fn empty(flipped: bool) -> EvalBarView {
        EvalBarView {
            flipped,
            ..EvalBarView::default()
        }
    }

    /// Builds the view of `value` with the options of the bar.
    pub fn new(
        source: EvalSource,
        value: Option<(EvalValue, Bound)>,
        stale: bool,
        prefs: EvalBarPrefs,
        flipped: bool,
    ) -> EvalBarView {
        let Some((value, bound)) = value else {
            return EvalBarView::empty(flipped);
        };
        let fill = bar_fill(value, prefs.scale, prefs.log);
        EvalBarView {
            source,
            value: Some(value),
            bound,
            stale,
            fill: Some(fill),
            flipped,
            label: prefs.label.then(|| value.label(bound)),
            label_on_white: fill >= 0.5,
            ticks: if prefs.ticks {
                bar_ticks(prefs.scale, prefs.log)
            } else {
                Vec::new()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bar_has_no_fill_and_no_label() {
        let v = EvalBarView::new(
            EvalSource::Comment,
            None,
            false,
            EvalBarPrefs::default(),
            true,
        );
        assert_eq!(v.fill, None);
        assert!(v.label.is_none() && v.flipped && v.value.is_none());
        assert_eq!(v.source, EvalSource::None);
    }

    #[test]
    fn view_carries_fill_label_and_ticks() {
        let prefs = EvalBarPrefs {
            ticks: true,
            ..EvalBarPrefs::default()
        };
        let v = EvalBarView::new(
            EvalSource::Comment,
            Some((EvalValue::Pawns(-1.0), Bound::Lower)),
            true,
            prefs,
            false,
        );
        assert!((v.fill.unwrap_or(0.0) - 0.4).abs() < 1e-6);
        assert_eq!(v.label.as_deref(), Some("\u{2265}-1.00"));
        assert!(!v.label_on_white && v.stale);
        assert_eq!(v.ticks.len(), 9);
        let no_label = EvalBarPrefs {
            label: false,
            ..prefs
        };
        let v = EvalBarView::new(
            EvalSource::Comment,
            Some((EvalValue::Pawns(1.0), Bound::Exact)),
            false,
            no_label,
            false,
        );
        assert!(v.label.is_none() && v.label_on_white);
    }
}
