//! The translated words of the maintenance window: the remarks of the
//! application, the flags, the types of database and the criteria of the sort
//! (spec M7-1c, D4100-D4119).

use fluent_bundle::FluentArgs;
use mychess_app::maintenance::{
    ConfirmKind, MaintFlag, MaintNotice, SortCriterion, WeakCriteria,
    data::{CheckCause, CleanerStep, StepStatus},
};

use crate::i18n::I18n;

/// The flags of the "Mark" menu, in the order of the Scid (`W B M E N P T Q K
/// ! ? U 1 2 3 4 5 6`); the delete flag has its own group of buttons.
pub const MARK_FLAGS: [MaintFlag; 18] = [
    MaintFlag::WhiteOpening,
    MaintFlag::BlackOpening,
    MaintFlag::Middlegame,
    MaintFlag::Endgame,
    MaintFlag::Novelty,
    MaintFlag::PawnStructure,
    MaintFlag::Tactics,
    MaintFlag::Kingside,
    MaintFlag::Queenside,
    MaintFlag::Brilliancy,
    MaintFlag::Blunder,
    MaintFlag::User,
    MaintFlag::Custom(1),
    MaintFlag::Custom(2),
    MaintFlag::Custom(3),
    MaintFlag::Custom(4),
    MaintFlag::Custom(5),
    MaintFlag::Custom(6),
];

/// The letter (or digit) the Scid shows for a flag.
pub fn flag_letter(flag: MaintFlag) -> String {
    match flag {
        MaintFlag::Delete => "D".to_owned(),
        MaintFlag::WhiteOpening => "W".to_owned(),
        MaintFlag::BlackOpening => "B".to_owned(),
        MaintFlag::Middlegame => "M".to_owned(),
        MaintFlag::Endgame => "E".to_owned(),
        MaintFlag::Novelty => "N".to_owned(),
        MaintFlag::PawnStructure => "P".to_owned(),
        MaintFlag::Tactics => "T".to_owned(),
        MaintFlag::Kingside => "K".to_owned(),
        MaintFlag::Queenside => "Q".to_owned(),
        MaintFlag::Brilliancy => "!".to_owned(),
        MaintFlag::Blunder => "?".to_owned(),
        MaintFlag::User => "U".to_owned(),
        MaintFlag::Custom(n) => n.to_string(),
    }
}

/// The entry of the "Mark" menu: `W: White opening`, and for a custom flag
/// `description (n)` or `Custom n`.
pub fn flag_label(i18n: &I18n, flag: MaintFlag, custom: &[String; 6]) -> String {
    if let MaintFlag::Custom(n) = flag {
        let description = usize::from(n)
            .checked_sub(1)
            .and_then(|i| custom.get(i))
            .map(String::as_str)
            .filter(|d| !d.is_empty());
        let mut args = FluentArgs::new();
        args.set("number", i64::from(n));
        return match description {
            Some(d) => {
                args.set("description", d.to_owned());
                i18n.tr_args("maint-flag-custom-described", &args)
            }
            None => i18n.tr_args("maint-flag-custom", &args),
        };
    }
    let key = match flag {
        MaintFlag::Delete => "maint-flag-delete",
        MaintFlag::WhiteOpening => "maint-flag-white-opening",
        MaintFlag::BlackOpening => "maint-flag-black-opening",
        MaintFlag::Middlegame => "maint-flag-middlegame",
        MaintFlag::Endgame => "maint-flag-endgame",
        MaintFlag::Novelty => "maint-flag-novelty",
        MaintFlag::PawnStructure => "maint-flag-pawn-structure",
        MaintFlag::Tactics => "maint-flag-tactics",
        MaintFlag::Kingside => "maint-flag-kingside",
        MaintFlag::Queenside => "maint-flag-queenside",
        MaintFlag::Brilliancy => "maint-flag-brilliancy",
        MaintFlag::Blunder => "maint-flag-blunder",
        MaintFlag::User => "maint-flag-user",
        MaintFlag::Custom(_) => "maint-flag-custom",
    };
    let mut args = FluentArgs::new();
    args.set("letter", flag_letter(flag));
    args.set("name", i18n.tr(key));
    i18n.tr_args("maint-flag-entry", &args)
}

/// The name of a database type (`0..=31`), or the number for any other.
pub fn base_type_label(i18n: &I18n, base_type: u32) -> String {
    if base_type < 32 {
        i18n.tr(&format!("maint-type-{base_type}"))
    } else {
        base_type.to_string()
    }
}

/// The name of a criterion of the sort.
pub fn criterion_label(i18n: &I18n, criterion: SortCriterion) -> String {
    i18n.tr(&format!("maint-sort-key-{}", criterion.name()))
}

/// The text of a step of the Cleaner.
pub fn step_key(step: CleanerStep) -> &'static str {
    match step {
        CleanerStep::Players => "maint-cleaner-step-players",
        CleanerStep::Events => "maint-cleaner-step-events",
        CleanerStep::Sites => "maint-cleaner-step-sites",
        CleanerStep::Rounds => "maint-cleaner-step-rounds",
        CleanerStep::Eco => "maint-cleaner-step-eco",
        CleanerStep::Elo => "maint-cleaner-step-elo",
        CleanerStep::Twins => "maint-cleaner-step-twins",
        CleanerStep::CompactNames => "maint-cleaner-step-compact-names",
        CleanerStep::CompactGames => "maint-cleaner-step-compact-games",
        CleanerStep::Tree => "maint-cleaner-step-tree",
        // `CleanerStep` may grow: an unknown step is shown by its position.
        _ => "maint-cleaner-step-unknown",
    }
}

/// The text of how a step ended.
pub fn status_key(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Skipped => "maint-cleaner-status-skipped",
        StepStatus::Done => "maint-cleaner-status-done",
        StepStatus::Interrupted => "maint-cleaner-status-interrupted",
        // `Failed`, and any status added later, read as a failure.
        _ => "maint-cleaner-status-failed",
    }
}

/// The text of the cause of a problem found by "Check games".
pub fn cause_key(cause: CheckCause) -> &'static str {
    match cause {
        CheckCause::Index => "maint-check-cause-index",
        CheckCause::Record => "maint-check-cause-record",
        CheckCause::Decode => "maint-check-cause-decode",
        CheckCause::Names => "maint-check-cause-names",
        CheckCause::IllegalMove => "maint-check-cause-illegal-move",
    }
}

/// The question of a confirmation (the weak-criteria warnings come apart).
pub fn confirm_key(kind: ConfirmKind) -> &'static str {
    match kind {
        ConfirmKind::MarkTwins => "maint-confirm-mark-twins",
        ConfirmKind::UndeleteFirst => "maint-confirm-undelete-first",
        ConfirmKind::UndeleteAll => "maint-confirm-undelete-all",
        ConfirmKind::Compact => "maint-confirm-compact",
        ConfirmKind::Sort => "maint-confirm-sort",
        ConfirmKind::Strip => "maint-confirm-strip",
        ConfirmKind::TagAdd => "maint-confirm-tag-add",
        ConfirmKind::TagStrip => "maint-confirm-tag-strip",
        ConfirmKind::Names => "maint-confirm-names",
        ConfirmKind::Repair => "maint-confirm-repair",
        ConfirmKind::Cleaner => "maint-confirm-cleaner",
        ConfirmKind::ShareTags => "maint-confirm-share-tags",
        ConfirmKind::WeakTwinCriteria => "maint-confirm-weak-twins",
    }
}

/// The text of a weak-criteria warning.
pub fn weak_key(weak: WeakCriteria) -> &'static str {
    match weak {
        WeakCriteria::NoMoves => "maint-weak-no-moves",
        WeakCriteria::FewFields => "maint-weak-few-fields",
    }
}

/// The text of a remark of the maintenance (the numbers reach the text as
/// arguments of the same names the files use).
pub fn notice_text(i18n: &I18n, notice: &MaintNotice) -> String {
    let mut args = FluentArgs::new();
    let mut number = |name: &'static str, value: u32| args.set(name, i64::from(value));
    match notice {
        MaintNotice::FlagsDone { changed } => number("changed", *changed),
        MaintNotice::TwinsFound { marked, protected } => {
            number("marked", *marked);
            number("protected", *protected);
        }
        MaintNotice::TwinsMarked { marked, .. } => number("marked", *marked),
        MaintNotice::CheckClean { examined } => number("examined", *examined),
        MaintNotice::CheckProblems { problems } => number("problems", *problems),
        MaintNotice::Compacted {
            games_removed,
            names_removed,
        } => {
            number("games_removed", *games_removed);
            number("names_removed", *names_removed);
        }
        MaintNotice::Sorted { moved } => number("moved", *moved),
        MaintNotice::FilterWithoutDeleted { removed } => number("removed", *removed),
        MaintNotice::Stripped { changed, skipped } | MaintNotice::TagsDone { changed, skipped } => {
            number("changed", *changed);
            number("skipped", *skipped);
        }
        MaintNotice::TagFiltered { matched } => number("matched", *matched),
        MaintNotice::NamesDone { games, edits } => {
            number("games", *games);
            number("edits", *edits);
        }
        MaintNotice::Repaired { games, .. } => number("games", *games),
        MaintNotice::CleanerDone { failed } => number("failed", *failed),
        MaintNotice::TagsShared { count } | MaintNotice::BackupsKept { count } => {
            number("count", *count);
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn a_custom_flag_shows_its_description_or_its_number() {
        let en = I18n::new(Language::EnUs);
        let mut custom: [String; 6] = Default::default();
        assert_eq!(flag_label(&en, MaintFlag::Custom(2), &custom), "Custom 2");
        custom[1] = "Mine".to_owned();
        assert_eq!(flag_label(&en, MaintFlag::Custom(2), &custom), "Mine (2)");
        assert_eq!(flag_letter(MaintFlag::Blunder), "?");
    }

    #[test]
    fn the_numbers_of_a_notice_reach_the_text_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            let text = notice_text(
                &i18n,
                &MaintNotice::Compacted {
                    games_removed: 7,
                    names_removed: 3,
                },
            );
            assert!(text.contains('7') && text.contains('3'), "{text}");
            let text = notice_text(&i18n, &MaintNotice::FlagsDone { changed: 1 });
            assert!(text.contains('1'), "{text}");
        }
    }

    #[test]
    fn every_type_of_database_has_a_name() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for n in 0..32 {
                assert!(i18n.has(&format!("maint-type-{n}")), "{n} {language:?}");
            }
            assert_eq!(base_type_label(&i18n, 99), "99");
        }
    }
}
