//! Line transport between a session and an engine: a reader task, a writer
//! task and, for real engines, the child process.
//!
//! The session never touches the pipes directly. Lines arrive through a
//! channel (so waiting for one is cancel-safe inside `select!`) and commands
//! leave through another (so writing never blocks the session, even when the
//! engine stops reading its input).

use std::collections::VecDeque;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::log::{LogHandle, LogKind};

/// The longest line accepted from an engine (spec m4-1, D411). The rest of a
/// longer line is discarded.
pub(crate) const MAX_LINE_BYTES: usize = 1 << 20;

/// Capacity of the queue of lines between the reader task and the session.
/// The queue is counted in lines, not bytes, so the worst case is
/// `LINE_QUEUE * MAX_LINE_BYTES` (64 MiB) held for a consumer that stopped
/// reading; past that the reader task blocks and the pipe backs up into the
/// engine (spec m4-1, D411). Real traffic is short lines, so 64 is plenty.
pub(crate) const LINE_QUEUE: usize = 64;

/// How many `stderr` lines are kept for the exit report.
const STDERR_TAIL_LINES: usize = 10;

/// The longest `stderr` line kept in the tail; a hostile engine could write
/// megabytes per line.
const STDERR_LINE_BYTES: usize = 4096;

/// How `Wire::terminate` takes leave of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Farewell {
    /// Send `quit` first (the engine is probably still listening).
    SayQuit,
    /// Just close the input (the engine is gone or misbehaving).
    Silent,
}

/// What the reader task reports.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WireEvent {
    /// A line; `truncated` when it exceeded [`MAX_LINE_BYTES`].
    Line { text: String, truncated: bool },
    /// The engine closed its output.
    Eof,
    /// Reading failed.
    Error(String),
}

/// Cuts a byte stream into lines, with a size limit, tolerating any bytes.
#[derive(Debug, Default)]
pub(crate) struct LineSplitter {
    buf: Vec<u8>,
    overflow: bool,
}

impl LineSplitter {
    fn append(&mut self, piece: &[u8]) {
        let room = MAX_LINE_BYTES.saturating_sub(self.buf.len());
        if piece.len() > room {
            self.buf.extend_from_slice(&piece[..room]);
            self.overflow = true;
        } else {
            self.buf.extend_from_slice(piece);
        }
    }

    fn take(&mut self) -> (String, bool) {
        let text = decode_line(&self.buf);
        let truncated = self.overflow;
        self.buf.clear();
        self.overflow = false;
        (text, truncated)
    }

    /// Feeds a chunk of bytes; complete lines are appended to `out`.
    pub(crate) fn feed(&mut self, chunk: &[u8], out: &mut Vec<(String, bool)>) {
        let mut rest = chunk;
        while let Some(pos) = rest.iter().position(|b| *b == b'\n') {
            self.append(&rest[..pos]);
            out.push(self.take());
            rest = &rest[pos + 1..];
        }
        self.append(rest);
    }

    /// The unterminated last line at end of input, if any.
    pub(crate) fn finish(&mut self) -> Option<(String, bool)> {
        (!self.buf.is_empty() || self.overflow).then(|| self.take())
    }
}

/// Decodes engine bytes: lossy UTF-8, NUL bytes dropped, trailing `\r` cut.
pub(crate) fn decode_line(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    text.retain(|c| c != '\0');
    while text.ends_with('\r') {
        text.pop();
    }
    text
}

/// The last lines the engine wrote to its standard error.
#[derive(Debug, Clone, Default)]
pub(crate) struct StderrTail(Arc<Mutex<VecDeque<String>>>);

impl StderrTail {
    fn push(&self, mut line: String) {
        if line.len() > STDERR_LINE_BYTES {
            line = crate::uci::parse::truncate_utf8(&line, STDERR_LINE_BYTES);
        }
        let mut tail = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if tail.len() == STDERR_TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }

    pub(crate) fn snapshot(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

fn spawn_reader<R>(mut reader: R, tx: mpsc::Sender<WireEvent>) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut splitter = LineSplitter::default();
        let mut chunk = vec![0_u8; 8192];
        let mut lines = Vec::new();
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => {
                    if let Some((text, truncated)) = splitter.finish() {
                        tx.send(WireEvent::Line { text, truncated }).await.ok();
                    }
                    tx.send(WireEvent::Eof).await.ok();
                    return;
                }
                Ok(n) => {
                    splitter.feed(&chunk[..n], &mut lines);
                    for (text, truncated) in lines.drain(..) {
                        if tx.send(WireEvent::Line { text, truncated }).await.is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    tx.send(WireEvent::Error(e.to_string())).await.ok();
                    return;
                }
            }
        }
    })
}

fn spawn_writer<W>(mut writer: W, mut rx: mpsc::Receiver<String>) -> JoinHandle<()>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(mut line) = rx.recv().await {
            line.push('\n');
            if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                return;
            }
        }
        // Channel closed: closing the pipe is how a well-behaved engine learns
        // that the session is over.
        writer.shutdown().await.ok();
    })
}

fn spawn_stderr_reader<R>(mut reader: R, tail: StderrTail, log: LogHandle) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut splitter = LineSplitter::default();
        let mut chunk = vec![0_u8; 8192];
        let mut lines = Vec::new();
        loop {
            let n = match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => {
                    lines.extend(splitter.finish());
                    0
                }
                Ok(n) => {
                    splitter.feed(&chunk[..n], &mut lines);
                    n
                }
            };
            for (text, _) in lines.drain(..) {
                log.push(LogKind::Stderr, &text);
                tail.push(text);
            }
            if n == 0 {
                return;
            }
        }
    })
}

/// Why a line could not be queued for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireClosed {
    /// The writer task is gone (the pipe broke or the wire is closed).
    Closed,
    /// The queue is full: the engine is not reading its input.
    Full,
}

impl WireClosed {
    /// A message for the exit report.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            WireClosed::Closed => "could not write to the engine",
            WireClosed::Full => "the engine stopped reading its input",
        }
    }
}

/// A connection to an engine.
#[derive(Debug)]
pub(crate) struct Wire {
    lines: mpsc::Receiver<WireEvent>,
    out: Option<mpsc::Sender<String>>,
    child: Option<Child>,
    tasks: Vec<JoinHandle<()>>,
    stderr_task: Option<JoinHandle<()>>,
    stderr_tail: StderrTail,
}

impl Wire {
    /// Wraps a reader (the engine's output) and a writer (its input).
    pub(crate) fn from_io<R, W>(read: R, write: W) -> Wire
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (line_tx, lines) = mpsc::channel(LINE_QUEUE);
        let (out_tx, out_rx) = mpsc::channel(1024);
        Wire {
            lines,
            out: Some(out_tx),
            child: None,
            tasks: vec![spawn_reader(read, line_tx), spawn_writer(write, out_rx)],
            stderr_task: None,
            stderr_tail: StderrTail::default(),
        }
    }

    /// Takes ownership of the engine process and drains its `stderr`.
    pub(crate) fn attach_child<E>(&mut self, child: Child, stderr: E, log: LogHandle)
    where
        E: AsyncRead + Unpin + Send + 'static,
    {
        self.child = Some(child);
        self.stderr_task = Some(spawn_stderr_reader(stderr, self.stderr_tail.clone(), log));
    }

    /// The process id, for a real engine.
    pub(crate) fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    /// Queues a line for the engine. Never blocks.
    pub(crate) fn send_line(&self, line: &str) -> Result<(), WireClosed> {
        let out = self.out.as_ref().ok_or(WireClosed::Closed)?;
        out.try_send(line.to_owned()).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => WireClosed::Full,
            mpsc::error::TrySendError::Closed(_) => WireClosed::Closed,
        })
    }

    /// The next event from the engine; `None` when the reader task is gone.
    pub(crate) async fn recv(&mut self) -> Option<WireEvent> {
        self.lines.recv().await
    }

    /// The last lines of `stderr`.
    pub(crate) fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.snapshot()
    }

    /// Ends the connection: optionally says `quit`, closes the engine's
    /// input, waits up to `wait` for it to go away and then kills it.
    /// Returns the exit status of a real process.
    pub(crate) async fn terminate(
        &mut self,
        farewell: Farewell,
        wait: Duration,
    ) -> Option<ExitStatus> {
        if farewell == Farewell::SayQuit {
            let _ = self.send_line("quit");
        }
        self.out = None; // the writer flushes, then closes the pipe
        let Wire { lines, child, .. } = self;
        let drain = async {
            while let Some(event) = lines.recv().await {
                if matches!(event, WireEvent::Eof | WireEvent::Error(_)) {
                    return;
                }
            }
        };
        let status = if let Some(child) = child.as_mut() {
            let waited = tokio::time::timeout(wait, async {
                tokio::pin!(drain);
                let mut drained = false;
                loop {
                    tokio::select! {
                        status = child.wait() => return status.ok(),
                        () = &mut drain, if !drained => drained = true,
                    }
                }
            })
            .await;
            if let Ok(status) = waited {
                status
            } else {
                tracing::debug!("engine ignored quit; killing it");
                child.kill().await.ok();
                child.try_wait().ok().flatten()
            }
        } else {
            tokio::time::timeout(wait, drain).await.ok();
            None
        };
        if let Some(task) = self.stderr_task.as_mut() {
            tokio::time::timeout(Duration::from_millis(100), task)
                .await
                .ok();
        }
        self.abort_tasks();
        status
    }

    fn abort_tasks(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(task) = &self.stderr_task {
            task.abort();
        }
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        self.abort_tasks();
        // `kill_on_drop` on the child does the rest.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(chunks: &[&[u8]]) -> Vec<(String, bool)> {
        let mut s = LineSplitter::default();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, &mut out);
        }
        out.extend(s.finish());
        out
    }

    #[test]
    fn splits_across_chunks_and_strips_cr() {
        let lines = split(&[b"uci", b"ok\r\nready", b"ok\n\n"]);
        let texts: Vec<_> = lines.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["uciok", "readyok", ""]);
    }

    #[test]
    fn keeps_the_last_unterminated_line_and_drops_nul_and_bad_utf8() {
        let lines = split(&[b"info string a\0b\xff\xfe end"]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].0.starts_with("info string ab"));
        assert!(lines[0].0.ends_with(" end"));
    }

    #[test]
    fn oversized_line_is_cut_and_flagged_and_the_next_one_is_clean() {
        let big = vec![b'a'; MAX_LINE_BYTES + 1000];
        let lines = split(&[&big, b"\nok\n"]);
        assert_eq!(lines[0].0.len(), MAX_LINE_BYTES);
        assert!(lines[0].1);
        assert_eq!(lines[1], ("ok".to_owned(), false));
    }

    #[tokio::test]
    async fn a_stalled_consumer_blocks_the_reader_and_bounds_the_memory() {
        // An engine flooding big lines while nobody reads the wire: the
        // reader must stop pulling bytes once the line queue is full.
        let (engine_side, session_side) = tokio::io::duplex(64 * 1024);
        let (_er, mut ew) = tokio::io::split(engine_side);
        let (sr, sw) = tokio::io::split(session_side);
        let wire = Wire::from_io(sr, sw);
        let written = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&written);
        let flood = tokio::spawn(async move {
            let mut line = vec![b'x'; 63 * 1024];
            line.push(b'\n');
            for _ in 0..100_000 {
                if ew.write_all(&line).await.is_err() {
                    return;
                }
                counter.fetch_add(line.len(), std::sync::atomic::Ordering::Relaxed);
            }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        let bytes = written.load(std::sync::atomic::Ordering::Relaxed);
        // Queue + the line in the reader's hand + the pipe buffer.
        let limit = (LINE_QUEUE + 4) * 64 * 1024;
        assert!(bytes <= limit, "{bytes} bytes pulled, limit {limit}");
        assert!(bytes >= LINE_QUEUE * 63 * 1024, "the queue was not used");
        flood.abort();
        drop(wire);
    }

    #[tokio::test]
    async fn wire_roundtrip_over_a_duplex() {
        let (engine_side, session_side) = tokio::io::duplex(4096);
        let (er, mut ew) = tokio::io::split(engine_side);
        let (sr, sw) = tokio::io::split(session_side);
        let mut wire = Wire::from_io(sr, sw);
        wire.send_line("uci").unwrap();
        let mut buf = [0_u8; 4];
        let mut er = er;
        er.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"uci\n");
        ew.write_all(b"uciok\n").await.unwrap();
        assert_eq!(
            wire.recv().await,
            Some(WireEvent::Line {
                text: "uciok".into(),
                truncated: false
            })
        );
        drop(ew);
        drop(er);
        assert!(
            wire.terminate(Farewell::Silent, Duration::from_millis(50))
                .await
                .is_none()
        );
    }
}

#[cfg(test)]
mod contract_tests;
