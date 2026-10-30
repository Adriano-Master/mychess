//! Reading `.eco` files: decoding and delegation to the core parser.
//!
//! Scid reads the raw bytes (`PBook::ReadEcoFile`), so `scid_fr.eco`, written
//! in `windows-1252`, shows up damaged in a UTF-8 interface. Here the bytes are
//! decoded as UTF-8 when valid and as `windows-1252` otherwise; a byte order
//! mark is ignored, `\r\n` is normalised and `.gz` files are accepted.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::MultiGzDecoder;
use mychess_core::{EcoBook, EcoBookError, EcoParseOptions, EcoReport};
use thiserror::Error;

/// Largest `.eco` file (after decompression) that is read, in bytes.
const MAX_ECO_BYTES: u64 = 16 << 20;

/// Why an `.eco` file could not be loaded.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EcoFileError {
    /// The file could not be opened or read.
    #[error("i/o error on {path}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is larger than the supported maximum (16 MiB by default).
    #[error("ECO file {path} is too large")]
    TooLarge {
        /// The file.
        path: PathBuf,
    },
    /// The text is not a valid ECO book.
    #[error("invalid ECO file {path}")]
    Parse {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        #[source]
        source: EcoBookError,
    },
}

/// Decodes the bytes of an `.eco` file to text: UTF-8 if valid, else
/// `windows-1252`; a leading BOM is dropped and `\r\n` becomes `\n`.
pub fn decode_eco_bytes(bytes: &[u8]) -> String {
    decode_eco_vec(bytes.to_vec())
}

/// Like [`decode_eco_bytes`], reusing the buffer when it is valid UTF-8.
fn decode_eco_vec(mut bytes: Vec<u8>) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(e.as_bytes())
            .0
            .into_owned(),
    };
    if text.contains('\r') {
        text.replace("\r\n", "\n")
    } else {
        text
    }
}

/// Reads and parses an `.eco` file (plain or gzip-compressed) with the
/// default options (abort at the first corrupt entry).
///
/// The text is limited to 16 MiB (after decompression); see
/// [`read_eco_file_limited`] to change that. Unlike Scid, which reads raw
/// bytes, the text is decoded (UTF-8, else `windows-1252`).
pub fn read_eco_file(path: impl AsRef<Path>) -> Result<EcoReport, EcoFileError> {
    read_eco_file_with(path, EcoParseOptions::default())
}

/// Like [`read_eco_file`], with parse options.
pub fn read_eco_file_with(
    path: impl AsRef<Path>,
    options: EcoParseOptions,
) -> Result<EcoReport, EcoFileError> {
    read_eco_file_limited(path, options, MAX_ECO_BYTES)
}

/// Like [`read_eco_file_with`], with an explicit limit, in bytes, on the
/// (decompressed) text. A file or `.gz` payload above it yields
/// [`EcoFileError::TooLarge`].
pub fn read_eco_file_limited(
    path: impl AsRef<Path>,
    options: EcoParseOptions,
    max_bytes: u64,
) -> Result<EcoReport, EcoFileError> {
    let path = path.as_ref();
    let io = |source| EcoFileError::Io {
        path: path.to_path_buf(),
        source,
    };
    let too_large = || EcoFileError::TooLarge {
        path: path.to_path_buf(),
    };
    let file = File::open(path).map_err(io)?;
    let mut reader = std::io::BufReader::new(file);
    let is_gz = {
        use std::io::BufRead;
        reader.fill_buf().map_err(io)?.starts_with(&[0x1F, 0x8B])
    };
    let mut raw = Vec::new();
    if is_gz {
        MultiGzDecoder::new(reader)
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut raw)
            .map_err(io)?;
    } else {
        reader
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut raw)
            .map_err(io)?;
    }
    if raw.len() as u64 > max_bytes {
        return Err(too_large());
    }
    let text = decode_eco_vec(raw);
    EcoBook::parse_with(&text, options).map_err(|source| EcoFileError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use mychess_core::{ClassifyOptions, EcoDiagnostic, Game, Position, SanMode};
    use std::io::Write;

    const TEXT: &str = "A00 \"d\u{e9}fense\" 1.e4 *\r\n";

    fn write_tmp(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write");
        (dir, path)
    }

    #[test]
    fn decodes_utf8_latin_and_bom() {
        assert_eq!(
            decode_eco_bytes(TEXT.as_bytes()),
            TEXT.replace("\r\n", "\n")
        );
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(b"A00");
        assert_eq!(decode_eco_bytes(&bom), "A00");
        assert_eq!(
            decode_eco_bytes(b"d\xE9fense d\x92A \x96"),
            "d\u{e9}fense d\u{2019}A \u{2013}"
        );
    }

    #[test]
    fn reads_plain_latin_and_gzip_files() {
        let (_d, p) = write_tmp("a.eco", b"A00 \"d\xE9fense\" 1.e4 *\r\n");
        let r = read_eco_file(&p).expect("read");
        assert_eq!(r.book.entries()[0].name(), "d\u{e9}fense");

        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(TEXT.as_bytes()).expect("gz");
        let (_d2, p2) = write_tmp("a.eco.gz", &gz.finish().expect("finish"));
        assert_eq!(read_eco_file(&p2).expect("read").book.len(), 1);
    }

    #[test]
    fn enforces_size_limit_plain_and_gzip() {
        let (_d, p) = write_tmp("big.eco", b"A00 \"x\" 1.e4 *\n");
        let r = read_eco_file_limited(&p, EcoParseOptions::default(), 4);
        assert!(matches!(r, Err(EcoFileError::TooLarge { .. })));
        assert!(read_eco_file_limited(&p, EcoParseOptions::default(), 64).is_ok());

        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&b"# pad\n".repeat(1000)).expect("gz");
        gz.write_all(b"A00 \"x\" 1.e4 *\n").expect("gz");
        let (_d2, p2) = write_tmp("big.eco.gz", &gz.finish().expect("finish"));
        // Small compressed file, large payload: reported as too large.
        let r = read_eco_file_limited(&p2, EcoParseOptions::default(), 100);
        assert!(matches!(r, Err(EcoFileError::TooLarge { .. })));
    }

    #[test]
    fn reads_multi_member_gzip() {
        let mut bytes = Vec::new();
        for text in ["A00 \"a\" 1.e4 *\n", "B00 \"b\" 1.d4 *\n"] {
            let mut gz = GzEncoder::new(Vec::new(), Compression::default());
            gz.write_all(text.as_bytes()).expect("gz");
            bytes.extend(gz.finish().expect("finish"));
        }
        let (_d, p) = write_tmp("m.eco.gz", &bytes);
        assert_eq!(read_eco_file(&p).expect("read").book.len(), 2);
    }

    #[test]
    fn reports_missing_file_and_corrupt_text() {
        let (d, p) = write_tmp("bad.eco", b"A00 \"x\" 1.e5 *");
        assert!(matches!(read_eco_file(&p), Err(EcoFileError::Parse { .. })));
        assert!(matches!(
            read_eco_file(d.path().join("none.eco")),
            Err(EcoFileError::Io { .. })
        ));
    }

    #[test]
    #[ignore = "requires reference/scid_vs_pc-4.27/scid_fr.eco"]
    fn compat_eco_loads_scid_fr_eco() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/scid_vs_pc-4.27/scid_fr.eco"
        );
        let r = read_eco_file(path).expect("read");
        assert_eq!(r.entries_read, 2_364);
        assert_eq!(r.book.len(), 2_342);
        let dups = r
            .diagnostics
            .iter()
            .filter(|d| matches!(d, EcoDiagnostic::DuplicatePosition { .. }))
            .count();
        assert_eq!(dups, 22);
        assert_eq!(r.book.entries()[0].name(), "attaque d\u{2019}Amsterdam");
        assert_eq!(r.book.fewest_pieces(), 22);

        let cases = [
            ("e4 c5", 2, "B20", "d\u{e9}fense sicilienne"),
            ("h4 h5", 1, "A00", "ouverture Anti-Borg (= Desprez)"),
            ("Nf3 Nf6 Ng1 Ng8", 2, "A05", "ouverture R\u{e9}ti"),
        ];
        for (sans, ply, code, name) in cases {
            let start = Position::new();
            let mut pos = start.clone();
            let mut mvs = Vec::new();
            for san in sans.split_whitespace() {
                let mv = pos.parse_san(san, SanMode::Strict).expect("san").mv;
                pos.play(mv).expect("play");
                mvs.push(mv);
            }
            let m = r
                .book
                .classify_line(&start, &mvs, ClassifyOptions::game())
                .expect(sans);
            assert_eq!(
                (m.ply, m.entry().eco().to_string(), m.entry().name()),
                (ply, code.to_owned(), name),
                "{sans}"
            );
        }
        let empty = Game::new();
        assert!(r.book.classify(&empty, ClassifyOptions::game()).is_none());
    }
}
