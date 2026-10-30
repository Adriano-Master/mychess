//! Tests of the electronic board in the state (spec M8-10, criterion 5),
//! driven message by message, with the effects checked as data.

use mychess_core::{Color, GameResult, Position};
use mychess_eboard::{BoardKind, BoardMove, Direction, EboardCommand, EboardEvent, Reason};

use super::*;
use crate::eboard::{
    BoardChoice, ConsoleNote, EboardEffect, EboardIssue, EboardMessage, EboardNotice, EboardPhase,
    EboardPrefs, FilterChoice, UserCommand,
};
use crate::flow::{AppNotice, DiscardChoice};
use crate::session::{GameMessage, Nav, PlayOptions};

fn send(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    update(state, message.into())
}

fn novag_prefs() -> EboardPrefs {
    EboardPrefs {
        kind: BoardChoice::Novag,
        port: "COM3".to_owned(),
        ..EboardPrefs::default()
    }
}

fn driver_prefs() -> EboardPrefs {
    EboardPrefs {
        kind: BoardChoice::InputEngine,
        port: "/dev/ttyUSB0".to_owned(),
        engine_command: "dgtdrv2".to_owned(),
        engine_params: "la".to_owned(),
        send_filter: FilterChoice::Both,
    }
}

/// Connects and makes the board ready; returns the epoch.
fn ready(state: &mut AppState, prefs: EboardPrefs) -> u64 {
    send(state, EboardMessage::Connect(Box::new(prefs)));
    let epoch = state.eboard().epoch_for_tests();
    send(state, EboardMessage::Connected { epoch });
    event(state, epoch, EboardEvent::Ready);
    epoch
}

fn event(state: &mut AppState, epoch: u64, event: EboardEvent) -> Vec<Effect> {
    send(state, EboardMessage::Event { epoch, event })
}

fn sent(effects: &[Effect]) -> Vec<EboardCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Eboard(EboardEffect::Send { command, .. }) => Some(command.clone()),
            _ => None,
        })
        .collect()
}

fn board_move(text: &str) -> BoardMove {
    BoardMove::parse(text).expect("move")
}

fn play(state: &mut AppState, uci: &str) -> Vec<Effect> {
    let mv = state.session.position().parse_uci(uci).expect("uci");
    send(
        state,
        GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        },
    )
}

fn notice(state: &AppState) -> Option<EboardNotice> {
    match &state.notice {
        Some(AppNotice::Eboard(n)) => Some(n.clone()),
        _ => None,
    }
}

#[test]
fn connecting_without_a_board_or_with_bad_settings_is_refused() {
    let mut state = AppState::default();
    let effects = send(&mut state, EboardMessage::Connect(Box::default()));
    assert!(effects.is_empty());
    assert_eq!(
        notice(&state),
        Some(EboardNotice::BadSettings(EboardIssue::NoBoardChosen))
    );
    assert_eq!(state.eboard().phase(), EboardPhase::Idle);

    let mut bad = novag_prefs();
    bad.port = String::new();
    send(&mut state, EboardMessage::Connect(Box::new(bad)));
    assert_eq!(
        notice(&state),
        Some(EboardNotice::BadSettings(EboardIssue::InvalidPort))
    );
}

#[test]
fn connect_asks_for_the_session_and_keeps_the_settings() {
    let mut state = AppState::default();
    let effects = send(&mut state, EboardMessage::Connect(Box::new(novag_prefs())));
    let connect = effects.iter().find_map(|e| match e {
        Effect::Eboard(EboardEffect::Connect { kind, epoch, .. }) => Some((kind.clone(), *epoch)),
        _ => None,
    });
    let (kind, epoch) = connect.expect("a connect effect");
    assert_eq!(
        kind,
        BoardKind::Novag {
            port: "COM3".to_owned()
        }
    );
    assert_eq!(epoch, 1);
    assert_eq!(state.eboard().phase(), EboardPhase::Connecting);
    assert_eq!(state.prefs.eboard.kind, BoardChoice::Novag);
    // A second connect while connecting does nothing.
    let again = send(&mut state, EboardMessage::Connect(Box::new(novag_prefs())));
    assert!(again.is_empty());
}

#[test]
fn ready_connects_and_stale_events_are_dropped() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    assert!(state.eboard().is_connected());
    assert_eq!(notice(&state), Some(EboardNotice::Connected));
    state.notice = None;
    event(&mut state, epoch + 7, EboardEvent::NewGame);
    event(
        &mut state,
        epoch + 7,
        EboardEvent::Move {
            mv: board_move("e2e4"),
            side: None,
        },
    );
    assert_eq!(state.session.position(), &Position::new());
    assert_eq!(state.notice, None);
}

#[test]
fn a_legal_move_of_the_board_is_played_and_not_sent_back() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    let effects = event(
        &mut state,
        epoch,
        EboardEvent::Move {
            mv: board_move("e2e4"),
            side: Some(Color::White),
        },
    );
    assert_eq!(state.session.position().turn(), Color::Black);
    assert!(sent(&effects).is_empty(), "{effects:?}");
    let last = state.eboard().last_move().expect("last move");
    assert!(last.accepted());
    assert_eq!(last.text, "e2e4");
}

#[test]
fn a_move_out_of_turn_is_the_echo_and_is_ignored_silently() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    state.notice = None;
    event(
        &mut state,
        epoch,
        EboardEvent::Move {
            mv: board_move("e7e5"),
            side: Some(Color::Black),
        },
    );
    assert_eq!(state.session.position(), &Position::new());
    assert_eq!(state.notice, None);
    assert!(state.eboard().last_move().is_none());
}

#[test]
fn an_illegal_move_of_the_board_changes_nothing_and_warns() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    event(
        &mut state,
        epoch,
        EboardEvent::Move {
            mv: board_move("e2e5"),
            side: None,
        },
    );
    assert_eq!(state.session.position(), &Position::new());
    assert_eq!(
        notice(&state),
        Some(EboardNotice::IllegalMove("e2e5".into()))
    );
    assert!(!state.eboard().last_move().expect("last").accepted());
    assert!(
        state
            .eboard()
            .console()
            .any(|l| l.direction == Direction::Note
                && l.note == Some(ConsoleNote::IllegalMove)
                && l.text == "e2e5")
    );
}

#[test]
fn a_move_of_the_board_over_an_existing_line_becomes_a_variation() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    play(&mut state, "e2e4");
    send(&mut state, GameMessage::Navigate(Nav::Start));
    event(
        &mut state,
        epoch,
        EboardEvent::Move {
            mv: board_move("d2d4"),
            side: None,
        },
    );
    // No question was asked, the move is in the game as a variation.
    assert!(state.session.prompt().is_none());
    let root = state.session.game().root();
    assert_eq!(
        state.session.game().children(root).map(<[_]>::len).ok(),
        Some(2)
    );
}

#[test]
fn the_driver_is_asked_for_the_position_after_an_accepted_move() {
    let mut state = AppState::default();
    // The first position of the driver starts the game.
    let epoch = ready(&mut state, driver_prefs());
    event(&mut state, epoch, EboardEvent::Fen(Position::new().fen()));
    let effects = event(
        &mut state,
        epoch,
        EboardEvent::Move {
            mv: board_move("g1f3"),
            side: None,
        },
    );
    assert_eq!(sent(&effects), [EboardCommand::Query]);
}

#[test]
fn the_user_move_on_the_screen_goes_to_the_novag_with_the_capture_flag() {
    let mut state = AppState::default();
    ready(&mut state, novag_prefs());
    let first = play(&mut state, "e2e4");
    assert_eq!(
        sent(&first),
        [EboardCommand::Move {
            uci: "e2e4".into(),
            capture: false
        }]
    );
    play(&mut state, "d7d5");
    let capture = play(&mut state, "e4d5");
    assert_eq!(
        sent(&capture),
        [EboardCommand::Move {
            uci: "e4d5".into(),
            capture: true
        }]
    );
    // Navigating is not a move of the user.
    let back = send(&mut state, GameMessage::Navigate(Nav::Back(1)));
    assert!(sent(&back).is_empty());
}

#[test]
fn nothing_goes_to_a_driver_or_to_a_board_that_is_not_ready() {
    let mut state = AppState::default();
    assert!(sent(&play(&mut state, "e2e4")).is_empty());
    let mut driver = AppState::default();
    ready(&mut driver, driver_prefs());
    assert!(sent(&play(&mut driver, "e2e4")).is_empty());
}

#[test]
fn the_first_position_of_the_driver_starts_the_game_and_later_ones_are_compared() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    let odd = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    event(&mut state, epoch, EboardEvent::Fen(odd.to_owned()));
    assert_eq!(state.session.position().fen(), odd);
    assert_eq!(state.eboard().in_sync(), Some(true));

    state.notice = None;
    event(
        &mut state,
        epoch,
        EboardEvent::Fen("4k3/8/8/8/8/8/3P4/4K3 w - - 0 1".to_owned()),
    );
    assert_eq!(state.eboard().in_sync(), Some(false));
    assert_eq!(notice(&state), Some(EboardNotice::OutOfSync));

    // Same pieces, different castling and move counters: in sync again.
    event(
        &mut state,
        epoch,
        EboardEvent::Fen("4k3/8/8/8/8/8/4P3/4K3 b - - 9 9".to_owned()),
    );
    assert_eq!(state.eboard().in_sync(), Some(true));
}

#[test]
fn a_position_that_is_not_a_fen_is_refused_and_the_start_stays_armed() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    event(&mut state, epoch, EboardEvent::Fen("not a fen".to_owned()));
    assert_eq!(notice(&state), Some(EboardNotice::BadPosition));
    event(&mut state, epoch, EboardEvent::Fen(Position::new().fen()));
    assert_eq!(notice(&state), Some(EboardNotice::BadPosition));
    assert_eq!(state.eboard().in_sync(), Some(true));
}

#[test]
fn synchronise_rearms_the_start_and_asks_the_driver() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    event(&mut state, epoch, EboardEvent::Fen(Position::new().fen()));
    let effects = send(&mut state, EboardMessage::Command(UserCommand::Synchronise));
    assert_eq!(sent(&effects), [EboardCommand::Query]);
    let odd = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
    event(&mut state, epoch, EboardEvent::Fen(odd.to_owned()));
    assert_eq!(state.session.position().fen(), odd);
}

#[test]
fn take_back_goes_one_move_back() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    play(&mut state, "e2e4");
    play(&mut state, "e7e5");
    event(&mut state, epoch, EboardEvent::TakeBack);
    assert_eq!(state.session.position().turn(), Color::Black);
    assert_eq!(state.session.game().mainline().count(), 2);
}

#[test]
fn a_new_game_of_the_board_asks_before_discarding_an_altered_game() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    event(&mut state, epoch, EboardEvent::NewGame);
    assert!(state.discard_prompt.is_none());
    play(&mut state, "e2e4");
    event(&mut state, epoch, EboardEvent::NewGame);
    assert!(state.discard_prompt.is_some());
    assert_eq!(state.session.position().turn(), Color::Black);
}

#[test]
fn the_end_of_the_game_writes_the_result_and_does_not_save() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    event(&mut state, epoch, EboardEvent::GameEnd(GameResult::Black));
    assert_eq!(state.session.game().tags().result, GameResult::Black);
    assert_eq!(
        notice(&state),
        Some(EboardNotice::ResultRecorded(GameResult::Black))
    );
}

#[test]
fn clocks_and_identification_are_kept_for_the_dialog() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    event(
        &mut state,
        epoch,
        EboardEvent::Clock {
            color: Color::White,
            seconds: 3599,
        },
    );
    event(&mut state, epoch, EboardEvent::ToMove(Color::Black));
    event(
        &mut state,
        epoch,
        EboardEvent::BoardIdentified("DGT XL".into()),
    );
    assert_eq!(state.eboard().clock(Color::White), Some(3599));
    assert_eq!(state.eboard().clock(Color::Black), None);
    assert_eq!(state.eboard().to_move(), Some(Color::Black));
    assert_eq!(state.eboard().board_name(), Some("DGT XL"));
    event(&mut state, epoch, EboardEvent::NoClock);
    assert_eq!(state.eboard().clock(Color::White), None);
}

#[test]
fn the_console_is_bounded() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    for i in 0..(crate::eboard::MAX_CONSOLE_LINES + 50) {
        event(
            &mut state,
            epoch,
            EboardEvent::Log {
                direction: Direction::In,
                text: format!("line {i}"),
            },
        );
    }
    assert_eq!(
        state.eboard().console().count(),
        crate::eboard::MAX_CONSOLE_LINES
    );
    assert!(
        state
            .eboard()
            .console()
            .last()
            .is_some_and(|l| l.text.ends_with("249"))
    );
    send(&mut state, EboardMessage::ClearConsole);
    assert_eq!(state.eboard().console().count(), 0);
}

#[test]
fn losing_the_device_ends_the_session_with_a_reason() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    event(
        &mut state,
        epoch,
        EboardEvent::Disconnected(Reason::DeviceLost("unplugged".into())),
    );
    assert_eq!(state.eboard().phase(), EboardPhase::Idle);
    assert_eq!(
        notice(&state),
        Some(EboardNotice::ConnectionLost("unplugged".into()))
    );
    assert!(state.eboard().issue().is_some());
    // Moves after that are not played.
    event(
        &mut state,
        epoch,
        EboardEvent::Move {
            mv: board_move("e2e4"),
            side: None,
        },
    );
    assert_eq!(state.session.position(), &Position::new());
}

#[test]
fn a_failed_connection_goes_back_to_idle_with_the_message() {
    let mut state = AppState::default();
    send(&mut state, EboardMessage::Connect(Box::new(novag_prefs())));
    let epoch = state.eboard().epoch_for_tests();
    send(
        &mut state,
        EboardMessage::ConnectFailed {
            epoch,
            error: "cannot open serial port COM3".into(),
        },
    );
    assert_eq!(state.eboard().phase(), EboardPhase::Idle);
    assert!(matches!(
        notice(&state),
        Some(EboardNotice::ConnectFailed(m)) if m.contains("COM3")
    ));
}

#[test]
fn cancelling_while_connecting_closes_what_arrives_late() {
    let mut state = AppState::default();
    send(&mut state, EboardMessage::Connect(Box::new(novag_prefs())));
    let epoch = state.eboard().epoch_for_tests();
    let cancel = send(&mut state, EboardMessage::Disconnect);
    assert!(cancel.iter().any(|e| matches!(
        e,
        Effect::Eboard(EboardEffect::Disconnect { epoch: e, .. }) if *e == epoch
    )));
    assert_eq!(state.eboard().phase(), EboardPhase::Idle);
    let late = send(&mut state, EboardMessage::Connected { epoch });
    assert!(late.iter().any(|e| matches!(
        e,
        Effect::Eboard(EboardEffect::Disconnect { epoch: e, .. }) if *e == epoch
    )));
}

#[test]
fn disconnect_and_quit_end_the_session_once() {
    let mut state = AppState::default();
    ready(&mut state, novag_prefs());
    let effects = send(&mut state, EboardMessage::Disconnect);
    assert_eq!(
        effects
            .iter()
            .filter(|e| matches!(e, Effect::Eboard(EboardEffect::Disconnect { .. })))
            .count(),
        1
    );
    assert!(send(&mut state, EboardMessage::Disconnect).is_empty());

    let mut other = AppState::default();
    ready(&mut other, novag_prefs());
    let effects = send(&mut other, Message::Quit);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Eboard(EboardEffect::Disconnect { .. })))
    );
}

#[test]
fn user_commands_reach_the_driver_only_when_ready() {
    let mut state = AppState::default();
    let early = send(&mut state, EboardMessage::Command(UserCommand::Rotate));
    assert!(sent(&early).is_empty());
    ready(&mut state, driver_prefs());
    let rotate = send(&mut state, EboardMessage::Command(UserCommand::Rotate));
    assert_eq!(sent(&rotate), [EboardCommand::Rotate, EboardCommand::Query]);
    let info = send(&mut state, EboardMessage::Command(UserCommand::Info));
    assert_eq!(sent(&info), [EboardCommand::SystemInfo]);
    let raw = send(
        &mut state,
        EboardMessage::Command(UserCommand::Raw("hello".into())),
    );
    assert_eq!(sent(&raw), [EboardCommand::Raw("hello".into())]);
    let filter = send(
        &mut state,
        EboardMessage::Command(UserCommand::Filter(FilterChoice::White)),
    );
    assert_eq!(
        sent(&filter),
        [EboardCommand::SendFilter(mychess_eboard::SendFilter::White)]
    );
    assert_eq!(state.prefs.eboard.send_filter, FilterChoice::White);
}

#[test]
fn the_ports_list_is_kept_or_its_error() {
    let mut state = AppState::default();
    let effects = send(&mut state, EboardMessage::RefreshPorts);
    assert!(effects.contains(&Effect::Eboard(EboardEffect::ListPorts)));
    send(
        &mut state,
        EboardMessage::PortsListed(Ok(vec![mychess_eboard::PortInfo {
            name: "COM4".into(),
            description: "USB".into(),
        }])),
    );
    assert_eq!(state.eboard().ports().len(), 1);
    send(&mut state, EboardMessage::PortsListed(Err("no".into())));
    assert!(state.eboard().ports().is_empty());
    assert_eq!(state.eboard().ports_error(), Some("no"));
}

#[test]
fn the_settings_round_trip_through_the_preferences_file() {
    let mut prefs = Preferences::default();
    *prefs.eboard = driver_prefs();
    let text = crate::prefs::render_preferences(&prefs);
    assert!(text.contains("[eboard]"), "{text}");
    let loaded = crate::prefs::load_preferences(&text);
    assert_eq!(*loaded.prefs.eboard, driver_prefs());
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);

    let bad = crate::prefs::load_preferences("[eboard]\nkind = \"abacus\"\nport = 5\n");
    assert_eq!(bad.prefs.eboard.kind, BoardChoice::None);
    assert_eq!(bad.issues.len(), 2);
}

#[test]
fn a_new_game_of_the_board_names_its_origin_in_the_event_tag() {
    for (prefs, tag) in [
        (novag_prefs(), "Novag Citrine"),
        (driver_prefs(), "InputEngine Input"),
    ] {
        let mut state = AppState::default();
        let epoch = ready(&mut state, prefs);
        assert_eq!(state.session.game().tags().event, "?");
        event(&mut state, epoch, EboardEvent::NewGame);
        assert_eq!(state.session.game().tags().event, tag);
    }
}

#[test]
fn the_event_tag_waits_for_the_answer_about_the_altered_game() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, novag_prefs());
    play(&mut state, "e2e4");
    event(&mut state, epoch, EboardEvent::NewGame);
    assert!(state.discard_prompt.is_some());
    assert_eq!(state.session.game().tags().event, "?", "nothing yet");
    send(&mut state, Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(state.session.game().tags().event, "Novag Citrine");
    assert_eq!(state.session.game().mainline().count(), 0);
}

#[test]
fn the_start_position_of_the_driver_also_sets_the_event_tag() {
    let mut state = AppState::default();
    let epoch = ready(&mut state, driver_prefs());
    event(&mut state, epoch, EboardEvent::StartupFen);
    event(
        &mut state,
        epoch,
        EboardEvent::Fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_owned()),
    );
    assert_eq!(state.session.game().tags().event, "InputEngine Input");
}
