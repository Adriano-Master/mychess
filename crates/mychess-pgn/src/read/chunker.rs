//! The game chunker: cuts a byte stream into [`RawGame`]s without parsing
//! them (D17, D30).
//!
//! It follows the Scid's three states (`Searching`, `Header`, `Movetext`) but
//! looks only at what decides where a game ends: comments (`{}`, `;`, `%`
//! lines, so a `[%clk ...]` line inside a comment never cuts a game), words
//! (a result token ends the game) and a line that starts with `[` (the next
//! header). Everything is incremental: the state survives a refill of the
//! 1 MiB read buffer, and no byte is examined twice for the same decision
//! (the `probe` field remembers how far a long line or word was searched).
//!
//! A game larger than `max_game_bytes` is reported as an oversize
//! [`RawGame`] and the bytes up to the next line starting with `[Event ` are
//! dropped on the fly.

use std::io::{self, Read};

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, WINDOWS_1252};

use super::chars::{
    blank_len, count_breaks, find_word_end, first_non_space, is_eol, is_space, result_token,
};
use super::error::PgnError;
use super::input::Input;
use super::parsed::Termination;

/// Size of one read from the input.
const CHUNK: usize = 1 << 20;
/// The UTF-8 byte order mark.
const BOM: [u8; 3] = [0xef, 0xbb, 0xbf];
/// Amount of undecodable text fed to the encoding detector (D18).
const EVIDENCE_BUDGET: usize = 256 * 1024;
/// Bytes of the stream (from the first game on) searched for evidence before
/// the first game is emitted. The prescan reads until it has this many bytes
/// or the stream ends, so the choice never depends on how `read` slices the
/// input (D18).
const PRESCAN_BYTES: usize = CHUNK;
/// Less undecodable text than this proves nothing (a lone `\xA3` is
/// "Ł" to the detector but "£" to every Western chess database), so the
/// default `windows-1252` stays.
const MIN_EVIDENCE: usize = 32;
/// Bytes of an oversize game kept for its header.
const OVERSIZE_KEEP: usize = 256 * 1024;
/// Bytes of text between games kept for `PreGameText::AsComment`.
const JUNK_KEEP: usize = 1 << 20;
/// Line prefix where reading resumes after an oversize game.
const RESYNC_PREFIX: &[u8] = b"[Event ";

/// The bytes of one game, cut from the stream but not yet parsed.
///
/// Parsing is [`RawGame::parse`], a pure function of this value and the
/// options, so a caller may parse many `RawGame`s in parallel and keep the
/// order.
#[derive(Debug, Clone)]
pub struct RawGame {
    pub(crate) number: u64,
    pub(crate) start_line: u64,
    pub(crate) offset: u64,
    pub(crate) bytes: Vec<u8>,
    /// Bytes of the stream covered by the game (more than `bytes.len()` for
    /// an oversize game).
    pub(crate) span_len: u64,
    /// Text between the previous game and the first header line.
    pub(crate) junk: Vec<u8>,
    pub(crate) junk_line: u64,
    pub(crate) junk_offset: u64,
    pub(crate) end: Termination,
    /// Encoding for text that is not UTF-8; `None` forces `U+FFFD`.
    pub(crate) fallback: Option<&'static Encoding>,
    /// Whether to report that the fallback encoding is in use.
    pub(crate) report_fallback: bool,
}

impl RawGame {
    /// Number of the game in the file (1-based).
    pub fn number(&self) -> u64 {
        self.number
    }

    /// Line (1-based) of the first header tag.
    pub fn start_line(&self) -> u64 {
        self.start_line
    }

    /// Offset of the first header tag in the decompressed stream.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The bytes of the game, from the first header tag.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Phase of the Scid's state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Searching,
    Header,
    Movetext,
}

enum Step {
    Cut { end: usize, term: Termination },
    NeedMore,
}

/// An oversize game being skipped.
struct Skipping {
    game: RawGame,
    at_line_start: bool,
}

/// Cuts a byte stream into [`RawGame`]s.
///
/// Gzip input (magic `1F 8B`) is decompressed on the fly and a UTF-8 BOM is
/// dropped. Memory use is bounded by the largest game (`max_game_bytes`) plus
/// one read buffer.
///
/// ```
/// use mychess_pgn::read::GameChunker;
///
/// let pgn = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 *\n";
/// let games: Vec<_> = GameChunker::new(pgn.as_bytes(), 1 << 20)
///     .collect::<Result<_, _>>()?;
/// assert_eq!(games.len(), 2);
/// assert_eq!(games[1].number(), 2);
/// assert_eq!(games[1].start_line(), 5);
/// # Ok::<(), mychess_pgn::read::PgnError>(())
/// ```
pub struct GameChunker<R: Read> {
    input: Input<R>,
    max_game_bytes: usize,
    buf: Vec<u8>,
    /// Stream offset of `buf[0]`.
    base: u64,
    /// Start of the current segment (bytes already emitted are before it).
    start: usize,
    /// Next byte to examine.
    scan: usize,
    /// Line (1-based) of `buf[start]`.
    line: u64,
    prev_cr: bool,
    phase: Phase,
    /// Start of the first header line of the current game.
    header_at: Option<usize>,
    /// First non-blank byte of the line being examined (line phases).
    line_p: Option<usize>,
    /// Bytes before this index were already searched for what the current
    /// step is looking for.
    probe: usize,
    in_brace: bool,
    in_line_comment: bool,
    /// Start of the current line (movetext phase).
    line_start: usize,
    /// Only white space seen since `line_start`.
    bol: bool,
    skipping: Option<Skipping>,
    next_number: u64,
    eof: bool,
    bom_done: bool,
    finished: bool,
    utf8_only: bool,
    detector: EncodingDetector,
    evidence: usize,
    fallback: &'static Encoding,
    fallback_reported: bool,
    /// The fallback encoding was chosen from enough evidence and no longer
    /// changes (D18).
    fallback_locked: bool,
    /// The first buffer was already searched for evidence.
    prescanned: bool,
    skipped: u64,
    /// A read error that follows the last readable game.
    pending_err: Option<PgnError>,
}

impl<R: Read> GameChunker<R> {
    /// A chunker over `input`; games larger than `max_game_bytes` are
    /// skipped (see [`Termination::Oversize`]).
    pub fn new(input: R, max_game_bytes: usize) -> GameChunker<R> {
        GameChunker {
            input: Input::new(input),
            max_game_bytes,
            buf: Vec::new(),
            base: 0,
            start: 0,
            scan: 0,
            line: 1,
            prev_cr: false,
            phase: Phase::Searching,
            header_at: None,
            line_p: None,
            probe: 0,
            in_brace: false,
            in_line_comment: false,
            line_start: 0,
            bol: true,
            skipping: None,
            next_number: 0,
            eof: false,
            bom_done: false,
            finished: false,
            utf8_only: false,
            detector: EncodingDetector::new(Iso2022JpDetection::Deny),
            evidence: 0,
            fallback: WINDOWS_1252,
            fallback_reported: false,
            fallback_locked: false,
            prescanned: false,
            skipped: 0,
            pending_err: None,
        }
    }

    /// Bytes of text outside any game seen so far: non-blank text between
    /// games and after the last one (a file without any `[` header line
    /// yields no game and a large count here).
    pub fn skipped_bytes(&self) -> u64 {
        self.skipped
    }

    /// Bytes of the decompressed stream consumed so far (the end of the last
    /// game returned).
    pub fn bytes_consumed(&self) -> u64 {
        self.base + self.start as u64
    }

    /// The encoding used for text that is not valid UTF-8, as guessed so far
    /// (`windows-1252` until there is evidence).
    pub fn fallback_encoding(&self) -> &'static Encoding {
        self.fallback
    }

    // ------------------------------------------------------------------
    // Reading
    // ------------------------------------------------------------------

    /// Drops the consumed prefix of the buffer when it is at least half.
    fn compact(&mut self) {
        let d = self.start;
        if d == 0 || d * 2 < self.buf.len() {
            return;
        }
        self.buf.drain(..d);
        self.base += d as u64;
        self.start = 0;
        self.scan = self.scan.saturating_sub(d);
        self.probe = self.probe.saturating_sub(d);
        self.line_start = self.line_start.saturating_sub(d);
        self.header_at = self.header_at.map(|h| h.saturating_sub(d));
        self.line_p = self.line_p.map(|p| p.saturating_sub(d));
    }

    fn fill(&mut self) -> Result<(), PgnError> {
        self.compact();
        self.fill_up_to(CHUNK)
    }

    /// One read of at most `max` bytes appended to the buffer (no compaction,
    /// so indices into the buffer stay valid).
    fn fill_up_to(&mut self, max: usize) -> Result<(), PgnError> {
        let old = self.buf.len();
        self.buf.resize(old + max, 0);
        let n = loop {
            match self.input.read(&mut self.buf[old..]) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(source) => {
                    self.buf.truncate(old);
                    return Err(self.fatal(source));
                }
            }
        };
        self.buf.truncate(old + n);
        if n == 0 {
            self.eof = true;
        }
        if !self.bom_done && (self.buf.len() >= BOM.len() || self.eof) {
            self.bom_done = true;
            let encoding = match self.buf.get(..2) {
                Some([0xff, 0xfe]) => Some("UTF-16LE"),
                Some([0xfe, 0xff]) => Some("UTF-16BE"),
                _ => None,
            };
            if let Some(encoding) = encoding {
                return Err(PgnError::UnsupportedEncoding { encoding });
            }
            if self.buf.starts_with(&BOM) {
                self.start = BOM.len();
                self.scan = BOM.len();
                self.probe = BOM.len();
                self.line_start = BOM.len();
                self.utf8_only = true;
            }
        }
        Ok(())
    }

    fn fatal(&self, source: io::Error) -> PgnError {
        let offset = self.base + self.buf.len() as u64;
        if self.input.is_gzip() && !self.input.source_failed() {
            PgnError::Gzip { offset, source }
        } else {
            PgnError::Io { offset, source }
        }
    }

    // ------------------------------------------------------------------
    // Bookkeeping
    // ------------------------------------------------------------------

    fn advance_to(&mut self, new_start: usize) {
        if new_start > self.start {
            self.line += count_breaks(&self.buf[self.start..new_start], &mut self.prev_cr);
            self.start = new_start;
        }
    }

    fn reset_line(&mut self) {
        self.line_p = None;
        self.probe = self.scan;
    }

    fn reset_segment(&mut self, at: usize) {
        self.phase = Phase::Searching;
        self.header_at = None;
        self.scan = at;
        self.line_start = at;
        self.bol = true;
        self.in_brace = false;
        self.in_line_comment = false;
        self.reset_line();
    }

    // ------------------------------------------------------------------
    // Scanning
    // ------------------------------------------------------------------

    fn scan_step(&mut self) -> Step {
        loop {
            match self.phase {
                Phase::Searching | Phase::Header => {
                    if !self.line_step() {
                        return Step::NeedMore;
                    }
                }
                Phase::Movetext => return self.movetext_step(),
            }
        }
    }

    /// Processes one line of the `Searching` or `Header` phase. `false` when
    /// more data is needed.
    fn line_step(&mut self) -> bool {
        let len = self.buf.len();
        let ls = self.scan;
        if ls >= len {
            return false;
        }
        let p = if let Some(p) = self.line_p {
            p
        } else {
            let mut i = self.probe.max(ls);
            loop {
                match self.buf.get(i) {
                    None => {
                        self.probe = len;
                        if self.eof {
                            self.scan = len;
                        }
                        return false;
                    }
                    // Half of a UTF-8 no-break space: wait for the other.
                    Some(0xc2) if i + 1 == len && !self.eof => {
                        self.probe = i;
                        return false;
                    }
                    Some(_) => match blank_len(&self.buf[i..]) {
                        0 => break,
                        n => i += n,
                    },
                }
            }
            self.line_p = Some(i);
            self.probe = i;
            i
        };
        let c = self.buf[p];
        if is_eol(c) {
            self.scan = p + 1;
            self.reset_line();
            return true;
        }
        match (self.phase, c) {
            (Phase::Searching, b'[') => {
                self.phase = Phase::Header;
                self.header_at = Some(ls);
                return true;
            }
            (Phase::Header, c) if !matches!(c, b'[' | b'%' | b';') => {
                // The first token of the movetext (no blank line needed).
                self.phase = Phase::Movetext;
                self.scan = p;
                self.line_start = ls;
                self.bol = false;
                self.in_brace = false;
                self.in_line_comment = false;
                self.reset_line();
                return true;
            }
            _ => {}
        }
        // A line to skip: junk, a tag line or a `%` / `;` line.
        let from = self.probe.max(p);
        let end = match self.buf[from..].iter().position(|&b| is_eol(b)) {
            Some(i) => from + i,
            None if self.eof => len,
            None => {
                self.probe = len;
                return false;
            }
        };
        self.scan = (end + 1).min(len);
        self.reset_line();
        true
    }

    fn movetext_step(&mut self) -> Step {
        let len = self.buf.len();
        loop {
            if self.scan >= len {
                return Step::NeedMore;
            }
            if self.in_brace {
                if let Some(i) = self.buf[self.scan..].iter().position(|&b| b == b'}') {
                    self.scan += i + 1;
                    self.in_brace = false;
                } else {
                    self.scan = len;
                    return Step::NeedMore;
                }
                continue;
            }
            if self.in_line_comment {
                if let Some(i) = self.buf[self.scan..].iter().position(|&b| is_eol(b)) {
                    self.scan += i;
                    self.in_line_comment = false;
                } else {
                    self.scan = len;
                    return Step::NeedMore;
                }
                continue;
            }
            match self.buf[self.scan] {
                b'\n' | b'\r' => {
                    self.scan += 1;
                    self.line_start = self.scan;
                    self.bol = true;
                }
                // Also the Latin-1 no-break space (Scid: `charIsSpace(160)`).
                b' ' | b'\t' | 0x0b | 0x0c | 0xa0 => self.scan += 1,
                b'{' => {
                    self.in_brace = true;
                    self.bol = false;
                    self.scan += 1;
                }
                b';' => {
                    self.in_line_comment = true;
                    self.bol = false;
                    self.scan += 1;
                }
                b'%' if self.scan == self.line_start => {
                    self.in_line_comment = true;
                    self.bol = false;
                    self.scan += 1;
                }
                b'[' if self.bol => {
                    return Step::Cut {
                        end: self.line_start,
                        term: Termination::NextGameHeader,
                    };
                }
                b'}' | b'(' | b')' | b'$' => {
                    self.bol = false;
                    self.scan += 1;
                }
                // UTF-8 no-break space: white space, also at the start of a
                // line.
                0xc2 if self.buf.get(self.scan + 1) == Some(&0xa0) => self.scan += 2,
                0xc2 if self.scan + 1 == len && !self.eof => return Step::NeedMore,
                _ => {
                    let from = self.probe.max(self.scan + 1);
                    let end = match find_word_end(&self.buf, self.scan, from) {
                        Some(end) => end,
                        None if self.eof => len,
                        None => {
                            self.probe = len;
                            return Step::NeedMore;
                        }
                    };
                    self.bol = false;
                    if result_token(&self.buf[self.scan..end]).is_some() {
                        return Step::Cut {
                            end,
                            term: Termination::ResultToken,
                        };
                    }
                    self.scan = end;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Emitting games
    // ------------------------------------------------------------------

    fn new_game(&mut self) -> RawGame {
        self.next_number += 1;
        RawGame {
            number: self.next_number,
            start_line: 0,
            offset: 0,
            bytes: Vec::new(),
            span_len: 0,
            junk: Vec::new(),
            junk_line: 0,
            junk_offset: 0,
            end: Termination::EndOfInput,
            fallback: if self.utf8_only {
                None
            } else {
                Some(self.fallback)
            },
            report_fallback: false,
        }
    }

    /// Takes the junk before the header of the current game into `game`.
    fn take_junk(&mut self, game: &mut RawGame, header: usize) {
        game.junk_line = self.line;
        game.junk_offset = self.base + self.start as u64;
        let junk = &self.buf[self.start..header];
        if first_non_space(junk).is_some() {
            game.junk = junk[..junk.len().min(JUNK_KEEP)].to_vec();
            self.skipped += junk.len() as u64;
        }
        self.advance_to(header);
    }

    /// Cuts the current game at `end` and starts looking for the next one.
    fn emit(&mut self, end: usize, term: Termination) -> RawGame {
        if !self.prescanned {
            self.prescan_encoding();
        }
        let header = self.header_at.unwrap_or(self.start).min(end);
        let mut game = self.new_game();
        self.take_junk(&mut game, header);
        game.start_line = self.line;
        game.offset = self.base + header as u64;
        game.span_len = (end - header) as u64;
        game.end = term;
        if end - header > self.max_game_bytes {
            // Same outcome as when the limit is hit while reading, whatever
            // the size of the reads.
            game.bytes = self.buf[header..header + (end - header).min(OVERSIZE_KEEP)].to_vec();
            game.end = Termination::Oversize;
        } else {
            game.bytes = self.buf[header..end].to_vec();
        }
        self.advance_to(end);
        self.observe_encoding(&mut game);
        self.reset_segment(end);
        game
    }

    /// Feeds the whole words that hold undecodable bytes to the detector
    /// (the detector needs the neighbours of a byte, and valid UTF-8 text
    /// must stay out). Returns whether anything was fed.
    fn feed_evidence(&mut self, bytes: &[u8]) -> bool {
        let mut fed = false;
        let is_cut = |b: u8| is_space(b) || matches!(b, b'"' | b'[' | b']' | b'{' | b'}');
        let mut pos = 0;
        let mut last_end = 0;
        for chunk in bytes.utf8_chunks() {
            let invalid_start = pos + chunk.valid().len();
            pos = invalid_start + chunk.invalid().len();
            if chunk.invalid().is_empty() || invalid_start < last_end {
                continue;
            }
            if self.evidence >= EVIDENCE_BUDGET {
                break;
            }
            let mut start = invalid_start;
            while start > last_end && !is_cut(bytes[start - 1]) && invalid_start - start < 128 {
                start -= 1;
            }
            let mut end = pos;
            while end < bytes.len() && !is_cut(bytes[end]) && end - pos < 128 {
                end += 1;
            }
            self.detector.feed(&bytes[start..end], false);
            self.detector.feed(b" ", false);
            self.evidence += end - start;
            last_end = end;
            fed = true;
        }
        fed
    }

    /// Looks for evidence in the data already read, before the first game is
    /// emitted, so that the first games are decoded with the same encoding
    /// as the rest of the file (D18). With enough evidence the choice is
    /// final; with too little, the detector starts over and the games feed
    /// it as they come.
    fn prescan_encoding(&mut self) {
        self.prescanned = true;
        if self.utf8_only {
            return;
        }
        // Read up to the budget (or the end), whatever the size of each read.
        while !self.eof && self.buf.len() - self.start < PRESCAN_BYTES {
            let want = PRESCAN_BYTES - (self.buf.len() - self.start);
            if let Err(e) = self.fill_up_to(want.min(CHUNK)) {
                self.read_failed(e);
            }
        }
        if self.start >= self.buf.len() {
            return;
        }
        let data = std::mem::take(&mut self.buf);
        let end = data.len().min(self.start + PRESCAN_BYTES);
        let fed = self.feed_evidence(&data[self.start..end]);
        self.buf = data;
        if fed && self.evidence >= MIN_EVIDENCE {
            self.fallback = self.detector.guess(None, Utf8Detection::Deny);
            self.fallback_locked = true;
        } else {
            self.detector = EncodingDetector::new(Iso2022JpDetection::Deny);
            self.evidence = 0;
        }
    }

    /// Feeds the undecodable text of `game` to the detector (D18).
    fn observe_encoding(&mut self, game: &mut RawGame) {
        if self.utf8_only || game.bytes.is_ascii() {
            return;
        }
        if self.fallback_locked {
            if !self.fallback_reported && std::str::from_utf8(&game.bytes).is_err() {
                self.fallback_reported = true;
                game.report_fallback = true;
            }
            return;
        }
        if self.evidence >= EVIDENCE_BUDGET {
            return;
        }
        let bytes = std::mem::take(&mut game.bytes);
        let fed = self.feed_evidence(&bytes);
        game.bytes = bytes;
        if fed {
            if self.evidence >= MIN_EVIDENCE {
                self.fallback = self.detector.guess(None, Utf8Detection::Deny);
            }
            game.fallback = Some(self.fallback);
            if !self.fallback_reported {
                self.fallback_reported = true;
                game.report_fallback = true;
            }
        }
    }

    fn finish_at_eof(&mut self) -> Option<RawGame> {
        self.finished = true;
        if self.header_at.is_none() {
            let rest = &self.buf[self.start.min(self.buf.len())..];
            if first_non_space(rest).is_some() {
                self.skipped += rest.len() as u64;
            }
            // Progress reaches the end of the stream.
            self.advance_to(self.buf.len());
            return None;
        }
        let end = self.buf.len();
        Some(self.emit(end, Termination::EndOfInput))
    }

    // ------------------------------------------------------------------
    // Oversize games
    // ------------------------------------------------------------------

    fn start_oversize(&mut self) {
        let header = self.header_at.unwrap_or(self.start);
        let mut game = self.new_game();
        self.take_junk(&mut game, header);
        game.start_line = self.line;
        game.offset = self.base + header as u64;
        game.end = Termination::Oversize;
        if self.header_at.is_some() {
            let keep = (self.buf.len() - header).min(OVERSIZE_KEEP);
            game.bytes = self.buf[header..header + keep].to_vec();
        }
        let at_line_start = self.scan > 0 && is_eol(self.buf[self.scan - 1]);
        self.advance_to(self.scan);
        self.skipping = Some(Skipping {
            game,
            at_line_start,
        });
    }

    /// Drops bytes until a line starting with `[Event `. `Some(index)` of
    /// that line, or `None` when more data is needed.
    fn skip_step(&mut self) -> Option<usize> {
        let len = self.buf.len();
        let eof = self.eof;
        loop {
            if self.scan >= len {
                self.advance_to(len);
                return None;
            }
            let at_line_start = self.skipping.as_ref().is_some_and(|s| s.at_line_start);
            if at_line_start {
                let rest = &self.buf[self.scan..];
                if rest.len() < RESYNC_PREFIX.len() && !eof {
                    self.advance_to(self.scan);
                    return None;
                }
                if rest.starts_with(RESYNC_PREFIX) {
                    return Some(self.scan);
                }
                if let Some(s) = self.skipping.as_mut() {
                    s.at_line_start = false;
                }
            }
            match self.buf[self.scan..].iter().position(|&b| is_eol(b)) {
                Some(i) => {
                    self.scan += i + 1;
                    if let Some(s) = self.skipping.as_mut() {
                        s.at_line_start = true;
                    }
                }
                None => {
                    self.scan = len;
                }
            }
        }
    }

    /// Completes the oversize game; the stream continues at `resume`.
    fn finish_oversize(&mut self, resume: usize) -> Option<RawGame> {
        let skipping = self.skipping.take()?;
        let mut game = skipping.game;
        self.advance_to(resume);
        game.span_len = (self.base + resume as u64).saturating_sub(game.offset);
        self.reset_segment(resume);
        Some(game)
    }

    // ------------------------------------------------------------------
    // The loop
    // ------------------------------------------------------------------

    /// A read failed: the bytes read so far are still cut into games (the
    /// last one as if the input had ended), then the error is returned.
    fn read_failed(&mut self, error: PgnError) {
        self.pending_err = Some(error);
        self.eof = true;
    }

    /// The end of the stream: the last game, if any, then a pending error.
    fn at_end(&mut self, game: Option<RawGame>) -> Option<Result<RawGame, PgnError>> {
        self.finished = true;
        match game {
            Some(game) => Some(Ok(game)),
            None => self.pending_err.take().map(Err),
        }
    }

    fn next_raw(&mut self) -> Option<Result<RawGame, PgnError>> {
        if self.finished {
            return self.pending_err.take().map(Err);
        }
        loop {
            if !self.bom_done {
                if let Err(e) = self.fill() {
                    self.finished = true;
                    return Some(Err(e));
                }
                continue;
            }
            if self.skipping.is_some() {
                if let Some(resume) = self.skip_step() {
                    return self.finish_oversize(resume).map(Ok);
                }
                if self.eof {
                    let end = self.buf.len();
                    let game = self.finish_oversize(end);
                    return self.at_end(game);
                }
                if let Err(e) = self.fill() {
                    self.read_failed(e);
                }
                continue;
            }
            match self.scan_step() {
                Step::Cut { end, term } => return Some(Ok(self.emit(end, term))),
                Step::NeedMore => {
                    if self.eof {
                        let game = self.finish_at_eof();
                        return self.at_end(game);
                    }
                    if self.buf.len() - self.start > self.max_game_bytes {
                        self.start_oversize();
                        continue;
                    }
                    if let Err(e) = self.fill() {
                        self.read_failed(e);
                    }
                }
            }
        }
    }
}

impl<R: Read> Iterator for GameChunker<R> {
    type Item = Result<RawGame, PgnError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_raw()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use pretty_assertions::assert_eq;

    use super::*;

    fn chunks(data: &[u8]) -> Vec<RawGame> {
        GameChunker::new(data, 1 << 20)
            .collect::<Result<_, _>>()
            .expect("no i/o error")
    }

    /// A reader that returns at most `n` bytes per call.
    struct Trickle<'a>(&'a [u8], usize);

    impl Read for Trickle<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.1.min(out.len()).min(self.0.len());
            out[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    fn text(g: &RawGame) -> &str {
        std::str::from_utf8(g.bytes()).expect("utf8")
    }

    const TWO: &str = "[Event \"A\"]\n[Site \"s\"]\n\n1. e4 e5 1-0\n\n[Event \"B\"]\n\n1. d4 *\n";

    #[test]
    fn cuts_games_after_the_result_token() {
        let games = chunks(TWO.as_bytes());
        assert_eq!(games.len(), 2);
        assert_eq!(
            text(&games[0]),
            "[Event \"A\"]\n[Site \"s\"]\n\n1. e4 e5 1-0"
        );
        assert_eq!(text(&games[1]), "[Event \"B\"]\n\n1. d4 *");
        assert_eq!(games[0].end, Termination::ResultToken);
        assert_eq!((games[0].number(), games[1].number()), (1, 2));
        assert_eq!((games[0].start_line(), games[1].start_line()), (1, 6));
        assert_eq!(games[0].offset(), 0);
        assert_eq!(games[1].offset(), TWO.find("[Event \"B\"]").unwrap() as u64);
    }

    #[test]
    fn nbsp_before_a_header_bracket_starts_a_game() {
        for lead in [&b"\xc2\xa0"[..], b"\xa0", b" \xc2\xa0\xa0"] {
            let mut pgn = b"[Event \"A\"]\n\n1. e4 e5\n".to_vec();
            pgn.extend_from_slice(lead);
            pgn.extend_from_slice(b"[Event \"B\"]\n\n1. d4 *\n");
            for n in [1, 3, 1 << 20] {
                let games: Vec<_> = GameChunker::new(Trickle(&pgn, n), 1 << 20)
                    .collect::<Result<_, _>>()
                    .expect("read");
                assert_eq!(games.len(), 2, "{lead:?} read size {n}");
                assert_eq!(games[0].end, Termination::NextGameHeader);
            }
        }
    }

    #[test]
    fn a_grave_at_line_start_is_not_a_no_break_space() {
        let pgn = "\u{e0}[Event \"A\"]\n\n1. e4 *\n";
        assert_eq!(chunks(pgn.as_bytes()).len(), 0);
    }

    #[test]
    fn bytes_consumed_reaches_the_end_with_trailing_blank_lines() {
        let pgn = b"[Event \"A\"]\n\n1. e4 1-0\n\n  \n\xc2\xa0\n";
        for n in [1, 5, 1 << 20] {
            let mut c = GameChunker::new(Trickle(pgn, n), 1 << 20);
            assert_eq!(c.by_ref().count(), 1);
            assert_eq!(c.bytes_consumed(), pgn.len() as u64, "read size {n}");
            assert_eq!(c.skipped_bytes(), 0);
        }
    }

    #[test]
    fn encoding_guess_does_not_depend_on_read_sizes() {
        let mut data = b"[White \"Anand\xa3\"]\n\n*\n\n[Event \"".to_vec();
        data.extend(std::iter::repeat_n(0xa5u8, 36));
        data.extend_from_slice(b"\"]\n\n*\n");
        let guess = |n: usize| {
            let mut c = GameChunker::new(Trickle(&data, n), 1 << 20);
            let first = c.next().expect("game").expect("read");
            (first.fallback, c.fallback_encoding())
        };
        assert_eq!(guess(1), guess(1 << 20));
        assert_eq!(guess(7), guess(1 << 20));
    }

    #[test]
    fn splitting_does_not_depend_on_read_sizes() {
        let expected: Vec<String> = chunks(TWO.as_bytes())
            .iter()
            .map(|g| text(g).to_owned())
            .collect();
        for n in [1, 2, 3, 7, 64] {
            let got: Vec<String> = GameChunker::new(Trickle(TWO.as_bytes(), n), 1 << 20)
                .map(|g| text(&g.expect("read")).to_owned())
                .collect();
            assert_eq!(got, expected, "read size {n}");
        }
    }

    #[test]
    fn header_line_ends_a_game_without_result() {
        let games = chunks(b"[Event \"A\"]\n\n1. e4 e5\n[Event \"B\"]\n\n1. d4 *\n");
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].end, Termination::NextGameHeader);
        assert_eq!(text(&games[0]), "[Event \"A\"]\n\n1. e4 e5\n");
        assert_eq!(games[1].end, Termination::ResultToken);
    }

    #[test]
    fn bracket_in_comment_does_not_cut() {
        let pgn = "[Event \"A\"]\n\n1. e4 {line\n[%clk 0:01:00]\nmore} e5 ; x\n*\n";
        let games = chunks(pgn.as_bytes());
        assert_eq!(games.len(), 1);
        assert_eq!(text(&games[0]), pgn.trim_end());
    }

    #[test]
    fn percent_line_and_semicolon_are_comments() {
        let pgn = "[Event \"A\"]\n\n1. e4\n% [Event \"X\"]\ne5 *\n";
        assert_eq!(chunks(pgn.as_bytes()).len(), 1);
    }

    #[test]
    fn result_inside_comment_is_not_a_result() {
        let pgn = "[Event \"A\"]\n\n1. e4 {1-0 *} e5 *\n";
        let games = chunks(pgn.as_bytes());
        assert_eq!(text(&games[0]), pgn.trim_end());
    }

    #[test]
    fn headers_without_movetext_between_them_are_one_game() {
        let games = chunks(b"[Event \"A\"]\n[Event \"B\"]\n\n1. e4 *\n");
        assert_eq!(games.len(), 1);
    }

    #[test]
    fn header_without_blank_line_before_moves() {
        let games = chunks(b"[Event \"A\"]\n1. e4 e5 *\n[Event \"B\"]\n1. d4 *\n");
        assert_eq!(games.len(), 2);
    }

    #[test]
    fn header_only_at_eof_is_a_game() {
        let games = chunks(b"[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n");
        assert_eq!(games.len(), 2);
        assert_eq!(games[1].end, Termination::EndOfInput);
        assert_eq!(text(&games[1]), "[Event \"B\"]\n");
    }

    #[test]
    fn missing_final_newline_and_result() {
        let games = chunks(b"[Event \"A\"]\n\n1. e4 e5");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].end, Termination::EndOfInput);
        assert_eq!(text(&games[0]), "[Event \"A\"]\n\n1. e4 e5");
    }

    #[test]
    fn junk_between_games_is_kept_apart() {
        let pgn = "garbage at the start\n[Event \"A\"]\n\n1. e4 *\nnoise\n\n[Event \"B\"]\n\n*\n";
        let games = chunks(pgn.as_bytes());
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].junk, b"garbage at the start\n");
        assert_eq!(games[0].start_line(), 2);
        assert_eq!(games[1].junk, b"\nnoise\n\n");
        assert_eq!(games[1].start_line(), 7);
        assert_eq!(games[1].junk_line, 4);
    }

    #[test]
    fn trailing_junk_without_header_gives_no_game() {
        let games = chunks(b"[Event \"A\"]\n\n1. e4 *\nthe end\n");
        assert_eq!(games.len(), 1);
        assert!(chunks(b"just text\nand more\n").is_empty());
        assert!(chunks(b"").is_empty());
        assert!(chunks(b"  \n\n").is_empty());
    }

    #[test]
    fn bom_is_dropped_and_counts_in_offsets() {
        let mut data = vec![0xef, 0xbb, 0xbf];
        data.extend_from_slice(TWO.as_bytes());
        let games = chunks(&data);
        assert_eq!(games[0].offset(), 3);
        assert_eq!(games[0].start_line(), 1);
        assert!(games[0].bytes().starts_with(b"[Event"));
        assert_eq!(games[0].fallback, None);
    }

    #[test]
    fn crlf_cr_and_lf_give_the_same_games_and_lines() {
        let lf = TWO.to_owned();
        let crlf = TWO.replace('\n', "\r\n");
        let cr = TWO.replace('\n', "\r");
        for data in [&lf, &crlf, &cr] {
            let games = chunks(data.as_bytes());
            assert_eq!(games.len(), 2, "{data:?}");
            assert_eq!(games[1].start_line(), 6, "{data:?}");
        }
    }

    #[test]
    fn indented_header_and_percent_only_at_column_zero() {
        let pgn = "  [Event \"A\"]\n\n1. e4\n  [Event \"B\"]\n\n2. e5 *\n";
        let games = chunks(pgn.as_bytes());
        // The indented `[` at the start of a line still starts a header.
        assert_eq!(games.len(), 2);
    }

    #[test]
    fn gzip_input_is_decompressed() {
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(TWO.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();
        let games = chunks(&gz);
        assert_eq!(games.len(), 2);
        assert_eq!(games[1].start_line(), 6);
    }

    #[test]
    fn corrupt_gzip_is_a_fatal_error_after_the_complete_games() {
        let mut enc = GzEncoder::new(Vec::new(), Compression::none());
        let mut pgn = TWO.to_owned();
        pgn.push_str(&"{x}".repeat(50_000));
        enc.write_all(pgn.as_bytes()).unwrap();
        let mut gz = enc.finish().unwrap();
        gz.truncate(gz.len() - 4000);
        let mut it = GameChunker::new(&gz[..], 1 << 20);
        assert!(it.next().expect("first").is_ok());
        let err = it.find_map(Result::err).expect("an error");
        assert!(matches!(err, PgnError::Gzip { .. }), "{err:?}");
    }

    #[test]
    fn oversize_game_is_reported_and_skipped_to_next_event() {
        let big = "x ".repeat(5000);
        let pgn = format!(
            "[Event \"small\"]\n\n1. e4 *\n[Event \"huge\"]\n\n1. d4 {{{big}}}\n2. d5\n[Event \"after\"]\n\n1. c4 *\n"
        );
        let games: Vec<_> = GameChunker::new(pgn.as_bytes(), 2000)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(games.len(), 3, "{games:#?}");
        assert_eq!(games[1].end, Termination::Oversize);
        assert_eq!(games[1].number(), 2);
        assert_eq!(games[1].start_line(), 4);
        assert!(games[1].bytes().starts_with(b"[Event \"huge\"]"));
        let after = pgn.find("[Event \"after\"]").unwrap() as u64;
        assert_eq!(games[1].offset() + games[1].span_len, after);
        assert_eq!(text(&games[2]), "[Event \"after\"]\n\n1. c4 *");
        assert_eq!(games[2].number(), 3);
        assert_eq!(games[2].start_line(), 8);
    }

    #[test]
    fn oversize_at_end_of_input() {
        let pgn = format!("[Event \"huge\"]\n\n{}", "e4 ".repeat(2000));
        let games: Vec<_> = GameChunker::new(pgn.as_bytes(), 1000)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].end, Termination::Oversize);
        assert_eq!(games[0].span_len, pgn.len() as u64);
    }

    #[test]
    fn huge_single_line_is_linear_and_bounded() {
        // 3 MiB without any delimiter: cut into oversize with a small limit.
        let pgn = format!(
            "[Event \"A\"]\n\n{}\n[Event \"B\"]\n\n*\n",
            "e".repeat(3 << 20)
        );
        let games: Vec<_> = GameChunker::new(pgn.as_bytes(), 1 << 20)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].end, Termination::Oversize);
    }

    #[test]
    fn game_larger_than_one_read_buffer_is_whole() {
        let moves = "e4 ".repeat(500_000);
        let pgn = format!("[Event \"A\"]\n\n{moves}*\n[Event \"B\"]\n\n*\n");
        let games: Vec<_> = GameChunker::new(pgn.as_bytes(), 8 << 20)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(
            games[0].bytes().len(),
            "[Event \"A\"]\n\n".len() + moves.len() + 1
        );
    }

    #[test]
    fn fallback_encoding_defaults_to_windows_1252() {
        let mut c = GameChunker::new(&b""[..], 1000);
        assert_eq!(c.fallback_encoding(), WINDOWS_1252);
        assert!(c.next().is_none());
    }

    #[test]
    fn latin1_text_is_detected_and_reported_once() {
        let mut data = Vec::new();
        for i in 0..2 {
            data.extend_from_slice(b"[Event \"A\"]\n[White \"Mu");
            data.push(0xf1); // n tilde in Latin-1
            data.extend_from_slice(b"oz, Jos");
            data.push(0xe9);
            data.extend_from_slice(format!("\"]\n\n1. e4 *\n\n[Site \"{i}\"]\n").as_bytes());
        }
        let games = chunks(&data);
        assert!(games.len() >= 2);
        assert!(games[0].report_fallback);
        assert!(!games[1].report_fallback);
        assert_eq!(games[0].fallback, Some(WINDOWS_1252));
    }

    /// Produces `games` copies of a small game without holding them.
    struct Endless {
        left: usize,
        pending: &'static [u8],
    }

    impl Read for Endless {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            const GAME: &[u8] = b"[Event \"x\"]\n[Site \"y\"]\n\n1. e4 e5 {c} 2. Nf3 Nc6 1-0\n\n";
            let mut n = 0;
            while n < out.len() {
                if self.pending.is_empty() {
                    if self.left == 0 {
                        break;
                    }
                    self.left -= 1;
                    self.pending = GAME;
                }
                let k = self.pending.len().min(out.len() - n);
                out[n..n + k].copy_from_slice(&self.pending[..k]);
                self.pending = &self.pending[k..];
                n += k;
            }
            Ok(n)
        }
    }

    #[test]
    fn memory_stays_bounded_while_streaming_many_games() {
        let mut chunker = GameChunker::new(
            Endless {
                left: 600_000,
                pending: b"",
            },
            64 << 20,
        );
        let mut games = 0u64;
        let mut peak = 0;
        while let Some(game) = chunker.next() {
            game.expect("no error");
            games += 1;
            peak = peak.max(chunker.buf.capacity());
        }
        assert_eq!(games, 600_000);
        // ~25 MB streamed; the buffer never holds more than a few reads.
        assert!(peak <= 4 * CHUNK, "peak buffer {peak}");
    }
}
