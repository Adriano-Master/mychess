//! Input engine session over in-memory streams and over the real fake
//! process (spec m8-10, criterion 4).

use std::path::PathBuf;
use std::time::Duration;

use mychess_core::{Color, GameResult};
use mychess_eboard::testing::{FakeDriver, run_fake_input_engine};
use mychess_eboard::{
    BoardKind, BoardMove, Direction, Eboard, EboardCommand, EboardEvent, EboardEvents, Note,
    Reason, SendFilter, Timeouts, Warning, connect, start_driver,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

fn quick() -> Timeouts {
    Timeouts {
        handshake: Duration::from_millis(300),
        shutdown: Duration::from_millis(300),
    }
}

async fn next(events: &mut EboardEvents) -> EboardEvent {
    timeout(WAIT, events.next())
        .await
        .expect("an event in time")
        .expect("the session is alive")
}

async fn next_significant(events: &mut EboardEvents) -> EboardEvent {
    loop {
        match next(events).await {
            EboardEvent::Log { .. } => {}
            other => return other,
        }
    }
}

/// A session wired to the in-process fake driver.
fn with_fake(fake: FakeDriver, timeouts: Timeouts) -> (Eboard, EboardEvents) {
    let (session_end, driver_end) = duplex(8192);
    let (session_read, session_write) = tokio::io::split(session_end);
    let (driver_read, driver_write) = tokio::io::split(driver_end);
    tokio::spawn(run_fake_input_engine(driver_read, driver_write, fake));
    start_driver(session_read, session_write, None, timeouts)
}

#[tokio::test]
async fn the_handshake_has_two_steps_and_ends_ready() {
    let (_handle, mut events) = with_fake(FakeDriver::default(), Timeouts::default());
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    // `getposition` after the xboard mode: the FEN comes back.
    let fen = loop {
        if let EboardEvent::Fen(fen) = next_significant(&mut events).await {
            break fen;
        }
    };
    assert!(fen.starts_with("rnbqkbnr/pppppppp"));
}

#[tokio::test]
async fn the_console_shows_what_was_sent_in_order() {
    let (_handle, mut events) = with_fake(FakeDriver::default(), Timeouts::default());
    let mut out = Vec::new();
    while out.len() < 3 {
        if let EboardEvent::Log {
            direction: Direction::Out,
            text,
        } = next(&mut events).await
        {
            out.push(text);
        }
    }
    assert_eq!(out, ["xboard", "announce", "getposition"]);
}

#[tokio::test]
async fn moves_and_markers_become_events() {
    let (handle, mut events) = with_fake(FakeDriver::default(), Timeouts::default());
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    for line in [
        "fake-move e2e4",
        "fake-raw info !black to move!",
        "fake-raw info Time White: #3599",
        "fake-raw info !end game 1/2-1/2!",
        "fake-raw info !enter setup mode!",
        "fake-raw info moving piece: P",
    ] {
        assert!(handle.send(EboardCommand::Raw(line.into())));
    }
    let mut got = Vec::new();
    while got.len() < 6 {
        match next_significant(&mut events).await {
            EboardEvent::Fen(_) => {}
            other => got.push(other),
        }
    }
    assert_eq!(
        got,
        [
            EboardEvent::Move {
                mv: BoardMove::parse("e2e4").unwrap(),
                side: None
            },
            EboardEvent::ToMove(Color::Black),
            EboardEvent::Clock {
                color: Color::White,
                seconds: 3599
            },
            EboardEvent::GameEnd(GameResult::Draw),
            EboardEvent::SetupMode(true),
            EboardEvent::MovingPiece('P'),
        ]
    );
}

#[tokio::test]
async fn commands_reach_the_driver_once_ready() {
    let (handle, mut events) = with_fake(FakeDriver::default(), Timeouts::default());
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    assert!(handle.send(EboardCommand::SendFilter(SendFilter::White)));
    assert!(handle.send(EboardCommand::Rotate));
    assert!(handle.send(EboardCommand::Query));
    let mut sent = Vec::new();
    while sent.len() < 3 {
        if let EboardEvent::Log {
            direction: Direction::Out,
            text,
        } = next(&mut events).await
        {
            sent.push(text);
        }
    }
    assert_eq!(
        sent,
        ["sendwhite", "rotateboard", "getposition"].map(String::from)
    );
}

#[tokio::test]
async fn a_command_before_ready_is_refused() {
    let silent = FakeDriver {
        silent: true,
        ..FakeDriver::default()
    };
    let (handle, mut events) = with_fake(silent, Timeouts::default());
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert!(handle.send(EboardCommand::Query));
    assert_eq!(
        next_significant(&mut events).await,
        EboardEvent::Warning(Warning::NotReady)
    );
}

#[tokio::test]
async fn a_silent_driver_times_out_and_gets_stop_and_quit() {
    let silent = FakeDriver {
        silent: true,
        ..FakeDriver::default()
    };
    let (_handle, mut events) = with_fake(silent, quick());
    let mut said = Vec::new();
    loop {
        match next(&mut events).await {
            EboardEvent::Log {
                direction: Direction::Out,
                text,
            } => said.push(text),
            EboardEvent::Disconnected(reason) => {
                assert_eq!(reason, Reason::HandshakeTimeout);
                break;
            }
            _ => {}
        }
    }
    assert_eq!(said, ["stop", "quit"]);
}

#[tokio::test]
async fn a_driver_error_is_reported_then_the_session_ends() {
    let broken = FakeDriver {
        error: Some("no board on COM9".into()),
        ..FakeDriver::default()
    };
    let (_handle, mut events) = with_fake(broken, Timeouts::default());
    assert_eq!(
        next_significant(&mut events).await,
        EboardEvent::DriverError("no board on COM9".into())
    );
    assert_eq!(
        next_significant(&mut events).await,
        EboardEvent::Disconnected(Reason::DriverError("no board on COM9".into()))
    );
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn a_driver_that_exits_is_a_disconnection() {
    let quitter = FakeDriver {
        exit_after_handshake: true,
        ..FakeDriver::default()
    };
    let (handle, mut events) = with_fake(quitter, Timeouts::default());
    loop {
        if let EboardEvent::Disconnected(reason) = next(&mut events).await {
            assert_eq!(reason, Reason::DriverExited);
            break;
        }
    }
    assert!(events.next().await.is_none());
    assert!(!handle.is_open());
}

#[tokio::test]
async fn close_says_goodbye_and_is_the_last_event() {
    let (handle, mut events) = with_fake(FakeDriver::default(), Timeouts::default());
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    handle.close();
    let mut tail = Vec::new();
    loop {
        match next(&mut events).await {
            EboardEvent::Disconnected(reason) => {
                assert_eq!(reason, Reason::Requested);
                break;
            }
            EboardEvent::Log {
                direction: Direction::Out,
                text,
            } => tail.push(text),
            _ => {}
        }
    }
    assert_eq!(&tail[tail.len() - 2..], ["stop", "quit"].map(String::from));
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn hostile_driver_output_does_not_stop_the_session() {
    let (session_end, driver_end) = duplex(1 << 16);
    let (session_read, session_write) = tokio::io::split(session_end);
    let (_driver_read, mut driver_write) = tokio::io::split(driver_end);
    let (_handle, mut events) =
        start_driver(session_read, session_write, None, Timeouts::default());
    driver_write
        .write_all(b"\xff\xfe\0\0\ninfo \xff\n")
        .await
        .unwrap();
    driver_write.write_all(&vec![b'x'; 100_000]).await.unwrap();
    driver_write
        .write_all(b"\nmove \nmove zz\ninfo Time White: 99999999999\ninfo string Chessboard found and initialised\ninfo Engine mode : xboard\n")
        .await
        .unwrap();
    loop {
        match next_significant(&mut events).await {
            EboardEvent::Ready => break,
            // The huge clock is cut to the limit; the bad moves are notes.
            EboardEvent::Clock { .. } | EboardEvent::Note(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn fake_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mychess-fake-inputengine"))
}

#[tokio::test]
async fn the_real_process_starts_talks_and_stops() {
    let kind = BoardKind::InputEngine {
        command: fake_binary(),
        port: "FAKE0".into(),
        params: "la".into(),
    };
    let (handle, mut events) = connect(kind).await.expect("the fake driver starts");
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    assert!(handle.send(EboardCommand::Raw("fake-move g1f3".into())));
    loop {
        if let EboardEvent::Move { mv, .. } = next_significant(&mut events).await {
            assert_eq!(mv, BoardMove::parse("g1f3").unwrap());
            break;
        }
    }
    handle.close();
    loop {
        if let EboardEvent::Disconnected(reason) = next(&mut events).await {
            assert_eq!(reason, Reason::Requested);
            break;
        }
    }
}

#[tokio::test]
async fn the_real_process_that_exits_by_itself_is_noticed() {
    let kind = BoardKind::InputEngine {
        command: fake_binary(),
        port: "FAKE0".into(),
        params: "--exit-after-handshake".into(),
    };
    let (_handle, mut events) = connect(kind).await.expect("starts");
    loop {
        if let EboardEvent::Disconnected(reason) = next(&mut events).await {
            assert_eq!(reason, Reason::DriverExited);
            break;
        }
    }
}

#[tokio::test]
async fn a_missing_driver_is_a_clean_error() {
    let missing = std::env::temp_dir().join("mychess-no-such-driver");
    let err = connect(BoardKind::InputEngine {
        command: missing,
        port: "COM1".into(),
        params: String::new(),
    })
    .await
    .expect_err("must fail");
    assert!(
        matches!(err, mychess_eboard::EboardError::Spawn { .. }),
        "{err:?}"
    );
    let err = connect(BoardKind::InputEngine {
        command: PathBuf::new(),
        port: "COM1".into(),
        params: String::new(),
    })
    .await
    .expect_err("empty command");
    assert!(matches!(err, mychess_eboard::EboardError::InvalidDriver(_)));
    let err = connect(BoardKind::Novag {
        port: "a\0b".into(),
    })
    .await
    .expect_err("bad port");
    assert!(matches!(err, mychess_eboard::EboardError::InvalidPort(_)));
}

#[tokio::test]
async fn the_fake_driver_reads_commands_it_does_not_know() {
    // Direct check of the fake: unknown commands are answered, `quit` ends it.
    let (a, b) = duplex(4096);
    let (read_a, mut write_a) = tokio::io::split(a);
    let (read_b, write_b) = tokio::io::split(b);
    let task = tokio::spawn(run_fake_input_engine(
        read_b,
        write_b,
        FakeDriver::default(),
    ));
    write_a.write_all(b"hello\nquit\n").await.unwrap();
    let mut lines = BufReader::new(read_a).lines();
    let mut all = Vec::new();
    while let Some(l) = lines.next_line().await.unwrap() {
        all.push(l);
    }
    task.await.unwrap();
    assert!(all.iter().any(|l| l == "info string ok hello"));
}

/// A session fed by the test through `driver_write`.
fn raw_session(
    timeouts: Timeouts,
) -> (
    Eboard,
    EboardEvents,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
) {
    let (session_end, driver_end) = duplex(1 << 16);
    let (session_read, session_write) = tokio::io::split(session_end);
    let (_driver_read, driver_write) = tokio::io::split(driver_end);
    let (handle, events) = start_driver(session_read, session_write, None, timeouts);
    (handle, events, driver_write)
}

#[tokio::test]
async fn handshake_lines_out_of_order_are_ignored() {
    let (_handle, mut events, mut driver) = raw_session(Timeouts::default());
    // The second step first, then the first twice, then the second twice:
    // only the right order counts, once.
    driver
        .write_all(
            b"info Engine mode : xboard\n\
              info string Chessboard found and initialised\n\
              info string Chessboard found and initialised\n\
              info Engine mode : xboard\n\
              info Engine mode : xboard\n",
        )
        .await
        .unwrap();
    let mut out = Vec::new();
    let mut ready = 0;
    while let Ok(Some(event)) = timeout(Duration::from_millis(300), events.next()).await {
        match event {
            EboardEvent::Log {
                direction: Direction::Out,
                text,
            } => out.push(text),
            EboardEvent::Ready => ready += 1,
            _ => {}
        }
    }
    assert_eq!(out, ["xboard", "announce", "getposition"]);
    assert_eq!(ready, 1);
}

#[tokio::test]
async fn a_flood_of_driver_lines_is_cut_with_a_count() {
    let (_handle, mut events, mut driver) = raw_session(Timeouts::default());
    driver
        .write_all(b"info string Chessboard found and initialised\ninfo Engine mode : xboard\n")
        .await
        .unwrap();
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    let started = std::time::Instant::now();
    let writer = tokio::spawn(async move {
        let chunk = "info Time White: #5\n".repeat(1000);
        for _ in 0..1000 {
            if driver.write_all(chunk.as_bytes()).await.is_err() {
                break;
            }
        }
        driver
    });
    // Every line is delivered as a `Log` (and a `Clock`) or counted as
    // omitted; the two handshake lines were read before.
    let mut delivered = 0_u64;
    let mut omitted = 0_u64;
    let mut events_seen = 0_u64;
    while delivered + omitted < 1_000_000 {
        let event = next(&mut events).await;
        events_seen += 1;
        match event {
            EboardEvent::Note(Note::LinesOmitted(n)) => omitted += u64::from(n),
            EboardEvent::Log {
                direction: Direction::In,
                ..
            } => delivered += 1,
            _ => {}
        }
    }
    let windows = started.elapsed().as_secs() + 2;
    // Two events per delivered line (the console line and the clock) at most.
    assert!(
        events_seen <= 2 * 200 * windows + 10,
        "{events_seen} events"
    );
    assert!(omitted > 0);
    writer.await.unwrap();
}

#[tokio::test]
async fn a_driver_that_ignores_stop_and_quit_is_killed() {
    let kind = BoardKind::InputEngine {
        command: fake_binary(),
        port: "FAKE0".into(),
        params: "--linger".into(),
    };
    let (handle, mut events) = connect(kind).await.expect("starts");
    assert_eq!(next_significant(&mut events).await, EboardEvent::StartupFen);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);
    let closing = std::time::Instant::now();
    handle.close();
    loop {
        if let EboardEvent::Disconnected(reason) = next(&mut events).await {
            assert_eq!(reason, Reason::Requested);
            break;
        }
    }
    // It waited for the default shutdown time and then killed the process:
    // the session did not hang on a driver that never leaves.
    let waited = closing.elapsed();
    assert!(waited >= Duration::from_millis(900), "{waited:?}");
    assert!(waited < Duration::from_secs(8), "{waited:?}");
    assert!(events.next().await.is_none());
}
