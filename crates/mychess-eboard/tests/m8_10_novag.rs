//! Novag session over the fake serial port (spec m8-10, criterion 3).

use std::time::Duration;

use mychess_core::Color;
use mychess_eboard::testing::fake_serial;
use mychess_eboard::{
    BoardMove, Direction, EboardCommand, EboardEvent, EboardEvents, Note, NovagTimings, Reason,
    Warning, start_novag,
};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(5);

fn fast() -> NovagTimings {
    NovagTimings {
        init_gap: Duration::from_millis(15),
        move_gap: Duration::from_millis(15),
        capture_gap: Duration::from_millis(80),
        reply_warning: Duration::from_millis(400),
    }
}

/// The writer thread runs on its own: wait until it has written `count` lines.
async fn written(board: &mychess_eboard::testing::FakeBoard, count: usize) -> Vec<String> {
    for _ in 0..200 {
        if board.written_lines().len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    board.written_lines()
}

async fn next(events: &mut EboardEvents) -> EboardEvent {
    timeout(WAIT, events.next())
        .await
        .expect("an event in time")
        .expect("the session is alive")
}

/// Skips console lines and returns the first other event.
async fn next_significant(events: &mut EboardEvents) -> EboardEvent {
    loop {
        match next(events).await {
            EboardEvent::Log { .. } => {}
            other => return other,
        }
    }
}

#[tokio::test]
async fn start_up_sends_the_four_commands_in_order_then_ready() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    assert_eq!(written(&board, 4).await, ["I", "N", "X ON", "U ON"]);
    assert!(board.written().starts_with("I\n\r\n"));
}

#[tokio::test]
async fn board_lines_become_events_with_the_side() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);

    board.say("M 1 e2-e4");
    board.say("M 1, e7-e5");
    board.say("T");
    let EboardEvent::Move { mv, side } = next_significant(&mut events).await else {
        panic!("a move");
    };
    assert_eq!(side, Some(Color::White));
    assert_eq!(mv, BoardMove::parse("e2e4").unwrap());
    let EboardEvent::Move { side, .. } = next_significant(&mut events).await else {
        panic!("a move");
    };
    assert_eq!(side, Some(Color::Black));
    assert_eq!(next_significant(&mut events).await, EboardEvent::TakeBack);
}

#[tokio::test]
async fn new_game_is_reported_and_answered_with_u_on() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    board.say("New Game");
    assert_eq!(next_significant(&mut events).await, EboardEvent::NewGame);
    // Wait for the answer to be written.
    for _ in 0..100 {
        if board.written_lines().len() == 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        board.written_lines().last().map(String::as_str),
        Some("U ON")
    );
}

#[tokio::test]
async fn a_move_is_sent_twice_with_the_longer_wait_after_a_capture() {
    let (device, _board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);

    assert!(handle.send(EboardCommand::Move {
        uci: "e7e8q".into(),
        capture: true
    }));
    // The first send comes at once, the second after `capture_gap`.
    let mut seen = Vec::new();
    let started = std::time::Instant::now();
    let mut second_at = None;
    while seen.len() < 2 && started.elapsed() < WAIT {
        if let EboardEvent::Log {
            direction: Direction::Out,
            text,
        } = next(&mut events).await
            && text.starts_with("M ")
        {
            seen.push(text);
            if seen.len() == 2 {
                second_at = Some(started.elapsed());
            }
        }
    }
    assert_eq!(seen, ["M e7e8/q", "M e7e8/q"]);
    assert!(second_at.is_some_and(|t| t >= Duration::from_millis(60)));
}

#[tokio::test]
async fn silence_gives_one_warning_and_the_session_goes_on() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    assert_eq!(
        next_significant(&mut events).await,
        EboardEvent::Warning(Warning::NoReply)
    );
    board.say("M 1 d2d4");
    assert!(matches!(
        next_significant(&mut events).await,
        EboardEvent::Move { .. }
    ));
}

#[tokio::test]
async fn a_reply_cancels_the_silence_warning() {
    let (device, board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    board.say("Novag Citrine");
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    tokio::time::sleep(Duration::from_millis(600)).await;
    handle.close();
    loop {
        match next(&mut events).await {
            EboardEvent::Warning(w) => panic!("unexpected {w:?}"),
            EboardEvent::Disconnected(reason) => {
                assert_eq!(reason, Reason::Requested);
                break;
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn an_unplugged_device_ends_the_session() {
    let (device, board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    board.fail(std::io::ErrorKind::BrokenPipe);
    let EboardEvent::Disconnected(Reason::DeviceLost(message)) =
        next_significant(&mut events).await
    else {
        panic!("device lost");
    };
    assert_ne!(message, "");
    assert!(events.next().await.is_none());
    assert!(!handle.send(EboardCommand::Query));
}

#[tokio::test]
async fn hang_up_is_a_lost_device_too() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    board.hang_up();
    loop {
        if let EboardEvent::Disconnected(reason) = next(&mut events).await {
            assert!(matches!(reason, Reason::DeviceLost(_)));
            break;
        }
    }
}

#[tokio::test]
async fn close_releases_the_device() {
    let (device, board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    assert!(handle.close());
    assert_eq!(
        next_significant(&mut events).await,
        EboardEvent::Disconnected(Reason::Requested)
    );
    // No waiting: the port is free when the session says it is over (item 7).
    assert!(
        board.reader_closed(),
        "the reader thread must drop its half"
    );
}

#[tokio::test]
async fn dropping_every_handle_ends_the_session() {
    let (device, _board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    drop(handle);
    loop {
        if let EboardEvent::Disconnected(reason) = next(&mut events).await {
            assert_eq!(reason, Reason::Requested);
            break;
        }
    }
}

#[tokio::test]
async fn hostile_input_does_not_stop_the_session() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    board.send_bytes(&[0xff, 0xfe, 0, b'\n']);
    board.send_bytes(&vec![b'x'; 20_000]);
    board.send_bytes(b"\r\nM 1 zz-zz\r\nM\r\nM 1 e2e4\r\n");
    loop {
        match next_significant(&mut events).await {
            EboardEvent::Move { .. } => break,
            EboardEvent::Note(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[tokio::test]
async fn driver_only_commands_are_noted_not_sent() {
    let (device, board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    handle.send(EboardCommand::Query);
    let note = next(&mut events).await;
    assert_eq!(note, EboardEvent::Note(Note::InputEngineOnly));
    assert_eq!(written(&board, 4).await.len(), 4);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(board.written_lines().len(), 4);
}

#[tokio::test]
async fn unreadable_moves_are_typed_notes() {
    let (device, board) = fake_serial();
    let (_handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    board.say("M 1 zz-zz");
    assert!(matches!(
        next_significant(&mut events).await,
        EboardEvent::Note(Note::UnreadableMove(_))
    ));
}

#[tokio::test]
async fn a_port_that_times_out_on_a_write_is_tried_again() {
    let (device, board) = fake_serial();
    board.stall_writes(3);
    let (handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    assert_eq!(written(&board, 4).await, ["I", "N", "X ON", "U ON"]);
    assert!(handle.is_open(), "a busy port is not a lost one");
}

#[tokio::test]
async fn a_flood_of_lines_is_cut_with_a_count() {
    let (device, board) = fake_serial();
    let (handle, mut events) = start_novag(device, fast());
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    let started = std::time::Instant::now();
    for _ in 0..1_000_000 {
        board.say("zzz");
    }
    // Every line is either delivered (one `Log`) or counted as omitted.
    let mut delivered = 0_u64;
    let mut omitted = 0_u64;
    while delivered + omitted < 1_000_000 {
        match next(&mut events).await {
            EboardEvent::Note(Note::LinesOmitted(n)) => omitted += u64::from(n),
            EboardEvent::Log { .. } => delivered += 1,
            _ => {}
        }
    }
    let windows = started.elapsed().as_secs() + 2;
    assert!(
        delivered <= 200 * windows,
        "{delivered} lines in {windows} windows"
    );
    assert!(delivered > 0 && omitted > 0, "{delivered} / {omitted}");
    handle.close();
}
