//! Applying the corrections of the name checker (rules 45 and 48 to 59).
//!
//! The work itself is [`Si4Writer::apply_name_changes`], which recounts the
//! frequencies and rewrites the `.sn4`; this module has the types, the
//! conversion from [`Proposal`]s and the text format of the Scid
//! (`"old"<tab>>> "new" (N)`), used to copy the list and in tests.
//!
//! [`Si4Writer::apply_name_changes`]: crate::si4::Si4Writer::apply_name_changes

use std::fmt::Write as _;

use mychess_core::PartialDate;

use super::check::Proposal;
use crate::si4::NameKind;
pub use crate::si4::{NameChange, NameChangeReport, life_span_allows};

/// The longest line the Scid reads back (`sc_name correct`).
const MAX_LINE_CHARS: usize = 511;

/// The changes to apply for the proposals: ambiguous ones are left out, as
/// the Scid does (their lines are not read back).
pub fn changes_from_proposals(proposals: &[Proposal]) -> Vec<NameChange> {
    proposals
        .iter()
        .filter(|p| !p.ambiguous)
        .map(|p| NameChange {
            old: p.old.clone(),
            new: p.new.clone(),
            born: p.born,
            died: p.died,
        })
        .collect()
}

fn date_text(date: PartialDate) -> String {
    if date.is_unknown() {
        String::new()
    } else {
        date.to_string()
    }
}

/// The text of the Scid for a list of proposals: the heading `Scid found N
/// <kind> name correction(s).`, then one line per proposal, with a blank
/// line before the group of a different initial.
pub fn proposals_to_text(kind: NameKind, proposals: &[Proposal]) -> String {
    let n = proposals.len();
    let mut out = format!(
        "Scid found {n} {kind} name correction{}.\n",
        if n == 1 { "" } else { "s" }
    );
    let mut previous: Option<u8> = None;
    for p in proposals {
        let first = p.old.as_bytes().first().copied();
        if previous != first || previous.is_none() {
            out.push('\n');
        }
        if p.ambiguous {
            out.push_str("Ambiguous: ");
        }
        let _ = write!(out, "\"{}\"\t>> \"{}\" ({})", p.old, p.new, p.games);
        if kind == NameKind::Player && !(p.born.is_unknown() && p.died.is_unknown()) {
            let _ = write!(out, "  {}--{}", date_text(p.born), date_text(p.died));
        }
        out.push('\n');
        previous = first;
    }
    out
}

/// Reads the text of the Scid back (rule 48): only lines that start with `"`
/// and have `"old" >> "new"`; the count and the dates are optional. Lines
/// longer than 511 characters are cut. Ambiguous and indented lines are
/// not read.
pub fn parse_corrections_text(text: &str) -> Vec<NameChange> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<NameChange> {
    let cut = line
        .char_indices()
        .nth(MAX_LINE_CHARS)
        .map_or(line.len(), |(i, _)| i);
    let line = line.get(..cut)?;
    let rest = line.strip_prefix('"')?;
    let (old, rest) = rest.split_once('"')?;
    let rest = rest.trim_start().strip_prefix(">>")?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let (new, rest) = rest.split_once('"')?;
    if old.is_empty() || new.is_empty() {
        return None;
    }
    let mut born = PartialDate::UNKNOWN;
    let mut died = PartialDate::UNKNOWN;
    let rest = rest.trim_start();
    // "(N)" then the dates; any part that does not fit is simply absent.
    let rest = rest
        .strip_prefix('(')
        .and_then(|r| r.split_once(')'))
        .map_or("", |(_, tail)| tail);
    let is_date = |c: char| c.is_ascii_digit() || c == '.' || c == '?';
    let rest = rest.trim_start();
    let end = rest.find(|c: char| !is_date(c)).unwrap_or(rest.len());
    if let Some(after) = rest.get(end..).and_then(|r| r.strip_prefix("--")) {
        born = PartialDate::parse_lenient(rest.get(..end).unwrap_or(""));
        let end = after.find(|c: char| !is_date(c)).unwrap_or(after.len());
        died = PartialDate::parse_lenient(after.get(..end).unwrap_or(""));
    }
    Some(NameChange {
        old: old.to_owned(),
        new: new.to_owned(),
        born,
        died,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(old: &str, new: &str, games: u32, ambiguous: bool) -> Proposal {
        Proposal {
            old: old.into(),
            new: new.into(),
            games,
            ambiguous,
            born: PartialDate::parse_lenient("1900.??.??"),
            died: PartialDate::parse_lenient("1970.04.??"),
        }
    }

    #[test]
    fn text_round_trip_keeps_names_and_dates() {
        let list = [p("Almeida, B", "Almeida, Bruno", 3, false)];
        let text = proposals_to_text(NameKind::Player, &list);
        assert!(text.starts_with("Scid found 1 player name correction.\n\n\"Almeida, B\"\t>> "));
        assert!(text.contains("(3)  1900.??.??--1970.04.??"));
        let back = parse_corrections_text(&text);
        assert_eq!(back, changes_from_proposals(&list));
    }

    #[test]
    fn ambiguous_and_odd_lines_are_not_read() {
        let text = "Ambiguous: \"a\"\t>> \"b\" (1)\n \"c\" >> \"d\"\n\"e\" >> \"f\"\n\"g\"\n\"\" >> \"h\"\n";
        let back = parse_corrections_text(text);
        assert_eq!(back, [NameChange::new("e", "f")]);
    }

    #[test]
    fn ambiguous_proposals_are_not_applied() {
        let list = [p("a", "b", 1, true), p("c", "d", 1, false)];
        assert_eq!(changes_from_proposals(&list).len(), 1);
    }

    #[test]
    fn groups_are_separated_by_a_blank_line() {
        let mut a = p("Abc", "Abd", 1, false);
        a.born = PartialDate::UNKNOWN;
        a.died = PartialDate::UNKNOWN;
        let mut b = a.clone();
        b.old = "Abx".into();
        let mut c = a.clone();
        c.old = "Bcd".into();
        let text = proposals_to_text(NameKind::Event, &[a, b, c]);
        assert_eq!(text.matches("\n\n").count(), 2, "{text:?}");
        assert!(!text.contains("--"));
    }
}
