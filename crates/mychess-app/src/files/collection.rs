//! An in-memory, read-only PGN collection (spec M2-2, D124-D125). M3
//! replaces it with the PGN-backed database.

use std::path::{Path, PathBuf};

use mychess_core::{Eco, GameResult, PartialDate, RatingType};
use mychess_pgn::read::{ParsedGame, RawGame, ReadOptions};

use crate::import::ScanLimit;

/// What the game chooser shows for one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameHeader {
    /// `White`, `?` when unknown.
    pub white: String,
    /// `Black`, `?` when unknown.
    pub black: String,
    /// `Event`, `?` when unknown.
    pub event: String,
    /// `Site`, `?` when unknown.
    pub site: String,
    /// `Round`, `?` when unknown.
    pub round: String,
    /// `Date`.
    pub date: PartialDate,
    /// `Result`.
    pub result: GameResult,
    /// `WhiteElo` (or another rating tag of White).
    pub white_elo: Option<u16>,
    /// `BlackElo` (or another rating tag of Black).
    pub black_elo: Option<u16>,
    /// `ECO`.
    pub eco: Option<Eco>,
}

impl GameHeader {
    /// Builds a header from tags exactly as written. Lenient: it never
    /// fails, a missing or unreadable tag is `?`, unknown or `None`. The
    /// first occurrence of a tag wins.
    pub fn from_scanned(tags: &[(String, String)]) -> GameHeader {
        let find = |name: &str| tags.iter().find(|(n, _)| n == name).map(|(_, v)| v.trim());
        let text = |name: &str| {
            find(name)
                .filter(|v| !v.is_empty())
                .unwrap_or("?")
                .to_owned()
        };
        let elo = |side: &str| {
            RatingType::ALL
                .iter()
                .find_map(|kind| find(&format!("{side}{}", kind.name())))
                .and_then(|v| v.parse::<u16>().ok())
                .filter(|v| (1..=mychess_core::MAX_ELO).contains(v))
        };
        GameHeader {
            white: text("White"),
            black: text("Black"),
            event: text("Event"),
            site: text("Site"),
            round: text("Round"),
            date: find("Date").map_or(PartialDate::UNKNOWN, PartialDate::parse_lenient),
            result: find("Result").map_or(GameResult::Unknown, GameResult::from_str_lenient),
            white_elo: elo("White"),
            black_elo: elo("Black"),
            eco: find("ECO").and_then(Eco::parse_prefix).map(|(eco, _)| eco),
        }
    }

    /// The lower-case text the chooser's filter searches (D125).
    fn search_key(&self) -> String {
        let year = self.date.year().map(|y| y.to_string()).unwrap_or_default();
        let fields = [
            self.white.as_str(),
            self.black.as_str(),
            self.event.as_str(),
            self.site.as_str(),
            self.round.as_str(),
            year.as_str(),
            self.result.as_str(),
        ];
        fold_case(&fields.join("\u{1f}"))
    }
}

/// Lower-cases `text` and unifies the Greek final sigma with the plain one, so
/// that "ΟΔΥΣΣΕΑΣ" and "οδυσσεασ" match.
fn fold_case(text: &str) -> String {
    text.to_lowercase().replace('\u{3c2}', "\u{3c3}")
}

/// Games read from a file or from pasted text, kept as raw bytes and parsed
/// one at a time. Read-only.
#[derive(Debug)]
pub struct PgnCollection {
    raws: Vec<RawGame>,
    headers: Vec<GameHeader>,
    keys: Vec<String>,
    path: Option<PathBuf>,
    encoding: &'static str,
    skipped_bytes: u64,
    truncated: Option<(u64, ScanLimit)>,
}

impl PgnCollection {
    pub(crate) fn new(
        raws: Vec<RawGame>,
        path: Option<PathBuf>,
        encoding: &'static str,
        skipped_bytes: u64,
        truncated: Option<(u64, ScanLimit)>,
    ) -> PgnCollection {
        let headers: Vec<GameHeader> = raws
            .iter()
            .map(|raw| GameHeader::from_scanned(&raw.scan_tags()))
            .collect();
        let keys = headers.iter().map(GameHeader::search_key).collect();
        PgnCollection {
            raws,
            headers,
            keys,
            path,
            encoding,
            skipped_bytes,
            truncated,
        }
    }

    /// Number of games.
    pub fn len(&self) -> usize {
        self.raws.len()
    }

    /// Whether there is no game.
    pub fn is_empty(&self) -> bool {
        self.raws.is_empty()
    }

    /// The header of game `index` (0-based).
    pub fn header(&self, index: usize) -> Option<&GameHeader> {
        self.headers.get(index)
    }

    /// All headers, in file order.
    pub fn headers(&self) -> &[GameHeader] {
        &self.headers
    }

    /// The file (canonical path), `None` for pasted text.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Name of the encoding used for text that is not UTF-8.
    pub fn encoding(&self) -> &'static str {
        self.encoding
    }

    /// Bytes of text outside any game.
    pub fn skipped_bytes(&self) -> u64 {
        self.skipped_bytes
    }

    /// `Some((games kept, limit))` when reading stopped at a limit.
    pub fn truncated(&self) -> Option<(u64, ScanLimit)> {
        self.truncated
    }

    /// Indices of the games whose white, black, event, site, round, year or
    /// result contain `query` (Unicode case-insensitive). An empty query
    /// matches every game.
    pub fn filter(&self, query: &str) -> Vec<usize> {
        let query = fold_case(query.trim());
        self.keys
            .iter()
            .enumerate()
            .filter(|(_, key)| key.contains(&query))
            .map(|(index, _)| index)
            .collect()
    }

    /// Parses game `index` (0-based).
    pub fn parse_game(&self, index: usize, opts: &ReadOptions) -> Option<ParsedGame> {
        self.raws.get(index).map(|raw| raw.parse(opts))
    }

    /// The first bytes of game `index` (for tests and diagnostics).
    pub fn raw_bytes(&self, index: usize) -> Option<&[u8]> {
        self.raws.get(index).map(RawGame::bytes)
    }
}

impl PartialEq for PgnCollection {
    fn eq(&self, other: &PgnCollection) -> bool {
        self.path == other.path
            && self.encoding == other.encoding
            && self.skipped_bytes == other.skipped_bytes
            && self.truncated == other.truncated
            && self.headers == other.headers
            && self.raws.len() == other.raws.len()
            && self
                .raws
                .iter()
                .zip(&other.raws)
                .all(|(a, b)| a.bytes() == b.bytes())
    }
}

impl Eq for PgnCollection {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn header_reading_is_lenient() {
        let h = GameHeader::from_scanned(&tags(&[
            ("White", " Carlsen, M. "),
            ("Result", "1/2"),
            ("Date", "2024.??.??"),
            ("WhiteElo", "abc"),
            ("BlackUSCF", "1800"),
            ("ECO", "zz"),
        ]));
        assert_eq!(h.white, "Carlsen, M.");
        assert_eq!(h.black, "?");
        assert_eq!(h.result, GameResult::Draw);
        assert_eq!(h.date.year(), Some(2024));
        assert_eq!(h.white_elo, None);
        assert_eq!(h.black_elo, Some(1800));
        assert_eq!(h.eco, None);
        assert_eq!(GameHeader::from_scanned(&[]).event, "?");
    }

    #[test]
    fn first_tag_wins_and_eco_extends() {
        let h = GameHeader::from_scanned(&tags(&[
            ("White", "A"),
            ("White", "B"),
            ("ECO", "B91a"),
            ("BlackElo", "99999"),
        ]));
        assert_eq!(h.white, "A");
        assert_eq!(h.eco.map(|e| e.to_string()), Some("B91a".to_owned()));
        assert_eq!(h.black_elo, None);
    }

    #[test]
    fn search_key_is_case_insensitive() {
        let h =
            GameHeader::from_scanned(&tags(&[("White", "\u{c9}douard"), ("Date", "1999.01.01")]));
        let key = h.search_key();
        assert!(key.contains("\u{e9}douard"));
        assert!(key.contains("1999"));
    }
}
