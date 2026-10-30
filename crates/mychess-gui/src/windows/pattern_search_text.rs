//! The texts of the window of the search for tactics and patterns: chosen
//! from the state of the application, never written in the code (spec M6-2,
//! D3288). Pure functions, so the tests need no window.
//!
//! A card is described by a [`Sentence`]: a Fluent key plus arguments, some
//! of them nested sentences. [`sentence_text`] translates the arguments
//! first (a nested sentence recursively, a list joined with the translated
//! conjunction) and then the message itself. Two conventions keep the
//! messages short:
//!
//! - a region carries its own preposition, in the form its place in the
//!   sentence asks for (`mode`: "at" by default, "from" and "to" for the
//!   origin and the destination of a move); "anywhere" inside a sentence is
//!   an empty string and every other region starts with a blank, so a
//!   message writes `{ $piece }{ $where }`;
//! - when a sentence has numeric `min` and `max`, `same` is `1` if they are
//!   equal, so that a range can be written as one number.

use fluent_bundle::FluentArgs;
use mychess_app::pattern_search::model::{MAX_GROUP_ITEMS, NamedRegion, Region, RoleName, Who};
use mychess_app::pattern_search::{
    EditRefusal, PatternIssue, PatternRunSummary, PatternSearchNotice, Sentence, SentenceArg,
    describe_region,
};

use crate::i18n::I18n;

const ANYWHERE: &str = "pattern-region-anywhere";
const REGION_PREFIX: &str = "pattern-region-";

/// The translated text of a remark about the search for patterns.
pub fn notice_text(i18n: &I18n, notice: &PatternSearchNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        PatternSearchNotice::InvalidPattern(issue) => return error_text(i18n, issue),
        PatternSearchNotice::InvalidParam(field) => args.set("field", field.as_str()),
        PatternSearchNotice::UnknownPreset(name) => {
            args.set("name", name.as_str());
            args.set("id", name.as_str());
        }
        PatternSearchNotice::EditRefused(EditRefusal::GroupFull) => {
            args.set("limit", i64::try_from(MAX_GROUP_ITEMS).unwrap_or(i64::MAX));
        }
        PatternSearchNotice::Unreadable { count }
        | PatternSearchNotice::UnknownPositions { count } => {
            args.set("count", i64::try_from(*count).unwrap_or(i64::MAX));
        }
        PatternSearchNotice::Saved(name)
        | PatternSearchNotice::Deleted(name)
        | PatternSearchNotice::SavedNotFound(name)
        | PatternSearchNotice::Imported(name) => args.set("name", name.as_str()),
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The translated text of what is wrong with a pattern. The path of the
/// field is shown as written; the reason of the database is not exposed.
pub fn error_text(i18n: &I18n, issue: &PatternIssue) -> String {
    let notice = PatternSearchNotice::InvalidPattern(issue.clone());
    let mut args = FluentArgs::new();
    if let Some(path) = issue.path() {
        args.set("path", path);
    } else {
        args.set("path", "");
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// `N / M games (s.cc s)`, then the positions that were too complex and the
/// games that could not be read.
pub fn summary_text(i18n: &I18n, summary: &PatternRunSummary) -> String {
    let mut args = FluentArgs::new();
    args.set("matched", i64::from(summary.matched));
    args.set("start", i64::from(summary.start_count));
    args.set("secs", format!("{:.2}", summary.elapsed.as_secs_f64()));
    let mut text = i18n.tr_args("pattern-summary", &args);
    for (count, key) in [
        (summary.unknown_positions, "pattern-unknown"),
        (summary.unreadable, "pattern-unreadable"),
    ] {
        if count > 0 {
            let mut args = FluentArgs::new();
            args.set("count", i64::try_from(count).unwrap_or(i64::MAX));
            text.push_str("  ");
            text.push_str(&i18n.tr_args(key, &args));
        }
    }
    text
}

/// The sentence of a card, in the language of `i18n`, with no key left raw.
pub fn sentence_text(i18n: &I18n, sentence: &Sentence) -> String {
    translate(i18n, sentence, None).trim().to_owned()
}

/// A set of squares as a phrase ("on e4, d4"; "anywhere").
pub fn region_text(i18n: &I18n, region: &Region) -> String {
    sentence_text(i18n, &describe_region(region))
}

/// The same with the first letter in capitals, for a button.
pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The Fluent key of "white", "black"...
pub const fn who_key(who: Who) -> &'static str {
    match who {
        Who::White => "pattern-who-white",
        Who::Black => "pattern-who-black",
        Who::Mover => "pattern-who-mover",
        Who::Opponent => "pattern-who-opponent",
        Who::Any => "pattern-who-any",
    }
}

/// The Fluent key of the name of a kind of piece.
pub const fn role_key(role: RoleName) -> &'static str {
    match role {
        RoleName::King => "pattern-role-king",
        RoleName::Queen => "pattern-role-queen",
        RoleName::Rook => "pattern-role-rook",
        RoleName::Bishop => "pattern-role-bishop",
        RoleName::Knight => "pattern-role-knight",
        RoleName::Pawn => "pattern-role-pawn",
    }
}

/// The Fluent key of the name of an area of the board.
pub const fn named_region_key(named: NamedRegion) -> &'static str {
    match named {
        NamedRegion::Center => "pattern-region-named-center",
        NamedRegion::ExtendedCenter => "pattern-region-named-extended-center",
        NamedRegion::QueenSide => "pattern-region-named-queen-side",
        NamedRegion::KingSide => "pattern-region-named-king-side",
        NamedRegion::WhiteHalf => "pattern-region-named-white-half",
        NamedRegion::BlackHalf => "pattern-region-named-black-half",
        NamedRegion::LightSquares => "pattern-region-named-light-squares",
        NamedRegion::DarkSquares => "pattern-region-named-dark-squares",
        NamedRegion::Edge => "pattern-region-named-edge",
        NamedRegion::LongDiagonals => "pattern-region-named-long-diagonals",
    }
}

fn number(value: i64) -> i64 {
    value
}

/// `nested` is the name of the argument that holds the sentence, when it is
/// one (it decides the preposition of a region).
fn translate(i18n: &I18n, sentence: &Sentence, nested: Option<&str>) -> String {
    let is_region = sentence.key.starts_with(REGION_PREFIX)
        && !sentence.key.starts_with("pattern-region-kind-")
        && !matches!(
            sentence.key,
            "pattern-region-pick"
                | "pattern-region-use-marks"
                | "pattern-region-clear"
                | "pattern-region-from"
                | "pattern-region-to"
        );
    if is_region && sentence.key == ANYWHERE {
        return if nested.is_some() {
            String::new()
        } else {
            i18n.tr(ANYWHERE)
        };
    }
    let mut owned: Vec<(&'static str, Value)> = Vec::new();
    for (name, arg) in &sentence.args {
        owned.push((name, value_of(i18n, name, arg)));
    }
    let mut args = FluentArgs::new();
    for (name, value) in &owned {
        match value {
            Value::Text(text) => args.set(*name, text.as_str()),
            Value::Number(n) => args.set(*name, number(*n)),
        }
    }
    if let (Some(SentenceArg::Number(min)), Some(SentenceArg::Number(max))) =
        (sentence.arg("min"), sentence.arg("max"))
    {
        args.set("same", i64::from(min == max));
    }
    if is_region {
        let mode = match nested {
            Some("from") => "from",
            Some("to") => "to",
            _ => "at",
        };
        args.set("mode", mode);
    }
    let text = i18n.tr_args(sentence.key, &args);
    if is_region && nested.is_some() {
        format!(" {text}")
    } else {
        text
    }
}

enum Value {
    Text(String),
    Number(i64),
}

fn value_of(i18n: &I18n, name: &str, arg: &SentenceArg) -> Value {
    match arg {
        SentenceArg::Number(n) => Value::Number(*n),
        other => Value::Text(text_of(i18n, name, other)),
    }
}

fn text_of(i18n: &I18n, name: &str, arg: &SentenceArg) -> String {
    match arg {
        SentenceArg::Key(key) => i18n.tr(key),
        SentenceArg::Text(text) => text.clone(),
        SentenceArg::Number(n) => n.to_string(),
        SentenceArg::List(items) => {
            let conjunction = if name == "parts" {
                "pattern-list-and"
            } else {
                "pattern-list-or"
            };
            let texts: Vec<String> = items.iter().map(|i| text_of(i18n, name, i)).collect();
            join_list(i18n, &texts, conjunction)
        }
        SentenceArg::Phrase(inner) => translate(i18n, inner, Some(name)),
    }
}

/// `a`, `a and b`, `a, b and c` (with the conjunction `conjunction`).
fn join_list(i18n: &I18n, items: &[String], conjunction: &str) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [head @ .., last] => {
            let mut args = FluentArgs::new();
            args.set("head", head.join(", "));
            args.set("last", last.as_str());
            i18n.tr_args(conjunction, &args)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;
    use mychess_app::pattern_search::{ConditionKind, describe};

    #[test]
    fn a_list_is_joined_with_the_conjunction() {
        let i18n = I18n::new(Language::EnUs);
        let items = ["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(join_list(&i18n, &items, "pattern-list-or"), "a, b or c");
        assert_eq!(join_list(&i18n, &items[..2], "pattern-list-and"), "a and b");
        assert_eq!(join_list(&i18n, &items[..1], "pattern-list-or"), "a");
        assert_eq!(join_list(&i18n, &[], "pattern-list-or"), "");
    }

    #[test]
    fn every_default_card_reads_as_a_sentence_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for kind in ConditionKind::ALL {
                let text = sentence_text(&i18n, &describe(&kind.default_condition()));
                assert!(!text.is_empty(), "{kind:?}");
                assert!(!text.contains("pattern-"), "{kind:?}: {text}");
                assert!(!text.contains('{'), "{kind:?}: {text}");
            }
        }
    }

    #[test]
    fn capitals_start_a_label() {
        assert_eq!(capitalized("in check"), "In check");
        assert_eq!(capitalized(""), "");
    }
}
