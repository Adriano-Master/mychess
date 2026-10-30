//! M8-1c (slice A) contract tests of the glue between the controller and
//! `AppState` (spec D1501, D1504, D1509, D1510, D1513): the editor follows a
//! take back, the final evaluation of the engine reaches the draw rule, the
//! notices, `PlayView` and `Store clock`. A fake engine answers through
//! synthetic events fed to `update`.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1c_common;

use std::collections::HashSet;
use std::time::Instant;

use m8_1c_common::*;
use mychess_app::{
    ColorChoice, Effect, GameMessage, Nav, PlayEnd, PlayMessage, PlayNotice, PlayPhase, TimeMode,
};
use mychess_core::{Color, GameResult, Move, Position};
use mychess_engine::{Bound, EngineEvent, Score, SearchId, SearchInfo, SearchRequest};

const DEPTH: TimeMode = TimeMode::Depth(2);

fn started(color: ColorChoice, mode: TimeMode) -> (Rig, Vec<Effect>) {
    let mut r = Rig::new();
    let cfg = r.config(color, mode);
    let fx = r.start(cfg);
    (r, fx)
}

/// The engine answers `request` with `mv` after the given reports.
fn answer_with(
    r: &mut Rig,
    request: &SearchRequest,
    mv: Move,
    infos: &[(u8, Score, Bound)],
) -> Vec<Effect> {
    let search = SearchId(5000 + request.tag % 1_000_000);
    r.event(EngineEvent::SearchTagged {
        search,
        tag: request.tag,
    });
    r.event(EngineEvent::SearchStarted(search));
    for (multipv, score, bound) in infos {
        r.event(EngineEvent::Info(SearchInfo {
            search,
            multipv: *multipv,
            score: Some((*score, *bound)),
            ..SearchInfo::default()
        }));
    }
    r.event(EngineEvent::BestMove(mychess_engine::BestMove {
        search,
        mv: Some(mv),
        ponder: None,
    }))
}

/// Plays `plies` quiet moves in the app: the player moves on the board, the
/// engine answers with the given exact evaluation (White's point of view).
/// `fx` holds the effects of the last event (the search to answer).
fn play_plies(
    r: &mut Rig,
    mut fx: Vec<Effect>,
    plies: usize,
    infos: &dyn Fn(usize) -> Vec<(u8, Score, Bound)>,
) -> Vec<Effect> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut pos = Position::new();
    seen.insert(pos.fen().split(' ').take(4).collect::<Vec<_>>().join(" "));
    let mut k = 0;
    for step in 0..plies {
        let pos_now = r.state.session.position().clone();
        let mv = quiet_move(&pos_now, &seen, step + 3);
        pos = pos_now;
        let from = mv.from().unwrap().to_string();
        let to = mv.to().unwrap().to_string();
        // The player is White in these tests: even steps are the player's.
        if step % 2 == 0 {
            fx = r.player_move(&from, &to, Instant::now());
        } else {
            let request = play_search(&fx).expect("the engine was asked");
            fx = answer_with(r, &request, mv, &infos(k));
            k += 1;
        }
        let after = pos.after(mv).unwrap();
        seen.insert(after.fen().split(' ').take(4).collect::<Vec<_>>().join(" "));
    }
    fx
}

fn exact(cp: i32) -> Vec<(u8, Score, Bound)> {
    vec![(1, Score::Cp(cp), Bound::Exact)]
}

// ---- take back ----

#[test]
fn take_back_brings_the_editor_back_to_the_position_and_the_player_moves_again() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    let fx = r.player_move("e2", "e4", Instant::now());
    r.reply(&fx, Some("e7e5"));
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
    let fx = r.send(PlayMessage::TakeBack(Instant::now()));
    assert!(r.mainline().is_empty());
    assert_eq!(r.state.session.position(), &Position::new());
    assert_eq!(r.phase(), PlayPhase::Player);
    assert!(
        commands(&fx).is_empty(),
        "the engine is idle: nothing is sent to it ({fx:?})"
    );
    assert_eq!(r.notice(), None);
    assert_eq!(
        r.state.session.game().tags().get_extra("Takebacks"),
        Some("1")
    );
    // The player plays something else; the engine is asked with that line.
    let fx = r.player_move("d2", "d4", Instant::now());
    let request = play_search(&fx).expect("asked again");
    assert_eq!(request.position.moves.len(), 1);
    assert_eq!(r.mainline(), ["d2d4"]);
}

#[test]
fn take_back_while_the_engine_thinks_is_refused_with_a_notice_and_the_search_goes_on() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    let fx = r.player_move("e2", "e4", Instant::now());
    let fx2 = r.send(PlayMessage::TakeBack(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::NothingToTakeBack));
    assert!(commands(&fx2).is_empty(), "no stop, no new search");
    assert_eq!(r.mainline(), ["e2e4"]);
    assert_eq!(r.phase(), PlayPhase::Opponent);
    // The answer of the engine is accepted.
    r.reply(&fx, Some("e7e5"));
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
}

#[test]
fn take_back_without_a_game_does_nothing() {
    let mut r = Rig::new();
    let fx = r.send(PlayMessage::TakeBack(Instant::now()));
    assert!(fx.is_empty() || commands(&fx).is_empty());
    assert!(!r.state.play_live());
    let fx = r.send(PlayMessage::OfferDraw(Instant::now()));
    assert!(commands(&fx).is_empty());
    assert!(!r.state.play_live());
}

#[test]
fn take_back_after_navigation_needs_resume_and_then_the_editor_shows_the_end() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    let fx = r.player_move("e2", "e4", Instant::now());
    r.reply(&fx, Some("e7e5"));
    // Looking at the start of the game pauses it.
    r.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(r.phase(), PlayPhase::Paused { lost: false });
    assert!(!r.view().can_take_back);
    r.send(PlayMessage::TakeBack(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::NothingToTakeBack));
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
    // After the resume the engine is asked for White's... the player moves
    // again (it was the player's turn), then a take back is possible.
    r.send(PlayMessage::Resume(Instant::now()));
    assert_eq!(r.phase(), PlayPhase::Player);
    assert!(r.view().can_take_back);
    r.send(PlayMessage::TakeBack(Instant::now()));
    assert!(r.mainline().is_empty());
    assert_eq!(r.state.session.position(), &Position::new());
}

#[test]
fn take_back_with_the_editor_cursor_in_the_middle_resynchronises_it() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    let fx = r.player_move("e2", "e4", Instant::now());
    r.reply(&fx, Some("e7e5"));
    let fx = r.player_move("g1", "f3", Instant::now());
    r.reply(&fx, Some("b8c6"));
    r.send(GameMessage::Navigate(Nav::Back(2)));
    r.send(PlayMessage::Resume(Instant::now()));
    r.send(PlayMessage::TakeBack(Instant::now()));
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
    // The editor shows the end of the shortened game, not a stale node.
    let expected = {
        let mut p = Position::new();
        p.play(p.parse_uci("e2e4").unwrap()).unwrap();
        p.play(p.parse_uci("e7e5").unwrap()).unwrap();
        p
    };
    assert_eq!(r.state.session.position(), &expected);
    // And the game has no variation or orphan node.
    assert_eq!(r.state.session.game().node_count(), 3);
}

#[test]
fn the_view_flags_follow_the_phase() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    let v = r.view();
    assert_eq!(v.phase, PlayPhase::Player);
    assert!(!v.can_take_back, "no move of the player yet");
    assert!(v.can_offer_draw);
    let fx = r.player_move("e2", "e4", Instant::now());
    let v = r.view();
    assert_eq!(v.phase, PlayPhase::Opponent);
    assert!(!v.can_take_back);
    assert!(!v.can_offer_draw);
    r.reply(&fx, Some("e7e5"));
    let v = r.view();
    assert_eq!(v.phase, PlayPhase::Player);
    assert!(v.can_take_back);
    assert!(v.can_offer_draw);
    r.send(PlayMessage::Pause(Instant::now()));
    let v = r.view();
    assert!(!v.can_take_back);
    assert!(!v.can_offer_draw);
    r.send(PlayMessage::Resume(Instant::now()));
    r.send(PlayMessage::Resign(Instant::now()));
    let v = r.view();
    assert!(!v.can_take_back);
    assert!(!v.can_offer_draw);
}

// ---- draw offer ----

#[test]
fn a_declined_offer_early_in_the_game_shows_the_notice_and_the_game_goes_on() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::DrawDeclined));
    assert_eq!(r.phase(), PlayPhase::Player);
    assert!(!r.view().can_offer_draw);
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::DrawNotAllowed));
    assert_eq!(r.view().result, GameResult::Unknown);
}

#[test]
fn offering_while_the_engine_thinks_is_not_allowed() {
    let (mut r, _) = started(ColorChoice::White, DEPTH);
    r.player_move("e2", "e4", Instant::now());
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::DrawNotAllowed));
    assert_eq!(r.phase(), PlayPhase::Opponent);
}

#[test]
fn the_exact_evaluation_of_the_engine_reaches_the_draw_rule_and_the_draw_is_recorded() {
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    // The engine (Black) is at 0 in every search.
    play_plies(&mut r, fx, 40, &|_| exact(0));
    assert_eq!(r.phase(), PlayPhase::Player);
    assert_eq!(r.mainline().len(), 40);
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(
        r.notice(),
        Some(PlayNotice::Ended(PlayEnd::DrawAgreed)),
        "{:?}",
        r.notice()
    );
    assert!(!r.state.play_live());
    let game = r.state.session.game();
    assert_eq!(game.tags().result, GameResult::Draw);
    assert_eq!(
        game.comment(game.mainline().last().unwrap()).unwrap(),
        Some("Draw agreed")
    );
    assert!(r.state.session.is_dirty(), "the game is left to be saved");
}

#[test]
fn a_losing_engine_accepts_and_a_winning_engine_does_not() {
    // Player White: +300 for White is -300 for the engine.
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    play_plies(&mut r, fx, 40, &|_| exact(300));
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::Ended(PlayEnd::DrawAgreed)));
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    play_plies(&mut r, fx, 40, &|_| exact(-300));
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::DrawDeclined));
    assert!(r.state.play_live());
}

#[test]
fn only_exact_evaluations_of_the_best_line_count() {
    // Every search reports a quiet evaluation but only as a bound, or on the
    // second line: the final evaluation is absent and the engine declines.
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    play_plies(&mut r, fx, 40, &|_| {
        vec![
            (1, Score::Cp(0), Bound::Lower),
            (1, Score::Cp(0), Bound::Upper),
            (2, Score::Cp(0), Bound::Exact),
        ]
    });
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::DrawDeclined));
}

#[test]
fn the_last_exact_evaluation_of_the_search_is_the_final_one() {
    // The search starts bad for the engine (+400 for it) and settles at 0:
    // the last exact line is the one that counts.
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    play_plies(&mut r, fx, 40, &|_| {
        vec![
            (1, white_pov(Color::White, 400), Bound::Exact),
            (1, white_pov(Color::White, 0), Bound::Exact),
            (1, white_pov(Color::White, 400), Bound::Upper),
        ]
        .into_iter()
        .collect()
    });
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::Ended(PlayEnd::DrawAgreed)));
}

#[test]
fn a_mate_score_against_the_engine_is_accepted_by_the_glue() {
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    // Mate(2): White mates; the engine is Black.
    play_plies(&mut r, fx, 40, &|_| vec![(1, Score::Mate(2), Bound::Exact)]);
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::Ended(PlayEnd::DrawAgreed)));
}

#[test]
fn an_engine_that_sends_no_info_declines_every_offer() {
    let (mut r, fx) = started(ColorChoice::White, DEPTH);
    play_plies(&mut r, fx, 40, &|_| Vec::new());
    r.send(PlayMessage::OfferDraw(Instant::now()));
    assert_eq!(r.notice(), Some(PlayNotice::DrawDeclined));
}

// ---- store clock ----

#[test]
fn store_clock_writes_the_comments_in_the_game_of_the_editor() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, blitz());
    cfg.store_clock = true;
    r.start(cfg);
    let fx = r.player_move("e2", "e4", Instant::now());
    r.reply(&fx, Some("e7e5"));
    let game = r.state.session.game();
    let comments: Vec<String> = game
        .mainline()
        .filter(|id| *id != game.root())
        .map(|id| game.comment(id).unwrap().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(comments.len(), 2);
    for text in &comments {
        assert!(text.starts_with("[%emt 0:00:0"), "{text}");
        assert!(
            text.contains("[%clk 0:05:0") || text.contains("[%clk 0:04:5"),
            "{text}"
        );
    }
}

#[test]
fn store_clock_is_off_unless_asked() {
    let (mut r, _) = started(ColorChoice::White, blitz());
    let fx = r.player_move("e2", "e4", Instant::now());
    r.reply(&fx, Some("e7e5"));
    let game = r.state.session.game();
    assert!(
        game.mainline()
            .filter(|id| *id != game.root())
            .all(|id| game.comment(id).unwrap().is_none())
    );
}
