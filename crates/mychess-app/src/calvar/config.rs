//! The configuration of a calculation exercise: what the dialog edits (spec
//! M8-6, D2301-D2308).

/// Where the position of the exercise comes from (D2301).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PositionSource {
    /// The position at the cursor of the game in the editor.
    #[default]
    Current,
    /// A FEN typed or pasted in the dialog.
    Fen,
}

impl PositionSource {
    /// The spelling used in `calvar.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            PositionSource::Current => "current",
            PositionSource::Fen => "fen",
        }
    }

    /// Reads the spelling of [`PositionSource::as_str`].
    pub fn from_name(text: &str) -> Option<PositionSource> {
        match text {
            "current" => Some(PositionSource::Current),
            "fen" => Some(PositionSource::Fen),
            _ => None,
        }
    }
}

/// Lines the user is asked for (D2305).
pub const LINES_RANGE: (u8, u8) = (1, 5);
/// Lines of the engine in the first stage (D2307).
pub const MULTIPV_RANGE: (u8, u8) = (3, 10);
/// Thinking time of the first stage, in milliseconds (D2308). The dialog
/// offers seconds; the floor lets the tests run fast.
pub const ROOT_MS_RANGE: (u64, u64) = (200, 300_000);
/// Thinking time per search of the second stage, in milliseconds (D2308).
pub const LINE_MS_RANGE: (u64, u64) = (200, 120_000);
/// Time limit in seconds, 0 meaning none (D2306).
pub const TIME_LIMIT_RANGE: (u16, u16) = (30, 3_600);
/// Longest FEN text kept.
pub const MAX_FEN_CHARS: usize = 120;

/// What the dialog edits. [`CalvarConfig::sanitized`] brings every field
/// into its range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalvarConfig {
    /// Where the position comes from.
    pub source: PositionSource,
    /// The FEN, when the source is [`PositionSource::Fen`].
    pub fen: String,
    /// The engine (`EngineId` number), `None` for the automatic choice.
    pub engine: Option<u64>,
    /// Lines the user must give before "Done" (D2305).
    pub lines_required: u8,
    /// Lines of the first stage.
    pub multipv: u8,
    /// Time of the first stage, in milliseconds.
    pub root_ms: u64,
    /// Time per search of the second stage, in milliseconds.
    pub line_ms: u64,
    /// Time limit in seconds, 0 for none.
    pub time_limit_s: u16,
    /// The board shows the position being calculated (D2303).
    pub follow_board: bool,
}

impl Default for CalvarConfig {
    fn default() -> CalvarConfig {
        CalvarConfig {
            source: PositionSource::Current,
            fen: String::new(),
            engine: None,
            lines_required: 3,
            multipv: 6,
            root_ms: 30_000,
            line_ms: 10_000,
            time_limit_s: 0,
            follow_board: false,
        }
    }
}

impl CalvarConfig {
    /// The same configuration with every number inside its range.
    #[must_use]
    pub fn sanitized(mut self) -> CalvarConfig {
        self.lines_required = self.lines_required.clamp(LINES_RANGE.0, LINES_RANGE.1);
        self.multipv = self.multipv.clamp(MULTIPV_RANGE.0, MULTIPV_RANGE.1);
        self.root_ms = self.root_ms.clamp(ROOT_MS_RANGE.0, ROOT_MS_RANGE.1);
        self.line_ms = self.line_ms.clamp(LINE_MS_RANGE.0, LINE_MS_RANGE.1);
        if self.time_limit_s != 0 {
            self.time_limit_s = self
                .time_limit_s
                .clamp(TIME_LIMIT_RANGE.0, TIME_LIMIT_RANGE.1);
        }
        if self.fen.chars().count() > MAX_FEN_CHARS {
            self.fen = self.fen.chars().take(MAX_FEN_CHARS).collect();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizing_brings_every_field_into_range() {
        let wild = CalvarConfig {
            lines_required: 0,
            multipv: 99,
            root_ms: 1,
            line_ms: u64::MAX,
            time_limit_s: 5,
            fen: "x".repeat(500),
            ..CalvarConfig::default()
        }
        .sanitized();
        assert_eq!(wild.lines_required, 1);
        assert_eq!(wild.multipv, 10);
        assert_eq!(wild.root_ms, ROOT_MS_RANGE.0);
        assert_eq!(wild.line_ms, LINE_MS_RANGE.1);
        assert_eq!(wild.time_limit_s, TIME_LIMIT_RANGE.0);
        assert_eq!(wild.fen.chars().count(), MAX_FEN_CHARS);
        assert_eq!(CalvarConfig::default().sanitized(), CalvarConfig::default());
    }

    #[test]
    fn the_source_spelling_round_trips() {
        for s in [PositionSource::Current, PositionSource::Fen] {
            assert_eq!(PositionSource::from_name(s.as_str()), Some(s));
        }
        assert_eq!(PositionSource::from_name("x"), None);
    }
}
