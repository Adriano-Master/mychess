//! Autocompletion of names seen during the session (spec M2-2, D139;
//! Scid: `sc_name_match`).

use std::collections::BTreeMap;

use mychess_core::Tags;

use crate::files::GameHeader;

/// Most suggestions returned (Scid: `MAX_NAME_MATCHES`).
pub const MAX_NAME_MATCHES: usize = 9;
/// Most distinct names kept per kind; beyond it only counts are updated.
pub const MAX_NAMES_PER_KIND: usize = 100_000;

/// What a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NameKind {
    /// A player (`White` and `Black` share the list).
    Player,
    /// An event.
    Event,
    /// A site.
    Site,
    /// A round.
    Round,
}

/// A suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameMatch {
    /// How many times the name was seen.
    pub count: u32,
    /// The name.
    pub name: String,
}

/// A place that can suggest names. The database of M3 will implement it.
pub trait NameSource {
    /// Up to `max` names of `kind` that start with `prefix`.
    fn matches(&self, kind: NameKind, prefix: &str, max: usize) -> Vec<NameMatch>;
}

/// Names seen in this session, with how often.
///
/// Keys are `(lower-case name, name)`, so a range scan returns names in the
/// Scid's order (case-insensitive, ties by bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameBook {
    players: BTreeMap<(String, String), u32>,
    events: BTreeMap<(String, String), u32>,
    sites: BTreeMap<(String, String), u32>,
    rounds: BTreeMap<(String, String), u32>,
}

impl NameBook {
    fn map(&self, kind: NameKind) -> &BTreeMap<(String, String), u32> {
        match kind {
            NameKind::Player => &self.players,
            NameKind::Event => &self.events,
            NameKind::Site => &self.sites,
            NameKind::Round => &self.rounds,
        }
    }

    fn map_mut(&mut self, kind: NameKind) -> &mut BTreeMap<(String, String), u32> {
        match kind {
            NameKind::Player => &mut self.players,
            NameKind::Event => &mut self.events,
            NameKind::Site => &mut self.sites,
            NameKind::Round => &mut self.rounds,
        }
    }

    /// Counts one use of `name` (`?` and blank names are ignored).
    pub fn observe(&mut self, kind: NameKind, name: &str) {
        let name = name.trim();
        if name.is_empty() || name == "?" {
            return;
        }
        let key = (name.to_lowercase(), name.to_owned());
        let map = self.map_mut(kind);
        if let Some(count) = map.get_mut(&key) {
            *count = count.saturating_add(1);
        } else if map.len() < MAX_NAMES_PER_KIND {
            map.insert(key, 1);
        }
    }

    /// Counts the names of a game's tags.
    pub fn observe_tags(&mut self, tags: &Tags) {
        self.observe(NameKind::Player, &tags.white);
        self.observe(NameKind::Player, &tags.black);
        self.observe(NameKind::Event, &tags.event);
        self.observe(NameKind::Site, &tags.site);
        self.observe(NameKind::Round, &tags.round);
    }

    /// Counts the names of a collection header.
    pub fn observe_header(&mut self, header: &GameHeader) {
        self.observe(NameKind::Player, &header.white);
        self.observe(NameKind::Player, &header.black);
        self.observe(NameKind::Event, &header.event);
        self.observe(NameKind::Site, &header.site);
        self.observe(NameKind::Round, &header.round);
    }

    /// Up to `max` names of `kind` starting with `prefix`, ignoring case,
    /// ordered by name. An empty prefix suggests nothing.
    pub fn matches(&self, kind: NameKind, prefix: &str, max: usize) -> Vec<NameMatch> {
        let prefix = prefix.trim().to_lowercase();
        if prefix.is_empty() {
            return Vec::new();
        }
        self.map(kind)
            .range((prefix.clone(), String::new())..)
            .take_while(|((lower, _), _)| lower.starts_with(&prefix))
            .take(max)
            .map(|((_, name), count)| NameMatch {
                count: *count,
                name: name.clone(),
            })
            .collect()
    }

    /// How many distinct names of `kind` are known.
    pub fn len(&self, kind: NameKind) -> usize {
        self.map(kind).len()
    }

    /// Whether no name of any kind is known.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
            && self.events.is_empty()
            && self.sites.is_empty()
            && self.rounds.is_empty()
    }
}

impl NameSource for NameBook {
    fn matches(&self, kind: NameKind, prefix: &str, max: usize) -> Vec<NameMatch> {
        NameBook::matches(self, kind, prefix, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ignore_case_and_sort_by_name() {
        let mut book = NameBook::default();
        for name in ["Carlsen, M.", "CARUANA, F.", "Anand, V.", "Carlsen, M."] {
            book.observe(NameKind::Player, name);
        }
        let found = book.matches(NameKind::Player, "car", MAX_NAME_MATCHES);
        let names: Vec<_> = found.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Carlsen, M.", "CARUANA, F."]);
        assert_eq!(found[0].count, 2);
        assert!(book.matches(NameKind::Player, "", 9).is_empty());
        assert!(book.matches(NameKind::Event, "car", 9).is_empty());
    }

    #[test]
    fn unknown_names_never_enter_and_results_are_capped() {
        let mut book = NameBook::default();
        book.observe(NameKind::Site, "?");
        book.observe(NameKind::Site, "  ");
        assert!(book.is_empty());
        for n in 0..20 {
            book.observe(NameKind::Round, &format!("{n:02}"));
        }
        assert_eq!(book.matches(NameKind::Round, "0", 9).len(), 9);
    }

    #[test]
    fn tags_and_headers_feed_the_book() {
        let mut book = NameBook::default();
        let mut tags = Tags::default();
        tags.white = "W".to_owned();
        book.observe_tags(&tags);
        book.observe_header(&GameHeader::from_scanned(&[(
            "Black".to_owned(),
            "B".to_owned(),
        )]));
        assert_eq!(book.len(NameKind::Player), 2);
        assert_eq!(book.len(NameKind::Event), 0);
    }
}
