// Derived from Scid vs PC 4.27, src/tree.cpp (`TreeCache`), src/tree.h and the
// cache calls of src/tkscid.cpp (`sc_tree_search`, `sc_tree_write`,
// `sc_tree_cachesize`, `sc_tree_cacheinfo`). Copyright (c) 1999-2002 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! [`StcCache`]: the main cache backed by the `.stc` file plus the memory-only
//! backup cache (decisions D1045 to D1050 and D1052).

use std::cell::OnceCell;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use mychess_core::{EcoBook, Position};

use super::StcError;
use super::extras::entry_extras;
use super::format::{
    CompressedFilter, FILE_VERSION, HEADER_LEN, Reader, StcEntry, StcHeader, board_key,
};
use super::policy::{Policy, Slots};
use crate::tree::{CachedTree, MAX_TREE_NODES, TreeCache};

/// The suffix of the cache file (`TREEFILE_SUFFIX`).
pub const STC_SUFFIX: &str = ".stc";
/// Capacity of the main cache (`SCID_TreeCacheSize`).
pub const DEFAULT_CACHE_SIZE: usize = 1000;
/// Capacity of the backup cache (`SCID_BackupCacheSize`).
pub const BACKUP_CACHE_SIZE: usize = 100;
/// The largest capacity accepted from a file or from [`StcCache::resize`].
pub const MAX_CACHE_SIZE: usize = 100_000;
/// A file bigger than this is not read (decision D1048). Legitimate files of
/// the Scid can be this big.
pub const MAX_STC_BYTES: u64 = 512 * 1024 * 1024;
/// The writer stops including entries when the file would pass this size
/// (decision D1062), half of what the reader accepts, so a file written by
/// MyChess is always readable by it.
pub const MAX_WRITE_BYTES: u64 = 256 * 1024 * 1024;

/// Longest part of the destination name kept in the temporary name, so the
/// whole name stays under the 255 bytes of common file systems.
const MAX_TEMP_NAME_BYTES: usize = 150;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The path of the cache file of a database: `base_path` without its SI4
/// extension (`.si4`, `.sn4` or `.sg4`, in any case) plus `.stc`, in the same
/// directory.
///
/// ```
/// use std::path::Path;
/// use mychess_db::tree::stc::stc_path;
///
/// assert_eq!(stc_path(Path::new("games/tactics.si4")), Path::new("games/tactics.stc"));
/// assert_eq!(stc_path(Path::new("games/tactics")), Path::new("games/tactics.stc"));
/// ```
pub fn stc_path(base_path: &Path) -> PathBuf {
    let known = base_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            ["si4", "sn4", "sg4"]
                .iter()
                .any(|k| e.eq_ignore_ascii_case(k))
        });
    let stem = if known {
        base_path.with_extension("")
    } else {
        base_path.to_path_buf()
    };
    let mut name: OsString = stem.into_os_string();
    name.push(STC_SUFFIX);
    PathBuf::from(name)
}

/// Removes the cache file of a database; a missing file is not an error.
/// Returns whether a file was removed.
///
/// # Errors
///
/// [`StcError::Io`] if the file exists and cannot be removed.
pub fn remove_stc(base_path: &Path) -> Result<bool, StcError> {
    let path = stc_path(base_path);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StcError::Io { path, source }),
    }
}

/// Follows a symlink at `path` to the file it points to; any other path (or a
/// link that cannot be resolved, e.g. dangling) is returned unchanged.
pub(crate) fn resolve_target(path: &Path) -> PathBuf {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

/// At most [`MAX_TEMP_NAME_BYTES`] bytes of `name`, cut at a char boundary.
fn truncate_name(name: &str) -> &str {
    let mut end = 0;
    for (i, c) in name.char_indices() {
        let next = i + c.len_utf8();
        if next > MAX_TEMP_NAME_BYTES {
            break;
        }
        end = next;
    }
    name.get(..end).unwrap_or_default()
}

/// Creates a fresh, exclusively owned temporary file in the directory of
/// `path` (`.{name}.{pid}.{n}.tmp`, as `mychess-book` does).
pub(crate) fn create_temp(path: &Path) -> std::io::Result<(PathBuf, File)> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = path
        .file_name()
        .map_or_else(|| "cache".into(), |n| n.to_string_lossy().into_owned());
    let name = truncate_name(&name);
    let mut last_error = None;
    for _ in 0..16 {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => return Ok((tmp_path, file)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => last_error = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("no temporary name available")))
}

/// The cache of the opening tree: a main layer of 1000 trees with the
/// [`Policy::Smallest`] policy, stored in the `.stc` file, and a backup layer
/// of 100 trees with [`Policy::Oldest`] that lives only in memory.
///
/// As a [`TreeCache`] it is looked up main first, then backup, and a stored
/// tree goes to both. The file is read lazily, the first time the cache is
/// used while empty; a missing or invalid file is not an error there (the
/// cause is kept for [`StcCache::take_load_error`]).
#[derive(Debug)]
pub struct StcCache {
    base: Option<PathBuf>,
    main: Slots,
    backup: Slots,
    loaded: bool,
    load_error: Option<StcError>,
    /// Entries left out by the last successful write (`write` takes `&self`).
    last_skipped: AtomicUsize,
}

impl StcCache {
    /// A cache for the database whose files start at `base_path` (any of its
    /// SI4 paths); `None` for a database that has no file (Clipbase, PGN):
    /// then nothing is ever read or written.
    pub fn new(base_path: Option<&Path>) -> StcCache {
        StcCache {
            base: base_path.map(Path::to_path_buf),
            main: Slots::new(Policy::Smallest, DEFAULT_CACHE_SIZE),
            backup: Slots::new(Policy::Oldest, BACKUP_CACHE_SIZE),
            loaded: false,
            load_error: None,
            last_skipped: AtomicUsize::new(0),
        }
    }

    /// The database path given to [`StcCache::new`].
    pub fn base_path(&self) -> Option<&Path> {
        self.base.as_deref()
    }

    /// `(trees in use, capacity)` of the main cache (`sc_tree cacheinfo`).
    pub fn info(&self) -> (usize, usize) {
        (self.main.len(), self.main.capacity())
    }

    /// `(trees in use, capacity)` of the memory-only backup cache.
    pub fn backup_info(&self) -> (usize, usize) {
        (self.backup.len(), self.backup.capacity())
    }

    /// `(LowestTotal, LowestTotalIndex)` of the main cache, `None` if empty.
    pub fn main_lowest(&self) -> Option<(u32, usize)> {
        (!self.main.is_empty()).then(|| self.main.lowest())
    }

    /// Empties both layers (`Clear`). The file stays; the next use reads it
    /// again, so call [`remove_stc`] too when the database changed
    /// (decision D1050).
    pub fn clear(&mut self) {
        self.main.clear();
        self.backup.clear();
        self.loaded = false;
        self.load_error = None;
    }

    /// Sets the capacity of the main cache, limited to `1..=MAX_CACHE_SIZE`
    /// (`CacheResize`): a smaller size empties it, a bigger one keeps its
    /// trees (decision D1047). The `.stc` file is untouched.
    pub fn resize(&mut self, size: usize) {
        self.main.resize(size.clamp(1, MAX_CACHE_SIZE));
    }

    /// The error of the last lazy read, if it failed for a reason other than
    /// a missing file. The caller logs it.
    pub fn take_load_error(&mut self) -> Option<StcError> {
        self.load_error.take()
    }

    /// Reads the cache file of this cache's database, if it has one.
    /// Does nothing when the main cache is not empty (`ReadFile`).
    ///
    /// # Errors
    ///
    /// See [`StcCache::read_from`].
    pub fn read(&mut self) -> Result<(), StcError> {
        match self.base.clone() {
            Some(base) => self.read_from(&base),
            None => Ok(()),
        }
    }

    /// Reads the cache file of the database at `base_path` into the main
    /// cache. Does nothing when the main cache is not empty. The `CacheSize`
    /// of the file replaces the configured one (limited to
    /// `1..=MAX_CACHE_SIZE`). On an error the cache is left as it was.
    ///
    /// # Errors
    ///
    /// [`StcError::Io`] if the file cannot be opened or read (including a
    /// missing file), [`StcError::TooLarge`] above [`MAX_STC_BYTES`] and
    /// [`StcError::Corrupt`] for a wrong magic or any invalid field: the whole
    /// file is rejected (decision D1048).
    pub fn read_from(&mut self, base_path: &Path) -> Result<(), StcError> {
        if !self.main.is_empty() {
            return Ok(());
        }
        let path = stc_path(base_path);
        let io = |source| StcError::Io {
            path: path.clone(),
            source,
        };
        let file = File::open(&path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        if len > MAX_STC_BYTES {
            return Err(StcError::TooLarge { len });
        }
        let mut reader = Reader::new(BufReader::new(file), len, &path);
        let header = reader.header()?;
        let capacity = usize::try_from(header.cache_size)
            .ok()
            .filter(|n| (1..=MAX_CACHE_SIZE).contains(n))
            .ok_or_else(|| StcError::corrupt("cache size out of range"))?;
        let in_use = usize::try_from(header.num_in_use)
            .ok()
            .filter(|n| *n <= capacity)
            .ok_or_else(|| StcError::corrupt("more trees than the cache size"))?;
        let mut entries = Vec::new();
        for _ in 0..in_use {
            entries.push(reader.entry()?);
        }
        let lowest_index = usize::try_from(header.lowest_total_index).unwrap_or(usize::MAX);
        self.main
            .load(capacity, entries, (header.lowest_total, lowest_index));
        Ok(())
    }

    /// Writes the main cache to the file of its database and returns the
    /// size of the file; `0` and no file for a cache without a database
    /// (`sc_tree write` on a memory-only base).
    ///
    /// This is synchronous and can be heavy (it encodes up to 100000 trees,
    /// computes SAN and ECO codes, and syncs the file): the application must
    /// call it from an asynchronous `Effect`, on a blocking thread
    /// (`spawn_blocking`), never from `update` or `view`.
    ///
    /// # Errors
    ///
    /// See [`StcCache::write_to`].
    pub fn write(&self, eco_book: Option<&EcoBook>) -> Result<u64, StcError> {
        match &self.base {
            Some(base) => self.write_to(base, eco_book),
            None => Ok(0),
        }
    }

    /// Writes the main cache to the `.stc` file of the database at
    /// `base_path`: a uniquely named temporary file in the same directory
    /// (created exclusively, with the permissions of the old file), flushed,
    /// synced and closed before the rename that replaces the old file
    /// (decisions D1049 and D1062). A symbolic link at the destination is
    /// followed, so the link is kept. `eco_book` fills the ECO code of the
    /// nodes (0 without it).
    ///
    /// The file never gets more than [`MAX_WRITE_BYTES`]: when the cache is
    /// bigger, the entries with the lowest totals are left out (D1062). An
    /// entry in which a sum does not fit 32 bits is left out too (D1064). The
    /// number of entries left out is in [`StcCache::last_write_skipped`].
    ///
    /// # Errors
    ///
    /// [`StcError::Io`]; the old file is kept and the temporary one removed.
    pub fn write_to(&self, base_path: &Path, eco_book: Option<&EcoBook>) -> Result<u64, StcError> {
        self.write_capped(base_path, eco_book, MAX_WRITE_BYTES)
    }

    /// The entries the last successful write left out, because of the size
    /// limit ([`MAX_WRITE_BYTES`]) or because a sum did not fit 32 bits
    /// (decision D1064). `0` before any write.
    pub fn last_write_skipped(&self) -> usize {
        self.last_skipped.load(Ordering::Relaxed)
    }

    fn write_capped(
        &self,
        base_path: &Path,
        eco_book: Option<&EcoBook>,
        cap: u64,
    ) -> Result<u64, StcError> {
        let dest = resolve_target(&stc_path(base_path));
        let io_dest = |source| StcError::Io {
            path: dest.clone(),
            source,
        };
        let permissions = match fs::metadata(&dest) {
            Ok(meta) => Some(meta.permissions()),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => return Err(io_dest(e)),
        };
        let (tmp, file) = create_temp(&dest).map_err(io_dest)?;
        let result = self.write_file(file, &tmp, permissions, eco_book, cap);
        let outcome = match result {
            Ok((size, skipped)) => fs::rename(&tmp, &dest)
                .map(|()| (size, skipped))
                .map_err(io_dest),
            Err(e) => Err(e),
        };
        match outcome {
            Ok((size, skipped)) => {
                self.last_skipped.store(skipped, Ordering::Relaxed);
                Ok(size)
            }
            Err(e) => {
                fs::remove_file(&tmp).ok();
                Err(e)
            }
        }
    }

    /// The entries to write, in file order, and how many are left out: the
    /// ones with sums over 32 bits, then the ones with the lowest totals once
    /// `cap` is reached.
    fn plan_write(&self, cap: u64) -> (Vec<usize>, usize) {
        let entries = self.main.entries();
        let mut order: Vec<usize> = (0..entries.len())
            .filter(|&i| entries.get(i).is_some_and(StcEntry::sums_fit))
            .collect();
        // Highest total first; the file order breaks ties.
        order.sort_by_key(|&i| {
            (
                std::cmp::Reverse(entries.get(i).map_or(0, |e| e.total_count)),
                i,
            )
        });
        let mut budget = cap.saturating_sub(HEADER_LEN as u64);
        let mut kept = Vec::new();
        for i in order {
            let size = entries.get(i).map_or(u64::MAX, StcEntry::encoded_len);
            if size > budget {
                break;
            }
            budget -= size;
            kept.push(i);
        }
        kept.sort_unstable();
        let skipped = entries.len() - kept.len();
        (kept, skipped)
    }

    fn write_file(
        &self,
        file: File,
        path: &Path,
        permissions: Option<fs::Permissions>,
        eco_book: Option<&EcoBook>,
        cap: u64,
    ) -> Result<(u64, usize), StcError> {
        let io = |source| StcError::Io {
            path: path.to_path_buf(),
            source,
        };
        let too_big = || StcError::corrupt("cache has too many entries");
        let (kept, skipped) = self.plan_write(cap);
        let entries = self.main.entries();
        let (lowest_total, lowest_index) = if skipped == 0 {
            self.main.lowest()
        } else {
            // First of the lowest totals among the entries that are written.
            kept.iter()
                .enumerate()
                .filter_map(|(at, &i)| entries.get(i).map(|e| (e.total_count, at)))
                .min()
                .unwrap_or((0, 0))
        };
        let in_use = !kept.is_empty();
        let header = StcHeader {
            version: FILE_VERSION,
            cache_size: u32::try_from(self.main.capacity()).map_err(|_| too_big())?,
            num_in_use: u32::try_from(kept.len()).map_err(|_| too_big())?,
            lowest_total: if in_use { lowest_total } else { 0 },
            lowest_total_index: if in_use {
                u32::try_from(lowest_index).map_err(|_| too_big())?
            } else {
                0
            },
        };
        let mut out = BufWriter::new(file);
        let mut written = 0u64;
        out.write_all(&header.to_bytes()).map_err(io)?;
        written += HEADER_LEN as u64;
        let mut buf = Vec::new();
        for &i in &kept {
            let Some(entry) = entries.get(i) else {
                continue;
            };
            buf.clear();
            entry.encode(&entry_extras(entry, eco_book), &mut buf)?;
            out.write_all(&buf).map_err(io)?;
            written += u64::try_from(buf.len()).unwrap_or(u64::MAX);
        }
        // Flush and sync, then close the file: the rename comes after (the
        // Windows lock).
        out.flush().map_err(io)?;
        let file = out.into_inner().map_err(|e| io(e.into_error()))?;
        if let Some(p) = permissions {
            file.set_permissions(p).map_err(io)?;
        }
        file.sync_all().map_err(io)?;
        drop(file);
        Ok((written, skipped))
    }

    /// Reads the file once, the first time the cache is used while empty.
    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if self.base.is_none() || !self.main.is_empty() {
            return;
        }
        if let Err(e) = self.read() {
            let missing =
                matches!(&e, StcError::Io { source, .. } if source.kind() == ErrorKind::NotFound);
            if !missing {
                self.load_error = Some(e);
            }
        }
    }

    fn find_valid(slots: &Slots, pos: &Position, len: u32) -> Option<CachedTree> {
        let index = slots.find(pos.turn(), &board_key(pos))?;
        let entry = slots.entries().get(index)?;
        // A hit only counts for a database of the same number of games
        // (decision D1045).
        if entry.filter.size != len {
            return None;
        }
        let filter = entry.filter.uncompress().ok()?;
        Some(CachedTree {
            nodes: entry.nodes.clone(),
            filter,
        })
    }
}

impl TreeCache for StcCache {
    fn lookup(&mut self, pos: &Position, len: u32) -> Option<CachedTree> {
        self.ensure_loaded();
        Self::find_valid(&self.main, pos, len).or_else(|| Self::find_valid(&self.backup, pos, len))
    }

    /// The file is read first if the cache is still empty (decision D1048),
    /// so a tree stored in a fresh instance joins the ones of the file. An
    /// entry for the same position but another number of games stays where it
    /// is (it never hits again, but is not replaced); the position is searched
    /// in full each time until the entry is evicted or the cache is cleared
    /// (decision D1065).
    fn store(&mut self, pos: &Position, tree: CachedTree) {
        if tree.nodes.len() > MAX_TREE_NODES {
            return;
        }
        self.ensure_loaded();
        let board = board_key(pos);
        let to_move = pos.turn();
        let total = tree
            .nodes
            .iter()
            .fold(0u32, |acc, n| acc.saturating_add(n.total));
        // Compressed at most once, for the layers that keep the tree.
        let compressed = OnceCell::new();
        let build = || StcEntry {
            to_move,
            board,
            total_count: total,
            nodes: tree.nodes.clone(),
            filter: compressed
                .get_or_init(|| CompressedFilter::compress(&tree.filter))
                .clone(),
        };
        // A stale entry (same position, another number of games) is not
        // replaced: `add_with` only marks it as the most recent (D1065).
        self.main.add_with(to_move, &board, total, build);
        self.backup.add_with(to_move, &board, total, build);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Filter;
    use crate::si4::RawMove;
    use crate::tree::TreeNode;
    use mychess_core::{Color, Square};

    fn tree(len: u32, total: u32) -> CachedTree {
        let mut filter = Filter::new(len);
        filter.fill(0);
        filter.set(2, 3);
        CachedTree {
            nodes: vec![TreeNode {
                mv: Some(RawMove {
                    from: Square::E2,
                    to: Square::E4,
                    promotion: None,
                }),
                total,
                freq: [total, 0, 0, 0],
                elo_sum: 2400,
                elo_count: 1,
                perf_sum: 0,
                perf_count: 0,
                year_sum: 2000,
                year_count: 1,
            }],
            filter,
        }
    }

    #[test]
    fn stc_path_replaces_si4_extensions() {
        assert_eq!(stc_path(Path::new("a/b.SG4")), Path::new("a/b.stc"));
        assert_eq!(stc_path(Path::new("a/b")), Path::new("a/b.stc"));
        assert_eq!(stc_path(Path::new("a/b.v2")), Path::new("a/b.v2.stc"));
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("games.si4");
        let pos = Position::new();
        let mut cache = StcCache::new(Some(&base));
        cache.store(&pos, tree(5, 7));
        assert_eq!(cache.info(), (1, DEFAULT_CACHE_SIZE));
        let size = cache.write(None).expect("write");
        let on_disk = fs::metadata(dir.path().join("games.stc"))
            .expect("file")
            .len();
        assert_eq!(size, on_disk);
        assert_eq!(
            fs::read_dir(dir.path()).expect("dir").count(),
            1,
            "no temporary"
        );

        let mut fresh = StcCache::new(Some(&base));
        let hit = fresh.lookup(&pos, 5).expect("loaded from the file");
        assert_eq!(hit, tree(5, 7));
        assert_eq!(fresh.main_lowest(), Some((7, 0)));
        // Another number of games: no hit.
        assert!(fresh.lookup(&pos, 6).is_none());
        assert!(remove_stc(&base).expect("remove"));
        assert!(!remove_stc(&base).expect("remove again"));
    }

    /// `n` entries with distinct boards and totals `1..=n`.
    fn many_trees(cache: &mut StcCache, n: u32) {
        for i in 0..n {
            let mut board = [7u8; 64];
            board[0] = u8::try_from(i % 14 + 1).expect("small");
            board[1] = u8::try_from(i / 14 + 1).expect("small");
            cache
                .main
                .add_with(Color::White, &board, i + 1, || StcEntry {
                    to_move: Color::White,
                    board,
                    total_count: i + 1,
                    nodes: Vec::new(),
                    filter: CompressedFilter::compress(&Filter::new(5)),
                });
        }
    }

    #[test]
    fn the_size_cap_keeps_the_biggest_totals_and_the_header_is_coherent() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("cap");
        let mut cache = StcCache::new(Some(&base));
        many_trees(&mut cache, 10);
        assert_eq!(cache.info().0, 10);
        let one = cache.main.entries()[0].encoded_len();
        // Room for the header and four entries.
        let cap = HEADER_LEN as u64 + 4 * one;
        let size = cache.write_capped(&base, None, cap).expect("write");
        assert_eq!(size, cap);
        assert_eq!(cache.last_write_skipped(), 6);
        let mut fresh = StcCache::new(Some(&base));
        fresh.read().expect("readable");
        assert_eq!(fresh.info().0, 4);
        let totals: Vec<u32> = fresh.main.entries().iter().map(|e| e.total_count).collect();
        assert_eq!(totals, [7, 8, 9, 10]);
        assert_eq!(fresh.main_lowest(), Some((7, 0)));
        // Without the cap nothing is skipped.
        cache.write(None).expect("write");
        assert_eq!(cache.last_write_skipped(), 0);
    }

    #[test]
    fn entries_with_sums_over_32_bits_are_not_written() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("big");
        let mut cache = StcCache::new(Some(&base));
        let pos = Position::new();
        let mut big = tree(5, 7);
        big.nodes[0].elo_sum = u64::from(u32::MAX) + 1;
        cache.store(&pos, big);
        assert_eq!(cache.write(None).expect("write"), HEADER_LEN as u64);
        assert_eq!(cache.last_write_skipped(), 1);
        let mut fresh = StcCache::new(Some(&base));
        fresh.read().expect("readable");
        assert_eq!(fresh.info().0, 0);
    }

    #[test]
    fn temporary_names_are_unique_short_and_cleaned_up() {
        let dir = tempfile::tempdir().expect("temp dir");
        let long = dir.path().join(format!("{}.stc", "é".repeat(120)));
        let (a, fa) = create_temp(&long).expect("first");
        let (b, fb) = create_temp(&long).expect("second");
        assert_ne!(a, b);
        for p in [&a, &b] {
            let name = p.file_name().expect("name").to_string_lossy().into_owned();
            assert!(name.len() <= 255, "{} bytes", name.len());
            assert!(name.starts_with('.') && name.contains(".tmp"));
        }
        drop((fa, fb));
        // A failed write leaves nothing behind: the destination is a
        // directory, so the rename fails.
        let base = dir.path().join("dirbase");
        fs::create_dir(dir.path().join("dirbase.stc")).expect("dir");
        let cache = StcCache::new(Some(&base));
        assert!(cache.write(None).is_err());
        let leftovers = fs::read_dir(dir.path())
            .expect("dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .count();
        assert_eq!(leftovers, 2, "only the two created above");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_is_followed_and_permissions_kept() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dir.path().join("real.stc");
        fs::write(&real, b"old").expect("write");
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).expect("chmod");
        symlink(&real, dir.path().join("g.stc")).expect("link");
        let base = dir.path().join("g");
        let mut cache = StcCache::new(Some(&base));
        cache.store(&Position::new(), tree(5, 7));
        cache.write(None).expect("write");
        let link = fs::symlink_metadata(dir.path().join("g.stc")).expect("meta");
        assert!(link.file_type().is_symlink(), "the link survives");
        let meta = fs::metadata(&real).expect("meta");
        assert!(meta.len() > 22);
        assert_eq!(meta.permissions().mode() & 0o777, 0o640);
    }

    #[test]
    fn backup_serves_when_the_main_size_differs_and_clear_empties_both() {
        let pos = Position::new();
        let mut cache = StcCache::new(None);
        cache.store(&pos, tree(5, 1));
        assert_eq!(cache.backup_info(), (1, BACKUP_CACHE_SIZE));
        assert!(cache.lookup(&pos, 5).is_some());
        cache.clear();
        assert_eq!((cache.info().0, cache.backup_info().0), (0, 0));
        assert!(cache.lookup(&pos, 5).is_none());
        assert_eq!(cache.write(None).expect("no-op"), 0);
    }

    #[test]
    fn corrupt_file_is_ignored_by_the_lazy_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().join("g");
        fs::write(dir.path().join("g.stc"), b"not a cache file at all").expect("write");
        let mut cache = StcCache::new(Some(&base));
        assert!(cache.lookup(&Position::new(), 1).is_none());
        assert!(matches!(
            cache.take_load_error(),
            Some(StcError::Corrupt { .. })
        ));
        assert_eq!(cache.info().0, 0);
    }
}
