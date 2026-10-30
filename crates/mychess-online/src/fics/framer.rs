//! Telnet-style line framing for the FICS stream (spec m8-9, D2603).
//!
//! The server speaks plain text over TCP with a trace of telnet: option
//! negotiation (`IAC WILL ...`), CR LF line ends and prompts that are never
//! terminated (`login: `). [`LineFramer`] is pure (no I/O): it takes the
//! byte chunks as the network produced them and yields complete lines, drops
//! the telnet commands and the control characters, and never fails: a line
//! longer than [`MAX_LINE_LEN`] is cut and the rest of it is thrown away
//! (D2604).

/// Longest line kept, in bytes. `who` and the games list can be long once
/// the wrapping is off (`iset nowrap 1`), but never this long.
pub const MAX_LINE_LEN: usize = 64 * 1024;

const IAC: u8 = 255;
const SB: u8 = 250;
const SE: u8 = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Telnet {
    #[default]
    Data,
    /// After `IAC`.
    Command,
    /// After `IAC WILL|WONT|DO|DONT`: one option byte follows.
    Option,
    /// Inside `IAC SB ... IAC SE`.
    Sub,
    /// Inside a sub-negotiation, after an `IAC`.
    SubIac,
}

/// Splits the bytes of the connection into lines.
#[derive(Debug)]
pub struct LineFramer {
    buf: Vec<u8>,
    state: Telnet,
    max_len: usize,
    /// The current line already hit the limit: bytes up to the next line
    /// end are dropped.
    overflow: bool,
}

impl Default for LineFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineFramer {
    /// Framer with the standard limit.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_len(MAX_LINE_LEN)
    }

    /// Framer with a custom limit (tests).
    #[must_use]
    pub fn with_max_len(max_len: usize) -> Self {
        Self {
            buf: Vec::new(),
            state: Telnet::Data,
            max_len: max_len.max(1),
            overflow: false,
        }
    }

    /// Feeds a chunk and returns the lines it completed, in order.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in chunk {
            match self.state {
                Telnet::Data => match byte {
                    IAC => self.state = Telnet::Command,
                    b'\n' => {
                        lines.push(decode(&self.buf));
                        self.buf.clear();
                        self.overflow = false;
                    }
                    _ => self.data(byte),
                },
                Telnet::Command => match byte {
                    // `IAC IAC` is a literal 0xFF.
                    IAC => {
                        self.data(IAC);
                        self.state = Telnet::Data;
                    }
                    251..=254 => self.state = Telnet::Option,
                    SB => self.state = Telnet::Sub,
                    _ => self.state = Telnet::Data,
                },
                Telnet::Option => self.state = Telnet::Data,
                Telnet::Sub => {
                    if byte == IAC {
                        self.state = Telnet::SubIac;
                    }
                }
                Telnet::SubIac => {
                    // `IAC IAC` inside the data of the option stays inside.
                    self.state = if byte == SE {
                        Telnet::Data
                    } else {
                        Telnet::Sub
                    };
                }
            }
        }
        lines
    }

    fn data(&mut self, byte: u8) {
        if self.overflow {
            return;
        }
        if self.buf.len() >= self.max_len {
            self.overflow = true;
            return;
        }
        self.buf.push(byte);
    }

    /// The text of the line in progress (a prompt such as `login: ` has no
    /// line end).
    #[must_use]
    pub fn partial(&self) -> String {
        decode(&self.buf)
    }

    /// Forgets the line in progress (a prompt that was answered).
    pub fn clear_partial(&mut self) {
        self.buf.clear();
        self.overflow = false;
    }
}

/// UTF-8 when valid, else Latin-1 (old servers and players); control
/// characters other than the tab are dropped (a bell, an escape).
fn decode(bytes: &[u8]) -> String {
    let text: String = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    };
    text.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_across_chunks_and_strips_crlf() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"hel"), Vec::<String>::new());
        assert_eq!(f.push(b"lo\r\nwor"), vec!["hello"]);
        assert_eq!(f.partial(), "wor");
        assert_eq!(f.push(b"ld\r\n\r\n"), vec!["world", ""]);
    }

    #[test]
    fn telnet_commands_are_dropped_even_when_split() {
        let mut f = LineFramer::new();
        let mut out = f.push(&[b'a', IAC, 251]);
        out.extend(f.push(&[1, b'b', IAC, SB, 24, 1, IAC]));
        out.extend(f.push(&[SE, b'c', b'\n']));
        assert_eq!(out, vec!["abc"]);
        assert_eq!(f.push(&[b'x', IAC, IAC, b'\n']), vec!["x\u{ff}"]);
    }

    #[test]
    fn partial_shows_an_unterminated_prompt() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"login: "), Vec::<String>::new());
        assert_eq!(f.partial(), "login: ");
        f.clear_partial();
        assert_eq!(f.partial(), "");
    }

    #[test]
    fn long_lines_are_cut_and_the_rest_is_dropped() {
        let mut f = LineFramer::with_max_len(8);
        assert_eq!(f.push(b"0123456789"), Vec::<String>::new());
        assert_eq!(f.push(b"abc\nnext\n"), vec!["01234567", "next"]);
    }

    #[test]
    fn invalid_utf8_falls_back_to_latin1_and_controls_go() {
        let mut f = LineFramer::new();
        assert_eq!(f.push(b"caf\xe9\x07 ok\x1b[0m\n"), vec!["caf\u{e9} ok[0m"]);
    }

    #[test]
    fn any_bytes_never_panic() {
        let mut f = LineFramer::with_max_len(16);
        let junk: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        for chunk in junk.chunks(7) {
            f.push(chunk);
        }
    }
}
