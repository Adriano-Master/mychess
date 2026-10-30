//! M8-8c (GUI, spec D2571-D2573, D2579-D2580): the menu entry and the
//! shortcut, the panel drawn from the view of the application, the colors
//! with symbols, and the Fluent files.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::large_futures
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_app::tablebase::{
    LocalHandle, OpenedSet, TablebaseEffect, TablebaseMessage, TablebaseNotice, TbCategory,
};
use mychess_app::{Action, AppNotice, Effect, Message as AppMessage, PaneKind, perform, update};
use mychess_core::{Game, Position};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::tablebase_panel::{TABLEBASE_PANE_ID, TablebaseUiMessage};
use mychess_gui::windows::tablebase_text::{dtm_text, dtz_text, issue_text, result_word};
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};
use mychess_tablebase::testing::{StubProbe, report};
use mychess_tablebase::{Category, Evaluation, OpenReport, Source};

const KQ: &str = "8/8/8/8/8/2k5/8/KQ6 w - - 0 1";

fn render(shell: &Shell) -> iced_test::Simulator<'_, Message> {
    let element = mychess_gui::windows::main_window::view(shell);
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        element,
    )
}

fn tools_entry(shell: &Shell) -> (Option<String>, bool) {
    for spec in menu_specs(&shell.app, &shell.ui, ViewOptions::default()) {
        if spec.label_key != "menu-tools" {
            continue;
        }
        for entry in &spec.entries {
            if let MenuEntry::Item {
                label: Label::Key("menu-tools-tablebase"),
                shortcut,
                checked,
                ..
            } = entry
            {
                return (shortcut.map(|s| s.to_string()), checked.unwrap_or(false));
            }
        }
    }
    panic!("the Tools menu has no entry for the tablebases");
}

/// A shell whose application has the local tables open (a stub) and the
/// position of the first rows answered.
async fn shell_with_list() -> Shell {
    let mut shell = Shell::new(None);
    let position = Position::from_fen(KQ).unwrap();
    shell
        .app
        .session
        .replace_game(Game::from_position(position.clone()));
    let stub = Arc::new(StubProbe::new(5));
    stub.answer(
        &position,
        Ok(report(
            &position,
            Source::Local,
            Evaluation {
                dtz: Some(5),
                ..Evaluation::of(Category::Win)
            },
            &[
                ("b1d1", Category::Win, Some(3)),
                ("b1e4", Category::Win, Some(9)),
                ("b1b2", Category::Draw, Some(0)),
                ("a1a2", Category::Loss, Some(-4)),
            ],
        )),
    );
    let mut queue: Vec<Effect> = update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::AddDirectory(PathBuf::from("syzygy"))),
    );
    queue.extend(update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::SetOpen(true)),
    ));
    shell.ui.open_pane(PaneKind::Tablebase);
    while let Some(effect) = queue.pop() {
        let Effect::Tablebase(tb) = effect else {
            continue;
        };
        let answer = match tb {
            TablebaseEffect::Open { request, .. } => {
                AppMessage::Tablebase(TablebaseMessage::Opened {
                    request,
                    result: Box::new(Ok(OpenedSet {
                        handle: LocalHandle::new(stub.clone()),
                        report: OpenReport {
                            max_pieces: 5,
                            ..OpenReport::default()
                        },
                    })),
                })
            }
            TablebaseEffect::Delay { .. } => continue,
            other => perform(Effect::Tablebase(other)).await,
        };
        queue.extend(update(&mut shell.app, answer));
    }
    shell
}

#[test]
fn the_tools_menu_offers_the_panel_with_a_function_key_shortcut() {
    let shell = Shell::new(None);
    let (shortcut, checked) = tools_entry(&shell);
    assert!(!checked);
    let shortcut = shortcut.expect("a shortcut");
    assert!(shortcut.contains("F8"), "{shortcut}");
}

#[test]
fn the_default_key_is_free_and_has_no_alt() {
    let shell = Shell::new(None);
    let key = shell.app.shortcuts.bindings(Action::ToggleTablebasePane)[0].to_string();
    let mut owners = Vec::new();
    for (action, bindings) in shell.app.shortcuts.iter() {
        if bindings.iter().any(|b| b.to_string() == key) {
            owners.push(action);
        }
    }
    assert_eq!(owners, [Action::ToggleTablebasePane]);
    assert!(
        !shell.app.shortcuts.bindings(Action::ToggleTablebasePane)[0]
            .to_string()
            .to_lowercase()
            .contains("alt")
    );
}

#[test]
fn the_action_and_the_menu_toggle_the_pane() {
    let mut shell = Shell::new(None);
    assert!(!shell.ui.is_open(PaneKind::Tablebase));
    drop(shell.update(Message::Ui(UiMessage::Action(Action::ToggleTablebasePane))));
    assert!(shell.ui.is_open(PaneKind::Tablebase));
    assert!(
        shell.app.tablebase_open(),
        "the application follows the pane"
    );
    assert!(tools_entry(&shell).1);
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Tablebase))));
    assert!(!shell.ui.is_open(PaneKind::Tablebase));
    assert!(!shell.app.tablebase_open());
}

#[test]
fn the_pane_closes_by_itself_during_an_online_game() {
    use mychess_app::online::{OnlinePhase, PlayingState};
    let mut shell = Shell::new(None);
    shell.app.online.phase = OnlinePhase::Playing(PlayingState::Live);
    drop(shell.update(Message::Ui(UiMessage::TogglePane(PaneKind::Tablebase))));
    assert!(!shell.app.tablebase_open());
    // The pane that cannot be served is closed instead of staying empty.
    assert!(!shell.ui.is_open(PaneKind::Tablebase));
}

#[test]
fn without_configuration_the_panel_invites_to_configure() {
    let mut shell = Shell::new(None);
    shell.ui.open_pane(PaneKind::Tablebase);
    drop(update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::SetOpen(true)),
    ));
    let mut sim = render(&shell);
    assert!(sim.find(iced::widget::Id::new(TABLEBASE_PANE_ID)).is_ok());
    let i18n = &shell.i18n;
    assert!(
        sim.find(i18n.tr("tablebase-status-not-configured").as_str())
            .is_ok()
    );
    assert!(sim.find(i18n.tr("tablebase-config-local").as_str()).is_ok());
    assert!(sim.find(i18n.tr("tablebase-config-help").as_str()).is_ok());
}

#[tokio::test]
async fn the_list_shows_every_move_with_a_symbol_and_a_word() {
    let shell = shell_with_list().await;
    let i18n = &shell.i18n;
    let mut sim = render(&shell);
    for san in ["Qd1", "Qe4", "Qb2+", "Ka2"] {
        assert!(sim.find(san).is_ok(), "{san}");
    }
    // The symbol and the word go together, so the color is never alone.
    assert!(
        sim.find(format!("+ {}", i18n.tr("tablebase-result-win")).as_str())
            .is_ok()
    );
    assert!(
        sim.find(format!("= {}", i18n.tr("tablebase-result-draw")).as_str())
            .is_ok()
    );
    assert!(
        sim.find(format!("- {}", i18n.tr("tablebase-result-loss")).as_str())
            .is_ok()
    );
    // Syzygy has no DTM: no such column.
    assert!(sim.find(i18n.tr("tablebase-col-dtm").as_str()).is_err());
    assert!(sim.find(i18n.tr("tablebase-col-dtz").as_str()).is_ok());
    assert!(sim.find("+3").is_ok() && sim.find("+9").is_ok());
    assert!(sim.find(iced::widget::Id::new("tablebase-row-0")).is_ok());
    assert!(sim.find(iced::widget::Id::new("tablebase-row-3")).is_ok());
    // The summary of the position is there (the control of the next test).
    assert!(sim.find(summary_text(&shell).as_str()).is_ok());
}

/// The line "Position: + Win | DTZ +5" as the panel writes it.
fn summary_text(shell: &Shell) -> String {
    let i18n = &shell.i18n;
    let mut args = fluent_bundle::FluentArgs::new();
    args.set(
        "result",
        format!(
            "+ {} | {} +5",
            i18n.tr("tablebase-result-win"),
            i18n.tr("tablebase-col-dtz")
        ),
    );
    i18n.tr_args("tablebase-position-result", &args)
}

#[tokio::test]
async fn the_training_hides_the_list() {
    let mut shell = shell_with_list().await;
    drop(update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::TrainStart),
    ));
    assert!(shell.app.tablebase_training());
    let mut sim = render(&shell);
    assert!(sim.find(iced::widget::Id::new("tablebase-row-0")).is_err());
    assert!(sim.find("Qd1").is_err());
    // Nor the summary: the result of the position and the counts are the
    // answer the user is training to find (D2574).
    assert!(sim.find(summary_text(&shell).as_str()).is_err());
    let i18n = &shell.i18n;
    let mut counts = fluent_bundle::FluentArgs::new();
    for (name, value) in [("wins", 2), ("draws", 1), ("losses", 1), ("unknown", 16)] {
        counts.set(name, value);
    }
    assert!(
        sim.find(i18n.tr_args("tablebase-counts", &counts).as_str())
            .is_err()
    );
}

#[test]
fn the_plurals_agree_with_the_number_in_both_languages() {
    use fluent_bundle::FluentArgs;
    let cases: [(&str, &[&str]); 4] = [
        ("tablebase-train-status", &["moves", "slips"]),
        ("tablebase-outcome-summary", &["moves", "slips"]),
        ("tablebase-config-dir-bad", &["count"]),
        ("tablebase-config-dir-uppercase", &["count"]),
    ];
    let wrong_for_one = [
        "1 moves",
        "1 slips",
        "1 lances",
        "1 escorregadas",
        "1 unusable files",
        "1 arquivos",
        "1 files ",
        "1 file have",
        "1 arquivo têm",
    ];
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for (key, names) in cases {
            let render = |n: i64| {
                let mut args = FluentArgs::new();
                for name in names {
                    args.set(*name, n);
                }
                args.set("side", "X");
                args.set("grade", "Y");
                i18n.tr_args(key, &args)
            };
            let (one, many) = (render(1), render(2));
            for wrong in wrong_for_one {
                assert!(!one.contains(wrong), "{key}: {one}");
            }
            assert_ne!(one, many, "{key}");
            assert!(one.contains('1') && many.contains('2'), "{key}");
        }
    }
}

#[tokio::test]
async fn a_folder_with_upper_case_table_files_is_told_to_be_renamed() {
    use mychess_tablebase::{DirReport, DirStatus};
    let mut shell = Shell::new(None);
    shell.ui.open_pane(PaneKind::Tablebase);
    shell.ui.dialogs.tablebase.config_open = true;
    let stub = Arc::new(StubProbe::new(5));
    let mut queue = update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::AddDirectory(PathBuf::from("syzygy"))),
    );
    queue.extend(update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::SetOpen(true)),
    ));
    while let Some(effect) = queue.pop() {
        if let Effect::Tablebase(TablebaseEffect::Open { request, .. }) = effect {
            queue.extend(update(
                &mut shell.app,
                AppMessage::Tablebase(TablebaseMessage::Opened {
                    request,
                    result: Box::new(Ok(OpenedSet {
                        handle: LocalHandle::new(stub.clone()),
                        report: OpenReport {
                            directories: vec![DirReport {
                                path: PathBuf::from("syzygy"),
                                status: DirStatus::Read,
                                wdl_files: 0,
                                dtz_files: 0,
                                max_pieces: 0,
                                uppercase_files: 2,
                                issues: Vec::new(),
                            }],
                            max_pieces: 5,
                            ..OpenReport::default()
                        },
                    })),
                }),
            ));
        }
    }
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("count", 2);
    let wanted = shell.i18n.tr_args("tablebase-config-dir-uppercase", &args);
    let mut sim = render(&shell);
    assert!(sim.find(wanted.as_str()).is_ok(), "{wanted}");
}

#[test]
fn the_notices_of_the_tablebase_have_texts() {
    let i18n = I18n::new(Language::EnUs);
    for notice in [
        TablebaseNotice::Busy,
        TablebaseNotice::NotAtEndOfLine,
        TablebaseNotice::NothingToTrain,
        TablebaseNotice::BadMaterial,
        TablebaseNotice::FolderNotSaved,
        TablebaseNotice::TooManyFolders,
    ] {
        let text =
            mychess_gui::windows::status_bar::app_notice_text(&i18n, &AppNotice::Tablebase(notice));
        assert!(
            !text.is_empty() && text != notice.message_key(),
            "{notice:?}"
        );
    }
}

#[test]
fn the_typed_folder_goes_to_the_application_once() {
    let mut shell = Shell::new(None);
    drop(
        shell.update(Message::TablebaseUi(TablebaseUiMessage::FolderEdit(
            "  /tb/syzygy ".to_owned(),
        ))),
    );
    drop(shell.update(Message::TablebaseUi(TablebaseUiMessage::AddFolder)));
    assert_eq!(shell.app.prefs.tablebase.directories, ["/tb/syzygy"]);
    assert!(shell.ui.dialogs.tablebase.folder.is_empty());
    // An empty field adds nothing.
    drop(shell.update(Message::TablebaseUi(TablebaseUiMessage::AddFolder)));
    assert_eq!(shell.app.prefs.tablebase.directories.len(), 1);
}

#[test]
fn a_typed_material_replaces_the_game_and_starts_the_training() {
    let mut shell = Shell::new(None);
    // A source must be turned on to train.
    drop(update(
        &mut shell.app,
        AppMessage::Tablebase(TablebaseMessage::SetOnline(true)),
    ));
    drop(
        shell.update(Message::TablebaseUi(TablebaseUiMessage::MaterialEdit(
            "KRvKP".to_owned(),
        ))),
    );
    drop(shell.update(Message::TablebaseUi(TablebaseUiMessage::NewTyped)));
    assert_eq!(
        mychess_tablebase::material_name(shell.app.session.position()),
        "KRvKP"
    );
    assert!(shell.app.tablebase_training());
}

// ---- Fluent ----

fn ftl(language: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("tablebase.ftl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .collect()
}

fn variables(text: &str) -> BTreeSet<String> {
    text.split("{ $")
        .skip(1)
        .filter_map(|rest| rest.split([' ', '}']).next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn both_languages_have_the_same_keys_and_variables() {
    let (en, pt) = (ftl("en-US"), ftl("pt-BR"));
    assert!(en.len() > 90, "the file was read: {}", en.len());
    assert_eq!(en.keys().collect::<Vec<_>>(), pt.keys().collect::<Vec<_>>());
    for (key, text) in &en {
        assert_eq!(variables(text), variables(&pt[key]), "variables of {key}");
    }
}

#[test]
fn every_result_issue_and_distance_has_a_text_in_both_languages() {
    use mychess_app::tablebase::IssueView;
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for category in TbCategory::ALL {
            let word = result_word(&i18n, category);
            assert!(
                !word.is_empty() && !word.starts_with("tablebase-"),
                "{category:?}"
            );
        }
        for issue in [
            IssueView::NoTables,
            IssueView::TooManyPieces { pieces: 8, max: 5 },
            IssueView::TooFewPieces,
            IssueView::Castling,
            IssueView::MissingTable {
                material: "KRvKP".to_owned(),
            },
            IssueView::Corrupt {
                material: "KQvK".to_owned(),
            },
            IssueView::Network,
            IssueView::TimedOut,
            IssueView::RateLimited { seconds: 42 },
            IssueView::Server,
            IssueView::Invalid,
            IssueView::Internal,
        ] {
            let text = issue_text(&i18n, &issue);
            assert!(!text.starts_with("tablebase-"), "{issue:?}: {text}");
        }
        assert_eq!(dtm_text(&i18n, None), "-");
        assert!(
            dtm_text(&i18n, Some(5)).contains('3'),
            "5 plies are 3 moves"
        );
        assert_eq!(dtz_text(Some(7), false), "+7");
    }
}
