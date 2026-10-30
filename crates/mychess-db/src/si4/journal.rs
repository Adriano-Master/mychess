// Original MyChess design (decision D290); there is no counterpart in Scid vs
// PC 4.27, which swaps files without a journal (docs/specs/m3-2-escrita-si4.md,
// "Atomicidade e ordem").

//! The transaction journal `<base>.txn` for operations that replace several
//! files at once (compacting games or names, upgrading an `.si3`).
//!
//! Protocol (`docs/specs/m3-2-escrita-si4.md`, "Protocolo de gravação
//! segura"):
//!
//! 1. every new file is written complete as `<file>.tmp` and synced;
//! 2. the journal (the list of steps) is written to `<base>.txn.tmp`, synced
//!    and renamed to `<base>.txn`: this is the commit point;
//! 3. the steps run in order (renames of the temporaries, `.si4` last; a
//!    final removal of the `.si3` when upgrading);
//! 4. the journal is removed.
//!
//! The directory is synced (`fsync` of the parent, Unix only and best effort;
//! a no-op elsewhere) after the rename of the journal, which also makes the
//! directory entries of the temporaries durable, and after each step. The
//! temporaries themselves are synced file by file before the journal exists.
//!
//! # Exclusion (decision D321)
//!
//! A transaction holds an exclusive lock on `<base>.lock` from the start of
//! its `build` until the journal is removed, and [`recover`] only touches the
//! files (journal, orphan `.tmp`) when it gets that lock too, so a second
//! process cannot take a live transaction's temporaries for orphans. The
//! lock file is separate from the `.si4` so it is not renamed. Scid ignores
//! it (as it ignores the `.si4` lock, D291). Residual windows: the `.lock`
//! file is removed when the transaction ends, so a process that opens it in
//! that instant can hold a lock on a deleted file; and a writer that opens
//! the database in the instant before the transaction creates the lock and
//! its temporaries is not stopped (it holds the `.si4` lock, which the
//! transaction does not take until the writer is consumed).
//!
//! # Journal validation (decision D320)
//!
//! A journal is validated entirely before any step runs. Every file name must
//! be a single path component without `:` (a Windows alternate data stream),
//! must start with the base name of the journal and a dot, and its extension
//! must be one of `si3 sn3 sg3 si4 sn4 sg4 txn`, optionally followed by
//! `.tmp`. Anything else is [`Si4Error::CorruptJournal`] and nothing runs.
//!
//! A process that dies before step 2 leaves only orphan `.tmp` files, which
//! are removed. A process that dies after it leaves a journal, and the next
//! writer rolls it forward: every step is idempotent (a rename whose source
//! no longer exists was already done). [`Si4Writer::open`](super::Si4Writer::open)
//! does this; [`Si4Database::open`](super::Si4Database::open) only detects the
//! journal and refuses with [`Si4Error::PendingJournal`] (decision D311).
//! [`recover`] lets the application finish it explicitly.
//!
//! # Fail points
//!
//! [`FailPoint`] is a hook for tests of atomicity. Every stage of a
//! transaction calls it with a label; a `FailPoint::at(i)` makes the `i`-th
//! stage return [`Si4Error::InjectedFailure`] **without any cleanup**, which
//! is what a killed process leaves on disk. Run once with
//! [`FailPoint::never`] and read [`FailPoint::reached`] to learn how many
//! stages an operation has. Labels, in order: `begin`, `tmp-begin:<file>` and
//! `tmp:<file>` for each new file, `journal-tmp`, `journal` (committed),
//! `step:<i>` after each step.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::database::{find_file, strip_known_extension};
use super::error::{Result, Si4Error};
use super::write::{io_error, sync_parent, with_suffix};

const JOURNAL_MAGIC: &str = "mychess-txn 1";

/// Hook that lets a test stop an operation at any stage (see the module
/// documentation). Intended for tests only; production code uses
/// [`FailPoint::never`] through the plain functions.
#[derive(Debug, Clone, Default)]
pub struct FailPoint {
    fail_at: Option<usize>,
    fail_label: Option<String>,
    reached: Vec<String>,
    /// The failure simulates a killed process: the code that hit it must not
    /// clean up (see [`FailPoint::kill_at_label`]).
    kill: bool,
    fired_kill: bool,
}

impl FailPoint {
    /// A hook that never fails and only records the stages reached.
    pub fn never() -> FailPoint {
        FailPoint::default()
    }

    /// A hook that fails at stage `index` (0-based, in the order of
    /// [`FailPoint::reached`] of a run that did not fail).
    pub fn at(index: usize) -> FailPoint {
        FailPoint {
            fail_at: Some(index),
            ..FailPoint::default()
        }
    }

    /// A hook that fails the first time a stage with this label is reached.
    pub fn at_label(label: &str) -> FailPoint {
        FailPoint {
            fail_label: Some(label.to_owned()),
            ..FailPoint::default()
        }
    }

    /// Like [`FailPoint::at_label`], but the failure stands for a process that
    /// died: bulk rewrites (`strip_games`, `edit_tags`, `edit_names`) leave on
    /// disk exactly what they had written at that moment, with no cut of the
    /// `.sg4` and no rollback, so a test can inspect the state a crash leaves.
    pub fn kill_at_label(label: &str) -> FailPoint {
        FailPoint {
            fail_label: Some(label.to_owned()),
            kill: true,
            ..FailPoint::default()
        }
    }

    /// Whether a [`FailPoint::kill_at_label`] failure was already raised.
    pub(crate) fn was_killed(&self) -> bool {
        self.fired_kill
    }

    /// The labels of the stages reached so far, in order. Includes the stage
    /// that failed, if any.
    pub fn reached(&self) -> &[String] {
        &self.reached
    }

    pub(crate) fn check(&mut self, label: &str) -> Result<()> {
        let index = self.reached.len();
        self.reached.push(label.to_owned());
        let by_label = self.fail_label.as_deref() == Some(label);
        if by_label {
            self.fail_label = None;
        }
        if self.fail_at == Some(index) || by_label {
            self.fired_kill |= self.kill && by_label;
            return Err(Si4Error::InjectedFailure {
                index,
                label: label.to_owned(),
            });
        }
        Ok(())
    }
}

/// One step of a journal. File names are relative to the directory of the
/// journal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Rename `from` over `to`; skipped when `from` does not exist but `to`
    /// does (already done). Neither existing is a corrupt journal.
    Rename { from: String, to: String },
    /// Remove a file; skipped when it does not exist.
    Remove { name: String },
}

/// What [`recover`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recovery {
    /// A journal was found and rolled forward.
    pub journal_completed: bool,
    /// How many steps of the journal still had work to do.
    pub steps_applied: usize,
    /// How many orphan `.tmp` files were removed.
    pub orphans_removed: usize,
}

fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

fn file_name_text(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.contains(['\t', '\n', '\r']))
        .map(str::to_owned)
        .ok_or_else(|| Si4Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "file name must be UTF-8 without tabs or newlines to be journaled",
            ),
        })
}

const JOURNAL_EXTENSIONS: [&str; 7] = ["si3", "sn3", "sg3", "si4", "sn4", "sg4", "txn"];

/// The base name a journal file name must start with: the file name of the
/// journal without `.txn` (compared without regard to case).
fn base_name_of(journal: &Path) -> String {
    let name = journal
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match name.len().checked_sub(4).and_then(|at| name.get(at..)) {
        Some(ext) if ext.eq_ignore_ascii_case(".txn") => {
            name.get(..name.len() - 4).unwrap_or_default().to_owned()
        }
        _ => name.to_owned(),
    }
}

/// Whether `name` may appear in a journal of the database `base_name`
/// (decision D320).
fn valid_journal_name(base_name: &str, name: &str) -> bool {
    if name.is_empty()
        || name.contains([':', '/', '\\', '\0', '\t', '\n', '\r'])
        || Path::new(name).file_name().is_none_or(|n| n != name)
    {
        return false;
    }
    let Some(head) = name.get(..base_name.len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case(base_name) {
        return false;
    }
    let Some(rest) = name
        .get(base_name.len()..)
        .and_then(|r| r.strip_prefix('.'))
    else {
        return false;
    };
    let mut parts = rest.split('.');
    let extension = parts.next().unwrap_or_default();
    JOURNAL_EXTENSIONS
        .iter()
        .any(|e| e.eq_ignore_ascii_case(extension))
        && parts.all(|p| p.eq_ignore_ascii_case("tmp"))
}

fn journal_path(stem: &Path) -> PathBuf {
    find_file(stem, "txn")
}

/// The journal file of the database, if one is waiting to be finished.
pub fn pending_journal(base: &Path) -> Option<PathBuf> {
    let path = journal_path(&strip_known_extension(base));
    path.exists().then_some(path)
}

fn orphan_tmp_files(stem: &Path) -> Vec<PathBuf> {
    [
        "si4.tmp",
        "sn4.tmp",
        "sg4.tmp",
        "txn.tmp",
        "si4.bak.tmp",
        "sn4.bak.tmp",
    ]
    .iter()
    .map(|ext| find_file(stem, ext))
    .filter(|p| p.exists())
    .collect()
}

/// The `<base>.si4.bakN.tmp` / `<base>.sn4.bakN.tmp` files that a repair that
/// died while copying a backup left behind (the first generation is already
/// in [`orphan_tmp_files`]). Needs a scan of the directory, so only recovery
/// itself does it.
fn orphan_backup_tmp_files(stem: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (stem.parent(), stem.file_name()) else {
        return Vec::new();
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let name = name.to_string_lossy().to_lowercase();
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(std::result::Result::ok)
        .filter(|entry| {
            let file = entry.file_name().to_string_lossy().to_lowercase();
            file.strip_prefix(&name)
                .and_then(|rest| rest.strip_suffix(".tmp"))
                .and_then(|rest| {
                    rest.strip_prefix(".si4.bak")
                        .or(rest.strip_prefix(".sn4.bak"))
                })
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                && entry.file_type().is_ok_and(|t| t.is_file())
        })
        .map(|entry| entry.path())
        .collect()
}

/// Whether there is a journal or an orphan temporary file.
pub(crate) fn needs_recovery(stem: &Path) -> bool {
    journal_path(stem).exists() || !orphan_tmp_files(stem).is_empty()
}

fn serialize(operation: &str, steps: &[Step]) -> String {
    let mut text = format!("{JOURNAL_MAGIC}\nop {operation}\n");
    for step in steps {
        match step {
            Step::Rename { from, to } => {
                let _ = writeln!(text, "rename\t{from}\t{to}");
            }
            Step::Remove { name } => {
                let _ = writeln!(text, "remove\t{name}");
            }
        }
    }
    text.push_str("end\n");
    text
}

fn parse(path: &Path, text: &str) -> Result<Vec<Step>> {
    let corrupt = |reason: &'static str| Si4Error::CorruptJournal {
        path: path.to_path_buf(),
        reason,
    };
    let base_name = base_name_of(path);
    let valid = |name: &str| valid_journal_name(&base_name, name);
    let mut lines = text.lines();
    if lines.next() != Some(JOURNAL_MAGIC) {
        return Err(corrupt("bad magic or unsupported journal version"));
    }
    if !lines.next().is_some_and(|l| l.starts_with("op ")) {
        return Err(corrupt("missing operation line"));
    }
    let mut steps = Vec::new();
    let mut ended = false;
    for line in lines {
        if ended {
            return Err(corrupt("data after the end marker"));
        }
        let mut parts = line.split('\t');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some("end"), None, None, None) => ended = true,
            (Some("rename"), Some(from), Some(to), None) if valid(from) && valid(to) => {
                steps.push(Step::Rename {
                    from: from.to_owned(),
                    to: to.to_owned(),
                });
            }
            (Some("remove"), Some(name), None, None) if valid(name) => {
                steps.push(Step::Remove {
                    name: name.to_owned(),
                });
            }
            _ => return Err(corrupt("invalid step")),
        }
    }
    if !ended {
        return Err(corrupt("missing end marker (truncated journal)"));
    }
    Ok(steps)
}

/// Runs the steps in `dir`; returns how many had work to do. The hook is
/// called after each step.
fn execute(journal: &Path, steps: &[Step], hook: &mut FailPoint) -> Result<usize> {
    let dir = dir_of(journal);
    let mut applied = 0;
    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Rename { from, to } => {
                let (from, to) = (dir.join(from), dir.join(to));
                if from.exists() {
                    fs::rename(&from, &to).map_err(io_error(&to))?;
                    applied += 1;
                } else if !to.exists() {
                    // A temporary that vanished would otherwise look like a
                    // step that was already done.
                    return Err(Si4Error::CorruptJournal {
                        path: journal.to_path_buf(),
                        reason: "neither the source nor the target of a rename exists",
                    });
                }
                sync_parent(&to);
            }
            Step::Remove { name } => {
                let path = dir.join(name);
                match fs::remove_file(&path) {
                    Ok(()) => applied += 1,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(source) => return Err(Si4Error::Io { path, source }),
                }
                sync_parent(&path);
            }
        }
        hook.check(&format!("step:{index}"))?;
    }
    Ok(applied)
}

/// Fails with [`Si4Error::AlreadyOpen`] if another writer holds the lock of
/// the `.si4` file. Nothing is kept locked.
fn ensure_not_locked(stem: &Path) -> Result<()> {
    let path = find_file(stem, "si4");
    if !path.exists() {
        return Ok(());
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(io_error(&path))?;
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(Si4Error::AlreadyOpen { path }),
        Err(TryLockError::Error(source)) => Err(Si4Error::Io { path, source }),
    }
}

/// The exclusive lock of `<base>.lock` (decision D321).
#[derive(Debug)]
pub(crate) struct TxLock {
    file: File,
    path: PathBuf,
}

impl TxLock {
    /// Creates the lock file if needed and locks it; fails with
    /// [`Si4Error::AlreadyOpen`] if someone else holds it.
    pub(crate) fn acquire(stem: &Path) -> Result<TxLock> {
        let path = with_suffix(stem, ".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_error(&path))?;
        match file.try_lock() {
            Ok(()) => Ok(TxLock { file, path }),
            Err(TryLockError::WouldBlock) => Err(Si4Error::AlreadyOpen { path }),
            Err(TryLockError::Error(source)) => Err(Si4Error::Io { path, source }),
        }
    }

    /// Fails with `AlreadyOpen` if a transaction holds the lock; creates and
    /// keeps nothing (a missing lock file means no transaction).
    pub(crate) fn probe(stem: &Path) -> Result<()> {
        let path = with_suffix(stem, ".lock");
        if !path.exists() {
            return Ok(());
        }
        TxLock::acquire(stem).map(TxLock::release)
    }

    /// Unlocks and removes the lock file (best effort).
    pub(crate) fn release(self) {
        let TxLock { file, path } = self;
        drop(file);
        fs::remove_file(path).ok();
    }
}

/// Finishes an interrupted transaction and removes orphan temporary files
/// (decision D290).
///
/// If `<base>.txn` exists it is validated as a whole (decision D320) and
/// rolled forward: the steps whose source file still exists are executed (a
/// rename with neither source nor target is a corrupt journal) and the
/// journal is deleted. Then the leftover
/// `.si4.tmp`, `.sn4.tmp`, `.sg4.tmp` and `.txn.tmp` are deleted. Fails with
/// [`Si4Error::AlreadyOpen`] if a writer has the database open or another
/// process is running a transaction (the `<base>.lock` lock, D321), and with
/// [`Si4Error::CorruptJournal`] (leaving the journal in place) if the journal
/// cannot be understood. [`Si4Writer::open`](super::Si4Writer::open) calls
/// this by itself.
pub fn recover(base: &Path) -> Result<Recovery> {
    let stem = strip_known_extension(base);
    let mut report = Recovery::default();
    // A transaction in progress holds `<base>.lock`: leave its files alone.
    TxLock::probe(&stem)?;
    if !needs_recovery(&stem) {
        return Ok(report);
    }
    ensure_not_locked(&stem)?;
    let lock = TxLock::acquire(&stem)?;
    let result = recover_locked(&stem, &mut report);
    lock.release();
    result.map(|()| report)
}

fn recover_locked(stem: &Path, report: &mut Recovery) -> Result<()> {
    let journal = journal_path(stem);
    if journal.exists() {
        let text = fs::read(&journal).map_err(io_error(&journal))?;
        let text = std::str::from_utf8(&text).map_err(|_| Si4Error::CorruptJournal {
            path: journal.clone(),
            reason: "not valid UTF-8",
        })?;
        let steps = parse(&journal, text)?;
        report.steps_applied = execute(&journal, &steps, &mut FailPoint::never())?;
        report.journal_completed = true;
        fs::remove_file(&journal).map_err(io_error(&journal))?;
        sync_parent(&journal);
    }
    let orphans = orphan_tmp_files(stem)
        .into_iter()
        .chain(orphan_backup_tmp_files(stem));
    for orphan in orphans {
        if fs::remove_file(&orphan).is_ok() {
            report.orphans_removed += 1;
        }
    }
    Ok(())
}

/// A transaction being built: new files are staged (written as `.tmp`) and
/// the steps to install them are recorded.
pub(crate) struct Transaction<'h> {
    journal: PathBuf,
    operation: &'static str,
    steps: Vec<Step>,
    tmp_files: Vec<PathBuf>,
    hook: &'h mut FailPoint,
    committed: bool,
    base_name: String,
}

impl Transaction<'_> {
    /// The journaled name of `path`, refusing a name that recovery would
    /// reject (so a journal we write can always be read back).
    fn journaled(&self, path: &Path) -> Result<String> {
        let name = file_name_text(path)?;
        if valid_journal_name(&self.base_name, &name) {
            Ok(name)
        } else {
            Err(Si4Error::Io {
                path: path.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "file name cannot be journaled (must be <base>.<ext> without ':')",
                ),
            })
        }
    }

    /// Writes `dest.tmp` with `write`, syncs it, and records the rename of
    /// the temporary over `dest`.
    pub(crate) fn stage_file(
        &mut self,
        dest: &Path,
        write: impl FnOnce(&mut BufWriter<File>, &Path) -> Result<()>,
    ) -> Result<()> {
        let tmp = with_suffix(dest, ".tmp");
        let name = self.journaled(dest)?;
        let from = self.journaled(&tmp)?;
        self.tmp_files.push(tmp.clone());
        let file = File::create(&tmp).map_err(io_error(&tmp))?;
        self.hook.check(&format!("tmp-begin:{name}"))?;
        let mut writer = BufWriter::with_capacity(1 << 16, file);
        write(&mut writer, &tmp)?;
        writer.flush().map_err(io_error(&tmp))?;
        let file = writer
            .into_inner()
            .map_err(|e| io_error(&tmp)(e.into_error()))?;
        file.sync_all().map_err(io_error(&tmp))?;
        drop(file);
        self.hook.check(&format!("tmp:{name}"))?;
        self.steps.push(Step::Rename { from, to: name });
        Ok(())
    }

    /// Records the rename of an existing file.
    pub(crate) fn stage_rename(&mut self, from: &Path, to: &Path) -> Result<()> {
        self.steps.push(Step::Rename {
            from: self.journaled(from)?,
            to: self.journaled(to)?,
        });
        Ok(())
    }

    /// Records the removal of a file.
    pub(crate) fn stage_remove(&mut self, path: &Path) -> Result<()> {
        self.steps.push(Step::Remove {
            name: self.journaled(path)?,
        });
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        let tmp = with_suffix(&self.journal, ".tmp");
        self.tmp_files.push(tmp.clone());
        let text = serialize(self.operation, &self.steps);
        let mut file = File::create(&tmp).map_err(io_error(&tmp))?;
        file.write_all(text.as_bytes()).map_err(io_error(&tmp))?;
        file.sync_all().map_err(io_error(&tmp))?;
        drop(file);
        self.hook.check("journal-tmp")?;
        fs::rename(&tmp, &self.journal).map_err(io_error(&self.journal))?;
        sync_parent(&self.journal);
        self.committed = true;
        self.hook.check("journal")?;
        execute(&self.journal, &self.steps, self.hook)?;
        fs::remove_file(&self.journal).map_err(io_error(&self.journal))?;
        sync_parent(&self.journal);
        Ok(())
    }

    /// Removes what a failed transaction staged. Not called for an injected
    /// failure (a killed process cleans nothing) nor after the commit point
    /// (the journal is then the way forward).
    fn abort(&self) {
        for tmp in &self.tmp_files {
            fs::remove_file(tmp).ok();
        }
    }
}

/// Runs a transaction on the database `stem`: `build` stages the new files
/// and steps (it must drop every handle of the database before returning, so
/// the renames work on Windows), then the journal is committed and executed.
pub(crate) fn run_transaction<T>(
    stem: &Path,
    operation: &'static str,
    hook: &mut FailPoint,
    build: impl FnOnce(&mut Transaction<'_>) -> Result<T>,
) -> Result<T> {
    // Held until the journal is gone (or the call fails).
    let lock = TxLock::acquire(stem)?;
    let journal = journal_path(stem);
    let mut txn = Transaction {
        base_name: base_name_of(&journal),
        journal,
        operation,
        steps: Vec::new(),
        tmp_files: Vec::new(),
        hook,
        committed: false,
    };
    let result = txn
        .hook
        .check("begin")
        .and_then(|()| build(&mut txn))
        .and_then(|value| txn.commit().map(|()| value));
    if let Err(error) = &result
        && !matches!(error, Si4Error::InjectedFailure { .. })
        && !txn.committed
    {
        txn.abort();
    }
    lock.release();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_text_round_trips() {
        let steps = vec![
            Step::Rename {
                from: "a.sg4.tmp".into(),
                to: "a.sg4".into(),
            },
            Step::Remove {
                name: "a.si3".into(),
            },
        ];
        let text = serialize("test", &steps);
        assert_eq!(parse(Path::new("a.txn"), &text).unwrap(), steps);
    }

    #[test]
    fn rejects_truncated_and_path_escaping_journals() {
        let p = Path::new("a.txn");
        assert!(parse(p, "mychess-txn 1\nop x\nrename\ta\tb\n").is_err());
        assert!(parse(p, "other\n").is_err());
        assert!(parse(p, "mychess-txn 1\nop x\nrename\t../a\tb\nend\n").is_err());
        assert!(parse(p, "mychess-txn 1\nop x\nremove\t/etc/passwd\nend\n").is_err());
        assert!(parse(p, "mychess-txn 1\nop x\nend\nremove\ta\n").is_err());
    }

    #[test]
    fn fail_point_counts_and_fails_at_the_chosen_stage() {
        let mut never = FailPoint::never();
        never.check("a").unwrap();
        never.check("b").unwrap();
        assert_eq!(never.reached(), ["a", "b"]);
        let mut at = FailPoint::at(1);
        at.check("a").unwrap();
        assert!(matches!(
            at.check("b"),
            Err(Si4Error::InjectedFailure { index: 1, .. })
        ));
    }

    #[test]
    fn transaction_installs_files_and_removes_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let stem = dir.path().join("db");
        let dest = dir.path().join("db.sn4");
        fs::write(&dest, b"old").unwrap();
        let mut hook = FailPoint::never();
        run_transaction(&stem, "test", &mut hook, |txn| {
            txn.stage_file(&dest, |w, tmp| w.write_all(b"new").map_err(io_error(tmp)))
        })
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
        assert!(!journal_path(&stem).exists());
        assert!(!needs_recovery(&stem));
        assert_eq!(hook.reached().first().map(String::as_str), Some("begin"));
    }

    #[test]
    fn every_fail_point_leaves_old_or_new_after_recovery() {
        let mut probe = FailPoint::never();
        let count = {
            let dir = tempfile::tempdir().unwrap();
            let stem = dir.path().join("db");
            let dest = dir.path().join("db.sn4");
            fs::write(&dest, b"old").unwrap();
            run_transaction(&stem, "test", &mut probe, |txn| {
                txn.stage_file(&dest, |w, tmp| w.write_all(b"new").map_err(io_error(tmp)))
            })
            .unwrap();
            probe.reached().len()
        };
        assert!(count >= 5);
        for i in 0..count {
            let dir = tempfile::tempdir().unwrap();
            let stem = dir.path().join("db");
            let dest = dir.path().join("db.sn4");
            fs::write(&dest, b"old").unwrap();
            let mut hook = FailPoint::at(i);
            let result = run_transaction(&stem, "test", &mut hook, |txn| {
                txn.stage_file(&dest, |w, tmp| w.write_all(b"new").map_err(io_error(tmp)))
            });
            assert!(matches!(result, Err(Si4Error::InjectedFailure { .. })));
            let label = hook.reached().last().cloned().unwrap_or_default();
            // Before the commit point the old file is still there.
            let committed = hook.reached().iter().any(|l| l == "journal");
            if !committed {
                assert_eq!(fs::read(&dest).unwrap(), b"old", "{label}");
            }
            recover(&stem).unwrap();
            let content = fs::read(&dest).unwrap();
            let expected: &[u8] = if committed { b"new" } else { b"old" };
            assert_eq!(content, expected, "{label}");
            assert!(!needs_recovery(&stem), "{label}");
        }
    }

    #[test]
    fn journal_names_must_belong_to_the_database() {
        let p = Path::new("a.txn");
        let step = |line: &str| parse(p, &format!("mychess-txn 1\nop x\n{line}\nend\n"));
        assert!(step("remove\ta.si3").is_ok());
        assert!(step("rename\tA.SG4.tmp\ta.sg4").is_ok());
        for bad in [
            "remove\tother.pgn",
            "remove\ta.pgn",
            "remove\ta.si3.bak",
            "remove\tab.si3",
            "remove\ta.si4:stream",
            "remove\ta.si4..tmp",
            "remove\ta.",
            "remove\ta",
            "rename\tx.pdf\ta.si4",
            "rename\ta.si4.tmp\tb.si4",
        ] {
            assert!(
                matches!(step(bad), Err(Si4Error::CorruptJournal { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_rename_with_neither_file_is_a_corrupt_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("db.txn");
        fs::write(
            &journal,
            "mychess-txn 1\nop x\nrename\tdb.sn4.tmp\tdb.sn4\nend\n",
        )
        .unwrap();
        assert!(matches!(
            recover(&dir.path().join("db")),
            Err(Si4Error::CorruptJournal { .. })
        ));
        assert!(journal.exists());
    }

    #[test]
    fn a_planted_journal_runs_nothing_when_any_step_is_foreign() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("other.pgn"), b"keep").unwrap();
        fs::write(dir.path().join("db.sg4.tmp"), b"x").unwrap();
        fs::write(
            dir.path().join("db.txn"),
            "mychess-txn 1\nop x\nrename\tdb.sg4.tmp\tdb.sg4\nremove\tother.pgn\nend\n",
        )
        .unwrap();
        assert!(recover(&dir.path().join("db")).is_err());
        assert!(dir.path().join("other.pgn").exists());
        assert!(dir.path().join("db.sg4.tmp").exists());
        assert!(!dir.path().join("db.sg4").exists());
    }

    #[test]
    fn recover_and_transactions_respect_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let stem = dir.path().join("db");
        fs::write(dir.path().join("db.sn4.tmp"), b"live").unwrap();
        let lock = TxLock::acquire(&stem).unwrap();
        assert!(matches!(recover(&stem), Err(Si4Error::AlreadyOpen { .. })));
        assert!(dir.path().join("db.sn4.tmp").exists());
        let mut hook = FailPoint::never();
        let result = run_transaction(&stem, "test", &mut hook, |_| Ok(()));
        assert!(matches!(result, Err(Si4Error::AlreadyOpen { .. })));
        lock.release();
        assert_eq!(recover(&stem).unwrap().orphans_removed, 1);
        assert!(!dir.path().join("db.lock").exists());
    }
}
