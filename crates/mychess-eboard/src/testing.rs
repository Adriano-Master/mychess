//! Test doubles: a fake serial port and a fake input engine (spec m8-10,
//! D2724). Nothing here touches hardware.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::DeviceIo;

/// What the fake port does next when read.
enum Feed {
    Bytes(Vec<u8>),
    Fail(io::ErrorKind),
    Eof,
}

/// The test's side of a [`fake_serial`] port: the "board".
#[derive(Debug, Clone)]
pub struct FakeBoard {
    feed: mpsc::Sender<FeedMsg>,
    written: Arc<Mutex<Vec<u8>>>,
    closed: Arc<Mutex<bool>>,
    stalls: Arc<AtomicU32>,
}

// `Feed` is not `Debug`; keep a debuggable wrapper for the derive above.
struct FeedMsg(Feed);

impl std::fmt::Debug for FeedMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FeedMsg")
    }
}

impl FakeBoard {
    /// The board says `text` followed by CR LF.
    pub fn say(&self, text: &str) {
        let mut bytes = text.as_bytes().to_vec();
        bytes.extend_from_slice(b"\r\n");
        self.feed.send(FeedMsg(Feed::Bytes(bytes))).ok();
    }

    /// The board sends raw bytes.
    pub fn send_bytes(&self, bytes: &[u8]) {
        self.feed.send(FeedMsg(Feed::Bytes(bytes.to_vec()))).ok();
    }

    /// The next read fails with `kind` (an unplugged adapter).
    pub fn fail(&self, kind: io::ErrorKind) {
        self.feed.send(FeedMsg(Feed::Fail(kind))).ok();
    }

    /// The next read returns end of data.
    pub fn hang_up(&self) {
        self.feed.send(FeedMsg(Feed::Eof)).ok();
    }

    /// The next `count` writes time out (a busy adapter) and then work.
    pub fn stall_writes(&self, count: u32) {
        self.stalls.store(count, Ordering::SeqCst);
    }

    /// Everything the application wrote so far, as text (lossy).
    pub fn written(&self) -> String {
        String::from_utf8_lossy(&self.written.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }

    /// The lines written so far (split on line breaks, empty ones dropped).
    pub fn written_lines(&self) -> Vec<String> {
        self.written()
            .split(['\n', '\r'])
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Whether the application dropped the reading half of the port.
    pub fn reader_closed(&self) -> bool {
        *self.closed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct FakeReader {
    rx: mpsc::Receiver<FeedMsg>,
    pending: Vec<u8>,
    closed: Arc<Mutex<bool>>,
}

impl Read for FakeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv_timeout(Duration::from_millis(10)) {
                Ok(FeedMsg(Feed::Bytes(bytes))) => self.pending = bytes,
                Ok(FeedMsg(Feed::Fail(kind))) => return Err(io::Error::new(kind, "fake failure")),
                Ok(FeedMsg(Feed::Eof)) => return Ok(0),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
                }
                // The test dropped its end: behave like a quiet port.
                Err(RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(10));
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "fake timeout"));
                }
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

impl Drop for FakeReader {
    fn drop(&mut self) {
        *self.closed.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }
}

struct FakeWriter {
    written: Arc<Mutex<Vec<u8>>>,
    stalls: Arc<AtomicU32>,
}

impl Write for FakeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let stalled = self
            .stalls
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if stalled {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fake write timeout",
            ));
        }
        self.written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A fake serial port: the device for a session and the board to drive it.
pub fn fake_serial() -> (DeviceIo, FakeBoard) {
    let (feed, rx) = mpsc::channel();
    let written = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(Mutex::new(false));
    let stalls = Arc::new(AtomicU32::new(0));
    let device = DeviceIo {
        reader: Box::new(FakeReader {
            rx,
            pending: Vec::new(),
            closed: Arc::clone(&closed),
        }),
        writer: Box::new(FakeWriter {
            written: Arc::clone(&written),
            stalls: Arc::clone(&stalls),
        }),
    };
    (
        device,
        FakeBoard {
            feed,
            written,
            closed,
            stalls,
        },
    )
}

/// How the fake input engine behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeDriver {
    /// The FEN `getposition` answers with.
    pub fen: String,
    /// Never say `Chessboard found` (the handshake hangs).
    pub silent: bool,
    /// Answer the handshake with `info string ERROR <text>` instead.
    pub error: Option<String>,
    /// Exit right after the handshake.
    pub exit_after_handshake: bool,
    /// Ignore `stop`, `quit` and the end of the input: only a kill ends it.
    pub linger: bool,
}

impl Default for FakeDriver {
    fn default() -> FakeDriver {
        FakeDriver {
            fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_owned(),
            silent: false,
            error: None,
            exit_after_handshake: false,
            linger: false,
        }
    }
}

impl FakeDriver {
    /// Reads the flags of the binary: `--silent`, `--error <text>`,
    /// `--exit-after-handshake`, `--linger`, `--fen <fen>`. The first positional
    /// arguments (port and parameters) are ignored.
    ///
    /// # Errors
    ///
    /// A message for a flag without its value.
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Result<FakeDriver, String> {
        let mut fake = FakeDriver::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--silent" => fake.silent = true,
                "--exit-after-handshake" => fake.exit_after_handshake = true,
                "--linger" => fake.linger = true,
                "--error" => fake.error = Some(args.next().ok_or("--error needs a text")?),
                "--fen" => fake.fen = args.next().ok_or("--fen needs a FEN")?,
                _ => {}
            }
        }
        Ok(fake)
    }
}

/// Runs the fake driver on any pair of streams until `quit` or end of input.
///
/// Behavior: after start it says `info string FEN <fen>` and, unless
/// `silent`, `info string Chessboard found and initialised`. It then answers
/// `xboard` with `info Engine mode    : xboard`, `getposition` with
/// `info FEN <fen>`, `getclock` with `info No Clock detected`, and acts on
/// the test commands `fake-move <move>` (prints `move <move>`) and
/// `fake-raw <text>` (prints `<text>` verbatim). `stop` is acknowledged;
/// `quit` ends the run.
pub async fn run_fake_input_engine<R, W>(reader: R, mut writer: W, fake: FakeDriver)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    async fn put<W: AsyncWrite + Unpin>(writer: &mut W, line: &str) -> bool {
        writer
            .write_all(format!("{line}\n").as_bytes())
            .await
            .is_ok()
            && writer.flush().await.is_ok()
    }
    if let Some(text) = &fake.error {
        put(&mut writer, &format!("info string ERROR {text}")).await;
        return;
    }
    if !put(&mut writer, &format!("info string FEN {}", fake.fen)).await {
        return;
    }
    if !fake.silent {
        put(
            &mut writer,
            "info string Chessboard found and initialised on fake port",
        )
        .await;
    }
    if fake.exit_after_handshake {
        return;
    }
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_owned();
        let keep_going = match line.as_str() {
            "xboard" => put(&mut writer, "info Engine mode    : xboard").await,
            "getposition" => put(&mut writer, &format!("info FEN {}", fake.fen)).await,
            "getclock" => put(&mut writer, "info No Clock detected").await,
            "stop" => put(&mut writer, "info string stopped").await,
            "quit" if fake.linger => true,
            "quit" => return,
            other => {
                if let Some(mv) = other.strip_prefix("fake-move ") {
                    put(&mut writer, &format!("move {mv}")).await
                } else if let Some(raw) = other.strip_prefix("fake-raw ") {
                    put(&mut writer, raw).await
                } else {
                    put(&mut writer, &format!("info string ok {other}")).await
                }
            }
        };
        if !keep_going {
            return;
        }
    }
    if fake.linger {
        std::future::pending::<()>().await;
    }
}
