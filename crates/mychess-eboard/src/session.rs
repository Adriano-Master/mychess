//! The handle of a running board session and the entry points that start
//! one (spec m8-10, D2707, D2721).
//!
//! A session is a task that owns the device (a serial port or a driver
//! process). The application talks to it through [`Eboard`] (commands) and
//! [`EboardEvents`] (events); nothing here blocks.

mod driver;
mod gate;
mod novag;

use tokio::sync::mpsc;

use crate::error::EboardError;
use crate::event::{BoardKind, EboardCommand, EboardEvent};
use crate::serial::{DeviceIo, open_serial, validate_port};

pub use driver::{DriverProcess, Timeouts};
pub use novag::NovagTimings;

/// Capacity of the queue of commands.
const COMMAND_QUEUE: usize = 64;

/// A handle to a session. Cloning shares the session; it ends when
/// [`EboardCommand::Close`] is sent or when every handle is dropped.
#[derive(Debug, Clone)]
pub struct Eboard {
    tx: mpsc::Sender<EboardCommand>,
}

/// The events of a session, in order. The last one is always
/// [`EboardEvent::Disconnected`].
#[derive(Debug)]
pub struct EboardEvents {
    rx: mpsc::Receiver<EboardEvent>,
}

impl Eboard {
    /// Queues a command without waiting. `false` when the queue is full or
    /// the session is over.
    pub fn send(&self, command: EboardCommand) -> bool {
        self.tx.try_send(command).is_ok()
    }

    /// Asks the session to end (the same as sending [`EboardCommand::Close`]).
    pub fn close(&self) -> bool {
        self.send(EboardCommand::Close)
    }

    /// Whether the session task is still there.
    pub fn is_open(&self) -> bool {
        !self.tx.is_closed()
    }
}

impl EboardEvents {
    /// The next event; `None` once the session is over and its last event
    /// was read.
    pub async fn next(&mut self) -> Option<EboardEvent> {
        self.rx.recv().await
    }

    /// An event already waiting, if any (for tests and polling).
    pub fn try_next(&mut self) -> Option<EboardEvent> {
        self.rx.try_recv().ok()
    }
}

pub(crate) struct Channels {
    pub(crate) commands: mpsc::Receiver<EboardCommand>,
    pub(crate) events: mpsc::Sender<EboardEvent>,
}

/// How long the end of a session waits for room in the event queue.
const LAST_EVENT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(1);

/// Sends an event while the session is ending: waits a little for room (the
/// application may be busy) but never for ever, so a stuck receiver cannot
/// keep the device or the driver process open.
pub(crate) async fn send_last(events: &mpsc::Sender<EboardEvent>, event: EboardEvent) {
    tokio::time::timeout(LAST_EVENT_DEADLINE, events.send(event))
        .await
        .ok();
}

pub(crate) fn channels() -> (Eboard, EboardEvents, Channels) {
    let (command_tx, commands) = mpsc::channel(COMMAND_QUEUE);
    let (events, event_rx) = mpsc::channel(crate::EVENT_QUEUE);
    (
        Eboard { tx: command_tx },
        EboardEvents { rx: event_rx },
        Channels { commands, events },
    )
}

/// Starts a session with a Novag board on an already open device. Must be
/// called inside a Tokio runtime.
pub fn start_novag(device: DeviceIo, timings: NovagTimings) -> (Eboard, EboardEvents) {
    let (handle, events, ch) = channels();
    tokio::spawn(novag::run(device, timings, ch));
    (handle, events)
}

/// Starts a session with a driver whose standard streams are `reader` and
/// `writer` (a pipe pair, or an in-memory pair in tests). Must be called
/// inside a Tokio runtime.
pub fn start_driver<R, W>(
    reader: R,
    writer: W,
    process: Option<DriverProcess>,
    timeouts: Timeouts,
) -> (Eboard, EboardEvents)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (handle, events, ch) = channels();
    tokio::spawn(driver::run(reader, writer, process, timeouts, ch));
    (handle, events)
}

/// Connects to a board: opens the port (off the executor) or starts the
/// driver, and starts the session. The first event of the session follows
/// soon after (`Ready` once the board answers, or `Disconnected`).
///
/// # Errors
///
/// [`EboardError`] when the port cannot be opened or the driver cannot be
/// started; nothing is left running then.
pub async fn connect(kind: BoardKind) -> Result<(Eboard, EboardEvents), EboardError> {
    match kind {
        BoardKind::Novag { port } => {
            validate_port(&port)?;
            let device = tokio::task::spawn_blocking(move || open_serial(&port))
                .await
                .map_err(|e| EboardError::OpenPort {
                    port: String::new(),
                    message: e.to_string(),
                })??;
            Ok(start_novag(device, NovagTimings::default()))
        }
        BoardKind::InputEngine {
            command,
            port,
            params,
        } => {
            validate_port(&port)?;
            let (reader, writer, process) = driver::spawn_process(&command, &port, &params)?;
            Ok(start_driver(
                reader,
                writer,
                Some(process),
                Timeouts::default(),
            ))
        }
    }
}
