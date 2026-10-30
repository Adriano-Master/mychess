// Derived from Scid vs PC 4.27, src/tkscid.cpp (`recalcFlagCounts` 169-290,
// `sc_base_stats` 2023-2081, `sc_compact_stats` 4552-4616). Copyright (c)
// 1999-2004 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Statistics of a whole database, read from the index only (decisions D3503,
//! D3504 and rules 7 to 10 of `docs/specs/m7-1-manutencao.md`), and the
//! counters the compaction dialog shows. The CLI `info` command uses the same
//! functions.

use std::io::{Read, Seek};

use mychess_core::GameResult;

use crate::progress::{CancelToken, ProgressSink, Ticker};
use crate::si4::write::plan_append;
use crate::si4::{GameFlags, IndexEntry, NameBase, NameKind, Si4Database};

/// Entries between two progress reports and cancellation checks (rule 7).
const CHECK_EVERY: u32 = 4096;

const KINDS: [NameKind; 4] = [
    NameKind::Player,
    NameKind::Event,
    NameKind::Site,
    NameKind::Round,
];

/// How many names of a kind exist and how many no game uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameUsage {
    /// Names in the name base.
    pub total: u32,
    /// Names no index entry points to (counted again from the index, rule 9).
    pub unused: u32,
}

/// The use count of every name, recounted from the index (the frequency
/// stored in the `.sn4` can be stale). The four vectors follow
/// `[Player, Event, Site, Round]` and have one slot per name ID.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameFrequencies {
    /// Counts for players, events, sites and rounds, in that order.
    pub counts: [Vec<u32>; 4],
}

impl NameFrequencies {
    fn new(names: &NameBase) -> NameFrequencies {
        NameFrequencies {
            counts: KINDS.map(|kind| vec![0; names.num_names(kind) as usize]),
        }
    }

    fn add(&mut self, entry: &IndexEntry) {
        let ids = [
            (0, entry.white_id),
            (0, entry.black_id),
            (1, entry.event_id),
            (2, entry.site_id),
            (3, entry.round_id),
        ];
        for (slot, id) in ids {
            let count = self
                .counts
                .get_mut(slot)
                .and_then(|v| v.get_mut(id as usize));
            if let Some(count) = count {
                *count = count.saturating_add(1);
            }
        }
    }

    /// The counts of one kind.
    pub fn of(&self, kind: NameKind) -> &[u32] {
        let slot = KINDS.iter().position(|&k| k == kind).unwrap_or(0);
        self.counts.get(slot).map_or(&[], Vec::as_slice)
    }

    /// Total and unused names of each kind.
    pub fn usage(&self) -> [NameUsage; 4] {
        std::array::from_fn(|slot| {
            let counts = self.counts.get(slot).map_or(&[][..], Vec::as_slice);
            NameUsage {
                total: u32::try_from(counts.len()).unwrap_or(u32::MAX),
                unused: u32::try_from(counts.iter().filter(|&&c| c == 0).count())
                    .unwrap_or(u32::MAX),
            }
        })
    }
}

/// Recounts the use of every name from the index (`printNameInfo` of the
/// `scidt -n` report). Entries that cannot be read are skipped.
pub fn name_frequencies<R: Read + Seek>(db: &Si4Database<R>) -> NameFrequencies {
    let mut freq = NameFrequencies::new(db.names());
    for entry in db.entries().flatten() {
        freq.add(&entry);
    }
    freq
}

/// Total and unused names of each kind, `[Player, Event, Site, Round]`.
pub fn name_usage<R: Read + Seek>(db: &Si4Database<R>) -> [NameUsage; 4] {
    name_frequencies(db).usage()
}

/// The size of the game file now and after compacting (`sc_compact stats`,
/// `printCompactInfo`), rule 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactionStats {
    /// Games now.
    pub games: u32,
    /// Games marked for deletion.
    pub deleted_games: u32,
    /// Games that stay after compacting (not deleted).
    pub kept_games: u32,
    /// Size of the `.sg4` file now.
    pub current_bytes: u64,
    /// Size of the `.sg4` after compacting, by the 131,072-byte block rule
    /// ([`plan_append`]).
    pub compacted_bytes: u64,
}

impl CompactionStats {
    /// Nothing to do: no deleted game and no wasted space
    /// (`compactGamesEmpty`).
    pub fn is_compact(&self) -> bool {
        self.deleted_games == 0 && self.current_bytes == self.compacted_bytes
    }
}

/// Accumulates the bytes after compaction while the entries go by in order.
#[derive(Default)]
struct CompactionPass {
    end: u64,
    kept: u32,
    deleted: u32,
}

impl CompactionPass {
    fn add(&mut self, entry: &IndexEntry) {
        if entry.flags.contains(GameFlags::DELETE) {
            self.deleted = self.deleted.saturating_add(1);
            return;
        }
        let length = usize::try_from(entry.length).unwrap_or(usize::MAX);
        let (offset, _padding) = plan_append(self.end, length);
        self.end = offset + u64::from(entry.length);
        self.kept = self.kept.saturating_add(1);
    }

    fn finish<R: Read + Seek>(&self, db: &Si4Database<R>) -> CompactionStats {
        CompactionStats {
            games: db.len(),
            deleted_games: self.deleted,
            kept_games: self.kept,
            current_bytes: db.games_file_len(),
            compacted_bytes: self.end,
        }
    }
}

/// The compaction counters of the dialog. Entries that cannot be read are
/// skipped, as the compaction itself would drop them.
///
/// The numbers come from the index only: a record whose body cannot be read
/// (a corrupt `.sg4`) is counted as kept here, while the real compaction
/// drops it, so the real result can have fewer games and bytes.
pub fn compaction_stats<R: Read + Seek>(db: &Si4Database<R>) -> CompactionStats {
    let mut pass = CompactionPass::default();
    for entry in db.entries().flatten() {
        pass.add(&entry);
    }
    pass.finish(db)
}

/// Counters of a whole database (`recalcFlagCounts`, decisions D3503 and D3504).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatabaseStats {
    /// Games in the database.
    pub games: u32,
    /// Games with each bit of [`GameFlags`] (22 bits, `START` first).
    pub flags: [u32; 22],
    /// Results: `1-0`, `1/2-1/2`, `0-1`, `*`.
    pub results: [u32; 4],
    /// Smallest, largest and mean (integer) year, over dates with a year
    /// above 0; `None` if no game has one.
    pub years: Option<(u16, u16, u16)>,
    /// Smallest, largest and mean (integer) Elo, one value per side with an
    /// Elo above 0; `None` if there is none.
    pub ratings: Option<(u16, u16, u16)>,
    /// Names in total and unused, `[Player, Event, Site, Round]`.
    pub names: [NameUsage; 4],
    /// Size of the `.sg4` file now.
    pub games_file_bytes: u64,
    /// Size of the `.sg4` after compacting.
    pub compacted_bytes: u64,
    /// Entries that could not be read (they count in `games` only).
    pub unreadable_entries: u32,
    /// The run was cancelled: the counters cover the games read so far.
    pub interrupted: bool,
}

impl DatabaseStats {
    /// Games with the flag `flag` (a single bit of [`GameFlags`]); `0` for
    /// anything else.
    pub fn flag_count(&self, flag: GameFlags) -> u32 {
        if flag.bits().count_ones() != 1 {
            return 0;
        }
        self.flags
            .get(flag.bits().trailing_zeros() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// `n * 100 / games`, integer, `0` for an empty database (`percentFormat`).
    pub fn percent(&self, n: u32) -> u32 {
        if self.games == 0 {
            return 0;
        }
        u32::try_from(u64::from(n) * 100 / u64::from(self.games)).unwrap_or(100)
    }
}

/// `(min, max, sum, count)` of a series of small numbers.
#[derive(Default)]
struct Range {
    min: u16,
    max: u16,
    sum: u64,
    count: u64,
}

impl Range {
    fn add(&mut self, value: u16) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.sum += u64::from(value);
        self.count += 1;
    }

    fn finish(&self) -> Option<(u16, u16, u16)> {
        let mean = self.sum.checked_div(self.count)?;
        Some((self.min, self.max, u16::try_from(mean).unwrap_or(u16::MAX)))
    }
}

/// Reads the index once and returns the counters of the whole database.
///
/// Cancelled, it returns what it had with `interrupted` set (the file sizes
/// and the name usage are then of the part read). Years count only dates with
/// a year above 0 (the Scid starts the range from game 1 even when it has no
/// date: a defect not copied, D3504).
pub fn database_stats<R: Read + Seek>(
    db: &Si4Database<R>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> DatabaseStats {
    let games = db.len();
    let mut stats = DatabaseStats {
        games,
        ..DatabaseStats::default()
    };
    let mut freq = NameFrequencies::new(db.names());
    let mut compaction = CompactionPass::default();
    let mut years = Range::default();
    let mut ratings = Range::default();
    let mut ticker = Ticker::new(CHECK_EVERY, crate::progress::CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                stats.interrupted = true;
                break;
            }
        }
        let Ok(entry) = db.entry(n) else {
            stats.unreadable_entries += 1;
            continue;
        };
        for (bit, slot) in stats.flags.iter_mut().enumerate() {
            if entry.flags.bits() & (1 << bit) != 0 {
                *slot += 1;
            }
        }
        let result = match entry.result {
            GameResult::White => 0,
            GameResult::Draw => 1,
            GameResult::Black => 2,
            _ => 3,
        };
        if let Some(slot) = stats.results.get_mut(result) {
            *slot += 1;
        }
        if let Some(year) = entry.date.year().filter(|&y| y > 0) {
            years.add(year);
        }
        for elo in [entry.white_elo, entry.black_elo] {
            if elo > 0 {
                ratings.add(elo);
            }
        }
        freq.add(&entry);
        compaction.add(&entry);
    }
    if !stats.interrupted {
        progress.progress(u64::from(games), u64::from(games));
    }
    stats.years = years.finish();
    stats.ratings = ratings.finish();
    stats.names = freq.usage();
    stats.games_file_bytes = db.games_file_len();
    stats.compacted_bytes = compaction.end;
    stats
}
