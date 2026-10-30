// Derived from Scid vs PC 4.27, src/spellchk.cpp (`Correct`, `Corrections`,
// `GetComment`, `GetCommentExact`, `GetBioData`, `CorrectPrefix`,
// `CorrectSuffix`, `CorrectInfix`). Copyright (c) 2000-2003 Shane Hudson;
// Scid vs PC contributors. Scid is distributed under the GNU General Public
// License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Matching of names (rules 25 to 36).
//!
//! The Scid keeps, for each first byte, a linked list with the last name
//! read at the head, and tests every node. Here the entries are sorted by
//! key, so the ones whose key starts with the query are a contiguous range
//! found by binary search: `O(log n + k)`. The order of the Scid is
//! recovered by looking at the entries by decreasing file index.

use std::cmp::Reverse;

use super::parse::strip;
use super::{Entry, SpellChecker};

/// The result of the scan of the Scid for one query.
struct Hit {
    /// Index into `entries`.
    index: usize,
    /// The key equals the query key.
    exact: bool,
}

impl SpellChecker {
    fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    /// The query key and the range of `sorted` whose keys start with it.
    fn range(&self, query: &str) -> Option<(String, std::ops::Range<usize>)> {
        let key = strip(query, &self.exclude);
        if key.is_empty() {
            return None;
        }
        let key_of = |i: u32| self.entries.get(i as usize).map_or("", |e| e.key.as_ref());
        let start = self.sorted.partition_point(|&i| key_of(i) < key.as_str());
        let len = self
            .sorted
            .get(start..)
            .unwrap_or(&[])
            .iter()
            .take_while(|&&i| key_of(i).starts_with(key.as_str()))
            .count();
        Some((key, start..start + len))
    }

    /// The entry the Scid's scan ends with: the exact match of the highest
    /// file index, or else the prefix match of the lowest one.
    fn resolve(&self, query: &str) -> Option<Hit> {
        let (key, range) = self.range(query)?;
        let slice = self.sorted.get(range)?;
        let mut lowest: Option<u32> = None;
        let mut exact: Option<u32> = None;
        for &i in slice {
            lowest = Some(lowest.map_or(i, |l| l.min(i)));
            if self
                .entry(i as usize)
                .is_some_and(|e| e.key.as_ref() == key)
            {
                // Sorted by (key, index): the last exact one is the highest.
                exact = Some(i);
            }
        }
        match (exact, lowest) {
            (Some(i), _) => Some(Hit {
                index: i as usize,
                exact: true,
            }),
            (None, Some(i)) => Some(Hit {
                index: i as usize,
                exact: false,
            }),
            _ => None,
        }
    }

    /// The indices that match, in the order of the scan of the Scid (file
    /// index decreasing), and the query key.
    fn candidates(&self, query: &str) -> (String, Vec<u32>) {
        let Some((key, range)) = self.range(query) else {
            return (String::new(), Vec::new());
        };
        let mut list: Vec<u32> = self.sorted.get(range).unwrap_or(&[]).to_vec();
        list.sort_unstable_by_key(|&i| Reverse(i));
        (key, list)
    }

    /// The correct name for `name` (rule 28): the exact match of the highest
    /// file index if there is one; otherwise the prefix match of the lowest.
    /// Case counts; the excluded characters are ignored. `None` for an empty
    /// key or no match.
    ///
    /// ```
    /// use mychess_db::spelling::{ReadOptions, SpellingSet};
    /// use mychess_db::si4::NameKind;
    ///
    /// let set = SpellingSet::read_bytes(
    ///     b"@PLAYER \"., -\"\nBraga, Carlos Eduardo\n  = Braga, Carlos\n",
    ///     &ReadOptions::default(),
    /// )?;
    /// let players = set.checker(NameKind::Player);
    /// assert_eq!(players.correct("Braga, Carlos"), Some("Braga, Carlos Eduardo"));
    /// assert_eq!(players.correct("Zzz"), None);
    /// # Ok::<(), mychess_db::spelling::SpellingError>(())
    /// ```
    pub fn correct(&self, name: &str) -> Option<&str> {
        let hit = self.resolve(name)?;
        self.entry(hit.index).map(|e| &*e.name)
    }

    /// Up to `max` corrections for `name` (rule 29), without repeats; an
    /// exact match returns only its own correct name.
    pub fn corrections(&self, name: &str, max: usize) -> Vec<&str> {
        let (key, list) = self.candidates(name);
        let mut out: Vec<&str> = Vec::new();
        for i in list {
            let Some(entry) = self.entry(i as usize) else {
                continue;
            };
            let correction: &str = &entry.name;
            if out.len() < max && !out.contains(&correction) {
                out.push(correction);
            }
            if entry.key.as_ref() == key {
                return if max == 0 {
                    Vec::new()
                } else {
                    vec![correction]
                };
            }
        }
        out
    }

    /// Whether the loose matching of `name` ends on an exact match (so the
    /// data is not approximate, D3319). `false` when nothing matches.
    pub fn match_is_exact(&self, name: &str) -> bool {
        self.resolve(name).is_some_and(|h| h.exact)
    }

    /// The comment found with the loose matching (rule 30): the one of the
    /// exact match, or of the lowest-index prefix match. `Some("")` when the
    /// name matches but has no comment.
    pub fn comment_loose(&self, name: &str) -> Option<&str> {
        let hit = self.resolve(name)?;
        self.entry(hit.index).map(|e| &*e.comment)
    }

    /// The comment of the correct name `name` (whole name, with the excluded
    /// characters; an alias has none). With repeated names the one of the
    /// highest file index.
    pub fn comment_exact(&self, name: &str) -> Option<&str> {
        let index = *self.exact.get(name)?;
        self.entry(index as usize).map(|e| &*e.comment)
    }

    /// The biography notes (rule 32).
    pub fn bio(&self, name: &str) -> &[String] {
        let Some(hit) = self.resolve(name) else {
            return &[];
        };
        let Some(entry) = self.entry(hit.index) else {
            return &[];
        };
        let own = |index: usize| {
            u32::try_from(index)
                .ok()
                .and_then(|i| self.side.bio.get(&i))
                .map_or(&[][..], Vec::as_slice)
        };
        match entry.canonical {
            None => own(hit.index),
            Some(principal) if hit.exact => own(principal as usize),
            Some(_) => &[],
        }
    }

    /// The `%Prefix` rule for `name` (rule 35): the first rule, last defined
    /// first, whose text starts the name.
    pub fn prefix_fix(&self, name: &str) -> Option<String> {
        self.prefixes.iter().find_map(|r| {
            name.strip_prefix(&*r.from)
                .map(|rest| format!("{}{rest}", r.to))
        })
    }

    /// The `%Suffix` rule for `name`.
    pub fn suffix_fix(&self, name: &str) -> Option<String> {
        self.suffixes.iter().find_map(|r| {
            name.strip_suffix(&*r.from)
                .map(|head| format!("{head}{}", r.to))
        })
    }

    /// The `%Infix` rule for `name`: the first rule whose text occurs in the
    /// name; its first occurrence is replaced.
    pub fn infix_fix(&self, name: &str) -> Option<String> {
        self.infixes.iter().find_map(|r| {
            let at = name.find(&*r.from)?;
            let head = name.get(..at)?;
            let tail = name.get(at + r.from.len()..)?;
            Some(format!("{head}{}{tail}", r.to))
        })
    }
}
