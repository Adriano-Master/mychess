// Derived from Scid vs PC 4.27, src/spellchk.h and src/spellchk.cpp (the
// `SpellChecker` class and the reading of the spelling file).
// Copyright notice of both files: Copyright (c) 2001-2003 Shane Hudson.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The spelling file (`.ssp`): names, aliases, player comments, general
//! replacement rules, biographies and Elo by period (milestone M6, slice
//! 3a; `docs/specs/m6-3-jogadores-torneios-spelling.md` and
//! `docs/formatos/spelling-ssp.md`).
//!
//! - [`SpellingSet::read`] / [`SpellingSet::read_bytes`] read the file in one
//!   pass and build one [`SpellChecker`] per kind of name (player, event,
//!   site, round). Reading is atomic: an error builds nothing.
//! - [`SpellChecker`] answers the queries of the Scid: [`correct`],
//!   [`corrections`], [`comment_loose`], [`comment_exact`], [`bio`] and the
//!   prefix, suffix and infix rules. The lookups by prefix are `O(log n + k)`
//!   on a sorted vector; the original walks a linked list.
//! - [`PlayerComment`] decodes the comment of a player (title, countries,
//!   peak rating, birth and death).
//! - [`elo`] interprets the `%Elo` lines (rating by period) and plans
//!   "Add Elo"; [`check`] proposes name corrections for a database;
//!   [`apply`] holds the types of the application of those corrections;
//!   [`photo`] reads the player photos (`.spf` files, without running Tcl).
//!
//! The spelling file of the Scid is not part of this repository.
//!
//! [`correct`]: SpellChecker::correct
//! [`corrections`]: SpellChecker::corrections
//! [`comment_loose`]: SpellChecker::comment_loose
//! [`comment_exact`]: SpellChecker::comment_exact
//! [`bio`]: SpellChecker::bio

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::crosstable::{PlayerBio, PlayerBioProvider};
use crate::search::{Title, TitleProvider};
use crate::si4::NameKind;

pub mod apply;
pub mod check;
mod comment;
pub mod elo;
mod r#match;
mod parse;
pub mod photo;
mod tables;

pub use comment::PlayerComment;
pub use parse::{MAX_BIO_NOTES, MAX_FILE_BYTES, MAX_LINE_BYTES, ReadOptions};
pub use tables::{COUNTRY_TABLE, TITLE_TABLE, country_name, title_name};

/// Why a spelling file could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SpellingError {
    /// The file could not be opened or read.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The system error.
        #[source]
        source: io::Error,
    },
    /// The path is not a regular file.
    #[error("{path} is not a file")]
    NotAFile {
        /// The path.
        path: PathBuf,
    },
    /// The file (after decompression) is larger than the limit.
    #[error("the spelling file is larger than {limit} bytes")]
    TooLarge {
        /// The limit in bytes.
        limit: u64,
    },
    /// The gzip stream is damaged.
    #[error("invalid gzip data: {0}")]
    Gzip(#[source] io::Error),
    /// A section header opens a quote and does not close it (the only syntax
    /// error of the Scid: "Error reading name spellcheck file.").
    #[error("line {line}: unterminated quote in a section header")]
    UnclosedQuote {
        /// The 1-based line number.
        line: u64,
    },
    /// The caller cancelled the reading.
    #[error("the reading was cancelled")]
    Cancelled,
}

/// What was read: the counts of correct names (rule 11), and what was left
/// out (rules 1, 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadReport {
    /// Correct player names.
    pub players: u32,
    /// Correct event names.
    pub events: u32,
    /// Correct site names.
    pub sites: u32,
    /// Correct round names.
    pub rounds: u32,
    /// Lines longer than [`MAX_LINE_BYTES`], ignored.
    pub skipped_lines: u32,
    /// `%Elo` lines kept.
    pub elo_lines: u32,
    /// `%Bio` notes dropped because a name already had [`MAX_BIO_NOTES`].
    pub bio_dropped: u32,
    /// `%Elo` lines dropped because a name already had too many.
    pub elo_dropped: u32,
}

impl LoadReport {
    /// The count of correct names of `kind`.
    pub fn names(&self, kind: NameKind) -> u32 {
        match kind {
            NameKind::Player => self.players,
            NameKind::Event => self.events,
            NameKind::Site => self.sites,
            NameKind::Round => self.rounds,
        }
    }
}

/// The four checkers built from one spelling file, plus the [`LoadReport`].
#[derive(Debug, Clone)]
pub struct SpellingSet {
    player: SpellChecker,
    event: SpellChecker,
    site: SpellChecker,
    round: SpellChecker,
    report: LoadReport,
}

impl SpellingSet {
    /// The checker for names of `kind`.
    pub fn checker(&self, kind: NameKind) -> &SpellChecker {
        match kind {
            NameKind::Player => &self.player,
            NameKind::Event => &self.event,
            NameKind::Site => &self.site,
            NameKind::Round => &self.round,
        }
    }

    /// The counts and the ignored lines of the reading.
    pub fn report(&self) -> &LoadReport {
        &self.report
    }
}

/// One name of the file: a correct name or an alias of one.
#[derive(Debug, Clone)]
struct Entry {
    /// The name without the excluded characters: what queries are compared
    /// to.
    key: Box<str>,
    /// The correct name (for an alias, the name it points to).
    name: Arc<str>,
    /// For an alias, the index of the correct name it points to.
    canonical: Option<u32>,
    /// The comment of the correct name (shared by its aliases); empty = none.
    comment: Arc<str>,
}

/// A `%Prefix`, `%Suffix` or `%Infix` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    from: Box<str>,
    to: Box<str>,
}

/// The rare data of a correct name, apart from the main table.
#[derive(Debug, Clone, Default)]
struct Side {
    /// `%Bio` notes, in file order.
    bio: HashMap<u32, Vec<String>>,
    /// Raw `%Elo` lines (the text after `%Elo`), interpreted on demand.
    elo: HashMap<u32, Vec<Box<str>>>,
    /// `%Render` text; stored and unused, as in the Scid.
    render: HashMap<u32, Box<str>>,
}

/// The spelling data of one kind of name.
///
/// Built by [`SpellingSet::read`]; immutable afterwards, so it can be shared
/// between threads (`Arc<SpellingSet>`).
#[derive(Debug, Clone)]
pub struct SpellChecker {
    kind: NameKind,
    exclude: Box<[char]>,
    /// Names and aliases, in file order.
    entries: Vec<Entry>,
    /// Indices into `entries` sorted by `(key, index)`.
    sorted: Vec<u32>,
    /// Correct name to the index of its last occurrence.
    exact: HashMap<Arc<str>, u32>,
    /// Rules in the order they are tried: the last one defined first.
    prefixes: Vec<Rule>,
    suffixes: Vec<Rule>,
    infixes: Vec<Rule>,
    side: Side,
    num_names: u32,
    num_aliases: u32,
    has_elo: bool,
    last_year: u16,
    fingerprint: u64,
}

/// The next fingerprint; 0 is never handed out (it stands for "no checker").
static NEXT_FINGERPRINT: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_fingerprint() -> u64 {
    NEXT_FINGERPRINT.fetch_add(1, Ordering::Relaxed)
}

impl SpellChecker {
    /// A number that identifies the data of this checker: unique per
    /// reading of a file, equal in clones, never 0. Caches derived from a
    /// checker (the countries of a `PlayerTable`) compare it to know when
    /// they are stale (D3441).
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// The kind of name this checker is for.
    pub fn kind(&self) -> NameKind {
        self.kind
    }

    /// The number of correct names (aliases not counted).
    pub fn num_names(&self) -> u32 {
        self.num_names
    }

    /// The number of aliases (`= name` lines).
    pub fn num_aliases(&self) -> u32 {
        self.num_aliases
    }

    /// Whether the file had at least one `%Elo` line for this kind (the
    /// Scid's `HasEloData`), valid or not.
    pub fn has_elo_data(&self) -> bool {
        self.has_elo
    }

    /// The "current year" the Elo tables stop at (rule 63).
    pub fn last_year(&self) -> u16 {
        self.last_year
    }

    /// The characters removed from names before comparing them.
    pub fn excluded_chars(&self) -> &[char] {
        &self.exclude
    }

    /// The correct names, in file order.
    pub fn correct_names(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|e| e.canonical.is_none())
            .map(|e| &*e.name)
    }

    /// The `%Render` text of a correct name (stored, unused by the Scid).
    pub fn render_name(&self, name: &str) -> Option<&str> {
        let index = *self.exact.get(name)?;
        self.side.render.get(&index).map(AsRef::as_ref)
    }

    /// The same checker with another "current year" (tests, or a file loaded
    /// on a machine with a wrong clock). Years of `%Elo` data above the new
    /// year stay in memory but are not reachable through [`SpellChecker::elo_at`].
    #[must_use]
    pub fn with_last_year(mut self, year: u16) -> SpellChecker {
        self.last_year = year;
        self
    }
}

/// The title of a player for the header search (rule 34): the title of the
/// comment of the correct name `name`, exact (aliases have none).
impl TitleProvider for SpellChecker {
    fn title(&self, name: &str) -> Option<Title> {
        let comment = self.comment_exact(name)?;
        PlayerComment::parse(comment).title()
    }
}

/// Title, country and birth date for the tournament table, from the comment
/// found with the loose matching (`crosstab.cpp`, rule 34).
impl PlayerBioProvider for SpellChecker {
    fn bio(&self, name: &str) -> Option<PlayerBio> {
        let comment = self.comment_loose(name)?;
        let parsed = PlayerComment::parse(comment);
        Some(PlayerBio {
            title: parsed.title_for_table().unwrap_or_default().to_owned(),
            country: parsed.last_country().to_owned(),
            birth: parsed.born(),
        })
    }
}

#[cfg(test)]
mod tests;
