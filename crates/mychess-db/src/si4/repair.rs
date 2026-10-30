// Derived from Scid vs PC 4.27, src/tkscid.cpp (sc_base_fix_corrupted,
// 3569-3730: `scidt -N`). Copyright (c) 1999-2003 Shane Hudson. All rights
// reserved (notices of the Scid sources); Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Repair of a damaged database (`docs/specs/m7-1-manutencao.md`, rule 43,
//! decisions D3526, D3527 and D3580-D3583).
//!
//! [`repair_database`] works on the **files**, with the database closed. It
//! rewrites the `.si4` and the `.sn4` and never touches the `.sg4`:
//!
//! 1. the number of entries is taken from the size of the `.si4` when the
//!    header disagrees (`(size - 182) / 47`, at most [`MAX_GAMES`]): the file
//!    wins, as in Scid. When the header says **fewer** games than the file
//!    holds, the surplus (an interrupted write, a concatenated file) is kept
//!    as entries and counted in [`RepairReport::entries_beyond_header`], so
//!    the caller can warn; they may well not open, and
//!    [`RepairOptions::drop_unreadable`] removes those;
//! 2. a name ID that the `.sn4` does not have becomes the name `?`
//!    (Scid: ID 0), and is counted in [`RepairReport::ids_fixed`];
//! 3. the frequencies are recounted from the entries, the names that no game
//!    uses are removed and the IDs are renumbered keeping the order;
//! 4. with [`RepairOptions::drop_unreadable`], an entry whose record is not
//!    in the `.sg4` or does not parse is removed from the index (counted in
//!    [`RepairReport::entries_dropped`]).
//!
//! Safety, which Scid does not have (it renames the originals to `_OLD`, and
//! deletes them even after an error):
//!
//! - nothing is changed until both new files are complete, synced and read
//!   back (the names parsed, every ID of every entry checked);
//! - the originals are copied to `<file>.bak` (`.bak1`, `.bak2`... when a
//!   backup already exists, never overwritten) **before** the swap, and the
//!   backups are never deleted by this code. Only a cancelled run, which
//!   changed nothing, removes the backups it had just made;
//! - the swap is the journaled transaction of [`super::journal`], so a
//!   process that dies leaves the old pair or the new pair, never a mix;
//! - the exclusive lock of the `.si4` is held during the whole run, so a
//!   database that a writer has open is refused ([`Si4Error::AlreadyOpen`]),
//!   and every handle is closed before the renames (Windows).
//!
//! Memory is proportional to the number of names and to one bit per entry:
//! the index is streamed, never loaded.

use std::cell::Cell;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::progress::{CancelToken, ProgressSink};

use super::compact::patch_name_ids;
use super::database::{find_file, strip_known_extension};
use super::entry::IndexEntry;
use super::error::{Result, Si4Error, Si4File};
use super::gfile::GameFile;
use super::header::{ENTRY_LEN, HEADER_LEN, Header, MAX_GAMES};
use super::header_write::HeaderBytes;
use super::journal::{self, FailPoint, run_transaction};
use super::names::{NameBase, NameKind};
use super::names_write::NameBuilder;
use super::raw::RawGame;
use super::write::{io_error, remove_cache, sync_parent, with_suffix};

/// Progress is reported and the token looked at every this many entries
/// (and at the first one).
const CHECK_STRIDE: u32 = 256;

/// The most backup generations looked at before giving up.
const MAX_BACKUPS: u32 = 10_000;

/// The kind of each of the five name fields of an entry, in the order of
/// [`name_ids`].
const FIELD_KINDS: [NameKind; 5] = [
    NameKind::Player,
    NameKind::Player,
    NameKind::Event,
    NameKind::Site,
    NameKind::Round,
];

/// What to do besides the repair of the names and of the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepairOptions {
    /// Remove from the index the entries whose record is outside the `.sg4`
    /// or cannot be parsed (the criterion of
    /// [`compact_games`](super::compact_games), without its `D` flag rule).
    /// Off by default: the games keep their numbers and the `.sg4` is not
    /// even opened. When on, the later games get new numbers.
    pub drop_unreadable: bool,
}

/// What [`repair_database`] found and did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepairReport {
    /// Entries in the repaired index.
    pub games: u32,
    /// Entries found in the damaged index (by the size of the file: the file
    /// wins over the header).
    pub games_read: u32,
    /// Entries the file holds beyond what its header said
    /// (`games_read - header games`, 0 when the header said the same or more).
    /// They come from bytes after the last game the header knew (an
    /// interrupted write, a concatenated file) and are treated as entries,
    /// so they may not open unless [`RepairOptions::drop_unreadable`] is on.
    pub entries_beyond_header: u32,
    /// Names removed (frequency 0 after the recount, or repeated bytes
    /// merged), per kind in the order of [`NameKind::ALL`].
    pub names_removed: [u32; 4],
    /// Name IDs that the `.sn4` did not have and became `?`.
    pub ids_fixed: u32,
    /// Games with at least one such ID.
    pub games_with_bad_ids: u32,
    /// The header said a different number of games than the file holds.
    pub header_fixed: bool,
    /// Bytes at the end of the `.si4` that do not make a whole entry (ignored
    /// and not copied).
    pub trailing_bytes: u64,
    /// Entries removed by [`RepairOptions::drop_unreadable`].
    pub entries_dropped: u32,
    /// The backups made: `.si4` first, then `.sn4`.
    pub backups: Vec<PathBuf>,
    /// The run was cancelled: nothing was changed and no backup was kept.
    pub interrupted: bool,
}

/// A failed [`repair_database`]: the [`Si4Error`] plus the backups that were
/// already made, so the caller can tell the user where the original is.
#[derive(Debug, thiserror::Error)]
#[error("repair failed")]
pub struct RepairError {
    /// What went wrong.
    #[source]
    pub source: Si4Error,
    /// The backups made before the failure (`.si4` first, then `.sn4`); empty
    /// when none was made or the incomplete set was removed again.
    pub backups: Vec<PathBuf>,
}

impl From<Si4Error> for RepairError {
    fn from(source: Si4Error) -> Self {
        RepairError {
            source,
            backups: Vec::new(),
        }
    }
}

/// Repairs the database `base` (the base name or the path of any of its three
/// files); see the module documentation. Reports progress (monotonic, up to
/// three passes over the entries) and looks at `cancel` every 256 entries
/// (and at the first).
///
/// # Errors
///
/// - [`Si4Error::AlreadyOpen`]: a writer or another transaction holds the
///   database;
/// - [`Si4Error::BadMagic`], [`Si4Error::OldVersion`], [`Si4Error::Truncated`]
///   for an unusable `.si4` header or `.sn4`: nothing can be repaired without
///   them (restore a backup);
/// - [`Si4Error::CorruptJournal`] for a journal that cannot be understood;
/// - [`Si4Error::NameLimit`] if `?` cannot be added;
/// - [`Si4Error::Io`] and [`Si4Error::InjectedFailure`].
///
/// Every error is a [`RepairError`], which also lists the backups already made
/// (so the caller can say "your original is at ..."). The originals are intact
/// after any error, except the (impossible to undo)
/// case of a journal that was already committed, which the next
/// [`Si4Writer::open`](super::Si4Writer::open) finishes.
pub fn repair_database(
    base: &Path,
    options: RepairOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<RepairReport, RepairError> {
    repair_database_with_hook(base, options, progress, cancel, &mut FailPoint::never())
}

/// [`repair_database`] with a [`FailPoint`] to simulate a crash at each
/// stage. Besides the stages of the transaction it has `backup:si4` and
/// `backup:sn4`, reached after each backup is complete.
pub fn repair_database_with_hook(
    base: &Path,
    options: RepairOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    hook: &mut FailPoint,
) -> Result<RepairReport, RepairError> {
    let mut backups = Vec::new();
    repair_inner(base, options, progress, cancel, hook, &mut backups).map_err(|source| {
        RepairError {
            source,
            backups: std::mem::take(&mut backups),
        }
    })
}

#[allow(clippy::too_many_lines)]
fn repair_inner(
    base: &Path,
    options: RepairOptions,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
    hook: &mut FailPoint,
    backups: &mut Vec<PathBuf>,
) -> Result<RepairReport> {
    let stem = strip_known_extension(base);
    // A committed journal is finished first (the same as `Si4Writer::open`).
    journal::recover(&stem)?;
    let index_path = find_file(&stem, "si4");
    let names_path = find_file(&stem, "sn4");
    let games_path = find_file(&stem, "sg4");

    let index_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&index_path)
        .map_err(io_error(&index_path))?;
    match index_file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(Si4Error::AlreadyOpen { path: index_path }),
        Err(TryLockError::Error(source)) => {
            return Err(Si4Error::Io {
                path: index_path,
                source,
            });
        }
    }
    let file_len = index_file.metadata().map_err(io_error(&index_path))?.len();
    let mut head = Vec::new();
    (&index_file)
        .take(HEADER_LEN as u64)
        .read_to_end(&mut head)
        .map_err(io_error(&index_path))?;
    let header_bytes = HeaderBytes::from_bytes(&head)?;
    let header = header_bytes.parse()?;
    let name_data = fs::read(&names_path).map_err(io_error(&names_path))?;
    let old_names = NameBase::parse(&name_data)?;

    let body = file_len.saturating_sub(HEADER_LEN as u64);
    let available = body / ENTRY_LEN as u64;
    let count = u32::try_from(available.min(u64::from(MAX_GAMES))).unwrap_or(MAX_GAMES);
    let trailing_bytes = if available > u64::from(MAX_GAMES) {
        body - u64::from(MAX_GAMES) * ENTRY_LEN as u64
    } else {
        body % ENTRY_LEN as u64
    };
    let mut report = RepairReport {
        games_read: count,
        entries_beyond_header: count.saturating_sub(header.num_games()),
        header_fixed: header.num_games() != count,
        trailing_bytes,
        ..RepairReport::default()
    };
    let total = u64::from(count) * 3;
    progress.progress(0, total);

    // Pass 1 (read only): what to keep, how often each name is used.
    let games_file = if options.drop_unreadable {
        Some(File::open(&games_path).map_err(io_error(&games_path))?)
    } else {
        None
    };
    let Some(plan) = analyse(
        &index_file,
        &index_path,
        count,
        &old_names,
        games_file.as_ref().map(|f| (f, games_path.as_path())),
        progress,
        cancel,
    )?
    else {
        report.interrupted = true;
        return Ok(report);
    };
    drop(games_file);
    report.ids_fixed = plan.ids_fixed;
    report.games_with_bad_ids = plan.bad_games;
    report.entries_dropped = plan.dropped;
    report.games = plan.kept;

    // The new name base and the map of the IDs.
    let (fresh, maps, added) = build_names(&old_names, &plan)?;
    for kind in NameKind::ALL {
        let k = kind.index();
        let kept_old = fresh.num_names(kind) - added[k];
        report.names_removed[k] = old_names.num_names(kind).saturating_sub(kept_old);
    }
    let names_bytes = fresh.to_bytes();
    let mut new_header = header_bytes.clone();
    new_header.set_num_games(plan.kept);
    let new_num = [
        fresh.num_names(NameKind::Player),
        fresh.num_names(NameKind::Event),
        fresh.num_names(NameKind::Site),
        fresh.num_names(NameKind::Round),
    ];

    // The originals are saved before anything is replaced.
    make_backups(&index_file, &index_path, &names_path, hook, backups)?;

    let cancelled = Cell::new(false);
    let cancelled_ref = &cancelled;
    let (stem_r, index_r, names_r) = (&stem, &index_path, &names_path);
    let progress_r = &mut *progress;
    // `move`: the handle of the `.si4` is closed when the closure returns,
    // before the renames of the commit (Windows).
    let result = run_transaction(&stem, "repair", hook, move |txn| {
        let (index_path, names_path) = (index_r, names_r);
        let progress = progress_r;
        remove_cache(stem_r);
        txn.stage_file(names_path, |out, tmp| {
            out.write_all(&names_bytes).map_err(io_error(tmp))
        })?;
        txn.stage_file(index_path, |out, tmp| {
            out.write_all(new_header.as_bytes())
                .map_err(io_error(tmp))?;
            let mut done = u64::from(count);
            let mut entry_number = 0u32;
            let finished = stream_entries(&index_file, index_path, count, |bytes| {
                let n = entry_number;
                entry_number += 1;
                done += 1;
                if n.is_multiple_of(CHECK_STRIDE) {
                    progress.progress(done, total);
                    if cancel.is_cancelled() {
                        cancelled_ref.set(true);
                        return Ok(false);
                    }
                }
                if !plan.keeps(n) {
                    return Ok(true);
                }
                let mut entry = *bytes;
                let (parsed, _) = IndexEntry::parse_lenient(&entry)
                    .map_err(|_| unreadable_entry(n.saturating_add(1)))?;
                let ids = name_ids(&parsed);
                let mut mapped = [0u32; 5];
                for (slot, (kind, id)) in mapped.iter_mut().zip(ids) {
                    *slot = maps.lookup(kind, id);
                }
                patch_name_ids(&mut entry, mapped);
                out.write_all(&entry).map_err(io_error(tmp))?;
                Ok(true)
            })?;
            if finished {
                Ok(())
            } else {
                Err(Si4Error::Io {
                    path: index_path.clone(),
                    source: io::Error::new(io::ErrorKind::Interrupted, "repair cancelled"),
                })
            }
        })?;
        // The new files must be complete and consistent before the swap.
        verify_staged(
            &with_suffix(index_path, ".tmp"),
            &with_suffix(names_path, ".tmp"),
            &names_bytes,
            plan.kept,
            new_num,
        )?;
        progress.progress(total, total);
        Ok(())
    });
    match result {
        Ok(()) => {}
        Err(_) if cancelled.get() => {
            // Nothing was replaced: the backups are copies of the originals.
            for backup in backups.drain(..) {
                fs::remove_file(backup).ok();
            }
            report.interrupted = true;
            report.games = 0;
            return Ok(report);
        }
        Err(error) => return Err(error),
    }
    report.backups.clone_from(backups);
    Ok(report)
}

/// The five name IDs of an entry: White, Black, Event, Site, Round.
fn name_ids(entry: &IndexEntry) -> [(NameKind, u32); 5] {
    [
        (FIELD_KINDS[0], entry.white_id),
        (FIELD_KINDS[1], entry.black_id),
        (FIELD_KINDS[2], entry.event_id),
        (FIELD_KINDS[3], entry.site_id),
        (FIELD_KINDS[4], entry.round_id),
    ]
}

fn unreadable_entry(game: u32) -> Si4Error {
    Si4Error::CorruptIndex {
        game,
        reason: "entry cannot be parsed",
    }
}

/// What the first pass learned.
struct Plan {
    /// One bit per entry (0-based): kept.
    keep: Vec<u64>,
    kept: u32,
    dropped: u32,
    ids_fixed: u32,
    bad_games: u32,
    /// Uses of each old name ID by the kept entries, per kind.
    uses: [Vec<u32>; 4],
    /// Name fields that point outside the name base, per kind.
    out_of_range: [u32; 4],
}

impl Plan {
    fn keeps(&self, zero_based: u32) -> bool {
        let word = usize::try_from(zero_based >> 6).unwrap_or(usize::MAX);
        self.keep
            .get(word)
            .is_some_and(|w| (w >> (zero_based & 63)) & 1 == 1)
    }
}

/// Pass 1. `None` when cancelled.
#[allow(clippy::too_many_arguments)]
fn analyse(
    index_file: &File,
    index_path: &Path,
    count: u32,
    names: &NameBase,
    records: Option<(&File, &Path)>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Option<Plan>> {
    let total = u64::from(count) * 3;
    let mut plan = Plan {
        keep: vec![0; usize::try_from(count.div_ceil(64)).unwrap_or(0)],
        kept: 0,
        dropped: 0,
        ids_fixed: 0,
        bad_games: 0,
        uses: NameKind::ALL.map(|k| vec![0u32; to_usize(names.num_names(k))]),
        out_of_range: [0; 4],
    };
    let mut source = match records {
        Some((file, path)) => Some(GameFile::new(file, path.to_path_buf())?),
        None => None,
    };
    let mut n = 0u32;
    let finished = stream_entries(index_file, index_path, count, |bytes| {
        let zero_based = n;
        n += 1;
        if zero_based.is_multiple_of(CHECK_STRIDE) {
            progress.progress(u64::from(n), total);
            if cancel.is_cancelled() {
                return Ok(false);
            }
        }
        // `parse_lenient` accepts any 47 bytes today. If it ever rejects one,
        // fail instead of silently dropping the entry: dropping is the job of
        // `drop_unreadable` (the record check below) alone.
        let (entry, _) = IndexEntry::parse_lenient(bytes).map_err(|_| unreadable_entry(n))?;
        if let Some(source) = &mut source {
            let readable = source
                .read_record(n, entry.offset, entry.length)
                .is_ok_and(|record| RawGame::parse(record).is_ok());
            if !readable {
                plan.dropped += 1;
                return Ok(true);
            }
        }
        let mut bad = false;
        for (kind, id) in name_ids(&entry) {
            let k = kind.index();
            if let Some(uses) = plan.uses[k].get_mut(to_usize(id)) {
                *uses = uses.saturating_add(1);
            } else {
                plan.out_of_range[k] = plan.out_of_range[k].saturating_add(1);
                plan.ids_fixed = plan.ids_fixed.saturating_add(1);
                bad = true;
            }
        }
        plan.bad_games += u32::from(bad);
        if let Some(word) = plan.keep.get_mut(to_usize(zero_based >> 6)) {
            *word |= 1 << (zero_based & 63);
        }
        plan.kept += 1;
        Ok(true)
    })?;
    progress.progress(u64::from(count), total);
    Ok(finished.then_some(plan))
}

fn to_usize(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// The map from old IDs to new ones.
struct Maps {
    /// Per kind, indexed by old ID; `u32::MAX` for a removed name.
    ids: [Vec<u32>; 4],
    /// Per kind: the new ID of `?`, for IDs the `.sn4` did not have.
    unknown: [u32; 4],
}

impl Maps {
    fn lookup(&self, kind: NameKind, id: u32) -> u32 {
        let k = kind.index();
        self.ids[k]
            .get(to_usize(id))
            .copied()
            .filter(|&new| new != u32::MAX)
            .unwrap_or(self.unknown[k])
    }
}

/// Builds the new name base (used names in the old order, `?` when some ID
/// was out of range and the name is not there yet) with the recounted
/// frequencies, the map of the IDs and how many `?` were added per kind.
fn build_names(old: &NameBase, plan: &Plan) -> Result<(NameBuilder, Maps, [u32; 4])> {
    let mut fresh = NameBuilder::new();
    let mut maps = Maps {
        ids: Default::default(),
        unknown: [0; 4],
    };
    let mut added = [0u32; 4];
    for kind in NameKind::ALL {
        let k = kind.index();
        let mut frequencies: Vec<(u32, u32)> = Vec::new();
        for (id, &uses) in plan.uses[k].iter().enumerate() {
            let old_id = u32::try_from(id).unwrap_or(u32::MAX);
            if uses == 0 {
                maps.ids[k].push(u32::MAX);
                continue;
            }
            let name = old.get(kind, old_id).unwrap_or(&[]);
            let new_id = fresh.add_name(kind, name)?;
            maps.ids[k].push(new_id);
            frequencies.push((new_id, uses));
        }
        if plan.out_of_range[k] > 0 {
            let before = fresh.num_names(kind);
            let question = fresh.add_name(kind, b"?")?;
            added[k] = fresh.num_names(kind) - before;
            maps.unknown[k] = question;
            frequencies.push((question, plan.out_of_range[k]));
        }
        for (new_id, uses) in frequencies {
            fresh.inc_frequency(kind, new_id, i32::try_from(uses).unwrap_or(i32::MAX));
        }
    }
    Ok((fresh, maps, added))
}

/// Calls `visit` with the bytes of each of the `count` entries, in order,
/// stopping when it answers `false`. Returns whether every entry was visited.
fn stream_entries(
    file: &File,
    path: &Path,
    count: u32,
    mut visit: impl FnMut(&[u8; ENTRY_LEN]) -> Result<bool>,
) -> Result<bool> {
    let mut handle = file;
    handle
        .seek(SeekFrom::Start(HEADER_LEN as u64))
        .map_err(io_error(path))?;
    let body = u64::from(count) * ENTRY_LEN as u64;
    let mut reader = BufReader::with_capacity(1 << 16, handle.take(body));
    let mut buffer = [0u8; ENTRY_LEN];
    for n in 1..=count {
        reader.read_exact(&mut buffer).map_err(|source| {
            if source.kind() == io::ErrorKind::UnexpectedEof {
                Si4Error::Truncated {
                    file: Si4File::Index,
                    offset: HEADER_LEN as u64 + ENTRY_LEN as u64 * u64::from(n - 1),
                    need: ENTRY_LEN as u64,
                }
            } else {
                Si4Error::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
        if !visit(&buffer)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Copies the `.si4` (through the handle that holds its lock, because on
/// Windows a second handle could not read it) and the `.sn4` to the first
/// free `.bak`, `.bak1`... generation. A backup is written as a temporary and
/// then linked to its final name, which refuses to replace a file that
/// appeared meanwhile, so a file called `.bak` is always complete and is never
/// overwritten. `made` gets the backups of this run; when the second one
/// cannot be made the first is removed again (it is ours), leaving `made`
/// empty.
fn make_backups(
    index_file: &File,
    index_path: &Path,
    names_path: &Path,
    hook: &mut FailPoint,
    made: &mut Vec<PathBuf>,
) -> Result<()> {
    for generation in 0..=MAX_BACKUPS {
        let suffix = if generation == 0 {
            ".bak".to_owned()
        } else {
            format!(".bak{generation}")
        };
        let si4_backup = with_suffix(index_path, &suffix);
        let sn4_backup = with_suffix(names_path, &suffix);
        let mut handle = index_file;
        handle
            .seek(SeekFrom::Start(0))
            .map_err(io_error(index_path))?;
        if !copy_new(&mut handle, &si4_backup)? {
            continue;
        }
        made.push(si4_backup.clone());
        hook.check("backup:si4")?;
        let second = File::open(names_path)
            .map_err(io_error(names_path))
            .and_then(|mut names| copy_new(&mut names, &sn4_backup));
        match second {
            Ok(true) => {}
            Ok(false) => {
                // The `.sn4` of this generation is taken: try the next one.
                fs::remove_file(&si4_backup).ok();
                made.clear();
                continue;
            }
            Err(error) => {
                fs::remove_file(&si4_backup).ok();
                made.clear();
                return Err(error);
            }
        }
        made.push(sn4_backup);
        hook.check("backup:sn4")?;
        return Ok(());
    }
    Err(Si4Error::Io {
        path: index_path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::AlreadyExists, "too many backups"),
    })
}

/// Writes everything `source` yields to `dest` through `dest.tmp`, never
/// replacing an existing `dest`. `Ok(false)` when `dest` already exists.
fn copy_new(source: &mut impl Read, dest: &Path) -> Result<bool> {
    let tmp = with_suffix(dest, ".tmp");
    let result = (|| {
        let mut file = File::create(&tmp).map_err(io_error(&tmp))?;
        io::copy(source, &mut file).map_err(io_error(&tmp))?;
        file.sync_all().map_err(io_error(&tmp))?;
        drop(file);
        match fs::hard_link(&tmp, dest) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            // No hard links here (FAT, some network shares): create the file
            // exclusively and copy into it.
            Err(_) => copy_exclusive(&tmp, dest),
        }
    })();
    fs::remove_file(&tmp).ok();
    if matches!(result, Ok(true)) {
        sync_parent(dest);
    }
    result
}

/// Fallback of [`copy_new`]: `dest` is created with `create_new` and filled
/// from `tmp`; a failure removes the partial file.
fn copy_exclusive(tmp: &Path, dest: &Path) -> Result<bool> {
    let mut out = match OpenOptions::new().write(true).create_new(true).open(dest) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(io_error(dest)(error)),
    };
    let copied = File::open(tmp)
        .and_then(|mut input| io::copy(&mut input, &mut out))
        .and_then(|_| out.sync_all());
    drop(out);
    match copied {
        Ok(()) => Ok(true),
        Err(error) => {
            fs::remove_file(dest).ok();
            Err(io_error(dest)(error))
        }
    }
}

/// Reads the staged files back: the names equal the bytes that were meant to
/// be written and parse, the index has the header and the size of `kept`
/// entries and every name ID of every entry exists.
pub(crate) fn verify_staged(
    index_tmp: &Path,
    names_tmp: &Path,
    names_bytes: &[u8],
    kept: u32,
    new_num: [u32; 4],
) -> Result<()> {
    let wrong = |path: &Path, message: &'static str| Si4Error::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidData, message),
    };
    let written = fs::read(names_tmp).map_err(io_error(names_tmp))?;
    if written != names_bytes {
        return Err(wrong(names_tmp, "staged name file differs from its source"));
    }
    let names = NameBase::parse(&written)?;
    let file = File::open(index_tmp).map_err(io_error(index_tmp))?;
    let size = file.metadata().map_err(io_error(index_tmp))?.len();
    if size != HEADER_LEN as u64 + ENTRY_LEN as u64 * u64::from(kept) {
        return Err(wrong(index_tmp, "staged index has the wrong size"));
    }
    let mut head = Vec::new();
    (&file)
        .take(HEADER_LEN as u64)
        .read_to_end(&mut head)
        .map_err(io_error(index_tmp))?;
    if Header::parse(&head)?.num_games() != kept {
        return Err(wrong(index_tmp, "staged header counts the wrong games"));
    }
    let mut number = 0u32;
    stream_entries(&file, index_tmp, kept, |bytes| {
        number = number.saturating_add(1);
        let (entry, _) = IndexEntry::parse_lenient(bytes).map_err(|_| unreadable_entry(number))?;
        entry
            .verify_names(&names)
            .map_err(|_| wrong(index_tmp, "staged entry uses a name that does not exist"))?;
        Ok(true)
    })?;
    // The counts of the base must be what the plan built.
    for (kind, expected) in NameKind::ALL.into_iter().zip(new_num) {
        if names.num_names(kind) != expected {
            return Err(wrong(names_tmp, "staged name file has the wrong names"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::si4::Si4Writer;
    use mychess_core::Game;

    /// A healthy database of two games; returns the `.si4`, the `.sn4`, the
    /// number of names per kind.
    fn staged() -> (tempfile::TempDir, PathBuf, PathBuf, Vec<u8>, [u32; 4]) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("db");
        let mut w = Si4Writer::create(&base, 1).unwrap();
        for white in ["A", "B"] {
            let mut g = Game::new();
            g.tags_mut().white = white.into();
            w.add_game(&g, crate::si4::GameFlags::empty()).unwrap();
        }
        let num = NameKind::ALL.map(|k| w.names().num_names(k));
        drop(w);
        let si4 = dir.path().join("db.si4");
        let sn4 = dir.path().join("db.sn4");
        let bytes = fs::read(&sn4).unwrap();
        (dir, si4, sn4, bytes, num)
    }

    #[test]
    fn verify_staged_accepts_a_consistent_pair() {
        let (_dir, si4, sn4, bytes, num) = staged();
        verify_staged(&si4, &sn4, &bytes, 2, num).unwrap();
    }

    #[test]
    fn verify_staged_refuses_a_names_file_that_differs_from_its_source() {
        let (dir, si4, sn4, mut bytes, num) = staged();
        // The expected bytes are not what is on disk.
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let error = verify_staged(&si4, &sn4, &bytes, 2, num).unwrap_err();
        assert!(matches!(error, Si4Error::Io { .. }), "{error:?}");
        drop(dir);
    }

    #[test]
    fn verify_staged_refuses_the_wrong_size_and_the_wrong_header_count() {
        let (_dir, si4, sn4, bytes, num) = staged();
        // Claims 3 entries; the file has 2.
        let error = verify_staged(&si4, &sn4, &bytes, 3, num).unwrap_err();
        assert!(matches!(error, Si4Error::Io { .. }), "{error:?}");
        // Right size, but the header counts another number of games.
        let mut data = fs::read(&si4).unwrap();
        data[16] = 9;
        fs::write(&si4, &data).unwrap();
        let error = verify_staged(&si4, &sn4, &bytes, 2, num).unwrap_err();
        assert!(matches!(error, Si4Error::Io { .. }), "{error:?}");
    }

    #[test]
    fn verify_staged_refuses_an_id_the_names_do_not_have() {
        let (_dir, si4, sn4, bytes, num) = staged();
        let mut data = fs::read(&si4).unwrap();
        // White ID of the second entry: 0x0FFF is beyond the 3 players.
        data[HEADER_LEN + ENTRY_LEN + 10] = 0x0F;
        data[HEADER_LEN + ENTRY_LEN + 11] = 0xFF;
        fs::write(&si4, &data).unwrap();
        let error = verify_staged(&si4, &sn4, &bytes, 2, num).unwrap_err();
        assert!(matches!(error, Si4Error::Io { .. }), "{error:?}");
    }

    #[test]
    fn verify_staged_refuses_other_counts_of_names_than_the_plan() {
        let (_dir, si4, sn4, bytes, mut num) = staged();
        num[0] += 1;
        let error = verify_staged(&si4, &sn4, &bytes, 2, num).unwrap_err();
        assert!(matches!(error, Si4Error::Io { .. }), "{error:?}");
    }

    #[test]
    fn verify_staged_reports_a_missing_file_as_an_io_error() {
        let (dir, si4, sn4, bytes, num) = staged();
        fs::remove_file(&si4).unwrap();
        let error = verify_staged(&si4, &sn4, &bytes, 2, num).unwrap_err();
        assert!(matches!(error, Si4Error::Io { .. }), "{error:?}");
        drop(dir);
    }
}
