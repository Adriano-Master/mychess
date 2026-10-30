//! Streams of the Board API (spec m8-3, D1309, D1310, D1315, D1316, D1333).
//!
//! Every stream is a Tokio task that owns the HTTP connection and sends
//! [`StreamMessage`]s through a bounded channel; the consumer (the app) never
//! touches the network. The task:
//!
//! * frames the body with [`LineFramer`] and decodes each line, tolerating bad
//!   ones;
//! * watches for silence (no byte for `silence`, 20 s by default: the server
//!   sends a keep-alive line every 7 s) and treats it as a dead connection;
//! * reconnects with [`Backoff`] after a drop, honoring the 429 block of the
//!   gate (except the urgent game stream, which only waits for its own 429); a reconnected game stream starts with a fresh `gameFull`, which the
//!   app uses to resynchronize everything;
//! * stops, closing the connection, when the handle is dropped.

use std::time::Duration;

use reqwest::{Method, RequestBuilder};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    backoff::Backoff,
    client::Client,
    error::{Error, Result},
    models::{ChallengeUpdate, Event, GameEvent, WatchEvent},
    ndjson::{Line, LineFramer, MAX_LINE_LEN},
};

/// Silence limit used by default (the keep-alive comes every 7 s).
pub const DEFAULT_SILENCE: Duration = Duration::from_secs(20);

/// Stream behavior shared by the client.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Time without any byte after which the connection is considered dead;
    /// `None` disables the watch.
    pub silence: Option<Duration>,
    /// Unit of the reconnection delays (1 s: 1, 2, 4, 8, 16, 30 s).
    pub backoff_unit: Duration,
    /// Consecutive failed attempts of the event stream before giving up
    /// (outside a game, 5); `None` retries forever.
    pub event_max_reconnects: Option<u32>,
    /// The same for the game stream (`None`: forever, the game is on the clock).
    pub game_max_reconnects: Option<u32>,
    /// Longest accepted line.
    pub max_line_len: usize,
    /// Wait for the response headers of a seek or challenge stream (30 s).
    pub header_timeout: Duration,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            silence: Some(DEFAULT_SILENCE),
            backoff_unit: Duration::from_secs(1),
            event_max_reconnects: Some(5),
            game_max_reconnects: None,
            max_line_len: MAX_LINE_LEN,
            header_timeout: Duration::from_secs(30),
        }
    }
}

/// What a stream tells its consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamMessage<T> {
    /// The connection is established (first connection or reconnection).
    Connected,
    /// A decoded line.
    Item(T),
    /// The connection was lost; a new attempt starts after `delay`.
    Reconnecting {
        /// Consecutive failed attempts so far.
        attempt: u32,
        /// Wait before the next attempt.
        delay: Duration,
        /// Why the connection was lost.
        cause: Error,
    },
    /// The stream ended normally (game over, challenge done, server closed a
    /// stream that does not reconnect). No more messages follow.
    Ended,
    /// The stream ended with an error. No more messages follow.
    Failed(Error),
}

/// Receiving end of a stream task. Dropping it closes the connection.
#[derive(Debug)]
pub struct StreamHandle<T> {
    rx: mpsc::Receiver<StreamMessage<T>>,
    task: JoinHandle<()>,
}

impl<T> StreamHandle<T> {
    /// Next message; `None` after [`StreamMessage::Ended`] or
    /// [`StreamMessage::Failed`] was delivered and the task is gone.
    pub async fn next(&mut self) -> Option<StreamMessage<T>> {
        self.rx.recv().await
    }

    /// Next message if one is already queued.
    pub fn try_next(&mut self) -> Option<StreamMessage<T>> {
        self.rx.try_recv().ok()
    }

    /// Closes the connection and stops the task.
    pub fn close(self) {
        drop(self);
    }
}

impl<T> Drop for StreamHandle<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Account event stream (`/api/stream/event`).
pub type EventStream = StreamHandle<Event>;
/// Game stream (`/api/board/game/stream/{id}`).
pub type GameStream = StreamHandle<GameEvent>;
/// Stream of an observed game (`/api/stream/game/{id}`, no authentication).
pub type WatchStream = StreamHandle<WatchEvent>;
/// Stream of a challenge sent with `keepAliveStream`; dropping it (or
/// [`StreamHandle::close`]) cancels the wait by closing the connection.
pub type ChallengeHandle = StreamHandle<ChallengeUpdate>;

struct Spec<T> {
    client: Client,
    request: Box<dyn Fn() -> RequestBuilder + Send + Sync>,
    parse: fn(&str) -> T,
    unparsed: fn(String) -> T,
    is_terminal: fn(&T) -> bool,
    silence: Option<Duration>,
    /// Wait for the response headers.
    header_timeout: Option<Duration>,
    /// Obey the gate's 429 block (false for the urgent game stream, which
    /// only waits for a 429 of its own).
    respect_block: bool,
    backoff_unit: Duration,
    max_reconnects: Option<u32>,
    reconnect: bool,
    max_line_len: usize,
}

enum Pump {
    Terminal,
    ReceiverGone,
    Eof,
    Lost(Error),
    Fatal(Error),
}

fn spawn_task<F>(fut: F) -> Result<JoinHandle<()>>
where
    F: Future<Output = ()> + Send + 'static,
{
    let handle = tokio::runtime::Handle::try_current().map_err(|_| Error::Closed)?;
    Ok(handle.spawn(fut))
}

fn spawn_stream<T: Send + 'static>(spec: Spec<T>) -> Result<StreamHandle<T>> {
    let (tx, rx) = mpsc::channel(64);
    let task = spawn_task(run(spec, tx))?;
    Ok(StreamHandle { rx, task })
}

pub(crate) fn spawn_event_stream(client: &Client) -> Result<EventStream> {
    let url = client.url(&["api", "stream", "event"])?;
    let cfg = client.inner.streams.clone();
    let c = client.clone();
    spawn_stream(Spec {
        client: client.clone(),
        request: Box::new(move || c.request(Method::GET, url.clone())),
        parse: Event::parse_line,
        unparsed: |line| Event::Unparsed { line },
        is_terminal: |_| false,
        silence: cfg.silence,
        header_timeout: cfg.silence,
        respect_block: true,
        backoff_unit: cfg.backoff_unit,
        max_reconnects: cfg.event_max_reconnects,
        reconnect: true,
        max_line_len: cfg.max_line_len,
    })
}

pub(crate) fn spawn_game_stream(client: &Client, game_id: &str) -> Result<GameStream> {
    let url = client.url(&["api", "board", "game", "stream", game_id])?;
    let cfg = client.inner.streams.clone();
    let c = client.clone();
    spawn_stream(Spec {
        client: client.clone(),
        request: Box::new(move || c.request(Method::GET, url.clone())),
        parse: GameEvent::parse_line,
        unparsed: |line| GameEvent::Unparsed { line },
        is_terminal: |e| match e {
            GameEvent::Full(full) => full.state.is_final(),
            GameEvent::State(state) => state.is_final(),
            _ => false,
        },
        silence: cfg.silence,
        header_timeout: cfg.silence,
        respect_block: false,
        backoff_unit: cfg.backoff_unit,
        max_reconnects: cfg.game_max_reconnects,
        reconnect: true,
        max_line_len: cfg.max_line_len,
    })
}

/// Opens `/api/stream/game/{id}` **without the token** (D3000): observing is
/// public, and the delayed feed must not be tied to the account.
pub(crate) fn spawn_watch_stream(client: &Client, game_id: &str) -> Result<WatchStream> {
    let url = client.url(&["api", "stream", "game", game_id])?;
    let cfg = client.inner.streams.clone();
    let c = client.clone();
    spawn_stream(Spec {
        client: client.clone(),
        request: Box::new(move || c.inner.http.request(Method::GET, url.clone())),
        parse: WatchEvent::parse_line,
        unparsed: |line| WatchEvent::Unparsed { line },
        is_terminal: |e| matches!(e, WatchEvent::Description(g) if g.is_final()),
        silence: cfg.silence,
        header_timeout: cfg.silence,
        respect_block: true,
        backoff_unit: cfg.backoff_unit,
        max_reconnects: cfg.event_max_reconnects,
        reconnect: true,
        max_line_len: cfg.max_line_len,
    })
}

pub(crate) fn spawn_challenge(
    client: &Client,
    username: &str,
    fields: Vec<(&'static str, String)>,
) -> Result<ChallengeHandle> {
    let url = client.url(&["api", "challenge", username])?;
    let cfg = client.inner.streams.clone();
    let c = client.clone();
    // A challenge is created by the POST: re-sending it would duplicate it,
    // so this stream never reconnects.
    spawn_stream(Spec {
        client: client.clone(),
        request: Box::new(move || c.request(Method::POST, url.clone()).form(&fields)),
        parse: ChallengeUpdate::parse_line,
        unparsed: |line| ChallengeUpdate::Unparsed { line },
        is_terminal: |u| matches!(u, ChallengeUpdate::Done(_)),
        silence: None,
        header_timeout: Some(cfg.header_timeout),
        respect_block: true,
        backoff_unit: cfg.backoff_unit,
        max_reconnects: Some(0),
        reconnect: false,
        max_line_len: cfg.max_line_len,
    })
}

/// How a seek connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeekOutcome {
    /// The server closed the connection: normally the seek was paired and the
    /// `gameStart` arrives through the event stream (D1315).
    Closed,
    /// The request failed or the connection broke.
    Failed(Error),
}

/// A real-time seek. The connection stays open while the seek is active;
/// dropping the handle or calling [`SeekHandle::cancel`] closes it, which
/// cancels the seek at Lichess.
#[derive(Debug)]
pub struct SeekHandle {
    rx: Option<oneshot::Receiver<SeekOutcome>>,
    task: JoinHandle<()>,
}

impl SeekHandle {
    /// Waits for the end of the connection.
    pub async fn outcome(&mut self) -> SeekOutcome {
        match self.rx.take() {
            Some(rx) => rx.await.unwrap_or(SeekOutcome::Failed(Error::Closed)),
            None => SeekOutcome::Failed(Error::Closed),
        }
    }

    /// Cancels the seek by closing the connection.
    pub fn cancel(self) {
        drop(self);
    }

    /// Whether the connection task already finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

impl Drop for SeekHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) fn spawn_seek(
    client: &Client,
    fields: &[(&'static str, String)],
    header_timeout: Duration,
) -> Result<SeekHandle> {
    let url = client.url(&["api", "board", "seek"])?;
    let rb = client.request(Method::POST, url).form(fields);
    let c = client.clone();
    let (tx, rx) = oneshot::channel();
    let task = spawn_task(async move {
        let outcome = match c.open_stream(rb, Some(header_timeout), true).await {
            Err(e) => SeekOutcome::Failed(e),
            Ok(mut resp) => loop {
                match resp.chunk().await {
                    Ok(Some(_)) => {}
                    Ok(None) => break SeekOutcome::Closed,
                    Err(e) => break SeekOutcome::Failed(crate::client::map_reqwest(&e)),
                }
            },
        };
        tx.send(outcome).ok();
    })?;
    Ok(SeekHandle { rx: Some(rx), task })
}

/// Sleeps unless the receiver goes away first; `true` means "receiver gone".
async fn sleep_or_closed<T>(tx: &mpsc::Sender<StreamMessage<T>>, d: Duration) -> bool {
    tokio::select! {
        () = tokio::time::sleep(d) => false,
        () = tx.closed() => true,
    }
}

async fn run<T: Send + 'static>(spec: Spec<T>, tx: mpsc::Sender<StreamMessage<T>>) {
    let mut backoff = Backoff::from_clock(spec.backoff_unit);
    let mut failures: u32 = 0;
    loop {
        // Honor a 429 block before (re)connecting (not for the urgent game
        // stream: a block caused by other requests must not delay it).
        if let Some(wait) = spec
            .client
            .gate()
            .blocked_for()
            .filter(|_| spec.respect_block)
        {
            let msg = StreamMessage::Reconnecting {
                attempt: failures,
                delay: wait,
                cause: Error::RateLimited { retry_after: wait },
            };
            if tx.send(msg).await.is_err() || sleep_or_closed(&tx, wait).await {
                return;
            }
        }
        let cause = match spec
            .client
            .open_stream((spec.request)(), spec.header_timeout, spec.respect_block)
            .await
        {
            Err(e) if !e.is_transient() => {
                tx.send(StreamMessage::Failed(e)).await.ok();
                return;
            }
            Err(e) => e,
            Ok(resp) => {
                if tx.send(StreamMessage::Connected).await.is_err() {
                    return;
                }
                let mut got_data = false;
                match pump(&spec, resp, &tx, &mut got_data).await {
                    Pump::Terminal => {
                        tx.send(StreamMessage::Ended).await.ok();
                        return;
                    }
                    Pump::ReceiverGone => return,
                    Pump::Fatal(e) => {
                        tx.send(StreamMessage::Failed(e)).await.ok();
                        return;
                    }
                    Pump::Eof if !spec.reconnect => {
                        tx.send(StreamMessage::Ended).await.ok();
                        return;
                    }
                    Pump::Eof => {
                        if got_data {
                            failures = 0;
                            backoff.reset();
                        }
                        Error::Network("the server closed the stream".to_owned())
                    }
                    Pump::Lost(e) => {
                        if got_data {
                            failures = 0;
                            backoff.reset();
                        }
                        e
                    }
                }
            }
        };
        failures += 1;
        if !spec.reconnect || spec.max_reconnects.is_some_and(|max| failures > max) {
            tx.send(StreamMessage::Failed(cause)).await.ok();
            return;
        }
        let block = if spec.respect_block {
            spec.client.gate().blocked_for().unwrap_or_default()
        } else if let Error::RateLimited { retry_after } = &cause {
            // The stream's own 429: wait it out.
            *retry_after
        } else {
            Duration::ZERO
        };
        let delay = backoff.next_delay().max(block);
        tracing::debug!(attempt = failures, ?delay, "stream lost; reconnecting");
        let msg = StreamMessage::Reconnecting {
            attempt: failures,
            delay,
            cause,
        };
        if tx.send(msg).await.is_err() || sleep_or_closed(&tx, delay).await {
            return;
        }
    }
}

async fn pump<T: Send + 'static>(
    spec: &Spec<T>,
    mut resp: reqwest::Response,
    tx: &mpsc::Sender<StreamMessage<T>>,
    got_data: &mut bool,
) -> Pump {
    let mut framer = LineFramer::with_max_len(spec.max_line_len);
    loop {
        let read = async {
            match spec.silence {
                Some(limit) => match tokio::time::timeout(limit, resp.chunk()).await {
                    Ok(r) => r.map_err(|e| crate::client::map_reqwest(&e)),
                    Err(_) => Err(Error::Timeout),
                },
                None => resp
                    .chunk()
                    .await
                    .map_err(|e| crate::client::map_reqwest(&e)),
            }
        };
        let chunk = tokio::select! {
            () = tx.closed() => return Pump::ReceiverGone,
            r = read => r,
        };
        let lines = match chunk {
            Err(e) => return Pump::Lost(e),
            Ok(None) => {
                let tail: Vec<Line> = framer.finish_stream().into_iter().collect();
                return match deliver(spec, tx, tail).await {
                    Some(end) => end,
                    None => Pump::Eof,
                };
            }
            Ok(Some(bytes)) => {
                *got_data = true;
                match framer.push(&bytes) {
                    Ok(lines) => lines,
                    Err(e) => return Pump::Fatal(e),
                }
            }
        };
        if let Some(end) = deliver(spec, tx, lines).await {
            return end;
        }
    }
}

/// Decodes and sends lines. `Some` means the pump must stop.
async fn deliver<T: Send + 'static>(
    spec: &Spec<T>,
    tx: &mpsc::Sender<StreamMessage<T>>,
    lines: Vec<Line>,
) -> Option<Pump> {
    for line in lines {
        let item = match line {
            Line::KeepAlive => continue,
            Line::Text(text) => (spec.parse)(&text),
            Line::Invalid(text) => (spec.unparsed)(text.chars().take(200).collect()),
        };
        let terminal = (spec.is_terminal)(&item);
        if tx.send(StreamMessage::Item(item)).await.is_err() {
            return Some(Pump::ReceiverGone);
        }
        if terminal {
            return Some(Pump::Terminal);
        }
    }
    None
}
