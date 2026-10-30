//! Typed NAG shortcuts (`!` Enter, `+-`, `=+`...), after the Scid's
//! `shortcutAddNag` bindings (`tcl/end.tcl:1753-1777`).
//!
//! The Scid binds key *sequences* and lets Tk match them by prefix. Here a
//! [`NagShortcutBuffer`] collects the keys. It holds no clock: the caller
//! passes the time of each key, so the 1.5 s time-out is testable.
//!
//! Sequences that need `Enter` (`!`, `!!`, `?`, `??`, `!?`, `?!`, `=`, `~`,
//! `~=`) are the ones that are a prefix of another (`!` of `!!`) or of a
//! move; the others (`+-`, `+/`, `+=`, `-+`, `-/`, `=+`) fire as soon as the
//! last key is typed.
//!
//! Any character may be fed to the buffer: those that start no sequence come
//! straight back as [`NagStep::Pass`], so the front-end can route every
//! typed character through it when the board has the focus.
//!
//! Spec: `docs/specs/m2-4-notacao-e-dialogos.md` (D181).

use std::time::{Duration, Instant};

use mychess_core::Nag;

/// How long a half-typed sequence waits for the next key.
pub const NAG_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(1500);

/// `(keys, needs Enter, NAG number)`.
const SEQUENCES: &[(&str, bool, u16)] = &[
    ("!", true, 1),
    ("!!", true, 3),
    ("?", true, 2),
    ("??", true, 4),
    ("!?", true, 5),
    ("?!", true, 6),
    ("=", true, 10),
    ("~", true, 13),
    ("~=", true, 44),
    ("+-", false, 18),
    ("+/", false, 16),
    ("+=", false, 14),
    ("-+", false, 19),
    ("-/", false, 17),
    ("=+", false, 15),
];

/// A key that may belong to a NAG sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NagKey {
    /// A character typed (the text of the key event, not the key code).
    Char(char),
    /// Enter / Return.
    Enter,
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NagStep {
    /// The key starts or continues a sequence; it was consumed.
    Pending,
    /// A sequence is complete: add this NAG.
    Fired(Nag),
    /// The key does not belong to a sequence. `chars` are the keys held back
    /// so far plus this one, to be handled as ordinary keys (`--` is the
    /// null move); `enter` tells that the key was an Enter to be handled
    /// after them.
    Pass {
        /// Characters to replay as ordinary keys, in order.
        chars: String,
        /// An Enter follows the characters.
        enter: bool,
    },
}

/// Collects the keys of a NAG sequence. See the module documentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NagShortcutBuffer {
    keys: String,
    last: Option<Instant>,
}

impl NagShortcutBuffer {
    /// An empty buffer.
    pub fn new() -> NagShortcutBuffer {
        NagShortcutBuffer::default()
    }

    /// Whether a sequence is half typed.
    pub fn is_pending(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The keys held back so far.
    pub fn keys(&self) -> &str {
        &self.keys
    }

    /// When the half-typed sequence times out, for the front-end's timer.
    pub fn expires_at(&self) -> Option<Instant> {
        self.last
            .filter(|_| self.is_pending())
            .map(|last| last + NAG_SEQUENCE_TIMEOUT)
    }

    /// If the half-typed sequence timed out at `now`, empties the buffer and
    /// returns the keys, which the caller handles as ordinary keys. Call it
    /// before [`push`](Self::push) and from the timer.
    pub fn expire(&mut self, now: Instant) -> Option<String> {
        let timed_out = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) >= NAG_SEQUENCE_TIMEOUT);
        if timed_out && self.is_pending() {
            self.last = None;
            Some(std::mem::take(&mut self.keys))
        } else {
            None
        }
    }

    /// Empties the buffer (any other key was pressed) and returns what was
    /// held back.
    pub fn flush(&mut self) -> String {
        self.last = None;
        std::mem::take(&mut self.keys)
    }

    /// Feeds one key typed at `now`. A sequence left over from before the
    /// time-out is dropped; use [`expire`](Self::expire) first to get its
    /// keys back.
    pub fn push(&mut self, key: NagKey, now: Instant) -> NagStep {
        drop(self.expire(now));
        match key {
            NagKey::Enter => {
                let nag = SEQUENCES
                    .iter()
                    .find(|(keys, needs_enter, _)| *needs_enter && *keys == self.keys)
                    .and_then(|&(_, _, number)| Nag::new(number).ok());
                match nag {
                    Some(nag) => {
                        self.flush();
                        NagStep::Fired(nag)
                    }
                    None => NagStep::Pass {
                        chars: self.flush(),
                        enter: true,
                    },
                }
            }
            NagKey::Char(ch) => {
                let mut candidate = self.keys.clone();
                candidate.push(ch);
                let complete = SEQUENCES
                    .iter()
                    .find(|(keys, needs_enter, _)| !*needs_enter && *keys == candidate)
                    .and_then(|&(_, _, number)| Nag::new(number).ok());
                if let Some(nag) = complete {
                    self.flush();
                    NagStep::Fired(nag)
                } else if SEQUENCES
                    .iter()
                    .any(|(keys, _, _)| keys.starts_with(&candidate))
                {
                    self.keys = candidate;
                    self.last = Some(now);
                    NagStep::Pending
                } else {
                    self.last = None;
                    let mut chars = std::mem::take(&mut self.keys);
                    chars.push(ch);
                    NagStep::Pass {
                        chars,
                        enter: false,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nag(n: u16) -> Nag {
        Nag::new(n).unwrap()
    }

    /// Types `keys` (`\n` is Enter) one millisecond apart; the last step.
    fn type_keys(buffer: &mut NagShortcutBuffer, start: Instant, keys: &str) -> NagStep {
        let mut step = NagStep::Pending;
        for (i, ch) in keys.chars().enumerate() {
            let at = start + Duration::from_millis(u64::try_from(i).unwrap());
            let key = if ch == '\n' {
                NagKey::Enter
            } else {
                NagKey::Char(ch)
            };
            step = buffer.push(key, at);
        }
        step
    }

    #[test]
    fn every_sequence_of_the_scid_fires() {
        let now = Instant::now();
        for (keys, needs_enter, number) in SEQUENCES {
            let mut buffer = NagShortcutBuffer::new();
            let typed = if *needs_enter {
                format!("{keys}\n")
            } else {
                (*keys).to_owned()
            };
            assert_eq!(
                type_keys(&mut buffer, now, &typed),
                NagStep::Fired(nag(*number)),
                "{keys}"
            );
            assert!(!buffer.is_pending());
        }
    }

    #[test]
    fn symbols_without_enter_wait() {
        let mut buffer = NagShortcutBuffer::new();
        let now = Instant::now();
        assert_eq!(type_keys(&mut buffer, now, "!"), NagStep::Pending);
        assert_eq!(buffer.keys(), "!");
        // `=` waits for Enter, but `=+` is complete without it.
        let mut buffer = NagShortcutBuffer::new();
        assert_eq!(type_keys(&mut buffer, now, "="), NagStep::Pending);
        assert_eq!(type_keys(&mut buffer, now, "\n"), NagStep::Fired(nag(10)));
        assert_eq!(type_keys(&mut buffer, now, "=+"), NagStep::Fired(nag(15)));
    }

    #[test]
    fn enter_without_a_sequence_is_an_ordinary_key() {
        let mut buffer = NagShortcutBuffer::new();
        let now = Instant::now();
        assert_eq!(
            buffer.push(NagKey::Enter, now),
            NagStep::Pass {
                chars: String::new(),
                enter: true
            }
        );
        // `+` is only the start of a sequence: Enter does not complete it.
        buffer.push(NagKey::Char('+'), now);
        assert_eq!(
            buffer.push(NagKey::Enter, now),
            NagStep::Pass {
                chars: "+".to_owned(),
                enter: true
            }
        );
    }

    #[test]
    fn two_minuses_are_not_a_nag() {
        let mut buffer = NagShortcutBuffer::new();
        let now = Instant::now();
        assert_eq!(buffer.push(NagKey::Char('-'), now), NagStep::Pending);
        assert_eq!(
            buffer.push(NagKey::Char('-'), now),
            NagStep::Pass {
                chars: "--".to_owned(),
                enter: false
            }
        );
        assert!(!buffer.is_pending());
    }

    #[test]
    fn another_key_empties_the_buffer() {
        let mut buffer = NagShortcutBuffer::new();
        let now = Instant::now();
        buffer.push(NagKey::Char('!'), now);
        assert_eq!(
            buffer.push(NagKey::Char('e'), now),
            NagStep::Pass {
                chars: "!e".to_owned(),
                enter: false
            }
        );
        // A key outside every sequence passes at once.
        assert_eq!(
            buffer.push(NagKey::Char('x'), now),
            NagStep::Pass {
                chars: "x".to_owned(),
                enter: false
            }
        );
        // `flush` is for non-character keys.
        buffer.push(NagKey::Char('?'), now);
        assert_eq!(buffer.flush(), "?");
        assert!(!buffer.is_pending());
    }

    #[test]
    fn half_typed_sequences_time_out() {
        let mut buffer = NagShortcutBuffer::new();
        let start = Instant::now();
        buffer.push(NagKey::Char('+'), start);
        assert_eq!(buffer.expires_at(), Some(start + NAG_SEQUENCE_TIMEOUT));
        assert_eq!(buffer.expire(start + Duration::from_millis(1499)), None);
        assert_eq!(
            buffer.expire(start + Duration::from_millis(1500)),
            Some("+".to_owned())
        );
        assert!(!buffer.is_pending());
        assert_eq!(buffer.expires_at(), None);
        // A late second key starts over instead of completing `+-`.
        buffer.push(NagKey::Char('+'), start);
        let late = start + Duration::from_secs(3);
        assert_eq!(buffer.push(NagKey::Char('-'), late), NagStep::Pending);
        assert_eq!(buffer.keys(), "-");
    }

    #[test]
    fn exclusive_prefixes_do_not_swallow_longer_sequences() {
        let mut buffer = NagShortcutBuffer::new();
        let now = Instant::now();
        assert_eq!(type_keys(&mut buffer, now, "!!\n"), NagStep::Fired(nag(3)));
        assert_eq!(type_keys(&mut buffer, now, "?!\n"), NagStep::Fired(nag(6)));
        assert_eq!(type_keys(&mut buffer, now, "~=\n"), NagStep::Fired(nag(44)));
    }
}
