//! NDJSON line framing (spec m8-3, D1309).
//!
//! [`LineFramer`] takes the byte chunks of a streaming HTTP body, in whatever
//! sizes the network produced, and yields complete lines. It is pure (no I/O)
//! so the fragmentation cases are tested without a network.

use crate::error::{Error, Result};

/// Maximum accepted line length (1 MiB).
pub const MAX_LINE_LEN: usize = 1 << 20;

/// One framed unit of an NDJSON stream.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Line {
    /// An empty line: the server's keep-alive.
    KeepAlive,
    /// A non-empty line, UTF-8 valid, without the line terminator.
    Text(String),
    /// A non-empty line that is not valid UTF-8 (lossy copy for the log).
    Invalid(String),
}

/// Splits a byte stream into lines at `\n` (accepting `\r\n`).
#[derive(Debug, Default)]
pub struct LineFramer {
    buf: Vec<u8>,
    max_len: usize,
}

impl LineFramer {
    /// Framer with the standard 1 MiB limit.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_len(MAX_LINE_LEN)
    }

    /// Framer with a custom limit (tests).
    #[must_use]
    pub fn with_max_len(max_len: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_len,
        }
    }

    /// Bytes of an incomplete line currently buffered.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// Feeds a chunk and returns the lines it completed, in order.
    ///
    /// # Errors
    /// [`Error::LineTooLong`] when a line (complete or not) exceeds the limit;
    /// the stream must then be abandoned.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Line>> {
        let mut out = Vec::new();
        let mut rest = chunk;
        while let Some(pos) = rest.iter().position(|&b| b == b'\n') {
            let (head, tail) = rest.split_at(pos);
            if self.exceeds(head) {
                return Err(Error::LineTooLong);
            }
            self.buf.extend_from_slice(head);
            out.push(Self::finish(std::mem::take(&mut self.buf)));
            rest = &tail[1..];
        }
        if self.exceeds(rest) {
            return Err(Error::LineTooLong);
        }
        self.buf.extend_from_slice(rest);
        Ok(out)
    }

    /// Whether the buffered bytes plus `tail` are longer than the limit.
    /// A final `\r` does not count: it belongs to the `\r\n` terminator
    /// (D1309: a line is too long only above the limit).
    fn exceeds(&self, tail: &[u8]) -> bool {
        let total = self.buf.len() + tail.len();
        let ends_in_cr = tail.last().or(self.buf.last()) == Some(&b'\r');
        total - usize::from(ends_in_cr) > self.max_len
    }

    /// Flushes an unterminated last line when the stream ends.
    pub fn finish_stream(&mut self) -> Option<Line> {
        if self.buf.is_empty() {
            None
        } else {
            Some(Self::finish(std::mem::take(&mut self.buf)))
        }
    }

    fn finish(mut bytes: Vec<u8>) -> Line {
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        if bytes.is_empty() {
            return Line::KeepAlive;
        }
        match String::from_utf8(bytes) {
            Ok(text) => Line::Text(text),
            Err(e) => Line::Invalid(String::from_utf8_lossy(e.as_bytes()).into_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn text(s: &str) -> Line {
        Line::Text(s.to_owned())
    }

    #[test]
    fn splits_lines_in_one_chunk() {
        let mut f = LineFramer::new();
        let got = f.push(b"{\"a\":1}\n{\"b\":2}\n").unwrap_or_default();
        assert_eq!(got, [text("{\"a\":1}"), text("{\"b\":2}")]);
        assert_eq!(f.pending(), 0);
    }

    #[test]
    fn line_split_between_chunks() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"{\"ty").unwrap_or_default(), []);
        assert_eq!(f.push(b"pe\":\"x\"").unwrap_or_default(), []);
        assert_eq!(
            f.push(b"}\n{").unwrap_or_default(),
            [text("{\"type\":\"x\"}")]
        );
        assert_eq!(f.push(b"}\n").unwrap_or_default(), [text("{}")]);
    }

    #[test]
    fn crlf_and_keepalive() {
        let mut f = LineFramer::new();
        let got = f.push(b"{}\r\n\n\r\n{}\n").unwrap_or_default();
        assert_eq!(
            got,
            [text("{}"), Line::KeepAlive, Line::KeepAlive, text("{}")]
        );
    }

    #[test]
    fn crlf_split_between_chunks() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"{}\r").unwrap_or_default(), []);
        assert_eq!(f.push(b"\n").unwrap_or_default(), [text("{}")]);
    }

    #[test]
    fn invalid_utf8_becomes_invalid_line() {
        let mut f = LineFramer::new();
        let got = f.push(b"\xff\xfe{}\n").unwrap_or_default();
        assert!(matches!(got.as_slice(), [Line::Invalid(_)]));
    }

    #[test]
    fn oversized_line_is_an_error_even_without_newline() {
        let mut f = LineFramer::with_max_len(8);
        assert_eq!(f.push(b"12345678").map(|v| v.len()), Ok(0));
        assert_eq!(f.push(b"9"), Err(Error::LineTooLong));
        let mut g = LineFramer::with_max_len(8);
        assert_eq!(g.push(b"123456789\n"), Err(Error::LineTooLong));
        let mut h = LineFramer::with_max_len(8);
        assert!(h.push(b"12345678\n").is_ok());
    }

    #[test]
    fn unterminated_tail_is_flushed_at_end() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"{}\n{\"x\"").unwrap_or_default(), [text("{}")]);
        assert_eq!(f.finish_stream(), Some(text("{\"x\"")));
        assert_eq!(f.finish_stream(), None);
    }

    proptest! {
        /// Any fragmentation of the byte stream yields the same lines.
        #[test]
        fn fragmentation_does_not_change_the_lines(
            lines in proptest::collection::vec("[ -~]{0,40}", 0..12),
            cuts in proptest::collection::vec(1usize..17, 1..40),
        ) {
            let mut wire = Vec::new();
            for l in &lines {
                wire.extend_from_slice(l.as_bytes());
                wire.extend_from_slice(b"\n");
            }
            let mut whole = LineFramer::new();
            let expected = whole.push(&wire).unwrap_or_default();

            let mut framer = LineFramer::new();
            let mut got = Vec::new();
            let mut pos = 0;
            let mut i = 0;
            while pos < wire.len() {
                let n = cuts[i % cuts.len()].min(wire.len() - pos);
                got.extend(framer.push(&wire[pos..pos + n]).unwrap_or_default());
                pos += n;
                i += 1;
            }
            prop_assert_eq!(got, expected);
        }
    }
}
