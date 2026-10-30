//! M7-1c (GUI, spec M7-1): the maintenance window shows the header and the
//! statistics of the database, follows the enabling table, runs the
//! operations through the application, takes the keys and is a modal layer.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

mod m7_1c_common;

use iced::widget::Id;
use m7_1c_common::*;

fn id(name: &'static str) -> Id {
    Id::new(name)
}

#[test]
fn opening_the_window_counts_the_database_and_shows_the_header() {
    let mut m = Maint::six();
    assert!(!m.is_open());
    m.open();
    assert!(m.is_open());
    assert!(m.shell().modal_layer_open(), "the window is a modal layer");
    let view = m.app().maintenance_view();
    assert_eq!(view.games, 6);
    assert!(view.stats.is_some(), "the statistics were counted");
    let mut sim = m.sim();
    for name in ["maint-window", "maint-content", "maint-close"] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(sim.find("Database maintenance").is_ok());
    assert!(sim.find("Games:").is_ok());
    assert!(sim.find("Deleted:").is_ok());
    assert!(sim.find("Years:").is_ok());
    assert!(sim.find("base.si4").is_ok());
}

#[test]
fn escape_closes_the_form_first_and_then_the_window() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Sort));
    assert_eq!(m.form(), Some(MaintForm::Sort));
    m.key(Key::Named(NamedKey::Escape));
    assert_eq!(m.form(), None);
    assert!(m.is_open(), "the first Esc closes only the card");
    m.key(Key::Named(NamedKey::Escape));
    assert!(!m.is_open());
    assert!(!m.shell().modal_layer_open());
}

#[test]
fn the_close_button_closes_the_window() {
    let mut m = Maint::six();
    m.open();
    let mut sim = m.sim();
    sim.click(id("maint-close")).expect("button");
    let sent: Vec<Message> = sim.into_messages().collect();
    for message in sent {
        m.rig.msg(message);
    }
    assert!(!m.is_open());
}

#[test]
fn a_pgn_file_cannot_be_edited_so_its_operations_are_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_pgn(dir.path(), "x.pgn", &six_pgn());
    rig.msg(Message::Maint(MaintUiMessage::Open));
    let view = rig.rig.shell.app.maintenance_view();
    assert!(!view.access.flags && !view.access.sort && !view.access.compact);
    assert!(view.access.check, "checking only reads");
    let element = mychess_gui::windows::main_window::view(&rig.rig.shell);
    let mut sim = Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1400.0, 1000.0),
        element,
    );
    // The button of the sort is drawn but does nothing.
    sim.click("Sort database").expect("the button is there");
    assert_eq!(sim.into_messages().count(), 0);
}

#[test]
fn delete_all_marks_every_game_and_the_counts_follow() {
    let mut m = Maint::six();
    m.open();
    assert_eq!(m.deleted(), 0);
    m.ui(MaintUiMessage::Run(MaintRequest::Flags {
        scope: FlagScope::All,
        flag: MaintFlag::Delete,
        action: FlagAction::Set,
    }));
    assert_eq!(m.deleted(), 6);
    assert!(matches!(
        m.shell().ui.status_notice.as_ref(),
        Some(mychess_app::AppNotice::Maintenance(
            MaintNotice::FlagsDone { changed: 6 }
        ))
    ));
    m.ui(MaintUiMessage::Run(MaintRequest::Flags {
        scope: FlagScope::All,
        flag: MaintFlag::Delete,
        action: FlagAction::Clear,
    }));
    assert_eq!(m.deleted(), 0);
}

#[test]
fn the_compact_card_lists_the_names_and_offers_the_games_after_a_delete() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::Run(MaintRequest::Flags {
        scope: FlagScope::All,
        flag: MaintFlag::Delete,
        action: FlagAction::Set,
    }));
    m.ui(MaintUiMessage::OpenForm(MaintForm::Compact));
    let mut sim = m.sim();
    assert!(sim.find(id("maint-form-box")).is_ok());
    assert!(sim.find("Name file").is_ok());
    assert!(sim.find("Game file").is_ok());
    assert!(sim.find("Compact the game file").is_ok());
    // Compacting asks first: the games are deleted for good.
    sim.click("Compact the game file").expect("button");
    for message in sim.into_messages().collect::<Vec<_>>() {
        m.rig.msg(message);
    }
    let view = m.app().maintenance_view();
    let confirm = view.confirm.expect("a confirmation");
    assert_eq!(confirm.count, 6);
    assert!(m.sim().find(id("maint-confirm-box")).is_ok());
    // Esc answers "no": nothing was compacted.
    m.key(Key::Named(NamedKey::Escape));
    assert!(m.app().maintenance_view().confirm.is_none());
    assert_eq!(m.app().maintenance_view().games, 6);
}

#[test]
fn the_sort_card_builds_the_criteria_and_sorting_rewrites_the_index() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Sort));
    m.ui(MaintUiMessage::Edit(MaintEdit::SortClear));
    m.ui(MaintUiMessage::Edit(MaintEdit::SortAdd(
        SortCriterion::Year,
    )));
    assert_eq!(m.shell().ui.dialogs.maint.sort_keys, [SortCriterion::Year]);
    // The sixteenth criterion is refused: fifteen at most.
    for _ in 0..20 {
        m.ui(MaintUiMessage::Edit(MaintEdit::SortAdd(
            SortCriterion::Month,
        )));
    }
    assert_eq!(m.shell().ui.dialogs.maint.sort_keys.len(), 15);
    m.ui(MaintUiMessage::Edit(MaintEdit::SortClear));
    m.ui(MaintUiMessage::Edit(MaintEdit::SortAdd(
        SortCriterion::Year,
    )));
    // Enter is the default button: it asks to confirm, then sorts.
    m.key(Key::Named(NamedKey::Enter));
    assert_eq!(m.form(), None, "the card closes when the sort starts");
    assert!(m.app().maintenance_view().confirm.is_some());
    // Enter on the destructive confirmation answers No (D4122) ...
    m.key(Key::Named(NamedKey::Enter));
    assert!(m.app().maintenance_view().confirm.is_none());
    // ... so the sort is asked again and confirmed with the button.
    m.ui(MaintUiMessage::OpenForm(MaintForm::Sort));
    m.key(Key::Named(NamedKey::Enter));
    assert!(m.app().maintenance_view().confirm.is_some());
    m.rig.msg(Message::Maint(MaintUiMessage::App(
        MaintenanceMessage::Confirm(true),
    )));
    assert!(m.app().maintenance_view().confirm.is_none());
    let years: Vec<String> = {
        let mut db = mychess_db::si4::Si4Database::open(&m.path).unwrap();
        (1..=db.len())
            .map(|n| db.game(n).unwrap().tags().date.to_string())
            .collect()
    };
    let mut sorted = years.clone();
    sorted.sort();
    assert_eq!(years, sorted, "the index is in year order");
}

#[test]
fn the_common_sorts_are_the_three_of_the_scid() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Sort));
    m.ui(MaintUiMessage::Edit(MaintEdit::SortCommon(2)));
    assert_eq!(
        m.shell().ui.dialogs.maint.sort_keys,
        [SortCriterion::Eco, SortCriterion::Rating]
    );
    assert_eq!(common_sort(0).len(), 6);
}

#[test]
fn the_flag_choice_of_the_mark_group_is_kept() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::SelectFlag(MaintFlag::Custom(3)));
    assert_eq!(m.shell().ui.dialogs.maint.flag, MaintFlag::Custom(3));
    m.ui(MaintUiMessage::Run(MaintRequest::Flags {
        scope: FlagScope::All,
        flag: MaintFlag::Custom(3),
        action: FlagAction::Set,
    }));
    let stats = m.app().maintenance_view().stats.unwrap();
    let index = MaintFlag::Custom(3).counter().unwrap();
    assert_eq!(stats.flags[index], 6);
}

#[test]
fn the_autoload_form_applies_the_typed_number_and_refuses_text() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Autoload));
    m.ui(MaintUiMessage::Edit(MaintEdit::Autoload("3".to_owned())));
    m.key(Key::Named(NamedKey::Enter));
    assert_eq!(
        m.app().maintenance_view().header.unwrap().auto_load,
        Some(3)
    );
    m.ui(MaintUiMessage::Edit(MaintEdit::Autoload("abc".to_owned())));
    m.key(Key::Named(NamedKey::Enter));
    assert!(matches!(
        m.shell().ui.status_notice.as_ref(),
        Some(mychess_app::AppNotice::Maintenance(
            MaintNotice::InvalidValue
        ))
    ));
    assert_eq!(
        m.app().maintenance_view().header.unwrap().auto_load,
        Some(3)
    );
}

#[test]
fn the_checker_opens_after_marking_and_its_keys_flip_the_delete_flag() {
    let mut m = Maint::with(&twins_pgn());
    m.open();
    let mut prefs = m.app().maintenance_view().prefs;
    prefs.twins.skip_short = false;
    m.ui(MaintUiMessage::Prefs(Box::new(prefs)));
    m.ui(MaintUiMessage::OpenForm(MaintForm::Twins));
    m.key(Key::Named(NamedKey::Enter));
    // The marking asks first; the button confirms.
    if m.app().maintenance_view().confirm.is_some() {
        m.rig.msg(Message::Maint(MaintUiMessage::App(
            MaintenanceMessage::Confirm(true),
        )));
    }
    let view = m.app().maintenance_view();
    assert!(view.twin_pairs > 0, "a pair was found");
    assert!(view.checker_open, "the checker opens by itself");
    assert!(m.sim().find(id("maint-checker-box")).is_ok());
    let pair = m.app().twin_pair_view().expect("a pair");
    let before = (pair.left_deleted, pair.right_deleted);
    m.key(Key::Char('1'));
    let pair = m.app().twin_pair_view().expect("a pair");
    assert_eq!(pair.left_deleted, !before.0, "1 flips the left game");
    assert_eq!(pair.right_deleted, before.1);
    m.key(Key::Named(NamedKey::Escape));
    assert!(!m.app().maintenance_view().checker_open);
    assert!(m.is_open(), "Esc closed only the checker");
}

#[test]
fn the_description_and_the_type_are_written_to_the_header() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Description));
    m.ui(MaintUiMessage::Edit(MaintEdit::Description(
        "My games".to_owned(),
    )));
    m.key(Key::Named(NamedKey::Enter));
    assert_eq!(m.form(), None);
    assert_eq!(
        m.app().maintenance_view().header.unwrap().description,
        "My games"
    );
    m.ui(MaintUiMessage::OpenForm(MaintForm::BaseType));
    let mut sim = m.sim();
    sim.click("Large database").expect("a type");
    for message in sim.into_messages().collect::<Vec<_>>() {
        m.rig.msg(message);
    }
    assert_eq!(m.app().maintenance_view().header.unwrap().base_type, 5);
}

#[test]
fn the_tags_form_lists_extra_tags_after_adding_one() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Tags));
    let mut prefs = m.app().maintenance_view().prefs;
    prefs.tag_scope = ScopeChoice::All;
    m.ui(MaintUiMessage::Prefs(Box::new(prefs)));
    m.ui(MaintUiMessage::Edit(MaintEdit::TagName(
        "Source".to_owned(),
    )));
    m.ui(MaintUiMessage::Edit(MaintEdit::TagValue("x".to_owned())));
    m.key(Key::Named(NamedKey::Enter));
    if m.app().maintenance_view().confirm.is_some() {
        m.rig.msg(Message::Maint(MaintUiMessage::App(
            MaintenanceMessage::Confirm(true),
        )));
    }
    m.ui(MaintUiMessage::Run(MaintRequest::Tags(
        mychess_app::maintenance::TagRequest::List,
    )));
    let listing = m
        .app()
        .maintenance()
        .tag_list()
        .expect("a list")
        .listing
        .clone();
    assert_eq!(listing.tags.len(), 1);
    assert_eq!(listing.tags[0].name, "Source");
    assert_eq!(listing.tags[0].games, 6);
    assert!(m.sim().find("Source  (6)").is_ok());
}

#[test]
fn a_name_replacement_needs_both_names_before_it_can_run() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Names));
    // Enter with empty fields does nothing.
    m.key(Key::Named(NamedKey::Enter));
    assert_eq!(m.form(), Some(MaintForm::Names));
    m.ui(MaintUiMessage::Edit(MaintEdit::NamesOld(
        "Delta, D".to_owned(),
    )));
    m.ui(MaintUiMessage::Edit(MaintEdit::NamesNew(
        "Delta, Daniel".to_owned(),
    )));
    m.key(Key::Named(NamedKey::Enter));
    if m.app().maintenance_view().confirm.is_some() {
        m.rig.msg(Message::Maint(MaintUiMessage::App(
            MaintenanceMessage::Confirm(true),
        )));
    }
    assert_eq!(m.form(), None);
    let names: Vec<String> = {
        let mut db = mychess_db::si4::Si4Database::open(&m.path).unwrap();
        (1..=db.len())
            .map(|n| db.game(n).unwrap().tags().white.clone())
            .collect()
    };
    assert!(names.contains(&"Delta, Daniel".to_owned()), "{names:?}");
    assert!(!names.contains(&"Delta, D".to_owned()));
}

#[test]
fn check_games_reports_a_clean_database() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Check));
    m.ui(MaintUiMessage::Run(MaintRequest::Check {
        scope: ScopeChoice::All,
    }));
    let report = m
        .app()
        .maintenance()
        .check_report()
        .expect("a report")
        .clone();
    assert_eq!(report.problems, 0);
    assert_eq!(report.examined, 6);
    assert!(m.sim().find("6 games examined, 0 with a problem").is_ok());
}
