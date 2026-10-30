//! M4-3 phase 2: extra edge cases derived from D436/D722 (`NewGame` when
//! another game is loaded), D727 (found engines) and hostile files.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_3_common;

use m4_3_common::*;
use mychess_app::engines::{
    EngineDraft, EngineId, EngineMessage, EngineRunState, StartMode, load_engine_list,
};
use mychess_app::{AppState, Command, DiscardChoice, Message, update};
use mychess_engine::testing::ScriptedEngine;

fn running(state: &mut AppState, id: EngineId) -> mychess_app::engines::RunId {
    let fx = send(state, EngineMessage::Start(id, StartMode::Analyze));
    let run = starts(&fx)[0];
    started(state, run);
    run
}

fn new_game(state: &mut AppState) -> Vec<mychess_app::Effect> {
    let mut fx = update(state, Message::Command(Command::NewGame));
    if state.discard_prompt.is_some() {
        fx.extend(update(
            state,
            Message::AnswerDiscard(DiscardChoice::DontSave),
        ));
    }
    fx
}

#[test]
fn an_engine_whose_analysis_was_off_while_another_game_was_loaded_gets_new_game_when_turned_on() {
    // D436: loading another game also emits `NewGame` (D408). An engine that
    // has analysed game 1 and is switched off, then switched on over game 2,
    // must not keep the state of game 1.
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    play(&mut state, "e2e4");
    send(&mut state, EngineMessage::ToggleMainAnalysis); // off
    new_game(&mut state);
    play(&mut state, "d2d4");
    let fx = send(&mut state, EngineMessage::ToggleMainAnalysis); // on
    assert_eq!(search_runs(&fx), [run]);
    assert!(
        has_new_game(&fx, run),
        "the engine saw game 1 and now gets game 2 without `ucinewgame`: {fx:?}"
    );
}

#[tokio::test]
async fn adding_the_same_found_executable_twice_quickly_does_not_duplicate_the_entry() {
    let Some(bin) = fake_binary() else {
        eprintln!("mychess-fake-engine not built: skipping");
        return;
    };
    let mut state = AppState::default();
    send(&mut state, EngineMessage::OpenDialog);
    let mut fx = send(&mut state, EngineMessage::AddFound(bin.clone()));
    fx.extend(send(&mut state, EngineMessage::AddFound(bin.clone())));
    settle(&mut state, fx).await;
    let same = state
        .engines
        .engines()
        .iter()
        .filter(|e| e.command == bin)
        .count();
    assert_eq!(
        same, 1,
        "D727: a found engine is offered once and added once"
    );
}

#[test]
fn hostile_ids_and_counters_in_the_file_do_not_overflow_or_panic() {
    for text in [
        "next_id = 9223372036854775807\n[[engine]]\nid = 9223372036854775807\nname=\"a\"\ncommand=\"c\"\n[[engine]]\nid = 9223372036854775807\nname=\"b\"\ncommand=\"c\"\n",
        "next_id = -1\n[[engine]]\nid = -5\nname=\"a\"\ncommand=\"c\"\n",
        "[[engine]]\nid = 1.5\nname=\"a\"\ncommand=\"c\"\n",
        "[[engine]]\nid = \"x\"\nname=\"a\"\ncommand=\"c\"\nmultipv = 99999\n",
        "[[engine]]\nname=\"a\"\ncommand=\"c\"\nmultipv = 0\nhotkey = \"f2\"\nlast_used = 9223372036854775807\n",
    ] {
        let loaded = load_engine_list(text).unwrap();
        assert!(!loaded.list.is_empty(), "{text}");
        let mut list = loaded.list;
        // The list stays usable: adding gives a fresh unique id.
        let id = list.add(EngineDraft::new("z", "z")).unwrap();
        let ids: std::collections::HashSet<_> = list.engines().iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), list.len(), "ids unique after {text}");
        assert!(list.get(id).is_some());
        for e in list.engines() {
            assert!(
                (1..=255).contains(&e.multipv),
                "multipv {} in {text}",
                e.multipv
            );
        }
        // And it renders and reloads.
        let again = load_engine_list(&mychess_app::engines::render_engine_list(&list)).unwrap();
        assert_eq!(again.list.len(), list.len());
    }
}

#[test]
fn a_crashed_run_can_be_stopped_and_the_engine_started_again_from_its_key() {
    let fake = ScriptedEngine::builder().build();
    let (mut state, ids) = state_with(&["A"], &fake);
    let run = running(&mut state, ids[0]);
    send(
        &mut state,
        EngineMessage::Event {
            run,
            event: Box::new(mychess_engine::EngineEvent::Exited(
                mychess_engine::ExitInfo {
                    reason: mychess_engine::ExitReason::Crashed,
                    status: None,
                    stderr_tail: vec![],
                },
            )),
        },
    );
    assert!(matches!(
        state.engine_runs()[0].state,
        EngineRunState::Failed(_)
    ));
    let fx = send(&mut state, EngineMessage::Start(ids[0], StartMode::Analyze));
    assert_eq!(starts(&fx).len(), 1);
    assert_eq!(state.engine_runs().len(), 1);
}
