//! M7-1c review fixes (GUI): the notice inside the window, the report of the
//! Cleaner in both languages, Esc that stops a job, F1 in the window.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m7_1c_common;

use m7_1c_common::*;
use mychess_app::maintenance::StripChoice;

fn cleaner_report_in(language: mychess_app::LanguagePref, expected: &str) {
    let mut m = Maint::six();
    m.rig.rig.shell.app.prefs.general.language = language;
    m.rig.rig.shell.i18n = mychess_gui::i18n::I18n::new(match language {
        mychess_app::LanguagePref::PtBr => mychess_gui::i18n::Language::PtBr,
        _ => mychess_gui::i18n::Language::EnUs,
    });
    m.open();
    m.ui(MaintUiMessage::OpenForm(MaintForm::Cleaner));
    m.ui(MaintUiMessage::Run(MaintRequest::Cleaner));
    if m.app().maintenance_view().confirm.is_some() {
        m.ui(MaintUiMessage::App(MaintenanceMessage::Confirm(true)));
    }
    let mut sim = m.sim();
    // The step "spelling" is skipped (no spelling file): its detail is a
    // translated sentence, never a Fluent key.
    assert!(
        sim.find("maint-cleaner-detail-no-spelling").is_err(),
        "no raw key on screen"
    );
    let Some(MaintReport::Cleaner(report)) = m.app().maintenance().last_report() else {
        panic!("the Cleaner left a report");
    };
    let record = report
        .records()
        .iter()
        .find(|r| r.detail == "maint-cleaner-detail-no-spelling")
        .expect("a step skipped for the missing spelling file");
    let i18n = &m.shell().i18n;
    let line = format!(
        "{}: {} ({expected})",
        i18n.tr(mychess_gui::windows::maintenance::text::step_key(
            record.step
        )),
        i18n.tr(mychess_gui::windows::maintenance::text::status_key(
            record.status
        )),
    );
    assert!(sim.find(line.as_str()).is_ok(), "{line}");
}

#[test]
fn the_cleaner_report_shows_translated_details_in_english() {
    cleaner_report_in(
        mychess_app::LanguagePref::EnUs,
        "No spelling file is loaded",
    );
}

#[test]
fn the_cleaner_report_shows_translated_details_in_portuguese() {
    cleaner_report_in(
        mychess_app::LanguagePref::PtBr,
        "Nenhum arquivo de ortografia carregado",
    );
}

#[test]
fn a_notice_of_the_maintenance_is_drawn_inside_the_window() {
    let mut m = Maint::six();
    m.open();
    m.ui(MaintUiMessage::SetReadOnly(true));
    m.ui(MaintUiMessage::Run(MaintRequest::Flags {
        scope: FlagScope::All,
        flag: MaintFlag::Delete,
        action: FlagAction::Set,
    }));
    let notice = match &m.shell().ui.status_notice {
        Some(mychess_app::AppNotice::Maintenance(n)) => n.clone(),
        other => panic!("a maintenance notice, got {other:?}"),
    };
    let text = mychess_gui::windows::maintenance::text::notice_text(&m.shell().i18n, &notice);
    assert_ne!(text, "", "the notice has text");
    let mut sim = m.sim();
    let found = sim.find(iced::widget::Id::new("maint-notice"));
    assert!(found.is_ok(), "the line of the notice is in the window");
    assert!(sim.find(text.as_str()).is_ok(), "{text}");
}

#[test]
fn escape_stops_a_running_preview_and_keeps_the_window() {
    let mut m = Maint::six();
    m.open();
    // The effect is not performed: the preview stays on.
    let _effects = mychess_app::update(
        &mut m.rig.rig.shell.app,
        mychess_app::Message::Maintenance(MaintenanceMessage::Start(MaintRequest::Strip {
            what: StripChoice::Comments,
            scope: ScopeChoice::All,
        })),
    );
    assert!(m.app().maintenance_view().previewing);
    m.key(Key::Named(NamedKey::Escape));
    assert!(m.is_open(), "the window stays with its Stop");
    assert!(!m.app().maintenance_view().previewing, "Esc stopped it");
}

#[test]
fn f1_in_the_window_opens_the_manual_at_the_chapter_of_the_maintenance() {
    let mut m = Maint::six();
    m.open();
    m.key(Key::Named(NamedKey::F(1)));
    assert!(m.app().manual().is_open());
    assert_eq!(m.app().manual().chapter(), 10);
}
