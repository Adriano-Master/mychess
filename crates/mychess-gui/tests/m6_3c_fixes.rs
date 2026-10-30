//! M6-3c review fixes (GUI): the keys and the timer read cheap flags, the
//! rows of the name checker are shared and rebuilt only when they change,
//! the cursor never stands on a row that is gone, the messages use real
//! arguments and the decimals follow the language.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_3c_common;

use m6_3c_common::*;
use mychess_app::{FileErrorKind, NameKind, SpellingLoadError, SpellingMessage, SpellingNotice};
use mychess_gui::windows::player_tools::text::spelling_notice_text;

fn open_checker() -> Tools {
    let mut t = Tools::new();
    t.open_base(&misspelled());
    t.load_ssp();
    t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
    t
}

fn press(t: &mut Tools, key: NamedKey) {
    t.named(key);
}

#[test]
fn the_rows_are_shared_between_views_and_rebuilt_when_a_row_changes() {
    let mut t = open_checker();
    let first = t.app().spell_view().expect("open");
    assert!(first.total > 0, "the sample has misspelled names");
    let second = t.app().spell_view().expect("open");
    assert!(first.rows == second.rows, "same allocation between updates");
    let flags = t.app().spell_flags().expect("open");
    assert_eq!(flags.rows, first.rows.len());
    assert!(!flags.busy() && !flags.confirm);
    // A tick of the clock (an unrelated message) keeps the same rows.
    t.rig.app_msg(SpellingMessage::SetFind(String::new()));
    let third = t.app().spell_view().unwrap();
    assert_eq!(third.ticked, first.ticked);
    // Ticking a row builds a new list with the new count.
    let index = first.rows[0].index;
    let was = first.rows[0].row.selected;
    t.rig.app_msg(SpellingMessage::ToggleRow(index));
    let after = t.app().spell_view().unwrap();
    assert_ne!(after.rows, first.rows);
    assert_eq!(after.rows[0].row.selected, !was);
    // The search narrows the rows and the total stays.
    t.rig
        .app_msg(SpellingMessage::SetFind("zzzz-no-such".into()));
    let none = t.app().spell_view().unwrap();
    assert_eq!(none.rows.len(), 0);
    assert_eq!(none.total, first.total);
    assert_eq!(t.app().spell_flags().unwrap().rows, 0);
}

#[test]
fn the_add_elo_and_info_flags_are_cheap_and_follow_the_dialogs() {
    let mut t = Tools::new();
    assert!(t.app().add_elo_flags().is_none() && t.app().spell_flags().is_none());
    assert_eq!(t.app().player_info_progress(), None);
    t.open_base(&misspelled());
    t.load_ssp();
    t.rig.app_msg(SpellingMessage::OpenAddElo);
    let flags = t.app().add_elo_flags().expect("open");
    let view = t.app().add_elo_view().unwrap();
    assert_eq!(flags.planning, view.planning.is_some());
    assert_eq!(flags.planned, view.planned.is_some());
    assert_eq!(flags.writing, view.writing.is_some());
    assert_eq!(flags.has_result, view.result.is_some());
}

#[test]
fn a_cursor_past_the_end_is_clamped_by_the_arrows_and_a_new_list_drops_it() {
    let mut t = open_checker();
    let rows = t.app().spell_flags().unwrap().rows;
    assert!(rows > 0);
    t.rig.rig.shell.ui.dialogs.player_tools.spell.cursor = Some(rows + 50);
    press(&mut t, NamedKey::Down);
    assert_eq!(
        t.shell().ui.dialogs.player_tools.spell.cursor,
        Some(rows - 1),
        "Down clamps"
    );
    t.shell();
    // A search that changes the rows drops the cursor.
    t.rig
        .app_msg(SpellingMessage::SetFind("zzzz-no-such".into()));
    assert_eq!(t.shell().ui.dialogs.player_tools.spell.cursor, None);
}

#[test]
fn a_failed_load_names_the_limit_in_both_languages() {
    let too_large = SpellingNotice::LoadFailed(SpellingLoadError::File(FileErrorKind::TooLarge {
        limit: 300 * 1024 * 1024,
    }));
    for i18n in [en(), pt()] {
        let text = spelling_notice_text(&i18n, &too_large);
        assert!(text.contains("300"), "{text}");
        assert!(!text.contains(" 0 "), "{text}");
    }
}

#[test]
fn the_decimal_follows_the_language() {
    assert_eq!(en().decimal("87.50%"), "87.50%");
    assert_eq!(pt().decimal("87.50%"), "87,50%");
}
