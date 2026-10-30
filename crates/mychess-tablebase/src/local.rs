//! Local Syzygy tablebases (spec m8-8, D2555, D2558, D2559).
//!
//! [`LocalTablebase::open`] registers the `.rtbw` (WDL) and `.rtbz` (DTZ)
//! files of some folders; the files are only opened at the first probe. Syzygy
//! has no distance to mate (D2554): the evaluation carries the DTZ when the
//! `.rtbz` file is there.
//!
//! The probing code of `shakmaty-syzygy` is written for intact files. The
//! files here come from the user's disk, so every call is wrapped in
//! `catch_unwind` and a panic becomes [`TablebaseError::Internal`].

use std::fmt;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use mychess_core::Position;
use shakmaty::Chess;
use shakmaty_syzygy::Tablebase;

use crate::category::{Evaluation, PositionReport};
use crate::convert;
use crate::error::{Result, TablebaseError};
use crate::probe::{PositionProbe, check_applicable};

/// Most folders a set may have.
pub const MAX_DIRECTORIES: usize = 8;
/// Most directory entries read from one folder.
pub const MAX_ENTRIES_PER_DIRECTORY: usize = 20_000;
/// Most file problems kept per folder in the report.
pub const MAX_ISSUES_PER_DIRECTORY: usize = 20;

/// What happened to a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DirStatus {
    /// The folder was read (it may hold no table).
    Read,
    /// The path does not exist or is not a folder.
    NotADirectory,
    /// The folder could not be listed.
    Unreadable(io::ErrorKind),
}

/// Why a file that looks like a table was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FileIssueKind {
    /// The size is not the one of a table: the file is truncated or is not a
    /// table.
    WrongSize,
    /// The file could not be examined.
    Unreadable(io::ErrorKind),
}

/// A table file that was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIssue {
    /// The name of the file (lossy).
    pub name: String,
    /// What is wrong.
    pub kind: FileIssueKind,
}

/// What one folder contributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirReport {
    /// The folder as configured.
    pub path: PathBuf,
    /// Whether it could be read.
    pub status: DirStatus,
    /// `.rtbw` files accepted.
    pub wdl_files: usize,
    /// `.rtbz` files accepted.
    pub dtz_files: usize,
    /// The largest table in the folder, in pieces (0 when none).
    pub max_pieces: u8,
    /// Files named like a table whose extension differs only by case
    /// (`.RTBW`): `shakmaty-syzygy` ignores them, so they must be renamed to
    /// lower case (D2581).
    pub uppercase_files: usize,
    /// Files that were not accepted (at most [`MAX_ISSUES_PER_DIRECTORY`]).
    pub issues: Vec<FileIssue>,
}

/// What [`LocalTablebase::open`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenReport {
    /// One entry per configured folder, in order.
    pub directories: Vec<DirReport>,
    /// Total `.rtbw` files.
    pub wdl_files: usize,
    /// Total `.rtbz` files.
    pub dtz_files: usize,
    /// The largest table, in pieces.
    pub max_pieces: u8,
}

impl OpenReport {
    /// Whether no table was found at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wdl_files == 0 && self.dtz_files == 0
    }
}

/// A set of Syzygy tables read lazily from some folders.
pub struct LocalTablebase {
    tables: Tablebase<Chess>,
    report: OpenReport,
}

impl fmt::Debug for LocalTablebase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalTablebase")
            .field("wdl_files", &self.report.wdl_files)
            .field("dtz_files", &self.report.dtz_files)
            .field("max_pieces", &self.report.max_pieces)
            .finish_non_exhaustive()
    }
}

impl LocalTablebase {
    /// Registers the tables of `dirs`.
    ///
    /// A folder that does not exist, is not a folder or cannot be listed, and
    /// a file with the wrong size, are reported in the [`OpenReport`] and do
    /// not stop the others. Files that are not tables are ignored. A set with
    /// no table at all is returned too (it answers [`TablebaseError::NoTables`]).
    ///
    /// # Errors
    /// [`TablebaseError::TooManyDirectories`].
    pub fn open(dirs: &[PathBuf]) -> Result<(LocalTablebase, OpenReport)> {
        if dirs.len() > MAX_DIRECTORIES {
            return Err(TablebaseError::TooManyDirectories {
                max: MAX_DIRECTORIES,
            });
        }
        let mut tables = Tablebase::new();
        let mut report = OpenReport::default();
        for dir in dirs {
            let dir_report = scan_directory(&mut tables, dir);
            report.wdl_files += dir_report.wdl_files;
            report.dtz_files += dir_report.dtz_files;
            report.max_pieces = report.max_pieces.max(dir_report.max_pieces);
            report.directories.push(dir_report);
        }
        report.max_pieces = u8::try_from(tables.max_pieces()).unwrap_or(u8::MAX);
        Ok((
            LocalTablebase {
                tables,
                report: report.clone(),
            },
            report,
        ))
    }

    /// What `open` found.
    #[must_use]
    pub fn report(&self) -> &OpenReport {
        &self.report
    }

    /// Whether the set has no table.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.report.is_empty()
    }

    /// The result of one position from the point of view of the side to move,
    /// without its moves.
    ///
    /// # Errors
    /// [`TablebaseError`].
    pub fn evaluate(&self, position: &Position) -> Result<Evaluation> {
        let max = self.max_pieces();
        check_applicable(position, max)?;
        convert::evaluate_chess(&self.tables, &convert::to_chess(position)?, max)
    }
}

impl PositionProbe for LocalTablebase {
    fn probe(&self, position: &Position) -> Result<PositionReport> {
        if self.is_empty() {
            return Err(TablebaseError::NoTables);
        }
        convert::probe_position(&self.tables, position, self.max_pieces())
    }

    fn max_pieces(&self) -> u8 {
        self.report.max_pieces
    }
}

/// The number of pieces named by a table file stem (`KRvKP` is 5).
fn pieces_in_stem(stem: &str) -> u8 {
    let count = stem
        .chars()
        .filter(|c| matches!(c, 'K' | 'Q' | 'R' | 'B' | 'N' | 'P'))
        .count();
    u8::try_from(count).unwrap_or(u8::MAX)
}

fn scan_directory(tables: &mut Tablebase<Chess>, dir: &Path) -> DirReport {
    let mut report = DirReport {
        path: dir.to_path_buf(),
        status: DirStatus::Read,
        wdl_files: 0,
        dtz_files: 0,
        max_pieces: 0,
        uppercase_files: 0,
        issues: Vec::new(),
    };
    match fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            report.status = DirStatus::NotADirectory;
            return report;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            report.status = DirStatus::NotADirectory;
            return report;
        }
        Err(error) => {
            report.status = DirStatus::Unreadable(error.kind());
            return report;
        }
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            report.status = DirStatus::Unreadable(error.kind());
            return report;
        }
    };
    // Sorted so that the report (and the table chosen when two folders hold
    // the same material) does not depend on the order of the listing.
    let mut paths: Vec<PathBuf> = entries
        .take(MAX_ENTRIES_PER_DIRECTORY)
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    paths.sort();
    for path in paths {
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let is_wdl = extension == "rtbw";
        if !is_wdl && extension != "rtbz" {
            if extension.eq_ignore_ascii_case("rtbw") || extension.eq_ignore_ascii_case("rtbz") {
                report.uppercase_files += 1;
            }
            continue;
        }
        // `add_file` only reads the metadata, but it is code of the crate that
        // reads a file of the user: guarded like the probes (D2558).
        let added = catch_unwind(AssertUnwindSafe(|| tables.add_file(&path)))
            .unwrap_or_else(|_| Err(io::Error::other("add_file panicked")));
        match added {
            Ok(()) => {
                if is_wdl {
                    report.wdl_files += 1;
                } else {
                    report.dtz_files += 1;
                }
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                report.max_pieces = report.max_pieces.max(pieces_in_stem(stem));
            }
            // The name is not the one of a table.
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
            Err(error) => {
                if report.issues.len() < MAX_ISSUES_PER_DIRECTORY {
                    let kind = if error.kind() == io::ErrorKind::InvalidData {
                        FileIssueKind::WrongSize
                    } else {
                        FileIssueKind::Unreadable(error.kind())
                    };
                    report.issues.push(FileIssue {
                        name: path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        kind,
                    });
                }
            }
        }
    }
    report
}
