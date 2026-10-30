// Derived from Scid vs PC 4.27, src/namebase.cpp (ReadNameFile).
// Copyright (c) 2001 Shane Hudson. All rights reserved (notice of
// namebase.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The `.sn4` name file (`docs/formatos/si4.md`, section 7).
//!
//! Four sections (players, events, sites, rounds), each with front-coded
//! names and explicit IDs. The whole file is parsed into one arena per kind;
//! every allocation is bounded by the size of the file.

use std::fmt;

use super::bytes::{read_u24_be, read_u32_be};
use super::error::{NameSection, Result, Si4Error, Si4File};

/// Size of the `.sn4` header in bytes.
pub const NAME_HEADER_LEN: usize = 36;
const MAGIC: &[u8; 8] = b"Scid.sn\0";
/// Smallest possible record: 2-byte ID, 1-byte frequency, 1-byte length.
const MIN_RECORD_LEN: u64 = 4;
/// Front-coding lets a record of a few bytes stand for a name of up to 255
/// bytes, so the arena can be larger than the file. Real databases stay below
/// 3 times the file size; a file that expands by more than this factor (plus
/// a small slack) is treated as corrupt, which keeps the memory used by a
/// hostile file within a fixed multiple of its size.
const MAX_EXPANSION: usize = 16;
const EXPANSION_SLACK: usize = 1 << 20;

/// The four kinds of names, in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NameKind {
    /// Player names (`NAME_PLAYER`).
    Player,
    /// Event names (`NAME_EVENT`).
    Event,
    /// Site names (`NAME_SITE`).
    Site,
    /// Round names (`NAME_ROUND`).
    Round,
}

impl NameKind {
    /// All kinds, in the order of the file.
    pub const ALL: [NameKind; 4] = [
        NameKind::Player,
        NameKind::Event,
        NameKind::Site,
        NameKind::Round,
    ];

    /// The position of the kind in [`NameKind::ALL`] (and in every per-kind
    /// array of the name file and the writer).
    pub(crate) fn index(self) -> usize {
        match self {
            NameKind::Player => 0,
            NameKind::Event => 1,
            NameKind::Site => 2,
            NameKind::Round => 3,
        }
    }
}

impl fmt::Display for NameKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NameKind::Player => "player",
            NameKind::Event => "event",
            NameKind::Site => "site",
            NameKind::Round => "round",
        })
    }
}

/// The names of one kind: bytes in an arena, spans and frequencies by ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NameTable {
    data: Vec<u8>,
    /// `(start, length)` into `data`, indexed by ID.
    spans: Vec<(u32, u8)>,
    /// File frequency, indexed by ID.
    frequencies: Vec<u32>,
}

impl NameTable {
    fn get(&self, id: u32) -> Option<&[u8]> {
        let &(start, len) = self.spans.get(usize::try_from(id).ok()?)?;
        let start = usize::try_from(start).ok()?;
        self.data.get(start..start.checked_add(usize::from(len))?)
    }
}

/// The decoded name base: IDs to raw name bytes for the four kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameBase {
    time_stamp: u32,
    max_frequency: [u32; 4],
    tables: [NameTable; 4],
}

impl NameBase {
    /// Parses the contents of a `.sn4` file.
    ///
    /// Validation: header size and magic; `numNames` and the record widths
    /// against the file size; for each record `prefixLen <= length`,
    /// `prefixLen <=` the previous name, `id < numNames` and no repeated ID;
    /// no read past the end. The alphabetical order is not checked (D260).
    pub fn parse(data: &[u8]) -> Result<NameBase> {
        let truncated = |offset: usize, need: usize| Si4Error::Truncated {
            file: Si4File::Names,
            offset: offset as u64,
            need: need as u64,
        };
        if data.len() < NAME_HEADER_LEN {
            return Err(truncated(0, NAME_HEADER_LEN));
        }
        if data.get(..MAGIC.len()) != Some(&MAGIC[..]) {
            return Err(Si4Error::BadMagic {
                file: Si4File::Names,
            });
        }
        let time_stamp = read_u32_be(data, 8).ok_or_else(|| truncated(8, 4))?;
        let mut num_names = [0u32; 4];
        let mut max_frequency = [0u32; 4];
        for i in 0..4 {
            num_names[i] = read_u24_be(data, 12 + 3 * i).ok_or_else(|| truncated(12, 24))?;
            max_frequency[i] = read_u24_be(data, 24 + 3 * i).ok_or_else(|| truncated(24, 12))?;
        }

        // Every record takes at least `MIN_RECORD_LEN` bytes, so the declared
        // counts must fit in the file. This bounds every allocation below.
        let total: u64 = num_names.iter().map(|&n| u64::from(n)).sum();
        let available = (data.len() - NAME_HEADER_LEN) as u64;
        if total.saturating_mul(MIN_RECORD_LEN) > available {
            return Err(truncated(
                NAME_HEADER_LEN,
                usize::try_from(total.saturating_mul(MIN_RECORD_LEN)).unwrap_or(usize::MAX),
            ));
        }

        let mut pos = NAME_HEADER_LEN;
        let mut tables: [NameTable; 4] = Default::default();
        for kind in NameKind::ALL {
            let k = kind.index();
            let section = NameSection::Names(kind);
            let table = parse_section(data, &mut pos, num_names[k], max_frequency[k], section)?;
            tables[k] = table;
        }
        Ok(NameBase {
            time_stamp,
            max_frequency,
            tables,
        })
    }

    /// The raw bytes of the name with `id`, or `None` if the ID is out of range.
    pub fn get(&self, kind: NameKind, id: u32) -> Option<&[u8]> {
        self.tables.get(kind.index())?.get(id)
    }

    /// The number of names of `kind` (IDs are `0..num_names`).
    pub fn num_names(&self, kind: NameKind) -> u32 {
        self.tables
            .get(kind.index())
            .map_or(0, |t| u32::try_from(t.spans.len()).unwrap_or(u32::MAX))
    }

    /// The usage count stored in the file for a name (only reliable for
    /// databases written with `-fast`; see `docs/formatos/si4.md`).
    pub fn frequency(&self, kind: NameKind, id: u32) -> Option<u32> {
        let table = self.tables.get(kind.index())?;
        table.frequencies.get(usize::try_from(id).ok()?).copied()
    }

    /// The `maxFrequency` field of the header for `kind`.
    pub fn max_frequency(&self, kind: NameKind) -> u32 {
        self.max_frequency.get(kind.index()).copied().unwrap_or(0)
    }

    /// The raw time stamp field of the header.
    pub fn time_stamp(&self) -> u32 {
        self.time_stamp
    }

    /// Appends a name of `kind` at the end and returns its new ID (the
    /// number of names before). It does not look for a name that is already
    /// there: the caller keeps its own index, so a database in memory that
    /// grows game by game does not rebuild the whole name base each time.
    /// The frequency starts at 0 (see [`NameBase::add_frequency`]). A name
    /// longer than 255 bytes is cut to 255. Unlike
    /// [`NameBuilder::add_name`](super::NameBuilder::add_name), which refuses
    /// even a name that already exists once the base is full, this never
    /// looks names up, so the caller's index decides (the Clipbase reuses an
    /// existing name even when the base is full).
    ///
    /// # Errors
    ///
    /// [`Si4Error::NameLimit`] when the base already holds the most names of
    /// that kind the format allows (the IDs have 20, 19, 19 and 18 bits).
    pub fn append_name(&mut self, kind: NameKind, name: &[u8]) -> Result<u32> {
        let limit = super::names_write::NAME_MAX_ID[kind.index()];
        let table = &mut self.tables[kind.index()];
        let id = u32::try_from(table.spans.len()).unwrap_or(u32::MAX);
        let full = Si4Error::NameLimit { kind, limit };
        if id >= limit {
            return Err(full);
        }
        let name = name.get(..255).unwrap_or(name);
        let start = u32::try_from(table.data.len()).map_err(|_| full)?;
        // At most 255 bytes: the length always fits in a byte.
        let len = u8::try_from(name.len()).unwrap_or(u8::MAX);
        table.data.extend_from_slice(name);
        table.spans.push((start, len));
        table.frequencies.push(0);
        Ok(id)
    }

    /// Removes the names of `kind` with an ID of `keep` or more, undoing
    /// [`NameBase::append_name`]. Does nothing if there are no more than
    /// `keep` names.
    pub fn truncate_names(&mut self, kind: NameKind, keep: u32) {
        let table = &mut self.tables[kind.index()];
        let Ok(keep) = usize::try_from(keep) else {
            return;
        };
        if let Some(&(start, _)) = table.spans.get(keep)
            && let Ok(start) = usize::try_from(start)
        {
            table.data.truncate(start);
        }
        table.spans.truncate(keep);
        table.frequencies.truncate(keep);
    }

    /// Adds `delta` to the frequency of a name, raising `maxFrequency` if
    /// needed. An unknown ID is ignored.
    pub fn add_frequency(&mut self, kind: NameKind, id: u32, delta: u32) {
        let Some(freq) = usize::try_from(id)
            .ok()
            .and_then(|i| self.tables[kind.index()].frequencies.get_mut(i))
        else {
            return;
        };
        *freq = freq.saturating_add(delta);
        let now = *freq;
        let max = &mut self.max_frequency[kind.index()];
        *max = (*max).max(now);
    }
}

fn parse_section(
    data: &[u8],
    pos: &mut usize,
    num: u32,
    max_frequency: u32,
    section: NameSection,
) -> Result<NameTable> {
    let corrupt = |reason: &'static str| Si4Error::CorruptNames { section, reason };
    let truncated = |offset: usize, need: usize| Si4Error::Truncated {
        file: Si4File::Names,
        offset: offset as u64,
        need: need as u64,
    };
    let id_width = if num >= 65_536 { 3 } else { 2 };
    let freq_width = if max_frequency >= 65_536 {
        3
    } else if max_frequency >= 256 {
        2
    } else {
        1
    };
    let count = usize::try_from(num).map_err(|_| corrupt("too many names"))?;
    let mut table = NameTable {
        data: Vec::new(),
        spans: vec![(0, 0); count],
        frequencies: vec![0; count],
    };
    let mut seen = vec![false; count];
    // `(start, length)` of the previous record in the file, for front-coding.
    let mut previous: Option<(usize, usize)> = None;

    for _ in 0..count {
        let id = read_be(data, *pos, id_width).ok_or_else(|| truncated(*pos, id_width))?;
        *pos += id_width;
        let frequency =
            read_be(data, *pos, freq_width).ok_or_else(|| truncated(*pos, freq_width))?;
        *pos += freq_width;
        let length = usize::from(*data.get(*pos).ok_or_else(|| truncated(*pos, 1))?);
        *pos += 1;
        let prefix = if previous.is_some() {
            let p = usize::from(*data.get(*pos).ok_or_else(|| truncated(*pos, 1))?);
            *pos += 1;
            p
        } else {
            0
        };
        if prefix > length {
            return Err(corrupt("prefix longer than the name"));
        }
        let start = table.data.len();
        if let Some((prev_start, prev_len)) = previous {
            if prefix > prev_len {
                return Err(corrupt("prefix longer than the previous name"));
            }
            table
                .data
                .extend_from_within(prev_start..prev_start + prefix);
        }
        let new_len = length - prefix;
        let new_bytes = data
            .get(*pos..*pos + new_len)
            .ok_or_else(|| truncated(*pos, new_len))?;
        *pos += new_len;
        table.data.extend_from_slice(new_bytes);
        if u32::try_from(table.data.len()).is_err()
            || table.data.len() > data.len().saturating_mul(MAX_EXPANSION) + EXPANSION_SLACK
        {
            return Err(corrupt("name data too large for the file"));
        }

        let slot = usize::try_from(id).map_err(|_| corrupt("name id out of range"))?;
        if id >= num {
            return Err(corrupt("name id out of range"));
        }
        match seen.get_mut(slot) {
            Some(flag) if !*flag => *flag = true,
            Some(_) => return Err(corrupt("repeated name id")),
            None => return Err(corrupt("name id out of range")),
        }
        let start_u32 = u32::try_from(start).map_err(|_| corrupt("name data too large"))?;
        let length_u8 = u8::try_from(length).map_err(|_| corrupt("name too long"))?;
        if let Some(span) = table.spans.get_mut(slot) {
            *span = (start_u32, length_u8);
        }
        if let Some(freq) = table.frequencies.get_mut(slot) {
            *freq = frequency;
        }
        previous = Some((start, length));
    }
    Ok(table)
}

/// Reads a big-endian integer of 1, 2 or 3 bytes.
fn read_be(data: &[u8], offset: usize, width: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(width)?)?;
    Some(bytes.iter().fold(0u32, |acc, &b| (acc << 8) | u32::from(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `.sn4` with the given (id, frequency, name) records per kind,
    /// in the order given (front-coding computed here).
    fn build(sections: [&[(u32, u32, &[u8])]; 4], max_freq: [u32; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&7u32.to_be_bytes());
        for s in &sections {
            out.extend_from_slice(&(s.len() as u32).to_be_bytes()[1..]);
        }
        for m in max_freq {
            out.extend_from_slice(&m.to_be_bytes()[1..]);
        }
        for (s, m) in sections.iter().zip(max_freq) {
            let id_width = if s.len() >= 65_536 { 3 } else { 2 };
            let freq_width = if m >= 65_536 {
                3
            } else if m >= 256 {
                2
            } else {
                1
            };
            let mut prev: &[u8] = &[];
            for (i, (id, freq, name)) in s.iter().enumerate() {
                out.extend_from_slice(&id.to_be_bytes()[4 - id_width..]);
                out.extend_from_slice(&freq.to_be_bytes()[4 - freq_width..]);
                out.push(name.len() as u8);
                let mut prefix = 0;
                if i > 0 {
                    prefix = prev
                        .iter()
                        .zip(name.iter())
                        .take_while(|(a, b)| a == b)
                        .count();
                    out.push(prefix as u8);
                }
                out.extend_from_slice(&name[prefix..]);
                prev = name;
            }
        }
        out
    }

    const NONE: &[(u32, u32, &[u8])] = &[];

    #[test]
    fn parses_front_coded_names_with_ids_out_of_file_order() {
        let players: &[(u32, u32, &[u8])] =
            &[(2, 5, b"Anand"), (0, 1, b"Anderssen"), (1, 300, b"Karpov")];
        let data = build([players, NONE, NONE, NONE], [300, 0, 0, 0]);
        let nb = NameBase::parse(&data).unwrap();
        assert_eq!(nb.num_names(NameKind::Player), 3);
        assert_eq!(nb.get(NameKind::Player, 0), Some(&b"Anderssen"[..]));
        assert_eq!(nb.get(NameKind::Player, 1), Some(&b"Karpov"[..]));
        assert_eq!(nb.get(NameKind::Player, 2), Some(&b"Anand"[..]));
        assert_eq!(nb.get(NameKind::Player, 3), None);
        assert_eq!(nb.frequency(NameKind::Player, 1), Some(300));
        assert_eq!(nb.max_frequency(NameKind::Player), 300);
        assert_eq!(nb.time_stamp(), 7);
        assert_eq!(nb.num_names(NameKind::Site), 0);
    }

    #[test]
    fn append_truncate_and_frequency_work_in_place() {
        let players: &[(u32, u32, &[u8])] = &[(0, 1, b"Anderssen")];
        let data = build([players, NONE, NONE, NONE], [1, 0, 0, 0]);
        let mut nb = NameBase::parse(&data).unwrap();
        let a = nb.append_name(NameKind::Player, b"Kieseritzky").unwrap();
        let b = nb.append_name(NameKind::Player, &[b'x'; 300]).unwrap();
        assert_eq!((a, b), (1, 2));
        assert_eq!(nb.get(NameKind::Player, 1), Some(&b"Kieseritzky"[..]));
        assert_eq!(nb.get(NameKind::Player, 2).map(<[u8]>::len), Some(255));
        assert_eq!(nb.num_names(NameKind::Player), 3);
        nb.add_frequency(NameKind::Player, 1, 4);
        assert_eq!(nb.frequency(NameKind::Player, 1), Some(4));
        assert_eq!(nb.max_frequency(NameKind::Player), 4);
        nb.add_frequency(NameKind::Player, 99, 1); // ignored
        nb.truncate_names(NameKind::Player, 1);
        assert_eq!(nb.num_names(NameKind::Player), 1);
        assert_eq!(nb.get(NameKind::Player, 1), None);
        assert_eq!(nb.get(NameKind::Player, 0), Some(&b"Anderssen"[..]));
        // IDs and data are reusable after a truncation.
        assert_eq!(nb.append_name(NameKind::Player, b"Zukertort").unwrap(), 1);
        assert_eq!(nb.get(NameKind::Player, 1), Some(&b"Zukertort"[..]));
    }

    #[test]
    fn empty_names_and_all_four_kinds() {
        let one: &[(u32, u32, &[u8])] = &[(0, 1, b"")];
        let data = build([one, one, one, one], [1, 1, 1, 1]);
        let nb = NameBase::parse(&data).unwrap();
        for kind in NameKind::ALL {
            assert_eq!(nb.get(kind, 0), Some(&b""[..]));
        }
    }

    #[test]
    fn frequency_widths_switch_at_256_and_65536() {
        for max in [255u32, 256, 65_535, 65_536] {
            let players: &[(u32, u32, &[u8])] = &[(0, max, b"A"), (1, 1, b"B")];
            let data = build([players, NONE, NONE, NONE], [max, 0, 0, 0]);
            let nb = NameBase::parse(&data).unwrap();
            assert_eq!(nb.frequency(NameKind::Player, 0), Some(max));
            assert_eq!(nb.get(NameKind::Player, 1), Some(&b"B"[..]));
        }
    }

    #[test]
    fn id_width_switches_at_65536_names() {
        for n in [65_535u32, 65_536] {
            let records: Vec<(u32, u32, Vec<u8>)> = (0..n)
                .map(|i| (i, 1, format!("N{i:06}").into_bytes()))
                .collect();
            let borrowed: Vec<(u32, u32, &[u8])> = records
                .iter()
                .map(|(i, f, v)| (*i, *f, v.as_slice()))
                .collect();
            let data = build([&borrowed, NONE, NONE, NONE], [1, 0, 0, 0]);
            let nb = NameBase::parse(&data).unwrap();
            assert_eq!(nb.num_names(NameKind::Player), n);
            let last = format!("N{:06}", n - 1);
            assert_eq!(nb.get(NameKind::Player, n - 1), Some(last.as_bytes()));
        }
    }

    #[test]
    fn rejects_bad_header() {
        let ok = build([NONE; 4], [0; 4]);
        assert!(matches!(
            NameBase::parse(&ok[..35]),
            Err(Si4Error::Truncated {
                file: Si4File::Names,
                ..
            })
        ));
        let mut bad = ok.clone();
        bad[0] = b'X';
        assert!(matches!(
            NameBase::parse(&bad),
            Err(Si4Error::BadMagic { .. })
        ));
    }

    #[test]
    fn rejects_counts_larger_than_the_file() {
        let mut data = build([NONE; 4], [0; 4]);
        data[12..15].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        assert!(matches!(
            NameBase::parse(&data),
            Err(Si4Error::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_and_out_of_range_ids() {
        let dup: &[(u32, u32, &[u8])] = &[(0, 1, b"A"), (0, 1, b"B")];
        let data = build([dup, NONE, NONE, NONE], [1, 0, 0, 0]);
        assert!(matches!(
            NameBase::parse(&data),
            Err(Si4Error::CorruptNames {
                reason: "repeated name id",
                ..
            })
        ));
        let range: &[(u32, u32, &[u8])] = &[(0, 1, b"A"), (2, 1, b"B")];
        let data = build([range, NONE, NONE, NONE], [1, 0, 0, 0]);
        assert!(matches!(
            NameBase::parse(&data),
            Err(Si4Error::CorruptNames {
                reason: "name id out of range",
                ..
            })
        ));
    }

    #[test]
    fn rejects_bad_prefixes() {
        let names: &[(u32, u32, &[u8])] = &[(0, 1, b"AB"), (1, 1, b"ABC")];
        let good = build([names, NONE, NONE, NONE], [1, 0, 0, 0]);
        assert!(NameBase::parse(&good).is_ok());
        // Second record: id(2) freq(1) length(1) prefix(1) at the end.
        let prefix_at = NAME_HEADER_LEN + (2 + 1 + 1 + 2) + (2 + 1 + 1);
        let mut longer_than_name = good.clone();
        longer_than_name[prefix_at] = 4; // > length 3
        assert!(matches!(
            NameBase::parse(&longer_than_name),
            Err(Si4Error::CorruptNames { .. })
        ));
        let mut longer_than_previous = good.clone();
        longer_than_previous[prefix_at] = 3; // previous name has 2 bytes
        assert!(matches!(
            NameBase::parse(&longer_than_previous),
            Err(Si4Error::CorruptNames { .. })
        ));
    }

    #[test]
    fn rejects_names_that_expand_far_beyond_the_file() {
        // Each record takes 6 bytes but stands for a 255-byte name.
        let records: Vec<(u32, u32, Vec<u8>)> = (0..9_000u32)
            .map(|i| {
                let mut name = vec![b'a'; 254];
                name.push(if i % 2 == 0 { b'b' } else { b'c' });
                (i, 1, name)
            })
            .collect();
        let borrowed: Vec<(u32, u32, &[u8])> = records
            .iter()
            .map(|(i, f, v)| (*i, *f, v.as_slice()))
            .collect();
        let data = build([&borrowed, NONE, NONE, NONE], [1, 0, 0, 0]);
        assert!(matches!(
            NameBase::parse(&data),
            Err(Si4Error::CorruptNames {
                reason: "name data too large for the file",
                ..
            })
        ));
    }

    #[test]
    fn truncation_at_every_byte_is_an_error() {
        let names: &[(u32, u32, &[u8])] = &[(0, 1, b"Anand"), (1, 1, b"Anderssen")];
        let data = build([names, names, NONE, NONE], [1, 1, 0, 0]);
        for cut in 0..data.len() {
            assert!(NameBase::parse(&data[..cut]).is_err(), "cut at {cut}");
        }
        assert!(NameBase::parse(&data).is_ok());
    }
}
