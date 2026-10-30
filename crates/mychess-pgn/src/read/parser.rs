//! Parsing of one game: header, start position and the bookkeeping shared
//! with the movetext lexer (`movetext.rs`).

use mychess_core::{FenMode, Game, NodeId, Position, Tags, Variant};

use super::chars::{count_breaks, first_non_space, is_eol};
use super::chunker::RawGame;
use super::diagnostics::{Diagnostic, DiagnosticKind};
use super::header::{
    HeaderItem, HeaderState, TagNote, VariantKind, classify_variant, is_explicit_standard,
    scan_header,
};
use super::options::{CommentWhitespace, PreGameText, ReadOptions, UnsupportedVariant};
use super::parsed::{ParsedGame, Termination};
use super::text::{
    Decoder, clip, collapse_whitespace, cut_for_decode, normalize_newlines, truncate_at_boundary,
};

/// Tags longer than this are truncated (D23).
const MAX_TAG_NAME: usize = 1024;
const MAX_TAG_VALUE: usize = 64 * 1024;
/// Longest text kept in a diagnostic.
pub(super) const MAX_DIAG_TEXT: usize = 80;

/// Maps byte positions to (line, column), moving forward in one pass.
pub(super) struct LineTracker {
    first_line: u64,
    pos: usize,
    line: u64,
    line_start: usize,
}

impl LineTracker {
    pub(super) fn new(first_line: u64) -> LineTracker {
        LineTracker {
            first_line,
            pos: 0,
            line: first_line,
            line_start: 0,
        }
    }

    /// Line and column (1-based; bytes) of `pos` in `data`. Fast when called
    /// with growing positions; a smaller position restarts from the top.
    pub(super) fn locate(&mut self, data: &[u8], pos: usize) -> (u64, u32) {
        let pos = pos.min(data.len());
        if pos < self.pos {
            *self = LineTracker::new(self.first_line);
        }
        for i in self.pos..pos {
            match data[i] {
                b'\n' => {
                    if i == 0 || data[i - 1] != b'\r' {
                        self.line += 1;
                    }
                    self.line_start = i + 1;
                }
                b'\r' => {
                    self.line += 1;
                    self.line_start = i + 1;
                }
                _ => {}
            }
        }
        self.pos = pos;
        let col = u32::try_from(pos - self.line_start + 1).unwrap_or(u32::MAX);
        (self.line, col)
    }
}

/// One line of play being read: the main line or a variation.
pub(super) struct Line {
    /// The node the next move is attached to.
    pub(super) cursor: NodeId,
    /// Whether a move was played in this line.
    pub(super) has_move: bool,
    /// An error ended this line: the rest of it is skipped (D21).
    pub(super) dead: bool,
    /// Depth of parentheses being skipped inside this line.
    pub(super) skip: u32,
    /// Comment found before the first move of a variation. It is dropped if
    /// the variation never gets a move (or its first move is an error).
    pub(super) pending_before: Option<String>,
}

/// Reads one game. `movetext.rs` adds the lexer for the moves.
pub(super) struct GameParser<'a> {
    pub(super) data: &'a [u8],
    pub(super) opts: &'a ReadOptions,
    pub(super) game_no: u64,
    pub(super) base_offset: u64,
    pub(super) tracker: LineTracker,
    pub(super) diags: Vec<Diagnostic>,
    overflow: bool,
    pub(super) decoder: Decoder,
    pub(super) game: Game,
    pub(super) stack: Vec<Line>,
    pub(super) error_commented: bool,
    pub(super) stray_brace: bool,
    /// A limit was hit: the movetext lexer must stop.
    pub(super) stop: bool,
}

impl<'a> GameParser<'a> {
    pub(super) fn new(
        data: &'a [u8],
        opts: &'a ReadOptions,
        game_no: u64,
        first_line: u64,
        base_offset: u64,
        decoder: Decoder,
        game: Game,
    ) -> GameParser<'a> {
        let root = game.root();
        GameParser {
            data,
            opts,
            game_no,
            base_offset,
            tracker: LineTracker::new(first_line),
            diags: Vec::new(),
            overflow: false,
            decoder,
            game,
            stack: vec![Line {
                cursor: root,
                has_move: false,
                dead: false,
                skip: 0,
                pending_before: None,
            }],
            error_commented: false,
            stray_brace: false,
            stop: false,
        }
    }

    /// Records a diagnostic at an explicit place.
    pub(super) fn emit(
        &mut self,
        kind: DiagnosticKind,
        line: u64,
        column: u32,
        offset: u64,
        text: Option<String>,
    ) {
        if self.diags.len() >= self.opts.max_diagnostics_per_game {
            if self.overflow {
                return;
            }
            self.overflow = true;
            self.push(
                DiagnosticKind::TooManyDiagnostics,
                line,
                column,
                offset,
                None,
            );
            return;
        }
        self.push(kind, line, column, offset, text);
    }

    fn push(
        &mut self,
        kind: DiagnosticKind,
        line: u64,
        column: u32,
        offset: u64,
        text: Option<String>,
    ) {
        self.diags.push(Diagnostic {
            severity: kind.severity(),
            kind,
            game: self.game_no,
            line,
            column,
            offset,
            text,
        });
    }

    /// Records a diagnostic at byte `pos` of the game data.
    pub(super) fn diag(&mut self, kind: DiagnosticKind, pos: usize, text: Option<String>) {
        let (line, column) = self.tracker.locate(self.data, pos);
        let offset = self.base_offset + pos as u64;
        self.emit(kind, line, column, offset, text);
    }

    /// Records a diagnostic with a clipped, lossily decoded token.
    pub(super) fn diag_token(&mut self, kind: DiagnosticKind, pos: usize, token: &[u8]) {
        let text = clip(&String::from_utf8_lossy(token), MAX_DIAG_TEXT);
        self.diag(kind, pos, Some(text));
    }

    // ------------------------------------------------------------------
    // Text before the game
    // ------------------------------------------------------------------

    /// Handles the text between the previous game and this one (D27).
    fn pre_game(&mut self, raw: &RawGame) -> Option<String> {
        let junk = raw.junk.as_slice();
        let first = first_non_space(junk)?;
        let mut breaks_prev_cr = false;
        let line = raw.junk_line + count_breaks(&junk[..first], &mut breaks_prev_cr);
        let line_start = junk[..first]
            .iter()
            .rposition(|&b| is_eol(b))
            .map_or(0, |i| i + 1);
        let column = u32::try_from(first - line_start + 1).unwrap_or(u32::MAX);
        let snippet = clip(String::from_utf8_lossy(&junk[first..]).trim_end(), 40);
        self.emit(
            DiagnosticKind::JunkBetweenGames,
            line,
            column,
            raw.junk_offset + first as u64,
            Some(snippet),
        );
        let text = self.comment_string(junk);
        (!text.is_empty()).then_some(text)
    }

    // ------------------------------------------------------------------
    // Header
    // ------------------------------------------------------------------

    fn header_item(&mut self, item: HeaderItem<'_>, header: &mut HeaderState) {
        match item {
            HeaderItem::Tag { name, value, pos } => {
                let mut name = String::from_utf8_lossy(name).into_owned();
                let value_bytes = cut_for_decode(&value, MAX_TAG_VALUE + 3);
                let mut value_text = self.decoder.decode(value_bytes).into_owned();
                let mut too_long = value.len() > MAX_TAG_VALUE;
                if value_text.len() > MAX_TAG_VALUE {
                    truncate_at_boundary(&mut value_text, MAX_TAG_VALUE);
                    too_long = true;
                }
                if name.len() > MAX_TAG_NAME {
                    truncate_at_boundary(&mut name, MAX_TAG_NAME);
                    too_long = true;
                }
                if too_long {
                    self.diag(
                        DiagnosticKind::TagTooLong,
                        pos,
                        Some(clip(&name, MAX_DIAG_TEXT)),
                    );
                }
                match header.apply(&name, value_text, pos, self.opts) {
                    Some(TagNote::RatingTooLarge) => {
                        self.diag(
                            DiagnosticKind::RatingTooLarge,
                            pos,
                            Some(clip(&name, MAX_DIAG_TEXT)),
                        );
                    }
                    Some(TagNote::BadName) => {
                        self.diag(
                            DiagnosticKind::BadTag,
                            pos,
                            Some(clip(&name, MAX_DIAG_TEXT)),
                        );
                    }
                    Some(TagNote::TooMany) => {
                        self.diag(
                            DiagnosticKind::TooManyTags,
                            pos,
                            Some(clip(&name, MAX_DIAG_TEXT)),
                        );
                    }
                    Some(TagNote::OverLimit) | None => {}
                }
            }
            HeaderItem::BadTag { pos } => {
                let data = self.data;
                self.diag_token(DiagnosticKind::BadTag, pos, line_snippet(data, pos));
            }
            HeaderItem::Junk { pos } => {
                let data = self.data;
                self.diag_token(
                    DiagnosticKind::InvalidTextInHeader,
                    pos,
                    line_snippet(data, pos),
                );
            }
        }
    }

    /// Builds the comment text of `bytes`: decoded, with white space handled
    /// per the options, truncated to `max_comment_len`.
    pub(super) fn comment_string(&mut self, bytes: &[u8]) -> String {
        let cap = self.opts.max_comment_len;
        let slice = cut_for_decode(bytes, cap.saturating_add(4));
        let decoded = self.decoder.decode(slice);
        let mut text = match self.opts.comment_whitespace {
            CommentWhitespace::Collapse => collapse_whitespace(&decoded),
            CommentWhitespace::Preserve => normalize_newlines(&decoded),
        };
        truncate_at_boundary(&mut text, cap);
        text
    }

    // ------------------------------------------------------------------
    // Whole game
    // ------------------------------------------------------------------

    fn run(mut self, raw: &RawGame) -> ParsedGame {
        let pre_game_text = self.pre_game(raw);
        let keep_pre_game = self.opts.pre_game_text == PreGameText::AsComment;
        let mut header = HeaderState::new();
        let data = self.data;
        let (header_end, reached_movetext) =
            scan_header(data, |item| self.header_item(item, &mut header));
        self.start_game(&mut header, pre_game_text.as_deref());

        let finish = |parser: GameParser<'_>, termination| ParsedGame {
            number: raw.number,
            first_line: raw.start_line,
            byte_range: raw.offset..raw.offset + raw.span_len,
            termination,
            pre_game_text: pre_game_text.clone().filter(|_| keep_pre_game),
            diagnostics: parser.diags,
            game: parser.game,
        };

        if raw.end == Termination::Oversize {
            self.diag(DiagnosticKind::GameTooLarge, 0, None);
            return finish(self, Termination::Oversize);
        }
        if let Some(variant) = header.variant.as_deref()
            && classify_variant(variant) == VariantKind::Other
            && self.opts.unsupported_variant == UnsupportedVariant::Skip
        {
            self.diag(
                DiagnosticKind::UnsupportedVariant,
                0,
                Some(clip(variant, MAX_DIAG_TEXT)),
            );
            return finish(self, Termination::UnsupportedVariant);
        }

        let termination = if reached_movetext {
            self.finish_movetext(header_end, &header, raw.end)
        } else {
            self.diag(DiagnosticKind::HeaderOnlyAtEof, data.len(), None);
            Termination::EndOfInput
        };
        if raw.report_fallback && self.decoder.used_fallback {
            self.emit(
                DiagnosticKind::EncodingFallback,
                raw.start_line,
                1,
                raw.offset,
                None,
            );
        }
        finish(self, termination)
    }

    /// Creates the game from the start position and the tags (D26).
    fn start_game(&mut self, header: &mut HeaderState, pre_game_text: Option<&str>) {
        let mut initial = Vec::new();
        if self.opts.pre_game_text == PreGameText::AsComment
            && let Some(text) = pre_game_text
        {
            initial.push(text.to_owned());
        }
        let kind = header.variant.as_deref().map(classify_variant);
        let chess960 = kind == Some(VariantKind::Chess960);
        // An explicit `Variant "Standard"` (or `Chess`, `Normal`) says the
        // FEN is standard: incoherent castling flags are dropped instead of
        // turning the game into Chess960. "From Position", empty and no tag
        // leave it to the position (D58).
        let explicit_standard = header.variant.as_deref().is_some_and(is_explicit_standard);
        let mode = if chess960 {
            FenMode::Chess960
        } else if explicit_standard {
            FenMode::Standard
        } else {
            FenMode::Auto
        };
        let mut position = None;
        if let Some((fen, pos)) = header.fen.take() {
            if let Ok(p) = Position::from_fen_with(&fen, mode) {
                if !chess960 && p.variant() == Variant::Chess960 {
                    self.diag(
                        DiagnosticKind::Chess960Detected,
                        pos,
                        Some(clip(&fen, MAX_DIAG_TEXT)),
                    );
                }
                position = Some(p);
            } else {
                self.diag(
                    DiagnosticKind::InvalidFen,
                    pos,
                    Some(clip(&fen, MAX_DIAG_TEXT)),
                );
                initial.push(format!("Error Invalid FEN: {}", clip(&fen, 200)));
            }
        }
        if position.is_none() && chess960 {
            position = Position::chess960_start(518).ok();
        }
        self.game = position.map_or_else(Game::new, Game::from_position);
        *self.game.tags_mut() = std::mem::replace(&mut header.tags, Tags::new());
        let root = self.game.root();
        for text in initial {
            // The root exists, so this cannot fail.
            drop(self.game.append_comment(root, &text));
        }
        self.stack[0].cursor = root;
    }

    /// Reads the movetext and settles the result (D25). Returns how the game
    /// ended.
    fn finish_movetext(
        &mut self,
        start: usize,
        header: &HeaderState,
        raw_end: Termination,
    ) -> Termination {
        use super::movetext::MovetextEnd;
        let len = self.data.len();
        match self.run_movetext(start) {
            MovetextEnd::Result(result, pos) => {
                self.report_unclosed(pos);
                if header.result_tag && self.game.tags().result != result {
                    self.diag(
                        DiagnosticKind::ResultMismatch,
                        pos,
                        Some(format!("{} != {}", self.game.tags().result, result)),
                    );
                }
                self.game.tags_mut().result = result;
                Termination::ResultToken
            }
            MovetextEnd::NextHeader(pos) => {
                self.report_unclosed(pos);
                self.diag(DiagnosticKind::MissingResult, pos, None);
                Termination::NextGameHeader
            }
            MovetextEnd::Limit => Termination::NodeLimit,
            MovetextEnd::Eof => {
                self.report_unclosed(len);
                self.diag(DiagnosticKind::MissingResult, len, None);
                if raw_end == Termination::NextGameHeader {
                    Termination::NextGameHeader
                } else {
                    Termination::EndOfInput
                }
            }
        }
    }

    fn report_unclosed(&mut self, pos: usize) {
        if self.stack.len() > 1 {
            self.diag(DiagnosticKind::UnclosedVariation, pos, None);
        }
    }
}

/// The rest of the line at `pos`, at most 200 bytes.
fn line_snippet(data: &[u8], pos: usize) -> &[u8] {
    let end = data[pos..]
        .iter()
        .position(|&b| is_eol(b))
        .map_or(data.len(), |i| pos + i);
    &data[pos..end.min(pos + 200)]
}

/// Reads one game from its raw bytes.
pub(crate) fn parse_raw(raw: &RawGame, opts: &ReadOptions) -> ParsedGame {
    let parser = GameParser::new(
        &raw.bytes,
        opts,
        raw.number,
        raw.start_line,
        raw.offset,
        Decoder::new(raw.fallback),
        Game::new(),
    );
    parser.run(raw)
}

/// Header tags of `raw` as written (no normalisation, duplicates kept).
pub(crate) fn scan_raw_tags(raw: &RawGame) -> Vec<(String, String)> {
    let mut decoder = Decoder::new(raw.fallback);
    let mut out = Vec::new();
    scan_header(&raw.bytes, |item| {
        if let HeaderItem::Tag { name, value, .. } = item {
            out.push((
                String::from_utf8_lossy(name).into_owned(),
                decoder.decode(&value).into_owned(),
            ));
        }
    });
    out
}

impl RawGame {
    /// Reads the game. A pure function of the bytes and the options: many
    /// `RawGame`s may be parsed in parallel and the results kept in order.
    ///
    /// Problems with the content never fail the call; they are in
    /// [`ParsedGame::diagnostics`].
    pub fn parse(&self, opts: &ReadOptions) -> ParsedGame {
        parse_raw(self, opts)
    }

    /// The header tags exactly as written (names and unescaped values, in
    /// order, duplicates kept), without reading the movetext.
    pub fn scan_tags(&self) -> Vec<(String, String)> {
        scan_raw_tags(self)
    }
}
