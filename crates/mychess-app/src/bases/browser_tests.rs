//! Sanity tests of the game browser (slice M3-4e). The full contract
//! suite is the tester's.
#![allow(clippy::large_futures)] // the test rig owns the whole `AppState` across awaits

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::bases::{
    BasesMessage, BasesNotice, BrowseMessage, BrowseMode, GameStep, MAX_BROWSER_GAMES, SlotId,
};
use crate::effect::{Effect, perform};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::state::{AppState, update};

#[allow(clippy::format_push_string)]
fn games(count: usize) -> String {
    let mut text = String::new();
    for n in 1..=count {
        text.push_str(&format!(
            "[Event \"Ev {n}\"]\n[White \"W{n}\"]\n[Black \"B{n}\"]\n[Result \"1-0\"]\n\n\
             1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0\n\n"
        ));
    }
    text
}

fn write(dir: &Path, count: usize) -> PathBuf {
    let path = dir.join("g.pgn");
    std::fs::write(&path, games(count)).expect("write");
    path
}

#[allow(clippy::field_reassign_with_default)] // private fields forbid the struct update
fn state() -> AppState {
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state
}

async fn drive(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    let mut seen = Vec::new();
    let mut queue: VecDeque<Effect> = update(state, message.into()).into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        seen.push(effect.clone());
        // Timers are driven by hand in the tests.
        if matches!(effect, Effect::BrowserTick { .. }) {
            continue;
        }
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
    seen
}

fn browse(message: BrowseMessage) -> Message {
    Message::Bases(BasesMessage::Browse(message))
}

async fn opened(count: usize) -> (AppState, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(&mut state, BasesMessage::Open(write(dir.path(), count))).await;
    let id = state.bases.current();
    (state, id, dir)
}

fn plies(state: &AppState) -> Vec<usize> {
    state
        .browser
        .as_ref()
        .map(|b| {
            b.games
                .iter()
                .map(|g| g.ready().map_or(usize::MAX, super::browser::ReadyGame::ply))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn one_game_opens_the_single_browser_at_the_start() {
    let (mut state, id, _dir) = opened(5).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![3],
        }),
    )
    .await;
    let browser = state.browser.as_ref().expect("open");
    assert_eq!(browser.mode, BrowseMode::Single);
    assert_eq!(browser.games[0].number, 3);
    let ready = browser.games[0].ready().expect("read");
    assert_eq!((ready.ply(), ready.max_ply()), (0, 6));
    assert_eq!(ready.header.white, "W3");
    assert!(ready.doc.is_some());
    assert!(state.modal_open());
}

#[tokio::test]
async fn several_games_open_a_grid_at_the_end_of_each_game() {
    let (mut state, id, _dir) = opened(5).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![1, 2, 3, 4, 5],
        }),
    )
    .await;
    let browser = state.browser.as_ref().expect("open");
    assert_eq!(browser.mode, BrowseMode::Grid);
    assert_eq!(browser.columns(), 3);
    assert_eq!(plies(&state), vec![6; 5]);
}

#[tokio::test]
async fn steps_are_clamped_and_move_all_boards_or_one() {
    let (mut state, id, _dir) = opened(3).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![1, 2, 3],
        }),
    )
    .await;
    drive(&mut state, browse(BrowseMessage::GoToStart { game: None })).await;
    assert_eq!(plies(&state), vec![0, 0, 0]);
    drive(
        &mut state,
        browse(BrowseMessage::Step {
            game: None,
            delta: -1,
        }),
    )
    .await;
    assert_eq!(plies(&state), vec![0, 0, 0]);
    drive(
        &mut state,
        browse(BrowseMessage::Step {
            game: Some(1),
            delta: 4,
        }),
    )
    .await;
    assert_eq!(plies(&state), vec![0, 4, 0]);
    drive(&mut state, browse(BrowseMessage::GoToEnd { game: None })).await;
    assert_eq!(plies(&state), vec![6, 6, 6]);
    let ready = state.browser.as_ref().and_then(|b| b.games[0].ready());
    assert!(ready.expect("ready").position().is_some());
}

#[tokio::test]
async fn a_selection_over_the_limit_shows_the_first_games_with_a_notice() {
    let (mut state, id, _dir) = opened(70).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: (1..=70).collect(),
        }),
    )
    .await;
    assert_eq!(
        state.browser.as_ref().map(|b| b.games.len()),
        Some(MAX_BROWSER_GAMES)
    );
    assert_eq!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::BrowserLimit {
            shown: 64,
            asked: 70
        }))
    );
}

#[tokio::test]
async fn an_answer_for_a_closed_or_reopened_browser_is_dropped() {
    let (mut state, id, _dir) = opened(3).await;
    let open = |n| {
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![n],
        })
    };
    let first = update(&mut state, open(1));
    let second = update(&mut state, open(2));
    let (Some(first), Some(second)) = (first.into_iter().next(), second.into_iter().next()) else {
        panic!("both opens read games");
    };
    // The answer of the first one arrives late.
    let late = perform(first).await;
    update(&mut state, late);
    assert!(
        state
            .browser
            .as_ref()
            .is_some_and(super::browser::BrowserState::is_loading)
    );
    let answer = perform(second).await;
    update(&mut state, answer);
    assert_eq!(state.browser.as_ref().map(|b| b.games[0].number), Some(2));
    assert!(
        !state
            .browser
            .as_ref()
            .is_some_and(super::browser::BrowserState::is_loading)
    );
    // Closing drops what is still in flight.
    let effects = update(&mut state, open(3));
    update(&mut state, browse(BrowseMessage::Close));
    for effect in effects {
        let answer = perform(effect).await;
        update(&mut state, answer);
    }
    assert!(state.browser.is_none());
    assert!(!state.modal_open());
}

#[tokio::test]
async fn the_game_buttons_follow_the_filter() {
    let (mut state, id, _dir) = opened(5).await;
    // Remove game 2 and 4 from the filter.
    update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::SelectNumbers {
                slot: id,
                numbers: vec![2, 4],
            },
        )),
    );
    update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::RemoveSelected { slot: id },
        )),
    );
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![3],
        }),
    )
    .await;
    let number = |s: &AppState| s.browser.as_ref().map(|b| b.games[0].number);
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::Next))).await;
    assert_eq!(number(&state), Some(5));
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::Next))).await;
    assert_eq!(number(&state), Some(5));
    drive(
        &mut state,
        browse(BrowseMessage::GoToGame(GameStep::Previous)),
    )
    .await;
    assert_eq!(number(&state), Some(3));
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::First))).await;
    assert_eq!(number(&state), Some(1));
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::Last))).await;
    assert_eq!(number(&state), Some(5));
}

#[tokio::test]
async fn load_puts_the_game_on_the_main_board_at_the_ply_and_closes() {
    let (mut state, id, _dir) = opened(3).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![2],
        }),
    )
    .await;
    drive(
        &mut state,
        browse(BrowseMessage::Step {
            game: None,
            delta: 3,
        }),
    )
    .await;
    drive(
        &mut state,
        browse(BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: true,
        }),
    )
    .await;
    assert!(state.browser.is_some(), "Ctrl+Load keeps it open");
    assert_eq!(state.session.game().tags().white, "W2");
    assert_eq!(
        state.session.game().ply(state.session.cursor()).ok(),
        Some(3)
    );
    drive(
        &mut state,
        browse(BrowseMessage::Step {
            game: None,
            delta: 1,
        }),
    )
    .await;
    drive(
        &mut state,
        browse(BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        }),
    )
    .await;
    assert!(state.browser.is_none());
    assert_eq!(
        state.session.game().ply(state.session.cursor()).ok(),
        Some(4)
    );
}

#[tokio::test]
async fn autoplay_steps_on_ticks_and_stops_at_the_end() {
    let (mut state, id, _dir) = opened(2).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![1],
        }),
    )
    .await;
    let effects = drive(&mut state, browse(BrowseMessage::ToggleAutoplay)).await;
    let Some(Effect::BrowserTick { generation, .. }) = effects.last().cloned() else {
        panic!("a tick is scheduled: {effects:?}");
    };
    assert_eq!(plies(&state), vec![1]);
    // A stale tick does nothing.
    update(
        &mut state,
        browse(BrowseMessage::AutoplayTick {
            generation: generation + 7,
        }),
    );
    assert_eq!(plies(&state), vec![1]);
    let mut generation = generation;
    for expected in 2..=6 {
        let next = update(
            &mut state,
            browse(BrowseMessage::AutoplayTick { generation }),
        );
        assert_eq!(plies(&state), vec![expected]);
        match next.into_iter().next() {
            Some(Effect::BrowserTick { generation: g, .. }) => generation = g,
            other => assert_eq!(expected, 6, "stops only at the end: {other:?}"),
        }
    }
    assert!(!state.browser.as_ref().is_some_and(|b| b.autoplay));
}

#[tokio::test]
async fn closing_the_slot_closes_the_browser() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = state();
    drive(&mut state, BasesMessage::Open(write(dir.path(), 3))).await;
    let id = state.bases.current();
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![1],
        }),
    )
    .await;
    state.close_slot(id);
    assert!(state.browser.is_none());
}

#[tokio::test]
async fn a_huge_selection_on_a_sorted_view_yields_only_the_first_games_in_view_order() {
    let (mut state, id, _dir) = opened(2000).await;
    drive(
        &mut state,
        Message::Bases(BasesMessage::List(crate::bases::ListMessage::SortBy {
            slot: id,
            column: crate::bases::ListColumn::White,
        })),
    )
    .await;
    // The first click sorts ascending: "W1", "W10", "W100", ...
    update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::SelectNumbers {
                slot: id,
                numbers: (1..=2000).collect(),
            },
        )),
    );
    let slot = state.bases.slot(id).expect("slot");
    let started = std::time::Instant::now();
    let targets = slot.browse_targets(None);
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
    assert_eq!(targets.len(), MAX_BROWSER_GAMES + 1);
    assert_eq!(targets, slot.visible_range(1, 65));
}

#[tokio::test]
async fn without_a_selection_the_current_game_is_browsed() {
    let (mut state, id, _dir) = opened(4).await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: id,
            number: 3,
        },
    )
    .await;
    let slot = state.bases.slot(id).expect("slot");
    assert_eq!(slot.browse_targets(None), vec![3]);
}

#[tokio::test]
async fn a_tick_of_a_previous_browser_never_revives_the_autoplay() {
    let (mut state, id, _dir) = opened(4).await;
    let open = |n| {
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![n],
        })
    };
    drive(&mut state, open(1)).await;
    let effects = drive(&mut state, browse(BrowseMessage::ToggleAutoplay)).await;
    let Some(Effect::BrowserTick {
        generation: old, ..
    }) = effects.last().cloned()
    else {
        panic!("a timer");
    };
    drive(&mut state, browse(BrowseMessage::GoToGame(GameStep::Next))).await;
    let effects = drive(&mut state, browse(BrowseMessage::ToggleAutoplay)).await;
    let Some(Effect::BrowserTick {
        generation: new, ..
    }) = effects.last().cloned()
    else {
        panic!("a timer");
    };
    assert_ne!(old, new);
    let before = plies(&state);
    let stale = update(
        &mut state,
        browse(BrowseMessage::AutoplayTick { generation: old }),
    );
    assert!(stale.is_empty());
    assert_eq!(plies(&state), before);
}

#[tokio::test]
async fn a_game_loaded_from_a_turned_browser_asks_for_the_main_board_to_turn_only_when_it_lands() {
    let (mut state, id, _dir) = opened(3).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![2],
        }),
    )
    .await;
    drive(&mut state, browse(BrowseMessage::Flip { game: None })).await;
    assert!(!state.board_flipped);
    drive(
        &mut state,
        browse(BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        }),
    )
    .await;
    assert!(state.board_flipped);
}

#[tokio::test]
async fn quitting_closes_the_browser() {
    let (mut state, id, _dir) = opened(3).await;
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![2],
        }),
    )
    .await;
    update(&mut state, Message::Quit);
    assert!(state.browser.is_none());
}

#[tokio::test]
async fn a_flag_job_winding_down_keeps_its_file_from_being_opened_again() {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let dir = tempfile::tempdir().expect("dir");
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).expect("create");
    import_pgn(
        games(3).as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    let mut state = state();
    drive(&mut state, BasesMessage::Open(base.with_extension("si4"))).await;
    let id = state.bases.current();
    let path = state
        .bases
        .slot(id)
        .and_then(|s| s.path.clone())
        .expect("path");
    update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::SelectNumbers {
                slot: id,
                numbers: vec![1],
            },
        )),
    );
    // The job starts (its effect is not run) and the slot is closed.
    let effects = update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::ToggleDeleted { slot: id },
        )),
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ToggleDeleted { .. }))
    );
    state.close_slot(id);
    let effects = update(&mut state, Message::Bases(BasesMessage::Open(path)));
    assert!(effects.is_empty());
    assert_eq!(state.notice, Some(AppNotice::Bases(BasesNotice::ListBusy)));
}

#[tokio::test]
async fn cancelling_the_discard_question_asks_for_no_flip_of_the_main_board() {
    let (mut state, id, _dir) = opened(3).await;
    drive(
        &mut state,
        BasesMessage::LoadGame {
            slot: id,
            number: 1,
        },
    )
    .await;
    update(
        &mut state,
        Message::Game(crate::session::GameMessage::Navigate(
            crate::session::Nav::GameEnd,
        )),
    );
    let mv = state.session.position().parse_uci("a2a3").expect("move");
    update(
        &mut state,
        Message::Game(crate::session::GameMessage::PlayMove {
            mv,
            options: crate::session::PlayOptions::default(),
        }),
    );
    assert!(state.is_dirty());
    drive(
        &mut state,
        browse(BrowseMessage::Open {
            slot: id,
            numbers: vec![3],
        }),
    )
    .await;
    drive(&mut state, browse(BrowseMessage::Flip { game: None })).await;
    drive(
        &mut state,
        browse(BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        }),
    )
    .await;
    assert!(state.discard_prompt.is_some());
    update(
        &mut state,
        Message::AnswerDiscard(crate::flow::DiscardChoice::Cancel),
    );
    assert!(!state.board_flipped);
    assert!(state.browser.is_some(), "the browser stays open");
}

#[tokio::test]
async fn cancelling_a_selection_copy_after_some_games_reports_how_many() {
    let (mut state, id, _dir) = opened(4).await;
    update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::SelectNumbers {
                slot: id,
                numbers: vec![1, 2],
            },
        )),
    );
    let effects = update(
        &mut state,
        Message::Bases(BasesMessage::List(
            crate::bases::ListMessage::CopySelected { slot: id },
        )),
    );
    let Some(Effect::CopyGames { request, .. }) = effects.into_iter().next() else {
        panic!("a copy job");
    };
    update(
        &mut state,
        Message::Bases(BasesMessage::List(crate::bases::ListMessage::GamesCopied {
            request,
            result: Ok(crate::bases::CopyOutcome {
                copied: 1,
                error: Some(crate::bases::BaseError::Cancelled),
                truncated: 0,
                reloaded: None,
            }),
        })),
    );
    assert!(matches!(
        state.notice,
        Some(AppNotice::Bases(BasesNotice::CopyStopped {
            count: 1,
            error: crate::bases::BaseError::Cancelled,
            ..
        }))
    ));
}
