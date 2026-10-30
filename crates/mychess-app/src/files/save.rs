//! Saving a game to a PGN file, atomically (spec M2-2, D127-D129).
//!
//! Blocking: only [`perform`](crate::perform) calls it, from
//! `spawn_blocking`.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use mychess_core::Game;
use mychess_pgn::write::{Encoding, PgnWriter, WriteOptions, game_to_string};

use crate::error::{FileError, FileErrorKind};
use crate::fsutil::write_atomic;
use crate::prefs::PgnExportPrefs;

/// How the game goes into the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMode {
    /// The file becomes this game alone.
    Overwrite,
    /// The game is added after the games already in the file.
    Append,
    /// Like [`SaveMode::Append`] (same encoding and separator rules), but the
    /// game is written at the end of the file itself, without copying it:
    /// the cost does not grow with the file (a tournament PGN). A crash
    /// during the write can leave a partial last game, which the rest of the
    /// file survives.
    AppendInPlace,
}

/// What `Append` needs to know about the file it extends.
struct Existing {
    /// Starts with a UTF-8 byte order mark.
    bom: bool,
    /// Valid UTF-8 from start to end.
    utf8: bool,
    /// Line feeds in the run of line breaks at the very end (0, 1 or 2+).
    trailing_newlines: usize,
}

fn render_error(path: &Path) -> FileError {
    FileError::new(
        FileErrorKind::Io(io::ErrorKind::InvalidData),
        Some(path.to_path_buf()),
    )
}

/// Reads the file once to learn its encoding and how it ends. `Ok(None)`
/// for a missing or empty file.
fn inspect(path: &Path) -> io::Result<Option<Existing>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut buf = vec![0u8; 64 * 1024];
    let mut head: Vec<u8> = Vec::with_capacity(4);
    let mut tail: Vec<u8> = Vec::with_capacity(8);
    let mut carry: Vec<u8> = Vec::new();
    let mut utf8 = true;
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        let chunk = &buf[..n];
        if head.len() < 4 {
            let take = (4 - head.len()).min(n);
            head.extend_from_slice(&chunk[..take]);
        }
        tail.extend_from_slice(chunk);
        let keep = tail.len().saturating_sub(8);
        tail.drain(..keep);
        if utf8 {
            carry.extend_from_slice(chunk);
            match std::str::from_utf8(&carry) {
                Ok(_) => carry.clear(),
                Err(e) if e.error_len().is_some() => utf8 = false,
                Err(e) => {
                    let valid = e.valid_up_to();
                    carry.drain(..valid);
                }
            }
        }
    }
    if total == 0 {
        return Ok(None);
    }
    if head.starts_with(&[0x1f, 0x8b]) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "gzip"));
    }
    if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "utf-16"));
    }
    if !carry.is_empty() {
        utf8 = false;
    }
    let trailing_newlines = tail
        .iter()
        .rev()
        .take_while(|b| **b == b'\n' || **b == b'\r')
        .filter(|b| **b == b'\n')
        .count();
    Ok(Some(Existing {
        bom: head.starts_with(&[0xef, 0xbb, 0xbf]),
        utf8,
        trailing_newlines,
    }))
}

/// Writes `game` to `path`.
///
/// - [`SaveMode::Overwrite`]: temporary file in the same directory, flushed,
///   then renamed over the target; on failure the target is untouched and no
///   temporary file remains. Encoding and layout come from `prefs`; line
///   ends are always `\n`.
/// - [`SaveMode::Append`]: the old content is copied byte for byte, a blank
///   line is ensured, then the game follows. The encoding is the one
///   detected in the old file (BOM or valid UTF-8: UTF-8 without a second
///   BOM, else Latin-1); characters that Latin-1 cannot hold refuse the
///   append with [`FileErrorKind::EncodingLossy`]. A missing or empty file is
///   created as in `Overwrite`.
///
/// # Errors
///
/// A [`FileError`] describing the failure.
pub fn save_pgn(
    path: &Path,
    mode: SaveMode,
    game: &Game,
    prefs: &PgnExportPrefs,
) -> Result<(), FileError> {
    let fail = |e: &io::Error| FileError::from_io(e, Some(path.to_path_buf()));
    let opts = prefs.write_options();
    let existing = match mode {
        SaveMode::Overwrite => None,
        SaveMode::Append | SaveMode::AppendInPlace => match inspect(path) {
            Ok(existing) => existing,
            Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
                let name = if e.to_string() == "gzip" {
                    "gzip"
                } else {
                    "UTF-16"
                };
                return Err(FileError::new(
                    FileErrorKind::UnsupportedEncoding(name),
                    Some(path.to_path_buf()),
                ));
            }
            Err(e) => return Err(fail(&e)),
        },
    };
    let Some(existing) = existing else {
        let bytes = render(game, &opts).map_err(|()| render_error(path))?;
        return write_atomic(path, |f| f.write_all(&bytes)).map_err(|e| fail(&e));
    };

    let encoding = if existing.bom || existing.utf8 {
        Encoding::Utf8
    } else {
        Encoding::Latin1
    };
    if encoding == Encoding::Latin1 {
        let text = game_to_string(game, &opts).map_err(|_| render_error(path))?;
        if text.chars().any(|c| u32::from(c) > 0xff) {
            return Err(FileError::new(
                FileErrorKind::EncodingLossy,
                Some(path.to_path_buf()),
            ));
        }
    }
    let bytes = render(game, &opts.with_encoding(encoding)).map_err(|()| render_error(path))?;
    let separator: &[u8] = match existing.trailing_newlines {
        0 => b"\n\n",
        1 => b"\n",
        _ => b"",
    };
    if mode == SaveMode::AppendInPlace {
        return append_in_place(path, separator, &bytes).map_err(|e| fail(&e));
    }
    write_atomic(path, |out| {
        // The old file is opened for this copy only and closed before the
        // rename (Windows refuses to replace an open file).
        {
            let mut old = File::open(path)?;
            io::copy(&mut old, out)?;
        }
        out.write_all(separator)?;
        out.write_all(&bytes)
    })
    .map_err(|e| fail(&e))
}

/// Adds `separator` and `bytes` at the end of `path` and flushes them.
fn append_in_place(path: &Path, separator: &[u8], bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    // One write, so a concurrent reader sees the game whole or not at all in
    // practice.
    let mut chunk = Vec::with_capacity(separator.len() + bytes.len());
    chunk.extend_from_slice(separator);
    chunk.extend_from_slice(bytes);
    file.write_all(&chunk)?;
    file.sync_all()
}

/// The bytes of one game, with a BOM if `opts` ask for one.
fn render(game: &Game, opts: &WriteOptions) -> Result<Vec<u8>, ()> {
    let mut writer = PgnWriter::new(Vec::new(), opts.clone());
    writer.write_game(game).map_err(|_| ())?;
    writer.finish().map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use mychess_core::Position;
    use mychess_pgn::read::{ReadOptions, parse_pgn_text};

    use super::*;
    use crate::prefs::EncodingPref;

    fn sample(white: &str) -> Game {
        let mut game = Game::new();
        game.tags_mut().white = white.to_owned();
        let e4 = Position::new().parse_uci("e2e4").expect("uci");
        let node = game.add_move(game.root(), e4).expect("move");
        game.set_comment(node, "good {move}").expect("comment");
        game
    }

    fn read_back(path: &Path) -> Vec<Game> {
        let bytes = std::fs::read(path).expect("read");
        let text = String::from_utf8_lossy(&bytes);
        parse_pgn_text(&text, &ReadOptions::default())
            .into_iter()
            .map(|p| p.game)
            .collect()
    }

    #[test]
    fn overwrite_round_trips_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("my game.pgn");
        let game = sample("\u{c9}douard");
        save_pgn(
            &path,
            SaveMode::Overwrite,
            &game,
            &PgnExportPrefs::default(),
        )
        .expect("save");
        save_pgn(
            &path,
            SaveMode::Overwrite,
            &game,
            &PgnExportPrefs::default(),
        )
        .expect("again");
        let games = read_back(&path);
        assert_eq!(games.len(), 1);
        assert!(games[0].same_content(&game) || games[0].tags().white == "\u{c9}douard");
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);
        let bytes = std::fs::read(&path).expect("read");
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert!(!bytes.contains(&b'\r'));
    }

    #[test]
    fn append_in_place_writes_at_the_end_without_replacing_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.pgn");
        let prefs = PgnExportPrefs::default();
        // A missing file is created like in `Overwrite`.
        save_pgn(&path, SaveMode::AppendInPlace, &sample("A"), &prefs).expect("first");
        let before = std::fs::read(&path).expect("read");
        save_pgn(&path, SaveMode::AppendInPlace, &sample("B"), &prefs).expect("second");
        let after = std::fs::read(&path).expect("read");
        assert!(after.starts_with(&before), "the old bytes are untouched");
        assert_eq!(read_back(&path).len(), 2);
        // The same file stays (no replacement by a new one): on Unix the
        // inode is kept.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let inode = std::fs::metadata(&path).expect("meta").ino();
            save_pgn(&path, SaveMode::AppendInPlace, &sample("C"), &prefs).expect("third");
            assert_eq!(std::fs::metadata(&path).expect("meta").ino(), inode);
            assert_eq!(read_back(&path).len(), 3);
        }
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);
    }

    #[test]
    fn append_keeps_old_bytes_and_one_blank_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.pgn");
        for old in [
            "[White \"Old\"]\n\n1. d4 *\n",
            "[White \"Old\"]\n\n1. d4 *",
            "[White \"Old\"]\n\n1. d4 *\n\n",
            "[White \"Old\"]\r\n\r\n1. d4 *\r\n",
        ] {
            std::fs::write(&path, old).expect("write");
            save_pgn(
                &path,
                SaveMode::Append,
                &sample("New"),
                &PgnExportPrefs::default(),
            )
            .expect("append");
            let bytes = std::fs::read(&path).expect("read");
            assert!(bytes.starts_with(old.as_bytes()), "{old:?}");
            let rest = String::from_utf8(bytes[old.len()..].to_vec()).expect("utf8");
            let separator: String = rest.chars().take_while(|c| *c == '\n').collect();
            let had = old.trim_end_matches(['\r', '\n']).len();
            let tail_newlines = old[had..].matches('\n').count();
            assert_eq!(tail_newlines + separator.len(), 2, "{old:?}");
            assert!(rest.trim_start().starts_with("[Event"), "{rest:?}");
            let games = read_back(&path);
            assert_eq!(games.len(), 2, "{old:?}");
        }
    }

    #[test]
    fn append_creates_missing_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("new.pgn");
        save_pgn(
            &path,
            SaveMode::Append,
            &sample("N"),
            &PgnExportPrefs::default(),
        )
        .expect("append");
        assert_eq!(read_back(&path).len(), 1);
    }

    #[test]
    fn append_does_not_repeat_the_bom() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bom.pgn");
        let prefs = PgnExportPrefs {
            encoding: EncodingPref::Utf8Bom,
            ..PgnExportPrefs::default()
        };
        save_pgn(&path, SaveMode::Overwrite, &sample("A"), &prefs).expect("save");
        assert!(
            std::fs::read(&path)
                .expect("read")
                .starts_with(&[0xef, 0xbb, 0xbf])
        );
        save_pgn(&path, SaveMode::Append, &sample("B"), &prefs).expect("append");
        let bytes = std::fs::read(&path).expect("read");
        let boms = bytes
            .windows(3)
            .filter(|w| *w == [0xef, 0xbb, 0xbf])
            .count();
        assert_eq!(boms, 1);
    }

    #[test]
    fn append_follows_the_encoding_of_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let latin = dir.path().join("latin.pgn");
        std::fs::write(&latin, b"[White \"Andr\xe9\"]\n\n1. d4 *\n").expect("write");
        save_pgn(
            &latin,
            SaveMode::Append,
            &sample("Jos\u{e9}"),
            &PgnExportPrefs::default(),
        )
        .expect("append");
        let bytes = std::fs::read(&latin).expect("read");
        assert!(
            bytes.windows(4).any(|w| w == b"Jos\xe9"),
            "latin-1 bytes expected"
        );
        let before = std::fs::read(&latin).expect("read");
        let err = save_pgn(
            &latin,
            SaveMode::Append,
            &sample("\u{265f}"),
            &PgnExportPrefs::default(),
        )
        .expect_err("lossy");
        assert_eq!(err.kind, FileErrorKind::EncodingLossy);
        assert_eq!(std::fs::read(&latin).expect("read"), before);
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);

        let utf8 = dir.path().join("utf8.pgn");
        std::fs::write(&utf8, "[White \"Andr\u{e9}\"]\n\n1. d4 *\n").expect("write");
        save_pgn(
            &utf8,
            SaveMode::Append,
            &sample("\u{265f}"),
            &PgnExportPrefs::default(),
        )
        .expect("append");
        assert!(String::from_utf8(std::fs::read(&utf8).expect("read")).is_ok());
    }

    #[test]
    fn append_refuses_gzip_and_utf16() {
        let dir = tempfile::tempdir().expect("tempdir");
        let gz = dir.path().join("a.gz");
        std::fs::write(&gz, [0x1f, 0x8b, 8, 0]).expect("write");
        let err = save_pgn(
            &gz,
            SaveMode::Append,
            &sample("A"),
            &PgnExportPrefs::default(),
        )
        .expect_err("gzip");
        assert!(matches!(
            err.kind,
            FileErrorKind::UnsupportedEncoding("gzip")
        ));
        let u16 = dir.path().join("u16.pgn");
        std::fs::write(&u16, [0xff, 0xfe, b'[', 0]).expect("write");
        let err = save_pgn(
            &u16,
            SaveMode::Append,
            &sample("A"),
            &PgnExportPrefs::default(),
        )
        .expect_err("utf16");
        assert!(matches!(
            err.kind,
            FileErrorKind::UnsupportedEncoding("UTF-16")
        ));
    }

    #[test]
    fn failures_leave_the_destination_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("no dir").join("x.pgn");
        let err = save_pgn(
            &missing,
            SaveMode::Overwrite,
            &sample("A"),
            &PgnExportPrefs::default(),
        )
        .expect_err("missing directory");
        assert_eq!(err.kind, FileErrorKind::NotFound);
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 0);
        let subdir = dir.path().join("sub");
        std::fs::create_dir(&subdir).expect("mkdir");
        assert!(
            save_pgn(
                &subdir,
                SaveMode::Overwrite,
                &sample("A"),
                &PgnExportPrefs::default()
            )
            .is_err()
        );
        assert!(subdir.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);
    }
}
