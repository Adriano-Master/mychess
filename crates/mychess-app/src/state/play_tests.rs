//! Tests of the game against an engine in the state: a fake engine answers
//! through synthetic events fed to `update` (spec M8-1, phase A).

use std::time::{Duration, Instant};

use mychess_core::{Color, GameResult, Move, Position, Square};
use mychess_engine::{
    BestMove, Capabilities, EngineCommand, EngineEvent, EngineIdentity, EngineOption, OptionKind,
    OptionValue, Protocol, SearchId, SearchRequest,
};

use super::{AppState, update};
use crate::analysis::AnalysisMessage;
use crate::annotate::AnnotateMessage;
use crate::effect::Effect;
use crate::engines::{EngineDraft, EngineId, EngineMessage, EngineRunState, RunId};
use crate::evalbar::EvalBarMessage;
use crate::flow::AppNotice;
use crate::message::Message;
use crate::play::{
    BookFailure, BookSelection, ColorChoice, PlayBook, PlayConfig, PlayEnd, PlayMessage,
    PlayNotice, PlayPhase, StartFrom, Strength, TimeMode,
};
use crate::session::{GameMessage, Nav};
use crate::training::ModeMessage;

const SEC: Duration = Duration::from_secs(1);

fn identity(options: Vec<EngineOption>) -> EngineIdentity {
    EngineIdentity {
        name: "Fake 1".to_owned(),
        author: None,
        options,
        capabilities: Capabilities::default(),
    }
}

fn setup_with(options: Vec<EngineOption>, draft: EngineDraft) -> (AppState, RunId, EngineId) {
    let mut state = AppState::default();
    let id = state.engines.add(draft).expect("add");
    let run = state.engine_runs.begin(id, true);
    let r = state.engine_runs.get_mut(run).expect("run");
    r.state = EngineRunState::Running;
    r.identity = Some(Box::new(identity(options)));
    state.flow.play.fixed_now = Some(Instant::now());
    state.flow.play.fixed_roll = Some(0);
    (state, run, id)
}

fn setup() -> (AppState, RunId, EngineId) {
    setup_with(Vec::new(), EngineDraft::new("Fake", "/fake"))
}

fn config(engine: EngineId, color: ColorChoice, mode: TimeMode) -> PlayConfig {
    let mut c = PlayConfig::new(engine);
    c.color = color;
    c.mode = mode;
    c
}

fn send(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    update(state, message.into())
}

fn start(state: &mut AppState, config: PlayConfig) -> Vec<Effect> {
    send(state, PlayMessage::Start(Box::new(config)))
}

fn commands(effects: &[Effect]) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand { command, .. } => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

fn last_search(effects: &[Effect]) -> Option<SearchRequest> {
    commands(effects).into_iter().rev().find_map(|c| match c {
        EngineCommand::Search(r) => Some(r),
        _ => None,
    })
}

fn event(state: &mut AppState, run: RunId, event: EngineEvent) -> Vec<Effect> {
    send(
        state,
        EngineMessage::Event {
            run,
            event: Box::new(event),
        },
    )
}

/// The engine answers the search of `effects` with `uci`.
fn reply(state: &mut AppState, run: RunId, effects: &[Effect], id: u64, uci: &str) -> Vec<Effect> {
    let request = last_search(effects).expect("a search was sent");
    let mut pos = request.position.start.clone();
    for m in &request.position.moves {
        pos.play(*m).expect("legal");
    }
    let mv = pos.parse_uci(uci).expect("uci");
    answer(state, run, &request, id, Some(mv))
}

fn answer(
    state: &mut AppState,
    run: RunId,
    request: &SearchRequest,
    id: u64,
    mv: Option<Move>,
) -> Vec<Effect> {
    let search = SearchId(id);
    event(
        state,
        run,
        EngineEvent::SearchTagged {
            search,
            tag: request.tag,
        },
    );
    event(state, run, EngineEvent::SearchStarted(search));
    event(
        state,
        run,
        EngineEvent::BestMove(BestMove {
            search,
            mv,
            ponder: None,
        }),
    )
}

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn play_move(state: &mut AppState, from: &str, to: &str) -> Vec<Effect> {
    let at = state.flow.play.fixed_now.expect("fixed");
    send(
        state,
        PlayMessage::PlayerMove {
            from: sq(from),
            to: sq(to),
            promotion: None,
            at,
        },
    )
}

fn view(state: &AppState) -> crate::play::PlayView {
    let now = state.flow.play.fixed_now.expect("fixed");
    state.play_view(now).expect("a game")
}

fn notice(state: &AppState) -> Option<PlayNotice> {
    match &state.notice {
        Some(AppNotice::Play(n)) => Some(*n),
        _ => None,
    }
}

fn now(state: &AppState) -> Instant {
    state.flow.play.fixed_now.expect("fixed")
}

// ---- starting ----

#[test]
fn an_unknown_engine_does_not_start() {
    let (mut state, _, _) = setup();
    start(
        &mut state,
        config(EngineId(99), ColorChoice::White, TimeMode::Depth(2)),
    );
    assert_eq!(notice(&state), Some(PlayNotice::NoEngine));
    assert!(!state.play_live());
}

#[test]
fn only_uci_engines_play() {
    let mut draft = EngineDraft::new("Old", "/old");
    draft.protocol = Protocol::XBoard;
    let (mut state, _, id) = setup_with(Vec::new(), draft);
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert_eq!(notice(&state), Some(PlayNotice::NotUci));
}

#[test]
fn a_game_with_the_player_as_white_waits_for_the_first_move() {
    let (mut state, run, id) = setup();
    let effects = start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert!(state.play_live());
    assert!(last_search(&effects).is_none());
    let v = view(&state);
    assert_eq!(v.phase, PlayPhase::Player);
    assert_eq!(v.player_color, Color::White);
    assert_eq!(v.opponent.name, "Fake 1");
    assert!(!state.board_flipped);
    // The analysis of the board leaves the engine (D1241).
    let r = state.engine_runs.get(run).expect("run");
    assert!(!r.analyzing);
    assert!(state.play_uses(run));
    assert!(commands(&effects).contains(&EngineCommand::NewGame));
    assert_eq!(state.session.game().tags().event, "UCI Game");
}

#[test]
fn a_black_player_turns_the_board_and_the_engine_moves_first() {
    let (mut state, run, id) = setup();
    let effects = start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    assert!(state.board_flipped);
    let request = last_search(&effects).expect("the engine thinks");
    assert!(request.position.moves.is_empty());
    assert_eq!(request.limits.depth, Some(2));
    assert!(crate::state::play_flow::is_play_tag(request.tag));
    let effects = reply(&mut state, run, &effects, 10, "e2e4");
    assert!(last_search(&effects).is_none());
    assert_eq!(state.session.game().mainline().count(), 1);
    assert_eq!(view(&state).phase, PlayPhase::Player);
}

#[test]
fn a_busy_state_or_an_unsaved_game_refuses_to_start() {
    let (mut state, _, id) = setup();
    let first = state.session.game().start_position().clone();
    let _ = first;
    // An altered game with moves would be lost.
    let mut game = mychess_core::Game::new();
    let mv = game.start_position().parse_uci("e2e4").expect("uci");
    game.add_move(game.root(), mv).expect("move");
    state.session.replace_game(game);
    state.session.mark_altered();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert_eq!(notice(&state), Some(PlayNotice::UnsavedGame));
    assert!(!state.play_live());
}

// ---- the round trip ----

#[test]
fn every_mode_makes_its_own_search() {
    for (mode, check) in [
        (
            TimeMode::MoveTime(6),
            (|r: &SearchRequest| r.limits.movetime == Some(6 * SEC)) as fn(&SearchRequest) -> bool,
        ),
        (TimeMode::Nodes(1000), |r| r.limits.nodes == Some(1000)),
        (TimeMode::Depth(5), |r| r.limits.depth == Some(5)),
        (
            TimeMode::TimeBonus {
                white_minutes: 5,
                white_bonus: 3,
                black_minutes: 5,
                black_bonus: 3,
            },
            |r| {
                r.limits
                    .clock
                    .is_some_and(|c| c.white_time == 300 * SEC && c.white_increment == 3 * SEC)
            },
        ),
    ] {
        let (mut state, _, id) = setup();
        let effects = start(&mut state, config(id, ColorChoice::Black, mode));
        let request = last_search(&effects).expect("search");
        assert!(check(&request), "{mode:?}: {:?}", request.limits);
    }
}

#[test]
fn the_player_and_the_engine_alternate_and_the_game_is_mirrored() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let effects = play_move(&mut state, "e2", "e4");
    let request = last_search(&effects).expect("the engine thinks");
    assert_eq!(request.position.moves.len(), 1);
    assert_eq!(view(&state).phase, PlayPhase::Opponent);
    reply(&mut state, run, &effects, 11, "e7e5");
    assert_eq!(state.session.game().mainline().count(), 2);
    assert_eq!(state.session.position().fen(), {
        let mut p = Position::new();
        for u in ["e2e4", "e7e5"] {
            let m = p.parse_uci(u).expect("uci");
            p.play(m).expect("play");
        }
        p.fen()
    });
    assert_eq!(view(&state).phase, PlayPhase::Player);
}

#[test]
fn an_illegal_move_of_the_player_is_refused_with_a_notice() {
    let (mut state, _, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    play_move(&mut state, "e2", "e5");
    assert_eq!(notice(&state), Some(PlayNotice::IllegalMove));
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn mate_ends_the_game_saves_the_result_and_gives_the_engine_back() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    for (n, (from, to, reply_uci)) in [("f2", "f3", "e7e5"), ("g2", "g4", "d8h4")]
        .into_iter()
        .enumerate()
    {
        let effects = play_move(&mut state, from, to);
        reply(&mut state, run, &effects, 20 + n as u64, reply_uci);
    }
    assert_eq!(
        notice(&state),
        Some(PlayNotice::Ended(PlayEnd::Checkmate {
            winner: Color::Black
        }))
    );
    assert_eq!(state.session.game().tags().result, GameResult::Black);
    assert!(state.session.is_dirty(), "the game waits to be saved");
    assert!(!state.play_live(), "the editor is the user's again");
    let r = state.engine_runs.get(run).expect("run");
    assert!(r.analyzing, "the analysis comes back");
    assert!(!state.play_wants_ticks());
}

#[test]
fn resign_records_the_loss() {
    let (mut state, _, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let at = now(&state);
    send(&mut state, PlayMessage::Resign(at));
    assert_eq!(state.session.game().tags().result, GameResult::Black);
    assert!(matches!(
        notice(&state),
        Some(PlayNotice::Ended(PlayEnd::Resignation {
            loser: Color::White
        }))
    ));
}

#[test]
fn quit_leaves_no_result_and_no_alteration() {
    let (mut state, run, id) = setup();
    let effects = start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    let at = now(&state);
    let out = send(&mut state, PlayMessage::Quit(at));
    assert!(commands(&out).contains(&EngineCommand::Stop));
    assert!(!state.play_live());
    assert!(state.play_view(at).is_none());
    assert_eq!(state.session.game().tags().result, GameResult::Unknown);
    assert!(!state.session.is_dirty());
    assert!(state.engine_runs.get(run).expect("run").analyzing);
    // The late answer of the stopped search is not for the analysis.
    let late = last_search(&effects).expect("search");
    let after = answer(
        &mut state,
        run,
        &late,
        30,
        late.position.start.legal_moves().first().copied(),
    );
    assert!(after.is_empty() || last_search(&after).is_none());
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn the_flag_of_the_player_loses_on_a_tick() {
    let (mut state, _, id) = setup();
    let mode = TimeMode::TimeBonus {
        white_minutes: 1,
        white_bonus: 0,
        black_minutes: 1,
        black_bonus: 0,
    };
    start(&mut state, config(id, ColorChoice::White, mode));
    assert!(state.play_wants_ticks());
    let t0 = now(&state);
    assert_eq!(view(&state).white_time, 60 * SEC);
    send(&mut state, PlayMessage::Tick(t0 + 30 * SEC));
    assert!(state.play_live());
    send(&mut state, PlayMessage::Tick(t0 + 61 * SEC));
    assert!(matches!(
        notice(&state),
        Some(PlayNotice::Ended(PlayEnd::Timeout {
            loser: Color::White
        }))
    ));
    assert_eq!(state.session.game().tags().result, GameResult::Black);
}

#[test]
fn an_engine_without_a_move_ends_the_game_without_result() {
    let (mut state, run, id) = setup();
    let effects = start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    let request = last_search(&effects).expect("search");
    answer(&mut state, run, &request, 40, None);
    assert!(matches!(
        notice(&state),
        Some(PlayNotice::Ended(PlayEnd::OpponentFailure))
    ));
    assert_eq!(state.session.game().tags().result, GameResult::Unknown);
}

// ---- the game of the editor ----

#[test]
fn editing_is_refused_and_navigation_pauses_then_resume_goes_on() {
    let (mut state, run, id) = setup();
    let effects = start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    // An edit is refused.
    send(&mut state, GameMessage::PlayNullMove);
    assert_eq!(notice(&state), Some(PlayNotice::Locked));
    assert!(state.play_live());
    // A new game is refused too.
    send(&mut state, crate::flow::Command::NewGame);
    assert!(state.play_live());
    // Navigation pauses (D1240) and the engine is told to stop.
    let out = send(&mut state, GameMessage::Navigate(Nav::Start));
    assert!(commands(&out).contains(&EngineCommand::Stop));
    assert_eq!(view(&state).phase, PlayPhase::Paused { lost: false });
    assert!(view(&state).can_resume);
    // The answer of the stopped search is dropped.
    let late = last_search(&effects).expect("search");
    let mv = late.position.start.parse_uci("e2e4").expect("uci");
    answer(&mut state, run, &late, 50, Some(mv));
    assert_eq!(state.session.game().mainline().count(), 0);
    // Resume clears the hash (when the engine can) and thinks again.
    let at = now(&state);
    let out = send(&mut state, PlayMessage::Resume(at));
    assert!(last_search(&out).is_some());
    assert_eq!(view(&state).phase, PlayPhase::Opponent);
}

#[test]
fn resume_sends_clear_hash_when_the_engine_has_the_button() {
    let button = EngineOption {
        name: "Clear Hash".to_owned(),
        kind: OptionKind::Button,
    };
    let (mut state, _, id) = setup_with(vec![button], EngineDraft::new("Fake", "/fake"));
    start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    let at = now(&state);
    send(&mut state, PlayMessage::Pause(at));
    let out = send(&mut state, PlayMessage::Resume(at));
    let sent = commands(&out);
    let clear = sent
        .iter()
        .position(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Clear Hash"));
    let search = sent
        .iter()
        .position(|c| matches!(c, EngineCommand::Search(_)));
    assert!(
        clear.is_some() && search.is_some() && clear < search,
        "{sent:?}"
    );
}

#[test]
fn restart_begins_again_with_the_same_configuration() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let effects = play_move(&mut state, "e2", "e4");
    reply(&mut state, run, &effects, 60, "e7e5");
    let at = now(&state);
    send(&mut state, PlayMessage::Restart(at));
    assert!(state.play_live());
    assert_eq!(state.session.game().mainline().count(), 0);
    assert_eq!(view(&state).phase, PlayPhase::Player);
}

// ---- strength ----

#[test]
fn limited_strength_options_are_sent_inside_the_engine_ranges() {
    let options = vec![
        EngineOption {
            name: "UCI_LimitStrength".to_owned(),
            kind: OptionKind::Check { default: false },
        },
        EngineOption {
            name: "UCI_Elo".to_owned(),
            kind: OptionKind::Spin {
                default: 1500,
                min: 1320,
                max: 3190,
            },
        },
        EngineOption {
            name: "Skill Level".to_owned(),
            kind: OptionKind::Spin {
                default: 20,
                min: 0,
                max: 20,
            },
        },
    ];
    let (mut state, _, id) = setup_with(options, EngineDraft::new("Fake", "/fake"));
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.strength = Strength {
        elo: Some(1000),
        skill: Some(50),
    };
    let effects = start(&mut state, c);
    let sent = commands(&effects);
    assert!(sent.contains(&EngineCommand::SetOption {
        name: "UCI_Elo".to_owned(),
        value: OptionValue::Int(1320),
    }));
    assert!(sent.contains(&EngineCommand::SetOption {
        name: "Skill Level".to_owned(),
        value: OptionValue::Int(20),
    }));
    // The tag records what was asked (D1245); the engine got the clamped one.
    assert_eq!(
        state.session.game().tags().get_extra("UCI_Elo"),
        Some("1000")
    );
}

#[test]
fn options_the_engine_does_not_declare_are_not_sent() {
    let (mut state, _, id) = setup();
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.strength = Strength {
        elo: Some(1500),
        skill: Some(5),
    };
    let effects = start(&mut state, c);
    assert!(
        !commands(&effects)
            .iter()
            .any(|c| matches!(c, EngineCommand::SetOption { .. }))
    );
}

// ---- the book ----

fn book_config(id: EngineId) -> PlayConfig {
    let mut c = config(id, ColorChoice::Black, TimeMode::Depth(2));
    c.book = Some(crate::play::BookChoice {
        path: std::path::PathBuf::from("book.bin"),
        selection: BookSelection::Best,
    });
    c
}

fn sample_book() -> PlayBook {
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").expect("uci");
    let mut book = mychess_book::Book::new();
    book.set_position_moves(&start, &[(e4, 100)]).expect("set");
    PlayBook::new(book, BookSelection::Best)
}

#[test]
fn the_book_is_read_before_the_game_begins_and_plays_for_the_engine() {
    let (mut state, _, id) = setup();
    let effects = start(&mut state, book_config(id));
    let request = effects.iter().find_map(|e| match e {
        Effect::LoadPlayBook { request, .. } => Some(*request),
        _ => None,
    });
    let request = request.expect("the book is read by an effect");
    assert!(state.play_live());
    assert!(state.play_view(now(&state)).is_none(), "not begun yet");
    let out = send(
        &mut state,
        PlayMessage::BookLoaded {
            request,
            result: Ok(sample_book()),
        },
    );
    // The engine (White) played e4 from the book: no search at all.
    assert!(last_search(&out).is_none());
    assert_eq!(state.session.game().mainline().count(), 1);
    assert_eq!(view(&state).phase, PlayPhase::Player);
}

#[test]
fn an_unreadable_book_warns_and_the_game_goes_on_without_it() {
    let (mut state, _, id) = setup();
    let effects = start(&mut state, book_config(id));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::LoadPlayBook { request, .. } => Some(*request),
            _ => None,
        })
        .expect("request");
    let out = send(
        &mut state,
        PlayMessage::BookLoaded {
            request,
            result: Err(BookFailure::NotFound),
        },
    );
    assert!(last_search(&out).is_some(), "the engine thinks instead");
    // The warning about the book is kept for the user.
    assert!(state.play_live());
}

#[test]
fn a_stale_book_answer_is_ignored() {
    let (mut state, _, id) = setup();
    start(&mut state, book_config(id));
    send(
        &mut state,
        PlayMessage::BookLoaded {
            request: 9999,
            result: Ok(sample_book()),
        },
    );
    assert!(state.play_view(now(&state)).is_none());
}

#[test]
fn leaving_the_book_raises_the_question_in_the_view() {
    let (mut state, _, id) = setup();
    let effects = start(&mut state, book_config(id));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::LoadPlayBook { request, .. } => Some(*request),
            _ => None,
        })
        .expect("request");
    let mut book = mychess_book::Book::new();
    let start_pos = Position::new();
    let e4 = start_pos.parse_uci("e2e4").expect("uci");
    let after = start_pos.after(e4).expect("after");
    let e5 = after.parse_uci("e7e5").expect("uci");
    book.set_position_moves(&start_pos, &[(e4, 10)])
        .expect("set");
    book.set_position_moves(&after, &[(e5, 10)]).expect("set");
    send(
        &mut state,
        PlayMessage::BookLoaded {
            request,
            result: Ok(PlayBook::new(book, BookSelection::Best)),
        },
    );
    play_move(&mut state, "c7", "c5");
    assert!(view(&state).question.is_some());
    let at = now(&state);
    send(&mut state, PlayMessage::Answer { yes: false, at });
    assert!(view(&state).question.is_none());
    assert_eq!(state.session.game().mainline().count(), 1);
    assert_eq!(notice(&state), Some(PlayNotice::MoveTakenBack));
}

// ---- the opening and the position ----

#[test]
fn a_specific_opening_is_played_and_tagged() {
    let (mut state, _, id) = setup();
    let mut c = config(id, ColorChoice::Black, TimeMode::Depth(2));
    c.start =
        StartFrom::Opening(crate::play::OpeningLine::parse("Sicilian: e4 c5 Nf3").expect("line"));
    start(&mut state, c);
    assert_eq!(state.session.game().mainline().count(), 1);
    assert_eq!(
        state.session.game().tags().get_extra("Opening"),
        Some("Sicilian")
    );
    play_move(&mut state, "c7", "c5");
    assert_eq!(state.session.game().mainline().count(), 3);
}

#[test]
fn a_bad_opening_aborts_and_gives_the_engine_back() {
    let (mut state, run, id) = setup();
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.start = StartFrom::Opening(crate::play::OpeningLine::parse("Bad: e4 e4").expect("line"));
    start(&mut state, c);
    assert_eq!(notice(&state), Some(PlayNotice::BadOpening));
    assert!(!state.play_live());
    assert!(state.engine_runs.get(run).expect("run").analyzing);
}

#[test]
fn the_game_starts_from_the_position_of_the_board() {
    let (mut state, _, id) = setup();
    let pos = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1").expect("fen");
    state
        .session
        .replace_game(mychess_core::Game::from_position(pos.clone()));
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.start = StartFrom::CurrentPosition;
    start(&mut state, c);
    assert_eq!(state.session.game().start_position(), &pos);
}

// ---- the engine is lost ----

#[test]
fn a_dead_engine_pauses_the_game_and_another_one_resumes_it() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    send(&mut state, EngineMessage::Stopped(run));
    assert_eq!(notice(&state), Some(PlayNotice::EngineLost));
    let v = view(&state);
    assert!(v.engine_lost && !v.can_resume);
    // Resume alone does nothing.
    let at = now(&state);
    send(&mut state, PlayMessage::Resume(at));
    assert!(view(&state).engine_lost);
    // A second engine takes over.
    let id2 = state
        .engines
        .add(EngineDraft::new("Second", "/second"))
        .expect("add");
    let run2 = state.engine_runs.begin(id2, true);
    {
        let r = state.engine_runs.get_mut(run2).expect("run");
        r.state = EngineRunState::Running;
        r.identity = Some(Box::new(EngineIdentity {
            name: "Second 2".to_owned(),
            ..identity(Vec::new())
        }));
    }
    let out = send(&mut state, PlayMessage::SwitchEngine(id2, at));
    assert!(last_search(&out).is_some(), "the new engine thinks");
    assert_eq!(view(&state).opponent.name, "Second 2");
    assert_eq!(state.session.game().tags().white, "Second 2");
    assert!(state.play_uses(run2));
}

#[test]
fn a_silent_engine_is_given_up_on_a_tick() {
    let (mut state, _, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::MoveTime(2)),
    );
    let t0 = now(&state);
    send(&mut state, PlayMessage::Tick(t0 + 10 * SEC));
    assert!(
        view(&state).phase == PlayPhase::Opponent,
        "still within the grace"
    );
    send(&mut state, PlayMessage::Tick(t0 + 40 * SEC));
    assert!(view(&state).engine_lost);
}

#[test]
fn a_second_game_cannot_start_over_the_first() {
    let (mut state, _, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert_eq!(notice(&state), Some(PlayNotice::AlreadyRunning));
}

// ---- review fixes ----

fn ready_engine(state: &mut AppState, name: &str, analyzing: bool) -> (RunId, EngineId) {
    let id = state
        .engines
        .add(EngineDraft::new(name, "/other"))
        .expect("add");
    let run = state.engine_runs.begin(id, analyzing);
    let r = state.engine_runs.get_mut(run).expect("run");
    r.state = EngineRunState::Running;
    r.identity = Some(Box::new(EngineIdentity {
        name: format!("{name} 1"),
        ..identity(Vec::new())
    }));
    (run, id)
}

fn sent_to(effects: &[Effect], run: RunId) -> Vec<EngineCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::EngineCommand {
                run: r, command, ..
            } if *r == run => Some((**command).clone()),
            _ => None,
        })
        .collect()
}

fn resign(state: &mut AppState) -> Vec<Effect> {
    let at = now(state);
    send(state, PlayMessage::Resign(at))
}

#[test]
fn annotation_and_training_modes_are_refused_during_the_game() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert!(state.play_live());
    let effects = send(&mut state, AnnotateMessage::Start);
    assert!(!state.annotation_running());
    assert_eq!(notice(&state), Some(PlayNotice::Busy));
    assert!(effects.is_empty());
    // The game's own engine and another one are both out of reach.
    let (other, _) = ready_engine(&mut state, "Other", false);
    for r in [run, other] {
        for m in [
            ModeMessage::ToggleFinishGame(r),
            ModeMessage::ToggleAutomove(r),
            ModeMessage::ToggleThreat(r),
        ] {
            state.notice = None;
            let effects = send(&mut state, Message::Mode(m));
            assert!(
                commands(&effects)
                    .iter()
                    .all(|c| !matches!(c, EngineCommand::Search(_)))
            );
            assert_eq!(notice(&state), Some(PlayNotice::Busy), "{r:?}");
        }
        let a = state.mode_availability(r);
        assert!(!a.threat && !a.finish_game && !a.training);
    }
    assert!(state.play_live());
}

#[test]
fn analysis_events_after_the_game_reach_the_analysis() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let effects = play_move(&mut state, "e2", "e4");
    reply(&mut state, run, &effects, 1, "e7e5");
    // The game is over (resigned) and the engine goes back to analysing.
    let effects = resign(&mut state);
    assert!(!state.play_live());
    let request = last_search(&effects).expect("the analysis restarts");
    assert!(!is_play(&request), "a search of the analysis");
    event(
        &mut state,
        run,
        EngineEvent::SearchTagged {
            search: SearchId(77),
            tag: request.tag,
        },
    );
    event(&mut state, run, EngineEvent::SearchStarted(SearchId(77)));
    let mut pos = Position::new();
    for u in ["e2e4", "e7e5"] {
        let m = pos.parse_uci(u).expect("uci");
        pos.play(m).expect("play");
    }
    let best = pos.parse_uci("g1f3").expect("uci");
    event(
        &mut state,
        run,
        EngineEvent::Info(mychess_engine::SearchInfo {
            search: SearchId(77),
            multipv: 1,
            depth: Some(5),
            score: Some((mychess_engine::Score::Cp(20), mychess_engine::Bound::Exact)),
            pv: vec![best],
            ..mychess_engine::SearchInfo::default()
        }),
    );
    let view = state.analysis_view(run).expect("view");
    assert_eq!(view.lines.len(), 1, "the report was not swallowed");
}

fn is_play(request: &SearchRequest) -> bool {
    super::play_flow::is_play_tag(request.tag)
}

fn strength_options() -> Vec<EngineOption> {
    vec![
        EngineOption {
            name: "UCI_LimitStrength".to_owned(),
            kind: OptionKind::Check { default: false },
        },
        EngineOption {
            name: "UCI_Elo".to_owned(),
            kind: OptionKind::Spin {
                default: 1500,
                min: 1320,
                max: 3190,
            },
        },
        EngineOption {
            name: "Skill Level".to_owned(),
            kind: OptionKind::Spin {
                default: 20,
                min: 0,
                max: 20,
            },
        },
    ]
}

fn limited(id: EngineId) -> PlayConfig {
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.strength = Strength {
        elo: Some(1400),
        skill: Some(5),
    };
    c
}

#[test]
fn limited_strength_is_put_back_when_the_game_ends() {
    let (mut state, run, id) = setup_with(strength_options(), EngineDraft::new("Fake", "/fake"));
    // The user had saved an Elo for this engine.
    state
        .engines
        .set_options(id, vec![("UCI_Elo".to_owned(), "2200".to_owned())])
        .expect("options");
    start(&mut state, limited(id));
    let effects = resign(&mut state);
    let sent = sent_to(&effects, run);
    let set = |name: &str, value: OptionValue| EngineCommand::SetOption {
        name: name.to_owned(),
        value,
    };
    assert!(sent.contains(&set("UCI_LimitStrength", OptionValue::Bool(false))));
    assert!(sent.contains(&set("UCI_Elo", OptionValue::Int(2200))));
    assert!(sent.contains(&set("Skill Level", OptionValue::Int(20))));
}

#[test]
fn limited_strength_is_put_back_on_quit_restart_and_switch() {
    let (mut state, run, id) = setup_with(strength_options(), EngineDraft::new("Fake", "/fake"));
    start(&mut state, limited(id));
    let at = now(&state);
    let effects = send(&mut state, PlayMessage::Quit(at));
    assert!(sent_to(&effects, run).contains(&EngineCommand::SetOption {
        name: "Skill Level".to_owned(),
        value: OptionValue::Int(20),
    }));
    // Restart: the old settings go back, then the new game sets them again.
    start(&mut state, limited(id));
    let effects = send(&mut state, PlayMessage::Restart(at));
    let sent = sent_to(&effects, run);
    let skill = |v| EngineCommand::SetOption {
        name: "Skill Level".to_owned(),
        value: OptionValue::Int(v),
    };
    let first = sent.iter().position(|c| *c == skill(20)).expect("restored");
    let second = sent.iter().position(|c| *c == skill(5)).expect("set again");
    assert!(first < second);
    // A silent engine that is replaced is put back too.
    play_move(&mut state, "e2", "e4");
    send(&mut state, PlayMessage::Tick(at + 400 * SEC));
    assert!(view(&state).engine_lost);
    let (_, id2) = ready_engine(&mut state, "Second", false);
    let effects = send(&mut state, PlayMessage::SwitchEngine(id2, at));
    assert!(sent_to(&effects, run).contains(&skill(20)));
}

#[test]
fn other_analyses_stop_for_the_game_and_come_back() {
    let (mut state, run, id) = setup();
    let (other, _) = ready_engine(&mut state, "Other", true);
    state.engine_runs.get_mut(run).expect("run").analyzing = false;
    let effects = start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert!(sent_to(&effects, other).contains(&EngineCommand::Stop));
    assert!(!state.engine_runs.get(other).expect("run").analyzing);
    // Nothing starts it again while the game goes on.
    let effects = play_move(&mut state, "e2", "e4");
    assert!(
        sent_to(&effects, other)
            .iter()
            .all(|c| !matches!(c, EngineCommand::Search(_)))
    );
    let effects = resign(&mut state);
    assert!(state.engine_runs.get(other).expect("run").analyzing);
    assert!(
        sent_to(&effects, other)
            .iter()
            .any(|c| matches!(c, EngineCommand::Search(_))),
        "the analysis is back"
    );
}

#[test]
fn an_analysis_turned_on_during_the_game_waits_for_the_end() {
    let (mut state, _, id) = setup();
    let (other, _) = ready_engine(&mut state, "Other", false);
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let effects = send(&mut state, AnalysisMessage::Toggle(other));
    assert!(
        sent_to(&effects, other)
            .iter()
            .all(|c| !matches!(c, EngineCommand::Search(_)))
    );
    let effects = resign(&mut state);
    assert!(
        sent_to(&effects, other)
            .iter()
            .any(|c| matches!(c, EngineCommand::Search(_)))
    );
}

#[test]
fn arrows_and_the_bar_are_suppressed_during_the_game() {
    let (mut state, _, id) = setup();
    let (other, _) = ready_engine(&mut state, "Other", true);
    send(
        &mut state,
        Message::EvalBar(EvalBarMessage::ToggleArrows(other)),
    );
    assert_eq!(state.arrows_engine(), Some(other));
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    assert_eq!(state.arrows_engine(), None);
    assert!(state.engine_arrows().is_empty());
    assert!(state.traffic_lights().is_empty());
    assert_eq!(
        state.eval_bar_view().source,
        crate::evalbar::EvalSource::None
    );
    resign(&mut state);
    assert_eq!(state.arrows_engine(), Some(other));
}

#[test]
fn restart_from_the_current_position_keeps_the_start_of_the_game() {
    let (mut state, run, id) = setup();
    let pos = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1").expect("fen");
    state
        .session
        .replace_game(mychess_core::Game::from_position(pos.clone()));
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.start = StartFrom::CurrentPosition;
    start(&mut state, c);
    let effects = play_move(&mut state, "e2", "e3");
    reply(&mut state, run, &effects, 1, "d8c8");
    assert_eq!(state.session.game().mainline().count(), 2);
    let at = now(&state);
    send(&mut state, PlayMessage::Restart(at));
    assert!(state.play_live());
    assert_eq!(state.session.game().start_position(), &pos);
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn restart_after_the_end_also_keeps_the_start() {
    let (mut state, _, id) = setup();
    let pos = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1").expect("fen");
    state
        .session
        .replace_game(mychess_core::Game::from_position(pos.clone()));
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    c.start = StartFrom::CurrentPosition;
    start(&mut state, c);
    play_move(&mut state, "e2", "e3");
    resign(&mut state);
    let at = now(&state);
    send(&mut state, PlayMessage::Restart(at));
    assert_eq!(state.session.game().start_position(), &pos);
}

fn lose_engine_of(state: &mut AppState, run: RunId) {
    send(state, EngineMessage::Stopped(run));
    assert!(view(state).engine_lost);
}

#[test]
fn a_switch_that_finishes_after_the_game_ended_prepares_nothing() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    lose_engine_of(&mut state, run);
    // A second engine that is still starting.
    let id2 = state
        .engines
        .add(EngineDraft::new("Second", "/second"))
        .expect("add");
    let run2 = state.engine_runs.begin(id2, true);
    let at = now(&state);
    send(&mut state, PlayMessage::SwitchEngine(id2, at));
    assert!(state.play_switching());
    resign(&mut state);
    let effects = send(
        &mut state,
        EngineMessage::Started {
            run: run2,
            identity: Box::new(identity(Vec::new())),
            at: 0,
        },
    );
    assert!(!state.play_switching());
    assert!(!state.play_live());
    let sent = sent_to(&effects, run2);
    assert!(
        sent.iter()
            .all(|c| !matches!(c, EngineCommand::NewGame | EngineCommand::SetMultiPv(_))),
        "{sent:?}"
    );
}

#[test]
fn losing_the_new_engine_while_switching_clears_the_wait() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::Depth(2)),
    );
    lose_engine_of(&mut state, run);
    let id2 = state
        .engines
        .add(EngineDraft::new("Second", "/second"))
        .expect("add");
    let run2 = state.engine_runs.begin(id2, true);
    let at = now(&state);
    send(&mut state, PlayMessage::SwitchEngine(id2, at));
    assert!(state.play_switching());
    send(&mut state, EngineMessage::Stopped(run2));
    assert!(!state.play_switching());
    assert!(view(&state).engine_lost);
}

#[test]
fn an_engine_that_chats_without_a_bestmove_is_given_up_at_the_ceiling() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::Black, TimeMode::MoveTime(2)),
    );
    let t0 = now(&state);
    event(&mut state, run, EngineEvent::SearchStarted(SearchId(5)));
    // The engine keeps talking: the silence limit never trips.
    for k in 1..=5 {
        state.flow.play.fixed_now = Some(t0 + 5 * k * SEC);
        event(
            &mut state,
            run,
            EngineEvent::Info(mychess_engine::SearchInfo {
                search: SearchId(5),
                ..mychess_engine::SearchInfo::default()
            }),
        );
        send(&mut state, PlayMessage::Tick(t0 + 5 * k * SEC));
    }
    assert!(!view(&state).engine_lost, "3 x 2 s + 30 s not reached yet");
    state.flow.play.fixed_now = Some(t0 + 37 * SEC);
    event(
        &mut state,
        run,
        EngineEvent::Info(mychess_engine::SearchInfo {
            search: SearchId(5),
            ..mychess_engine::SearchInfo::default()
        }),
    );
    send(&mut state, PlayMessage::Tick(t0 + 37 * SEC));
    assert!(view(&state).engine_lost);
}

// ---- premove against the engine (D4450-D4455) ----

fn premove(state: &mut AppState, from: &str, to: &str) {
    send(
        state,
        PlayMessage::SetPremove {
            from: sq(from),
            to: sq(to),
            promotion: None,
        },
    );
}

/// White plays e4; the engine is thinking. Returns the effects that hold
/// the search.
fn thinking_after_e4() -> (AppState, RunId, Vec<Effect>) {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let effects = play_move(&mut state, "e2", "e4");
    (state, run, effects)
}

#[test]
fn a_premove_is_played_when_the_engine_answers_and_it_is_legal() {
    let (mut state, run, effects) = thinking_after_e4();
    premove(&mut state, "g1", "f3");
    assert_eq!(
        state.play_premove().map(|p| (p.from, p.to)),
        Some((sq("g1"), sq("f3")))
    );
    let out = reply(&mut state, run, &effects, 31, "e7e5");
    assert!(
        state.play_premove().is_none(),
        "the slot is empty after use"
    );
    assert_eq!(state.session.game().mainline().count(), 3, "e4 e5 Nf3");
    assert_eq!(
        view(&state).phase,
        PlayPhase::Opponent,
        "the engine thinks again"
    );
    assert_eq!(last_search(&out).expect("search").position.moves.len(), 3);
}

#[test]
fn an_illegal_premove_is_dropped_without_a_word() {
    let (mut state, run, effects) = thinking_after_e4();
    // The pawn push is blocked by the reply e5: e4-e5 is illegal then.
    premove(&mut state, "e4", "e5");
    assert!(state.play_premove().is_some());
    reply(&mut state, run, &effects, 32, "e7e5");
    assert!(state.play_premove().is_none());
    assert_eq!(state.session.game().mainline().count(), 2);
    assert_eq!(view(&state).phase, PlayPhase::Player);
    assert_ne!(notice(&state), Some(PlayNotice::IllegalMove));
}

#[test]
fn only_one_premove_is_kept_and_it_can_be_cancelled() {
    let (mut state, _run, _effects) = thinking_after_e4();
    premove(&mut state, "g1", "f3");
    premove(&mut state, "d2", "d4");
    assert_eq!(state.play_premove().map(|p| p.to), Some(sq("d4")));
    send(&mut state, PlayMessage::CancelPremove);
    assert!(state.play_premove().is_none());
}

#[test]
fn a_premove_is_refused_on_the_players_turn_and_for_impossible_moves() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    premove(&mut state, "e2", "e4");
    assert!(state.play_premove().is_none(), "it is the player's turn");
    let effects = play_move(&mut state, "e2", "e4");
    premove(&mut state, "g1", "g3");
    assert!(state.play_premove().is_none(), "a knight cannot do that");
    premove(&mut state, "e7", "e5");
    assert!(state.play_premove().is_none(), "not a piece of the player");
    drop((run, effects));
}

#[test]
fn switching_the_premove_off_drops_it_and_refuses_new_ones() {
    let (mut state, _run, _effects) = thinking_after_e4();
    assert!(state.play_premove_enabled());
    premove(&mut state, "g1", "f3");
    send(&mut state, PlayMessage::EnablePremove(false));
    assert!(!state.play_premove_enabled());
    assert!(state.play_premove().is_none());
    assert!(state.play_premove_context().is_none());
    premove(&mut state, "g1", "f3");
    assert!(state.play_premove().is_none());
    send(&mut state, PlayMessage::EnablePremove(true));
    assert!(state.play_premove_context().is_some());
}

#[test]
fn quit_and_a_new_game_clear_the_premove() {
    let (mut state, _run, _effects) = thinking_after_e4();
    premove(&mut state, "g1", "f3");
    let at = now(&state);
    send(&mut state, PlayMessage::Quit(at));
    assert!(state.play_premove().is_none());
    assert!(state.play_premove_context().is_none(), "no game");
}

#[test]
fn a_premove_does_not_survive_the_end_of_the_game() {
    let (mut state, run, id) = setup();
    start(
        &mut state,
        config(id, ColorChoice::White, TimeMode::Depth(2)),
    );
    let effects = play_move(&mut state, "f2", "f3");
    reply(&mut state, run, &effects, 40, "e7e5");
    let effects = play_move(&mut state, "g2", "g4");
    premove(&mut state, "a2", "a3");
    reply(&mut state, run, &effects, 41, "d8h4");
    assert!(state.play_premove().is_none(), "mate: nothing to play");
    assert!(matches!(view(&state).phase, PlayPhase::Ended(_)));
}

#[test]
fn a_castling_premove_waits_for_the_squares_to_be_free() {
    let (mut state, run, id) = setup();
    let mut c = config(id, ColorChoice::White, TimeMode::Depth(2));
    let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
    c.start = StartFrom::Position(Box::new(pos));
    start(&mut state, c);
    let effects = play_move(&mut state, "a1", "a2");
    premove(&mut state, "e1", "g1");
    assert!(state.play_premove().is_some());
    reply(&mut state, run, &effects, 50, "e8d8");
    assert_eq!(state.session.game().mainline().count(), 3, "a2, Kd8, O-O");
    assert!(state.play_premove().is_none());
}

#[test]
fn a_chess960_game_needs_an_engine_with_uci_chess960() {
    // The identity of the fake has no `UCI_Chess960` (default capabilities).
    let (mut state, _, id) = setup();
    let mut config = config(id, ColorChoice::White, TimeMode::Depth(2));
    config.start = StartFrom::Position(Box::new(Position::chess960_start(7).expect("chess960")));
    start(&mut state, config);
    assert!(!state.play_live());
    assert_eq!(notice(&state), Some(PlayNotice::Chess960Unsupported));
}

#[test]
fn a_chess960_game_starts_with_an_engine_that_announces_the_option() {
    let (mut state, run, id) = setup();
    if let Some(identity) = state
        .engine_runs
        .get_mut(run)
        .and_then(|r| r.identity.as_mut())
    {
        identity.capabilities.chess960 = true;
    }
    let mut config = config(id, ColorChoice::White, TimeMode::Depth(2));
    config.start = StartFrom::Position(Box::new(Position::chess960_start(7).expect("chess960")));
    start(&mut state, config);
    assert!(state.play_live());
    assert_eq!(
        state.session.game().start_position().chess960_number(),
        Some(7)
    );
}
