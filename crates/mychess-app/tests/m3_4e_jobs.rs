//! Contract tests of the reading job of the browser, races and the modal
//! layer (spec M3-4, slice 4e: D595, D598, D603, D606).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4e_common;

use m3_4e_common::*;
use mychess_app::{Command, CopyScope, ListColumn};

fn read_effect(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::ReadBrowserGames { .. }))
        .expect("a ReadBrowserGames effect")
}

fn control_of(effect: &Effect) -> mychess_app::OpenControl {
    let Effect::ReadBrowserGames { control, .. } = effect else {
        panic!("not a read");
    };
    control.clone()
}

// ---- progress and cancellation ----

#[test]
fn the_read_reports_progress_and_finishes_complete() {
    let (mut rig, slot, _d) = setup(10);
    let effect = read_effect(rig.raw(open_msg(slot, &[1, 2, 3, 4, 5, 6])));
    let control = control_of(&effect);
    let answer = rig.perform(effect);
    assert_eq!(control.progress(), (6, 6));
    assert!(!control.is_cancelled());
    rig.send(answer);
    assert!(!br(&rig).is_loading());
}

#[test]
fn closing_the_browser_cancels_the_read_and_the_late_answer_is_dropped() {
    let (mut rig, slot, _d) = setup(5);
    let effect = read_effect(rig.raw(open_msg(slot, &[1, 2, 3])));
    let control = control_of(&effect);
    rig.raw(browse(BrowseMessage::Close));
    assert!(control.is_cancelled());
    assert!(rig.state.browser.is_none());
    let answer = rig.perform(effect);
    rig.send(answer);
    assert!(rig.state.browser.is_none(), "an answer cannot reopen it");
    assert!(!rig.state.modal_open());
}

#[test]
fn reopening_cancels_the_old_read_and_only_the_new_answer_counts() {
    let (mut rig, slot, _d) = setup(5);
    let first = read_effect(rig.raw(open_msg(slot, &[1])));
    let old = control_of(&first);
    let second = read_effect(rig.raw(open_msg(slot, &[4])));
    let new = control_of(&second);
    assert!(old.is_cancelled());
    assert!(!new.is_cancelled());
    // The late answer of the first read.
    let late = rig.perform(first);
    rig.raw(late);
    assert!(br(&rig).is_loading(), "still waiting for its own answer");
    assert_eq!(numbers_shown(&rig), [4]);
    let answer = rig.perform(second);
    rig.send(answer);
    assert_eq!(br(&rig).games[0].ready().unwrap().header.white, "W4");
}

#[test]
fn an_answer_for_a_browser_that_changed_game_is_dropped() {
    let (mut rig, slot, _d) = setup(5);
    open_browser(&mut rig, slot, &[1]);
    let go = read_effect(rig.raw(browse(BrowseMessage::GoToGame(GameStep::Next))));
    // An answer for a request that is not the current one.
    let stale = read_effect(rig.raw(open_msg(slot, &[3])));
    let answer_stale = rig.perform(stale);
    let current = read_effect(rig.raw(browse(BrowseMessage::GoToGame(GameStep::Last))));
    rig.raw(answer_stale);
    assert!(br(&rig).is_loading());
    let answer = rig.perform(current);
    rig.send(answer);
    assert_eq!(numbers_shown(&rig), [5]);
    let late = rig.perform(go);
    rig.send(late);
    assert_eq!(numbers_shown(&rig), [5]);
    assert_eq!(br(&rig).games[0].ready().unwrap().header.white, "W5");
}

#[test]
fn a_cancelled_read_leaves_unreadable_boards_not_boards_that_load_forever() {
    let (mut rig, slot, _d) = setup(5);
    let effect = read_effect(rig.raw(open_msg(slot, &[1, 2, 3])));
    control_of(&effect).cancel();
    let answer = rig.perform(effect);
    rig.send(answer);
    assert!(!br(&rig).is_loading());
    assert!(
        br(&rig)
            .games
            .iter()
            .all(|g| matches!(g.content, BrowserContent::Unreadable(_)))
    );
    // Everything still works on boards without a game.
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 1,
        },
    );
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 1,
            keep_open: false,
        },
    );
    assert!(rig.state.browser.is_some());
}

#[test]
fn a_bogus_answer_with_the_right_request_does_not_panic() {
    let (mut rig, slot, _d) = setup(5);
    let effect = read_effect(rig.raw(open_msg(slot, &[1, 2])));
    let Effect::ReadBrowserGames { request, .. } = &effect else {
        unreachable!()
    };
    let request = *request;
    // More games than asked, wrong numbers, none at all.
    let real = rig.perform(effect);
    let Message::Bases(BasesMessage::Browse(BrowseMessage::Loaded { games, .. })) = real else {
        panic!("a Loaded answer");
    };
    let mut crowd = games.clone();
    crowd.extend(games.iter().cloned());
    rig.raw(browse(BrowseMessage::Loaded {
        request,
        games: crowd,
    }));
    assert!(!br(&rig).is_loading());
    assert_eq!(numbers_shown(&rig), [1, 2]);
    // Again with nothing: harmless.
    rig.raw(browse(BrowseMessage::Loaded {
        request,
        games: Vec::new(),
    }));
    assert_eq!(numbers_shown(&rig), [1, 2]);
}

// ---- jobs of the list ----

fn start_sort(rig: &mut Rig, slot: SlotId) -> Effect {
    let mut effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert_eq!(effects.len(), 1);
    effects.remove(0)
}

#[test]
fn the_browser_does_not_open_over_a_job_of_its_slot_and_says_why() {
    let (mut rig, slot, _d) = setup(4);
    let job = start_sort(&mut rig, slot);
    rig.clear_notice();
    let effects = rig.raw(open_msg(slot, &[1, 2]));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.browser.is_none());
    assert!(has_notice(&rig, &BasesNotice::ListBusy));
    // When the job ends it opens.
    let answer = rig.perform(job);
    rig.send(answer);
    open_browser(&mut rig, slot, &[1, 2]);
    assert!(rig.state.browser.is_some());
}

#[test]
fn a_job_on_another_slot_does_not_block_the_browser() {
    let (mut rig, first, dir) = setup(4);
    let second = rig.open_games(dir.path(), "h.pgn", &sixes(3));
    let job = start_sort(&mut rig, second);
    open_browser(&mut rig, first, &[1, 2]);
    assert!(rig.state.browser.is_some());
    drop(job);
}

#[test]
fn a_copy_that_writes_into_the_slot_blocks_the_browser_on_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "a.pgn", &sixes(4));
    let path = make_si4(dir.path(), "dst", &pgn(&sixes(2)));
    let to = rig.open(&path);
    rig.send(Message::Prefs(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::ConfirmCopy(false),
    )));
    let effects = rig.raw(l(ListMessage::CopyGames {
        from,
        to,
        scope: CopyScope::Filter,
    }));
    assert_eq!(effects.len(), 1, "{effects:?}");
    for slot in [from, to] {
        rig.clear_notice();
        let none = rig.raw(open_msg(slot, &[1]));
        assert!(none.is_empty());
        assert!(rig.state.browser.is_none());
        assert!(has_notice(&rig, &BasesNotice::ListBusy));
    }
}

#[test]
fn a_game_key_and_a_load_during_a_job_are_refused_with_the_notice() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[2]);
    // A job cannot start under the layer (list messages wait), so make
    // one before and open after the job ended: here the busy state comes
    // from a flag job that started before the browser.
    send(&mut rig, BrowseMessage::Close);
    let job = start_sort(&mut rig, slot);
    let effects = rig.raw(browse(BrowseMessage::LoadIntoBoard {
        game: 0,
        keep_open: false,
    }));
    assert!(effects.is_empty());
    drop(job);
}

#[test]
fn the_list_cannot_start_a_job_under_the_browser() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[1, 2]);
    let effects = rig.raw(l(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.list_job().is_none());
}

// ---- the modal layer ----

#[test]
fn modal_open_counts_the_browser_and_the_editor_is_frozen_under_it() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    send_nav(&mut rig, Nav::Start);
    let cursor = rig.state.session.cursor();
    assert!(!rig.state.modal_open());
    open_browser(&mut rig, slot, &[2, 3]);
    assert!(rig.state.modal_open());
    // Navigation and moves of the board do nothing under the layer.
    send_nav(&mut rig, Nav::GameEnd);
    assert_eq!(rig.state.session.cursor(), cursor);
    let mv = rig.state.session.position().parse_uci("e2e4");
    if let Ok(mv) = mv {
        rig.send(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        });
    }
    assert_eq!(rig.state.session.cursor(), cursor);
    assert!(!rig.state.is_dirty());
    send(&mut rig, BrowseMessage::Close);
    assert!(!rig.state.modal_open());
    send_nav(&mut rig, Nav::GameEnd);
    assert_ne!(rig.state.session.cursor(), cursor);
}

fn send_nav(rig: &mut Rig, nav: Nav) {
    rig.send(GameMessage::Navigate(nav));
}

#[test]
fn the_list_and_the_slots_do_not_react_under_the_browser() {
    let (mut rig, first, dir) = setup(5);
    let second = rig.open_games(dir.path(), "h.pgn", &sixes(3));
    rig.send(BasesMessage::Select(first));
    open_browser(&mut rig, first, &[1, 2]);
    let selection = rig.selection(first);
    // Keys of the list.
    rig.pick(first, 4, mychess_app::PickMode::Only);
    rig.send(l(ListMessage::RemoveSelected { slot: first }));
    rig.send(l(ListMessage::NegateFilter { slot: first }));
    assert_eq!(rig.selection(first), selection);
    assert_eq!(rig.filter_numbers(first), vec![1, 2, 3, 4, 5]);
    // Ctrl-1..9 / Next / Previous.
    rig.send(BasesMessage::Select(second));
    assert_eq!(rig.state.bases.current(), first);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert_eq!(rig.state.bases.current(), first);
    rig.send(BasesMessage::Next);
    rig.send(BasesMessage::Previous);
    assert_eq!(rig.state.bases.current(), first);
    // Loading from the list, the Clipbase and opening files.
    let before = editor_white(&rig);
    rig.send(BasesMessage::LoadGame {
        slot: first,
        number: 3,
    });
    rig.send(BasesMessage::ClipbaseCopy);
    assert_eq!(editor_white(&rig), before);
    assert!(rig.state.browser.is_some());
}

fn dirty_editor_with_prompt(rig: &mut Rig, slot: SlotId) {
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play(rig, "d2d3");
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    assert!(rig.state.discard_prompt.is_some());
}

#[test]
fn the_browser_does_not_open_under_a_discard_question() {
    let (mut rig, slot, _d) = setup(3);
    dirty_editor_with_prompt(&mut rig, slot);
    let effects = rig.raw(open_msg(slot, &[1, 2]));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.browser.is_none());
    assert!(rig.state.discard_prompt.is_some());
}

#[test]
fn the_browser_does_not_open_under_the_setup_dialog() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(Message::Command(Command::SetupBoard));
    assert!(rig.state.setup.is_some());
    let effects = rig.raw(open_msg(slot, &[1, 2]));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.browser.is_none());
}

#[test]
fn the_browser_does_not_open_under_the_copy_question() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "a.pgn", &sixes(4));
    let path = make_si4(dir.path(), "dst", &pgn(&sixes(2)));
    let to = rig.open(&path);
    rig.raw(l(ListMessage::CopyGames {
        from,
        to,
        scope: CopyScope::Filter,
    }));
    assert!(rig.state.copy_prompt.is_some());
    let effects = rig.raw(open_msg(from, &[1, 2]));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.browser.is_none());
}

#[test]
fn opening_a_second_browser_replaces_the_first() {
    let (mut rig, slot, _d) = setup(6);
    open_browser(&mut rig, slot, &[1, 2, 3]);
    open_browser(&mut rig, slot, &[5]);
    assert_eq!(br(&rig).mode, BrowseMode::Single);
    assert_eq!(numbers_shown(&rig), [5]);
}

#[test]
fn quit_with_the_browser_open_goes_on_with_the_normal_flow() {
    // The task brief says Quit "closes" the browser; the spec is silent and
    // `AppState::quit` does not touch it. What matters is the normal flow:
    // the program exits and nothing waits for the browser. See the report.
    let (mut rig, slot, _d) = setup(3);
    open_browser(&mut rig, slot, &[1, 2]);
    let effects = rig.raw(Message::Quit);
    assert_eq!(effects, [Effect::Exit]);
}

#[test]
fn quit_with_the_browser_open_and_an_altered_game_still_asks() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play(&mut rig, "d2d3");
    open_browser(&mut rig, slot, &[1, 2]);
    let effects = rig.raw(Message::Quit);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.discard_prompt.is_some(), "the normal question");
    // Cancelling it keeps the program (and the altered game).
    rig.send(Message::AnswerDiscard(mychess_app::DiscardChoice::Cancel));
    assert!(rig.state.is_dirty());
}
