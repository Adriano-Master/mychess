//! Helpers of the M6-1a contract tests written from the spec (files
//! `m6_1a_contract_*.rs`): big databases that span several scan blocks, a
//! progress sink that records and cancels, a thread-pool helper and the
//! stale-index patch. The including file declares `base` (M3-3a support)
//! and `support` (M6-1a support) before this module.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation
)]

use mychess_core::Game;
use mychess_db::si4::Si4Database;
use mychess_db::{CancelToken, ProgressSink};

use crate::base::{self, Fixture};
use crate::support::{db_of, game};

/// `plies` half moves of knights going and coming back (`plies` even leaves
/// White to move in the standard start position).
pub fn shuffle(plies: usize) -> String {
    const CYCLE: [&str; 4] = ["g1f3", "g8f6", "f3g1", "f6g8"];
    (0..plies)
        .map(|i| CYCLE[i % 4])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Games cycling through `templates` until there are `count`.
pub fn crowd(count: usize, templates: &[&str]) -> (Fixture, Si4Database) {
    let games: Vec<Game> = (0..count)
        .map(|i| game(templates[i % templates.len()]))
        .collect();
    db_of(games)
}

/// Records every progress report and, if asked, cancels the token at the
/// first one.
pub struct Recorder {
    pub calls: Vec<(u64, u64)>,
    cancel: Option<CancelToken>,
}

impl Recorder {
    pub fn recording() -> Recorder {
        Recorder {
            calls: Vec::new(),
            cancel: None,
        }
    }

    pub fn cancelling(token: &CancelToken) -> Recorder {
        Recorder {
            calls: Vec::new(),
            cancel: Some(token.clone()),
        }
    }
}

impl ProgressSink for Recorder {
    fn progress(&mut self, done: u64, total: u64) {
        self.calls.push((done, total));
        if let Some(token) = &self.cancel {
            token.cancel();
        }
    }
}

/// Runs `f` in a local pool with `threads` workers.
pub fn with_threads<R: Send>(threads: usize, f: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(f)
}

/// Overwrites `FinalMatSig` (bytes 34 to 36) with the most material and the
/// home-pawn data (38 to 46) with 16 changes, as an old Scid may leave stale
/// values on a game with its own start position: an index trusted as a
/// verdict would discard the game.
pub fn spoil_index(fx: &Fixture, n: u32) {
    base::patch_entry(fx, n, |e| {
        e[34..37].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        e[38] = (e[38] & 0xC0) | 16;
        e[39..47].fill(0);
    });
}

/// Zeroes `FinalMatSig` and the home-pawn list: the opposite lie, "nothing
/// is left", which makes a game look reachable by every position.
pub fn empty_index(fx: &Fixture, n: u32) {
    base::patch_entry(fx, n, |e| {
        e[34..37].copy_from_slice(&[0, 0, 0]);
        e[38] &= 0xC0;
        e[39..47].fill(0);
    });
}
