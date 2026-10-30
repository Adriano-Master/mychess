//! M6-4d (GUI, spec D3430-D3434): the crosstable panel drawn from the view
//! of the application, the clicks, the buttons and the states.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_4d_common;

use m6_4d_common::*;
use mychess_app::crosstable::{
    CrossFlag, CrossFormat, CrossSort, CrosstableBody, CrosstableEffect, CrosstableFailure,
    CrosstableNotice, ExportFormat,
};
use mychess_gui::host::CrosstableSaveDialog;

/// The sample open, game 1 loaded, the panel open and the table computed.
fn ready() -> Rig {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    rig.toggle(PaneKind::Crosstable);
    rig.app(CrosstableMessage::Refresh);
    assert!(
        matches!(
            rig.shell.app.crosstable_view().body,
            CrosstableBody::Table(_)
        ),
        "the table is computed"
    );
    rig
}

fn messages(sim: Simulator<'_, Message>) -> Vec<Message> {
    sim.into_messages().collect()
}

#[test]
fn the_table_shows_the_players_their_scores_and_the_tournament() {
    let rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    for text in [
        "Club",
        "Rio, 2020.05.01",
        "A",
        "B",
        "C",
        "1.5 / 2",
        "1.0 / 2",
        "0.5 / 2",
    ] {
        assert!(sim.find(text).is_ok(), "{text}");
    }
    // The header words are those the application holds (sent by the shell).
    assert!(sim.find("Player").is_ok() && sim.find("Score").is_ok());
    assert!(sim.find(iced::widget::Id::new("crosstable-pane")).is_ok());
}

#[test]
fn the_headers_sort_and_the_table_follows() {
    let mut rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click("Player").unwrap();
    let sent = messages(sim);
    let Some(Message::App(message)) = sent.into_iter().next() else {
        panic!("a message of the application");
    };
    assert!(matches!(
        message,
        AppMessage::Crosstable(CrosstableMessage::SetSort(CrossSort::Name))
    ));
    rig.app(message);
    assert_eq!(rig.shell.app.crosstable_view().prefs.sort, CrossSort::Name);
}

#[test]
fn a_click_on_a_result_selects_the_game_and_the_strip_offers_browse_and_load() {
    let mut rig = ready();
    // The draw is the only half point on the grid.
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click("\u{bd}").unwrap();
    let picked = messages(sim)
        .into_iter()
        .find_map(|m| match m {
            Message::Panels(DataPanelsMessage::SelectGame(game)) => Some(game),
            _ => None,
        })
        .expect("SelectGame");
    assert_eq!(picked, Some(2), "the draw is game 2");
    rig.ui(Message::Panels(DataPanelsMessage::SelectGame(picked)));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("Game 2").is_ok());
    sim.click(rig.tr("crosstab-browse").as_str()).unwrap();
    assert!(
        messages(sim)
            .into_iter()
            .any(|m| matches!(m, Message::Panels(DataPanelsMessage::BrowseGame(2))))
    );
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("crosstab-load").as_str()).unwrap();
    assert!(messages(sim).into_iter().any(|m| matches!(
        m,
        Message::App(AppMessage::Crosstable(CrosstableMessage::LoadGame(2)))
    )));
}

#[test]
fn browse_lists_the_game_of_the_table_in_the_browser() {
    let mut rig = ready();
    assert!(rig.shell.app.browser.is_none());
    rig.ui(Message::Panels(DataPanelsMessage::BrowseGame(2)));
    assert!(rig.shell.app.browser.is_some());
}

#[test]
fn load_puts_the_game_of_the_table_in_the_editor() {
    let mut rig = ready();
    rig.app(CrosstableMessage::LoadGame(3));
    let slot = rig.shell.app.bases.current();
    assert_eq!(
        rig.shell.app.bases.slot(slot).unwrap().current_game,
        Some(3)
    );
}

#[test]
fn set_filter_and_add_to_filter_act_on_the_games_of_the_tournament() {
    let mut rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("crosstab-set-filter").as_str()).unwrap();
    assert!(messages(sim).into_iter().any(|m| matches!(
        m,
        Message::App(AppMessage::Crosstable(CrosstableMessage::SetFilter(None)))
    )));
    rig.app(CrosstableMessage::SetFilter(None));
    let slot = rig.shell.app.bases.current();
    let held: Vec<u32> = rig
        .shell
        .app
        .bases
        .slot(slot)
        .unwrap()
        .filter()
        .iter_games()
        .collect();
    assert_eq!(held, [1, 2, 3], "the games 4 and 5 are other tournaments");
}

#[test]
fn the_options_unfold_and_a_checkbox_sends_its_flag() {
    let mut rig = ready();
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(rig.tr("crosstab-format-label").as_str()).is_err());
    sim.click(rig.tr("crosstab-options").as_str()).unwrap();
    assert!(
        messages(sim)
            .into_iter()
            .any(|m| matches!(m, Message::Panels(DataPanelsMessage::ToggleCrossOptions)))
    );
    rig.ui(Message::Panels(DataPanelsMessage::ToggleCrossOptions));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(rig.tr("crosstab-format-label").as_str()).is_ok());
    assert!(sim.find(rig.tr("crosstab-sort-label").as_str()).is_ok());
    sim.click(rig.tr("crosstab-opt-tallies").as_str()).unwrap();
    assert!(messages(sim).into_iter().any(|m| matches!(
        m,
        Message::App(AppMessage::Crosstable(CrosstableMessage::SetFlag(
            CrossFlag::Tallies,
            false
        )))
    )));
}

#[test]
fn a_typed_row_color_reaches_the_application_only_when_valid() {
    let mut rig = ready();
    rig.ui(Message::Panels(DataPanelsMessage::RowColorEdit(
        "#12".into(),
    )));
    assert_eq!(rig.shell.app.crosstable_view().prefs.row_color, "#cccccc");
    rig.ui(Message::Panels(DataPanelsMessage::RowColorEdit(
        "#336699".into(),
    )));
    assert_eq!(rig.shell.app.crosstable_view().prefs.row_color, "#336699");
}

#[test]
fn the_format_swiss_and_rounds_draw_their_own_grids() {
    let mut rig = ready();
    rig.app(CrosstableMessage::SetFormat(CrossFormat::Swiss));
    let mut sim = rig.sim(1400.0, 900.0);
    // A, white against B in round 1 and winning: opponent 2, `w`, `+`.
    assert!(sim.find("2w+").is_ok());
    assert!(sim.find("3w=").is_ok(), "round 2 is the draw with C");
    drop(sim);
    rig.app(CrosstableMessage::SetFormat(CrossFormat::Knockout));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("1:").is_ok(), "the round is a header");
    drop(sim);
    rig.app(CrosstableMessage::SetFormat(CrossFormat::AllPlayAll));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("1.5 / 2").is_ok());
}

#[test]
fn the_export_asks_for_a_file_and_the_answer_writes_it() {
    let mut rig = ready();
    let task = rig
        .shell
        .update(Message::Panels(DataPanelsMessage::PickExport(
            ExportFormat::Html,
        )));
    assert!(task.units() >= 1, "the dialog is a task");
    // Cancelled: nothing happens.
    let none = rig
        .shell
        .update(Message::Panels(DataPanelsMessage::ExportPicked(
            ExportFormat::Html,
            None,
        )));
    assert_eq!(none.units(), 0);
    // The file chosen: the application gets the export.
    let path = rig.dir.path().join("t.html");
    let task = rig
        .shell
        .update(Message::Panels(DataPanelsMessage::ExportPicked(
            ExportFormat::Html,
            Some(path.clone()),
        )));
    assert!(task.units() >= 1);
    // And the effect writes a complete document with the translated words.
    rig.app(CrosstableMessage::Export {
        format: ExportFormat::Html,
        path: path.clone(),
    });
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<!DOCTYPE html>") && html.contains("Club"));
    for (format, ext) in [
        (ExportFormat::Text, ".txt"),
        (ExportFormat::Html, ".html"),
        (ExportFormat::Latex, ".tex"),
    ] {
        let name = rig.shell.app.crosstable_export_name(format).unwrap();
        assert!(name.ends_with(ext) && name.starts_with("Club"), "{name}");
    }
}

#[test]
fn the_save_dialog_filters_by_the_extension_of_the_format() {
    let rig = Rig::new();
    for (format, ext) in [
        (ExportFormat::Text, "txt"),
        (ExportFormat::Html, "html"),
        (ExportFormat::Latex, "tex"),
    ] {
        let dialog = CrosstableSaveDialog::new(&rig.shell.i18n, format, format!("x.{ext}"));
        assert_eq!(dialog.file_name, format!("x.{ext}"));
        assert!(dialog.filters[0].1.contains(&ext), "{format:?}");
        assert_eq!(dialog.filters.len(), 2);
    }
}

#[test]
fn copy_sends_the_text_to_the_clipboard() {
    let mut rig = ready();
    let host = rig.app(CrosstableMessage::Copy);
    assert!(
        host.iter()
            .any(|e| matches!(e, Effect::WriteClipboard(t) if t.contains("Club"))),
        "{host:?}"
    );
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(rig.tr("crosstab-notice-copied").as_str()).is_ok());
}

#[test]
fn while_computing_the_panel_says_so_and_stop_ends_with_the_cancelled_notice() {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES);
    rig.load(slot, 1);
    // Opens the pane; the effect of the computation is not run.
    rig.toggle(PaneKind::Crosstable);
    assert_eq!(
        rig.shell.app.crosstable_view().body,
        CrosstableBody::Computing
    );
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(rig.tr("crosstab-computing").as_str()).is_ok());
    sim.click(rig.tr("crosstab-stop").as_str()).unwrap();
    let stop = messages(sim)
        .into_iter()
        .find(|m| {
            matches!(
                m,
                Message::App(AppMessage::Crosstable(CrosstableMessage::Cancel))
            )
        })
        .expect("Cancel");
    rig.ui(stop);
    assert_eq!(
        rig.shell.app.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::Cancelled)
    );
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(
        sim.find(rig.tr("crosstab-notice-cancelled").as_str())
            .is_ok()
    );
}

/// The request of the computation that a Refresh starts.
fn pending_request(rig: &mut Rig) -> mychess_app::RequestId {
    let effects = update(&mut rig.shell.app, CrosstableMessage::Refresh.into());
    effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Crosstable(CrosstableEffect::Compute { request, .. }) => Some(request),
            _ => None,
        })
        .expect("a computation starts")
}

#[test]
fn a_failed_or_empty_tournament_shows_its_notice() {
    for (failure, notice) in [
        (CrosstableFailure::Internal, "crosstab-notice-failed"),
        (CrosstableFailure::NoTable, "crosstab-no-table"),
        (
            CrosstableFailure::InvalidGame,
            "crosstab-notice-invalid-game",
        ),
    ] {
        let mut rig = Rig::new();
        let slot = rig.open_games(&GAMES);
        rig.load(slot, 1);
        rig.toggle(PaneKind::Crosstable);
        let request = pending_request(&mut rig);
        rig.app(CrosstableMessage::Computed {
            request,
            result: Box::new(Err(failure)),
        });
        let mut sim = rig.sim(1400.0, 900.0);
        assert!(sim.find(rig.tr(notice).as_str()).is_ok(), "{notice}");
    }
}

#[test]
fn a_game_alone_in_its_event_still_builds_a_two_player_table() {
    let mut rig = Rig::new();
    let slot = rig.open_games(&GAMES[3..4]);
    rig.load(slot, 1);
    rig.toggle(PaneKind::Crosstable);
    rig.app(CrosstableMessage::Refresh);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("Open").is_ok() && sim.find("0.0 / 1").is_ok());
}

#[test]
fn the_table_is_dimmed_and_still_there_while_a_new_one_is_computed() {
    let mut rig = ready();
    let _ = pending_request(&mut rig);
    let view = rig.shell.app.crosstable_view();
    assert!(matches!(view.body, CrosstableBody::Table(_)));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("1.5 / 2").is_ok());
}

#[test]
fn without_a_game_from_a_database_the_panel_says_so() {
    let mut rig = Rig::new();
    rig.toggle(PaneKind::Crosstable);
    rig.app(CrosstableMessage::Refresh);
    assert_eq!(
        rig.shell.app.crosstable_view().body,
        CrosstableBody::Empty(CrosstableNotice::NoGame)
    );
}

/// M7-6d review B2: "Merge" takes the game from the database of the table,
/// not from the current one.
#[test]
fn the_merge_button_uses_the_slot_of_the_table() {
    let mut rig = ready();
    let table_slot = rig.shell.app.crosstable_slot().expect("a table");
    // Another database becomes the current one, with a game in the editor.
    let path = rig.dir.path().join("other.pgn");
    std::fs::write(&path, pgn(&GAMES)).unwrap();
    rig.app(BasesMessage::Open(path));
    let other = rig.shell.app.bases.current();
    assert_ne!(other, table_slot);
    rig.load(other, 1);
    // The table is pinned to the first database.
    rig.app(CrosstableMessage::OpenGame {
        slot: table_slot,
        game: 1,
    });
    assert_eq!(rig.shell.app.crosstable_slot(), Some(table_slot));
    rig.ui(Message::Panels(DataPanelsMessage::SelectGame(Some(2))));
    let mut sim = rig.sim(1400.0, 900.0);
    sim.click(rig.tr("merge-button").as_str()).unwrap();
    let sent = format!("{:?}", messages(sim));
    let want = format!("slot: {table_slot:?}");
    assert!(
        sent.contains("Merge") && sent.contains(&want) && sent.contains("number: 2"),
        "{sent} / {want}"
    );
}
