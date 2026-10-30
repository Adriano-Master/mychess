// Derived from Scid vs PC 4.27, src/spellchk.cpp (`ReadSpellCheckFile`,
// `getNameAndComment`, `AddPrefixSuffix`, `AddBioData`) and src/namebase.cpp
// (`NameTypeFromString`). Copyright (c) 2000-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Reading of the `.ssp` file: bytes to text, lines, and the grammar of
//! `docs/formatos/spelling-ssp.md` (rules 1 to 15), in one pass.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use flate2::read::MultiGzDecoder;

use super::{Entry, LoadReport, Rule, Side, SpellChecker, SpellingError, SpellingSet};
use crate::progress::{CancelToken, NoProgress, ProgressSink};
use crate::si4::NameKind;

/// The longest line read, in bytes; a longer one is ignored and counted in
/// [`LoadReport::skipped_lines`] (the Scid cuts it into pieces).
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// The largest file read, after decompression (256 MiB, D3440): the real
/// `spelling.ssp` has a few MiB, the limit only stops absurd allocations.
pub const MAX_FILE_BYTES: u64 = 256 << 20;

/// The most `%Bio` notes kept for a name; the rest are dropped and counted.
pub const MAX_BIO_NOTES: usize = 1000;

/// The most `%Elo` lines kept for a name.
const MAX_ELO_LINES: usize = 64;

/// Bytes of non-ASCII text needed before the encoding guess is trusted (the
/// guess of `chardetng` on a few bytes is not reliable: it prefers
/// Windows-1250 for a lone `0xA3`).
const GUESS_EVIDENCE: usize = 32;

/// How much of the file the encoding detector looks at.
const GUESS_BUDGET: usize = 4 << 20;

/// Lines between two progress reports and cancellation checks.
const CHECK_EVERY: u32 = 4096;

/// Options of the reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOptions {
    /// The "current year" of the Elo tables (rule 63); `None` reads the
    /// clock.
    pub current_year: Option<u16>,
    /// The largest file accepted, after decompression.
    pub max_bytes: u64,
}

impl Default for ReadOptions {
    fn default() -> ReadOptions {
        ReadOptions {
            current_year: None,
            max_bytes: MAX_FILE_BYTES,
        }
    }
}

impl SpellingSet {
    /// Reads a `.ssp` or `.ssp.gz` file (a gzip stream is recognised by its
    /// first bytes). Progress counts bytes of the text.
    ///
    /// # Errors
    ///
    /// [`SpellingError`]: unreadable file, damaged gzip, too large, an
    /// unterminated quote in a section header, or cancelled. Nothing is
    /// built in that case.
    pub fn read(
        path: &Path,
        opts: &ReadOptions,
        cancel: &CancelToken,
        progress: &mut dyn ProgressSink,
    ) -> Result<SpellingSet, SpellingError> {
        let io_err = |source| SpellingError::Io {
            path: path.to_path_buf(),
            source,
        };
        let meta = std::fs::metadata(path).map_err(io_err)?;
        if !meta.is_file() {
            return Err(SpellingError::NotAFile {
                path: path.to_path_buf(),
            });
        }
        let mut raw = Vec::new();
        File::open(path)
            .map_err(io_err)?
            .take(opts.max_bytes.saturating_add(1))
            .read_to_end(&mut raw)
            .map_err(io_err)?;
        build_from_bytes(&raw, opts, cancel, progress)
    }

    /// Reads the file from memory (also accepts a gzip stream).
    ///
    /// # Errors
    ///
    /// As [`SpellingSet::read`], except for the file errors.
    pub fn read_bytes(bytes: &[u8], opts: &ReadOptions) -> Result<SpellingSet, SpellingError> {
        build_from_bytes(bytes, opts, &CancelToken::new(), &mut NoProgress)
    }
}

fn build_from_bytes(
    raw: &[u8],
    opts: &ReadOptions,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<SpellingSet, SpellingError> {
    let limit = opts.max_bytes;
    let data: std::borrow::Cow<'_, [u8]> = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        MultiGzDecoder::new(raw)
            .take(limit.saturating_add(1))
            .read_to_end(&mut out)
            .map_err(SpellingError::Gzip)?;
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(raw)
    };
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > limit {
        return Err(SpellingError::TooLarge { limit });
    }
    let text = decode(&data);
    let year = opts.current_year.unwrap_or_else(current_year);
    parse_text(&text, year, cancel, progress)
}

/// Bytes to text (rule 1): BOM removed; UTF-8 if valid; otherwise the
/// encoding guessed by `chardetng`, Windows-1252 when there is little
/// evidence.
fn decode(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return std::borrow::Cow::Borrowed(text);
    }
    let sample = bytes.get(..GUESS_BUDGET).unwrap_or(bytes);
    let evidence = sample.iter().filter(|b| **b >= 0x80).count();
    let mut encoding = encoding_rs::WINDOWS_1252;
    if evidence >= GUESS_EVIDENCE {
        let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
        detector.feed(sample, bytes.len() <= GUESS_BUDGET);
        let guess = detector.guess(None, Utf8Detection::Allow);
        if guess != encoding_rs::UTF_8 {
            encoding = guess;
        }
    }
    let (text, _) = encoding.decode_without_bom_handling(bytes);
    text
}

/// The calendar year of the clock (UTC).
fn current_year() -> u16 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    u16::try_from(year).unwrap_or(2026)
}

/// `NameTypeFromString` (rule 4), a little more tolerant than the original
/// (which does not accept `@pl "x"`): the leading letters of the text, after
/// spaces, must be a prefix of `player`, `event`, `site` or `round`, or start
/// with one of them.
pub(super) fn name_type_from_str(text: &str) -> Option<NameKind> {
    let word: String = text
        .trim_start_matches(' ')
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if word.is_empty() {
        return None;
    }
    [
        ("player", NameKind::Player),
        ("event", NameKind::Event),
        ("site", NameKind::Site),
        ("round", NameKind::Round),
    ]
    .into_iter()
    .find(|(full, _)| full.starts_with(&word) || word.starts_with(full))
    .map(|(_, kind)| kind)
}

/// Removes the excluded characters (`strCopyExclude`).
pub(super) fn strip(text: &str, exclude: &[char]) -> String {
    if exclude.is_empty() {
        return text.to_owned();
    }
    text.chars().filter(|c| !exclude.contains(c)).collect()
}

/// What a kind accumulates while the file is read.
struct Builder {
    kind: NameKind,
    exclude: Vec<char>,
    entries: Vec<Entry>,
    exact: HashMap<Arc<str>, u32>,
    last_correct: Option<u32>,
    prefixes: Vec<Rule>,
    suffixes: Vec<Rule>,
    infixes: Vec<Rule>,
    side: Side,
    num_names: u32,
    num_aliases: u32,
    has_elo: bool,
}

impl Builder {
    fn new(kind: NameKind) -> Builder {
        Builder {
            kind,
            exclude: Vec::new(),
            entries: Vec::new(),
            exact: HashMap::new(),
            last_correct: None,
            prefixes: Vec::new(),
            suffixes: Vec::new(),
            infixes: Vec::new(),
            side: Side::default(),
            num_names: 0,
            num_aliases: 0,
            has_elo: false,
        }
    }

    fn next_index(&self) -> Option<u32> {
        u32::try_from(self.entries.len()).ok()
    }

    fn add_correct(&mut self, name: &str, comment: &str) {
        let Some(index) = self.next_index() else {
            return;
        };
        let name: Arc<str> = Arc::from(name);
        self.entries.push(Entry {
            key: strip(&name, &self.exclude).into_boxed_str(),
            name: Arc::clone(&name),
            canonical: None,
            comment: Arc::from(comment),
        });
        self.exact.insert(name, index);
        self.last_correct = Some(index);
        self.num_names = self.num_names.saturating_add(1);
    }

    fn add_alias(&mut self, text: &str) {
        let (Some(index), Some(target)) = (self.next_index(), self.last_correct) else {
            return;
        };
        let Some(principal) = usize::try_from(target)
            .ok()
            .and_then(|i| self.entries.get(i))
        else {
            return;
        };
        let entry = Entry {
            key: strip(text, &self.exclude).into_boxed_str(),
            name: Arc::clone(&principal.name),
            canonical: Some(target),
            comment: Arc::clone(&principal.comment),
        };
        debug_assert_eq!(index as usize, self.entries.len());
        self.entries.push(entry);
        self.num_aliases = self.num_aliases.saturating_add(1);
    }

    /// `%Elo`, `%Bio` and `%Render` lines: they belong to the last correct
    /// name of this kind, whatever section the line is in (as in the Scid,
    /// which reads the file once per kind).
    fn add_directive(&mut self, part: &str, report: &mut LoadReport) {
        let Some(last) = self.last_correct else {
            return;
        };
        if let Some(rest) = part.strip_prefix("%Elo ") {
            self.has_elo = true;
            let lines = self.side.elo.entry(last).or_default();
            if lines.len() < MAX_ELO_LINES {
                lines.push(rest.into());
                report.elo_lines = report.elo_lines.saturating_add(1);
            } else {
                report.elo_dropped = report.elo_dropped.saturating_add(1);
            }
        } else if let Some(rest) = part.strip_prefix("%Bio ") {
            let notes = self.side.bio.entry(last).or_default();
            if notes.len() < MAX_BIO_NOTES {
                notes.push(rest.to_owned());
            } else {
                report.bio_dropped = report.bio_dropped.saturating_add(1);
            }
        } else if let Some(rest) = part.strip_prefix("%Render ") {
            self.side
                .render
                .insert(last, rest.trim_start_matches(' ').into());
        }
    }

    /// `%Prefix`, `%Suffix` and `%Infix` (rule 10).
    fn add_rule(&mut self, part: &str) {
        let list = if part.starts_with("%Prefix ") {
            &mut self.prefixes
        } else if part.starts_with("%Suffix ") {
            &mut self.suffixes
        } else if part.starts_with("%Infix ") {
            &mut self.infixes
        } else {
            return;
        };
        if let Some(rule) = parse_rule(part) {
            // The rule defined last is tried first.
            list.insert(0, rule);
        }
    }

    fn finish(self, last_year: u16) -> SpellChecker {
        let mut sorted: Vec<u32> =
            (0..u32::try_from(self.entries.len()).unwrap_or(u32::MAX)).collect();
        sorted.sort_unstable_by(|&a, &b| {
            let ka = self.entries.get(a as usize).map(|e| &e.key);
            let kb = self.entries.get(b as usize).map(|e| &e.key);
            ka.cmp(&kb).then(a.cmp(&b))
        });
        SpellChecker {
            kind: self.kind,
            exclude: self.exclude.into_boxed_slice(),
            entries: self.entries,
            sorted,
            exact: self.exact,
            prefixes: self.prefixes,
            suffixes: self.suffixes,
            infixes: self.infixes,
            side: self.side,
            num_names: self.num_names,
            num_aliases: self.num_aliases,
            has_elo: self.has_elo,
            last_year,
            fingerprint: super::next_fingerprint(),
        }
    }
}

/// `"a" "b"` after the keyword: four double quotes, `a` not empty.
fn parse_rule(part: &str) -> Option<Rule> {
    let q1 = part.find('"')?;
    let rest = part.get(q1 + 1..)?;
    if rest.starts_with('"') {
        // Empty `from`: undefined in the Scid, ignored here.
        return None;
    }
    let q2 = rest.find('"')?;
    let from = rest.get(..q2)?;
    let rest = rest.get(q2 + 1..)?;
    let q3 = rest.find('"')?;
    let rest = rest.get(q3 + 1..)?;
    let q4 = rest.find('"')?;
    let to = rest.get(..q4)?;
    Some(Rule {
        from: from.into(),
        to: to.into(),
    })
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// The lines of a text, ended by `\n`, `\r\n` or `\r`; the last one needs no
/// terminator.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(['\n', '\r']).unwrap_or(rest.len());
        let line = rest.get(..end).unwrap_or("");
        let mut next = end;
        let tail = rest.get(end..).unwrap_or("");
        if tail.starts_with("\r\n") {
            next += 2;
        } else if !tail.is_empty() {
            next += 1;
        }
        rest = rest.get(next..).unwrap_or("");
        Some(line)
    })
}

fn parse_text(
    text: &str,
    last_year: u16,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<SpellingSet, SpellingError> {
    let mut builders = NameKind::ALL.map(Builder::new);
    let mut report = LoadReport::default();
    let mut section: Option<usize> = None;
    let total = u64::try_from(text.len()).unwrap_or(u64::MAX);
    let mut done = 0u64;
    let mut since_check = 0u32;
    for (line_no, line) in (1u64..).zip(lines(text)) {
        done += u64::try_from(line.len()).unwrap_or(0) + 1;
        since_check += 1;
        if since_check >= CHECK_EVERY {
            since_check = 0;
            progress.progress(done.min(total), total);
            if cancel.is_cancelled() {
                return Err(SpellingError::Cancelled);
            }
        }
        if line.len() > MAX_LINE_BYTES {
            report.skipped_lines = report.skipped_lines.saturating_add(1);
            continue;
        }
        let (part, comment) = match line.split_once('#') {
            Some((p, c)) => (p, c),
            None => (line, ""),
        };
        let part = part.trim_matches(is_space);
        let comment = comment.trim_end_matches(is_space);
        let Some(first) = part.chars().next() else {
            continue;
        };
        match first {
            '>' => {}
            '%' => {
                for b in &mut builders {
                    b.add_directive(part, &mut report);
                }
                if let Some(b) = section.and_then(|k| builders.get_mut(k)) {
                    b.add_rule(part);
                }
            }
            '@' => {
                let kind = name_type_from_str(part.get(1..).unwrap_or(""));
                section = kind.map(NameKind::index);
                if let Some(b) = section.and_then(|k| builders.get_mut(k))
                    && let Some(open) = part.find('"')
                {
                    let rest = part.get(open + 1..).unwrap_or("");
                    let close = rest
                        .find('"')
                        .ok_or(SpellingError::UnclosedQuote { line: line_no })?;
                    b.exclude = rest.get(..close).unwrap_or("").chars().collect();
                }
            }
            '=' => {
                if let Some(b) = section.and_then(|k| builders.get_mut(k)) {
                    b.add_alias(part.get(1..).unwrap_or("").trim_start_matches(' '));
                }
            }
            _ => {
                if let Some(b) = section.and_then(|k| builders.get_mut(k)) {
                    b.add_correct(part, comment);
                }
            }
        }
    }
    progress.progress(total, total);
    let [player, event, site, round] = builders;
    report.players = player.num_names;
    report.events = event.num_names;
    report.sites = site.num_names;
    report.rounds = round.num_names;
    Ok(SpellingSet {
        player: player.finish(last_year),
        event: event.finish(last_year),
        site: site.finish(last_year),
        round: round.finish(last_year),
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_splitting_handles_all_terminators() {
        let all: Vec<&str> = lines("a\nb\r\nc\rd").collect();
        assert_eq!(all, ["a", "b", "c", "d"]);
        let blank: Vec<&str> = lines("a\n\nb\n").collect();
        assert_eq!(blank, ["a", "", "b"]);
        assert_eq!(lines("").count(), 0);
    }

    #[test]
    fn section_names() {
        for (text, kind) in [
            ("P", Some(NameKind::Player)),
            ("PLAYER \"., -_*\"", Some(NameKind::Player)),
            ("pl \"x\"", Some(NameKind::Player)),
            ("site", Some(NameKind::Site)),
            ("ROUND \"\"", Some(NameKind::Round)),
            ("Ev", Some(NameKind::Event)),
            ("FOO", None),
            ("", None),
            (" ", None),
        ] {
            assert_eq!(name_type_from_str(text), kind, "{text:?}");
        }
    }

    #[test]
    fn rules_need_four_quotes() {
        let r = parse_rule("%Prefix \"GM \" \"\"").unwrap();
        assert_eq!((&*r.from, &*r.to), ("GM ", ""));
        let r = parse_rule("%Suffix \" \" \"\"").unwrap();
        assert_eq!((&*r.from, &*r.to), (" ", ""));
        assert!(parse_rule("%Prefix \"GM \"").is_none());
        assert!(parse_rule("%Prefix \"\" \"x\"").is_none());
        assert!(parse_rule("%Prefix GM").is_none());
    }

    #[test]
    fn current_year_is_plausible() {
        assert!((2024..2200).contains(&current_year()));
    }

    #[test]
    fn strip_removes_excluded_chars() {
        assert_eq!(strip("Almeida, B C", &[',', ' ']), "AlmeidaBC");
        assert_eq!(strip("é-x", &['-']), "éx");
    }
}
