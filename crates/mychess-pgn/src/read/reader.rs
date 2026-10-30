//! The public entry points: [`PgnReader`], [`parse_pgn_text`] and
//! [`parse_movetext`].

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use encoding_rs::Encoding;
use mychess_core::{Game, GameResult, Position};

use super::chunker::GameChunker;
use super::diagnostics::Diagnostic;
use super::error::PgnError;
use super::movetext::MovetextEnd;
use super::options::ReadOptions;
use super::parsed::{ParsedGame, Termination};
use super::parser::GameParser;
use super::text::Decoder;

/// Reads the games of a PGN stream one at a time, in file order.
///
/// Gzip input is detected by its magic bytes. Content problems are
/// [`Diagnostic`]s inside each [`ParsedGame`]; the iterator yields an `Err`
/// (and then ends) only for I/O and gzip failures; the games read before the
/// failure are yielded first, the last one as if the input had ended there.
/// UTF-16 input is rejected with [`PgnError::UnsupportedEncoding`].
///
/// An unterminated `{` comment swallows the games that follow it, as in the
/// Scid. Use [`ReadOptions::max_game_bytes`] to bound the damage. For parallel parsing use
/// [`GameChunker`] and [`RawGame::parse`](super::RawGame::parse) directly.
///
/// ```
/// use mychess_pgn::read::{PgnReader, ReadOptions};
///
/// let pgn = "[White \"A\"]\n[Black \"B\"]\n\n1. e4 e5 2. Nf3 1-0\n";
/// let mut reader = PgnReader::new(pgn.as_bytes(), ReadOptions::default());
/// let parsed = reader.next().expect("one game")?;
/// assert_eq!(parsed.game.mainline().count(), 3);
/// assert!(parsed.diagnostics.is_empty());
/// assert!(reader.next().is_none());
/// # Ok::<(), mychess_pgn::read::PgnError>(())
/// ```
pub struct PgnReader<R: Read> {
    chunker: GameChunker<R>,
    opts: ReadOptions,
}

impl<R: Read> PgnReader<R> {
    /// A reader over `input` with the given options.
    pub fn new(input: R, opts: ReadOptions) -> PgnReader<R> {
        PgnReader {
            chunker: GameChunker::new(input, opts.max_game_bytes),
            opts,
        }
    }

    /// Bytes of the decompressed stream consumed so far (progress).
    pub fn bytes_consumed(&self) -> u64 {
        self.chunker.bytes_consumed()
    }

    /// Bytes of non-blank text seen outside any game (between games and
    /// after the last one). A file with no `[` header line yields no game and
    /// reports all of its text here.
    pub fn skipped_bytes(&self) -> u64 {
        self.chunker.skipped_bytes()
    }

    /// The encoding used for text that is not valid UTF-8, as guessed so far
    /// (`windows-1252` until the file shows evidence otherwise).
    pub fn fallback_encoding(&self) -> &'static Encoding {
        self.chunker.fallback_encoding()
    }
}

impl PgnReader<Box<dyn Read + Send>> {
    /// Opens `path`. A gzip file (`.pgn.gz`) is recognised by its content.
    pub fn open(
        path: &Path,
        opts: ReadOptions,
    ) -> Result<PgnReader<Box<dyn Read + Send>>, PgnError> {
        let file = File::open(path).map_err(|source| PgnError::Open {
            path: path.to_owned(),
            source,
        })?;
        Ok(PgnReader::new(Box::new(file), opts))
    }
}

impl<R: Read> Iterator for PgnReader<R> {
    type Item = Result<ParsedGame, PgnError>;

    fn next(&mut self) -> Option<Self::Item> {
        let raw = self.chunker.next()?;
        Some(raw.map(|raw| raw.parse(&self.opts)))
    }
}

/// Reads every game of `text` (pasting PGN, Scid's `sc_game import`).
///
/// ```
/// use mychess_pgn::read::{ReadOptions, parse_pgn_text};
///
/// let games = parse_pgn_text("1. e4 e5 *", &ReadOptions::default());
/// assert!(games.is_empty()); // no header: nothing to cut
/// let games = parse_pgn_text("[Event \"x\"]\n\n1. e4 e5 *", &ReadOptions::default());
/// assert_eq!(games.len(), 1);
/// ```
pub fn parse_pgn_text(text: &str, opts: &ReadOptions) -> Vec<ParsedGame> {
    PgnReader::new(Cursor::new(text.as_bytes()), opts.clone())
        .filter_map(Result::ok)
        .collect()
}

/// The outcome of [`parse_movetext`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MovetextParse {
    /// The game, starting at the given position.
    pub game: Game,
    /// The result token that ended the text, if any.
    pub result: Option<GameResult>,
    /// Why the reading stopped.
    pub termination: Termination,
    /// Everything wrong with the text (`game` is 0, lines are 1-based).
    pub diagnostics: Vec<Diagnostic>,
}

/// Reads moves only (no header) starting from `start`: the fallback of the
/// import and Scid's `sc_move addSan`. Comments, NAGs and variations are
/// kept; tolerances and errors are the same as for a whole game.
///
/// ```
/// use mychess_core::Position;
/// use mychess_pgn::read::{ReadOptions, parse_movetext};
///
/// let parsed = parse_movetext("1. e4 e5 2. Nf3 {develops} Nc6", &Position::new(), &ReadOptions::default());
/// assert_eq!(parsed.game.mainline().count(), 4);
/// assert!(parsed.diagnostics.is_empty());
/// ```
pub fn parse_movetext(text: &str, start: &Position, opts: &ReadOptions) -> MovetextParse {
    let mut parser = GameParser::new(
        text.as_bytes(),
        opts,
        0,
        1,
        0,
        Decoder::new(None),
        Game::from_position(start.clone()),
    );
    let (result, termination) = match parser.run_movetext(0) {
        MovetextEnd::Result(result, _) => (Some(result), Termination::ResultToken),
        MovetextEnd::NextHeader(_) => (None, Termination::NextGameHeader),
        MovetextEnd::Eof => (None, Termination::EndOfInput),
        MovetextEnd::Limit => (None, Termination::NodeLimit),
    };
    if let Some(result) = result {
        parser.game.tags_mut().result = result;
    }
    MovetextParse {
        game: parser.game,
        result,
        termination,
        diagnostics: parser.diags,
    }
}
