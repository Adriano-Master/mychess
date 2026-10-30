//! The running session of the board and the effects that drive it (spec
//! M8-10, D2726, D2727).

use std::fmt;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mychess_eboard::{
    BoardKind, Eboard, EboardCommand, EboardEvent, EboardEvents as SessionEvents, Reason,
};
use tokio::sync::mpsc;

use super::message::EboardMessage;
use crate::message::Message;

/// How many events wait for the front-end before the forwarding task waits
/// too: a slow consumer slows the reading, it does not grow the memory.
const EVENT_QUEUE: usize = 256;

/// Something for the board to do.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EboardEffect {
    /// Opens the port or starts the driver and installs the session in the
    /// hub; answers [`EboardMessage::Connected`] or
    /// [`EboardMessage::ConnectFailed`].
    Connect {
        /// The epoch of the attempt.
        epoch: u64,
        /// The board.
        kind: BoardKind,
        /// The hub that keeps the session.
        hub: EboardHub,
    },
    /// Sends a command to the session of the hub (never blocks).
    Send {
        /// The command.
        command: EboardCommand,
        /// The hub.
        hub: EboardHub,
    },
    /// Ends the session of that epoch, if it is still the current one.
    Disconnect {
        /// The epoch to end.
        epoch: u64,
        /// The hub.
        hub: EboardHub,
    },
    /// Lists the serial ports; answers [`EboardMessage::PortsListed`].
    ListPorts,
}

struct Inner {
    tx: mpsc::Sender<Message>,
    rx: Mutex<Option<mpsc::Receiver<Message>>>,
    current: Mutex<Option<(u64, Eboard)>>,
}

/// The session of the board and the channel of its events. Cheap to clone;
/// clones share everything.
#[derive(Clone)]
pub struct EboardHub {
    inner: Arc<Inner>,
}

impl fmt::Debug for EboardHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EboardHub").finish_non_exhaustive()
    }
}

impl PartialEq for EboardHub {
    fn eq(&self, other: &EboardHub) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for EboardHub {}

impl Hash for EboardHub {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

impl Default for EboardHub {
    fn default() -> EboardHub {
        EboardHub::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The receiver of the events of the hub, taken once by the front-end.
#[derive(Debug)]
pub struct EboardEvents {
    rx: mpsc::Receiver<Message>,
}

impl EboardEvents {
    /// The next message; `None` when the hub is gone.
    pub async fn next(&mut self) -> Option<Message> {
        self.rx.recv().await
    }

    /// A message already waiting, if any.
    pub fn try_next(&mut self) -> Option<Message> {
        self.rx.try_recv().ok()
    }
}

impl EboardHub {
    /// A hub with no session.
    #[must_use]
    pub fn new() -> EboardHub {
        let (tx, rx) = mpsc::channel(EVENT_QUEUE);
        EboardHub {
            inner: Arc::new(Inner {
                tx,
                rx: Mutex::new(Some(rx)),
                current: Mutex::new(None),
            }),
        }
    }

    async fn open(
        &self,
        kind: BoardKind,
    ) -> Result<(Eboard, SessionEvents), mychess_eboard::EboardError> {
        mychess_eboard::connect(kind).await
    }

    /// Takes the receiver of the events: the first call gets it.
    #[must_use]
    pub fn events(&self) -> Option<EboardEvents> {
        lock(&self.inner.rx).take().map(|rx| EboardEvents { rx })
    }

    /// Whether a session is installed.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        lock(&self.inner.current)
            .as_ref()
            .is_some_and(|(_, handle)| handle.is_open())
    }

    /// Installs a session as the current one (ending any other) and starts
    /// forwarding its events as messages of that epoch. A session of an
    /// epoch older than the installed one is a slow attempt that lost the
    /// race (a later connection is already there): it is closed and
    /// `false` comes back.
    pub fn install(&self, epoch: u64, handle: Eboard, events: SessionEvents) -> bool {
        {
            let mut current = lock(&self.inner.current);
            if current.as_ref().is_some_and(|(e, _)| *e > epoch) {
                drop(current);
                handle.close();
                return false;
            }
            if let Some((_, old)) = current.replace((epoch, handle)) {
                old.close();
            }
        }
        let tx = self.inner.tx.clone();
        tokio::spawn(forward(tx, epoch, events));
        true
    }

    /// Sends a command to the current session. `false` if there is none or
    /// its queue is full.
    pub fn send(&self, command: EboardCommand) -> bool {
        lock(&self.inner.current)
            .as_ref()
            .is_some_and(|(_, handle)| handle.send(command))
    }

    /// Ends the current session if it is the one of `epoch`.
    pub fn close(&self, epoch: u64) {
        let mut current = lock(&self.inner.current);
        if current.as_ref().is_some_and(|(e, _)| *e == epoch)
            && let Some((_, handle)) = current.take()
        {
            handle.close();
        }
    }
}

/// Where the events of a session come from (the seam of the tests).
trait Source: Send + 'static {
    fn next_event(&mut self) -> impl Future<Output = Option<EboardEvent>> + Send;
}

impl Source for SessionEvents {
    fn next_event(&mut self) -> impl Future<Output = Option<EboardEvent>> + Send {
        self.next()
    }
}

/// Forwards the events of a session as messages of `epoch` until the stream
/// ends. The last event of a session is a `Disconnected`, but the session
/// sends it with a deadline and may lose it (a receiver that stayed busy):
/// when the stream ends without one, a synthetic one goes out, so the state
/// is never left "connected" to a dead session.
async fn forward(tx: mpsc::Sender<Message>, epoch: u64, mut events: impl Source) {
    let mut disconnected = false;
    while let Some(event) = events.next_event().await {
        disconnected = matches!(event, EboardEvent::Disconnected(_));
        let message = Message::Eboard(EboardMessage::Event { epoch, event });
        if tx.send(message).await.is_err() {
            return;
        }
    }
    if !disconnected {
        let event = EboardEvent::Disconnected(Reason::DeviceLost("event stream ended".to_owned()));
        tx.send(Message::Eboard(EboardMessage::Event { epoch, event }))
            .await
            .ok();
    }
}

/// Runs one effect of the board (the `perform` of the module).
pub(crate) async fn perform(effect: EboardEffect) -> Message {
    match effect {
        EboardEffect::Connect { epoch, kind, hub } => match hub.open(kind).await {
            Ok((handle, events)) => {
                hub.install(epoch, handle, events);
                Message::Eboard(EboardMessage::Connected { epoch })
            }
            Err(error) => Message::Eboard(EboardMessage::ConnectFailed {
                epoch,
                error: error.to_string(),
            }),
        },
        EboardEffect::Send { command, hub } => {
            hub.send(command);
            Message::Eboard(EboardMessage::Ack)
        }
        EboardEffect::Disconnect { epoch, hub } => {
            hub.close(epoch);
            Message::Eboard(EboardMessage::Ack)
        }
        EboardEffect::ListPorts => {
            let listed = tokio::task::spawn_blocking(mychess_eboard::list_ports)
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            Message::Eboard(EboardMessage::PortsListed(listed))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Source for std::vec::IntoIter<EboardEvent> {
        fn next_event(&mut self) -> impl Future<Output = Option<EboardEvent>> + Send {
            std::future::ready(self.next())
        }
    }

    async fn run(events: Vec<EboardEvent>) -> Vec<EboardEvent> {
        let (tx, mut rx) = mpsc::channel(16);
        forward(tx, 7, events.into_iter()).await;
        let mut out = Vec::new();
        while let Some(Message::Eboard(EboardMessage::Event { epoch, event })) = rx.recv().await {
            assert_eq!(epoch, 7);
            out.push(event);
        }
        out
    }

    #[tokio::test]
    async fn a_stream_that_ends_without_disconnected_gets_a_synthetic_one() {
        let out = run(vec![EboardEvent::Ready]).await;
        assert_eq!(out.len(), 2);
        assert!(matches!(
            out.last(),
            Some(EboardEvent::Disconnected(Reason::DeviceLost(_)))
        ));
    }

    #[tokio::test]
    async fn a_stream_that_ends_with_disconnected_gets_no_second_one() {
        let out = run(vec![
            EboardEvent::Ready,
            EboardEvent::Disconnected(Reason::Requested),
        ])
        .await;
        assert_eq!(out.len(), 2);
        assert!(matches!(
            out.last(),
            Some(EboardEvent::Disconnected(Reason::Requested))
        ));
    }
}
