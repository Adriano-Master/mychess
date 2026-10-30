//! The texts of the calculation exercise: they are chosen from the state of
//! the application, never written in the code (spec M8-6, D2328). Pure
//! functions, so the tests need no window.

use fluent_bundle::FluentArgs;
use mychess_app::calvar::{
    Assessment, CalvarFailure, CalvarNotice, CalvarOrigin, CalvarPhase, CalvarStats, CalvarView,
    HistoryRow, LineReport, Report,
};
use mychess_core::Color;

use crate::i18n::I18n;

/// The Fluent key of the meaning of an evaluation symbol.
pub fn meaning_key(assessment: Assessment) -> &'static str {
    match assessment {
        Assessment::BlackWinning => "calvar-eval-black-winning",
        Assessment::BlackBetter => "calvar-eval-black-better",
        Assessment::BlackSlightly => "calvar-eval-black-slightly",
        Assessment::Equal => "calvar-eval-equal",
        Assessment::WhiteSlightly => "calvar-eval-white-slightly",
        Assessment::WhiteBetter => "calvar-eval-white-better",
        Assessment::WhiteWinning => "calvar-eval-white-winning",
    }
}

/// "+= (White is slightly better)": the tip of an evaluation button.
pub fn eval_tip(assessment: Assessment, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("symbol", assessment.symbol());
    args.set("meaning", i18n.tr(meaning_key(assessment)));
    i18n.tr_args("calvar-eval-tip", &args)
}

/// The title of the panel: the side that moves in the position.
pub fn title_text(view: &CalvarView, i18n: &I18n) -> String {
    match view.mover {
        Some(Color::White) => i18n.tr("calvar-title-white"),
        Some(Color::Black) => i18n.tr("calvar-title-black"),
        None => i18n.tr("calvar-title"),
    }
}

/// The line that says what the user has to do or what happened (D2319).
pub fn status_text(view: &CalvarView, i18n: &I18n) -> String {
    let key = match view.phase {
        CalvarPhase::Entering if view.can_done => "calvar-status-ready",
        CalvarPhase::Entering => "calvar-status-entering",
        CalvarPhase::Evaluating if !view.root_ready => "calvar-status-waiting-root",
        CalvarPhase::Evaluating => "calvar-status-evaluating",
        CalvarPhase::Failed(CalvarFailure::TimedOut) => "calvar-status-failed-timed-out",
        CalvarPhase::Failed(CalvarFailure::TimeUp) => "calvar-status-failed-time-up",
        CalvarPhase::Failed(_) => "calvar-status-failed-engine",
        _ => "calvar-title",
    };
    i18n.tr(key)
}

/// The remark of the application, translated.
pub fn notice_text(notice: CalvarNotice, i18n: &I18n) -> String {
    i18n.tr(notice.message_key())
}

/// The text refused by the last entry, if any.
pub fn rejected_text(view: &CalvarView, i18n: &I18n) -> Option<String> {
    let text = view.rejected.as_ref()?;
    let mut args = FluentArgs::new();
    args.set("text", text.as_str());
    Some(i18n.tr_args("calvar-rejected", &args))
}

/// "m:ss".
pub fn clock_text(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// "Lines: 2 of 3".
pub fn lines_text(view: &CalvarView, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("count", i64::try_from(view.lines.len()).unwrap_or(i64::MAX));
    args.set("required", i64::from(view.required));
    i18n.tr_args("calvar-lines-given", &args)
}

/// Whether the engine is still thinking about the start position, and its
/// name.
pub fn engine_text(view: &CalvarView, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("name", view.engine_name.as_str());
    let key = if view.root_ready {
        "calvar-engine-ready"
    } else {
        "calvar-engine-thinking"
    };
    i18n.tr_args(key, &args)
}

/// "Line in progress: 1. e4 e5", or the invitation to type.
pub fn current_text(view: &CalvarView, i18n: &I18n) -> String {
    if view.current_plies == 0 {
        return i18n.tr("calvar-no-line-in-progress");
    }
    let mut args = FluentArgs::new();
    args.set("line", view.current_san.as_str());
    i18n.tr_args("calvar-line-in-progress", &args)
}

/// "2026-10-27": the date (UTC) of a time in seconds since the Unix epoch.
/// Local time would need the time zone of the machine; the date is only a
/// label for the row.
pub fn date_text(seconds: u64) -> String {
    // Civil date from a day count (H. Hinnant's algorithm), days from
    // 1970-01-01; the count fits `i64` because `seconds / 86_400` does.
    let days = i64::try_from(seconds / 86_400).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// One row of the history.
pub fn history_text(row: &HistoryRow, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("date", date_text(row.when));
    args.set("score", i64::from(row.score));
    args.set("found", i64::from(row.found));
    args.set("good", i64::from(row.good));
    args.set("lines", i64::from(row.lines));
    i18n.tr_args("calvar-history-row", &args)
}

/// Where the position of the exercise came from, when it did not come from
/// the editor or the dialog (M8-6d, D2367).
pub fn origin_text(view: &CalvarView, i18n: &I18n) -> Option<String> {
    let mut args = FluentArgs::new();
    let key = match view.origin? {
        CalvarOrigin::Game(number) => {
            args.set("number", i64::from(number));
            "calvar-origin-game"
        }
        CalvarOrigin::Tactic => "calvar-origin-tactic",
        CalvarOrigin::Review(when) => {
            args.set("date", date_text(when));
            "calvar-origin-review"
        }
        _ => return None,
    };
    Some(i18n.tr_args(key, &args))
}

fn percent(x: f32) -> i64 {
    // A fraction of 0 to 1 as a whole percentage.
    let rounded = (x.clamp(0.0, 1.0) * 100.0).round();
    #[allow(clippy::cast_possible_truncation)]
    let whole = rounded as i64;
    whole
}

/// The counters and the last scores in one line.
pub fn stats_text(stats: &CalvarStats, i18n: &I18n) -> String {
    if stats.done == 0 {
        return i18n.tr("calvar-no-history");
    }
    let mut args = FluentArgs::new();
    args.set("done", i64::from(stats.done));
    args.set("mean", stats.mean_score().map_or(0, i64::from));
    args.set("best", i64::from(stats.best_score));
    args.set("found", i64::from(stats.found));
    args.set("good", i64::from(stats.good));
    i18n.tr_args("calvar-stats", &args)
}

/// "Last scores: 72, 80, 65".
pub fn recent_text(scores: &[u8], i18n: &I18n) -> Option<String> {
    if scores.is_empty() {
        return None;
    }
    let list: Vec<String> = scores.iter().map(u8::to_string).collect();
    let mut args = FluentArgs::new();
    args.set("scores", list.join(", "));
    Some(i18n.tr_args("calvar-recent", &args))
}

/// "Score: 72 out of 100".
pub fn score_text(report: &Report, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("score", i64::from(report.score));
    i18n.tr_args("calvar-result-score", &args)
}

/// "Good moves found: 2 of 3".
pub fn found_text(report: &Report, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("found", i64::from(report.found));
    args.set("total", i64::from(report.good_total));
    i18n.tr_args("calvar-result-found", &args)
}

/// The crucial move that was missed, or the sentence that none was.
pub fn crucial_text(report: &Report, i18n: &I18n) -> Option<String> {
    let crucial = report.good.iter().find(|g| g.crucial)?;
    let mut args = FluentArgs::new();
    args.set("move", crucial.san.as_str());
    args.set("eval", crucial.eval.label());
    Some(i18n.tr_args("calvar-result-crucial", &args))
}

/// The other good moves that were not considered.
pub fn missed_text(report: &Report, i18n: &I18n) -> Option<String> {
    let others: Vec<&str> = report
        .good
        .iter()
        .filter(|g| !g.found && !g.crucial)
        .map(|g| g.san.as_str())
        .collect();
    if others.is_empty() {
        return None;
    }
    let mut args = FluentArgs::new();
    args.set("moves", others.join(", "));
    Some(i18n.tr_args("calvar-result-missed", &args))
}

/// The parts of the score.
pub fn parts_text(report: &Report, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("candidates", percent(report.parts.candidates));
    args.set("accuracy", percent(report.parts.accuracy));
    args.set("quality", percent(report.parts.quality));
    args.set("penalty", percent(report.parts.penalty / 100.0));
    i18n.tr_args("calvar-result-parts", &args)
}

/// One line of the result: what the user said and what the engine says.
pub fn line_result_text(line: &LineReport, i18n: &I18n) -> String {
    let mut args = FluentArgs::new();
    args.set("line", line.san.as_str());
    args.set("yours", line.assessment.symbol());
    let (Some(class), Some(eval)) = (line.engine_assessment, line.end.or(line.first)) else {
        return i18n.tr_args("calvar-result-line-unknown", &args);
    };
    args.set("engine", class.symbol());
    args.set("eval", eval.label());
    args.set("plies", i64::try_from(line.agree_plies).unwrap_or(i64::MAX));
    let mut verdict = i18n.tr(if line.sound {
        "calvar-verdict-sound"
    } else {
        "calvar-verdict-unsound"
    });
    if !line.good_candidate {
        verdict.push(' ');
        verdict.push_str(&i18n.tr("calvar-verdict-weak"));
    }
    args.set("verdict", verdict);
    i18n.tr_args("calvar-result-line", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_civil_dates_in_utc() {
        assert_eq!(date_text(0), "1970-01-01");
        assert_eq!(date_text(951_782_400), "2000-02-29");
        assert_eq!(date_text(1_700_000_000), "2023-11-14");
        assert!(date_text(u64::MAX).len() > 9);
    }

    #[test]
    fn the_clock_has_minutes_and_two_digit_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(75), "1:15");
    }

    #[test]
    fn a_fraction_becomes_a_whole_percentage() {
        assert_eq!(percent(0.5), 50);
        assert_eq!(percent(1.7), 100);
        assert_eq!(percent(-1.0), 0);
        assert_eq!(percent(f32::NAN), 0);
    }
}
