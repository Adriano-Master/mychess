// Derived from Scid vs PC 4.27, src/namebase.cpp (AddName, WriteNameFile),
// src/namebase.h (IncFrequency) and src/strtree.h (iteration order).
// Copyright (c) 2001 Shane Hudson. All rights reserved (notice of
// namebase.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The writable name base and the `.sn4` encoder (`docs/formatos/si4.md`,
//! section 7; `docs/specs/m3-2-escrita-si4.md`, rules 25 to 32).
//!
//! [`NameBuilder`] keeps, for each kind, the names by ID, their frequencies
//! and the historic `maxFrequency` marker, and writes the whole file: names
//! sorted in Scid's order, front-coded, with the ID and frequency widths that
//! depend on the counts.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::error::{Result, Si4Error};
use super::names::{NAME_HEADER_LEN, NameBase, NameKind};

/// The most names of each kind (`NAME_MAX_ID`): player, event, site, round.
/// Adding a name fails when the base already has this many, even if the name
/// exists (rule 8).
pub const NAME_MAX_ID: [u32; 4] = [1_048_575, 524_287, 524_287, 262_143];
/// The longest name, in bytes (its length is one byte).
pub const MAX_NAME_LEN: usize = 255;

const MAGIC: &[u8; 8] = b"Scid.sn\0";

fn slot(kind: NameKind) -> usize {
    kind.index()
}

/// The largest prefix of `name` of at most 255 bytes that ends at a
/// character boundary (decision D280). The flag tells if it was shortened.
pub fn truncate_name(name: &str) -> (&str, bool) {
    if name.len() <= MAX_NAME_LEN {
        return (name, false);
    }
    let mut end = MAX_NAME_LEN;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    (name.get(..end).unwrap_or(""), true)
}

/// Scid's order of names (rule 26, decision D286): by the first byte as an
/// unsigned number, then byte by byte as *signed* characters with the NUL
/// terminator counting as a byte. The empty name comes first.
pub fn name_order(a: &[u8], b: &[u8]) -> Ordering {
    let first = |n: &[u8]| n.first().copied().unwrap_or(0);
    first(a).cmp(&first(b)).then_with(|| {
        let mut i = 0;
        loop {
            let ca = a.get(i).copied().unwrap_or(0);
            let cb = b.get(i).copied().unwrap_or(0);
            if ca != cb {
                // `char` is signed in the reference build.
                return signed(ca).cmp(&signed(cb));
            }
            if ca == 0 {
                return Ordering::Equal;
            }
            i += 1;
        }
    })
}

fn signed(byte: u8) -> i8 {
    i8::from_ne_bytes([byte])
}

#[derive(Debug, Clone, Default)]
struct KindTable {
    names: Vec<Vec<u8>>,
    frequencies: Vec<u32>,
    lookup: HashMap<Vec<u8>, u32>,
}

/// What a transaction changed, to undo it.
#[derive(Debug, Clone)]
struct Journal {
    added: Vec<NameKind>,
    deltas: Vec<(NameKind, u32, u32, u32)>,
    max_frequency: [u32; 4],
}

/// The name base in memory, ready to be changed and written.
///
/// Frequencies are the number of games that use each name. `maxFrequency`
/// is a historic high-water mark: it only goes up, and it decides the width
/// of the frequency field of the file (decision D279).
#[derive(Debug, Clone, Default)]
pub struct NameBuilder {
    time_stamp: u32,
    max_frequency: [u32; 4],
    kinds: [KindTable; 4],
    journal: Option<Journal>,
}

impl NameBuilder {
    /// An empty name base: no names, `timeStamp` 0 (what `sc_base create`
    /// writes).
    pub fn new() -> NameBuilder {
        NameBuilder::default()
    }

    /// Copies the names, the `timeStamp` and the `maxFrequency` markers of a
    /// parsed `.sn4`. The frequencies start at 0: the file's values are only
    /// reliable for databases written with `-fast`, so the caller counts the
    /// games with [`NameBuilder::inc_frequency`] (what Scid does when it
    /// opens a database).
    pub fn from_name_base(base: &NameBase) -> NameBuilder {
        let mut builder = NameBuilder {
            time_stamp: base.time_stamp(),
            ..NameBuilder::default()
        };
        for kind in NameKind::ALL {
            let table = &mut builder.kinds[slot(kind)];
            builder.max_frequency[slot(kind)] = base.max_frequency(kind);
            for id in 0..base.num_names(kind) {
                let name = base.get(kind, id).unwrap_or(&[]).to_vec();
                table.lookup.entry(name.clone()).or_insert(id);
                table.names.push(name);
                table.frequencies.push(0);
            }
        }
        builder
    }

    /// The number of names of `kind`.
    pub fn num_names(&self, kind: NameKind) -> u32 {
        u32::try_from(self.kinds[slot(kind)].names.len()).unwrap_or(u32::MAX)
    }

    /// The raw bytes of a name.
    pub fn get(&self, kind: NameKind, id: u32) -> Option<&[u8]> {
        let index = usize::try_from(id).ok()?;
        self.kinds[slot(kind)].names.get(index).map(Vec::as_slice)
    }

    /// The ID of an exact name, if present.
    pub fn find(&self, kind: NameKind, name: &[u8]) -> Option<u32> {
        self.kinds[slot(kind)].lookup.get(name).copied()
    }

    /// The number of games that use a name.
    pub fn frequency(&self, kind: NameKind, id: u32) -> Option<u32> {
        let index = usize::try_from(id).ok()?;
        self.kinds[slot(kind)].frequencies.get(index).copied()
    }

    /// The `maxFrequency` marker of `kind`.
    pub fn max_frequency(&self, kind: NameKind) -> u32 {
        self.max_frequency[slot(kind)]
    }

    /// The `timeStamp` of the file (preserved when a database is rewritten).
    pub fn time_stamp(&self) -> u32 {
        self.time_stamp
    }

    /// Sets the `timeStamp`.
    pub fn set_time_stamp(&mut self, value: u32) {
        self.time_stamp = value;
    }

    /// Adds a name and returns its ID, or returns the ID it already has
    /// (`NameBase::AddName`). The limit is tested first: with `NAME_MAX_ID`
    /// names in the base, even an existing name is refused (rule 8). A name
    /// of more than 255 bytes is cut to 255 (use [`truncate_name`] first to
    /// keep a UTF-8 text valid).
    pub fn add_name(&mut self, kind: NameKind, name: &[u8]) -> Result<u32> {
        let limit = NAME_MAX_ID[slot(kind)];
        if self.num_names(kind) >= limit {
            return Err(Si4Error::NameLimit { kind, limit });
        }
        let name = name.get(..MAX_NAME_LEN).unwrap_or(name);
        if let Some(id) = self.find(kind, name) {
            return Ok(id);
        }
        let table = &mut self.kinds[slot(kind)];
        let id = u32::try_from(table.names.len()).unwrap_or(u32::MAX);
        table.names.push(name.to_vec());
        table.frequencies.push(0);
        table.lookup.insert(name.to_vec(), id);
        if let Some(journal) = &mut self.journal {
            journal.added.push(kind);
        }
        Ok(id)
    }

    /// Adds `delta` to the frequency of a name (`NameBase::IncFrequency`):
    /// `maxFrequency` goes up if the new frequency is above it and never goes
    /// down. The frequency never goes below 0. An unknown ID is ignored.
    pub fn inc_frequency(&mut self, kind: NameKind, id: u32, delta: i32) {
        let Some(index) = usize::try_from(id).ok() else {
            return;
        };
        let Some(freq) = self.kinds[slot(kind)].frequencies.get_mut(index) else {
            return;
        };
        let before = *freq;
        *freq = before.saturating_add_signed(delta);
        let after = *freq;
        if after > self.max_frequency[slot(kind)] {
            self.max_frequency[slot(kind)] = after;
        }
        if let Some(journal) = &mut self.journal {
            journal.deltas.push((kind, id, before, after));
        }
    }

    /// Starts recording changes so that [`NameBuilder::rollback`] can undo
    /// them. A transaction already open is replaced.
    pub fn begin(&mut self) {
        self.journal = Some(Journal {
            added: Vec::new(),
            deltas: Vec::new(),
            max_frequency: self.max_frequency,
        });
    }

    /// Keeps the changes made since [`NameBuilder::begin`].
    pub fn commit(&mut self) {
        self.journal = None;
    }

    /// Undoes everything since [`NameBuilder::begin`]: new names are removed,
    /// frequencies and the `maxFrequency` markers go back.
    pub fn rollback(&mut self) {
        let Some(journal) = self.journal.take() else {
            return;
        };
        for (kind, id, before, _) in journal.deltas.into_iter().rev() {
            if let Some(freq) = usize::try_from(id)
                .ok()
                .and_then(|i| self.kinds[slot(kind)].frequencies.get_mut(i))
            {
                *freq = before;
            }
        }
        for kind in journal.added.into_iter().rev() {
            let table = &mut self.kinds[slot(kind)];
            if let Some(name) = table.names.pop() {
                table.frequencies.pop();
                // Another ID may hold the same bytes only in a corrupt file.
                table.lookup.remove(&name);
            }
        }
        self.max_frequency = journal.max_frequency;
    }

    /// The bytes of the whole `.sn4` file (`NameBase::WriteNameFile`).
    ///
    /// Names of each kind are written in [`name_order`], front-coded against
    /// the record before them; the ID takes 3 bytes from 65,536 names, the
    /// frequency 1, 2 or 3 bytes depending on `maxFrequency` (below 256, below
    /// 65,536, or more). Counts in the header are 24 bits.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(NAME_HEADER_LEN);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.time_stamp.to_be_bytes());
        for kind in NameKind::ALL {
            push_be(&mut out, self.num_names(kind), 3);
        }
        for kind in NameKind::ALL {
            push_be(&mut out, self.max_frequency(kind), 3);
        }
        for kind in NameKind::ALL {
            let table = &self.kinds[slot(kind)];
            let count = table.names.len();
            let id_width = if count >= 65_536 { 3 } else { 2 };
            let max = self.max_frequency(kind);
            let freq_width = if max >= 65_536 {
                3
            } else if max >= 256 {
                2
            } else {
                1
            };
            let mut order: Vec<usize> = (0..count).collect();
            order.sort_by(|&a, &b| {
                let (na, nb) = (&table.names[a], &table.names[b]);
                name_order(na, nb).then(a.cmp(&b))
            });
            let mut previous: &[u8] = &[];
            for (position, &index) in order.iter().enumerate() {
                let name = table.names[index].as_slice();
                push_be(&mut out, u32::try_from(index).unwrap_or(0), id_width);
                push_be(&mut out, table.frequencies[index], freq_width);
                out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
                let mut prefix = 0;
                if position > 0 {
                    prefix = name
                        .iter()
                        .zip(previous)
                        .take_while(|(a, b)| a == b)
                        .count();
                    out.push(u8::try_from(prefix).unwrap_or(u8::MAX));
                }
                out.extend_from_slice(name.get(prefix..).unwrap_or(&[]));
                previous = name;
            }
        }
        out
    }
}

/// Appends the low `width` bytes of `value`, big-endian.
fn push_be(out: &mut Vec<u8>, value: u32, width: usize) {
    let bytes = value.to_be_bytes();
    out.extend_from_slice(bytes.get(4 - width..).unwrap_or(&[]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_index_matches_all_and_the_limits_table() {
        for (i, kind) in NameKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), i);
            assert_eq!(slot(kind), i);
        }
        assert_eq!(NAME_MAX_ID.len(), NameKind::ALL.len());
    }

    fn builder(names: &[(NameKind, &str)]) -> NameBuilder {
        let mut b = NameBuilder::new();
        for &(kind, name) in names {
            let id = b.add_name(kind, name.as_bytes()).unwrap();
            b.inc_frequency(kind, id, 1);
        }
        b
    }

    #[test]
    fn an_empty_base_is_the_36_byte_file_of_scid() {
        let bytes = NameBuilder::new().to_bytes();
        assert_eq!(bytes.len(), 36);
        assert_eq!(&bytes[..8], b"Scid.sn\0");
        assert!(bytes[8..].iter().all(|&b| b == 0));
        assert!(NameBase::parse(&bytes).is_ok());
    }

    #[test]
    fn written_names_are_read_back_with_ids_and_front_coding() {
        let b = builder(&[
            (NameKind::Player, "Carlsen"),
            (NameKind::Player, "Anand"),
            (NameKind::Player, "Anderssen"),
            (NameKind::Player, "Carlsen"),
            (NameKind::Event, "Wijk"),
        ]);
        let bytes = b.to_bytes();
        let parsed = NameBase::parse(&bytes).unwrap();
        assert_eq!(parsed.num_names(NameKind::Player), 3);
        assert_eq!(parsed.get(NameKind::Player, 0), Some(&b"Carlsen"[..]));
        assert_eq!(parsed.get(NameKind::Player, 1), Some(&b"Anand"[..]));
        assert_eq!(parsed.get(NameKind::Player, 2), Some(&b"Anderssen"[..]));
        assert_eq!(parsed.frequency(NameKind::Player, 0), Some(2));
        assert_eq!(parsed.max_frequency(NameKind::Player), 2);
        assert_eq!(parsed.get(NameKind::Event, 0), Some(&b"Wijk"[..]));
        // Player section: Anand, Anderssen (prefix 2), Carlsen.
        let section = &bytes[36..];
        assert_eq!(&section[..2], &[0, 1]);
        assert_eq!(section[2], 1); // frequency
        assert_eq!(section[3], 5); // length of "Anand"
        assert_eq!(&section[4..9], b"Anand");
        assert_eq!(&section[9..11], &[0, 2]);
        assert_eq!(section[13], 2); // prefix of "Anderssen" after "Anand"
        assert_eq!(&section[14..21], b"derssen");
    }

    #[test]
    fn names_are_ordered_by_first_unsigned_byte_then_signed_bytes() {
        assert_eq!(name_order(b"", b"a"), Ordering::Less);
        assert_eq!(name_order(b"a", b"b"), Ordering::Less);
        // "ab" sorts after "ab\xC3\xA9" because 0 > -61 as signed chars.
        assert_eq!(name_order(b"ab", "abé".as_bytes()), Ordering::Greater);
        // Within the same first byte, UTF-8 bytes come before ASCII.
        assert_eq!(name_order("aé".as_bytes(), b"az"), Ordering::Less);
        // A first byte of 0x80 or more sorts after ASCII (unsigned).
        assert_eq!(name_order("é".as_bytes(), b"z"), Ordering::Greater);
        assert_eq!(name_order(b"same", b"same"), Ordering::Equal);
    }

    #[test]
    fn the_file_keeps_the_signed_order_and_round_trips() {
        let mut b = NameBuilder::new();
        for name in ["ab", "abé", "", "aé", "az", "é", "z"] {
            b.add_name(NameKind::Site, name.as_bytes()).unwrap();
        }
        let parsed = NameBase::parse(&b.to_bytes()).unwrap();
        for id in 0..7 {
            assert_eq!(parsed.get(NameKind::Site, id), b.get(NameKind::Site, id));
        }
    }

    #[test]
    fn frequency_widths_follow_the_historic_maximum() {
        for (max, width) in [(0u32, 1usize), (255, 1), (256, 2), (65_535, 2), (65_536, 3)] {
            let mut b = builder(&[(NameKind::Player, "A")]);
            b.max_frequency[0] = max;
            let bytes = b.to_bytes();
            // 36 + id (2) + frequency + length (1) + 1 byte of name.
            assert_eq!(bytes.len(), 36 + 2 + width + 1 + 1, "max {max}");
            assert!(NameBase::parse(&bytes).is_ok());
        }
    }

    #[test]
    fn id_width_grows_at_65536_names() {
        let mut b = NameBuilder::new();
        for i in 0..65_536u32 {
            b.add_name(NameKind::Player, i.to_string().as_bytes())
                .unwrap();
        }
        let big = b.to_bytes();
        let parsed = NameBase::parse(&big).unwrap();
        assert_eq!(parsed.num_names(NameKind::Player), 65_536);
        assert_eq!(parsed.get(NameKind::Player, 65_535), Some(&b"65535"[..]));
        // One name less keeps the 2-byte ID: the size difference shows it.
        let mut small = NameBuilder::new();
        for i in 0..65_535u32 {
            small
                .add_name(NameKind::Player, i.to_string().as_bytes())
                .unwrap();
        }
        let parsed_small = NameBase::parse(&small.to_bytes()).unwrap();
        assert_eq!(parsed_small.num_names(NameKind::Player), 65_535);
    }

    #[test]
    fn max_frequency_only_goes_up_and_frequency_never_below_zero() {
        let mut b = builder(&[(NameKind::Player, "A")]);
        b.inc_frequency(NameKind::Player, 0, 4);
        assert_eq!(b.frequency(NameKind::Player, 0), Some(5));
        assert_eq!(b.max_frequency(NameKind::Player), 5);
        b.inc_frequency(NameKind::Player, 0, -100);
        assert_eq!(b.frequency(NameKind::Player, 0), Some(0));
        assert_eq!(b.max_frequency(NameKind::Player), 5);
        b.inc_frequency(NameKind::Player, 9, 1); // unknown id: ignored
        assert_eq!(b.num_names(NameKind::Player), 1);
    }

    #[test]
    fn the_name_limit_is_tested_before_the_lookup() {
        let mut b = NameBuilder::new();
        b.add_name(NameKind::Round, b"1").unwrap();
        // Fake a full base without adding 262,143 names.
        b.kinds[3].names.resize(262_143, Vec::new());
        b.kinds[3].frequencies.resize(262_143, 0);
        assert!(matches!(
            b.add_name(NameKind::Round, b"1"),
            Err(Si4Error::NameLimit {
                kind: NameKind::Round,
                limit: 262_143
            })
        ));
    }

    #[test]
    fn long_names_are_truncated_on_a_character_boundary() {
        let long = "a".repeat(300);
        let (cut, flag) = truncate_name(&long);
        assert_eq!((cut.len(), flag), (255, true));
        // 254 ASCII bytes plus a 2-byte letter: only 254 fit.
        let text = format!("{}é", "a".repeat(254));
        let (cut, flag) = truncate_name(&text);
        assert_eq!((cut.len(), flag), (254, true));
        assert_eq!(truncate_name("short"), ("short", false));
        let mut b = NameBuilder::new();
        let id = b.add_name(NameKind::Event, &[b'x'; 300]).unwrap();
        assert_eq!(b.get(NameKind::Event, id).map(<[u8]>::len), Some(255));
    }

    #[test]
    fn rollback_undoes_names_frequencies_and_the_maximum() {
        let mut b = builder(&[(NameKind::Player, "A")]);
        b.begin();
        let id = b.add_name(NameKind::Player, b"B").unwrap();
        b.inc_frequency(NameKind::Player, id, 3);
        b.inc_frequency(NameKind::Player, 0, 5);
        b.rollback();
        assert_eq!(b.num_names(NameKind::Player), 1);
        assert_eq!(b.find(NameKind::Player, b"B"), None);
        assert_eq!(b.frequency(NameKind::Player, 0), Some(1));
        assert_eq!(b.max_frequency(NameKind::Player), 1);
        b.begin();
        b.add_name(NameKind::Player, b"B").unwrap();
        b.commit();
        b.rollback(); // nothing open: no effect
        assert_eq!(b.num_names(NameKind::Player), 2);
    }

    #[test]
    fn from_name_base_keeps_the_stamp_and_the_maximum() {
        let mut b = builder(&[(NameKind::Player, "A")]);
        b.set_time_stamp(77);
        b.inc_frequency(NameKind::Player, 0, 299);
        let parsed = NameBase::parse(&b.to_bytes()).unwrap();
        let again = NameBuilder::from_name_base(&parsed);
        assert_eq!(again.time_stamp(), 77);
        assert_eq!(again.max_frequency(NameKind::Player), 300);
        assert_eq!(again.frequency(NameKind::Player, 0), Some(0));
        assert_eq!(again.to_bytes().len(), b.to_bytes().len());
    }
}
