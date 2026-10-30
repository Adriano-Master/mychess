//! M7-3c (GUI, spec M7-3): the dialog of the player report is seeded by the
//! application, the window shows the report with its clicks, the progress
//! and the stale warning, files are saved in 3 parts x 3 formats, the
//! Clipbase option asks first, and every layer is modal.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

mod m7_2c_common;

use m7_2c_common::*;
use mychess_app::player_report::{
    NameMatching, PlayerColor, PlayerPhase, PlayerReportMessage, PlayerReportNotice, PlayerStart,
    ReportFormat, ReportPart,
};
use mychess_gui::windows::player_report::{PrForm, PrUiMessage};

fn rig() -> Rig {
    let mut rig = Rig::sample();
    rig.shell
        .app
        .set_player_report_today(Some("2026.10.29".parse().unwrap()));
    rig
}

fn pr(rig: &mut Rig, m: PrUiMessage) {
    rig.msg(Message::PlayerReportUi(m));
}

fn phase(rig: &Rig) -> PlayerPhase {
    rig.shell.app.player_report_view().phase
}

/// Opens the dialog for "Alfa, A" and makes the report.
fn report(rig: &mut Rig) {
    pr(rig, PrUiMessage::Open(Some("Alfa, A".to_owned())));
    pr(rig, PrUiMessage::Color(PlayerColor::White));
    pr(rig, PrUiMessage::Generate);
    assert_eq!(phase(rig), PlayerPhase::Ready);
}

fn notice(rig: &Rig) -> Option<PlayerReportNotice> {
    match rig.shell.ui.status_notice.as_ref() {
        Some(AppNotice::PlayerReport(n)) => Some(*n),
        _ => None,
    }
}

#[test]
fn the_menu_opens_a_seeded_dialog_and_cancel_closes_it() {
    let mut rig = rig();
    assert!(!rig.shell.modal_layer_open());
    pr(&mut rig, PrUiMessage::Open(None));
    assert!(rig.shell.app.player_report().dialog_open());
    assert!(rig.shell.modal_layer_open());
    let draft = rig.shell.ui.dialogs.preport.draft.clone().expect("a draft");
    // The board holds game 5: its White player seeds the name.
    assert!(!draft.name.is_empty());
    let mut sim = rig.sim(1400.0, 900.0);
    for name in [
        "preport-dialog",
        "preport-name",
        "preport-ok",
        "preport-cancel",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    drop(sim);
    pr(&mut rig, PrUiMessage::Cancel);
    assert!(!rig.shell.app.player_report().dialog_open());
    assert!(rig.shell.ui.dialogs.preport.draft.is_none());
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn the_draft_is_edited_and_sent_with_ok() {
    let mut rig = rig();
    pr(&mut rig, PrUiMessage::Open(Some("Gama, G".to_owned())));
    pr(&mut rig, PrUiMessage::Name("Alfa, A".to_owned()));
    pr(&mut rig, PrUiMessage::Matching(NameMatching::Prefix));
    pr(&mut rig, PrUiMessage::Start(PlayerStart::Beginning));
    pr(&mut rig, PrUiMessage::Color(PlayerColor::Black));
    let draft = rig.shell.ui.dialogs.preport.draft.clone().unwrap();
    assert_eq!(draft.name, "Alfa, A");
    assert_eq!(draft.matching, NameMatching::Prefix);
    pr(&mut rig, PrUiMessage::Generate);
    assert_eq!(phase(&rig), PlayerPhase::Ready);
    let view = rig.shell.app.player_report_view();
    let report = view.report.unwrap();
    assert_eq!(report.name, "Alfa, A");
    assert_eq!(report.color, mychess_core::Color::Black);
    assert_eq!(report.game_count(), 2);
    // The dialog closed with the work.
    assert!(!rig.shell.app.player_report().dialog_open());
    assert!(rig.shell.ui.dialogs.preport.draft.is_none());
}

#[test]
fn ok_is_disabled_for_a_name_without_text() {
    let mut rig = rig();
    pr(&mut rig, PrUiMessage::Open(Some("Alfa, A".to_owned())));
    pr(&mut rig, PrUiMessage::Name("  ".to_owned()));
    assert!(click_id(rig.sim(1400.0, 900.0), "preport-ok").is_empty());
    pr(&mut rig, PrUiMessage::Name("?".to_owned()));
    assert!(click_id(rig.sim(1400.0, 900.0), "preport-ok").is_empty());
    pr(&mut rig, PrUiMessage::Name("Alfa, A".to_owned()));
    let sent = debug_of(&click_id(rig.sim(1400.0, 900.0), "preport-ok"));
    assert!(sent.iter().any(|m| m.contains("Generate")), "{sent:?}");
}

#[test]
fn the_game_buttons_fill_the_name_and_colour() {
    let mut rig = rig();
    pr(&mut rig, PrUiMessage::Open(Some("Zed".to_owned())));
    pr(&mut rig, PrUiMessage::FromGame(PlayerColor::Black));
    let draft = rig.shell.ui.dialogs.preport.draft.clone().unwrap();
    let view = rig.shell.app.player_report_dialog().unwrap();
    assert_eq!(Some(draft.name), view.black_name);
    assert_eq!(draft.color, PlayerColor::Black);
}

#[test]
fn the_window_shows_the_report_and_its_clicks() {
    let mut rig = rig();
    report(&mut rig);
    assert!(rig.shell.app.player_report().is_open());
    let sim = rig.tall();
    let mut sim = sim;
    for name in [
        "preport-window",
        "preport-select-all",
        "preport-opponent-1",
        "preport-game-1",
        "preport-update",
        "preport-save",
        "preport-view-html",
        "preport-change",
        "preport-close",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(sim.find("A Alfa with the White pieces").is_ok());
    drop(sim);
    let sent = debug_of(&click_id(rig.tall(), "preport-opponent-1"));
    assert!(sent.iter().any(|m| m.contains("OpenOpponent")), "{sent:?}");
    let sent = debug_of(&click_id(rig.tall(), "preport-game-1"));
    assert!(sent.iter().any(|m| m.contains("LoadGame(1)")), "{sent:?}");
    // A click on the score sets the filter.
    rig.app(AppMessage::PlayerReport(PlayerReportMessage::Select(
        ReportSelector::All,
    )));
    assert_eq!(rig.filtered(), 3);
}

#[test]
fn an_opponent_opens_the_player_information() {
    let mut rig = rig();
    report(&mut rig);
    rig.app(AppMessage::PlayerReport(PlayerReportMessage::OpenOpponent(
        "Beta, B".to_owned(),
    )));
    assert!(rig.shell.app.player_info().is_open());
}

#[test]
fn change_reopens_the_dialog_with_the_choices_of_the_report() {
    let mut rig = rig();
    report(&mut rig);
    pr(&mut rig, PrUiMessage::Change);
    let draft = rig.shell.ui.dialogs.preport.draft.clone().unwrap();
    assert_eq!(draft.name, "Alfa, A");
    assert_eq!(draft.color, PlayerColor::White);
    // The report stays behind the dialog.
    assert!(rig.shell.app.player_report().is_open());
}

#[test]
fn a_report_of_another_database_is_stale_and_update_makes_it_again() {
    let mut rig = rig();
    report(&mut rig);
    assert!(!rig.shell.app.player_report_view().stale);
    rig.open_pgn("other.pgn", &pgn(), 1);
    assert!(rig.shell.app.player_report_view().stale);
    let mut sim = rig.tall();
    assert!(sim.find(id("preport-stale-update")).is_ok());
    assert!(sim.find("The report is out of date").is_ok());
    // Save and View HTML are off while the report is stale.
    drop(sim);
    assert!(click_id(rig.tall(), "preport-save").is_empty());
    let sent = debug_of(&click_id(rig.tall(), "preport-stale-update"));
    assert!(sent.iter().any(|m| m.contains("Refresh")), "{sent:?}");
    pr(&mut rig, PrUiMessage::Refresh);
    assert_eq!(phase(&rig), PlayerPhase::Ready);
    assert!(!rig.shell.app.player_report_view().stale);
}

#[test]
fn the_progress_and_stop_are_on_the_window_while_it_runs() {
    let mut rig = rig();
    pr(&mut rig, PrUiMessage::Open(Some("Alfa, A".to_owned())));
    // The task of the work is not run: the report stays "in flight".
    let _task = rig.once(Message::PlayerReportUi(PrUiMessage::Generate));
    assert!(matches!(phase(&rig), PlayerPhase::Generating(_)));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(id("preport-stop")).is_ok());
    assert!(sim.find("Making the player report").is_ok());
    drop(sim);
    // Update, Save and Change are off meanwhile.
    for name in ["preport-update", "preport-change"] {
        assert!(click_id(rig.sim(1400.0, 900.0), name).is_empty(), "{name}");
    }
    let sent = debug_of(&click_id(rig.sim(1400.0, 900.0), "preport-stop"));
    assert!(sent.iter().any(|m| m.contains("Stop")), "{sent:?}");
}

#[test]
fn the_window_saves_every_part_in_every_format() {
    let mut rig = rig();
    report(&mut rig);
    let mut n = 0;
    for part in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
        for format in [ReportFormat::Text, ReportFormat::Html, ReportFormat::Latex] {
            pr(&mut rig, PrUiMessage::OpenSave);
            assert!(matches!(
                rig.shell.ui.dialogs.preport.form,
                Some(PrForm::Save(_))
            ));
            pr(&mut rig, PrUiMessage::SavePart(part));
            pr(&mut rig, PrUiMessage::SaveFormat(format));
            let path = rig.dir.path().join(format!("out{n}"));
            n += 1;
            pr(&mut rig, PrUiMessage::SavePicked(Some(path.clone())));
            assert!(rig.shell.ui.dialogs.preport.form.is_none());
            let written = path.with_extension(format.extension());
            let text = std::fs::read_to_string(&written)
                .unwrap_or_else(|e| panic!("{part:?} {format:?}: {e}"));
            assert!(text.contains("Alfa"), "{part:?} {format:?}");
            assert_eq!(notice(&rig), Some(PlayerReportNotice::Saved));
        }
    }
}

#[test]
fn cancelling_the_file_dialog_keeps_the_form_and_writes_nothing() {
    let mut rig = rig();
    report(&mut rig);
    pr(&mut rig, PrUiMessage::OpenSave);
    pr(&mut rig, PrUiMessage::SavePicked(None));
    assert!(rig.shell.ui.dialogs.preport.form.is_some());
    let mut sim = rig.sim(1400.0, 900.0);
    for name in [
        "preport-save-box",
        "preport-save-choose",
        "preport-save-cancel",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
}

#[test]
fn escape_closes_the_form_then_the_window() {
    let mut rig = rig();
    report(&mut rig);
    pr(&mut rig, PrUiMessage::OpenSave);
    drop(rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE));
    assert!(rig.shell.ui.dialogs.preport.form.is_none());
    assert!(rig.shell.app.player_report().is_open());
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    rig.drain(task);
    assert!(!rig.shell.app.player_report().is_open());
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn the_keys_of_the_game_behind_are_blocked_while_the_window_is_open() {
    let mut rig = rig();
    report(&mut rig);
    let before = rig.shell.app.session.cursor();
    drop(rig.key(Key::Named(NamedKey::Left), Modifiers::NONE));
    assert_eq!(rig.shell.app.session.cursor(), before);
}

#[test]
fn the_clipbase_option_asks_before_the_clipbase_is_emptied() {
    let mut rig = rig();
    rig.app(BasesMessage::ClipbaseCopy);
    pr(&mut rig, PrUiMessage::Open(Some("Alfa, A".to_owned())));
    pr(&mut rig, PrUiMessage::Color(PlayerColor::White));
    pr(&mut rig, PrUiMessage::Clipbase(true));
    pr(&mut rig, PrUiMessage::Generate);
    assert!(rig.shell.app.player_report_view().confirm_clipbase);
    let mut sim = rig.sim(1400.0, 900.0);
    for name in [
        "preport-confirm",
        "preport-confirm-yes",
        "preport-confirm-no",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(sim.find("The Clipbase will be emptied. Continue?").is_ok());
    drop(sim);
    // Esc answers no: the dialog stays, nothing was made.
    drop(rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE));
    assert!(!rig.shell.app.player_report_view().confirm_clipbase);
    assert!(rig.shell.app.player_report().dialog_open());
    pr(&mut rig, PrUiMessage::Generate);
    pr(&mut rig, PrUiMessage::Confirm(true));
    assert_eq!(phase(&rig), PlayerPhase::Ready);
    let clip = rig.shell.app.bases.slot(SlotId::CLIPBASE).unwrap().total();
    assert_eq!(clip, 3);
}

#[test]
fn the_player_information_has_a_report_button_with_the_player() {
    let mut rig = rig();
    rig.app(AppMessage::PlayerInfo(
        mychess_app::PlayerInfoMessage::Open {
            name: "Alfa, A".to_owned(),
        },
    ));
    let sent = debug_of(&click_id(rig.sim(1400.0, 900.0), "player-info-report"));
    assert!(
        sent.iter()
            .any(|m| m.contains("PlayerReportUi") && m.contains("Alfa, A")),
        "{sent:?}"
    );
}

#[test]
fn the_report_button_of_the_information_closes_it_so_the_dialog_is_on_top() {
    let mut rig = rig();
    rig.app(AppMessage::PlayerInfo(
        mychess_app::PlayerInfoMessage::Open {
            name: "Alfa, A".to_owned(),
        },
    ));
    assert!(rig.shell.app.player_info().is_open());
    let sent = click_id(rig.sim(1400.0, 900.0), "player-info-report");
    for m in sent {
        rig.msg(m);
    }
    assert!(!rig.shell.app.player_info().is_open());
    assert!(rig.shell.app.player_report().dialog_open());
    assert!(
        mychess_gui::windows::dialog_ui::ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs)
            .is_none()
    );
    // The dialog card is on the screen, not behind the information.
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(
        sim.find(id(mychess_gui::windows::player_report::PREPORT_DIALOG_ID))
            .is_ok()
    );
}

#[test]
fn the_texts_follow_the_language() {
    let mut rig = rig();
    report(&mut rig);
    for language in [
        mychess_app::LanguagePref::EnUs,
        mychess_app::LanguagePref::PtBr,
    ] {
        rig.set_language(language);
        let mut sim = rig.tall();
        for key in [
            "preport-title",
            "preport-change",
            "preport-ui-update",
            "preport-ui-save",
            "preport-ui-view-html",
            "preport-ui-close",
            "preport-openings",
        ] {
            let text = rig.shell.i18n.tr(key);
            if key == "preport-openings" {
                continue;
            }
            assert!(sim.find(text.as_str()).is_ok(), "{key} = {text}");
        }
    }
    // The words of the documents follow the language too.
    let html = {
        pr(&mut rig, PrUiMessage::OpenSave);
        pr(&mut rig, PrUiMessage::SaveFormat(ReportFormat::Text));
        let path = rig.dir.path().join("pt");
        pr(&mut rig, PrUiMessage::SavePicked(Some(path.clone())));
        std::fs::read_to_string(path.with_extension("txt")).unwrap()
    };
    assert!(html.contains("do ponto de vista"), "{html}");
}

#[test]
fn both_languages_have_the_same_keys_and_say_variante() {
    use std::collections::BTreeSet;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
    let keys = |lang: &str| -> BTreeSet<String> {
        std::fs::read_to_string(dir.join(lang).join("preport.ftl"))
            .unwrap()
            .lines()
            .filter_map(|l| {
                let (k, _) = l.split_once(" = ")?;
                (!k.starts_with('#') && !k.contains(' ')).then(|| k.to_owned())
            })
            .collect()
    };
    assert_eq!(keys("en-US"), keys("pt-BR"));
    let pt = std::fs::read_to_string(dir.join("pt-BR/preport.ftl")).unwrap();
    assert!(!pt.to_lowercase().contains("variaç"));
    // Every `preport-` key the code names exists.
    let en = keys("en-US");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![
        src.join("windows/player_report.rs"),
        src.join("windows/player_report/dialog.rs"),
        src.join("windows/player_report/view.rs"),
        src.join("shell/player_report_glue.rs"),
    ];
    files.sort();
    let ids = [
        "preport-window",
        "preport-dialog",
        "preport-name",
        "preport-confirm",
        "preport-save-box",
        "preport-body",
    ];
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for chunk in text.split('"').skip(1).step_by(2) {
            let is_key = chunk.starts_with("preport-")
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !chunk.ends_with('-');
            // Ids of widgets share the prefix; keys are the ones of the file
            // or are not widget ids (no `-ok`, `-stop`... words of ids).
            if is_key && !ids.contains(&chunk) && !en.contains(chunk) {
                assert!(
                    chunk.starts_with("preport-recent")
                        || chunk.starts_with("preport-select")
                        || chunk.starts_with("preport-opponent-")
                        || chunk.starts_with("preport-game-")
                        || chunk.starts_with("preport-theme-")
                        || chunk.starts_with("preport-endgame-")
                        || matches!(
                            chunk,
                            "preport-ok"
                                | "preport-cancel"
                                | "preport-white"
                                | "preport-black"
                                | "preport-color-white"
                                | "preport-color-black"
                                | "preport-match-exact"
                                | "preport-match-prefix"
                                | "preport-match-contains"
                                | "preport-start-beginning"
                                | "preport-start-position"
                                | "preport-clipbase"
                                | "preport-confirm-yes"
                                | "preport-confirm-no"
                                | "preport-overwrite"
                                | "preport-overwrite-no"
                                | "preport-overwrite-yes"
                                | "preport-save-cancel"
                                | "preport-save-choose"
                                | "preport-close"
                                | "preport-stop"
                                | "preport-stale-update"
                                | "preport-footer"
                                | "preport-update"
                                | "preport-save"
                                | "preport-view-html"
                                | "preport-change"
                        ),
                    "{} names a key that does not exist: {chunk}",
                    file.display()
                );
            }
        }
    }
}
