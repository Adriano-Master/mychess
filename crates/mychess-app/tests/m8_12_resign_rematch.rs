//! M8-12 contract tests: which mode answers "Resign" and "Rematch", and the
//! rematch against an engine (same configuration, colours swapped; spec
//! `docs/specs/m8-12-desistir-e-revanche.md`, D3100-D3105).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m8_1_common;

use std::time::{Duration, Instant};

use m8_1_common::*;
use mychess_app::{
    Action, AppState, ColorChoice, GameMode, PlayMessage, PlayPhase, Shortcuts, StartFrom, TimeMode,
};
use mychess_core::Color;

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

fn fools_mate(r: &mut Rig) {
    let t = soon();
    let fx = r.player_move("f2", "f3", t);
    r.reply(&fx, Some("e7e5"));
    let fx = r.player_move("g2", "g4", t);
    r.reply(&fx, Some("d8h4"));
}

fn game_as(color: ColorChoice) -> Rig {
    let mut r = Rig::new();
    let cfg = r.config(color, TimeMode::Depth(2));
    r.start(cfg);
    r
}

#[test]
fn without_a_game_nothing_can_be_resigned_or_replayed() {
    let state = AppState::default();
    assert_eq!(state.resign_mode(), None);
    assert_eq!(state.rematch_mode(), None);
}

#[test]
fn a_game_on_against_the_engine_can_be_resigned_not_replayed() {
    let r = game_as(ColorChoice::White);
    assert_eq!(r.state.resign_mode(), Some(GameMode::Engine));
    assert_eq!(r.state.rematch_mode(), None);
}

#[test]
fn a_finished_game_against_the_engine_can_be_replayed_not_resigned() {
    let mut r = game_as(ColorChoice::White);
    fools_mate(&mut r);
    assert!(matches!(r.phase(), PlayPhase::Ended(_)));
    assert_eq!(r.state.resign_mode(), None);
    assert_eq!(r.state.rematch_mode(), Some(GameMode::Engine));
}

#[test]
fn the_rematch_swaps_the_colours_and_keeps_the_configuration() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(3));
    cfg.store_clock = true;
    r.start(cfg);
    let before = r.state.play_config().expect("a game").clone();
    fools_mate(&mut r);
    r.send(PlayMessage::Rematch(soon()));
    assert!(r.state.play_live());
    assert_eq!(r.view().player_color, Color::Black);
    // Everything but the colour is the configuration of the game that ended
    // (D3105): engine, limits, clock, book, ponder, coach, store_clock.
    let after = r.state.play_config().expect("a game").clone();
    assert_eq!(after.color, ColorChoice::Black);
    assert_eq!(
        mychess_app::PlayConfig {
            color: ColorChoice::White,
            ..after
        },
        before
    );
    // The engine opens the new game as White.
    assert_eq!(r.view().phase, PlayPhase::Opponent);
    assert!(r.mainline().is_empty());
    // And again: the colours swap back.
    r.send(PlayMessage::Resign(soon()));
    r.send(PlayMessage::Rematch(soon()));
    assert_eq!(r.view().player_color, Color::White);
}

#[test]
fn a_random_colour_swaps_from_the_colour_really_played() {
    let mut r = game_as(ColorChoice::Random);
    let first = r.view().player_color;
    r.send(PlayMessage::Resign(soon()));
    r.send(PlayMessage::Rematch(soon()));
    assert_eq!(r.view().player_color, !first);
}

#[test]
fn the_rematch_starts_from_the_start_of_the_game_that_ended() {
    let mut r = Rig::new();
    let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    cfg.start = StartFrom::Position(Box::new(mychess_core::Position::from_fen(fen).unwrap()));
    r.start(cfg);
    r.player_move("e2", "e4", soon());
    r.send(PlayMessage::Resign(soon()));
    r.send(PlayMessage::Rematch(soon()));
    assert_eq!(r.view().player_color, Color::Black);
    assert_eq!(r.state.session.game().start_position().fen(), fen);
}

#[test]
fn a_rematch_of_a_game_that_is_on_is_ignored() {
    let mut r = game_as(ColorChoice::White);
    let fx = r.send(PlayMessage::Rematch(soon()));
    assert!(fx.is_empty());
    assert_eq!(r.view().player_color, Color::White);
    assert_eq!(r.phase(), PlayPhase::Player);
}

#[test]
fn a_rematch_without_a_game_does_nothing() {
    let mut r = Rig::new();
    let fx = r.send(PlayMessage::Rematch(soon()));
    assert!(fx.is_empty());
    assert!(!r.state.play_live());
}

#[test]
fn once_the_game_is_closed_the_rematch_is_gone() {
    let mut r = game_as(ColorChoice::White);
    r.send(PlayMessage::Resign(soon()));
    assert_eq!(r.state.rematch_mode(), Some(GameMode::Engine));
    r.send(PlayMessage::Quit(soon()));
    assert_eq!(r.state.rematch_mode(), None);
}

#[test]
fn the_actions_have_names_and_no_default_key() {
    for action in [Action::ResignGame, Action::Rematch] {
        assert_eq!(Action::from_name(action.name()), Some(action));
        assert!(Shortcuts::defaults().bindings(action).is_empty());
    }
}
