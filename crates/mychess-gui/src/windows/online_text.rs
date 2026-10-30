//! The texts of the online play: remarks, errors and the end of a game
//! (spec M8-3c, D1328, D1473). The application says *what* happened with
//! variants; the words are Fluent keys of `online.ftl`.

use fluent_bundle::FluentArgs;
use mychess_app::online::{EndKind, OnlineError, OnlineNotice, OnlineOutcome};
use mychess_core::Color;

use crate::i18n::I18n;

/// The reasons a challenge can be declined with (the `reason` the server
/// reports, spec D1313).
const DECLINE_REASONS: [&str; 11] = [
    "generic",
    "later",
    "tooFast",
    "tooSlow",
    "timeControl",
    "rated",
    "casual",
    "standard",
    "variant",
    "noBot",
    "onlyBot",
];

/// The Fluent key of a decline reason, if it is a known one.
fn decline_key(reason: &str) -> Option<&'static str> {
    let at = DECLINE_REASONS.iter().position(|r| *r == reason)?;
    Some(match at {
        0 => "online-decline-generic",
        1 => "online-decline-later",
        2 => "online-decline-too-fast",
        3 => "online-decline-too-slow",
        4 => "online-decline-time-control",
        5 => "online-decline-rated",
        6 => "online-decline-casual",
        7 => "online-decline-standard",
        8 => "online-decline-variant",
        9 => "online-decline-no-bot",
        _ => "online-decline-only-bot",
    })
}

/// The text of a failure; the reason the server gave is shown as it came
/// (it is short, and never carries the token).
pub fn error_text(i18n: &I18n, error: &OnlineError) -> String {
    let mut args = FluentArgs::new();
    match error {
        OnlineError::RateLimited { secs } => {
            args.set("secs", i64::try_from(*secs).unwrap_or(i64::MAX));
        }
        OnlineError::Rejected { reason } => args.set("reason", reason.as_str()),
        _ => {}
    }
    i18n.tr_args(error.message_key(), &args)
}

/// The text of a remark of the online play for the status bar.
pub fn notice_text(i18n: &I18n, notice: &OnlineNotice) -> String {
    match notice {
        OnlineNotice::Error(error) => error_text(i18n, error),
        OnlineNotice::MoveRejected { reason } => {
            let mut args = FluentArgs::new();
            args.set("reason", reason.as_str());
            i18n.tr_args(notice.message_key(), &args)
        }
        OnlineNotice::ChallengeDeclined {
            reason: Some(reason),
        } => match decline_key(reason) {
            Some(key) => {
                let mut args = FluentArgs::new();
                args.set("reason", i18n.tr(key));
                i18n.tr_args("online-notice-challenge-declined-reason", &args)
            }
            None => i18n.tr(notice.message_key()),
        },
        other => i18n.tr(other.message_key()),
    }
}

/// Whether the remark is a failure the open window should show (the status
/// bar is behind its scrim).
pub fn is_failure(notice: &OnlineNotice) -> bool {
    matches!(
        notice,
        OnlineNotice::Error(_)
            | OnlineNotice::Rejected(_)
            | OnlineNotice::ChallengeDeclined { .. }
            | OnlineNotice::MoveRejected { .. }
    )
}

/// The Fluent key of how a game ended.
pub fn end_kind_key(kind: EndKind) -> &'static str {
    match kind {
        EndKind::Mate => "online-end-mate",
        EndKind::Resign => "online-end-resign",
        EndKind::Stalemate => "online-end-stalemate",
        EndKind::Timeout => "online-end-timeout",
        EndKind::Draw => "online-end-draw",
        EndKind::InsufficientMaterial => "online-end-insufficient",
        EndKind::Cheat => "online-end-cheat",
        EndKind::VariantEnd => "online-end-variant",
        EndKind::Aborted => "online-end-aborted",
        _ => "online-end-unknown",
    }
}

/// The line that says how the game ended for the user, for example
/// "You won. By checkmate."
pub fn outcome_text(i18n: &I18n, outcome: OnlineOutcome, me: Color) -> String {
    let who = if outcome.kind == EndKind::Aborted {
        "online-end-no-result"
    } else {
        crate::windows::play_panel::outcome_key(outcome.result, me)
    };
    let mut args = FluentArgs::new();
    args.set("outcome", i18n.tr(who));
    args.set("reason", i18n.tr(end_kind_key(outcome.kind)));
    i18n.tr_args("online-end-line", &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn a_known_decline_reason_is_translated_and_an_unknown_one_is_not_shown() {
        let en = I18n::new(Language::EnUs);
        let known = notice_text(
            &en,
            &OnlineNotice::ChallengeDeclined {
                reason: Some("later".to_owned()),
            },
        );
        assert!(
            known.contains("later") || known.contains("Later"),
            "{known}"
        );
        let unknown = notice_text(
            &en,
            &OnlineNotice::ChallengeDeclined {
                reason: Some("<script>".to_owned()),
            },
        );
        assert!(!unknown.contains("script"), "{unknown}");
    }

    #[test]
    fn the_rate_limit_names_the_seconds() {
        let en = I18n::new(Language::EnUs);
        let text = error_text(&en, &OnlineError::RateLimited { secs: 42 });
        assert!(text.contains("42"), "{text}");
    }
}
