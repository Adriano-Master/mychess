//! The texts of the window of the advanced searches: chosen from the state
//! of the application, never written in the code (spec M6-1, D3241). Pure
//! functions, so the tests need no window.

use fluent_bundle::FluentArgs;
use mychess_app::{AdvancedSearchNotice, CheckKind, SearchSummary};

use crate::i18n::I18n;

/// The translated text of a remark about the searches.
pub fn notice_text(i18n: &I18n, notice: &AdvancedSearchNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        AdvancedSearchNotice::InvalidMoveRange { min, max } => {
            args.set("min", i64::from(*min));
            args.set("max", i64::from(*max));
        }
        AdvancedSearchNotice::BadMoveToken(token)
        | AdvancedSearchNotice::CheckTestTooLow(token) => args.set("token", token.as_str()),
        AdvancedSearchNotice::CheckTestRaised(kind) => {
            args.set("test", i18n.tr(check_key(*kind)));
        }
        AdvancedSearchNotice::Unreadable { count } => {
            args.set("count", i64::try_from(*count).unwrap_or(i64::MAX));
        }
        AdvancedSearchNotice::Saved(name)
        | AdvancedSearchNotice::Deleted(name)
        | AdvancedSearchNotice::SavedNotFound(name) => args.set("name", name.as_str()),
        AdvancedSearchNotice::Imported { warnings } => {
            args.set("warnings", i64::try_from(*warnings).unwrap_or(i64::MAX));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The Fluent key of the name of a check test.
pub fn check_key(kind: CheckKind) -> &'static str {
    match kind {
        CheckKind::No => "advsearch-check-no",
        CheckKind::Check => "advsearch-check-check",
        CheckKind::Mate => "advsearch-check-mate",
    }
}

/// `N / M  (s.cc s)`, and the games that could not be read (the Scid's
/// line under the window, 16054-16063).
pub fn summary_text(i18n: &I18n, summary: &SearchSummary) -> String {
    let mut args = FluentArgs::new();
    args.set("matched", i64::from(summary.matched));
    args.set("total", i64::from(summary.start_count));
    args.set("seconds", format!("{:.2}", summary.elapsed.as_secs_f64()));
    let mut text = i18n.tr_args("advsearch-result", &args);
    if summary.unreadable > 0 {
        let mut args = FluentArgs::new();
        args.set(
            "count",
            i64::try_from(summary.unreadable).unwrap_or(i64::MAX),
        );
        text.push_str("  ");
        text.push_str(&i18n.tr_args("advsearch-result-unreadable", &args));
    }
    text
}

/// The Fluent key of the name of a common ending or pattern.
pub fn preset_key(key: &str) -> String {
    format!("search-{key}")
}
