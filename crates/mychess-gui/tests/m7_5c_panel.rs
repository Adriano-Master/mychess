//! M7-5c (GUI, spec M7-5): the EPD panel opens with the first file, lists the
//! records (virtualized), edits an opcode, sorts, removes, saves and refuses
//! the same file twice.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m7_5c_common;

use m7_5c_common::*;
use mychess_app::epd_window::{EpdSort, SortKey};

#[test]
fn opening_a_file_opens_the_pane_with_one_tab_and_the_rows() {
    let mut rig = Rig::new();
    assert!(!rig.shell.ui.is_open(PaneKind::Epd));
    rig.open("cinco.epd", CINCO);
    assert!(rig.shell.ui.is_open(PaneKind::Epd));
    assert_eq!(rig.view().tabs.len(), 1);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(id("epd-pane")).is_ok());
    let tab = format!("epd-tab-{}", rig.tab().id.0);
    assert!(sim.find(id(&tab)).is_ok(), "{tab}");
    for n in 1..=5 {
        assert!(sim.find(id(&format!("epd-row-{n}"))).is_ok(), "row {n}");
    }
    assert!(sim.find("5 positions").is_ok());
    for theme in themes() {
        assert!(rig.sim(1400.0, 900.0).snapshot(&theme).is_ok());
    }
}

#[test]
fn the_list_builds_only_the_rows_near_the_viewport() {
    let mut text = String::new();
    // Kings on e1/e8, a rook and a knight on every pair of other squares.
    for rook in 0..64usize {
        for knight in 0..64usize {
            let squares = [(rook, 'R'), (knight, 'N'), (4, 'K'), (60, 'k')];
            let distinct = rook != knight && ![4, 60].contains(&rook) && ![4, 60].contains(&knight);
            if !distinct {
                continue;
            }
            let mut ranks = Vec::new();
            for rank in (0..8).rev() {
                let mut line = String::new();
                let mut empty = 0;
                for file in 0..8 {
                    match squares.iter().find(|(sq, _)| *sq == rank * 8 + file) {
                        Some((_, c)) => {
                            if empty > 0 {
                                line.push_str(&empty.to_string());
                                empty = 0;
                            }
                            line.push(*c);
                        }
                        None => empty += 1,
                    }
                }
                if empty > 0 {
                    line.push_str(&empty.to_string());
                }
                ranks.push(line);
            }
            text.push_str(&format!(
                "{} w - - id \"R{rook}N{knight}\";\n",
                ranks.join("/")
            ));
        }
    }
    let mut rig = Rig::new();
    rig.open("big.epd", &text);
    let records = rig.tab().records;
    assert!(records > 500, "{records} distinct positions");
    let sim_ok = {
        let mut sim = rig.sim(1400.0, 900.0);
        let first = sim.find(id("epd-row-1")).is_ok();
        let far = sim.find(id(&format!("epd-row-{}", records - 1))).is_ok();
        first && !far
    };
    assert!(sim_ok, "the first row is built and a far one is not");
}

#[test]
fn editing_an_opcode_marks_the_file_altered_and_commit_writes_the_record() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = rig.shell.app.epd_rows(rig.tab().id, 0..1)[0].index;
    rig.send(EpdMessage::Select(first));
    let tab = rig.tab();
    assert!(tab.editor_text.contains("bm e5"));
    assert!(tab.editor_enabled);
    // The editor shows the record.
    assert!(rig.shell.ui.dialogs.epd.content.text().contains("bm e5"));
    // Typing goes through the shell to the draft.
    use iced::widget::text_editor::{Action, Edit, Motion};
    rig.ui(Message::Epd(EpdUiMessage::Editor(Action::Move(
        Motion::DocumentEnd,
    ))));
    for c in "ce 7\n".chars() {
        let edit = if c == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(c)
        };
        rig.ui(Message::Epd(EpdUiMessage::Editor(Action::Edit(edit))));
    }
    assert!(rig.tab().editor_draft);
    assert!(rig.tab().editor_text.contains("ce 7"));
    rig.send(EpdMessage::Commit);
    let tab = rig.tab();
    assert!(tab.dirty && !tab.editor_draft);
    assert!(tab.editor_text.contains("ce 7"));
}

#[test]
fn a_read_only_file_ignores_typing_and_disables_the_save() {
    let mut rig = Rig::new();
    rig.open_read_only("ro.epd", CINCO);
    let tab = rig.tab();
    assert!(tab.read_only);
    assert!(!tab.editor_enabled);
    let first = rig.shell.app.epd_rows(tab.id, 0..1)[0].index;
    rig.send(EpdMessage::Select(first));
    use iced::widget::text_editor::{Action, Edit};
    let before = rig.shell.ui.dialogs.epd.content.text();
    rig.ui(Message::Epd(EpdUiMessage::Editor(Action::Edit(
        Edit::Insert('x'),
    ))));
    assert_eq!(rig.shell.ui.dialogs.epd.content.text(), before);
    assert!(!rig.tab().editor_draft);
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(id("epd-btn-save")).unwrap();
    assert_eq!(sim.into_messages().count(), 0, "Save is disabled");
}

#[test]
fn sorting_orders_the_rows_and_reversing_flips_them() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let id_tab = rig.tab().id;
    let fens = |rig: &Rig| -> Vec<String> {
        rig.shell
            .app
            .epd_rows(id_tab, 0..5)
            .into_iter()
            .map(|r| r.fen)
            .collect()
    };
    let natural = fens(&rig);
    rig.send(EpdMessage::SetSort(EpdSort {
        key: SortKey::Fen,
        descending: false,
    }));
    let mut sorted = natural.clone();
    sorted.sort();
    assert_eq!(fens(&rig), sorted);
    rig.send(EpdMessage::SetSort(EpdSort {
        key: SortKey::Fen,
        descending: true,
    }));
    sorted.reverse();
    assert_eq!(fens(&rig), sorted);
    // The header shows the order on the button.
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("Position ▼").is_ok());
}

#[test]
fn removing_a_position_asks_first_and_closes_the_gap() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = rig.shell.app.epd_rows(rig.tab().id, 0..1)[0].index;
    rig.send(EpdMessage::Select(first));
    rig.send(EpdMessage::RemovePosition);
    assert!(rig.tab().pending_remove.is_some());
    assert!(rig.shell.modal_layer_open());
    assert!(rig.sim(1400.0, 900.0).find(id("epd-remove-box")).is_ok());
    rig.send(EpdMessage::ConfirmRemove(true));
    let tab = rig.tab();
    assert_eq!(tab.records, 4);
    assert!(tab.dirty);
    assert!(!rig.shell.modal_layer_open());
    assert!(rig.sim(1400.0, 900.0).find(id("epd-remove-box")).is_err());
}

#[test]
fn cancelling_the_removal_keeps_the_record() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    let first = rig.shell.app.epd_rows(rig.tab().id, 0..1)[0].index;
    rig.send(EpdMessage::Select(first));
    rig.send(EpdMessage::RemovePosition);
    // Esc answers "no".
    rig.ui(Message::Ui(UiMessage::Key {
        input: mychess_app::KeyInput {
            key: mychess_app::Key::Named(mychess_app::NamedKey::Escape),
            modifiers: mychess_app::Modifiers::NONE,
        },
        text_focused: false,
    }));
    assert_eq!(rig.tab().records, 5);
    assert!(rig.tab().pending_remove.is_none());
}

#[test]
fn saving_writes_the_file_and_clears_the_marker() {
    let mut rig = Rig::new();
    let path = rig.open("cuatro.epd", &cuatro());
    rig.send(EpdMessage::AddPosition);
    assert!(rig.tab().dirty);
    rig.send(EpdMessage::Save);
    assert!(!rig.tab().dirty);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 5, "{text}");
}

#[test]
fn the_same_file_cannot_be_opened_twice() {
    let mut rig = Rig::new();
    let path = rig.open("cinco.epd", CINCO);
    rig.send(EpdMessage::OpenPath(path));
    assert_eq!(rig.view().tabs.len(), 1);
    let notice = rig.shell.app.notice.clone();
    assert!(
        matches!(
            notice,
            Some(mychess_app::AppNotice::Epd(
                mychess_app::epd_window::EpdNotice::AlreadyOpen
            ))
        ),
        "{notice:?}"
    );
}

#[test]
fn a_fifth_file_is_refused() {
    let mut rig = Rig::new();
    for n in 0..4 {
        rig.open(&format!("f{n}.epd"), CINCO.lines().nth(n).unwrap());
    }
    assert_eq!(rig.view().tabs.len(), 4);
    rig.open("f5.epd", CINCO);
    assert_eq!(rig.view().tabs.len(), 4);
    let notice = rig.shell.app.notice.clone();
    assert!(
        matches!(
            notice,
            Some(mychess_app::AppNotice::Epd(
                mychess_app::epd_window::EpdNotice::TooMany { max: 4 }
            ))
        ),
        "{notice:?}"
    );
}

#[test]
fn closing_an_altered_file_asks_and_cancel_keeps_the_tab() {
    let mut rig = Rig::new();
    rig.open("cuatro.epd", &cuatro());
    rig.send(EpdMessage::AddPosition);
    let file = rig.tab().id;
    rig.send(EpdMessage::Close(file));
    assert!(rig.view().close_prompt.is_some());
    assert!(rig.sim(1400.0, 900.0).find(id("epd-close-box")).is_ok());
    rig.send(EpdMessage::AnswerClose(
        mychess_app::epd_window::EpdCloseChoice::Cancel,
    ));
    assert_eq!(rig.view().tabs.len(), 1);
    rig.send(EpdMessage::Close(file));
    rig.send(EpdMessage::AnswerClose(
        mychess_app::epd_window::EpdCloseChoice::Discard,
    ));
    assert!(rig.view().tabs.is_empty());
    assert!(rig.sim(1400.0, 900.0).find("No EPD file is open.").is_ok());
}

#[test]
fn quitting_with_an_altered_file_asks_first() {
    let mut rig = Rig::new();
    rig.open("cuatro.epd", &cuatro());
    rig.send(EpdMessage::AddPosition);
    rig.ui(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    assert!(rig.shell.modal_layer_open());
    assert!(rig.sim(1400.0, 900.0).find(id("epd-quit-box")).is_ok());
    rig.ui(Message::Epd(EpdUiMessage::ConfirmQuit(false)));
    assert!(rig.shell.ui.dialogs.epd.form.is_none());
}

#[test]
fn the_analysis_form_validates_the_seconds_and_lists_the_modes() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.ui(Message::Epd(EpdUiMessage::OpenAnalyze));
    assert!(rig.shell.modal_layer_open());
    {
        let mut sim = rig.sim(1400.0, 900.0);
        assert!(sim.find(id("epd-analyze-box")).is_ok());
        for word in ["Count best moves", "Annotate", "Both", "All"] {
            assert!(sim.find(word).is_ok(), "{word}");
        }
    }
    rig.ui(Message::Epd(EpdUiMessage::AnalyzeSeconds("0".into())));
    let Some(EpdForm::Analyze(draft)) = rig.shell.ui.dialogs.epd.form.clone() else {
        panic!("form");
    };
    assert!(draft.config().is_none());
    rig.ui(Message::Epd(EpdUiMessage::AnalyzeSeconds("301".into())));
    let Some(EpdForm::Analyze(draft)) = rig.shell.ui.dialogs.epd.form.clone() else {
        panic!("form");
    };
    assert!(draft.config().is_none());
    rig.ui(Message::Epd(EpdUiMessage::AnalyzeSeconds("12".into())));
    let Some(EpdForm::Analyze(draft)) = rig.shell.ui.dialogs.epd.form.clone() else {
        panic!("form");
    };
    assert_eq!(draft.config().unwrap().seconds, 12);
    assert!(rig.sim(1400.0, 900.0).find("Only those without ce").is_ok());
    rig.ui(Message::Epd(EpdUiMessage::CloseForm));
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn the_strip_form_removes_the_opcode_from_every_position() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    rig.ui(Message::Epd(EpdUiMessage::OpenStrip));
    rig.ui(Message::Epd(EpdUiMessage::StripInput("id".into())));
    assert!(rig.sim(1400.0, 900.0).find(id("epd-strip-box")).is_ok());
    // The shell sent the message to the application: the batch job is in
    // flight (its effect is a task the test drops), so the file is busy.
    rig.ui(Message::Epd(EpdUiMessage::StripGo));
    assert!(rig.shell.ui.dialogs.epd.form.is_none());
    assert!(rig.tab().busy);
}

#[test]
fn the_ignored_lines_open_a_report() {
    let mut rig = Rig::new();
    let text = format!("{CINCO}not a position\n");
    rig.open("sujo.epd", &text);
    assert!(rig.tab().skipped_lines >= 1);
    rig.ui(Message::Epd(EpdUiMessage::OpenReport));
    assert!(rig.shell.modal_layer_open());
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(id("epd-report-box")).is_ok());
    assert!(sim.find("Lines ignored in sujo.epd").is_ok());
}
