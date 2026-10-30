//! M7-2c review fixes (spec M7-2, D3681-D3687): the questions of the EPD
//! window take the keys before the report, a rename of a favorite does not
//! follow a change of places, F5 respects the "Update" button, the closed
//! window drops its model and the search is found once per change.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::items_after_statements
)]

mod m7_2c_common;

use m7_2c_common::*;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::epd::EpdForm;
use mychess_gui::windows::merge_dialog as md;

fn run(rig: &mut Rig, task: iced::Task<Message>) {
    rig.drain(task);
}

fn names(rig: &Rig) -> Vec<String> {
    rig.shell
        .app
        .opening_report_view()
        .favorites
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

#[test]
fn esc_answers_the_epd_quit_question_before_the_report_and_keeps_the_report() {
    let mut rig = Rig::sample();
    rig.open_window();
    let path = rig.dir.path().join("a.epd");
    std::fs::write(
        &path,
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - id \"T.1\";\n",
    )
    .unwrap();
    rig.app(AppMessage::Epd(
        mychess_app::epd_window::EpdMessage::OpenPath(path),
    ));
    rig.app(AppMessage::Epd(
        mychess_app::epd_window::EpdMessage::AddPosition,
    ));
    rig.msg(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(rig.shell.ui.dialogs.epd.form, Some(EpdForm::ConfirmQuit));
    assert!(rig.is_open());
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    run(&mut rig, task);
    assert!(
        rig.shell.ui.dialogs.epd.form.is_none(),
        "the question above the report took the Esc"
    );
    assert!(rig.is_open(), "the report stays");
    // With nothing above it, Esc closes the report again.
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    run(&mut rig, task);
    assert!(!rig.is_open());
}

#[test]
fn moving_or_deleting_a_favorite_drops_an_open_rename() {
    for change in ["up", "down", "delete"] {
        let mut rig = Rig::sample();
        rig.open_window();
        rig.ui(OpRepUiMessage::OpenFavorites);
        for name in ["Ruy", "Italiana", "Siciliana"] {
            rig.ui(OpRepUiMessage::FavName(name.to_owned()));
            rig.ui(OpRepUiMessage::FavAdd);
        }
        assert_eq!(names(&rig), ["Ruy", "Italiana", "Siciliana"]);
        rig.ui(OpRepUiMessage::FavRenameStart(1));
        rig.ui(OpRepUiMessage::FavRenameInput("Renamed".to_owned()));
        match change {
            "up" => rig.ui(OpRepUiMessage::FavUp(1)),
            "down" => rig.ui(OpRepUiMessage::FavDown(0)),
            _ => rig.ui(OpRepUiMessage::FavDelete(0)),
        }
        let Some(OpRepForm::Favorites(draft)) = &rig.shell.ui.dialogs.oprep.form else {
            panic!("the form stays");
        };
        assert!(draft.rename.is_none(), "{change}: the field is gone");
        // The OK of the field that is gone renames nothing.
        let before = names(&rig);
        rig.ui(OpRepUiMessage::FavRenameGo);
        assert_eq!(names(&rig), before, "{change}");
        assert!(!before.iter().any(|n| n == "Renamed"), "{change}");
    }
}

#[test]
fn f5_does_not_start_a_report_while_the_batch_of_the_favorites_runs() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::OpenFavorites);
    rig.ui(OpRepUiMessage::FavName("Ruy".to_owned()));
    rig.ui(OpRepUiMessage::FavAdd);
    let out = rig.dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    // The first item is asked for but never run: the batch stays active.
    let task = rig.once(Message::OpRep(OpRepUiMessage::FavDirPicked(Some(out))));
    drop(task);
    assert!(rig.shell.app.opening_report_view().batch.is_some());
    // The job, if one started, is not run: it would end at once.
    let task = rig.key(Key::Named(NamedKey::F(5)), Modifiers::NONE);
    drop(task);
    assert!(
        !rig.shell.app.opening_report().is_running(),
        "no job competes with the batch"
    );
}

#[test]
fn f5_updates_the_report_when_nothing_is_running() {
    let mut rig = Rig::sample();
    rig.open_window();
    let task = rig.key(Key::Named(NamedKey::F(5)), Modifiers::NONE);
    drop(task);
    assert!(rig.shell.app.opening_report().is_running());
}

#[test]
fn closing_the_window_drops_the_model_and_the_hits() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::Find("lima".to_owned()));
    assert!(rig.shell.ui.dialogs.oprep.model().is_some());
    assert!(rig.shell.ui.dialogs.oprep.hits().count() > 0);
    rig.ui(OpRepUiMessage::Close);
    assert!(!rig.is_open());
    assert!(rig.shell.ui.dialogs.oprep.model().is_none());
    assert_eq!(rig.shell.ui.dialogs.oprep.hits().count(), 0);
    // Opening again builds a fresh one.
    rig.open_window();
    assert!(rig.shell.ui.dialogs.oprep.model().is_some());
}

#[test]
fn the_hits_follow_the_text_and_the_model_and_agree_with_the_count() {
    let mut rig = Rig::sample();
    rig.open_window();
    assert_eq!(rig.shell.ui.dialogs.oprep.hits().count(), 0);
    rig.ui(OpRepUiMessage::Find("LIMA".to_owned()));
    let count = rig.shell.ui.dialogs.oprep.hits().count();
    assert_eq!(count, rig.model().count_matches("lima"));
    assert!(count >= 2);
    // The text stays through an update; the hits are found again.
    rig.ui(OpRepUiMessage::Refresh);
    assert_eq!(rig.shell.ui.dialogs.oprep.hits().count(), count);
    rig.ui(OpRepUiMessage::Find(String::new()));
    assert_eq!(rig.shell.ui.dialogs.oprep.hits().count(), 0);
    // A word of the theory table marks that row only.
    rig.ui(OpRepUiMessage::Find("Nc6".to_owned()));
    use mychess_gui::windows::opening_report::model::Block;
    let block = rig
        .model()
        .blocks
        .iter()
        .position(|b| matches!(b, Block::Theory(_)))
        .unwrap();
    let theory = rig.theory();
    let expected: Vec<bool> = theory
        .rows
        .iter()
        .map(|r| r.cells.iter().any(|c| c.0.to_lowercase().contains("nc6")))
        .collect();
    assert!(expected.iter().any(|h| *h));
    for (i, want) in expected.iter().enumerate() {
        assert_eq!(
            rig.shell.ui.dialogs.oprep.hits().hit(block, i),
            *want,
            "row {i}"
        );
    }
}

#[test]
fn merge_opens_its_dialog_above_the_report_with_the_games_and_esc_keeps_the_report() {
    let mut rig = Rig::sample();
    rig.open_window();
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-merge");
    for m in messages {
        rig.msg(m);
    }
    let top = || ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs);
    assert_eq!(top(), Some(ActiveDialog::Merge));
    let view = rig.shell.app.merge_view().expect("the merge dialog");
    assert!(view.batch, "the list of games of the report (OpenBatch)");
    assert!(!view.numbers.is_empty());
    // The dialog is drawn above the report, which stays under it.
    {
        let mut sim = rig.sim(1400.0, 900.0);
        assert!(sim.find(id(md::MERGE_BOX_ID)).is_ok());
        assert!(sim.find(id("oprep-window")).is_ok());
    }
    let task = rig.key(Key::Named(NamedKey::Escape), Modifiers::NONE);
    run(&mut rig, task);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        None
    );
    assert!(rig.is_open(), "Esc closed the dialog, not the report");
    // The confirmation belongs to the dialog.
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-merge");
    for m in messages {
        rig.msg(m);
    }
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Merge)
    );
    let depth = rig.shell.app.session.undo_depth();
    let task = rig.key(Key::Named(NamedKey::Enter), Modifiers::NONE);
    run(&mut rig, task);
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        None
    );
    assert!(rig.is_open());
    assert_eq!(rig.shell.app.session.undo_depth(), depth + 1);
}
