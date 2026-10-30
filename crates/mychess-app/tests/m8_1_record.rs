//! M8-1a contract tests of the record of the game (spec D1231, D1232,
//! D1235, D1237, D1245): the tags, the result, the comments of the
//! automatic draws, and the PGN the game becomes.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use m8_1_common::*;
use mychess_app::{
    PLAY_EVENT, PlayEnd, Strength, TagSetup, TimeMode, write_play_result, write_play_start_tags,
};
use mychess_core::{Color, Game, GameResult, PartialDate};
use mychess_pgn::read::{ReadOptions, parse_pgn_text};
use mychess_pgn::write::{WriteOptions, game_to_string};
use rstest::rstest;

fn date() -> PartialDate {
    PartialDate::new(Some(2026), Some(10), Some(24)).expect("date")
}

fn tag_setup(color: Color, mode: TimeMode) -> TagSetup {
    TagSetup {
        opponent: "Fake 1".to_owned(),
        player: "Ana".to_owned(),
        player_color: color,
        mode,
        strength: Strength::default(),
        opening: None,
        date: date(),
    }
}

fn tagged(setup: &TagSetup) -> Game {
    let mut game = Game::new();
    write_play_start_tags(&mut game, setup);
    game
}

#[test]
fn the_event_is_uci_game() {
    assert_eq!(PLAY_EVENT, "UCI Game");
    assert_eq!(
        tagged(&tag_setup(Color::White, blitz())).tags().event,
        "UCI Game"
    );
}

#[test]
fn the_player_and_the_engine_take_the_tag_of_their_colour() {
    let g = tagged(&tag_setup(Color::White, blitz()));
    assert_eq!(
        (g.tags().white.as_str(), g.tags().black.as_str()),
        ("Ana", "Fake 1")
    );
    let g = tagged(&tag_setup(Color::Black, blitz()));
    assert_eq!(
        (g.tags().white.as_str(), g.tags().black.as_str()),
        ("Fake 1", "Ana")
    );
}

#[rstest]
#[case("")]
#[case("   ")]
#[case("\t")]
fn a_blank_player_name_is_a_question_mark(#[case] name: &str) {
    let mut s = tag_setup(Color::White, blitz());
    s.player = name.to_owned();
    assert_eq!(tagged(&s).tags().white, "?");
}

#[test]
fn the_date_is_today_and_the_result_is_open() {
    let g = tagged(&tag_setup(Color::White, blitz()));
    assert_eq!(g.tags().date, date());
    assert_eq!(g.tags().result, GameResult::Unknown);
}

#[rstest]
#[case(blitz(), "Time+Bonus 5+3/5+3", Some("300+3"))]
#[case(timebonus(10, 0, 10, 0), "Time+Bonus 10+0/10+0", Some("600"))]
#[case(timebonus(1, 60, 1, 60), "Time+Bonus 1+60/1+60", Some("60+60"))]
#[case(timebonus(120, 0, 120, 0), "Time+Bonus 120+0/120+0", Some("7200"))]
#[case(timebonus(5, 3, 10, 0), "Time+Bonus 5+3/10+0", None)]
#[case(TimeMode::MoveTime(6), "Movetime 6", Some("1/6"))]
#[case(TimeMode::Depth(3), "Depth 3", None)]
#[case(TimeMode::Nodes(1000), "Nodes 1000", None)]
fn timemode_and_timecontrol_follow_the_mode(
    #[case] mode: TimeMode,
    #[case] timemode: &str,
    #[case] control: Option<&str>,
) {
    let g = tagged(&tag_setup(Color::White, mode));
    assert_eq!(g.tags().get_extra("Timemode"), Some(timemode));
    assert_eq!(g.tags().get_extra("TimeControl"), control);
}

#[test]
fn out_of_range_modes_are_clamped_in_the_tags_too() {
    let g = tagged(&tag_setup(Color::White, TimeMode::Depth(0)));
    assert_eq!(g.tags().get_extra("Timemode"), Some("Depth 1"));
    let g = tagged(&tag_setup(Color::White, timebonus(0, 99, 500, 0)));
    assert_eq!(
        g.tags().get_extra("Timemode"),
        Some("Time+Bonus 1+60/120+0")
    );
}

#[test]
fn strength_and_opening_tags_appear_only_when_asked() {
    let none = tagged(&tag_setup(Color::White, blitz()));
    for name in ["UCI_Elo", "SkillLevel", "Opening"] {
        assert_eq!(none.tags().get_extra(name), None, "{name}");
    }
    let mut s = tag_setup(Color::White, blitz());
    s.strength = Strength {
        elo: Some(1650),
        skill: Some(7),
    };
    s.opening = Some("Sicilian Defence".to_owned());
    let g = tagged(&s);
    assert_eq!(g.tags().get_extra("UCI_Elo"), Some("1650"));
    assert_eq!(g.tags().get_extra("SkillLevel"), Some("7"));
    assert_eq!(g.tags().get_extra("Opening"), Some("Sicilian Defence"));
    let mut only_elo = tag_setup(Color::White, blitz());
    only_elo.strength.elo = Some(1500);
    let g = tagged(&only_elo);
    assert_eq!(g.tags().get_extra("UCI_Elo"), Some("1500"));
    assert_eq!(g.tags().get_extra("SkillLevel"), None);
}

#[test]
fn a_tag_value_the_tags_refuse_does_not_stop_the_game() {
    let mut s = tag_setup(Color::White, blitz());
    s.opening = Some("Two\nLines".to_owned());
    let g = tagged(&s);
    assert_eq!(g.tags().get_extra("Opening"), None);
    assert_eq!(g.tags().event, "UCI Game", "the rest was written");
}

// ---- the result ----

#[test]
fn the_result_is_written_with_the_end() {
    let mut g = tagged(&tag_setup(Color::White, blitz()));
    let root = g.root();
    write_play_result(
        &mut g,
        PlayEnd::Checkmate {
            winner: Color::White,
        },
        root,
    )
    .unwrap();
    assert_eq!(g.tags().result, GameResult::White);
}

#[test]
fn the_automatic_draw_comment_goes_on_the_last_move_not_on_the_root() {
    let mut g = Game::new();
    let mv = g.start_position().parse_uci("e2e4").unwrap();
    let node = g.add_move(g.root(), mv).unwrap();
    write_play_result(&mut g, PlayEnd::FiftyMoves, node).unwrap();
    assert_eq!(g.comment(node).unwrap(), Some("50 move rule"));
    assert_eq!(g.initial_comment(), None);
    assert_eq!(g.tags().result, GameResult::Draw);
    // On a game without moves nothing is commented.
    let mut empty = Game::new();
    let root = empty.root();
    write_play_result(&mut empty, PlayEnd::ThreefoldRepetition, root).unwrap();
    assert_eq!(empty.initial_comment(), None);
    assert_eq!(empty.tags().result, GameResult::Draw);
}

#[test]
fn the_draw_comment_keeps_the_comment_the_move_already_had() {
    let mut g = Game::new();
    let mv = g.start_position().parse_uci("e2e4").unwrap();
    let node = g.add_move(g.root(), mv).unwrap();
    g.append_comment(node, "Last book move (Mini)").unwrap();
    write_play_result(&mut g, PlayEnd::ThreefoldRepetition, node).unwrap();
    let comment = g.comment(node).unwrap().unwrap().to_owned();
    assert!(comment.contains("Last book move (Mini)"), "{comment}");
    assert!(comment.contains("3 fold repetition"), "{comment}");
}

#[test]
fn a_node_that_is_not_in_the_game_is_an_error_not_a_panic() {
    let mut other = Game::new();
    let mv = other.start_position().parse_uci("e2e4").unwrap();
    let foreign = other.add_move(other.root(), mv).unwrap();
    let foreign2 = other
        .add_move(
            foreign,
            other
                .position_at(foreign)
                .unwrap()
                .parse_uci("e7e5")
                .unwrap(),
        )
        .unwrap();
    let mut g = Game::new();
    assert!(write_play_result(&mut g, PlayEnd::FiftyMoves, foreign2).is_err());
}

#[rstest]
#[case(PlayEnd::Checkmate { winner: Color::White }, "1-0")]
#[case(PlayEnd::Checkmate { winner: Color::Black }, "0-1")]
#[case(PlayEnd::Stalemate, "1/2-1/2")]
#[case(PlayEnd::OpponentFailure, "*")]
fn the_result_tag_is_the_pgn_one(#[case] end: PlayEnd, #[case] text: &str) {
    let mut g = tagged(&tag_setup(Color::White, blitz()));
    let root = g.root();
    write_play_result(&mut g, end, root).unwrap();
    let pgn = game_to_string(&g, &WriteOptions::default()).unwrap();
    assert!(pgn.contains(&format!("[Result \"{text}\"]")), "{pgn}");
}

// ---- the whole game as PGN ----

#[test]
fn a_played_game_becomes_pgn_that_reads_back_the_same() {
    let (mut c, _, t0) = begin(setup(Color::White, blitz()));
    play_ucis(&mut c, &["f2f3", "e7e5", "g2g4", "d8h4"], t0);
    assert_eq!(c.result(), GameResult::Black);
    let pgn = game_to_string(c.game(), &WriteOptions::default()).unwrap();
    for needle in [
        "[Event \"UCI Game\"]",
        "[White \"Ana\"]",
        "[Black \"Fake 1\"]",
        "[Result \"0-1\"]",
        "[Timemode \"Time+Bonus 5+3/5+3\"]",
        "[TimeControl \"300+3\"]",
        "[Date \"2026.10.24\"]",
    ] {
        assert!(pgn.contains(needle), "missing {needle} in\n{pgn}");
    }
    assert!(pgn.contains("Qh4#"), "{pgn}");
    let parsed = parse_pgn_text(&pgn, &ReadOptions::default());
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].game.same_content(c.game()));
}

#[test]
fn a_game_from_a_position_carries_its_fen() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    let (mut c, _, t0) = begin(fen_setup(Color::White, TimeMode::Depth(2), fen));
    play_ucis(&mut c, &["f1b5"], t0);
    c.resign(t0);
    let pgn = game_to_string(c.game(), &WriteOptions::default()).unwrap();
    assert!(pgn.contains(&format!("[FEN \"{fen}\"]")), "{pgn}");
    assert!(pgn.contains("[SetUp \"1\"]"), "{pgn}");
    let parsed = parse_pgn_text(&pgn, &ReadOptions::default());
    assert!(parsed[0].game.same_content(c.game()));
}

#[test]
fn hostile_names_cannot_break_the_pgn() {
    let mut s = setup(Color::White, TimeMode::Depth(2));
    s.player_name = "Ana \"The\" Great]\n[Result \"1-0\"]\r\nX".to_owned();
    s.opponent.name = "Engine\" 9.0 \\ ] [".to_owned();
    let (mut c, _, t0) = begin(s);
    play_ucis(&mut c, &["e2e4", "e7e5"], t0);
    c.resign(t0);
    let pgn = game_to_string(c.game(), &WriteOptions::default()).unwrap();
    let parsed = parse_pgn_text(&pgn, &ReadOptions::default());
    assert_eq!(parsed.len(), 1, "one game, no injected tags:\n{pgn}");
    assert_eq!(
        parsed[0].game.tags().result,
        GameResult::Black,
        "the result tag is the one of the game:\n{pgn}"
    );
    assert_eq!(parsed[0].game.mainline().count(), 2);
}

#[test]
fn the_result_of_a_game_in_progress_or_quit_is_an_asterisk() {
    let (mut c, _, t0) = begin(setup(Color::White, TimeMode::Depth(2)));
    play_ucis(&mut c, &["e2e4"], t0);
    let pgn = game_to_string(c.game(), &WriteOptions::default()).unwrap();
    assert!(pgn.contains("[Result \"*\"]"), "{pgn}");
    c.quit(t0);
    let pgn = game_to_string(c.game(), &WriteOptions::default()).unwrap();
    assert!(pgn.contains("[Result \"*\"]"), "{pgn}");
}
