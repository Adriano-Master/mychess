// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_spellcheck`,
// `strPromoteSurname`) and src/misc.cpp (`strIsSurnameOnly`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Copyright notice of src/misc.cpp: Copyright (c) 2001-2003 Shane Hudson.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The name checker: the corrections the spelling file proposes for a
//! database (rules 37 to 47). It never writes; see [`super::apply`] for
//! applying the proposals.

use mychess_core::PartialDate;

use super::{PlayerComment, SpellChecker};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::names_write::name_order;
use crate::si4::{NameKind, bytes_to_string};
use crate::source::GameSource;

/// The most candidates considered for a name (`maxAmbiguous` of the Scid).
const MAX_CANDIDATES: usize = 10;

/// Names between two looks at the progress and the cancel flag (Scid: 1000).
const CHECK_EVERY: usize = 1000;

/// Options of [`scan_names`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanOptions {
    /// The kind of name to check; it must be the kind of the checker.
    pub kind: NameKind,
    /// Also check names that are only a surname (players).
    pub surnames: bool,
    /// Also propose when there are several candidates (marked ambiguous).
    pub ambiguous: bool,
}

impl ScanOptions {
    /// The defaults of the interface: no surnames, no ambiguous proposals.
    pub fn new(kind: NameKind) -> ScanOptions {
        ScanOptions {
            kind,
            surnames: false,
            ambiguous: false,
        }
    }
}

/// One proposed correction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The name in the database.
    pub old: String,
    /// The proposed name.
    pub new: String,
    /// The number of games that use the old name.
    pub games: u32,
    /// There are other candidates for the same old name.
    pub ambiguous: bool,
    /// Birth date of the player (from the spelling file), if known.
    pub born: PartialDate,
    /// Death date of the player, if known.
    pub died: PartialDate,
}

/// The proposals, in the order of the names of the database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    /// The proposals.
    pub proposals: Vec<Proposal>,
    /// The scan was cancelled; the proposals are the ones found so far.
    pub interrupted: bool,
}

/// Whether a name looks like a surname alone (`strIsSurnameOnly`): only
/// ASCII letters and at most one capital. The empty name counts.
///
/// ```
/// use mychess_db::spelling::check::is_surname_only;
///
/// assert!(is_surname_only("Kasparov"));
/// assert!(!is_surname_only("McDonald"));
/// assert!(!is_surname_only("Müller"));
/// ```
pub fn is_surname_only(name: &str) -> bool {
    let mut capitals = 0;
    for c in name.chars() {
        if !c.is_ascii_alphabetic() {
            return false;
        }
        if c.is_ascii_uppercase() {
            capitals += 1;
            if capitals > 1 {
                return false;
            }
        }
    }
    true
}

/// `strPromoteSurname`: the text after the last space goes to the front.
fn promote_surname(text: &str) -> String {
    match text.rfind(' ') {
        None => text.to_owned(),
        Some(at) => format!(
            "{} {}",
            text.get(at + 1..).unwrap_or(""),
            text.get(..at).unwrap_or("")
        ),
    }
}

/// ` (XXX)` at the end of a player name longer than 6 bytes: the name
/// without it.
fn without_country(name: &str) -> Option<&str> {
    let b = name.as_bytes();
    let n = b.len();
    if n > 6
        && b.get(n - 6) == Some(&b' ')
        && b.get(n - 5) == Some(&b'(')
        && b.get(n - 4..n - 1)?.iter().all(u8::is_ascii_uppercase)
        && b.get(n - 1) == Some(&b')')
    {
        name.get(..n - 6)
    } else {
        None
    }
}

/// How many games use each name of `kind`, counted from the index (the
/// frequency stored in the `.sn4` is not trusted, rule 38). `None` when
/// cancelled.
fn count_names<D: GameSource + ?Sized>(
    db: &D,
    kind: NameKind,
    total: u64,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Option<Vec<u32>> {
    let size = usize::try_from(db.names().num_names(kind)).unwrap_or(0);
    let mut counts = vec![0u32; size];
    let mut bump = |id: u32| {
        if let Some(slot) = usize::try_from(id).ok().and_then(|i| counts.get_mut(i)) {
            *slot = slot.saturating_add(1);
        }
    };
    for n in 1..=db.len() {
        if n % 8192 == 0 {
            progress.progress(u64::from(n), total);
            if cancel.is_cancelled() {
                return None;
            }
        }
        let Ok(e) = db.entry(n) else {
            continue;
        };
        match kind {
            NameKind::Player => {
                bump(e.white_id);
                bump(e.black_id);
            }
            NameKind::Event => bump(e.event_id),
            NameKind::Site => bump(e.site_id),
            NameKind::Round => bump(e.round_id),
        }
    }
    Some(counts)
}

/// Looks for corrections of the names of one kind (`sc_name spellcheck`).
///
/// Names are visited in the order of the name base; unused names are
/// skipped. For each name the first of these that proposes something ends
/// it: the `%Prefix`, `%Suffix` and `%Infix` rules, then the search by name
/// (rules 40 to 44). A proposal equal to the old name is never made. If
/// `checker.kind()` differs from `opts.kind` the result is empty.
///
/// Progress counts the games read (to recount the frequencies) and then
/// the names. Cancelling returns what was found with `interrupted` set.
pub fn scan_names<D: GameSource + ?Sized>(
    db: &D,
    checker: &SpellChecker,
    opts: ScanOptions,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> ScanResult {
    let mut result = ScanResult::default();
    let kind = opts.kind;
    if checker.kind() != kind {
        return result;
    }
    let names = db.names();
    let total_names = names.num_names(kind);
    let total = u64::from(db.len()) + u64::from(total_names);
    let Some(counts) = count_names(db, kind, total, cancel, progress) else {
        result.interrupted = true;
        return result;
    };
    let mut order: Vec<u32> = (0..total_names).collect();
    order.sort_by(|&a, &b| {
        let na = names.get(kind, a).unwrap_or(&[]);
        let nb = names.get(kind, b).unwrap_or(&[]);
        name_order(na, nb).then(a.cmp(&b))
    });
    for (position, &id) in order.iter().enumerate() {
        if position % CHECK_EVERY == 0 {
            progress.progress(u64::from(db.len()) + position as u64, total);
            if cancel.is_cancelled() {
                result.interrupted = true;
                return result;
            }
        }
        let games = usize::try_from(id)
            .ok()
            .and_then(|i| counts.get(i))
            .copied()
            .unwrap_or(0);
        if games == 0 {
            continue;
        }
        let Some(raw) = names.get(kind, id) else {
            continue;
        };
        let name = bytes_to_string(raw);
        if kind == NameKind::Player && !opts.surnames && is_surname_only(&name) {
            continue;
        }
        propose(checker, opts, &name, games, &mut result.proposals);
    }
    progress.progress(total, total);
    result
}

/// The proposals for one name.
fn propose(
    checker: &SpellChecker,
    opts: ScanOptions,
    name: &str,
    games: u32,
    out: &mut Vec<Proposal>,
) {
    let make = |new: String, ambiguous: bool| {
        let (born, died) = if opts.kind == NameKind::Player {
            checker
                .comment_exact(&new)
                .map(PlayerComment::parse)
                .map_or((PartialDate::UNKNOWN, PartialDate::UNKNOWN), |c| {
                    (c.born(), c.died())
                })
        } else {
            (PartialDate::UNKNOWN, PartialDate::UNKNOWN)
        };
        Proposal {
            old: name.to_owned(),
            new,
            games,
            ambiguous,
            born,
            died,
        }
    };
    for fixed in [
        checker.prefix_fix(name),
        checker.suffix_fix(name),
        checker.infix_fix(name),
    ]
    .into_iter()
    .flatten()
    {
        if fixed != name {
            out.push(make(fixed, false));
            return;
        }
    }
    let search = if opts.kind == NameKind::Player {
        without_country(name).unwrap_or(name)
    } else {
        name
    };
    let mut candidates: Vec<String> = checker
        .corrections(search, MAX_CANDIDATES)
        .into_iter()
        .map(str::to_owned)
        .collect();
    if opts.kind == NameKind::Player && candidates.len() < MAX_CANDIDATES {
        let promoted = promote_surname(search);
        for extra in checker.corrections(&promoted, MAX_CANDIDATES - candidates.len()) {
            if !candidates.iter().any(|c| c == extra) {
                candidates.push(extra.to_owned());
            }
        }
    }
    let ambiguous = candidates.len() > 1;
    if ambiguous && !opts.ambiguous {
        return;
    }
    for candidate in candidates {
        if candidate != name {
            out.push(make(candidate, ambiguous));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surname_only_rules() {
        assert!(is_surname_only("Kasparov"));
        assert!(is_surname_only("kasparov"));
        assert!(is_surname_only(""));
        assert!(!is_surname_only("McDonald"));
        assert!(!is_surname_only("Van der Wiel"));
        assert!(!is_surname_only("Müller"));
        assert!(!is_surname_only("Smith, J"));
    }

    #[test]
    fn promote_and_country_helpers() {
        assert_eq!(promote_surname("R. J. Fischer"), "Fischer R. J.");
        assert_eq!(promote_surname("Fischer, R"), "R Fischer,");
        assert_eq!(promote_surname("Fischer"), "Fischer");
        assert_eq!(
            without_country("Almeida, Bruno (BRA)"),
            Some("Almeida, Bruno")
        );
        assert_eq!(without_country("Almeida, Bruno (Bra)"), None);
        assert_eq!(without_country(" (BRA)"), None);
        assert_eq!(without_country("é (BRA)é"), None);
    }
}
