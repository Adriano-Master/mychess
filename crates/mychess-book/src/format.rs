//! The on-disk layout: 16-byte big-endian records, sorted by key.
//!
//! Scid: `src/polyglot/book.cpp`, `book_read_entry`.

/// Size of one record in bytes.
pub const RECORD_SIZE: usize = 16;

/// The largest weight the editing API produces (Scid: `0xfff0`).
pub const MAX_WEIGHT: u16 = 0xfff0;

/// One record of a `.bin` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Polyglot key of the position **before** the move.
    pub key: u64,
    /// Encoded move (see [`crate::encode_move`]); `0` means absent.
    pub mv: u16,
    /// Weight of the move.
    pub weight: u16,
    /// The `learn` field, kept as opaque bytes (`n` in the high half, `sum`
    /// in the low half).
    pub learn: u32,
}

impl Entry {
    /// Decodes one record. `bytes` must hold at least 16 bytes, otherwise
    /// `None` is returned.
    pub(crate) fn parse(bytes: &[u8]) -> Option<Entry> {
        let rec = bytes.get(..RECORD_SIZE)?;
        Some(Entry {
            key: u64::from_be_bytes(rec.get(0..8)?.try_into().ok()?),
            mv: u16::from_be_bytes(rec.get(8..10)?.try_into().ok()?),
            weight: u16::from_be_bytes(rec.get(10..12)?.try_into().ok()?),
            learn: u32::from_be_bytes(rec.get(12..16)?.try_into().ok()?),
        })
    }

    /// Appends the 16-byte record to `out`.
    pub(crate) fn write_to(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.key.to_be_bytes());
        out.extend_from_slice(&self.mv.to_be_bytes());
        out.extend_from_slice(&self.weight.to_be_bytes());
        out.extend_from_slice(&self.learn.to_be_bytes());
    }
}

/// Index of the first entry whose key is `>= key` (binary search).
pub(crate) fn lower_bound(entries: &[Entry], key: u64) -> usize {
    entries.partition_point(|e| e.key < key)
}

/// The half-open index range of the entries with exactly `key`.
pub(crate) fn key_range(entries: &[Entry], key: u64) -> std::ops::Range<usize> {
    let start = lower_bound(entries, key);
    let len = entries
        .get(start..)
        .map_or(0, |tail| tail.partition_point(|e| e.key == key));
    start..start + len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: u64, mv: u16) -> Entry {
        Entry {
            key,
            mv,
            weight: 1,
            learn: 0,
        }
    }

    #[test]
    fn record_round_trip_is_big_endian() {
        let entry = Entry {
            key: 0x0102_0304_0506_0708,
            mv: 0x0910,
            weight: 0x1112,
            learn: 0x1314_1516,
        };
        let mut out = Vec::new();
        entry.write_to(&mut out);
        assert_eq!(
            out,
            [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16
            ]
        );
        assert_eq!(Entry::parse(&out), Some(entry));
        assert_eq!(Entry::parse(&out[..15]), None);
    }

    #[test]
    fn binary_search_finds_the_first_of_repeated_keys() {
        let entries = [e(1, 1), e(5, 1), e(5, 2), e(5, 3), e(9, 1)];
        assert_eq!(lower_bound(&entries, 5), 1);
        assert_eq!(key_range(&entries, 5), 1..4);
        assert_eq!(key_range(&entries, 6), 4..4);
        assert_eq!(key_range(&entries, 99), 5..5);
        assert_eq!(key_range(&[], 5), 0..0);
    }
}
