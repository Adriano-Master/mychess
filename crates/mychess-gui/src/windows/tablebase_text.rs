//! The texts of the tablebase panel (spec M8-8, D2580): everything is
//! formatted here from the view of the application.

use fluent_bundle::FluentArgs;
use mychess_app::tablebase::{EndReason, Grade, IssueView, TbCategory, TrainingOutcome, Verdict};
use mychess_core::Color;

use crate::i18n::I18n;

/// The word of a result.
pub fn result_word(i18n: &I18n, category: TbCategory) -> String {
    i18n.tr(match category {
        TbCategory::Win => "tablebase-result-win",
        TbCategory::MaybeWin => "tablebase-result-maybe-win",
        TbCategory::CursedWin => "tablebase-result-cursed-win",
        TbCategory::Draw => "tablebase-result-draw",
        TbCategory::BlessedLoss => "tablebase-result-blessed-loss",
        TbCategory::MaybeLoss => "tablebase-result-maybe-loss",
        TbCategory::Loss => "tablebase-result-loss",
        _ => "tablebase-result-unknown",
    })
}

/// The explanation of the results the 50-move rule changes.
pub fn result_tip(i18n: &I18n, category: TbCategory) -> Option<String> {
    let key = match category {
        TbCategory::CursedWin => "tablebase-result-cursed-win-tip",
        TbCategory::BlessedLoss => "tablebase-result-blessed-loss-tip",
        TbCategory::MaybeWin | TbCategory::MaybeLoss => "tablebase-result-maybe-tip",
        _ => return None,
    };
    Some(i18n.tr(key))
}

/// A distance to zeroing: signed, with `~` when it may be one ply off.
pub fn dtz_text(dtz: Option<i32>, rounded: bool) -> String {
    match dtz {
        None => "-".to_owned(),
        Some(n) => {
            let sign = if n > 0 { "+" } else { "" };
            let tilde = if rounded { "~" } else { "" };
            format!("{tilde}{sign}{n}")
        }
    }
}

/// A distance to mate in plies as "mate in N moves" (`#N`).
pub fn dtm_text(i18n: &I18n, dtm: Option<i32>) -> String {
    match dtm {
        None => "-".to_owned(),
        Some(plies) => {
            let moves = plies.unsigned_abs().div_ceil(2);
            let mut args = FluentArgs::new();
            args.set("moves", i64::from(moves));
            let text = i18n.tr_args("tablebase-mate-in", &args);
            if plies < 0 { format!("-{text}") } else { text }
        }
    }
}

/// The warning line.
pub fn issue_text(i18n: &I18n, issue: &IssueView) -> String {
    let mut args = FluentArgs::new();
    match issue {
        IssueView::TooManyPieces { pieces, max } => {
            args.set("pieces", i64::from(*pieces));
            args.set("max", i64::from(*max));
        }
        IssueView::MissingTable { material } | IssueView::Corrupt { material } => {
            args.set("material", material.as_str());
        }
        IssueView::RateLimited { seconds } => {
            args.set("seconds", i64::try_from(*seconds).unwrap_or(i64::MAX));
        }
        _ => {}
    }
    i18n.tr_args(issue.message_key(), &args)
}

/// The side, for the training.
pub fn side_text(i18n: &I18n, side: Color) -> String {
    i18n.tr(match side {
        Color::White => "tablebase-side-white",
        Color::Black => "tablebase-side-black",
    })
}

/// A grade of the user: "a win", "a draw", "a loss".
pub fn grade_text(i18n: &I18n, grade: Grade) -> String {
    i18n.tr(match grade {
        Grade::Win => "tablebase-grade-win",
        Grade::Draw => "tablebase-grade-draw",
        Grade::Loss => "tablebase-grade-loss",
    })
}

/// The verdict of a training.
pub fn verdict_text(i18n: &I18n, verdict: Verdict) -> String {
    i18n.tr(match verdict {
        Verdict::Converted => "tablebase-verdict-converted",
        Verdict::Held => "tablebase-verdict-held",
        Verdict::Escaped => "tablebase-verdict-escaped",
        Verdict::LostAsExpected => "tablebase-verdict-lost-as-expected",
        Verdict::Failed => "tablebase-verdict-failed",
        _ => "tablebase-verdict-inconclusive",
    })
}

/// How the training ended.
pub fn reason_text(i18n: &I18n, reason: EndReason) -> String {
    i18n.tr(match reason {
        EndReason::Checkmate => "tablebase-end-checkmate",
        EndReason::Stalemate => "tablebase-end-stalemate",
        EndReason::InsufficientMaterial => "tablebase-end-insufficient-material",
        EndReason::FiftyMoves => "tablebase-end-fifty-moves",
        EndReason::Repetition => "tablebase-end-repetition",
        _ => "tablebase-end-out-of-range",
    })
}

/// The line under the verdict: moves and slips.
pub fn summary_text(i18n: &I18n, outcome: &TrainingOutcome) -> String {
    let mut args = FluentArgs::new();
    args.set("moves", i64::from(outcome.user_moves));
    args.set("slips", i64::from(outcome.slips));
    i18n.tr_args("tablebase-outcome-summary", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_are_signed_and_marked_when_rounded() {
        assert_eq!(dtz_text(None, false), "-");
        assert_eq!(dtz_text(Some(23), false), "+23");
        assert_eq!(dtz_text(Some(-4), false), "-4");
        assert_eq!(dtz_text(Some(0), false), "0");
        assert_eq!(dtz_text(Some(100), true), "~+100");
    }
}
