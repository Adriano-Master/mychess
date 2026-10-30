//! Cutting a byte stream into lines (spec m8-10, D2706).

/// The longest line kept; the rest of a longer line is dropped.
pub const MAX_LINE_BYTES: usize = 4096;

/// A line cut from the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The text: lossy UTF-8, NUL bytes removed, trimmed.
    pub text: String,
    /// The line was longer than [`MAX_LINE_BYTES`].
    pub truncated: bool,
}

/// Cuts bytes into lines at `\n` and at `\r` (a board may send CR, LF or
/// CRLF). Empty lines are skipped. Any bytes are tolerated.
#[derive(Debug, Default)]
pub struct LineSplitter {
    buf: Vec<u8>,
    overflow: bool,
}

impl LineSplitter {
    /// An empty splitter.
    pub fn new() -> LineSplitter {
        LineSplitter::default()
    }

    fn push(&mut self, byte: u8) {
        if self.buf.len() < MAX_LINE_BYTES {
            self.buf.push(byte);
        } else {
            self.overflow = true;
        }
    }

    fn take(&mut self) -> Option<Line> {
        let truncated = std::mem::take(&mut self.overflow);
        let bytes = std::mem::take(&mut self.buf);
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        text.retain(|c| c != '\0');
        let text = text.trim().to_owned();
        (!text.is_empty()).then_some(Line { text, truncated })
    }

    /// Feeds a chunk; complete lines are appended to `out`.
    pub fn feed(&mut self, chunk: &[u8], out: &mut Vec<Line>) {
        for &byte in chunk {
            if byte == b'\n' || byte == b'\r' {
                out.extend(self.take());
            } else {
                self.push(byte);
            }
        }
    }

    /// The unterminated last line at the end of the input, if any.
    pub fn finish(&mut self) -> Option<Line> {
        self.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(chunks: &[&[u8]]) -> Vec<String> {
        let mut splitter = LineSplitter::new();
        let mut out = Vec::new();
        for chunk in chunks {
            splitter.feed(chunk, &mut out);
        }
        out.extend(splitter.finish());
        out.into_iter().map(|l| l.text).collect()
    }

    #[test]
    fn accepts_lf_cr_and_crlf_and_skips_empty_lines() {
        assert_eq!(lines(&[b"a\nb\r\nc\rd\n\n\r\n"]), ["a", "b", "c", "d"]);
    }

    #[test]
    fn joins_lines_split_across_chunks_and_keeps_the_tail() {
        assert_eq!(lines(&[b"M 1 e2", b"-e4\r", b"T"]), ["M 1 e2-e4", "T"]);
    }

    #[test]
    fn tolerates_garbage_bytes_and_nul() {
        let got = lines(&[b"ab\x00c\xff\xfe\n"]);
        assert_eq!(got.len(), 1);
        assert!(got[0].starts_with("abc"));
    }

    #[test]
    fn long_lines_are_cut_and_flagged() {
        let mut splitter = LineSplitter::new();
        let mut out = Vec::new();
        splitter.feed(&vec![b'x'; MAX_LINE_BYTES * 3], &mut out);
        splitter.feed(b"\nok\n", &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text.len(), MAX_LINE_BYTES);
        assert!(out[0].truncated);
        assert!(!out[1].truncated);
    }
}
