//! The texts of the tactics training: they are chosen from the state of the
//! application, never written in the code (spec M8-4, D1941). Pure
//! functions, so the tests need no window.

use fluent_bundle::FluentArgs;
use mychess_app::tactics::{
    FailReason, Feedback, ModeStats, ProblemOutcome, TacticsMode, TacticsNotice, TacticsPhase,
    TacticsView, Wrong,
};

use crate::i18n::I18n;

/// The Fluent key of the line that says what the user has to do or what
/// happened (D1928).
pub fn status_key(view: &TacticsView) -> &'static str {
    match view.phase {
        TacticsPhase::Opening => "tactics-status-opening",
        TacticsPhase::Searching => "tactics-status-searching",
        TacticsPhase::Judging => "tactics-status-judging",
        TacticsPhase::Solving => match view.feedback {
            Feedback::Correct => "tactics-feedback-correct",
            Feedback::Wrong(Wrong::NoMate) => "tactics-feedback-no-mate",
            Feedback::Wrong(Wrong::LongerMate) => "tactics-feedback-longer-mate",
            Feedback::Wrong(_) => "tactics-feedback-not-best",
            _ => match view.mode {
                TacticsMode::BestMove => "tactics-status-solving-best",
                _ => "tactics-status-solving-mate",
            },
        },
        TacticsPhase::Solved(ProblemOutcome::Perfect) => "tactics-result-perfect",
        TacticsPhase::Solved(ProblemOutcome::WithMistakes) => "tactics-result-mistakes",
        TacticsPhase::Solved(_) => "tactics-result-help",
        TacticsPhase::Failed(FailReason::GaveUp) => "tactics-result-gave-up",
        TacticsPhase::Failed(FailReason::NoAttempts) => "tactics-result-no-attempts",
        TacticsPhase::Failed(_) => "tactics-result-time-up",
        TacticsPhase::Exhausted => "tactics-status-exhausted",
        _ => "tactics-status-closed",
    }
}

/// The line of the status, translated.
pub fn status_text(view: &TacticsView, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("n", i64::from(view.mate_in.unwrap_or(0)));
    args.set("count", i64::try_from(view.examined).unwrap_or(i64::MAX));
    i18n.tr_args(status_key(view), &args)
}

/// The title of the panel: the mode, and the mate that is left.
pub fn title_text(view: &TacticsView, i18n: &I18n) -> String {
    match (view.mode, view.mate_in) {
        (TacticsMode::Mate, Some(n)) => {
            let mut args = FluentArgs::new();
            args.set("n", i64::from(n));
            i18n.tr_args("tactics-title-mate", &args)
        }
        (TacticsMode::BestMove, _) => i18n.tr("tactics-title-best"),
        _ => i18n.tr("tactics-title"),
    }
}

/// The remark of the application, translated.
pub fn notice_text(notice: TacticsNotice, i18n: &I18n) -> String {
    i18n.tr(notice.message_key())
}

/// "m:ss".
pub fn clock_text(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The score of a mode in one line.
pub fn stats_text(stats: &ModeStats, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("solved", i64::from(stats.solved()));
    args.set("shown", i64::from(stats.shown));
    args.set("percent", stats.success_percent().map_or(0, i64::from));
    args.set("streak", i64::from(stats.streak));
    args.set("best", i64::from(stats.best_streak));
    args.set(
        "time",
        stats.average_ms().map_or_else(
            || "-".to_owned(),
            |ms| clock_text(u32::try_from(ms / 1000).unwrap_or(u32::MAX)),
        ),
    );
    i18n.tr_args("tactics-stats", &args)
}

/// "N mate problems beyond the limit were skipped" (D2155).
pub fn beyond_text(view: &TacticsView, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("count", i64::from(view.beyond_limit));
    args.set("limit", i64::from(view.config.mate_max));
    i18n.tr_args("tactics-beyond-limit", &args)
}

/// The line of the hint: the piece to move (level one).
pub fn hint_text(view: &TacticsView, i18n: &I18n) -> Option<String> {
    let hint = view.hint?;
    let mut args = FluentArgs::new();
    args.set("square", hint.from.to_string());
    Some(i18n.tr_args("tactics-hint-piece", &args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_has_minutes_and_two_digit_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(75), "1:15");
        assert_eq!(clock_text(3600), "60:00");
    }
}
