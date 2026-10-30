//! A real serial-port code path without hardware: a pseudo-terminal pair
//! (Unix only). The slave is opened by name with `open_serial`, like a
//! `/dev/ttyUSB0`; the master plays the board.

#![cfg(unix)]

use std::io::{Read, Write};
use std::time::Duration;

use mychess_eboard::{
    BoardKind, EboardCommand, EboardEvent, EboardEvents, NovagTimings, Reason, connect,
    open_serial, start_novag,
};
use serialport::{SerialPort, TTYPort};
use tokio::time::timeout;

async fn next_significant(events: &mut EboardEvents) -> EboardEvent {
    loop {
        let event = timeout(Duration::from_secs(10), events.next())
            .await
            .expect("an event in time")
            .expect("alive");
        if !matches!(event, EboardEvent::Log { .. }) {
            return event;
        }
    }
}

/// The master, the name of the slave and the slave itself, which the test
/// keeps open: closing every slave makes the master read EIO/EOF.
fn pair() -> (TTYPort, String, TTYPort) {
    let (master, slave) = TTYPort::pair().expect("a pty pair");
    let name = slave.name().expect("the slave has a name");
    (master, name, slave)
}

#[tokio::test]
async fn novag_talks_through_a_real_tty_device() {
    let (mut master, name, _slave) = pair();
    master
        .set_timeout(Duration::from_millis(50))
        .expect("timeout");
    let device = open_serial(&name).expect("open the slave");
    let timings = NovagTimings {
        init_gap: Duration::from_millis(10),
        ..NovagTimings::default()
    };
    let (handle, mut events) = start_novag(device, timings);
    assert_eq!(next_significant(&mut events).await, EboardEvent::Ready);

    // What the board received: the four start-up commands.
    let mut got = String::new();
    let mut buf = [0_u8; 128];
    for _ in 0..100 {
        if let Ok(n) = master.read(&mut buf) {
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        if got.contains("U ON") {
            break;
        }
    }
    assert!(got.contains("I\n") && got.contains("X ON"), "{got:?}");

    master.write_all(b"M 1 e2-e4\r\n").expect("write");
    master.flush().ok();
    let EboardEvent::Move { .. } = next_significant(&mut events).await else {
        panic!("a move");
    };
    assert!(handle.send(EboardCommand::Raw("hello".into())));
    handle.close();
    assert_eq!(
        next_significant(&mut events).await,
        EboardEvent::Disconnected(Reason::Requested)
    );
}

#[tokio::test]
async fn a_plain_file_is_not_a_serial_port() {
    let dir = std::env::temp_dir();
    let err = connect(BoardKind::Novag {
        port: dir.display().to_string(),
    })
    .await;
    assert!(err.is_err());
}
