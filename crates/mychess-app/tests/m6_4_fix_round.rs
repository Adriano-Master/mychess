//! Tests of the correction round of the M6-4 controller (decisions D3421 to
//! D3428): the revision counters, the cancel buttons, the options that start
//! at once and the small fixes.
#![allow(
    let_underscore_drop,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_4_common;

use m6_4_common::*;
use mychess_app::charts::ChartsNotice;
use mychess_app::crosstable::{CrosstableBody, CrosstableNotice};

fn has_stats(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::Charts(ChartsEffect::Stats { .. })))
}

fn has_rating(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::Charts(ChartsEffect::Rating { .. })))
}

fn has_compute(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::Crosstable(CrosstableEffect::Compute { .. })))
}

fn has_track(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::PieceTracker(PieceTrackerEffect::Track { .. })))
}

fn has_delay(effects: &[Effect]) -> bool {
    effects.iter().any(|e| {
        matches!(
            e,
            Effect::Crosstable(CrosstableEffect::Delay { .. })
                | Effect::Charts(ChartsEffect::Delay { .. })
                | Effect::PieceTracker(PieceTrackerEffect::Delay { .. })
        )
    })
}

#[test]
fn the_slot_counts_the_changes_of_its_filter() {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter_rev();
    rig.send(BasesMessage::List(ListMessage::NegateFilter { slot }));
    let after = rig.state.bases.slot(slot).unwrap().filter_rev();
    assert_ne!(before, after, "a change of the filter bumps the counter");
    // A message that does not touch the filter leaves it alone.
    rig.send(CrosstableMessage::Open);
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter_rev(), after);
}

#[test]
fn a_panel_follows_the_filter_with_the_counter_and_is_idle_otherwise() {
    let (mut rig, slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    assert!(has_stats(&rig.raw(ChartsMessage::OpenStats)));
    // Nothing changed: no new computation, on every update.
    assert!(!has_stats(&rig.raw(CrosstableMessage::Open)));
    assert!(!has_stats(&rig.raw(ChartsMessage::SetGraphDots(true))));
    // The filter changed: the table is asked for again (after the debounce
    // because there is a result).
    let effects = rig.send(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(effects.iter().all(|e| !has_stats(std::slice::from_ref(e))));
    rig.fire_delays();
    assert_eq!(rig.state.stats_view().filter_games, 0);
    assert!(!rig.state.stats_view().stale);
}

#[test]
fn cancelling_the_crosstable_leaves_an_idle_panel_that_can_try_again() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(CrosstableMessage::Open);
    assert!(has_compute(&effects));
    rig.raw(CrosstableMessage::Cancel);
    let view = rig.state.crosstable_view();
    assert_eq!(
        view.body,
        CrosstableBody::Empty(CrosstableNotice::Cancelled)
    );
    assert!(view.progress.is_none());
    assert!(!rig.state.crosstable().is_running());
    // Updates do not start it again by themselves...
    assert!(!has_compute(&rig.raw(ChartsMessage::SetGraphDots(true))));
    // ...but Refresh does.
    assert!(has_compute(&rig.raw(CrosstableMessage::Refresh)));
}

#[test]
fn cancelling_the_statistics_says_so_and_waits_for_a_refresh() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    assert!(has_stats(&rig.raw(ChartsMessage::OpenStats)));
    rig.raw(ChartsMessage::CancelStats);
    let view = rig.state.stats_view();
    assert_eq!(view.issue, Some(ChartsNotice::Cancelled));
    assert!(view.progress.is_none());
    assert!(!has_stats(&rig.raw(ChartsMessage::SetGraphDots(true))));
    assert!(has_stats(&rig.raw(ChartsMessage::RefreshStats)));
    // A new computation clears the remark.
    assert_eq!(rig.state.stats_view().issue, None);
}

#[test]
fn cancelling_the_rating_graph_says_so_and_waits_for_a_refresh() {
    let (mut rig, _slot, _dir) = sample();
    assert!(has_rating(&rig.raw(ChartsMessage::OpenRating)));
    rig.raw(ChartsMessage::CancelRating);
    let view = rig.state.rating_view();
    assert_eq!(view.issue, Some(ChartsNotice::Cancelled));
    assert!(view.progress.is_none());
    assert!(!has_rating(&rig.raw(ChartsMessage::SetRatingDots(true))));
    assert!(has_rating(&rig.raw(ChartsMessage::RefreshRating)));
}

#[test]
fn a_change_of_the_statistics_options_starts_at_once() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(2026));
    rig.send(ChartsMessage::OpenStats);
    rig.delays.clear();
    let effects = rig.raw(ChartsMessage::SetStatsElo(2400, false));
    assert!(has_stats(&effects), "{effects:?}");
    assert!(!has_delay(&effects));
    let effects = rig.raw(ChartsMessage::ResetStats);
    assert!(has_stats(&effects), "{effects:?}");
}

#[test]
fn the_options_wait_for_the_year() {
    let (mut rig, _slot, _dir) = sample();
    rig.raw(ChartsMessage::OpenStats);
    let before = (*rig.state.prefs.stats).clone();
    rig.raw(ChartsMessage::SetStatsElo(2400, false));
    rig.raw(ChartsMessage::SetStatsYear(2012, false));
    assert_eq!(*rig.state.prefs.stats, before);
}

#[test]
fn an_absurd_year_is_kept_in_range() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(ChartsMessage::SetYear(u16::MAX));
    let effects = rig.raw(ChartsMessage::OpenStats);
    let Some(Effect::Charts(ChartsEffect::Stats { config, .. })) = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Charts(ChartsEffect::Stats { .. })))
    else {
        panic!("a stats effect");
    };
    assert!(config.years.keys().all(|y| *y <= 2999));
}

#[test]
fn the_piece_tracker_counts_at_once_for_an_option_but_waits_for_the_filter() {
    use mychess_core::Square;
    let (mut rig, slot, _dir) = sample();
    rig.send(PieceTrackerMessage::Open);
    let effects = rig.raw(PieceTrackerMessage::SelectPiece {
        square: Square::G1,
        sister: false,
    });
    assert!(has_track(&effects), "{effects:?}");
    assert!(!has_delay(&effects));
    let effects = rig.raw(PieceTrackerMessage::SetMode(
        mychess_app::piece_tracker::TrackerMode::Time,
    ));
    assert!(has_track(&effects), "{effects:?}");
    // A change of the filter is not a click on an option: it waits.
    let effects = rig.raw(BasesMessage::List(ListMessage::NegateFilter { slot }));
    assert!(!has_track(&effects));
    assert!(has_delay(&effects), "{effects:?}");
}

#[test]
fn the_view_of_the_table_is_shared_between_calls() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    let (CrosstableBody::Table(a), CrosstableBody::Table(b)) = (
        rig.state.crosstable_view().body,
        rig.state.crosstable_view().body,
    ) else {
        panic!("a table");
    };
    assert!(std::sync::Arc::ptr_eq(&a, &b), "converted once (D3425)");
}

#[test]
fn changing_the_event_in_the_editor_restarts_the_crosstable() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(CrosstableMessage::Open);
    assert_eq!(table(&rig).event, "Club");
    rig.send(mychess_app::GameInfoMessage::Open);
    rig.send(mychess_app::GameInfoMessage::SetText(
        mychess_app::TagField::Event,
        "Open".to_owned(),
    ));
    rig.send(mychess_app::GameInfoMessage::Apply);
    rig.fire_delays();
    // The editor's Event is the key: the old table is gone (Open/Rio has no
    // other game, so the answer is "no table").
    assert!(
        !matches!(rig.state.crosstable_view().body, CrosstableBody::Table(_)),
        "the table follows the editor: {:?}",
        rig.state.crosstable_view().body
    );
}

fn table(rig: &Rig) -> mychess_app::crosstable::CrossTableView {
    match rig.state.crosstable_view().body {
        CrosstableBody::Table(t) => (*t).clone(),
        other => panic!("not a table: {other:?}"),
    }
}
