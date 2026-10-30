//! The sounds of a FICS game inside `AppState` (spec M8-11, D4400-D4401):
//! the board lines of the server go through `update` and the new moves, the
//! start and the end make `Effect::Sound`; a repeated board is silent.

use std::time::Instant;

use mychess_online::fics::{FicsMessage as Net, parse_line};

use crate::effect::Effect;
use crate::fics::{ConnectRequest, FicsMessage, NetUpdate};
use crate::message::Message;
use crate::sound::SoundEffect;
use crate::state::{AppState, update};

const START: &str = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 me bob 1 5 0 39 39 300 300 1 none (0:00) none 0";
const E4: &str = "<12> rnbqkbnr pppppppp -------- -------- ----P--- -------- PPPP-PPP RNBQKBNR B 4 1 1 1 1 0 7 me bob -1 5 0 39 39 297 300 1 P/e2-e4 (0:03) e4 0";

fn plays(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::Sound(SoundEffect::Play(_))))
        .count()
}

fn line(state: &mut AppState, text: &str) -> Vec<Effect> {
    let message = Message::Fics(FicsMessage::Net(NetUpdate {
        epoch: state.fics.epoch(),
        at: Instant::now(),
        message: Net::Event(parse_line(text)),
    }));
    update(state, message)
}

fn logged_in(sound: bool) -> AppState {
    let mut state = AppState::default();
    state.prefs.sound.enabled = sound;
    update(
        &mut state,
        Message::Fics(FicsMessage::Connect(ConnectRequest { name: None })),
    );
    let message = Message::Fics(FicsMessage::Net(NetUpdate {
        epoch: state.fics.epoch(),
        at: Instant::now(),
        message: Net::LoggedIn { name: "me".into() },
    }));
    update(&mut state, message);
    state
}

#[test]
fn a_fics_game_plays_the_start_the_new_move_and_the_end_once_each() {
    let mut state = logged_in(true);
    assert_eq!(plays(&line(&mut state, START)), 1, "the start, no move");
    assert_eq!(plays(&line(&mut state, E4)), 1, "the new move");
    assert_eq!(
        plays(&line(&mut state, E4)),
        0,
        "the same board again is silent"
    );
    let end = line(&mut state, "{Game 7 (me vs. bob) bob resigns} 1-0");
    assert_eq!(plays(&end), 1, "the end of the game");
}

#[test]
fn a_fics_game_is_silent_with_the_sounds_off() {
    let mut state = logged_in(false);
    let mut all = line(&mut state, START);
    all.extend(line(&mut state, E4));
    all.extend(line(&mut state, "{Game 7 (me vs. bob) bob resigns} 1-0"));
    assert_eq!(plays(&all), 0);
}
