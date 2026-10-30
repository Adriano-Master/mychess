//! The hub that talks to the FICS server (spec m8-9, D2602, D2611).
//!
//! [`FicsCommand`]s are data (an [`Effect`](crate::Effect) carries one);
//! [`perform_fics`] runs them against a [`FicsHub`], which owns the
//! connection, the task that reads it and the staged password. The password
//! is handed to [`FicsHub::stage_password`] once and stays inside the hub:
//! never in a command, a message, a state, a log or a `Debug`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use mychess_online::fics::{
    Command, Disconnect, FicsConfig, FicsError, FicsMessage as Net, FicsSender, Login, Password,
    connect,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::message::{FicsCommand, FicsMessage, NetUpdate};

/// How many messages wait for the front-end before the task that reads the
/// connection waits too.
const EVENT_QUEUE: usize = 256;

struct Inner {
    tx: mpsc::Sender<FicsMessage>,
    rx: Mutex<Option<mpsc::Receiver<FicsMessage>>>,
    staged: Mutex<Option<Password>>,
    sender: Mutex<Option<FicsSender>>,
    task: Mutex<Option<JoinHandle<()>>>,
    /// Where to connect instead of the settings (tests).
    override_port: Mutex<Option<u16>>,
    /// The epoch of the last connection asked for.
    epoch: Mutex<u64>,
    /// The newest epoch whose disconnect came before its connect.
    cancelled: Mutex<u64>,
}

/// The connection and the staged password. Cheap to clone; clones share
/// everything.
#[derive(Clone)]
pub struct FicsHub {
    inner: Arc<Inner>,
}

impl fmt::Debug for FicsHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Nothing about the password or the connection is printed.
        f.debug_struct("FicsHub").finish_non_exhaustive()
    }
}

impl PartialEq for FicsHub {
    fn eq(&self, other: &FicsHub) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for FicsHub {}

impl Hash for FicsHub {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

impl Default for FicsHub {
    fn default() -> FicsHub {
        FicsHub::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The messages of the hub, in the order they happened. The front-end wraps
/// each in [`Message::Fics`](crate::Message::Fics).
#[derive(Debug)]
pub struct FicsEvents {
    rx: mpsc::Receiver<FicsMessage>,
}

impl FicsEvents {
    /// The next message; `None` when the hub is gone.
    pub async fn next(&mut self) -> Option<FicsMessage> {
        self.rx.recv().await
    }
}

impl FicsHub {
    /// A hub with no connection.
    #[must_use]
    pub fn new() -> FicsHub {
        let (tx, rx) = mpsc::channel(EVENT_QUEUE);
        FicsHub {
            inner: Arc::new(Inner {
                tx,
                rx: Mutex::new(Some(rx)),
                staged: Mutex::new(None),
                sender: Mutex::new(None),
                task: Mutex::new(None),
                override_port: Mutex::new(None),
                epoch: Mutex::new(0),
                cancelled: Mutex::new(0),
            }),
        }
    }

    /// Takes the receiver of the messages: the first call gets it.
    pub fn events(&self) -> Option<FicsEvents> {
        lock(&self.inner.rx).take().map(|rx| FicsEvents { rx })
    }

    /// Keeps the password for the next [`FicsCommand::Connect`] of a
    /// registered account. The text never leaves the hub.
    ///
    /// # Errors
    /// [`FicsError::InvalidPassword`] when the text is empty, too long or
    /// has control characters.
    pub fn stage_password(&self, text: &str) -> Result<(), FicsError> {
        *lock(&self.inner.staged) = Some(Password::new(text)?);
        Ok(())
    }

    /// Forgets a staged password.
    pub fn clear_staged(&self) {
        *lock(&self.inner.staged) = None;
    }

    /// Whether a password waits for the next connection.
    #[must_use]
    pub fn has_staged_password(&self) -> bool {
        lock(&self.inner.staged).is_some()
    }

    /// Connects to this port whatever the settings say (tests with the
    /// fake server).
    pub fn set_port_override(&self, port: Option<u16>) {
        *lock(&self.inner.override_port) = port;
    }

    fn close(&self) {
        if let Some(task) = lock(&self.inner.task).take() {
            // Dropping the events inside the task closes the socket.
            task.abort();
        }
        *lock(&self.inner.sender) = None;
    }

    async fn deliver(&self, message: FicsMessage) {
        // The receiver is gone when the program ends: nothing to tell.
        self.inner.tx.send(message).await.ok();
    }
}

fn failed(epoch: u64, end: Disconnect) -> FicsMessage {
    FicsMessage::Net(NetUpdate {
        epoch,
        at: Instant::now(),
        message: Net::Disconnected(end),
    })
}

/// Runs one command; the answer is a message (most are `None`: what happens
/// next arrives through [`FicsHub::events`]).
pub(crate) async fn perform_fics(command: FicsCommand, hub: &FicsHub) -> FicsMessage {
    match command {
        FicsCommand::Connect {
            epoch,
            host,
            port,
            name,
            init,
        } => {
            if *lock(&hub.inner.cancelled) >= epoch {
                // The user disconnected before this attempt started.
                return FicsMessage::Ack;
            }
            *lock(&hub.inner.epoch) = epoch;
            hub.close();
            let staged = lock(&hub.inner.staged).take();
            let login = match (name, staged) {
                (None, _) => Login::Guest,
                (Some(name), Some(password)) => Login::Registered { name, password },
                (Some(_), None) => {
                    return failed(
                        epoch,
                        Disconnect::LoginFailed(
                            mychess_online::fics::LoginFailure::InvalidPassword,
                        ),
                    );
                }
            };
            let port = lock(&hub.inner.override_port).unwrap_or(port);
            let mut config = FicsConfig::new(&host, port, login);
            config.init_commands = init;
            let Ok((sender, mut events)) = connect(config) else {
                return failed(epoch, Disconnect::ConnectFailed);
            };
            *lock(&hub.inner.sender) = Some(sender);
            let forward = hub.clone();
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                hub.close();
                return failed(epoch, Disconnect::ConnectFailed);
            };
            let task = runtime.spawn(async move {
                while let Some(message) = events.next().await {
                    let ended = matches!(message, Net::Disconnected(_));
                    forward
                        .deliver(FicsMessage::Net(NetUpdate {
                            epoch,
                            at: Instant::now(),
                            message,
                        }))
                        .await;
                    if ended {
                        break;
                    }
                }
            });
            *lock(&hub.inner.task) = Some(task);
            FicsMessage::Ack
        }
        FicsCommand::Send(command) => {
            let sender = lock(&hub.inner.sender).clone();
            let epoch = *lock(&hub.inner.epoch);
            let line = command.line().to_owned();
            match sender.map(|s| s.send(command)) {
                Some(Ok(())) => FicsMessage::Ack,
                _ => FicsMessage::SendFailed { epoch, line },
            }
        }
        FicsCommand::Disconnect { quit, epoch } => {
            // A newer connection (disconnect then connect at once) is not
            // the one this command is about (D2691).
            {
                let current = *lock(&hub.inner.epoch);
                if current < epoch {
                    *lock(&hub.inner.cancelled) = epoch;
                }
                if current != epoch {
                    return FicsMessage::Ack;
                }
            }
            let sender = lock(&hub.inner.sender).clone();
            if quit && let Some(sender) = sender {
                sender.send(Command::quit()).ok();
                // Let the line out before the socket closes.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            if *lock(&hub.inner.epoch) == epoch {
                hub.close();
            }
            FicsMessage::Ack
        }
    }
}
