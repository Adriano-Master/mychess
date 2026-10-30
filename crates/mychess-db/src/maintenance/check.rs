// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_base_check` 1287-1351)
// and tcl/file/maint.tcl (`checkAllGames` 764-824). Copyright (c) 1999-2004
// Shane Hudson; Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! "Check games" (decision D3515, rule 42 of `docs/specs/m7-1-manutencao.md`):
//! every game is fetched, read and decoded, and each problem is reported with
//! its cause. Nothing is written.
//!
//! The records are read in order in blocks and decoded in parallel with
//! `rayon`; the report does not depend on the number of threads.

use rayon::prelude::*;

use super::{MaintError, Scope};
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::build_game;
use crate::si4::{IndexEntry, NameBase, RawGame, Si4Error};
use crate::source::GameSource;

/// The most issues kept in a [`CheckReport`]; the rest only count.
pub const MAX_CHECK_ISSUES: usize = 1000;

/// Most games in a block of the parallel decoding.
const BLOCK_GAMES: usize = 1024;
/// Most record bytes in a block.
const BLOCK_BYTES: usize = 8 << 20;

/// What is wrong with a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckCause {
    /// The index entry cannot be fetched (unreadable or length 0).
    Index,
    /// The record cannot be read from the game file (truncated, out of range).
    Record,
    /// The record cannot be decoded.
    Decode,
    /// A name ID of the entry is not in the name base.
    Names,
    /// A move of the record is not legal in its position.
    IllegalMove,
}

/// One problem found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckIssue {
    /// The game, numbered from 1.
    pub game: u32,
    /// The cause.
    pub cause: CheckCause,
    /// A diagnostic in English, for logs (the interface translates the cause).
    pub detail: String,
}

/// The result of [`check_games`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckReport {
    /// Games examined (in scope and reached before a cancellation).
    pub examined: u32,
    /// Games with a problem (every game has at most one issue).
    pub problems: u32,
    /// The first [`MAX_CHECK_ISSUES`] issues, by game number.
    pub issues: Vec<CheckIssue>,
    /// The run was cancelled; the report covers the games reached.
    pub interrupted: bool,
}

impl CheckReport {
    fn push(&mut self, game: u32, cause: CheckCause, detail: String) {
        self.problems = self.problems.saturating_add(1);
        if self.issues.len() < MAX_CHECK_ISSUES {
            self.issues.push(CheckIssue {
                game,
                cause,
                detail,
            });
        }
    }
}

/// A record waiting for the parallel decoding of its block.
struct Job {
    number: u32,
    entry: IndexEntry,
    start: usize,
    end: usize,
}

fn cause_of(error: &Si4Error) -> CheckCause {
    match error {
        Si4Error::IllegalMove { .. } | Si4Error::Game { .. } => CheckCause::IllegalMove,
        _ => CheckCause::Decode,
    }
}

fn decode(job: &Job, record: &[u8], names: &NameBase) -> Option<(CheckCause, String)> {
    let raw = match RawGame::parse(record) {
        Ok(raw) => raw,
        Err(error) => return Some((CheckCause::Decode, error.to_string())),
    };
    match build_game(job.number, &job.entry, names, &raw) {
        Ok(_) => None,
        Err(error) => Some((cause_of(&error), error.to_string())),
    }
}

/// Checks the games in `scope`: index entry, record, name IDs and the decoding
/// of the whole tree with the legality of every move. Reports a problem per
/// game (the first cause found) and never stops at one.
///
/// Progress is reported every 5000 games or 100 ms, where `cancel` is also
/// looked at; a cancelled run returns what it had with `interrupted` set.
/// A source that is not an SI4 database is checked game by game through
/// [`GameSource::game`].
///
/// # Errors
///
/// [`MaintError::FilterSize`] if the scope filter does not cover the database.
pub fn check_games<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<CheckReport, MaintError> {
    let games = db.len();
    scope.validate(games)?;
    let total = u64::from(scope.count(games));
    let mut report = CheckReport::default();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    let mut buffer: Vec<u8> = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut next = 1u32;
    let mut done = 0u64;
    let mut stopped = false;

    while next <= games && !stopped {
        buffer.clear();
        jobs.clear();
        // Problems of this block that need no decoding, kept apart so the
        // final list stays ordered by game number.
        let mut early: Vec<(u32, CheckCause, String)> = Vec::new();
        while next <= games && jobs.len() < BLOCK_GAMES && buffer.len() < BLOCK_BYTES {
            let n = next;
            next += 1;
            if !scope.contains(n) {
                continue;
            }
            if ticker.tick() {
                progress.progress(done, total);
                if cancel.is_cancelled() {
                    // The games already queued are still checked.
                    stopped = true;
                    break;
                }
            }
            report.examined += 1;
            done += 1;
            let entry = match db.entry(n) {
                Ok(entry) => entry,
                Err(error) => {
                    early.push((n, CheckCause::Index, error.to_string()));
                    continue;
                }
            };
            if entry.length == 0 {
                early.push((n, CheckCause::Index, "empty index entry".to_owned()));
                continue;
            }
            if let Err(error) = entry.verify_names(db.names()) {
                early.push((n, CheckCause::Names, error.to_string()));
                continue;
            }
            if !db.is_scid_database() {
                if let Err(error) = db.game(n) {
                    early.push((n, cause_of(&error), error.to_string()));
                }
                continue;
            }
            match db.record_bytes(n) {
                Ok(record) => {
                    let start = buffer.len();
                    buffer.extend_from_slice(record);
                    jobs.push(Job {
                        number: n,
                        entry,
                        start,
                        end: buffer.len(),
                    });
                }
                Err(error) => early.push((n, CheckCause::Record, error.to_string())),
            }
        }
        let names = db.names();
        let decoded: Vec<Option<(CheckCause, String)>> = jobs
            .par_iter()
            .map(|job| decode(job, buffer.get(job.start..job.end).unwrap_or(&[]), names))
            .collect();
        for (job, found) in jobs.iter().zip(decoded) {
            if let Some((cause, detail)) = found {
                early.push((job.number, cause, detail));
            }
        }
        early.sort_by_key(|(n, _, _)| *n);
        for (n, cause, detail) in early {
            report.push(n, cause, detail);
        }
    }
    report.interrupted = stopped;
    if !stopped {
        progress.progress(total, total);
    }
    Ok(report)
}
