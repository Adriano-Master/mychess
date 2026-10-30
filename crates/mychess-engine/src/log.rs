//! The protocol log: a bounded ring of everything exchanged with an engine.
//!
//! The Scid keeps a log file per engine that stops growing at `logMax` lines
//! (`analysis.tcl:2160`), which cuts off exactly the part one wants to read.
//! The ring here drops the oldest entries instead.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// Default ring size, as in the Scid (`start.tcl:424`).
pub const DEFAULT_LOG_LINES: usize = 5000;

/// The longest text kept per entry, in bytes. A hostile engine could write
/// lines of a megabyte; with 5000 entries that would be gigabytes.
pub const MAX_LOG_ENTRY_BYTES: usize = 4096;

/// What a log entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogKind {
    /// A line sent to the engine (`> `).
    Sent,
    /// A line received on the engine's standard output (`< `).
    Received,
    /// A line on the engine's standard error (`! `).
    Stderr,
    /// A note from the session itself (`# `).
    Note,
}

impl LogKind {
    /// The one-character prefix used by [`LogHandle::save_to`].
    pub fn prefix(self) -> &'static str {
        match self {
            LogKind::Sent => "> ",
            LogKind::Received => "< ",
            LogKind::Stderr => "! ",
            LogKind::Note => "# ",
        }
    }
}

/// One log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Position in the whole log, counting from 0 and never reused.
    pub seq: u64,
    /// What was recorded.
    pub kind: LogKind,
    /// The text, cut to [`MAX_LOG_ENTRY_BYTES`] (a cut entry ends with a
    /// ` [... N more bytes]` marker, which may exceed the limit by a few bytes).
    pub text: String,
}

#[derive(Debug)]
struct Ring {
    capacity: usize,
    entries: VecDeque<LogEntry>,
    next_seq: u64,
}

/// A shared handle to the protocol log of one engine. Cloning is cheap.
#[derive(Debug, Clone)]
pub struct LogHandle {
    ring: Arc<Mutex<Ring>>,
}

impl LogHandle {
    /// A log keeping the last `capacity` entries; `0` keeps nothing.
    pub fn new(capacity: usize) -> LogHandle {
        LogHandle {
            ring: Arc::new(Mutex::new(Ring {
                capacity,
                entries: VecDeque::new(),
                next_seq: 0,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Appends an entry (cutting long text at a character boundary).
    pub fn push(&self, kind: LogKind, text: &str) {
        let mut ring = self.lock();
        if ring.capacity == 0 {
            return;
        }
        let mut cut = text.len().min(MAX_LOG_ENTRY_BYTES);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut text_owned = text[..cut].to_owned();
        if cut < text.len() {
            let _ = write!(text_owned, " [... {} more bytes]", text.len() - cut);
        }
        let seq = ring.next_seq;
        ring.next_seq += 1;
        if ring.entries.len() == ring.capacity {
            ring.entries.pop_front();
        }
        ring.entries.push_back(LogEntry {
            seq,
            kind,
            text: text_owned,
        });
    }

    /// The entries with `seq >= since` still in the ring, and the value to
    /// pass next time to get only what is new.
    ///
    /// ```
    /// use mychess_engine::{LogHandle, LogKind};
    ///
    /// let log = LogHandle::new(10);
    /// log.push(LogKind::Sent, "uci");
    /// let (first, next) = log.snapshot_since(0);
    /// assert_eq!(first.len(), 1);
    /// log.push(LogKind::Received, "uciok");
    /// let (second, _) = log.snapshot_since(next);
    /// assert_eq!(second[0].text, "uciok");
    /// ```
    pub fn snapshot_since(&self, since: u64) -> (Vec<LogEntry>, u64) {
        let ring = self.lock();
        let entries = ring
            .entries
            .iter()
            .filter(|e| e.seq >= since)
            .cloned()
            .collect();
        (entries, ring.next_seq)
    }

    /// Writes the ring to `path`, one entry per line with `\n` line ends and
    /// the prefixes of [`LogKind::prefix`].
    ///
    /// The file is written next to the destination and renamed over it, so a
    /// reader never sees a half-written log. This does blocking file I/O:
    /// call it from an effect, not from `update` or `view`.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        use std::io::Write as _;
        let mut out = String::new();
        for entry in &self.lock().entries {
            out.push_str(entry.kind.prefix());
            out.push_str(&entry.text);
            out.push('\n');
        }
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let stem = path
            .file_name()
            .map_or_else(|| "log".into(), |n| n.to_string_lossy());
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let temp = dir.join(format!(".{stem}.{}.{unique}.tmp", std::process::id()));
        let written = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(out.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, path)
        })();
        if written.is_err() {
            std::fs::remove_file(&temp).ok();
        }
        written
    }

    /// How many entries are in the ring.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_drops_the_oldest_entries() {
        let log = LogHandle::new(3);
        for i in 0..5 {
            log.push(LogKind::Received, &format!("line {i}"));
        }
        let (all, next) = log.snapshot_since(0);
        assert_eq!(next, 5);
        assert_eq!(all.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 3, 4]);
        assert_eq!(log.snapshot_since(5).0.len(), 0);
    }

    #[test]
    fn zero_capacity_disables_logging() {
        let log = LogHandle::new(0);
        log.push(LogKind::Sent, "uci");
        assert!(log.is_empty());
        assert_eq!(log.snapshot_since(0), (Vec::new(), 0));
    }

    #[test]
    fn long_entries_are_cut_on_a_char_boundary() {
        let log = LogHandle::new(2);
        log.push(LogKind::Received, &"é".repeat(MAX_LOG_ENTRY_BYTES));
        let (entries, _) = log.snapshot_since(0);
        assert!(entries[0].text.contains("more bytes"));
    }

    #[test]
    fn save_writes_prefixed_lines_with_newlines_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engine.log");
        let log = LogHandle::new(10);
        log.push(LogKind::Sent, "uci");
        log.push(LogKind::Received, "uciok");
        log.push(LogKind::Stderr, "oops");
        log.push(LogKind::Note, "hi");
        log.save_to(&path).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"> uci\n< uciok\n! oops\n# hi\n"
        );
    }
}
