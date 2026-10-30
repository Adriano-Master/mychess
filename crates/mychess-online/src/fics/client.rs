//! The connection to a FICS server (spec m8-9, D2601-D2604, D2609-D2611).
//!
//! [`connect`] starts a task that owns the TCP socket: it logs in (guest or
//! registered), sends the initial `iset`/`set` commands, frames and parses
//! what the server says ([`FicsMessage`]), writes the [`Command`]s it is
//! given and pings the server to keep the session alive and measure the
//! latency. The caller gets a [`FicsSender`] and a [`FicsEvents`]; dropping
//! the latter closes the connection.
//!
//! The protocol is plain text over TCP, **without encryption**: the
//! password of a registered account crosses the network in clear (D2602).
//! The password is kept in a [`Password`], written to the socket and
//! nowhere else: not in `Debug`, logs, errors, messages or disk.

use std::fmt;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::command::Command;
use super::event::{FicsEvent, LoginFailure, parse_line};
use super::framer::LineFramer;

/// The public server.
pub const DEFAULT_HOST: &str = "freechess.org";
/// The plain port of FICS.
pub const DEFAULT_PORT: u16 = 5000;

/// Longest password accepted.
const MAX_PASSWORD: usize = 64;

/// Why a connection could not even start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FicsError {
    /// Called outside a Tokio runtime.
    #[error("no async runtime is running")]
    NoRuntime,
    /// The host or the account name is not acceptable.
    #[error("invalid connection settings")]
    InvalidConfig,
    /// The password is empty, too long or has control characters.
    #[error("invalid password")]
    InvalidPassword,
    /// The command queue is full.
    #[error("the command queue is full")]
    QueueFull,
    /// The connection is closed.
    #[error("the connection is closed")]
    Closed,
}

/// A password. It has no `Clone`, a redacted `Debug` and no `Display`.
pub struct Password(String);

impl Password {
    /// Checks the text.
    ///
    /// # Errors
    /// [`FicsError::InvalidPassword`] when empty, longer than 64
    /// characters or with a control character (it could carry a second
    /// command).
    pub fn new(text: &str) -> Result<Password, FicsError> {
        if text.is_empty()
            || text.chars().count() > MAX_PASSWORD
            || text.chars().any(char::is_control)
        {
            return Err(FicsError::InvalidPassword);
        }
        Ok(Password(text.to_owned()))
    }
}

impl Drop for Password {
    /// Overwrites the password before the memory goes back to the allocator
    /// (best effort, like the token of the Lichess client).
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

/// How to log in.
#[derive(Debug)]
#[non_exhaustive]
pub enum Login {
    /// A guest account: no password (D2601).
    Guest,
    /// A registered account.
    Registered {
        /// The handle.
        name: String,
        /// The password.
        password: Password,
    },
}

/// Settings of a connection.
#[derive(Debug)]
pub struct FicsConfig {
    /// The server.
    pub host: String,
    /// The port.
    pub port: u16,
    /// The account.
    pub login: Login,
    /// Time to open the TCP connection.
    pub connect_timeout: Duration,
    /// Time from the connection to the start of the session.
    pub login_timeout: Duration,
    /// Time between pings (D2610).
    pub ping_interval: Duration,
    /// No byte from the server for this long, after a ping, is a dead
    /// connection.
    pub silence_timeout: Duration,
    /// Time to write one line.
    pub write_timeout: Duration,
    /// Commands sent after the mandatory ones (the user's own).
    pub init_commands: Vec<Command>,
}

impl FicsConfig {
    /// Settings for `host:port` with the standard timeouts.
    #[must_use]
    pub fn new(host: &str, port: u16, login: Login) -> FicsConfig {
        FicsConfig {
            host: host.to_owned(),
            port,
            login,
            connect_timeout: Duration::from_secs(15),
            login_timeout: Duration::from_secs(60),
            ping_interval: Duration::from_secs(60),
            silence_timeout: Duration::from_secs(200),
            write_timeout: Duration::from_secs(10),
            init_commands: Vec::new(),
        }
    }

    fn valid(&self) -> bool {
        let host_ok = !self.host.is_empty()
            && self.host.len() <= 253
            && self
                .host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':');
        let login_ok = match &self.login {
            Login::Guest => true,
            Login::Registered { name, .. } => {
                (2..=17).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric())
            }
        };
        host_ok && login_ok && self.port != 0
    }
}

/// Why the connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Disconnect {
    /// The server closed the connection (or the user quit).
    ServerClosed,
    /// The owner dropped the handle or the command channel.
    Local,
    /// The TCP connection could not be opened.
    ConnectFailed,
    /// The login or the silence took too long.
    Timeout,
    /// A socket error.
    Io,
    /// The server refused the login.
    LoginFailed(LoginFailure),
}

/// What the connection reports.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FicsMessage {
    /// The TCP connection is open; the login follows.
    Connected,
    /// The session started.
    LoggedIn {
        /// The handle the server gave (the guest name for a guest).
        name: String,
    },
    /// A line of the server, parsed.
    Event(FicsEvent),
    /// The round trip of a `ping` (D2610).
    Latency(Duration),
    /// The connection ended; nothing follows.
    Disconnected(Disconnect),
}

/// Writes commands to the connection. Cheap to clone.
#[derive(Debug, Clone)]
pub struct FicsSender {
    tx: mpsc::Sender<Command>,
}

impl FicsSender {
    /// Queues a command; it never waits.
    ///
    /// # Errors
    /// [`FicsError::QueueFull`] or [`FicsError::Closed`].
    pub fn send(&self, command: Command) -> Result<(), FicsError> {
        self.tx.try_send(command).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => FicsError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => FicsError::Closed,
        })
    }
}

/// The messages of a connection. Dropping it closes the connection.
#[derive(Debug)]
pub struct FicsEvents {
    rx: mpsc::Receiver<FicsMessage>,
    task: JoinHandle<()>,
}

impl FicsEvents {
    /// The next message; `None` after [`FicsMessage::Disconnected`].
    pub async fn next(&mut self) -> Option<FicsMessage> {
        self.rx.recv().await
    }
}

impl Drop for FicsEvents {
    fn drop(&mut self) {
        // The task owns the socket: aborting it closes the connection.
        self.task.abort();
    }
}

/// How many messages wait for the consumer, and how many commands wait for
/// the socket.
const QUEUE: usize = 256;

/// Starts a connection. Needs a Tokio runtime.
///
/// # Errors
/// [`FicsError::NoRuntime`] or [`FicsError::InvalidConfig`].
pub fn connect(config: FicsConfig) -> Result<(FicsSender, FicsEvents), FicsError> {
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| FicsError::NoRuntime)?;
    if !config.valid() {
        return Err(FicsError::InvalidConfig);
    }
    let (event_tx, event_rx) = mpsc::channel(QUEUE);
    let (cmd_tx, cmd_rx) = mpsc::channel(QUEUE);
    let task = runtime.spawn(async move {
        let end = session(&config, &event_tx, cmd_rx).await;
        event_tx.send(FicsMessage::Disconnected(end)).await.ok();
    });
    Ok((FicsSender { tx: cmd_tx }, FicsEvents { rx: event_rx, task }))
}

/// Commands every session starts with (D2609).
fn mandatory_commands() -> Vec<Command> {
    [
        "iset seekremove 1",
        "iset seekinfo 1",
        "style 12",
        "iset nowrap 1",
        "iset nohighlight 1",
        "set interface MyChess",
        "set availinfo off",
    ]
    .iter()
    .filter_map(|c| Command::raw(c).ok())
    .collect()
}

async fn write_text(
    wr: &mut OwnedWriteHalf,
    text: &str,
    limit: Duration,
) -> Result<(), Disconnect> {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(b'\n');
    match tokio::time::timeout(limit, async {
        wr.write_all(&bytes).await?;
        wr.flush().await
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(Disconnect::Io),
        Err(_) => Err(Disconnect::Timeout),
    }
}

#[derive(Default)]
struct LoginState {
    sent_name: bool,
    sent_password: bool,
    sent_return: bool,
    logged_in: bool,
}

async fn session(
    config: &FicsConfig,
    events: &mpsc::Sender<FicsMessage>,
    mut commands: mpsc::Receiver<Command>,
) -> Disconnect {
    let address = (config.host.as_str(), config.port);
    let stream =
        match tokio::time::timeout(config.connect_timeout, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(_)) => return Disconnect::ConnectFailed,
            Err(_) => return Disconnect::Timeout,
        };
    // A move must not wait for Nagle.
    stream.set_nodelay(true).ok();
    let (mut rd, mut wr) = stream.into_split();
    if events.send(FicsMessage::Connected).await.is_err() {
        return Disconnect::Local;
    }

    let mut framer = LineFramer::new();
    let mut st = LoginState::default();
    let mut buf = vec![0u8; 8192];
    let login_deadline = tokio::time::Instant::now() + config.login_timeout;
    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + config.ping_interval,
        config.ping_interval.max(Duration::from_millis(1)),
    );
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_rx = Instant::now();
    let mut ping_sent: Option<Instant> = None;

    loop {
        tokio::select! {
            read = rd.read(&mut buf) => {
                let n = match read {
                    Ok(0) => return Disconnect::ServerClosed,
                    Ok(n) => n,
                    Err(_) => return Disconnect::Io,
                };
                last_rx = Instant::now();
                for line in framer.push(&buf[..n]) {
                    if let Some(end) =
                        on_line(config, events, &mut wr, &mut st, &mut ping_sent, &line).await
                    {
                        return end;
                    }
                }
                if !st.logged_in
                    && let Some(end) = on_prompt(config, events, &mut wr, &mut st, &mut framer).await
                {
                    return end;
                }
            }
            command = commands.recv(), if st.logged_in => {
                let Some(command) = command else { return Disconnect::Local };
                if let Err(end) = write_text(&mut wr, command.line(), config.write_timeout).await {
                    return end;
                }
            }
            _ = ping.tick(), if st.logged_in => {
                if last_rx.elapsed() > config.silence_timeout {
                    return Disconnect::Timeout;
                }
                ping_sent = Some(Instant::now());
                if let Err(end) =
                    write_text(&mut wr, Command::ping().line(), config.write_timeout).await
                {
                    return end;
                }
            }
            () = tokio::time::sleep_until(login_deadline), if !st.logged_in => {
                return Disconnect::Timeout;
            }
        }
    }
}

/// Answers `login:`, `password:` and `Press return...` (they have no line
/// end).
async fn on_prompt(
    config: &FicsConfig,
    events: &mpsc::Sender<FicsMessage>,
    wr: &mut OwnedWriteHalf,
    st: &mut LoginState,
    framer: &mut LineFramer,
) -> Option<Disconnect> {
    let partial = framer.partial();
    let text = partial.trim_end().to_ascii_lowercase();
    if text.ends_with("login:") && !st.sent_name {
        let name = match &config.login {
            Login::Guest => "guest",
            Login::Registered { name, .. } => name.as_str(),
        };
        st.sent_name = true;
        framer.clear_partial();
        return write_text(wr, name, config.write_timeout).await.err();
    }
    if text.ends_with("password:") {
        framer.clear_partial();
        let Login::Registered { password, .. } = &config.login else {
            // A guest is never asked for a password.
            return Some(Disconnect::LoginFailed(LoginFailure::Refused));
        };
        if st.sent_password {
            return Some(Disconnect::LoginFailed(LoginFailure::InvalidPassword));
        }
        st.sent_password = true;
        return write_text(wr, &password.0, config.write_timeout)
            .await
            .err();
    }
    if text.contains("press return to enter the server as") && !st.sent_return {
        st.sent_return = true;
        let event = parse_line(&partial);
        framer.clear_partial();
        if events.send(FicsMessage::Event(event)).await.is_err() {
            return Some(Disconnect::Local);
        }
        return write_text(wr, "", config.write_timeout).await.err();
    }
    None
}

async fn on_line(
    config: &FicsConfig,
    events: &mpsc::Sender<FicsMessage>,
    wr: &mut OwnedWriteHalf,
    st: &mut LoginState,
    ping_sent: &mut Option<Instant>,
    line: &str,
) -> Option<Disconnect> {
    let event = parse_line(line);
    let mut end = None;
    match &event {
        FicsEvent::SessionStart { name } if !st.logged_in => {
            st.logged_in = true;
            let started = FicsMessage::LoggedIn { name: name.clone() };
            if events.send(started).await.is_err() {
                return Some(Disconnect::Local);
            }
            for command in mandatory_commands().iter().chain(&config.init_commands) {
                if let Err(e) = write_text(wr, command.line(), config.write_timeout).await {
                    return Some(e);
                }
            }
            return None;
        }
        FicsEvent::LoginFailed(failure) if !st.logged_in => {
            end = Some(Disconnect::LoginFailed(*failure));
        }
        FicsEvent::Ping { .. } => {
            if let Some(sent) = ping_sent.take()
                && events
                    .send(FicsMessage::Latency(sent.elapsed()))
                    .await
                    .is_err()
            {
                return Some(Disconnect::Local);
            }
        }
        FicsEvent::GuestName(_) if !st.sent_return => {
            st.sent_return = true;
            if let Err(e) = write_text(wr, "", config.write_timeout).await {
                return Some(e);
            }
        }
        _ => {}
    }
    if events.send(FicsMessage::Event(event)).await.is_err() {
        return Some(Disconnect::Local);
    }
    end
}
