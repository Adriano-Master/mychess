//! A fake Lichess for tests (spec m8-3, D1339). Behind the `testing` feature.
//!
//! [`FakeLichess`] is a tiny HTTP/1.1 server on `127.0.0.1:0` (no TLS) that
//! answers from a script: fixed JSON replies, chunked NDJSON streams made of
//! [`Step`]s (lines, raw fragments, delays, abrupt drops, holding the
//! connection open), live streams fed by the test, 429s and any status. It
//! records every request and the closing of every stream, so tests can check
//! that cancelling a seek really closes the connection. Nothing here touches
//! the real network.

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpListener,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

/// One step of a scripted stream.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Step {
    /// A line (a newline is appended) as one HTTP chunk.
    Line(String),
    /// Raw bytes as one HTTP chunk (to split a line across chunks).
    Raw(Vec<u8>),
    /// An empty line (the server keep-alive).
    KeepAlive,
    /// Pause before the next step.
    Delay(Duration),
    /// End the stream properly (terminating chunk).
    Close,
    /// Drop the connection without finishing the chunked body.
    Drop,
    /// Keep the connection open until the client closes it.
    Hold,
}

impl Step {
    /// Shorthand for [`Step::Line`].
    #[must_use]
    pub fn line(text: &str) -> Self {
        Step::Line(text.to_owned())
    }
}

/// Feeds a live stream from the test.
#[derive(Debug, Clone)]
pub struct LiveStream {
    tx: mpsc::UnboundedSender<Step>,
}

impl LiveStream {
    /// Sends any step.
    pub fn send(&self, step: Step) {
        self.tx.send(step).ok();
    }

    /// Sends a line.
    pub fn line(&self, text: &str) {
        self.send(Step::line(text));
    }

    /// Ends the stream properly.
    pub fn close(&self) {
        self.send(Step::Close);
    }

    /// Drops the connection abruptly.
    pub fn drop_connection(&self) {
        self.send(Step::Drop);
    }
}

type SharedRx = Arc<Mutex<Option<mpsc::UnboundedReceiver<Step>>>>;

#[derive(Clone)]
enum Body {
    Fixed(Vec<u8>),
    Steps(VecDeque<Step>),
    Live(SharedRx),
}

/// A scripted response.
#[derive(Clone)]
pub struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Body,
    head_delay: Duration,
}

impl Response {
    /// A fixed JSON reply.
    #[must_use]
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: Body::Fixed(body.as_bytes().to_vec()),
            head_delay: Duration::ZERO,
        }
    }

    /// A reply with an empty body (for example `429`).
    #[must_use]
    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Body::Fixed(Vec::new()),
            head_delay: Duration::ZERO,
        }
    }

    /// `200` with `{"ok":true}`.
    #[must_use]
    pub fn ok() -> Self {
        Self::json(200, r#"{"ok":true}"#)
    }

    /// A `200` chunked stream made of the given steps; without a final
    /// [`Step::Close`] the stream ends properly after the last step.
    #[must_use]
    pub fn stream(steps: Vec<Step>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/x-ndjson".to_owned())],
            body: Body::Steps(steps.into()),
            head_delay: Duration::ZERO,
        }
    }

    /// A `200` chunked stream fed by the test through the returned handle.
    #[must_use]
    pub fn live() -> (Self, LiveStream) {
        let (tx, rx) = mpsc::unbounded_channel();
        let resp = Self {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "application/x-ndjson".to_owned())],
            body: Body::Live(Arc::new(Mutex::new(Some(rx)))),
            head_delay: Duration::ZERO,
        };
        (resp, LiveStream { tx })
    }

    /// Waits before sending the status line and headers (to test header
    /// timeouts).
    #[must_use]
    pub fn head_delay(mut self, delay: Duration) -> Self {
        self.head_delay = delay;
        self
    }

    /// Adds a response header.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// A request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// Method (`GET`, `POST`).
    pub method: String,
    /// Path without the query string.
    pub path: String,
    /// Query string without `?` (may be empty).
    pub query: String,
    /// Headers with lowercase names.
    pub headers: Vec<(String, String)>,
    /// Body as text.
    pub body: String,
}

impl RecordedRequest {
    /// First header with that name (case-insensitive).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The `application/x-www-form-urlencoded` body decoded.
    #[must_use]
    pub fn form(&self) -> Vec<(String, String)> {
        decode_pairs(&self.body)
    }

    /// The query string decoded.
    #[must_use]
    pub fn query_pairs(&self) -> Vec<(String, String)> {
        decode_pairs(&self.query)
    }
}

fn decode_pairs(text: &str) -> Vec<(String, String)> {
    text.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Default)]
struct Route {
    queue: VecDeque<Response>,
    fallback: Option<Response>,
}

#[derive(Default)]
struct Shared {
    routes: Mutex<HashMap<String, Route>>,
    requests: Mutex<Vec<RecordedRequest>>,
    closed: Mutex<HashMap<String, usize>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn key(method: &str, path: &str) -> String {
    format!("{method} {path}")
}

/// The fake server. Dropping it stops the server and its connections.
pub struct FakeLichess {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
}

impl std::fmt::Debug for FakeLichess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeLichess")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl Drop for FakeLichess {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl FakeLichess {
    /// Starts the server on a free loopback port.
    ///
    /// # Errors
    /// The bind error.
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        let accept = tokio::spawn(async move {
            let mut conns = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((sock, _)) = accepted else { break };
                        conns.spawn(handle(sock, s.clone()));
                    }
                    Some(_) = conns.join_next(), if !conns.is_empty() => {}
                }
            }
        });
        Ok(Self {
            addr,
            shared,
            accept,
        })
    }

    /// `http://127.0.0.1:<port>`, for `ClientConfig::base_url`.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Queues a response for `method path`; consumed by one request each.
    pub fn enqueue(&self, method: &str, path: &str, response: Response) {
        lock(&self.shared.routes)
            .entry(key(method, path))
            .or_default()
            .queue
            .push_back(response);
    }

    /// Sets the response used when the queue of `method path` is empty.
    pub fn fallback(&self, method: &str, path: &str, response: Response) {
        lock(&self.shared.routes)
            .entry(key(method, path))
            .or_default()
            .fallback = Some(response);
    }

    /// Every request received so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        lock(&self.shared.requests).clone()
    }

    /// Requests for one route.
    #[must_use]
    pub fn requests_for(&self, method: &str, path: &str) -> Vec<RecordedRequest> {
        lock(&self.shared.requests)
            .iter()
            .filter(|r| r.method == method && r.path == path)
            .cloned()
            .collect()
    }

    /// How many stream connections of a route the client has closed.
    #[must_use]
    pub fn closed_count(&self, method: &str, path: &str) -> usize {
        lock(&self.shared.closed)
            .get(&key(method, path))
            .copied()
            .unwrap_or(0)
    }

    /// Waits (up to 5 s) until `method path` was requested `n` times.
    pub async fn wait_requests(&self, method: &str, path: &str, n: usize) -> bool {
        for _ in 0..1000 {
            if self.requests_for(method, path).len() >= n {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        false
    }

    /// Waits (up to 5 s) until the client closed `n` connections of the route.
    pub async fn wait_closed(&self, method: &str, path: &str, n: usize) -> bool {
        for _ in 0..1000 {
            if self.closed_count(method, path) >= n {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        false
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Option<RecordedRequest> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let len = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(1 << 20);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some(RecordedRequest {
        method,
        path: path.to_owned(),
        query: query.to_owned(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

async fn handle(mut sock: tokio::net::TcpStream, shared: Arc<Shared>) {
    let Some(req) = read_request(&mut sock).await else {
        return;
    };
    let k = key(&req.method, &req.path);
    lock(&shared.requests).push(req);
    let response = {
        let mut routes = lock(&shared.routes);
        routes
            .get_mut(&k)
            .and_then(|r| r.queue.pop_front().or_else(|| r.fallback.clone()))
    };
    let response = response.unwrap_or_else(|| Response::json(404, r#"{"error":"not scripted"}"#));
    let (rd, mut wr) = sock.into_split();
    let mut extra = String::new();
    for (n, v) in &response.headers {
        extra.push_str(n);
        extra.push_str(": ");
        extra.push_str(v);
        extra.push_str("\r\n");
    }
    let status = response.status;
    if !response.head_delay.is_zero() {
        tokio::time::sleep(response.head_delay).await;
    }
    match response.body {
        Body::Fixed(bytes) => {
            let head = format!(
                "HTTP/1.1 {status} {}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                reason(status),
                bytes.len()
            );
            wr.write_all(head.as_bytes()).await.ok();
            wr.write_all(&bytes).await.ok();
            wr.shutdown().await.ok();
        }
        body => {
            let head = format!(
                "HTTP/1.1 {status} {}\r\n{extra}Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                reason(status)
            );
            if wr.write_all(head.as_bytes()).await.is_err() || wr.flush().await.is_err() {
                note_closed(&shared, &k);
                return;
            }
            serve_steps(rd, wr, body, &shared, &k).await;
        }
    }
}

fn note_closed(shared: &Shared, k: &str) {
    *lock(&shared.closed).entry(k.to_owned()).or_default() += 1;
}

async fn write_chunk(wr: &mut OwnedWriteHalf, data: &[u8]) -> bool {
    let frame = [format!("{:x}\r\n", data.len()).as_bytes(), data, b"\r\n"].concat();
    wr.write_all(&frame).await.is_ok() && wr.flush().await.is_ok()
}

/// Waits for the client to go away; never returns otherwise.
async fn client_gone(rd: &mut OwnedReadHalf) {
    let mut probe = [0u8; 64];
    loop {
        match rd.read(&mut probe).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

async fn serve_steps(
    mut rd: OwnedReadHalf,
    mut wr: OwnedWriteHalf,
    body: Body,
    shared: &Shared,
    k: &str,
) {
    let mut list = match &body {
        Body::Steps(steps) => Some(steps.clone()),
        _ => None,
    };
    let mut live = match &body {
        Body::Live(rx) => lock(rx).take(),
        _ => None,
    };
    loop {
        let step = if let Some(list) = list.as_mut() {
            list.pop_front().unwrap_or(Step::Close)
        } else if let Some(rx) = live.as_mut() {
            tokio::select! {
                s = rx.recv() => s.unwrap_or(Step::Close),
                () = client_gone(&mut rd) => {
                    note_closed(shared, k);
                    return;
                }
            }
        } else {
            Step::Close
        };
        match step {
            Step::Line(text) => {
                if !write_chunk(&mut wr, format!("{text}\n").as_bytes()).await {
                    note_closed(shared, k);
                    return;
                }
            }
            Step::Raw(bytes) => {
                if !write_chunk(&mut wr, &bytes).await {
                    note_closed(shared, k);
                    return;
                }
            }
            Step::KeepAlive => {
                if !write_chunk(&mut wr, b"\n").await {
                    note_closed(shared, k);
                    return;
                }
            }
            Step::Delay(d) => {
                tokio::select! {
                    () = tokio::time::sleep(d) => {}
                    () = client_gone(&mut rd) => {
                        note_closed(shared, k);
                        return;
                    }
                }
            }
            Step::Close => {
                wr.write_all(b"0\r\n\r\n").await.ok();
                wr.shutdown().await.ok();
                return;
            }
            Step::Drop => return,
            Step::Hold => {
                client_gone(&mut rd).await;
                note_closed(shared, k);
                return;
            }
        }
    }
}
