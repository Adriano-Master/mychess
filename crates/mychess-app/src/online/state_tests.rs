//! Tests of the state machine of the online play (spec M8-3, D1311-D1333),
//! driven with messages only: no network, no clock.

use std::time::{Duration, Instant};

use mychess_core::Color;
use mychess_online::models::{ChallengeUpdate, Event, GameEvent};
use mychess_online::{Error, SeekOutcome, StreamMessage};

use super::message::{
    AccountView, ActionDone, ActionFailure, ActionKind, ChallengeRequest, ConnectRequest,
    MarkerInfo, OnlineColor, OnlineError, OnlineMessage, OnlineNotice, OnlineVariant, RejectReason,
    RetryKind, SeekRequest, StoreChoice, StreamKind, StreamUpdate, TokenSource,
};
use super::prefs::OnlinePrefs;
use super::session::{Phase, PlayingState};
use super::state::{Ctx, Link, OnlineState, Out};
use super::{OnlineCommand, OnlineHub};

const ID: &str = "abcd1234";

struct Harness {
    state: OnlineState,
    prefs: OnlinePrefs,
    busy: bool,
    t0: Instant,
    account_epoch: u64,
    game_epoch: u64,
    seek_epoch: u64,
    challenge_epoch: u64,
}

impl Harness {
    fn new() -> Harness {
        let mut state = OnlineState::new(OnlineHub::new());
        state.marker_dir = Some(std::path::PathBuf::from("marker-dir"));
        Harness {
            state,
            prefs: OnlinePrefs::default(),
            busy: false,
            t0: Instant::now(),
            account_epoch: 0,
            game_epoch: 0,
            seek_epoch: 0,
            challenge_epoch: 0,
        }
    }

    fn at(&self, secs: u64) -> Instant {
        self.t0 + Duration::from_secs(secs)
    }

    fn send(&mut self, message: OnlineMessage) -> Vec<Out> {
        let outs = {
            let ctx = Ctx {
                busy: self.busy,
                prefs: &self.prefs,
            };
            self.state.apply(message, &ctx)
        };
        for out in &outs {
            match out {
                Out::Command(OnlineCommand::Connect { epoch, .. }) => self.account_epoch = *epoch,
                Out::Command(OnlineCommand::OpenGame { epoch, .. }) => self.game_epoch = *epoch,
                Out::Command(OnlineCommand::Seek { epoch, .. }) => self.seek_epoch = *epoch,
                Out::Command(OnlineCommand::Challenge { epoch, .. }) => {
                    self.challenge_epoch = *epoch;
                }
                _ => {}
            }
        }
        outs
    }

    fn update(&mut self, epoch: u64, secs: u64, kind: StreamKind) -> Vec<Out> {
        let at = self.at(secs);
        self.send(OnlineMessage::Stream(StreamUpdate { epoch, at, kind }))
    }

    fn account(&mut self, secs: u64, message: StreamMessage<Event>) -> Vec<Out> {
        self.update(self.account_epoch, secs, StreamKind::Account(message))
    }

    fn event(&mut self, secs: u64, json: &str) -> Vec<Out> {
        self.account(secs, StreamMessage::Item(Event::parse_line(json)))
    }

    fn game(&mut self, secs: u64, message: StreamMessage<GameEvent>) -> Vec<Out> {
        self.update(
            self.game_epoch,
            secs,
            StreamKind::Game {
                id: ID.to_owned(),
                message,
            },
        )
    }

    fn game_line(&mut self, secs: u64, json: &str) -> Vec<Out> {
        self.game(secs, StreamMessage::Item(GameEvent::parse_line(json)))
    }

    /// Connected as `bob`, event stream up.
    fn connected() -> Harness {
        let mut h = Harness::new();
        h.send(OnlineMessage::Connect(ConnectRequest {
            source: TokenSource::Staged,
            store: StoreChoice::Session,
        }));
        h.send(OnlineMessage::Connected {
            epoch: h.account_epoch,
            result: Ok(AccountView {
                login: "Bob".to_owned(),
                id: "bob".to_owned(),
                title: None,
                ratings: vec![("rapid".to_owned(), 1500, false)],
                can_challenge: true,
            }),
        });
        h.account(0, StreamMessage::Connected);
        h
    }

    /// In a game as Black against `alice`, nothing played.
    fn playing() -> Harness {
        let mut h = Harness::connected();
        h.send(OnlineMessage::Seek(seek()));
        h.event(1, &game_start());
        h.game(1, StreamMessage::Connected);
        h.game_line(1, &full("", "started", 600_000, 600_000));
        h
    }
}

fn seek() -> SeekRequest {
    SeekRequest {
        variant: OnlineVariant::Standard,
        minutes: 10,
        increment: 0,
        rated: false,
        color: OnlineColor::Random,
        rating_range: None,
    }
}

fn game_start() -> String {
    format!(r#"{{"type":"gameStart","game":{{"gameId":"{ID}","color":"black","speed":"rapid"}}}}"#)
}

fn state_json(moves: &str, status: &str, wtime: u64, btime: u64, extra: &str) -> String {
    format!(
        r#"{{"type":"gameState","moves":"{moves}","wtime":{wtime},"btime":{btime},"winc":5000,"binc":5000,"status":"{status}"{extra}}}"#
    )
}

fn full(moves: &str, status: &str, wtime: u64, btime: u64) -> String {
    format!(
        r#"{{"type":"gameFull","id":"{ID}","variant":{{"key":"standard","name":"Standard"}},"speed":"rapid","perf":{{"name":"Rapid"}},"rated":false,"createdAt":1700000000000,"white":{{"id":"alice","name":"alice","rating":1800}},"black":{{"id":"bob","name":"Bob","rating":1500}},"initialFen":"startpos","clock":{{"initial":600000,"increment":5000}},"state":{}}}"#,
        state_json(moves, status, wtime, btime, "")
    )
}

fn sq(text: &str) -> mychess_core::Square {
    text.parse().expect("square")
}

fn play(h: &mut Harness, from: &str, to: &str, secs: u64) -> Vec<Out> {
    let at = h.at(secs);
    h.send(OnlineMessage::PlayerMove {
        from: sq(from),
        to: sq(to),
        promotion: None,
        offer_draw: false,
        at,
    })
}

fn commands(outs: &[Out]) -> Vec<&OnlineCommand> {
    outs.iter()
        .filter_map(|o| match o {
            Out::Command(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn rejected(outs: &[Out], reason: RejectReason) -> bool {
    outs.contains(&Out::Notice(OnlineNotice::Rejected(reason)))
}

fn moves_of(h: &Harness) -> usize {
    h.state
        .game
        .as_ref()
        .map_or(0, super::game_builder::OnlineGame::plies)
}

// ---- connection and states (D1311) ----

#[test]
fn connecting_goes_through_connecting_to_idle_and_ignores_a_stale_answer() {
    let mut h = Harness::new();
    let outs = h.send(OnlineMessage::Connect(ConnectRequest {
        source: TokenSource::Staged,
        store: StoreChoice::Keyring,
    }));
    assert_eq!(h.state.phase, Phase::Connecting);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Connect { base_url, .. }] if base_url == "https://lichess.org"
    ));
    // A second request while connecting is refused, not queued.
    let outs = h.send(OnlineMessage::Connect(ConnectRequest {
        source: TokenSource::Staged,
        store: StoreChoice::Keyring,
    }));
    assert!(rejected(&outs, RejectReason::WrongState));
    // The answer of another attempt is dropped.
    h.send(OnlineMessage::Connected {
        epoch: h.account_epoch + 99,
        result: Err(OnlineError::Network),
    });
    assert_eq!(h.state.phase, Phase::Connecting);
    let view = AccountView {
        login: "Bob".to_owned(),
        id: "bob".to_owned(),
        title: None,
        ratings: Vec::new(),
        can_challenge: true,
    };
    let outs = h.send(OnlineMessage::Connected {
        epoch: h.account_epoch,
        result: Ok(view),
    });
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(outs.contains(&Out::Login("Bob".to_owned())));
}

#[test]
fn a_failed_connection_goes_back_to_disconnected_with_the_reason() {
    let mut h = Harness::new();
    h.send(OnlineMessage::Connect(ConnectRequest {
        source: TokenSource::Staged,
        store: StoreChoice::Keyring,
    }));
    let outs = h.send(OnlineMessage::Connected {
        epoch: h.account_epoch,
        result: Err(OnlineError::Unauthorized),
    });
    assert_eq!(h.state.phase, Phase::Disconnected);
    assert!(outs.contains(&Out::Notice(OnlineNotice::Error(OnlineError::Unauthorized))));
}

#[test]
fn an_insecure_server_address_is_never_used() {
    let mut h = Harness::new();
    h.prefs.base_url = "http://lichess.org".to_owned();
    let outs = h.send(OnlineMessage::Connect(ConnectRequest {
        source: TokenSource::Staged,
        store: StoreChoice::Keyring,
    }));
    assert_eq!(h.state.phase, Phase::Disconnected);
    assert!(commands(&outs).is_empty());
}

#[test]
fn a_seek_needs_the_event_stream_a_free_program_and_a_slow_enough_clock() {
    let mut h = Harness::connected();
    h.busy = true;
    assert!(rejected(
        &h.send(OnlineMessage::Seek(seek())),
        RejectReason::Busy
    ));
    h.busy = false;
    let bullet = SeekRequest {
        variant: OnlineVariant::Standard,
        minutes: 1,
        ..seek()
    };
    assert!(rejected(
        &h.send(OnlineMessage::Seek(bullet)),
        RejectReason::TimeControl
    ));
    h.account(
        2,
        StreamMessage::Reconnecting {
            attempt: 1,
            delay: Duration::from_secs(1),
            cause: Error::Timeout,
        },
    );
    assert_eq!(h.state.link, Link::Reconnecting);
    assert!(rejected(
        &h.send(OnlineMessage::Seek(seek())),
        RejectReason::EventsNotReady
    ));
    h.account(3, StreamMessage::Connected);
    let outs = h.send(OnlineMessage::Seek(seek()));
    assert_eq!(h.state.phase, Phase::Seeking);
    assert!(outs.contains(&Out::Notice(OnlineNotice::AssistanceWillBeDisabled)));
    assert!(matches!(commands(&outs)[..], [OnlineCommand::Seek { .. }]));
    // Nothing is locked until a game starts.
    assert!(!h.state.assist_locked());
}

#[test]
fn cancelling_a_seek_closes_it_and_drops_what_it_says_later() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::Seek(seek()));
    let old = h.seek_epoch;
    let outs = h.send(OnlineMessage::CancelSeek);
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(matches!(commands(&outs)[..], [OnlineCommand::CancelSeek]));
    h.update(
        old,
        3,
        StreamKind::Seek(SeekOutcome::Failed(Error::Timeout)),
    );
    assert_eq!(h.state.notice, Some(OnlineNotice::AssistanceWillBeDisabled));
}

#[test]
fn a_seek_that_fails_returns_to_idle() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::Seek(seek()));
    h.update(
        h.seek_epoch,
        2,
        StreamKind::Seek(SeekOutcome::Failed(Error::Unauthorized)),
    );
    assert_eq!(h.state.phase, Phase::Idle);
    assert_eq!(
        h.state.notice,
        Some(OnlineNotice::Error(OnlineError::Unauthorized))
    );
}

// ---- the game (D1317-D1319, D1330) ----

#[test]
fn the_game_start_locks_the_assistance_and_stops_everything() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::Seek(seek()));
    let outs = h.event(1, &game_start());
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Live));
    assert!(h.state.assist_locked());
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Starting { resumed: false, .. }))
    );
    assert!(matches!(
        commands(&outs)[..],
        [
            OnlineCommand::OpenGame { .. },
            OnlineCommand::WriteMarker { .. }
        ]
    ));
}

#[test]
fn a_game_that_was_not_asked_for_is_a_resumed_one() {
    let mut h = Harness::connected();
    let outs = h.event(1, &game_start());
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Starting { resumed: true, .. }))
    );
    assert!(outs.contains(&Out::Notice(OnlineNotice::Resumed)));
}

#[test]
fn a_second_game_is_ignored_with_a_notice() {
    let mut h = Harness::playing();
    let other = r#"{"type":"gameStart","game":{"gameId":"zzzz9999"}}"#;
    let outs = h.event(5, other);
    assert!(outs.contains(&Out::Notice(OnlineNotice::AnotherGame)));
    assert_eq!(h.state.my_color(), Some(Color::Black));
}

#[test]
fn the_game_full_opens_the_game_and_the_clock_waits_for_two_plies() {
    let h = Harness::playing();
    let game = h.state.game.as_ref().expect("game");
    assert_eq!(game.my_color, Color::Black);
    assert!(!h.state.clock.is_running(), "no ply played yet");
    assert_eq!(
        h.state.shown(Color::White, h.at(100)),
        Duration::from_secs(600)
    );
}

#[test]
fn the_clock_follows_the_server_runs_for_the_side_to_move_and_adds_no_increment() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4 e7e5", "started", 598_000, 600_000, ""));
    assert!(h.state.clock.is_running());
    // White to move: only its clock runs, from the instant of the message.
    assert_eq!(
        h.state.shown(Color::White, h.at(7)),
        Duration::from_secs(593)
    );
    assert_eq!(
        h.state.shown(Color::Black, h.at(7)),
        Duration::from_secs(600)
    );
}

#[test]
fn a_flag_that_falls_locally_does_not_end_the_game() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4 e7e5", "started", 1000, 600_000, ""));
    assert_eq!(h.state.clock.flagged(h.at(60)), Some(Color::White));
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Live));
    assert!(h.state.finished.is_none());
}

#[test]
fn the_same_state_twice_changes_nothing_and_a_shorter_one_takes_back() {
    let mut h = Harness::playing();
    let line = state_json("e2e4 e7e5 g1f3", "started", 590_000, 590_000, "");
    h.game_line(2, &line);
    h.game_line(2, &line);
    assert_eq!(moves_of(&h), 3);
    h.game_line(3, &state_json("e2e4 e7e5", "started", 590_000, 590_000, ""));
    assert_eq!(moves_of(&h), 2);
}

#[test]
fn castling_arrives_in_both_notations() {
    for castle in ["e1g1", "e1h1"] {
        let mut h = Harness::playing();
        let moves = format!("e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 {castle}");
        h.game_line(2, &state_json(&moves, "started", 500_000, 500_000, ""));
        assert_eq!(moves_of(&h), 7, "{castle}");
    }
}

#[test]
fn an_illegal_move_of_the_server_reopens_the_stream_once_then_stops_following() {
    let mut h = Harness::playing();
    let before = h.game_epoch;
    let outs = h.game_line(2, &state_json("e2e5", "started", 1, 1, ""));
    assert!(outs.contains(&Out::Notice(OnlineNotice::Resynchronised)));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::OpenGame { .. }]
    ));
    assert_ne!(h.game_epoch, before);
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Reconnecting));
    // The old stream is dropped from now on.
    let late = h.update(
        before,
        3,
        StreamKind::Game {
            id: ID.to_owned(),
            message: StreamMessage::Item(GameEvent::parse_line(&state_json(
                "e2e4", "started", 1, 1, "",
            ))),
        },
    );
    assert!(late.is_empty());
    h.game_line(4, &state_json("e2e5", "started", 1, 1, ""));
    h.game_line(5, &state_json("e2e5", "started", 1, 1, ""));
    let outs = h.game_line(6, &state_json("e2e5", "started", 1, 1, ""));
    assert!(outs.contains(&Out::Notice(OnlineNotice::Desynced)));
    // The lock stays: the server is still the truth.
    assert!(h.state.assist_locked());
}

#[test]
fn a_game_of_another_variant_is_not_opened() {
    let mut h = Harness::connected();
    h.event(1, &game_start());
    let json = full("", "started", 1, 1).replace(
        r#""variant":{"key":"standard","name":"Standard"}"#,
        r#""variant":{"key":"atomic","name":"Atomic"}"#,
    );
    let outs = h.game_line(1, &json);
    assert!(outs.contains(&Out::Notice(OnlineNotice::UnsupportedVariant)));
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(h.state.game.is_none());
}

// ---- moves and premoves (D1320, D1321) ----

#[test]
fn the_move_of_the_user_is_shown_at_once_and_sent() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    let outs = play(&mut h, "e7", "e5", 3);
    assert_eq!(moves_of(&h), 2);
    assert!(outs.contains(&Out::Changed));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Move { uci, ply: 2, offer_draw: false, .. }] if uci == "e7e5"
    ));
    // The next move made before the server answers can only be a premove.
    assert!(commands(&play(&mut h, "g8", "f6", 3)).is_empty());
    assert!(h.state.premove.is_set());
}

#[test]
fn the_confirmation_measures_the_latency() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    play(&mut h, "e7", "e5", 3);
    h.game_line(4, &state_json("e2e4 e7e5", "started", 598_000, 595_000, ""));
    assert_eq!(h.state.ping, Some(Duration::from_secs(1)));
    assert_eq!(moves_of(&h), 2);
}

#[test]
fn a_move_the_server_refuses_is_taken_back() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    play(&mut h, "e7", "e5", 3);
    let outs = h.send(OnlineMessage::Done(ActionDone {
        action: ActionKind::Move,
        id: Some(ID.to_owned()),
        ply: Some(2),
        result: Err(ActionFailure {
            error: OnlineError::Rejected {
                reason: "Not your turn".to_owned(),
            },
            ambiguous: false,
        }),
        at: h.at(4),
    }));
    assert_eq!(moves_of(&h), 1);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Notice(OnlineNotice::MoveRejected { .. })))
    );
}

#[test]
fn a_retried_move_that_is_refused_stays_until_the_server_says() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    play(&mut h, "e7", "e5", 3);
    let outs = h.send(OnlineMessage::Done(ActionDone {
        action: ActionKind::Move,
        id: Some(ID.to_owned()),
        ply: Some(2),
        result: Err(ActionFailure {
            error: OnlineError::Rejected {
                reason: "x".to_owned(),
            },
            ambiguous: true,
        }),
        at: h.at(4),
    }));
    assert_eq!(moves_of(&h), 2, "D1347: no undo");
    assert!(outs.contains(&Out::Notice(OnlineNotice::MoveUncertain)));
}

#[test]
fn an_unrelated_state_does_not_drop_a_move_waiting_for_the_server() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    play(&mut h, "e7", "e5", 3);
    // A state that predates the move (a draw offer, say).
    h.game_line(
        3,
        &state_json("e2e4", "started", 597_000, 600_000, r#","wdraw":true"#),
    );
    assert_eq!(moves_of(&h), 2);
}

#[test]
fn a_premove_is_kept_then_sent_when_it_becomes_legal() {
    let mut h = Harness::playing();
    // Alice to move: the move of the user is a premove.
    let outs = play(&mut h, "e7", "e5", 1);
    assert_eq!(moves_of(&h), 0);
    assert!(commands(&outs).is_empty());
    assert!(h.state.premove.is_set());
    let outs = h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Move { uci, .. }] if uci == "e7e5"
    ));
    assert!(!h.state.premove.is_set());
    assert_eq!(moves_of(&h), 2);
}

#[test]
fn an_illegal_premove_is_dropped_without_a_word() {
    let mut h = Harness::playing();
    // Queen along the diagonal: the right shape, blocked by the pawn later.
    play(&mut h, "d8", "h4", 1);
    assert!(h.state.premove.is_set());
    let outs = h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    assert!(commands(&outs).is_empty());
    assert!(!h.state.premove.is_set());
    assert_eq!(moves_of(&h), 1);
}

#[test]
fn a_move_with_the_wrong_shape_is_not_a_premove_and_the_switch_turns_it_off() {
    let mut h = Harness::playing();
    assert!(rejected(
        &play(&mut h, "e7", "e3", 1),
        RejectReason::NotYourTurn
    ));
    h.prefs.premove = false;
    assert!(rejected(
        &play(&mut h, "e7", "e5", 1),
        RejectReason::NotYourTurn
    ));
    assert!(!h.state.premove.is_set());
}

#[test]
fn cancelling_or_a_takeback_drops_the_premove() {
    let mut h = Harness::playing();
    play(&mut h, "e7", "e5", 1);
    h.send(OnlineMessage::CancelPremove);
    assert!(!h.state.premove.is_set());
    h.game_line(2, &state_json("e2e4 e7e5", "started", 1, 1, ""));
    play(&mut h, "b8", "c6", 3);
    assert!(h.state.premove.is_set());
    h.game_line(4, &state_json("e2e4", "started", 1, 1, ""));
    assert!(!h.state.premove.is_set());
}

// ---- draws, resigning, aborting, takebacks, absence ----

#[test]
fn abort_before_two_plies_and_resign_after() {
    let mut h = Harness::playing();
    assert!(rejected(
        &h.send(OnlineMessage::Resign),
        RejectReason::NotAvailable
    ));
    assert!(h.state.can_abort());
    let outs = h.send(OnlineMessage::Abort);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Game {
            action: ActionKind::Abort,
            ..
        }]
    ));
    h.game_line(2, &state_json("e2e4 e7e5", "started", 1, 1, ""));
    assert!(!h.state.can_abort());
    assert!(rejected(
        &h.send(OnlineMessage::Abort),
        RejectReason::NotAvailable
    ));
    let outs = h.send(OnlineMessage::Resign);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Game {
            action: ActionKind::Resign,
            ..
        }]
    ));
}

#[test]
fn draw_offers_are_offered_accepted_declined_and_not_repeated() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4 e7e5", "started", 1, 1, ""));
    let outs = h.send(OnlineMessage::OfferDraw);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Game {
            action: ActionKind::DrawYes,
            ..
        }]
    ));
    // Answering needs an offer of the opponent.
    assert!(rejected(
        &h.send(OnlineMessage::AnswerDraw(true)),
        RejectReason::NotAvailable
    ));
    // The user is Black: his own offer shows in `bdraw`.
    h.game_line(
        3,
        &state_json("e2e4 e7e5", "started", 1, 1, r#","bdraw":true"#),
    );
    assert!(rejected(
        &h.send(OnlineMessage::OfferDraw),
        RejectReason::NotAvailable
    ));
    h.game_line(
        4,
        &state_json("e2e4 e7e5", "started", 1, 1, r#","wdraw":true"#),
    );
    assert!(h.state.draw_offered_to_me());
    for (yes, kind) in [(true, ActionKind::DrawYes), (false, ActionKind::DrawNo)] {
        let outs = h.send(OnlineMessage::AnswerDraw(yes));
        assert!(
            matches!(commands(&outs)[..], [OnlineCommand::Game { action, .. }] if *action == kind)
        );
    }
}

#[test]
fn a_takeback_proposal_can_be_accepted_or_declined_and_truncates_the_game() {
    let mut h = Harness::playing();
    h.game_line(
        2,
        &state_json("e2e4 e7e5", "started", 1, 1, r#","wtakeback":true"#),
    );
    assert!(h.state.takeback_proposed_to_me());
    let outs = h.send(OnlineMessage::AnswerTakeback(true));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Game {
            action: ActionKind::TakebackYes,
            ..
        }]
    ));
    h.game_line(3, &state_json("e2e4", "started", 1, 1, ""));
    assert_eq!(moves_of(&h), 1);
    assert!(!h.state.takeback_proposed_to_me());
}

#[test]
fn the_absent_opponent_counts_down_to_the_claim() {
    let mut h = Harness::playing();
    assert!(rejected(
        &h.send(OnlineMessage::ClaimVictory { at: h.at(1) }),
        RejectReason::NotAvailable
    ));
    h.game(
        10,
        StreamMessage::Item(GameEvent::OpponentGone {
            gone: true,
            claim_win_in_seconds: Some(20),
        }),
    );
    assert!(!h.state.can_claim_victory(h.at(15)));
    assert!(h.state.can_claim_victory(h.at(30)));
    // Too early: the wait announced by the server is not over (D3116).
    for early in [
        OnlineMessage::ClaimVictory { at: h.at(15) },
        OnlineMessage::ClaimDraw { at: h.at(15) },
    ] {
        assert!(rejected(&h.send(early), RejectReason::NotAvailable));
    }
    let outs = h.send(OnlineMessage::ClaimVictory { at: h.at(30) });
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Game {
            action: ActionKind::ClaimVictory,
            ..
        }]
    ));
    h.game(
        31,
        StreamMessage::Item(GameEvent::OpponentGone {
            gone: false,
            claim_win_in_seconds: None,
        }),
    );
    assert!(!h.state.can_claim_victory(h.at(99)));
}

// ---- the end (D1327) ----

#[test]
fn the_final_status_stops_the_clock_releases_the_lock_and_asks_for_the_import() {
    let mut h = Harness::playing();
    let outs = h.game_line(
        5,
        &state_json("e2e4 e7e5", "resign", 1000, 2000, r#","winner":"white""#),
    );
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Finished));
    assert!(!h.state.assist_locked());
    assert!(outs.contains(&Out::Finished));
    assert!(outs.contains(&Out::Notice(OnlineNotice::GameOver)));
    assert!(commands(&outs).contains(&&OnlineCommand::CloseGame));
    assert!(
        commands(&outs)
            .iter()
            .any(|c| matches!(c, OnlineCommand::RemoveMarker { .. }))
    );
    assert!(!h.state.clock.is_running());
    let outcome = h.state.finished.expect("outcome");
    assert_eq!(outcome.result, mychess_core::GameResult::White);
    // A late message of the same game changes nothing.
    assert!(
        h.game_line(6, &state_json("e2e4 e7e5", "resign", 1, 1, ""))
            .iter()
            .all(|o| *o != Out::Finished)
    );
}

#[test]
fn a_game_that_arrives_over_is_finished_at_once() {
    let mut h = Harness::connected();
    h.event(1, &game_start());
    let json = full("e2e4 e7e5", "mate", 1, 1)
        .replace(r#""status":"mate""#, r#""status":"mate","winner":"black""#);
    let outs = h.game_line(1, &json);
    assert!(outs.contains(&Out::Opened));
    assert!(outs.contains(&Out::Finished));
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Finished));
}

#[test]
fn an_aborted_game_is_marked_aborted_and_a_new_search_may_follow() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("", "aborted", 1, 1, ""));
    let outcome = h.state.finished.expect("outcome");
    assert_eq!(outcome.kind, super::EndKind::Aborted);
    let outs = h.send(OnlineMessage::Seek(seek()));
    assert_eq!(h.state.phase, Phase::Seeking);
    assert!(!commands(&outs).is_empty());
}

#[test]
fn dismissing_the_result_goes_back_to_idle() {
    let mut h = Harness::playing();
    assert!(rejected(
        &h.send(OnlineMessage::DismissResult),
        RejectReason::WrongState
    ));
    h.game_line(2, &state_json("e2e4", "draw", 1, 1, ""));
    let outs = h.send(OnlineMessage::DismissResult);
    assert!(outs.contains(&Out::Dismissed));
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(h.state.game.is_none());
}

// ---- reconnection (D1333) ----

#[test]
fn the_game_survives_a_lost_stream_and_resends_a_move_the_server_never_saw() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    play(&mut h, "e7", "e5", 3);
    h.game(
        4,
        StreamMessage::Reconnecting {
            attempt: 1,
            delay: Duration::from_secs(1),
            cause: Error::Timeout,
        },
    );
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Reconnecting));
    assert!(h.state.assist_locked(), "the lock holds while reconnecting");
    h.game(5, StreamMessage::Connected);
    // The new `gameFull` shows the move missing and it is still his turn.
    let outs = h.game_line(5, &full("e2e4", "started", 590_000, 600_000));
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Live));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Move { uci, .. }] if uci == "e7e5"
    ));
    assert_eq!(moves_of(&h), 2, "no duplicated move");
}

#[test]
fn a_move_the_server_already_has_is_not_sent_again_after_reconnecting() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 598_000, 600_000, ""));
    play(&mut h, "e7", "e5", 3);
    let outs = h.game_line(5, &full("e2e4 e7e5", "started", 590_000, 595_000));
    assert!(commands(&outs).is_empty());
    assert_eq!(moves_of(&h), 2);
}

#[test]
fn the_event_stream_dying_between_games_disconnects_but_not_during_one() {
    let mut h = Harness::playing();
    h.account(9, StreamMessage::Failed(Error::Timeout));
    assert_eq!(h.state.link, Link::Down);
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Live));
    let mut h = Harness::connected();
    h.account(9, StreamMessage::Failed(Error::Timeout));
    assert_eq!(h.state.phase, Phase::Disconnected);
    assert!(h.state.account.is_none());
}

#[test]
fn signing_out_is_refused_during_a_game() {
    let mut h = Harness::playing();
    assert!(rejected(
        &h.send(OnlineMessage::Disconnect),
        RejectReason::GameInProgress
    ));
    let mut h = Harness::connected();
    let outs = h.send(OnlineMessage::Disconnect);
    assert_eq!(h.state.phase, Phase::Disconnected);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Disconnect { login }] if login == "Bob"
    ));
}

// ---- challenges (D1316) ----

fn challenge_event(id: &str, extra: &str) -> String {
    format!(
        r#"{{"type":"challenge","challenge":{{"id":"{id}","status":"created","challenger":{{"id":"alice","name":"alice","rating":1800{extra}}},"variant":{{"key":"standard","name":"Standard"}},"rated":false,"speed":"rapid","timeControl":{{"type":"clock","limit":600,"increment":5,"show":"10+5"}},"direction":"in"}}}}"#
    )
}

#[test]
fn a_challenge_received_can_be_accepted_or_declined() {
    let mut h = Harness::connected();
    h.event(1, &challenge_event("chal0001", ""));
    h.event(1, &challenge_event("chal0002", ""));
    assert_eq!(h.state.incoming.len(), 2);
    assert_eq!(h.state.incoming[0].time, "10+5");
    let outs = h.send(OnlineMessage::DeclineChallenge("chal0002".to_owned()));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::DeclineChallenge { reason, .. }] if reason == "generic"
    ));
    let outs = h.send(OnlineMessage::AcceptChallenge("chal0001".to_owned()));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::AcceptChallenge { .. }]
    ));
    assert!(h.state.incoming.is_empty());
    // The game that follows was asked for.
    let outs = h.event(2, &game_start());
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Starting { resumed: false, .. }))
    );
}

#[test]
fn a_challenge_the_app_cannot_play_only_allows_declining_and_a_bot_is_declined_alone() {
    let mut h = Harness::connected();
    let bullet = challenge_event("chal0003", "").replace(
        r#""limit":600,"increment":5"#,
        r#""limit":60,"increment":0"#,
    );
    h.event(1, &bullet);
    assert!(!h.state.incoming[0].playable);
    assert!(rejected(
        &h.send(OnlineMessage::AcceptChallenge("chal0003".to_owned())),
        RejectReason::ChallengeNotPlayable
    ));
    let outs = h.event(2, &challenge_event("chal0004", r#","title":"BOT""#));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::DeclineChallenge { reason, .. }] if reason == "noBot"
    ));
    assert_eq!(h.state.incoming.len(), 1);
}

#[test]
fn a_canceled_challenge_leaves_the_list() {
    let mut h = Harness::connected();
    h.event(1, &challenge_event("chal0001", ""));
    let canceled =
        challenge_event("chal0001", "").replace("\"challenge\",", "\"challengeCanceled\",");
    h.event(2, &canceled);
    assert!(h.state.incoming.is_empty());
}

fn request() -> ChallengeRequest {
    ChallengeRequest {
        variant: OnlineVariant::Standard,
        username: "alice".to_owned(),
        limit_secs: 600,
        increment: 5,
        rated: false,
        color: OnlineColor::White,
    }
}

#[test]
fn a_challenge_sent_waits_and_is_answered_by_the_stream_or_the_event() {
    let mut h = Harness::connected();
    let outs = h.send(OnlineMessage::Challenge(request()));
    assert_eq!(h.state.phase, Phase::Challenging);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::Challenge { .. }]
    ));
    // Declined by the event stream, with the reason.
    let declined = challenge_event("chal0009", "")
        .replace("\"challenge\",", "\"challengeDeclined\",")
        .replace(
            r#""direction":"in""#,
            r#""direction":"out","declineReasonKey":"later""#,
        );
    let outs = h.event(3, &declined);
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Notice(OnlineNotice::ChallengeDeclined { .. })))
    );
}

#[test]
fn cancelling_a_challenge_sent_names_it_once_the_server_did() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::Challenge(request()));
    let line =
        challenge_event("chal0009", "").replace(r#""direction":"in""#, r#""direction":"out""#);
    h.update(
        h.challenge_epoch,
        2,
        StreamKind::Challenge(StreamMessage::Item(ChallengeUpdate::parse_line(
            // The stream sends the bare challenge.
            &line[line.find("\"challenge\":").map_or(0, |i| i + 12)..line.len() - 1],
        ))),
    );
    assert_eq!(h.state.outgoing.as_deref(), Some("chal0009"));
    let outs = h.send(OnlineMessage::CancelChallenge);
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::CancelChallenge { id: Some(id) }] if id == "chal0009"
    ));
}

#[test]
fn a_challenge_needs_a_valid_name_a_valid_clock_and_the_scope() {
    let mut h = Harness::connected();
    let bad_name = ChallengeRequest {
        variant: OnlineVariant::Standard,
        username: "../x".to_owned(),
        ..request()
    };
    assert!(rejected(
        &h.send(OnlineMessage::Challenge(bad_name)),
        RejectReason::BadUser
    ));
    let bullet = ChallengeRequest {
        variant: OnlineVariant::Standard,
        limit_secs: 60,
        increment: 0,
        ..request()
    };
    assert!(rejected(
        &h.send(OnlineMessage::Challenge(bullet)),
        RejectReason::TimeControl
    ));
}

// ---- the marker (D1332) ----

fn marker() -> MarkerInfo {
    MarkerInfo {
        game_id: ID.to_owned(),
        date: "2026.10.24".to_owned(),
    }
}

#[test]
fn a_marker_locks_the_assistance_until_the_grace_period_shows_no_game() {
    let mut h = Harness::new();
    h.send(OnlineMessage::MarkerRead(Some(marker())));
    assert!(h.state.assist_locked());
    // Connected: the event stream is asked for the game and a timer starts.
    let mut h = Harness::connected();
    h.send(OnlineMessage::MarkerRead(Some(marker())));
    assert!(h.state.assist_locked());
    let outs = h.account(5, StreamMessage::Connected);
    let generation = commands(&outs)
        .into_iter()
        .find_map(|c| match c {
            OnlineCommand::Grace { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("timer");
    // An old timer does nothing; the right one clears the marker.
    h.send(OnlineMessage::MarkerGrace(generation + 1));
    assert!(h.state.assist_locked());
    let outs = h.send(OnlineMessage::MarkerGrace(generation));
    assert!(!h.state.assist_locked());
    assert!(outs.contains(&Out::Notice(OnlineNotice::MarkerCleared)));
}

#[test]
fn a_resumed_game_keeps_the_lock_beyond_the_grace_period() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::MarkerRead(Some(marker())));
    let outs = h.account(5, StreamMessage::Connected);
    let generation = commands(&outs)
        .into_iter()
        .find_map(|c| match c {
            OnlineCommand::Grace { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("timer");
    h.event(6, &game_start());
    h.send(OnlineMessage::MarkerGrace(generation));
    assert!(h.state.assist_locked());
}

#[test]
fn the_end_of_the_game_of_the_marker_releases_the_lock() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::MarkerRead(Some(marker())));
    let finish = format!(r#"{{"type":"gameFinish","game":{{"id":"{ID}"}}}}"#);
    let outs = h.event(3, &finish);
    assert!(!h.state.assist_locked());
    assert!(
        commands(&outs)
            .iter()
            .any(|c| matches!(c, OnlineCommand::RemoveMarker { .. }))
    );
}

#[test]
fn the_lock_is_released_by_hand_only_after_confirmation() {
    let mut h = Harness::new();
    h.send(OnlineMessage::MarkerRead(Some(marker())));
    h.send(OnlineMessage::RemoveLock { confirmed: false });
    assert!(h.state.assist_locked());
    let outs = h.send(OnlineMessage::RemoveLock { confirmed: true });
    assert!(!h.state.assist_locked());
    assert!(outs.contains(&Out::Notice(OnlineNotice::LockReleased)));
    // A live game released by hand without network stays released until
    // the next game.
    let mut h = Harness::playing();
    h.account(5, StreamMessage::Failed(Error::Timeout));
    assert_eq!(h.state.link, Link::Down);
    assert!(h.state.can_remove_lock());
    h.send(OnlineMessage::RemoveLock { confirmed: true });
    assert!(!h.state.assist_locked());
}

#[test]
fn the_lock_cannot_be_released_while_the_server_is_reachable() {
    // A live game with the event stream up: refused (D1373).
    let mut h = Harness::playing();
    assert_eq!(h.state.link, Link::Up);
    assert!(!h.state.can_remove_lock());
    let outs = h.send(OnlineMessage::RemoveLock { confirmed: true });
    assert!(h.state.assist_locked());
    assert!(!h.state.released);
    assert!(rejected(&outs, RejectReason::NotAvailable));
    // The same for a marker while the stream is up.
    let mut h = Harness::connected();
    h.send(OnlineMessage::MarkerRead(Some(marker())));
    assert!(!h.state.can_remove_lock());
    h.send(OnlineMessage::RemoveLock { confirmed: true });
    assert!(h.state.marker.is_some() && h.state.assist_locked());
    // Reconnecting is not "down": the grace period or the server decides.
    let mut h = Harness::playing();
    h.account(
        5,
        StreamMessage::Reconnecting {
            attempt: 1,
            delay: Duration::from_secs(1),
            cause: Error::Timeout,
        },
    );
    assert_eq!(h.state.link, Link::Reconnecting);
    assert!(h.state.can_remove_lock());
}

#[test]
fn loading_the_marker_asks_for_it_only_when_there_is_a_directory() {
    let mut h = Harness::new();
    let outs = h.send(OnlineMessage::LoadMarker);
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::ReadMarker { .. }]
    ));
    h.state.marker_dir = None;
    assert!(commands(&h.send(OnlineMessage::LoadMarker)).is_empty());
}

#[test]
fn restoring_the_previous_game_waits_for_the_end_of_the_game() {
    let mut h = Harness::playing();
    assert!(rejected(
        &h.send(OnlineMessage::RestorePrevious),
        RejectReason::GameInProgress
    ));
    h.game_line(2, &state_json("e2e4", "draw", 1, 1, ""));
    assert!(
        h.send(OnlineMessage::RestorePrevious)
            .contains(&Out::RestorePrevious)
    );
}

// ---- review fixes (D1363-D1373) ----

fn retry_generation(outs: &[Out], wanted: RetryKind) -> Option<u64> {
    commands(outs).iter().find_map(|c| match c {
        OnlineCommand::Retry {
            kind, generation, ..
        } if *kind == wanted => Some(*generation),
        _ => None,
    })
}

#[test]
fn an_ambiguous_move_failure_asks_the_server_for_the_whole_game() {
    let mut h = Harness::playing();
    h.game_line(2, &state_json("e2e4", "started", 600_000, 600_000, ""));
    let outs = play(&mut h, "e7", "e5", 3);
    assert!(
        commands(&outs)
            .iter()
            .any(|c| matches!(c, OnlineCommand::Move { .. }))
    );
    let outs = h.send(OnlineMessage::Done(ActionDone {
        action: ActionKind::Move,
        id: Some(ID.to_owned()),
        ply: Some(2),
        result: Err(ActionFailure {
            error: OnlineError::Network,
            ambiguous: true,
        }),
        at: h.at(4),
    }));
    assert!(outs.contains(&Out::Notice(OnlineNotice::MoveUncertain)));
    assert!(
        commands(&outs)
            .iter()
            .any(|c| matches!(c, OnlineCommand::OpenGame { .. })),
        "a fresh gameFull is requested"
    );
}

#[test]
fn a_failed_game_stream_is_opened_again_and_then_given_up() {
    let mut h = Harness::playing();
    let mut outs = h.game(5, StreamMessage::Failed(Error::Timeout));
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Reconnecting));
    let mut opened = 0;
    for _ in 0..20 {
        let Some(generation) = retry_generation(&outs, RetryKind::Game) else {
            break;
        };
        // A timer that is not the last one does nothing.
        let stale = h.send(OnlineMessage::Retry {
            kind: RetryKind::Game,
            generation: generation.wrapping_sub(1),
        });
        assert!(commands(&stale).is_empty());
        outs = h.send(OnlineMessage::Retry {
            kind: RetryKind::Game,
            generation,
        });
        if commands(&outs)
            .iter()
            .any(|c| matches!(c, OnlineCommand::OpenGame { .. }))
        {
            opened += 1;
        }
        // The new stream fails again.
        let more = h.game(6, StreamMessage::Failed(Error::Timeout));
        if retry_generation(&more, RetryKind::Game).is_none() {
            outs = more;
            break;
        }
        if h.state.phase == Phase::Idle {
            break;
        }
    }
    assert!(opened >= 1, "the stream was opened again");
    // Out of attempts the game is closed here, the lock is gone.
    for _ in 0..20 {
        if h.state.phase == Phase::Idle {
            break;
        }
        let Some(generation) = retry_generation(&outs, RetryKind::Game) else {
            outs = h.game(7, StreamMessage::Failed(Error::Timeout));
            continue;
        };
        outs = h.send(OnlineMessage::Retry {
            kind: RetryKind::Game,
            generation,
        });
    }
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(!h.state.assist_locked());
    assert!(h.state.game.is_none());
    assert!(
        h.state.notice == Some(OnlineNotice::GameLost)
            || outs.contains(&Out::Notice(OnlineNotice::GameLost))
    );
}

#[test]
fn a_game_finish_for_the_current_game_asks_for_one_last_game_full() {
    let mut h = Harness::playing();
    let finish = format!(r#"{{"type":"gameFinish","game":{{"id":"{ID}"}}}}"#);
    let outs = h.event(3, &finish);
    assert!(
        commands(&outs)
            .iter()
            .any(|c| matches!(c, OnlineCommand::OpenGame { .. }))
    );
    // Only once.
    let outs = h.event(4, &finish);
    assert!(commands(&outs).is_empty());
    // The answer closes the game.
    h.game_line(5, &full("e2e4", "resign", 1, 1));
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Finished));
}

#[test]
fn leaving_a_game_with_a_dead_event_stream_disconnects_without_forgetting_the_token() {
    let mut h = Harness::playing();
    h.account(5, StreamMessage::Failed(Error::Timeout));
    assert_eq!(h.state.link, Link::Down);
    assert!(h.state.phase.is_live());
    h.game_line(6, &state_json("e2e4", "resign", 1, 1, ""));
    assert_eq!(h.state.phase, Phase::Playing(PlayingState::Finished));
    let outs = h.send(OnlineMessage::DismissResult);
    assert_eq!(h.state.phase, Phase::Disconnected);
    let commands = commands(&outs);
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, OnlineCommand::Shutdown))
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, OnlineCommand::Disconnect { .. })),
        "the token stays in the stores"
    );
}

#[test]
fn a_seek_that_closed_without_a_game_start_goes_back_to_idle() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::Seek(seek()));
    let outs = h.update(h.seek_epoch, 2, StreamKind::Seek(SeekOutcome::Closed));
    assert_eq!(h.state.phase, Phase::Seeking);
    let generation = retry_generation(&outs, RetryKind::Search).expect("watchdog armed");
    let outs = h.send(OnlineMessage::Retry {
        kind: RetryKind::Search,
        generation,
    });
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(outs.contains(&Out::Notice(OnlineNotice::SearchTimedOut)));
}

#[test]
fn the_watchdog_of_the_search_is_dropped_when_the_game_starts() {
    let mut h = Harness::connected();
    h.send(OnlineMessage::Seek(seek()));
    let outs = h.update(h.seek_epoch, 2, StreamKind::Seek(SeekOutcome::Closed));
    let generation = retry_generation(&outs, RetryKind::Search).expect("watchdog armed");
    h.event(3, &game_start());
    h.send(OnlineMessage::Retry {
        kind: RetryKind::Search,
        generation,
    });
    assert!(h.state.phase.is_live());
}

#[test]
fn an_accepted_challenge_without_a_game_start_and_a_silent_end_are_failures() {
    use mychess_online::models::ChallengeUpdate;
    let mut h = Harness::connected();
    h.send(OnlineMessage::Challenge(request()));
    let outs = h.update(
        h.challenge_epoch,
        2,
        StreamKind::Challenge(StreamMessage::Item(ChallengeUpdate::Done(
            "accepted".to_owned(),
        ))),
    );
    // The end of the stream after "accepted" is normal.
    h.update(
        h.challenge_epoch,
        3,
        StreamKind::Challenge(StreamMessage::Ended),
    );
    assert_eq!(h.state.phase, Phase::Challenging);
    let generation = retry_generation(&outs, RetryKind::Search).expect("watchdog armed");
    h.send(OnlineMessage::Retry {
        kind: RetryKind::Search,
        generation,
    });
    assert_eq!(h.state.phase, Phase::Idle);
    // A stream that ends before saying anything is a failure.
    h.send(OnlineMessage::Challenge(request()));
    let outs = h.update(
        h.challenge_epoch,
        4,
        StreamKind::Challenge(StreamMessage::Ended),
    );
    assert_eq!(h.state.phase, Phase::Idle);
    assert!(
        outs.iter()
            .any(|o| matches!(o, Out::Notice(OnlineNotice::Error(_))))
    );
}

#[test]
fn a_move_waiting_for_the_server_does_not_flip_the_clock() {
    let mut h = Harness::playing();
    // Black (the user) is to move with the clocks running.
    h.game_line(
        2,
        &state_json("e2e4 e7e5 g1f3", "started", 590_000, 595_000, ""),
    );
    // The user moves; the move waits for the server. A state that does not
    // include it arrives (a clock update).
    play(&mut h, "b8", "c6", 3);
    h.game_line(
        4,
        &state_json("e2e4 e7e5 g1f3", "started", 589_000, 594_000, ""),
    );
    // Black is still the side to move in the confirmed position: his clock
    // runs, White's stays (D1364).
    let later = h.at(14);
    assert_eq!(
        h.state.clock.shown(Color::White, later),
        Duration::from_secs(589)
    );
    assert!(h.state.clock.shown(Color::Black, later) < Duration::from_secs(594));
}

#[test]
fn the_text_of_the_server_is_cut_and_stripped_of_controls() {
    let mut h = Harness::connected();
    let huge = "x".repeat(1 << 20);
    let json = format!(
        r#"{{"type":"challenge","challenge":{{"id":"chal0001","direction":"in","rated":false,"variant":{{"key":"standard"}},"timeControl":{{"type":"clock","limit":600,"increment":0,"show":"{huge}"}},"challenger":{{"id":"x","name":"a\u0007b\nc{huge}","title":"{huge}"}}}}}}"#
    );
    h.event(1, &json);
    let c = h.state.incoming.first().expect("challenge");
    assert!(c.from.chars().count() <= 64, "{}", c.from.len());
    assert!(c.from.starts_with("abc"), "controls are dropped");
    assert!(c.title.as_deref().is_some_and(|t| t.chars().count() <= 32));
    assert!(c.time.chars().count() <= 32);
}

#[test]
fn a_premove_without_a_promotion_piece_promotes_to_a_queen() {
    use mychess_core::Role;
    let mut h = Harness::playing();
    // Black (the user) has a pawn on b2 and White is to move.
    h.game_line(
        2,
        &state_json(
            "a2a3 b7b5 h2h3 b5b4 h3h4 b4a3 h4h5 a3b2",
            "started",
            1,
            1,
            "",
        ),
    );
    assert!(h.state.game.as_ref().is_some_and(|g| !g.my_turn()));
    h.prefs.premove = true;
    play(&mut h, "b2", "b1", 3);
    let premove = h.state.premove.get().expect("premove set");
    assert_eq!(premove.promotion, Some(Role::Queen));
}

// ---- Chess960 (M9-6) ----

const START_0: &str = "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1";

#[test]
fn m9_6_an_incoming_chess960_challenge_is_playable_and_flagged() {
    let mut h = Harness::connected();
    let line = challenge_event("chal0960", "").replace(
        r#""variant":{"key":"standard","name":"Standard"}"#,
        r#""variant":{"key":"chess960","name":"Chess960"},"initialFen":"bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1""#,
    );
    h.event(1, &line);
    assert!(h.state.incoming[0].playable);
    assert!(h.state.incoming[0].chess960);
    let outs = h.send(OnlineMessage::AcceptChallenge("chal0960".to_owned()));
    assert!(matches!(
        commands(&outs)[..],
        [OnlineCommand::AcceptChallenge { .. }]
    ));
    // Another variant is still refused with the `variant` reason.
    let atomic = challenge_event("chal0001", "").replace("\"standard\"", "\"atomic\"");
    h.event(2, &atomic);
    let item = h.state.incoming.iter().find(|c| c.id == "chal0001");
    assert!(item.is_some_and(|c| !c.playable && c.decline_reason == "variant" && !c.chess960));
}

#[test]
fn m9_6_a_chess960_game_stream_opens_the_game_on_its_start_position() {
    let mut h = Harness::connected();
    h.event(1, &game_start());
    let json = full("", "started", 600_000, 600_000)
        .replace(
            r#""variant":{"key":"standard","name":"Standard"}"#,
            r#""variant":{"key":"chess960","name":"Chess960"}"#,
        )
        .replace(
            r#""initialFen":"startpos""#,
            &format!(r#""initialFen":"{START_0}""#),
        );
    let outs = h.game_line(1, &json);
    assert!(!outs.contains(&Out::Notice(OnlineNotice::UnsupportedVariant)));
    assert!(h.state.game.is_some());
}

#[test]
fn m9_6_chess960_seek_and_challenge_carry_the_variant_to_the_command() {
    let mut h = Harness::connected();
    let seek = SeekRequest {
        variant: OnlineVariant::Chess960 { start: None },
        ..seek()
    };
    let outs = h.send(OnlineMessage::Seek(seek));
    assert!(matches!(
        &commands(&outs)[..],
        [OnlineCommand::Seek { request, .. }] if request.variant.is_chess960()
    ));
    let mut h = Harness::connected();
    let challenge = ChallengeRequest {
        variant: OnlineVariant::Chess960 { start: Some(0) },
        ..request()
    };
    let outs = h.send(OnlineMessage::Challenge(challenge));
    assert!(matches!(
        &commands(&outs)[..],
        [OnlineCommand::Challenge { request, .. }]
            if request.variant == (OnlineVariant::Chess960 { start: Some(0) })
    ));
}

#[test]
fn m9_6_a_fixed_start_needs_a_valid_number_a_casual_game_and_no_seek() {
    let mut h = Harness::connected();
    let seek = SeekRequest {
        variant: OnlineVariant::Chess960 { start: Some(5) },
        ..seek()
    };
    assert!(rejected(
        &h.send(OnlineMessage::Seek(seek)),
        RejectReason::BadVariant
    ));
    let too_big = ChallengeRequest {
        variant: OnlineVariant::Chess960 { start: Some(960) },
        ..request()
    };
    assert!(rejected(
        &h.send(OnlineMessage::Challenge(too_big)),
        RejectReason::BadVariant
    ));
    let rated = ChallengeRequest {
        variant: OnlineVariant::Chess960 { start: Some(10) },
        rated: true,
        ..request()
    };
    assert!(rejected(
        &h.send(OnlineMessage::Challenge(rated)),
        RejectReason::BadVariant
    ));
    assert_eq!(h.state.phase, Phase::Idle);
    // Rated random Chess960 is fine: the server draws the position.
    let ok = ChallengeRequest {
        variant: OnlineVariant::Chess960 { start: None },
        rated: true,
        ..request()
    };
    assert!(!rejected(
        &h.send(OnlineMessage::Challenge(ok)),
        RejectReason::BadVariant
    ));
}

#[test]
fn m8_3_d4520_a_resynchronised_game_is_not_an_entry() {
    let mut h = Harness::connected();
    h.event(1, &game_start());
    let json = full("", "started", 600_000, 600_000);
    let first = h.game_line(1, &json);
    assert!(first.contains(&Out::Opened));
    let again = h.game_line(1, &json);
    assert!(!again.contains(&Out::Opened), "{again:?}");
}
