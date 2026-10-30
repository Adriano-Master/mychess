//! PGN writer with the export options of Scid vs PC.
//!
//! [`WriteOptions`] maps Scid's `PGN_STYLE_*` mask; its default is **strict**
//! PGN, and [`WriteOptions::scid_export`] reproduces the defaults of Scid's
//! export dialog. Games go out through [`write_game`] (one game to any
//! [`Write`]), [`PgnWriter`] (a stream of games with a blank line between
//! them) or [`game_to_string`].
//!
//! The movetext is walked with an explicit stack, so main lines of any length
//! and variations nested to any depth are safe. The deliberate differences
//! from Scid's output are listed in decision D32 of `docs/specs/m1-3-pgn.md`
//! (`1... e5` instead of `1. ...e5`, no spaces inside parentheses, no
//! trailing spaces, NAGs independent of comments, escaped tag values).
//!
//! # Examples
//!
//! ```
//! use mychess_core::{Game, SanMode};
//! use mychess_pgn::write::{WriteOptions, game_to_string};
//!
//! let mut game = Game::new();
//! let pos = game.start_position().clone();
//! let e4 = game.add_move(game.root(), pos.parse_san("e4", SanMode::Strict)?.mv)?;
//! game.set_comment(e4, "King's pawn")?;
//! let pgn = game_to_string(&game, &WriteOptions::default())?;
//! assert!(pgn.ends_with("\n1. e4 {King's pawn} *\n"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod encode;
mod header;
mod layout;
mod movetext;
mod options;

use std::io::Write;

use mychess_core::{Game, GameError, MoveError};

pub use options::{
    BraceHandling, Encoding, Header, LineWidth, NagOutput, Newline, NullMoves, TagSet, WriteOptions,
};

/// Failure while writing a game.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// The output refused the bytes.
    #[error("i/o error while writing pgn")]
    Io(#[from] std::io::Error),
    /// The game tree is inconsistent (cannot happen with a valid [`Game`]).
    #[error("inconsistent game tree")]
    Game(#[from] GameError),
    /// A move of the game cannot be written in SAN (cannot happen with a
    /// valid [`Game`]).
    #[error("cannot write the move at ply {ply}")]
    Move {
        /// Ply of the offending move.
        ply: u32,
        /// Why it failed.
        #[source]
        source: MoveError,
    },
}

fn newline_str(newline: Newline) -> &'static str {
    match newline {
        Newline::CrLf => "\r\n",
        // `Lf` and any future variant.
        _ => "\n",
    }
}

/// Renders one game (header, blank line, movetext, result) as text, with
/// the line ending of the options.
fn render_game(game: &Game, opts: &WriteOptions) -> Result<String, WriteError> {
    let nl = newline_str(opts.newline);
    let mut text = String::new();
    header::write_header(game, opts, nl, &mut text);
    text.push_str(&movetext::write_movetext(game, opts, nl)?);
    Ok(text)
}

/// Writes exactly one game to `w`, encoded as [`WriteOptions::encoding`]
/// says. The text ends with a line terminator; no blank line follows it.
///
/// This is the building block of [`PgnWriter`]: it writes **no** byte order
/// mark (for [`Encoding::Utf8Bom`] the encoding is plain UTF-8 here) and does
/// not call `flush`. The BOM, the blank line between games and the final
/// flush belong to [`PgnWriter`]; use it for files and streams.
///
/// The game is built in memory first, so nothing is written if rendering
/// fails. A `comment_before` stored on a main-line node (it can remain after
/// `Game::promote_variation`) has no place in PGN and is dropped silently;
/// only the ones that open a variation are written.
pub fn write_game<W: Write>(w: &mut W, game: &Game, opts: &WriteOptions) -> Result<(), WriteError> {
    let text = render_game(game, opts)?;
    let mut buf = Vec::with_capacity(text.len());
    encode::encode_into(&text, opts.encoding, &mut buf);
    w.write_all(&buf)?;
    Ok(())
}

/// The PGN text of one game as a `String`.
///
/// Always UTF-8 without BOM, whatever [`WriteOptions::encoding`] says; the
/// encoding applies when the bytes are written. Errors only for a game that
/// is internally inconsistent.
pub fn game_to_string(game: &Game, opts: &WriteOptions) -> Result<String, WriteError> {
    render_game(game, opts)
}

/// Writes a stream of games: a BOM first for [`Encoding::Utf8Bom`], and a
/// blank line between consecutive games.
///
/// After an I/O error the writer is *poisoned*: the sink may hold a partial
/// game, so every later [`PgnWriter::write_game`] and [`PgnWriter::finish`]
/// fails with an [`std::io::ErrorKind::Other`] error instead of appending to
/// a corrupt stream. Rendering errors (an inconsistent game) do not poison
/// it, because nothing was written.
///
/// ```
/// use mychess_core::Game;
/// use mychess_pgn::write::{PgnWriter, WriteOptions};
///
/// let mut out = Vec::new();
/// let mut writer = PgnWriter::new(&mut out, WriteOptions::default());
/// writer.write_game(&Game::new())?;
/// writer.write_game(&Game::new())?;
/// writer.finish()?;
/// let text = String::from_utf8(out)?;
/// assert_eq!(text.matches("[Event").count(), 2);
/// assert!(text.contains("*\n\n[Event"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct PgnWriter<W: Write> {
    out: W,
    opts: WriteOptions,
    games: u64,
    buf: Vec<u8>,
    poisoned: bool,
}

impl<W: Write> PgnWriter<W> {
    /// A writer on `out`.
    pub fn new(out: W, opts: WriteOptions) -> PgnWriter<W> {
        PgnWriter {
            out,
            opts,
            games: 0,
            buf: Vec::new(),
            poisoned: false,
        }
    }

    fn check_poisoned(&self) -> Result<(), WriteError> {
        if self.poisoned {
            return Err(WriteError::Io(std::io::Error::other(
                "pgn writer is poisoned by an earlier i/o error",
            )));
        }
        Ok(())
    }

    /// Appends one game. Nothing is written if the game cannot be rendered.
    ///
    /// # Errors
    ///
    /// [`WriteError::Io`] when the sink fails (the writer is then poisoned,
    /// see the type documentation) or was poisoned before.
    pub fn write_game(&mut self, game: &Game) -> Result<(), WriteError> {
        self.check_poisoned()?;
        let text = render_game(game, &self.opts)?;
        self.buf.clear();
        if self.games == 0 {
            if self.opts.encoding == Encoding::Utf8Bom {
                self.buf.extend_from_slice(&encode::BOM);
            }
        } else {
            encode::encode_into(
                newline_str(self.opts.newline),
                self.opts.encoding,
                &mut self.buf,
            );
        }
        encode::encode_into(&text, self.opts.encoding, &mut self.buf);
        if let Err(e) = self.out.write_all(&self.buf) {
            self.poisoned = true;
            return Err(e.into());
        }
        self.games += 1;
        Ok(())
    }

    /// The number of games written so far.
    pub fn games_written(&self) -> u64 {
        self.games
    }

    /// Flushes and returns the underlying writer.
    ///
    /// `flush` only drains `std::io` buffers. A wrapper that needs a trailer
    /// (for example `flate2::write::GzEncoder`) must be finished by the
    /// caller on the returned writer (`encoder.finish()`); dropping it does
    /// not report errors.
    ///
    /// # Errors
    ///
    /// [`WriteError::Io`] when the flush fails or the writer is poisoned.
    pub fn finish(mut self) -> Result<W, WriteError> {
        self.check_poisoned()?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one option at a time
mod tests {
    use super::*;
    use mychess_core::{GameResult, Move, NodeId, Position, SanMode};
    use pretty_assertions::assert_eq;

    fn mv(g: &mut Game, parent: NodeId, san: &str) -> NodeId {
        let m = g
            .position_at(parent)
            .unwrap()
            .parse_san(san, SanMode::Strict)
            .unwrap()
            .mv;
        g.add_move(parent, m).unwrap()
    }

    fn mv_root(g: &mut Game, san: &str) -> NodeId {
        let root = g.root();
        mv(g, root, san)
    }

    fn sample() -> Game {
        let mut g = Game::new();
        let t = g.tags_mut();
        t.event = "Test".into();
        t.white = "A".into();
        t.black = "B".into();
        t.result = GameResult::White;
        let e4 = mv_root(&mut g, "e4");
        let e5 = mv(&mut g, e4, "e5");
        mv(&mut g, e4, "c5");
        mv(&mut g, e5, "Nf3");
        g
    }

    const SAMPLE: &str = "[Event \"Test\"]\n[Site \"?\"]\n[Date \"????.??.??\"]\n[Round \"?\"]\n\
                          [White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n\
                          1. e4 e5 (1... c5) 2. Nf3 1-0\n";

    #[test]
    fn writes_header_blank_line_movetext() {
        assert_eq!(
            game_to_string(&sample(), &WriteOptions::default()).unwrap(),
            SAMPLE
        );
    }

    #[test]
    fn write_game_matches_game_to_string() {
        let mut bytes = Vec::new();
        write_game(&mut bytes, &sample(), &WriteOptions::default()).unwrap();
        assert_eq!(bytes, SAMPLE.as_bytes());
    }

    #[test]
    fn stream_separates_games_with_one_blank_line() {
        let mut w = PgnWriter::new(Vec::new(), WriteOptions::default());
        w.write_game(&sample()).unwrap();
        w.write_game(&sample()).unwrap();
        assert_eq!(w.games_written(), 2);
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(out, format!("{SAMPLE}\n{SAMPLE}"));
    }

    #[test]
    fn bom_only_once_at_the_start() {
        let mut o = WriteOptions::default();
        o.encoding = Encoding::Utf8Bom;
        let mut w = PgnWriter::new(Vec::new(), o);
        w.write_game(&sample()).unwrap();
        w.write_game(&sample()).unwrap();
        let out = w.finish().unwrap();
        assert_eq!(&out[..3], &[0xEF, 0xBB, 0xBF]);
        assert_eq!(
            out.windows(3).filter(|w| w == &[0xEF, 0xBB, 0xBF]).count(),
            1
        );
    }

    #[test]
    fn latin1_replaces_unrepresentable_characters() {
        let mut g = sample();
        g.tags_mut().white = "Müller ♟ €".into();
        g.set_initial_comment("Zoë 日本");
        let mut o = WriteOptions::default();
        o.encoding = Encoding::Latin1;
        let mut bytes = Vec::new();
        write_game(&mut bytes, &g, &o).unwrap();
        assert!(bytes.windows(10).any(|w| w == b"M\xFCller ? ?"));
        assert!(bytes.windows(8).any(|w| w == b"{Zo\xEB ??}"));
        assert!(!bytes.windows(3).any(|w| w == [0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn crlf_applies_to_header_movetext_and_separators() {
        let mut o = WriteOptions::default();
        o.newline = Newline::CrLf;
        let mut w = PgnWriter::new(Vec::new(), o);
        w.write_game(&sample()).unwrap();
        w.write_game(&sample()).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(out, format!("{0}\r\n{0}", SAMPLE.replace('\n', "\r\n")));
    }

    #[test]
    fn io_errors_propagate() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let err = write_game(&mut Broken, &sample(), &WriteOptions::default()).unwrap_err();
        assert!(matches!(err, WriteError::Io(_)), "{err:?}");
    }

    #[test]
    fn write_game_has_no_bom_and_does_not_flush() {
        struct NoFlush(Vec<u8>);
        impl Write for NoFlush {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("flush called"))
            }
        }
        let mut o = WriteOptions::default();
        o.encoding = Encoding::Utf8Bom;
        let mut sink = NoFlush(Vec::new());
        write_game(&mut sink, &sample(), &o).unwrap();
        assert_eq!(sink.0, SAMPLE.as_bytes());
    }

    #[test]
    fn writer_is_poisoned_after_an_io_error() {
        struct FailOnce(bool, Vec<u8>);
        impl Write for FailOnce {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    return Err(std::io::Error::other("boom"));
                }
                self.1.extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut w = PgnWriter::new(FailOnce(false, Vec::new()), WriteOptions::default());
        assert!(w.write_game(&sample()).is_err());
        assert!(matches!(w.write_game(&sample()), Err(WriteError::Io(_))));
        assert_eq!(w.games_written(), 0);
        assert!(w.finish().is_err());
    }

    #[test]
    fn fen_game_and_scid_export_preset_end_to_end() {
        let mut g =
            Game::from_position(Position::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap());
        let e = g.add_move(g.root(), Move::Null);
        assert!(e.is_ok());
        let mut bytes = Vec::new();
        write_game(&mut bytes, &g, &WriteOptions::scid_export()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K2R w K - 0 1\"]\n\n"));
        assert!(text.ends_with("\n{1. --} *\n"), "{text}");
    }
}
