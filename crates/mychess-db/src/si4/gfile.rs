// Derived from Scid vs PC 4.27, src/gfile.cpp (ReadGame).
// Copyright (c) 2001 Shane Hudson. All rights reserved (notice of
// gfile.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Access to the `.sg4` game file (`docs/formatos/si4.md`, section 8).
//!
//! The file is not loaded: each record is read with a positioned read of
//! exactly `length` bytes into a reusable buffer (decision D263).

use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

use super::error::{Result, Si4Error, Si4File};

/// Size of a block of the `.sg4` file (`GF_BLOCKSIZE`).
pub const BLOCK_SIZE: u64 = 131_072;

/// A readable, seekable `.sg4` file and the buffer of the last record read.
#[derive(Debug)]
pub(crate) struct GameFile<R> {
    reader: R,
    len: u64,
    path: PathBuf,
    buffer: Vec<u8>,
}

impl<R: Read + Seek> GameFile<R> {
    /// Wraps `reader`; `path` is only used in error messages.
    pub(crate) fn new(mut reader: R, path: PathBuf) -> Result<GameFile<R>> {
        let len = reader
            .seek(SeekFrom::End(0))
            .map_err(|source| Si4Error::Io {
                path: path.clone(),
                source,
            })?;
        Ok(GameFile {
            reader,
            len,
            path,
            buffer: Vec::new(),
        })
    }

    /// The size of the file in bytes.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// Reads the record of `game` (`offset` and `length` come from its index
    /// entry). A record must not be empty, must lie inside one block and
    /// inside the file.
    pub(crate) fn read_record(&mut self, game: u32, offset: u32, length: u32) -> Result<&[u8]> {
        let corrupt = |reason: &'static str| Si4Error::CorruptRecord {
            game,
            offset: u64::from(offset),
            reason,
        };
        if length == 0 {
            return Err(corrupt("empty record"));
        }
        let start = u64::from(offset);
        let end = start + u64::from(length); // at most 2^32 + 2^17, no overflow
        if start / BLOCK_SIZE != (end - 1) / BLOCK_SIZE {
            return Err(corrupt("record crosses a block boundary"));
        }
        if start / BLOCK_SIZE >= self.len.div_ceil(BLOCK_SIZE) {
            return Err(corrupt("record is in a block beyond the end of the file"));
        }
        if end > self.len {
            return Err(Si4Error::Truncated {
                file: Si4File::Games,
                offset: start,
                need: u64::from(length),
            });
        }
        let size = usize::try_from(length).map_err(|_| corrupt("record too large"))?;
        self.buffer.clear();
        self.buffer.resize(size, 0);
        let io_error = |source: io::Error| Si4Error::Io {
            path: self.path.clone(),
            source,
        };
        self.reader.seek(SeekFrom::Start(start)).map_err(io_error)?;
        self.reader.read_exact(&mut self.buffer).map_err(|source| {
            if source.kind() == io::ErrorKind::UnexpectedEof {
                Si4Error::Truncated {
                    file: Si4File::Games,
                    offset: start,
                    need: u64::from(length),
                }
            } else {
                Si4Error::Io {
                    path: self.path.clone(),
                    source,
                }
            }
        })?;
        Ok(&self.buffer)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn file(len: usize) -> GameFile<Cursor<Vec<u8>>> {
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        GameFile::new(Cursor::new(data), PathBuf::from("test.sg4")).unwrap()
    }

    #[test]
    fn reads_a_record_inside_the_file() {
        let mut f = file(1000);
        assert_eq!(f.len(), 1000);
        let rec = f.read_record(1, 10, 5).unwrap();
        assert_eq!(rec, &[10, 11, 12, 13, 14]);
        // The buffer is reused and a second read gives other bytes.
        assert_eq!(f.read_record(2, 0, 2).unwrap(), &[0, 1]);
    }

    #[test]
    fn rejects_empty_records_and_records_past_the_end() {
        let mut f = file(1000);
        assert!(matches!(
            f.read_record(3, 0, 0),
            Err(Si4Error::CorruptRecord { game: 3, .. })
        ));
        assert!(matches!(
            f.read_record(1, 990, 20),
            Err(Si4Error::Truncated {
                file: Si4File::Games,
                ..
            })
        ));
        assert!(matches!(
            f.read_record(1, 2_000_000, 10),
            Err(Si4Error::CorruptRecord { .. })
        ));
        assert!(f.read_record(1, 995, 5).is_ok());
    }

    #[test]
    fn rejects_records_that_cross_a_block() {
        let mut f = file(300_000);
        let block = u32::try_from(BLOCK_SIZE).unwrap();
        assert!(f.read_record(1, block - 10, 10).is_ok());
        assert!(matches!(
            f.read_record(1, block - 10, 11),
            Err(Si4Error::CorruptRecord {
                reason: "record crosses a block boundary",
                ..
            })
        ));
        assert!(f.read_record(1, block, 100).is_ok());
    }

    #[test]
    fn extreme_offsets_do_not_overflow() {
        let mut f = file(100);
        assert!(f.read_record(1, u32::MAX, 131_071).is_err());
        assert!(f.read_record(1, u32::MAX, u32::MAX).is_err());
    }
}
