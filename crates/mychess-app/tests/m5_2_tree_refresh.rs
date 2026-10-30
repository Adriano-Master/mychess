//! M5-2a contract tests of the refresh: when a search starts (D963, D978),
//! at most one in flight with generations (D962, D976), the search modes
//! (D964), the order, the inhibitions and the slot identity (D961, D977).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use std::collections::VecDeque;

use m5_2_common::*;
use mychess_app::{AnnotateMessage, PrefChange, PrefsMessage};
use tempfile::tempdir;

// ---- what triggers a search ----

#[test]
fn a_move_on_the_board_searches_the_new_position() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let effects = rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(tree_searches(&effects), 1);
    assert_eq!(rig.started(), started + 1);
}

#[test]
fn navigating_to_another_position_searches_again() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    let started = rig.started();
    let effects = rig.raw(GameMessage::Navigate(Nav::Start));
    assert_eq!(tree_searches(&effects), 1);
    assert_eq!(rig.started(), started + 1);
}

#[test]
fn navigating_to_the_same_place_does_not_search() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    let effects = rig.raw(GameMessage::Navigate(Nav::Start));
    assert_eq!(tree_searches(&effects), 0);
    assert_eq!(rig.started(), started);
}

#[test]
fn unrelated_messages_do_not_search() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    for message in [
        Message::Tree(TreeMessage::SetShort(false)),
        Message::Tree(TreeMessage::SetShort(true)),
        Message::Tree(TreeMessage::SetSort(TreeSort::Frequency)),
        Message::Tree(TreeMessage::SetFastMode(TreeFastMode::Slow)),
        Message::Tree(TreeMessage::RowClicked(99)),
    ] {
        let effects = update(&mut rig.state, message);
        assert_eq!(tree_searches(&effects), 0);
    }
    assert_eq!(rig.started(), started);
}

#[test]
fn editing_the_game_without_changing_the_cursor_position_does_not_search() {
    // D978: the key is the position, not the revision of the game.
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    let started = rig.started();
    let effects = rig.raw(GameMessage::AddNag(mychess_core::Nag::new(1).unwrap()));
    assert_eq!(tree_searches(&effects), 0, "{effects:?}");
    assert_eq!(rig.started(), started);
    let effects = rig.raw(GameMessage::SetComment {
        node: rig.state.session.cursor(),
        text: "x".to_owned(),
    });
    assert_eq!(tree_searches(&effects), 0);
    assert_eq!(rig.started(), started);
}

#[test]
fn another_game_at_the_same_position_does_not_search_but_another_position_does() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    rig.state.session.replace_game(mychess_core::Game::new());
    let effects = rig.raw(TreeMessage::SetShort(false));
    assert_eq!(tree_searches(&effects), 0, "same FEN, same tree");
    // A game whose cursor is on another position.
    rig.state.session.replace_game(game_of(&["d2d4"]));
    rig.state.session.update(GameMessage::Navigate(Nav::End));
    let effects = rig.raw(TreeMessage::SetShort(true));
    assert_eq!(tree_searches(&effects), 1);
    assert_eq!(rig.started(), started + 1);
}

#[test]
fn switching_the_slot_searches_the_new_base() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let first = rig.state.bases.current();
    let other = "[Result \"1-0\"]\n\n1. c4 e5 1-0\n\n";
    let second = rig.open_pgn(dir.path(), "other.pgn", other);
    assert_ne!(first, second);
    assert_eq!(rig.sans(), ["c4"], "the tree follows the current base");
    rig.send(BasesMessage::Select(first));
    assert_eq!(rig.sorted_sans(), ["Nf3", "d4", "e4"]);
    rig.send(BasesMessage::Select(second));
    assert_eq!(rig.sans(), ["c4"]);
}

#[test]
fn selecting_the_current_slot_again_does_not_search() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    let slot = rig.state.bases.current();
    let effects = rig.raw(BasesMessage::Select(slot));
    assert_eq!(tree_searches(&effects), 0);
    assert_eq!(rig.started(), started);
}

#[test]
fn a_closed_panel_never_searches_on_changes() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let effects = rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(tree_searches(&effects), 0);
    assert_eq!(rig.started(), 0);
}

#[test]
fn growing_the_base_refreshes_the_tree() {
    // D977: the key holds the number of games.
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let source = rig.state.bases.current();
    let clip = sel(9);
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::ConfirmCopy(
        false,
    ))));
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: clip,
        scope: mychess_app::CopyScope::Filter,
    }));
    rig.send(BasesMessage::Select(clip));
    rig.open_tree();
    assert_eq!(rig.view().total.expect("total").games, 5);
    // Copy again: the clipbase grows from 5 to 10 games, same position.
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: clip,
        scope: mychess_app::CopyScope::Filter,
    }));
    assert_eq!(
        rig.state.bases.slot(clip).expect("clip").total(),
        10,
        "precondition"
    );
    assert_eq!(rig.state.bases.current(), clip);
    assert_eq!(rig.view().total.expect("total").games, 10);
}

// ---- inhibitions and "Refresh" off ----

#[test]
fn with_refresh_off_the_table_stays_and_clicks_are_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let table = rig.view().rows;
    rig.send(TreeMessage::ToggleAutoRefresh);
    assert!(!rig.view().autorefresh);
    let started = rig.started();
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let effects = rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(tree_searches(&effects), 0);
    assert_eq!(rig.started(), started);
    let view = rig.view();
    assert_eq!(view.rows.len(), table.len(), "the table is left as it was");
    assert!(
        view.rows.iter().all(|r| !r.clickable),
        "and nothing can be clicked"
    );
    let nodes = rig.nodes();
    let effects = rig.raw(TreeMessage::RowClicked(0));
    assert!(effects.is_empty());
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 1);
}

#[test]
fn turning_refresh_back_on_catches_up_with_the_position() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::ToggleAutoRefresh);
    rig.play("e2e4");
    assert_eq!(rig.view().rows.len(), 3, "still the start position's table");
    let effects = rig.raw(TreeMessage::ToggleAutoRefresh);
    assert_eq!(tree_searches(&effects), 1);
    let flight = jobs(effects);
    let answer = rig.run_job(flight.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"]);
}

#[test]
fn a_table_of_another_position_is_not_clickable_while_the_new_one_loads() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let first = rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(tree_searches(&first), 1, "a search is now running");
    let mv = rig.state.session.position().parse_uci("e7e5").unwrap();
    let effects = rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(effects.is_empty(), "queued behind the one in flight");
    assert!(rig.view().searching);
    assert!(rig.view().rows.iter().all(|r| !r.clickable));
}

/// A state with a UCI engine ready and the std base with its tree open.
fn annotating(dir: &std::path::Path) -> Rig {
    use mychess_app::engines::{EngineDraft, EngineMessage, StartMode};
    let mut rig = Rig::new();
    rig.open_pgn(dir, "std.pgn", STD);
    let engine = rig
        .state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .unwrap();
    let fx = rig.raw(Message::Engine(EngineMessage::Start(
        engine,
        StartMode::Analyze,
    )));
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    rig.raw(Message::Engine(EngineMessage::Started {
        run,
        identity: Box::new(mychess_engine::EngineIdentity {
            name: "Fake".to_owned(),
            author: None,
            options: Vec::new(),
            capabilities: mychess_engine::Capabilities::default(),
        }),
        at: 1_700_000_000,
    }));
    rig.state
        .session
        .replace_game(game_of(&["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"]));
    rig.state.session.update(GameMessage::Navigate(Nav::Start));
    rig
}

#[test]
fn an_annotation_run_inhibits_the_refresh_until_it_ends() {
    let dir = tempdir().unwrap();
    let mut rig = annotating(dir.path());
    rig.raw(AnnotateMessage::Start);
    assert!(rig.state.annotation_running(), "precondition");
    // Opening the panel while the run owns the game starts nothing.
    let effects = rig.raw(TreeMessage::Open);
    assert_eq!(tree_searches(&effects), 0, "inhibited");
    assert_eq!(rig.started(), 0);
    let effects = rig.raw(TreeMessage::SetSort(TreeSort::Score));
    assert_eq!(tree_searches(&effects), 0, "options do not break the rule");
    // When the run ends, the tree catches up.
    let effects = rig.raw(AnnotateMessage::Cancel);
    assert!(!rig.state.annotation_running());
    assert_eq!(tree_searches(&effects), 1, "{effects:?}");
}

// ---- generations, one search in flight ----

#[test]
fn several_changes_in_a_row_keep_one_search_in_flight_and_draw_only_the_last() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut first = jobs(rig.raw(TreeMessage::Open));
    assert_eq!(first.len(), 1);
    let g0 = rig.state.tree().generation();
    // Position changes while the first search runs.
    let mut spawned = 0;
    for uci in ["e2e4", "e7e5", "g1f3"] {
        let mv = rig.state.session.position().parse_uci(uci).unwrap();
        spawned += tree_searches(&rig.raw(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
    }
    assert_eq!(spawned, 0, "no new search while one is in flight");
    assert!(rig.state.tree().generation() > g0);
    assert!(rig.state.tree().searching());
    // The first one returns (cancelled): exactly one search follows, for the
    // last position.
    let answer = rig.run_job(first.remove(0));
    assert!(was_cancelled(&answer), "{answer:?}");
    let next = jobs(rig.raw(answer));
    assert_eq!(next.len(), 1);
    assert_eq!(rig.started(), 2, "the intermediate positions were skipped");
    let answer = rig.run_job(next.into_iter().next().unwrap());
    assert!(rig.raw(answer).is_empty());
    assert_eq!(rig.sans(), ["[end]"], "only game 1 reaches 1.e4 e5 2.Nf3");
    assert!(!rig.state.tree().searching());
}

#[test]
fn a_cancelled_answer_is_never_an_issue() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut first = jobs(rig.raw(TreeMessage::Open));
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    let answer = rig.run_job(first.remove(0));
    assert!(was_cancelled(&answer));
    rig.raw(answer);
    assert!(!matches!(rig.view().body, TreeBody::Issue(_)));
}

#[test]
fn a_cancelled_answer_with_nothing_wanted_next_does_not_leave_the_panel_stuck() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut flight = jobs(rig.raw(TreeMessage::Open));
    rig.raw(TreeMessage::Close);
    let answer = rig.run_job(flight.remove(0));
    rig.raw(answer);
    assert!(!rig.state.tree().searching());
    // And the panel works afterwards.
    rig.send(TreeMessage::Open);
    assert_eq!(rig.view().body, TreeBody::Rows);
}

#[test]
fn an_answer_delivered_twice_is_ignored_the_second_time() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut flight = jobs(rig.raw(TreeMessage::Open));
    let answer = rig.run_job(flight.remove(0));
    assert!(rig.raw(answer.clone()).is_empty());
    let before = rig.view();
    let started = rig.started();
    assert!(rig.raw(answer).is_empty());
    assert_eq!(rig.view(), before);
    assert_eq!(rig.started(), started);
}

#[test]
fn the_generation_grows_with_every_search_wanted_and_jobs_carry_it() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let flight = jobs(rig.raw(TreeMessage::Open));
    assert_eq!(flight[0].generation(), rig.state.tree().generation());
    let g = rig.state.tree().generation();
    rig.send(TreeMessage::Close);
    assert!(rig.state.tree().generation() > g, "closing makes it stale");
}

#[test]
fn the_job_names_the_slot_it_searches() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_pgn(dir.path(), "std.pgn", STD);
    let flight = jobs(rig.raw(TreeMessage::Open));
    assert_eq!(flight[0].slot(), slot);
}

#[test]
fn a_late_answer_for_a_base_that_was_replaced_is_not_shown() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut old = jobs(rig.raw(TreeMessage::Open));
    // The base goes away and another one takes its place while the search
    // runs.
    rig.send(BasesMessage::Close(slot));
    let other = "[Result \"1-0\"]\n\n1. c4 e5 1-0\n\n";
    rig.open_pgn(dir.path(), "other.pgn", other);
    assert!(rig.state.tree().searching(), "waiting for the old one");
    let answer = rig.run_job(old.remove(0));
    let next = jobs(rig.raw(answer));
    assert_eq!(next.len(), 1, "the new base is searched next");
    assert!(
        !rig.sans().contains(&"e4".to_owned()),
        "nothing of the old base: {:?}",
        rig.sans()
    );
    let answer = rig.run_job(next.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(rig.sans(), ["c4"]);
}

#[test]
fn quit_cancels_the_search_in_flight() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut flight = jobs(rig.raw(TreeMessage::Open));
    rig.raw(Message::Quit);
    let answer = rig.run_job(flight.remove(0));
    assert!(was_cancelled(&answer), "Quit must cancel the token");
}

#[test]
fn a_new_wish_cancels_the_token_of_the_search_in_flight() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut flight = jobs(rig.raw(TreeMessage::Open));
    rig.raw(TreeMessage::SetSort(TreeSort::Score));
    let answer = rig.run_job(flight.remove(0));
    assert!(was_cancelled(&answer));
}

// ---- order and modes ----

#[test]
fn the_order_refreshes_at_once_and_reorders_the_rows() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetSort(TreeSort::Alphabetical));
    assert_eq!(tree_searches(&effects), 1);
    let answer = rig.run_job(jobs(effects).remove(0));
    rig.raw(answer);
    assert_eq!(rig.started(), started + 1);
    // Byte order, upper case first (Scid strCompare).
    assert_eq!(rig.sans(), ["Nf3", "d4", "e4"]);
    assert_eq!(rig.view().sort, TreeSort::Alphabetical);
    assert_eq!(rig.state.prefs.tree.sort, TreeSort::Alphabetical);
}

#[test]
fn score_order_puts_the_best_for_white_first_when_white_moves() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::SetSort(TreeSort::Score));
    // Nf3 1000, e4 666, d4 0.
    assert_eq!(rig.sans(), ["Nf3", "e4", "d4"]);
}

#[test]
fn score_order_is_reversed_for_black_to_move() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    // After 1.e4: e5 (1-0), c5 (1/2), end of game (no result).
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.play("e2e4");
    rig.open_tree();
    rig.send(TreeMessage::SetSort(TreeSort::Score));
    let sans = rig.sans();
    let pos = |s: &str| sans.iter().position(|x| x == s).unwrap();
    // Black prefers the lowest White score: e5 (1000) is the worst for Black.
    assert!(
        pos("c5") < pos("[end]") && pos("[end]") < pos("e5"),
        "{sans:?}"
    );
}

#[test]
fn every_order_shows_the_same_rows() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    for sort in TreeSort::ALL {
        rig.send(TreeMessage::SetSort(sort));
        assert_eq!(rig.sorted_sans(), ["Nf3", "d4", "e4"], "{sort:?}");
        assert_eq!(rig.view().total.expect("total").games, 5);
    }
}

#[test]
fn the_fast_mode_refreshes_at_once() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetFastMode(TreeFastMode::Fast));
    assert_eq!(tree_searches(&effects), 1);
    assert_eq!(rig.started(), started + 1);
    assert_eq!(rig.view().fast_mode, TreeFastMode::Fast);
}

#[test]
fn the_first_search_is_complete_even_in_the_fast_mode() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(TreeMessage::SetFastMode(TreeFastMode::Fast));
    rig.open_tree();
    assert_eq!(rig.view().total.expect("total").games, 5);
}

/// Two games that transpose: both reach the board after 1.d4 d5 2.Nf3 and
/// then play Nf6.
const TRANSPOSITION: &str = "[Result \"1-0\"]\n\n1. d4 d5 2. Nf3 Nf6 1-0\n\n\
    [Result \"0-1\"]\n\n1. Nf3 d5 2. d4 Nf6 0-1\n\n";

#[test]
fn fast_then_slow_shows_the_fast_table_and_then_replaces_it_by_the_slow_one() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "t.pgn", TRANSPOSITION);
    rig.send(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow));
    rig.open_tree();
    rig.play("d2d4");
    rig.play("d7d5");
    let started = rig.started();
    // The step that the fast pass cannot see through: the transposition.
    let mv = rig.state.session.position().parse_uci("g1f3").unwrap();
    let mut first = jobs(rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }));
    assert_eq!(first.len(), 1, "the fast pass");
    let answer = rig.run_job(first.remove(0));
    let mut second = jobs(rig.raw(answer));
    assert_eq!(second.len(), 1, "the slow pass follows at once");
    // Between the passes the fast table is on screen, the panel is busy.
    let mid = rig.view();
    assert_eq!(mid.body, TreeBody::Rows);
    assert!(mid.searching);
    assert_eq!(mid.rows.len(), 1);
    assert_eq!(mid.rows[0].san, "Nf6");
    assert_eq!(
        mid.rows[0].games, 1,
        "the fast pass misses the transposition"
    );
    assert!(
        mid.rows.iter().all(|r| !r.clickable),
        "not interactive while the slow pass runs"
    );
    let answer = rig.run_job(second.remove(0));
    assert!(rig.raw(answer).is_empty());
    let done = rig.view();
    assert!(!done.searching);
    assert_eq!(done.rows[0].games, 2, "the slow pass finds both games");
    assert_eq!(done.total.expect("total").games, 2);
    assert_eq!(rig.started(), started + 2, "one search per phase");
}

#[test]
fn slow_mode_finds_the_transposition_in_one_search() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "t.pgn", TRANSPOSITION);
    rig.open_tree();
    rig.play("d2d4");
    rig.play("d7d5");
    let started = rig.started();
    rig.play("g1f3");
    assert_eq!(rig.started(), started + 1);
    assert_eq!(rig.view().rows[0].games, 2);
}

#[test]
fn fast_then_slow_runs_two_searches_when_opened() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow));
    rig.open_tree();
    assert_eq!(rig.started(), 2);
    assert_eq!(rig.view().total.expect("total").games, 5);
}

#[test]
fn a_change_between_the_two_passes_drops_the_slow_pass_of_the_old_position() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(TreeMessage::SetFastMode(TreeFastMode::FastThenSlow));
    let mut first = jobs(rig.raw(TreeMessage::Open));
    // The cursor moves while the fast pass runs.
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    let answer = rig.run_job(first.remove(0));
    let mut queue: VecDeque<Effect> = rig.raw(answer).into();
    // Whatever follows, drain it: the final table is for the cursor.
    while let Some(effect) = queue.pop_front() {
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"]);
    assert!(!rig.state.tree().searching());
}

// ---- slot state ----

#[test]
fn a_replaced_base_in_the_same_slot_is_searched_afresh() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let slot = rig.state.bases.current();
    rig.send(BasesMessage::Close(slot));
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    let other = "[Result \"1-0\"]\n\n1. c4 e5 1-0\n\n";
    rig.open_pgn(dir.path(), "other.pgn", other);
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert_eq!(rig.sans(), ["c4"]);
}

#[test]
fn the_unavailable_message_goes_away_when_a_base_with_games_is_current() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_tree();
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    assert_ne!(rig.view().body, TreeBody::Unavailable);
}
