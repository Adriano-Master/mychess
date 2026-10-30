//! M2-4 game information panel (spec D183-D185, rules 30-36).

#![allow(
    clippy::assigning_clones,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

mod m2_4_common;

use iced::{Settings, Size};
use iced_test::Simulator;
use m2_4_common::{Rig, has, line, messages, play, sample};
use mychess_app::{GameMessage, Message as AppMessage, Nav, TagField};
use mychess_core::{Game, GameResult, Nag, Position};
use mychess_gui::Message;
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::game_info;

fn panel(rig: &Rig) -> Simulator<'static, Message> {
    Simulator::with_size(
        Settings::default(),
        Size::new(900.0, 520.0),
        game_info::view(&rig.shell.app.game_info(), &rig.shell.i18n),
    )
}

fn tagged() -> Rig {
    let mut game = Game::new();
    let root = game.root();
    line(&mut game, root, &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]);
    let tags = game.tags_mut();
    tags.white = "Carlsen, M.".to_owned();
    tags.black = "Caruana, F.".to_owned();
    tags.event = "Candidates".to_owned();
    tags.site = "London".to_owned();
    tags.round = "3".to_owned();
    tags.result = GameResult::White;
    tags.white_rating = Some(mychess_core::Rating::new(
        mychess_core::Elo::new(2850).expect("elo"),
        mychess_core::RatingType::Elo,
    ));
    tags.date = "2024.05.17".parse().expect("date");
    Rig::with_game(game)
}

#[test]
fn m2_4_the_first_line_shows_players_with_elo_and_the_result() {
    let rig = tagged();
    let mut sim = panel(&rig);
    assert!(sim.find("Carlsen, M. (2850)").is_ok());
    assert!(sim.find("Caruana, F.").is_ok());
    assert!(sim.find("--").is_ok());
    assert!(sim.find("(1-0)").is_ok());
}

#[test]
fn m2_4_clicking_the_unknown_white_opens_the_dialog_on_the_white_field() {
    let rig = Rig::new();
    let mut sim = panel(&rig);
    sim.click("?").expect("the first unknown name");
    let got = messages(sim);
    assert!(
        has(
            &got,
            &Message::Dialog(DialogMessage::OpenHeader(Some(TagField::White)))
        ),
        "{got:?}"
    );
}

#[test]
fn m2_4_the_event_line_has_event_site_date_and_round() {
    let rig = tagged();
    let mut sim = panel(&rig);
    assert!(sim.find("Event: Candidates, London").is_ok());
    assert!(sim.find("(2024.05.17, Round 3)").is_ok());
}

#[test]
fn m2_4_the_move_line_shows_previous_and_next_moves_and_start_or_end() {
    let mut rig = tagged();
    {
        let mut sim = panel(&rig);
        assert!(sim.find("Start of game").is_ok());
        assert!(sim.find("Next: 1. e4").is_ok());
    }
    let nodes: Vec<_> = rig.game().mainline().collect();
    assert_eq!(nodes.len(), 5);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        Nav::GoToMoveNumber(1),
    )));
    {
        let mut sim = panel(&rig);
        assert!(sim.find("1. e4").is_ok());
        assert!(sim.find("Next: 1... e5").is_ok());
    }
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::GameEnd)));
    let mut sim = panel(&rig);
    assert!(sim.find("3. Bb5").is_ok());
    assert!(sim.find("End of game").is_ok());
}

#[test]
fn m2_4_clicking_next_goes_forward() {
    let rig = tagged();
    let mut sim = panel(&rig);
    sim.click("Next: 1. e4").expect("next");
    let got = messages(sim);
    assert!(
        has(
            &got,
            &Message::App(AppMessage::Game(GameMessage::Navigate(Nav::Forward(1))))
        ),
        "{got:?}"
    );
}

#[test]
fn m2_4_nags_follow_the_moves_of_the_line() {
    let mut rig = tagged();
    let first = rig.game().mainline().next().expect("first move");
    rig.set_game({
        let mut game = rig.game().clone();
        game.add_nag(first, Nag::new(1).expect("nag")).expect("nag");
        game
    });
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(
        Nav::GoToMoveNumber(1),
    )));
    let mut sim = panel(&rig);
    assert!(
        sim.find("1. e4!").is_ok(),
        "the symbol is stuck to the move"
    );
}

#[test]
fn m2_4_inside_a_variation_the_panel_offers_to_leave_it() {
    let s = sample();
    let nc3 = s.nc3;
    let mut rig = Rig::with_game(s.game);
    rig.goto(nc3);
    let mut sim = panel(&rig);
    sim.click("(Var)").expect("exit link");
    let got = messages(sim);
    assert!(has(
        &got,
        &Message::App(AppMessage::Game(GameMessage::Navigate(Nav::ExitVariation)))
    ));
}

#[test]
fn m2_4_length_counts_full_moves_and_material_has_a_sign() {
    // 5 plies = 3 moves.
    let rig = tagged();
    {
        let mut sim = panel(&rig);
        assert!(sim.find("Length: 3").is_ok());
        assert!(sim.find("Material: 0").is_ok());
    }
    // White is a knight up (3 points), then a rook down (-5 for white).
    for (fen, expected) in [
        ("4k3/8/8/8/8/8/8/3NK3 w - - 0 1", "Material: +3"),
        ("r3k3/8/8/8/8/8/8/4K3 w - - 0 1", "Material: -5"),
        (
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "Material: 0",
        ),
        ("4k3/8/8/8/8/8/PPPPPPPP/4K3 w - - 0 1", "Material: +8"),
        ("4k3/qq6/8/8/8/8/8/4K3 w - - 0 1", "Material: -18"),
    ] {
        let position = Position::from_fen(fen).expect("fen");
        let mut rig = Rig::with_game(Game::from_position(position));
        rig.sync();
        let mut sim = panel(&rig);
        assert!(sim.find(expected).is_ok(), "{fen}: {expected}");
    }
}

#[test]
fn m2_4_at_most_five_variations_are_listed_and_clicking_one_enters_it() {
    let mut game = Game::new();
    let root = game.root();
    play(&mut game, root, "e2e4");
    for uci in ["d2d4", "c2c4", "g1f3", "b1c3", "a2a3", "b2b3", "h2h3"] {
        play(&mut game, root, uci);
    }
    let rig = Rig::with_game(game);
    let mut sim = panel(&rig);
    assert!(sim.find("1: 1. d4").is_ok());
    assert!(sim.find("5: 1. b3").is_ok() || sim.find("5: 1. a3").is_ok());
    assert!(sim.find("6: 1. h3").is_err(), "no sixth line");
    assert!(sim.find("7: 1. h3").is_err());
    sim.click("2: 1. c4").expect("second variation");
    let got = messages(sim);
    assert!(has(
        &got,
        &Message::App(AppMessage::Game(GameMessage::Navigate(
            Nav::EnterVariation(2)
        )))
    ));
}

#[test]
fn m2_4_the_comment_is_shown_without_marks_and_clicking_it_opens_the_editor() {
    // D185: the comment of the current move, without `[%...]` codes; a click
    // opens the editor.
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    game.set_comment(e4, "[%clk 0:01:00] A strong centre")
        .expect("comment");
    let mut rig = Rig::with_game(game);
    rig.goto(e4);
    let mut sim = panel(&rig);
    assert!(sim.find("A strong centre").is_ok(), "marks are stripped");
    sim.click("A strong centre").expect("the comment");
    let got = messages(sim);
    assert!(
        !got.is_empty(),
        "a click on the comment of the panel should open the editor"
    );
}

#[test]
fn m2_4_a_long_comment_is_cut_with_an_ellipsis() {
    let mut game = Game::new();
    let root = game.root();
    let e4 = play(&mut game, root, "e2e4");
    let long = "word ".repeat(60);
    game.set_comment(e4, &long).expect("comment");
    let mut rig = Rig::with_game(game);
    rig.goto(e4);
    let view = rig.shell.app.game_info();
    let shown = view.comment.expect("a comment");
    assert!(shown.ends_with("..."), "{shown}");
    assert!(shown.len() < long.len());
}

#[test]
fn m2_4_the_texts_are_translated() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(Game::new());
    let mut sim = panel(&rig);
    assert!(sim.find("Início do jogo").is_ok() || sim.find("Início da partida").is_ok());
}
