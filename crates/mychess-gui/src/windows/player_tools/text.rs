//! The texts of the player tools: chosen from the state of the application,
//! never written in the code (spec M6-3). Pure functions, so the tests need
//! no window.

use fluent_bundle::FluentArgs;
use mychess_app::{FinderNotice, NameKind, PlayerInfoNotice, SpellingNotice};
use mychess_core::PartialDate;

use crate::i18n::I18n;

fn count(n: u32) -> i64 {
    i64::from(n)
}

/// The Fluent key of a kind of name ("player names").
pub fn kind_key(kind: NameKind) -> &'static str {
    match kind {
        NameKind::Event => "spell-kind-event",
        NameKind::Site => "spell-kind-site",
        NameKind::Round => "spell-kind-round",
        _ => "spell-kind-player",
    }
}

/// The remark of the status bar about the finders.
pub fn finder_notice_text(i18n: &I18n, notice: FinderNotice) -> String {
    let mut args = FluentArgs::new();
    if let FinderNotice::Filtered { games } = notice {
        args.set("games", count(games));
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The remark of the status bar about the player information.
pub fn info_notice_text(i18n: &I18n, notice: PlayerInfoNotice) -> String {
    let mut args = FluentArgs::new();
    if let PlayerInfoNotice::FilterSet { games } = notice {
        args.set("games", count(games));
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The remark about the spelling file, the name checker and "Add Elo".
pub fn spelling_notice_text(i18n: &I18n, notice: &SpellingNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        SpellingNotice::Loaded(counts) => {
            args.set("players", count(counts.players));
            args.set("events", count(counts.events));
            args.set("sites", count(counts.sites));
            args.set("rounds", count(counts.rounds));
        }
        SpellingNotice::LoadFailed(error) => {
            let mut reason = FluentArgs::new();
            if let mychess_app::SpellingLoadError::UnclosedQuote { line } = error {
                reason.set("line", i64::try_from(*line).unwrap_or(i64::MAX));
            }
            match error {
                mychess_app::SpellingLoadError::File(mychess_app::FileErrorKind::TooLarge {
                    limit,
                }) => reason.set("limit", *limit / (1024 * 1024)),
                mychess_app::SpellingLoadError::File(
                    mychess_app::FileErrorKind::UnsupportedEncoding(encoding),
                ) => reason.set("encoding", *encoding),
                _ => {}
            }
            args.set("reason", i18n.tr_args(error.reason_key(), &reason));
        }
        SpellingNotice::Applied {
            kind,
            names,
            times,
            skipped_by_date,
            not_found,
            interrupted,
        } => {
            return applied_text(
                i18n,
                *kind,
                [*names, *times, *skipped_by_date, *not_found],
                *interrupted,
            );
        }
        SpellingNotice::EloAdded { ratings, games } => {
            args.set("ratings", count(*ratings));
            args.set("games", count(*games));
        }
        SpellingNotice::Failed(error) => return i18n.tr(error.message_key()),
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The result of a correction: the line of the names and, when they apply,
/// the lines about the dates, the names that were gone and the stop.
fn applied_text(
    i18n: &I18n,
    kind: NameKind,
    [names, times, skipped, gone]: [u32; 4],
    stopped: bool,
) -> String {
    let mut args = FluentArgs::new();
    args.set("kind", i18n.tr(kind_key(kind)));
    args.set("names", count(names));
    args.set("times", count(times));
    let mut text = i18n.tr_args("spell-done", &args);
    if skipped > 0 {
        let mut more = FluentArgs::new();
        more.set("count", count(skipped));
        text.push(' ');
        text.push_str(&i18n.tr_args("spell-done-skipped", &more));
    }
    if gone > 0 {
        let mut more = FluentArgs::new();
        more.set("count", count(gone));
        text.push(' ');
        text.push_str(&i18n.tr_args("spell-not-found", &more));
    }
    if stopped {
        text.push(' ');
        text.push_str(&i18n.tr("spell-interrupted"));
    }
    text
}

/// A year or nothing.
pub fn year(year: Option<u16>) -> String {
    year.map(|y| y.to_string()).unwrap_or_default()
}

/// A Elo or nothing.
pub fn elo(elo: Option<u16>) -> String {
    elo.map(|e| e.to_string()).unwrap_or_default()
}

/// `1904.??.??--1979.08.15` as `1904--1979.08.15`; empty when both are
/// unknown.
pub fn life(born: PartialDate, died: PartialDate) -> String {
    match (born.is_unknown(), died.is_unknown()) {
        (true, true) => String::new(),
        (false, true) => format!("{}--", born.short()),
        (true, false) => format!("--{}", died.short()),
        (false, false) => format!("{}--{}", born.short(), died.short()),
    }
}

/// `87.50%` from hundredths of a percent.
pub fn percent(i18n: &I18n, hundredths: u32) -> String {
    i18n.decimal(&format!("{}.{:02}%", hundredths / 100, hundredths % 100))
}

/// A rating history entry, `2020.01  2500`.
pub fn rating_entry(year: u16, month: u8, elo: u16) -> String {
    format!("{year}.{month:02}  {elo}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn percent_keeps_two_decimals() {
        let en = I18n::new(Language::EnUs);
        assert_eq!(percent(&en, 8750), "87.50%");
        assert_eq!(percent(&en, 0), "0.00%");
        assert_eq!(percent(&en, 10000), "100.00%");
        assert_eq!(percent(&I18n::new(Language::PtBr), 8750), "87,50%");
    }

    #[test]
    fn life_shows_what_is_known() {
        let born = PartialDate::parse_lenient("1904.??.??");
        let died = PartialDate::parse_lenient("1979.08.15");
        assert_eq!(life(born, died), "1904--1979.08.15");
        assert_eq!(life(born, PartialDate::UNKNOWN), "1904--");
        assert_eq!(life(PartialDate::UNKNOWN, PartialDate::UNKNOWN), "");
    }

    #[test]
    fn the_result_of_a_correction_adds_the_extra_lines() {
        let i18n = I18n::new(Language::EnUs);
        let text = applied_text(&i18n, NameKind::Player, [4, 7, 2, 0], true);
        assert!(
            text.contains("4 names (player names)") && text.contains('7'),
            "{text}"
        );
        assert!(text.contains("2 player name changes"), "{text}");
        assert!(text.contains("Stopped"), "{text}");
    }
}
