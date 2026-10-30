//! M5-2a contract tests of "adjust filter" (D967, D979) and of what happens
//! to the engine state of the slots (D961, D977).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use tempfile::tempdir;

/// The std base with the tree open and the cursor after 1.e4 (games 1, 3 and
/// 5 reach it).
fn after_e4(dir: &std::path::Path) -> (Rig, SlotId) {
    let mut rig = Rig::std_with_tree(dir);
    let slot = rig.state.bases.current();
    rig.play("e2e4");
    (rig, slot)
}

#[test]
fn adjust_leaves_in_the_filter_only_the_games_that_reach_the_position() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    assert_eq!(rig.filtered(slot), 5);
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
    assert_eq!(rig.filtered(slot), 3);
    assert!(rig.view().adjust);
    assert!(rig.state.prefs.tree.auto_adjust);
}

#[test]
fn the_filter_value_of_each_game_is_the_ply_of_the_match_plus_one() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    let filter = rig.state.bases.slot(slot).unwrap().filter().clone();
    for n in [1, 3, 5] {
        assert_eq!(filter.get(n), 2, "game {n}: position at ply 1");
    }
    assert_eq!(filter.get(2), 0);
}

#[test]
fn the_list_is_positioned_at_the_end_of_the_adjusted_filter() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    let first = rig.state.bases.slot(slot).unwrap().list.first;
    assert_eq!(first, 3, "the last game of the filter is in the page");
}

#[test]
fn the_adjusted_filter_is_the_intersection_with_the_filter_of_the_base() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    // The user keeps games 1, 2 and 3 before adjusting.
    rig.keep(slot, &[1, 2, 3]);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3]);
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 3], "5 reaches e4 but was out");
}

#[test]
fn the_adjusted_filter_follows_the_position() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    rig.play("e7e5");
    assert_eq!(rig.filter_numbers(slot), [1]);
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter().get(1), 3);
    rig.nav(Nav::Start);
    assert_eq!(rig.filtered(slot), 5, "the start position is in every game");
}

#[test]
fn an_empty_tree_adjusts_to_an_empty_filter() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    rig.play("a7a6");
    assert_eq!(rig.view().body, TreeBody::Empty);
    assert_eq!(rig.filtered(slot), 0);
}

#[test]
fn closing_the_panel_gives_the_old_filter_back() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.keep(slot, &[1, 2, 3]);
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filtered(slot), 2);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3], "the filter of before");
}

#[test]
fn turning_adjust_off_confirms_the_current_filter_as_the_base_one() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    rig.send(TreeMessage::SetAdjust(false));
    assert!(!rig.view().adjust);
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
    // The filter no longer follows the cursor.
    rig.play("e7e5");
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
    // And closing does not undo it.
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
}

#[test]
fn closing_without_adjust_never_touches_the_filter_the_user_made() {
    // The tree keeps a copy of the filter of the first search; closing must
    // not put it back unless the slot was really adjusted.
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.keep(slot, &[2, 4]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filter_numbers(slot), [2, 4]);
}

#[test]
fn turning_adjust_on_and_off_without_a_search_in_between_is_harmless() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    rig.send(TreeMessage::SetAdjust(false));
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
    rig.send(TreeMessage::Close);
    // Off then on again: the base is the one confirmed when it was turned
    // off, which is the adjusted filter.
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
}

#[test]
fn setting_adjust_to_the_current_value_does_nothing() {
    let dir = tempdir().unwrap();
    let (mut rig, _slot) = after_e4(dir.path());
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetAdjust(false));
    assert!(effects.is_empty());
    assert_eq!(rig.started(), started);
}

#[test]
fn turning_adjust_on_searches_again_with_the_new_option() {
    let dir = tempdir().unwrap();
    let (mut rig, _slot) = after_e4(dir.path());
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetAdjust(true));
    assert_eq!(tree_searches(&effects), 1);
    assert_eq!(rig.started(), started + 1);
}

#[test]
fn reopening_with_the_preference_on_adjusts_again() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filtered(slot), 5);
    rig.send(TreeMessage::Open);
    assert!(rig.state.prefs.tree.auto_adjust);
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
}

#[test]
fn adjust_works_on_an_si4_base_too() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_si4(dir.path(), "std", STD);
    rig.open_tree();
    rig.play("e2e4");
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filter_numbers(slot), [1, 3, 5]);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filtered(slot), 5);
}

// ---- the state is out in a search (Pending) ----

#[test]
fn closing_while_the_adjusted_state_is_out_restores_when_it_returns() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filtered(slot), 3);
    // A new position starts a search; the state of the slot is out.
    let mv = rig.state.session.position().parse_uci("e7e5").unwrap();
    let mut flight = jobs(rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }));
    assert_eq!(flight.len(), 1);
    rig.raw(TreeMessage::Close);
    // The answer comes back: the tree is ended and the filter restored.
    let answer = rig.run_job(flight.remove(0));
    rig.raw(answer);
    assert_eq!(rig.filtered(slot), 5);
    assert!(!rig.state.tree().searching());
}

#[test]
fn turning_adjust_off_and_on_while_the_state_is_out_leaves_a_consistent_filter() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    // A search that nobody answers yet.
    let mut flight = jobs(rig.raw(TreeMessage::SetAdjust(true)));
    assert_eq!(flight.len(), 1);
    // Off again before it returns.
    let effects = rig.raw(TreeMessage::SetAdjust(false));
    assert!(effects.is_empty(), "waits for the one in flight");
    let answer = rig.run_job(flight.remove(0));
    let mut queue: std::collections::VecDeque<Effect> = rig.raw(answer).into();
    while let Some(effect) = queue.pop_front() {
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
    // Nothing adjusted the filter: it is the full one, and stays so.
    assert_eq!(rig.filtered(slot), 5);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filtered(slot), 5);
    assert!(!rig.state.tree().searching());
}

#[test]
fn a_running_list_job_defers_the_adjusted_search_until_it_ends() {
    let dir = tempdir().unwrap();
    let (mut rig, slot) = after_e4(dir.path());
    // A text filter job on the list, left running.
    rig.send(BasesMessage::SetSearchText {
        slot,
        text: "W".to_owned(),
    });
    let job = rig.raw(BasesMessage::List(ListMessage::FilterByText { slot }));
    assert!(rig.state.list_job().is_some(), "precondition: a job runs");
    assert!(!job.is_empty());
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetAdjust(true));
    assert_eq!(tree_searches(&effects), 0, "waits for the list job");
    assert_eq!(rig.started(), started);
    // The job ends; the search with adjust follows.
    let mut queue: std::collections::VecDeque<Effect> = job.into();
    while let Some(effect) = queue.pop_front() {
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
    assert!(rig.state.list_job().is_none());
    assert_eq!(rig.started(), started + 1);
    assert!(rig.state.tree().is_open());
}

// ---- slots ----

#[test]
fn each_slot_keeps_its_own_adjusted_state_and_closing_restores_all_of_them() {
    let dir = tempdir().unwrap();
    let (mut rig, first) = after_e4(dir.path());
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filtered(first), 3);
    // A second base becomes current: the tree follows and adjusts it too.
    let other = "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n[Result \"1-0\"]\n\n1. d4 d5 1-0\n\n";
    let second = rig.open_pgn(dir.path(), "other.pgn", other);
    assert_ne!(first, second);
    // The new base starts with a game at the start position.
    assert_eq!(rig.sorted_sans(), ["d4", "e4"]);
    assert_eq!(rig.filter_numbers(second), [1, 2]);
    assert_eq!(
        rig.filtered(first),
        3,
        "the first slot keeps its adjusted filter while it is not current"
    );
    rig.send(TreeMessage::Close);
    assert_eq!(rig.filtered(first), 5, "restored even if not current");
    assert_eq!(rig.filtered(second), 2);
}

#[test]
fn going_back_to_a_slot_searches_it_again_with_its_own_base() {
    let dir = tempdir().unwrap();
    let (mut rig, first) = after_e4(dir.path());
    let other = "[Result \"1-0\"]\n\n1. e4 c5 1-0\n\n";
    rig.open_pgn(dir.path(), "other.pgn", other);
    assert_eq!(
        rig.sans(),
        ["e4"],
        "the game of the new slot is at the start"
    );
    rig.send(BasesMessage::Select(first));
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"]);
}

// ---- identity of the slot (D977) ----

#[test]
fn closing_a_slot_whose_filter_was_adjusted_does_not_panic_and_the_tree_goes_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let first = rig.state.bases.current();
    rig.send(TreeMessage::SetAdjust(true));
    rig.send(BasesMessage::Close(first));
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    assert!(!rig.state.tree().searching());
    rig.send(TreeMessage::Close);
    assert_eq!(rig.view().body, TreeBody::Closed);
}

#[test]
fn a_base_that_grows_is_searched_again_and_its_state_is_not_reused() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let source = rig.state.bases.current();
    let clip = sel(9);
    rig.send(Message::Prefs(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::ConfirmCopy(false),
    )));
    let copy = |rig: &mut Rig| {
        rig.send(BasesMessage::List(ListMessage::CopyGames {
            from: source,
            to: clip,
            scope: mychess_app::CopyScope::Filter,
        }));
    };
    copy(&mut rig);
    rig.send(BasesMessage::Select(clip));
    rig.open_tree();
    rig.send(TreeMessage::SetAdjust(true));
    assert_eq!(rig.filtered(clip), 5);
    copy(&mut rig);
    // 10 games now; the adjusted search must cover all of them.
    assert_eq!(rig.state.bases.slot(clip).unwrap().total(), 10);
    assert_eq!(rig.state.bases.slot(clip).unwrap().filter().len(), 10);
    assert_eq!(rig.view().total.expect("total").games, 10);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.state.bases.slot(clip).unwrap().filter().len(), 10);
}
