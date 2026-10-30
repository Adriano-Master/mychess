//! Reader options: the "tolerant Scid profile" and its switches.

use mychess_core::SanMode;

/// What to do with text found between the end of a game and the next header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PreGameText {
    /// Drop it; an informative `JunkBetweenGames` diagnostic is recorded
    /// (Scid's import dialog and `sc_base import`).
    #[default]
    Ignore,
    /// Keep it as the initial comment of the next game (Scid's `pgnscid`).
    AsComment,
}

/// How player names are normalised (Scid's `standardPlayerName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PlayerNames {
    /// One space after each comma, no trailing blanks, `van der` prefixes in
    /// a single spelling.
    #[default]
    Scid,
    /// Keep the name as written (an `(dddd)` rating suffix is still read).
    AsIs,
}

/// What happens to white space inside comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CommentWhitespace {
    /// Runs of white space (line breaks included) become one space and the
    /// ends are trimmed (Scid's `strSingleSpace`).
    #[default]
    Collapse,
    /// Keep the text; only line breaks are normalised to `\n`.
    Preserve,
}

/// What happens to `;` comments (to the end of the line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SemicolonComments {
    /// Keep them as ordinary comments (the Scid drops them).
    #[default]
    Keep,
    /// Drop them, as the Scid does.
    Drop,
}

/// What to do with games of a variant that is not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum UnsupportedVariant {
    /// Return only the header, with an `UnsupportedVariant` diagnostic.
    #[default]
    Skip,
    /// Read the moves as standard chess anyway.
    TryStandard,
}

/// Switches of the PGN reader. [`ReadOptions::default`] is the "tolerant
/// Scid profile".
///
/// The struct can be built with a literal; name the fields you change and
/// finish with `..Default::default()` so that new options do not break the
/// code. The `with_*` methods are an alternative for the limits.
///
/// # Memory
///
/// A node of the game tree costs about 350 bytes (the move, the position
/// bookkeeping and the annotations), so a game of `max_nodes_per_game` nodes
/// needs a few hundred MB; lower the limit for untrusted input. The raw
/// bytes of a game (up to `max_game_bytes`) are held in memory as well.
///
/// ```
/// use mychess_pgn::read::ReadOptions;
///
/// let opts = ReadOptions {
///     max_nodes_per_game: 10_000,
///     ..Default::default()
/// };
/// assert_eq!(opts, ReadOptions::default().with_max_nodes_per_game(10_000));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOptions {
    /// Text between games, see [`PreGameText`].
    pub pre_game_text: PreGameText,
    /// Player name normalisation, see [`PlayerNames`].
    pub player_names: PlayerNames,
    /// White space inside comments, see [`CommentWhitespace`].
    pub comment_whitespace: CommentWhitespace,
    /// `;` comments, see [`SemicolonComments`].
    pub semicolon_comments: SemicolonComments,
    /// SAN strictness; [`SanMode::Tolerant`] by default.
    pub san_mode: SanMode,
    /// Tags (exact names) that are read and dropped.
    pub ignored_tags: Vec<String>,
    /// Longest comment kept, in bytes (1 MiB); longer ones are truncated with
    /// a `CommentTruncated` warning.
    pub max_comment_len: usize,
    /// Largest game accepted, in bytes (16 MiB); a larger one is skipped up
    /// to the next `[Event ` line with a `GameTooLarge` diagnostic.
    pub max_game_bytes: usize,
    /// Most moves (tree nodes) read for one game (1 000 000, about 350 MB of
    /// memory, see the type documentation). At the limit the reading of the
    /// moves stops with a `TooManyNodes` diagnostic and
    /// [`Termination::NodeLimit`](super::Termination::NodeLimit).
    pub max_nodes_per_game: usize,
    /// Most tags without a dedicated field (`Annotator`, `Opening`...) kept
    /// for one game (256); the rest are dropped with one `TooManyTags`
    /// warning. Keeps the work per game linear.
    pub max_tags_per_game: usize,
    /// Diagnostics kept per game (100); one `TooManyDiagnostics` follows.
    pub max_diagnostics_per_game: usize,
    /// Games of an unsupported variant, see [`UnsupportedVariant`].
    pub unsupported_variant: UnsupportedVariant,
}

impl Default for ReadOptions {
    fn default() -> ReadOptions {
        ReadOptions {
            pre_game_text: PreGameText::default(),
            player_names: PlayerNames::default(),
            comment_whitespace: CommentWhitespace::default(),
            semicolon_comments: SemicolonComments::default(),
            san_mode: SanMode::Tolerant,
            ignored_tags: Vec::new(),
            max_comment_len: 1 << 20,
            max_game_bytes: 16 << 20,
            max_nodes_per_game: 1_000_000,
            max_tags_per_game: 256,
            max_diagnostics_per_game: 100,
            unsupported_variant: UnsupportedVariant::default(),
        }
    }
}

impl ReadOptions {
    /// Sets [`max_comment_len`](ReadOptions::max_comment_len).
    #[must_use]
    pub fn with_max_comment_len(mut self, bytes: usize) -> ReadOptions {
        self.max_comment_len = bytes;
        self
    }

    /// Sets [`max_game_bytes`](ReadOptions::max_game_bytes).
    #[must_use]
    pub fn with_max_game_bytes(mut self, bytes: usize) -> ReadOptions {
        self.max_game_bytes = bytes;
        self
    }

    /// Sets [`max_nodes_per_game`](ReadOptions::max_nodes_per_game).
    #[must_use]
    pub fn with_max_nodes_per_game(mut self, nodes: usize) -> ReadOptions {
        self.max_nodes_per_game = nodes;
        self
    }

    /// Sets [`max_tags_per_game`](ReadOptions::max_tags_per_game).
    #[must_use]
    pub fn with_max_tags_per_game(mut self, tags: usize) -> ReadOptions {
        self.max_tags_per_game = tags;
        self
    }

    /// Sets [`max_diagnostics_per_game`](ReadOptions::max_diagnostics_per_game).
    #[must_use]
    pub fn with_max_diagnostics_per_game(mut self, count: usize) -> ReadOptions {
        self.max_diagnostics_per_game = count;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_tolerant_scid_profile() {
        let o = ReadOptions::default();
        assert_eq!(o.pre_game_text, PreGameText::Ignore);
        assert_eq!(o.player_names, PlayerNames::Scid);
        assert_eq!(o.comment_whitespace, CommentWhitespace::Collapse);
        assert_eq!(o.semicolon_comments, SemicolonComments::Keep);
        assert_eq!(o.san_mode, SanMode::Tolerant);
        assert_eq!(o.ignored_tags, Vec::<String>::new());
        assert_eq!(o.max_comment_len, 1 << 20);
        assert_eq!(o.max_game_bytes, 16 << 20);
        assert_eq!(o.max_nodes_per_game, 1_000_000);
        assert_eq!(o.max_tags_per_game, 256);
        assert_eq!(o.max_diagnostics_per_game, 100);
        assert_eq!(o.unsupported_variant, UnsupportedVariant::Skip);
    }
}
