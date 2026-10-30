//! A local fake FICS server for tests (spec m8-9, D2620). Feature `testing`.
//!
//! It listens on `127.0.0.1:0`, plays the login dialogue (guest or one
//! registered account), records every line the client sends and writes what
//! the test tells it to, in the way the real server does: CR LF line ends,
//! telnet noise and prompts without line end.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, tcp::OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// How the fake behaves.
#[derive(Debug, Clone)]
pub struct FakeOptions {
    /// The handle the server gives to guests.
    pub guest_name: String,
    /// The registered account: handle and password.
    pub account: Option<(String, String)>,
    /// Answer `ping` with a `ping time` line.
    pub answer_ping: bool,
    /// Write the banner in one-byte chunks (framing test).
    pub dribble: bool,
}

impl Default for FakeOptions {
    fn default() -> Self {
        Self {
            guest_name: "GuestTEST".to_owned(),
            account: Some(("alice".to_owned(), "s3cret".to_owned())),
            answer_ping: true,
            dribble: false,
        }
    }
}

#[derive(Default)]
struct Shared {
    received: Vec<String>,
    connections: usize,
    out: Option<mpsc::UnboundedSender<Vec<u8>>>,
    closed: usize,
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The fake server. Dropping it stops it.
pub struct FakeFics {
    addr: SocketAddr,
    shared: Arc<Mutex<Shared>>,
    task: JoinHandle<()>,
}

impl Drop for FakeFics {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeFics {
    /// Starts the server on a free local port.
    ///
    /// # Panics
    /// If the local socket cannot be opened (test environment broken).
    pub async fn start(options: FakeOptions) -> FakeFics {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the fake FICS");
        let addr = listener.local_addr().expect("local address");
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (tx, rx) = mpsc::unbounded_channel();
                {
                    let mut s = lock(&state);
                    s.connections += 1;
                    s.out = Some(tx);
                }
                let state = Arc::clone(&state);
                let options = options.clone();
                // One connection at a time is all the tests need.
                serve(stream, rx, &state, &options).await;
                lock(&state).closed += 1;
            }
        });
        FakeFics { addr, shared, task }
    }

    /// The host to connect to.
    #[must_use]
    pub fn host(&self) -> String {
        self.addr.ip().to_string()
    }

    /// The port to connect to.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Writes a line (with CR LF) to the connected client.
    pub fn send_line(&self, text: &str) {
        self.send_raw(format!("{text}\r\n").as_bytes());
    }

    /// Writes raw bytes.
    pub fn send_raw(&self, bytes: &[u8]) {
        if let Some(tx) = &lock(&self.shared).out {
            tx.send(bytes.to_vec()).ok();
        }
    }

    /// The lines the client sent so far.
    #[must_use]
    pub fn received(&self) -> Vec<String> {
        lock(&self.shared).received.clone()
    }

    /// Connections accepted so far.
    #[must_use]
    pub fn connections(&self) -> usize {
        lock(&self.shared).connections
    }

    /// Connections that already ended.
    #[must_use]
    pub fn closed(&self) -> usize {
        lock(&self.shared).closed
    }

    /// Waits (up to `limit`) for a received line that satisfies `pred`.
    pub async fn wait_line(&self, pred: impl Fn(&str) -> bool, limit: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            if let Some(found) = lock(&self.shared)
                .received
                .iter()
                .find(|l| pred(l))
                .cloned()
            {
                return Some(found);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Waits (up to `limit`) until `count` connections ended.
    pub async fn wait_closed(&self, count: usize, limit: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + limit;
        while self.closed() < count {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        true
    }
}

async fn write_all(wr: &mut OwnedWriteHalf, bytes: &[u8], dribble: bool) -> bool {
    if dribble {
        for b in bytes {
            if wr.write_all(&[*b]).await.is_err() {
                return false;
            }
            wr.flush().await.ok();
        }
        true
    } else {
        wr.write_all(bytes).await.is_ok()
    }
}

async fn read_line(rd: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<String> {
    let mut raw = Vec::new();
    match rd.read_until(b'\n', &mut raw).await {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(String::from_utf8_lossy(&raw).trim().to_owned()),
    }
}

async fn serve(
    stream: tokio::net::TcpStream,
    mut out: mpsc::UnboundedReceiver<Vec<u8>>,
    shared: &Arc<Mutex<Shared>>,
    options: &FakeOptions,
) {
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd);
    // Telnet negotiation, a banner and the prompt without line end.
    let mut banner = vec![255, 251, 1];
    banner.extend_from_slice(b"\r\n    Welcome to the Free Internet Chess Server\r\n\r\nlogin: ");
    if !write_all(&mut wr, &banner, options.dribble).await {
        return;
    }
    let Some(name) = read_line(&mut rd).await else {
        return;
    };
    let session_name = if name.eq_ignore_ascii_case("guest") {
        let prompt = format!(
            "\r\n\"{0}\" is a guest account.\r\nPress return to enter the server as \"{0}\":",
            options.guest_name
        );
        if !write_all(&mut wr, prompt.as_bytes(), options.dribble).await {
            return;
        }
        if read_line(&mut rd).await.is_none() {
            return;
        }
        options.guest_name.clone()
    } else {
        let Some((user, password)) = &options.account else {
            let text = format!("\r\n\"{name}\" is not a registered name.\r\n");
            write_all(&mut wr, text.as_bytes(), false).await;
            return;
        };
        if !name.eq_ignore_ascii_case(user) {
            let text = format!("\r\n\"{name}\" is not a registered name.\r\n");
            write_all(&mut wr, text.as_bytes(), false).await;
            return;
        }
        let prompt = format!(
            "\r\n\"{user}\" is a registered name.  If it is yours, type the password.\r\npassword: "
        );
        if !write_all(&mut wr, prompt.as_bytes(), options.dribble).await {
            return;
        }
        let Some(given) = read_line(&mut rd).await else {
            return;
        };
        if given != *password {
            write_all(&mut wr, b"\r\n**** Invalid password! ****\r\n\r\n", false).await;
            return;
        }
        user.clone()
    };
    let started = format!("\r\n**** Starting FICS session as {session_name}(U) ****\r\n\r\nfics% ");
    if !write_all(&mut wr, started.as_bytes(), false).await {
        return;
    }
    loop {
        tokio::select! {
            line = read_line(&mut rd) => {
                let Some(line) = line else { return };
                lock(shared).received.push(line.clone());
                if options.answer_ping && line == "ping" {
                    let text = format!("\r\nAverage ping time for {session_name}: 12 ms\r\nfics% ");
                    if !write_all(&mut wr, text.as_bytes(), false).await {
                        return;
                    }
                }
                if line == "quit" {
                    write_all(&mut wr, b"\r\nBye.\r\n", false).await;
                    return;
                }
            }
            bytes = out.recv() => {
                let Some(bytes) = bytes else { return };
                if !write_all(&mut wr, &bytes, false).await {
                    return;
                }
            }
        }
    }
}
