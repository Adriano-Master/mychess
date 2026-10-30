// Derived from Scid vs PC 4.27, src/pgnparse.cpp (ExtractPgnTag and the tag mapping).
// Copyright (c) 2001-2003 Shane Hudson. All rights reserved (notice of
// pgnparse.cpp); Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The header section: tag-line lexer (D23) and the mapping of tags onto
//! [`Tags`] (D24-D26).

use std::borrow::Cow;

use mychess_core::{Eco, Elo, GameResult, MAX_ELO, PartialDate, Rating, RatingType, Tags};

use super::chars::{first_non_space, is_blank, is_eol};
use super::options::{PlayerNames, ReadOptions};
use super::text::{leading_number, split_name_rating, standard_player_name};

/// One thing found in the header section. Positions are indices in the data.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HeaderItem<'a> {
    /// A tag with its name and unescaped value (still undecoded bytes).
    Tag {
        name: &'a [u8],
        value: Cow<'a, [u8]>,
        pos: usize,
    },
    /// Something that looked like a tag but is not readable.
    BadTag { pos: usize },
    /// Text after the last tag of a line.
    Junk { pos: usize },
}

fn skip_blank(data: &[u8], mut i: usize, end: usize) -> usize {
    while i < end && is_blank(data[i]) {
        i += 1;
    }
    i
}

fn find_eol(data: &[u8], from: usize) -> usize {
    data[from..]
        .iter()
        .position(|&b| is_eol(b))
        .map_or(data.len(), |i| from + i)
}

/// `\"` and `\\` are decoded; any other backslash stays.
fn unescape(raw: &[u8]) -> Cow<'_, [u8]> {
    if !raw.contains(&b'\\') {
        return Cow::Borrowed(raw);
    }
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && matches!(raw.get(i + 1), Some(b'"' | b'\\')) {
            out.push(raw[i + 1]);
            i += 2;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// Reads the tags of the line `data[start..end]`, which starts with `[`.
///
/// A value ends at the first unescaped `"` followed by blanks and `]`. When
/// there is none, the last `"` of the line is used if only `]` and blanks
/// follow it (this recovers `[White "Doe, "Jack""]`).
///
/// Anything after the last tag of the line that is not another tag (for
/// instance moves written on the tag line) is reported as `Junk` and lost,
/// as in the Scid, which reads a tag line as a whole.
fn lex_tag_line<'a>(
    data: &'a [u8],
    start: usize,
    end: usize,
    on_item: &mut impl FnMut(HeaderItem<'a>),
) {
    let mut i = start;
    loop {
        let tag_pos = i;
        i = skip_blank(data, i + 1, end);
        let name_start = i;
        while i < end && !is_blank(data[i]) && data[i] != b'"' && data[i] != b']' {
            i += 1;
        }
        let name = &data[name_start..i];
        i = skip_blank(data, i, end);
        if i >= end || data[i] != b'"' {
            on_item(HeaderItem::BadTag { pos: tag_pos });
            return;
        }
        let value_start = i + 1;
        let mut escaped = false;
        let mut last_quote = None;
        let mut found = None;
        let mut j = value_start;
        while j < end {
            match data[j] {
                b'"' => {
                    last_quote = Some(j);
                    if !escaped {
                        let k = skip_blank(data, j + 1, end);
                        if k < end && data[k] == b']' {
                            found = Some((j, k + 1));
                            break;
                        }
                    }
                    escaped = false;
                }
                b'\\' => escaped = !escaped,
                _ => escaped = false,
            }
            j += 1;
        }
        if found.is_none()
            && let Some(q) = last_quote
        {
            let k = skip_blank(data, q + 1, end);
            if k >= end || (data[k] == b']' && skip_blank(data, k + 1, end) >= end) {
                found = Some((q, end));
            }
        }
        let Some((value_end, close)) = found else {
            on_item(HeaderItem::BadTag { pos: tag_pos });
            return;
        };
        on_item(HeaderItem::Tag {
            name,
            value: unescape(&data[value_start..value_end]),
            pos: tag_pos,
        });
        i = skip_blank(data, close, end);
        if i >= end {
            return;
        }
        if data[i] != b'[' {
            on_item(HeaderItem::Junk { pos: i });
            return;
        }
    }
}

/// Walks the header section of `data` (Scid's `PARSE_Header`): tag lines,
/// blank lines and `%` / `;` lines, up to the first other character, which
/// starts the movetext and is not consumed.
///
/// Returns where the header ends and whether the movetext starts there (it
/// does not when the data ends first).
pub(crate) fn scan_header<'a>(
    data: &'a [u8],
    mut on_item: impl FnMut(HeaderItem<'a>),
) -> (usize, bool) {
    let mut pos = 0;
    loop {
        // NBSP counts as white space (Scid: `charIsSpace(160)`).
        match first_non_space(&data[pos..]) {
            Some(n) => pos += n,
            None => return (data.len(), false),
        }
        let Some(&c) = data.get(pos) else {
            return (data.len(), false);
        };
        match c {
            b'[' => {
                let end = find_eol(data, pos);
                lex_tag_line(data, pos, end, &mut on_item);
                pos = end;
            }
            b'%' | b';' => pos = find_eol(data, pos),
            _ => return (pos, true),
        }
    }
}

/// What applying a tag may report besides storing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagNote {
    /// A rating above 4000 was cut.
    RatingTooLarge,
    /// The tag name cannot be stored (not a PGN symbol).
    BadName,
    /// The first tag beyond `max_tags_per_game`; it was dropped.
    TooMany,
    /// Another tag beyond the limit (dropped, not reported again).
    OverLimit,
}

/// The header being assembled.
pub(crate) struct HeaderState {
    pub(crate) tags: Tags,
    /// The value of the last `FEN` tag and the position of that tag.
    pub(crate) fen: Option<(String, usize)>,
    pub(crate) variant: Option<String>,
    /// Whether a `Result` tag was seen.
    pub(crate) result_tag: bool,
    /// Tags without a dedicated field seen so far (kept or rejected).
    extra_seen: usize,
}

#[derive(Clone, Copy)]
enum Side {
    White,
    Black,
}

impl HeaderState {
    pub(crate) fn new() -> HeaderState {
        HeaderState {
            tags: Tags::new(),
            fen: None,
            variant: None,
            result_tag: false,
            extra_seen: 0,
        }
    }

    fn rating_slot(&mut self, side: Side) -> &mut Option<Rating> {
        match side {
            Side::White => &mut self.tags.white_rating,
            Side::Black => &mut self.tags.black_rating,
        }
    }

    fn set_rating(&mut self, side: Side, raw: u32, kind: RatingType) -> Option<TagNote> {
        let note = (raw > u32::from(MAX_ELO)).then_some(TagNote::RatingTooLarge);
        let capped = u16::try_from(raw.min(u32::from(MAX_ELO))).unwrap_or(MAX_ELO);
        // Rating 0 means "no rating" (Scid), so it is stored as `None`.
        *self.rating_slot(side) = (capped > 0)
            .then(|| Elo::new(capped).ok())
            .flatten()
            .map(|elo| Rating::new(elo, kind));
        note
    }

    fn set_player(&mut self, side: Side, value: &str, opts: &ReadOptions) -> Option<TagNote> {
        // The rating is extracted before the name is normalised (D24).
        let (name, rating) = split_name_rating(value);
        let note = rating.and_then(|raw| self.set_rating(side, raw, RatingType::Elo));
        let name = match opts.player_names {
            PlayerNames::Scid => standard_player_name(name),
            PlayerNames::AsIs => name.to_owned(),
        };
        let name = if name.is_empty() {
            "?".to_owned()
        } else {
            name
        };
        match side {
            Side::White => self.tags.white = name,
            Side::Black => self.tags.black = name,
        }
        note
    }

    /// Applies one tag. `pos` is the position of the tag in the data.
    pub(crate) fn apply(
        &mut self,
        name: &str,
        mut value: String,
        pos: usize,
        opts: &ReadOptions,
    ) -> Option<TagNote> {
        if opts.ignored_tags.iter().any(|t| t == name) {
            return None;
        }
        // The Scid ignores empty values of these tags.
        let nonempty = !value.is_empty();
        match name {
            "Event" => {
                if nonempty {
                    self.tags.event = value;
                }
            }
            "Site" => {
                if nonempty {
                    self.tags.site = value;
                }
            }
            "Round" => {
                if nonempty {
                    self.tags.round = value;
                }
            }
            "White" if nonempty => return self.set_player(Side::White, &value, opts),
            "Black" if nonempty => return self.set_player(Side::Black, &value, opts),
            "Result" => {
                self.tags.result = GameResult::from_str_lenient(&value);
                self.result_tag = true;
            }
            "Date" => self.tags.date = PartialDate::parse_lenient(&value),
            "EventDate" => self.tags.event_date = PartialDate::parse_lenient(&value),
            "ECO" => self.tags.eco = value.trim().parse::<Eco>().ok(),
            "FEN" => self.fen = Some((value, pos)),
            // An empty name is ignored, like the Scid does; `SetUp` is derived
            // from the start position when writing.
            "White" | "Black" | "SetUp" => {}
            _ => return self.apply_other(name, &mut value, opts),
        }
        None
    }

    /// Rating tags (`WhiteElo`...) and everything else.
    fn apply_other(
        &mut self,
        name: &str,
        value: &mut String,
        opts: &ReadOptions,
    ) -> Option<TagNote> {
        let rating = [("White", Side::White), ("Black", Side::Black)]
            .into_iter()
            .find_map(|(prefix, side)| {
                let kind = RatingType::from_name(name.strip_prefix(prefix)?)?;
                Some((side, kind))
            });
        if let Some((side, kind)) = rating
            && self.rating_slot(side).is_none_or(|r| r.value.get() == 0)
        {
            let raw = leading_number(value);
            return self.set_rating(side, raw, kind);
        }
        if name == "Variant" {
            self.variant = Some(value.clone());
            // Chess960 is derived from the start position, never kept as a
            // tag (the writer emits it again).
            if classify_variant(value) == VariantKind::Chess960 {
                return None;
            }
        }
        // `Tags::set_extra` looks the name up in a list: bound the list.
        if self.extra_seen >= opts.max_tags_per_game {
            let first = self.extra_seen == opts.max_tags_per_game;
            self.extra_seen = self.extra_seen.saturating_add(1);
            return Some(if first {
                TagNote::TooMany
            } else {
                TagNote::OverLimit
            });
        }
        self.extra_seen += 1;
        if value.contains('\0') {
            *value = value.replace('\0', "\u{fffd}");
        }
        self.tags
            .set_extra(name, value)
            .err()
            .map(|_| TagNote::BadName)
    }
}

/// What a `Variant` tag value means (D58).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VariantKind {
    /// Standard chess (also empty and "From Position").
    Standard,
    /// Chess960 under any of its spellings.
    Chess960,
    /// Anything else (Crazyhouse, Atomic, ...).
    Other,
}

fn variant_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Classifies a `Variant` value: case, spaces, `-` and `_` do not matter.
pub(crate) fn classify_variant(value: &str) -> VariantKind {
    match variant_key(value).as_str() {
        "chess960" | "fischerandom" | "fischerrandom" | "fischerandomchess"
        | "fischerrandomchess" | "frc" | "960" | "freestyle" => VariantKind::Chess960,
        "" | "standard" | "chess" | "normal" | "fromposition" | "std" => VariantKind::Standard,
        _ => VariantKind::Other,
    }
}

/// Whether a `Variant` value states that the game is standard chess
/// (`Standard`, `Chess`, `Normal`), as opposed to empty or "From Position",
/// which say nothing about the FEN.
pub(crate) fn is_explicit_standard(value: &str) -> bool {
    matches!(
        variant_key(value).as_str(),
        "standard" | "chess" | "normal" | "std"
    )
}

#[cfg(test)]
mod tests {
    use mychess_core::GameResult;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    /// The tags of a header text as (name, value) pairs; `!` marks a bad tag
    /// and `?` junk.
    fn lex(text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        scan_header(text.as_bytes(), |item| match item {
            HeaderItem::Tag { name, value, .. } => out.push((
                String::from_utf8_lossy(name).into_owned(),
                String::from_utf8_lossy(&value).into_owned(),
            )),
            HeaderItem::BadTag { .. } => out.push(("!".into(), String::new())),
            HeaderItem::Junk { .. } => out.push(("?".into(), String::new())),
        });
        out
    }

    fn pair(n: &str, v: &str) -> (String, String) {
        (n.to_owned(), v.to_owned())
    }

    #[rstest]
    #[case("[Event \"x\"]", "x")]
    #[case("[Event \"\"]", "")]
    #[case("[Event \"a \\\"b\\\" c\"]", "a \"b\" c")]
    #[case("[Event \"back\\\\slash\"]", "back\\slash")]
    #[case("[Event \"keep\\nthis\"]", "keep\\nthis")]
    #[case("[White \"Doe, \"Jack\"\"]", "Doe, \"Jack\"")]
    #[case("[White \"A\" ]", "A")]
    #[case("  [ White   \"A\" ]  ", "A")]
    #[case("[White \"A\"", "A")]
    #[case("[White \"A\"  ", "A")]
    #[case("[Event \"x ] y\"]", "x ] y")]
    #[case("[Event \"C:\\\"]", "C:\\")]
    fn lexes_one_tag(#[case] line: &str, #[case] value: &str) {
        let tags = lex(line);
        assert_eq!(tags.len(), 1, "{tags:?}");
        assert_eq!(tags[0].1, value);
    }

    #[rstest]
    #[case("[Event]")]
    #[case("[Event x]")]
    #[case("[Event \"x]")]
    #[case("[Event \"x] trailing")]
    #[case("[]")]
    fn rejects_malformed_tags(#[case] line: &str) {
        assert_eq!(lex(line), vec![pair("!", "")], "{line}");
    }

    #[test]
    fn several_tags_per_line_and_tag_after_bad_one() {
        assert_eq!(
            lex("[A \"1\"] [B \"2\"][C \"3\"]\n[D \"4\"]"),
            vec![
                pair("A", "1"),
                pair("B", "2"),
                pair("C", "3"),
                pair("D", "4")
            ]
        );
        assert_eq!(
            lex("[Bad\n[Good \"1\"]"),
            vec![pair("!", ""), pair("Good", "1")]
        );
        assert_eq!(lex("[A \"1\"] junk"), vec![pair("A", "1"), pair("?", "")]);
    }

    #[test]
    fn header_ends_at_first_foreign_character() {
        let text = "[A \"1\"]\n\n% comment\n; other\n\n[B \"2\"]\n1. e4";
        let (end, movetext) = scan_header(text.as_bytes(), |_| {});
        assert!(movetext);
        assert_eq!(&text[end..], "1. e4");
        let (end, movetext) = scan_header(b"[A \"1\"]\n\n  ", |_| {});
        assert!(!movetext);
        assert_eq!(end, 11);
    }

    #[test]
    fn crlf_and_cr_line_ends_separate_tags() {
        for nl in ["\r\n", "\r"] {
            let text = format!("[A \"1\"]{nl}[B \"2\"]{nl}{nl}1. e4");
            assert_eq!(lex(&text), vec![pair("A", "1"), pair("B", "2")], "{nl:?}");
        }
    }

    fn apply(h: &mut HeaderState, name: &str, value: &str) -> Option<TagNote> {
        h.apply(name, value.to_owned(), 0, &ReadOptions::default())
    }

    #[test]
    fn maps_the_standard_tags() {
        let mut h = HeaderState::new();
        for (n, v) in [
            ("Event", "Open"),
            ("Site", "Rome"),
            ("Round", "3"),
            ("Date", "2024.05.??"),
            ("EventDate", "2024.05.01"),
            ("Result", "1/2-1/2"),
            ("ECO", "b91"),
        ] {
            assert_eq!(apply(&mut h, n, v), None);
        }
        let t = &h.tags;
        assert_eq!(
            (t.event.as_str(), t.site.as_str(), t.round.as_str()),
            ("Open", "Rome", "3")
        );
        assert_eq!(t.date.to_string(), "2024.05.??");
        assert_eq!(t.event_date.to_string(), "2024.05.01");
        assert_eq!(t.result, GameResult::Draw);
        assert_eq!(t.eco.map(|e| e.to_string()).as_deref(), Some("B91"));
        assert!(h.result_tag);
    }

    #[test]
    fn empty_values_keep_the_default_and_repeats_take_the_last() {
        let mut h = HeaderState::new();
        for (n, v) in [("Event", ""), ("White", ""), ("Site", "A"), ("Site", "B")] {
            apply(&mut h, n, v);
        }
        assert_eq!(h.tags.event, "?");
        assert_eq!(h.tags.white, "?");
        assert_eq!(h.tags.site, "B");
    }

    #[test]
    fn invalid_result_eco_and_date_are_tolerated() {
        let mut h = HeaderState::new();
        apply(&mut h, "Result", "draw?");
        apply(&mut h, "ECO", "Z99");
        apply(&mut h, "Date", "3000.13.45");
        assert_eq!(h.tags.result, GameResult::Unknown);
        assert_eq!(h.tags.eco, None);
        assert!(h.tags.date.is_unknown());
    }

    #[test]
    fn rating_in_player_name() {
        let mut h = HeaderState::new();
        assert_eq!(apply(&mut h, "White", "Kasparov,G (2800)"), None);
        assert_eq!(h.tags.white, "Kasparov, G");
        let r = h.tags.white_rating.expect("rating");
        assert_eq!((r.value.get(), r.kind), (2800, RatingType::Elo));
    }

    #[test]
    fn rating_above_4000_is_cut_with_a_note() {
        let mut h = HeaderState::new();
        assert_eq!(
            apply(&mut h, "White", "X (9999)"),
            Some(TagNote::RatingTooLarge)
        );
        assert_eq!(h.tags.white_rating.map(|r| r.value.get()), Some(4000));
        assert_eq!(
            apply(&mut h, "BlackElo", "5000"),
            Some(TagNote::RatingTooLarge)
        );
        assert_eq!(h.tags.black_rating.map(|r| r.value.get()), Some(4000));
    }

    #[test]
    fn only_the_first_rating_type_wins() {
        let mut h = HeaderState::new();
        apply(&mut h, "WhiteElo", "2700");
        apply(&mut h, "WhiteRapid", "2650");
        apply(&mut h, "BlackUSCF", "1800");
        let w = h.tags.white_rating.expect("white");
        assert_eq!((w.value.get(), w.kind), (2700, RatingType::Elo));
        assert_eq!(h.tags.get_extra("WhiteRapid"), Some("2650"));
        let b = h.tags.black_rating.expect("black");
        assert_eq!((b.value.get(), b.kind), (1800, RatingType::Uscf));
        // A rating in the name came first: WhiteElo goes to the extras.
        let mut h = HeaderState::new();
        apply(&mut h, "White", "A (2500)");
        apply(&mut h, "WhiteElo", "2600");
        assert_eq!(h.tags.white_rating.map(|r| r.value.get()), Some(2500));
        assert_eq!(h.tags.get_extra("WhiteElo"), Some("2600"));
    }

    #[test]
    fn non_numeric_rating_is_consumed_as_zero() {
        let mut h = HeaderState::new();
        apply(&mut h, "WhiteElo", "unknown");
        assert_eq!(h.tags.white_rating, None);
        assert_eq!(h.tags.extras().len(), 0);
        // Zero means "none", so the next type may still set it.
        apply(&mut h, "WhiteUSCF", "1500");
        assert_eq!(h.tags.white_rating.map(|r| r.value.get()), Some(1500));
    }

    #[test]
    fn other_tags_become_extras_in_order() {
        let mut h = HeaderState::new();
        apply(&mut h, "TimeControl", "40/7200");
        apply(&mut h, "ScidFlags", "WBM");
        apply(&mut h, "Variant", "Standard");
        apply(&mut h, "SetUp", "1");
        apply(&mut h, "FEN", "8/8/8/8/8/8/8/K6k w - - 0 1");
        let names: Vec<_> = h.tags.extras().iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["TimeControl", "ScidFlags", "Variant"]);
        assert_eq!(h.variant.as_deref(), Some("Standard"));
        assert_eq!(
            h.fen.as_ref().map(|f| f.0.as_str()),
            Some("8/8/8/8/8/8/8/K6k w - - 0 1")
        );
    }

    #[test]
    fn unstorable_names_report_a_note() {
        let mut h = HeaderState::new();
        assert_eq!(apply(&mut h, "Time-Control", "x"), Some(TagNote::BadName));
        assert_eq!(h.tags.extras().len(), 0);
    }

    #[test]
    fn ignored_tags_are_dropped() {
        let mut h = HeaderState::new();
        let opts = ReadOptions {
            ignored_tags: vec!["PlyCount".to_owned(), "Event".to_owned()],
            ..ReadOptions::default()
        };
        h.apply("PlyCount", "40".to_owned(), 0, &opts);
        h.apply("Event", "x".to_owned(), 0, &opts);
        assert_eq!(h.tags.extras().len(), 0);
        assert_eq!(h.tags.event, "?");
    }

    #[test]
    fn as_is_keeps_the_name_text() {
        let mut h = HeaderState::new();
        let opts = ReadOptions {
            player_names: PlayerNames::AsIs,
            ..ReadOptions::default()
        };
        h.apply("White", "van der Berg,  J (2100)".to_owned(), 0, &opts);
        assert_eq!(h.tags.white, "van der Berg,  J");
        assert_eq!(h.tags.white_rating.map(|r| r.value.get()), Some(2100));
    }
}
