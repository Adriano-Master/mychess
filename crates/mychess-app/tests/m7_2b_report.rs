//! Contract tests of the flow of the opening report (spec
//! `m7-2-relatorio-de-abertura.md`, criteria 21, 22, 25 and 27).
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod m7_2b_common;

use m7_2b_common::*;
use mychess_app::{ListColumn, ListMessage, MergeMessage};

fn lines(rig: &Rig) -> Vec<u32> {
    rig.state
        .opening_report_view()
        .report
        .expect("a report")
        .core
        .lines
        .iter()
        .map(|l| l.game)
        .collect()
}

#[test]
fn generate_makes_the_report_of_the_position_of_the_board() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    let view = rig.state.opening_report_view();
    assert!(view.open);
    assert_eq!(view.phase, ReportPhase::Ready);
    assert!(!view.stale);
    assert_eq!(view.report.unwrap().game_count(), 5);
    assert_eq!(lines(&rig), vec![1, 2, 3, 4, 6]);
    // The date is the one injected (D3608), the database is named.
    assert_eq!(view.database_name, Some("report.pgn"));
    assert!(
        rig.state
            .opening_report_exclude_choices()
            .contains(&"Nc6".to_owned())
    );
}

#[test]
fn update_does_not_block_it_only_asks_for_the_effect() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(OpeningReportMessage::Generate);
    assert!(Rig::compute_of(&effects).is_some());
    assert_eq!(effects.len(), 1, "nothing else is asked for: {effects:?}");
    let view = rig.state.opening_report_view();
    assert!(matches!(view.phase, ReportPhase::Generating { .. }));
    assert!(rig.state.opening_report().is_running());
}

#[test]
fn an_answer_of_an_old_request_is_ignored() {
    let (mut rig, _slot, _dir) = sample();
    let first = rig.raw(OpeningReportMessage::Generate);
    let second = rig.raw(OpeningReportMessage::Refresh);
    let old = rig.perform(Rig::compute_of(&first).unwrap());
    rig.raw(old);
    assert!(
        matches!(
            rig.state.opening_report_view().phase,
            ReportPhase::Generating { .. }
        ),
        "the old answer changes nothing"
    );
    assert!(rig.state.opening_report_view().report.is_none());
    let new = rig.perform(Rig::compute_of(&second).unwrap());
    rig.raw(new);
    assert_eq!(rig.state.opening_report_view().phase, ReportPhase::Ready);
}

#[test]
fn quit_in_the_middle_stops_the_job_and_drops_the_answer() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(OpeningReportMessage::Generate);
    let compute = Rig::compute_of(&effects).unwrap();
    rig.raw(Message::Quit);
    assert!(!rig.state.opening_report().is_running());
    let late = rig.perform(compute);
    rig.raw(late);
    assert!(rig.state.opening_report_view().report.is_none());
}

#[test]
fn closing_the_slot_in_the_middle_cancels_the_job() {
    let (mut rig, slot, _dir) = sample();
    let effects = rig.raw(OpeningReportMessage::Generate);
    let compute = Rig::compute_of(&effects).unwrap();
    rig.raw(BasesMessage::Close(slot));
    assert!(!rig.state.opening_report().is_running());
    let late = rig.perform(compute);
    rig.raw(late);
    assert!(rig.state.opening_report_view().report.is_none());
}

#[test]
fn switching_the_slot_in_the_middle_cancels_the_job() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(OpeningReportMessage::Generate);
    let compute = Rig::compute_of(&effects).unwrap();
    rig.raw(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(!rig.state.opening_report().is_running());
    let late = rig.perform(compute);
    rig.raw(late);
    assert!(rig.state.opening_report_view().report.is_none());
}

#[test]
fn stop_during_the_search_opens_the_window_without_a_report() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(OpeningReportMessage::Generate);
    rig.raw(OpeningReportMessage::Stop);
    let answer = rig.perform(Rig::compute_of(&effects).unwrap());
    rig.raw(answer);
    let view = rig.state.opening_report_view();
    assert!(view.open);
    assert_eq!(view.phase, ReportPhase::SearchInterrupted);
    assert_eq!(rig.notice(), Some(OpeningReportNotice::Interrupted));
    // The options are still usable: a new report is made.
    rig.send(OpeningReportMessage::Refresh);
    assert_eq!(rig.state.opening_report_view().phase, ReportPhase::Ready);
}

#[test]
fn the_progress_ends_complete_in_both_phases() {
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(OpeningReportMessage::Generate);
    let answer = rig.perform(Rig::compute_of(&effects).unwrap());
    // The numbers are read from the controls of the job, which the state
    // keeps until the answer is taken.
    let ReportPhase::Generating { search, generate } = rig.state.opening_report_view().phase else {
        panic!("still generating");
    };
    assert_eq!(search.0, search.1);
    assert!(search.1 > 0);
    assert_eq!(generate.0, generate.1);
    assert!(generate.1 > 0);
    rig.raw(answer);
}

#[test]
fn the_filter_of_the_slot_is_the_same_before_and_after() {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let rev = rig.state.bases.slot(slot).unwrap().filter_rev();
    rig.send(OpeningReportMessage::Generate);
    rig.send(OpeningReportMessage::SetExclude(Some("Nc6".to_owned())));
    let after = rig.state.bases.slot(slot).unwrap();
    assert_eq!(*after.filter(), before);
    assert_eq!(after.filter_rev(), rev);
    assert_eq!(after.filtered(), 7);
}

#[test]
fn a_click_sets_the_filter_with_the_ply_of_the_position() {
    let (mut rig, slot, _dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    rig.send(OpeningReportMessage::Select(ReportSelector::All));
    {
        let s = rig.state.bases.slot(slot).unwrap();
        let games: Vec<u32> = s.filter().iter_games().collect();
        assert_eq!(games, vec![1, 2, 3, 4, 6]);
        // Ply 3 of the main line: the value of the filter is ply + 1.
        assert!(games.iter().all(|g| s.filter().get(*g) == 4));
        assert_eq!(s.list.first, 1);
    }
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FilterSet));
    let id = rig
        .state
        .opening_report_view()
        .report
        .unwrap()
        .core
        .move_orders[1]
        .id;
    rig.send(OpeningReportMessage::Select(ReportSelector::MoveOrder(id)));
    let games: Vec<u32> = rig
        .state
        .bases
        .slot(slot)
        .unwrap()
        .filter()
        .iter_games()
        .collect();
    assert_eq!(games, vec![6]);
    // An endgame class: all five end with queens, rooks and minor pieces.
    rig.send(OpeningReportMessage::Select(ReportSelector::Endgame(7)));
    let count = rig.state.bases.slot(slot).unwrap().filtered();
    assert_eq!(count, 5);
}

#[test]
fn a_game_reference_loads_the_game_at_the_position() {
    let (mut rig, slot, _dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    rig.send(OpeningReportMessage::LoadGame(1));
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
    assert_eq!(
        rig.state
            .session
            .game()
            .ply(rig.state.session.cursor())
            .unwrap(),
        3
    );
}

#[test]
fn exclude_removes_the_move_from_the_theory_counts_only() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    rig.send(OpeningReportMessage::SetExclude(Some("Nc6".to_owned())));
    let view = rig.state.opening_report_view();
    let report = view.report.unwrap();
    assert_eq!(view.exclude, Some("Nc6"));
    assert_eq!(report.core.total.white, 3);
    assert_eq!(
        (
            report.core.theory.white,
            report.core.theory.draws,
            report.core.theory.black
        ),
        (1, 0, 0)
    );
    assert!(
        rig.state
            .opening_report_exclude_choices()
            .iter()
            .any(|m| m == "Nc6")
    );
}

#[test]
fn the_report_is_stale_after_the_games_change_or_the_slot_changes() {
    let (mut rig, slot, _dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    assert!(!rig.state.opening_report_view().stale);
    // Another database: stale; back again: current.
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(rig.state.opening_report_view().stale);
    rig.send(OpeningReportMessage::Select(ReportSelector::All));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::Stale));
    rig.send(BasesMessage::Select(slot));
    assert!(!rig.state.opening_report_view().stale);
    // Sorting the list is a view: the games are the same, nothing is stale.
    rig.send(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: ListColumn::Date,
    }));
    assert!(!rig.state.opening_report_view().stale);
    // The other database again, and a click is refused: the filter of the
    // slot of the report is not touched.
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    rig.send(OpeningReportMessage::Select(ReportSelector::Theme(0)));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::Stale));
    assert_eq!(*rig.state.bases.slot(slot).unwrap().filter(), before);
    rig.send(OpeningReportMessage::MergeGames);
    assert!(rig.state.merge_view().is_none());
    // "Update" makes the same report again over the database now (the
    // Clipbase here), with the same line.
    rig.send(OpeningReportMessage::Refresh);
    assert!(!rig.state.opening_report_view().stale);
    assert_eq!(rig.state.opening_report_view().phase, ReportPhase::Ready);
    assert_eq!(
        rig.state.opening_report_view().report.unwrap().game_count(),
        0
    );
}

#[test]
fn merge_games_takes_the_best_by_rating_in_one_undo_step() {
    let (mut rig, slot, _dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    let depth = rig.state.session.undo_depth();
    let before = rig.state.session.game().clone();
    rig.send(OpeningReportMessage::MergeGames);
    let view = rig.state.merge_view().expect("the confirmation");
    assert!(view.batch);
    assert_eq!(view.slot, slot);
    // By average rating, ties to the higher number (spec criterion 9).
    assert_eq!(view.numbers.iter().collect::<Vec<_>>(), vec![1, 6, 2, 3, 4]);
    rig.send(MergeMessage::Apply { comment: None });
    assert_eq!(rig.state.session.undo_depth(), depth + 1);
    assert!(!rig.state.session.game().same_content(&before));
    rig.send(GameMessage::Undo);
    assert!(rig.state.session.game().same_content(&before));
}

#[test]
fn merge_games_skips_the_game_of_the_editor() {
    let (mut rig, slot, _dir) = sample();
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    rig.send(GameMessage::Navigate(mychess_app::Nav::GoToMainlinePly(3)));
    rig.send(OpeningReportMessage::Generate);
    rig.send(OpeningReportMessage::MergeGames);
    let view = rig.state.merge_view().expect("the confirmation");
    assert_eq!(view.numbers.iter().collect::<Vec<_>>(), vec![1, 6, 3, 4]);
    assert_eq!(view.batch_report.skipped_same, 1);
}

#[test]
fn merge_games_needs_a_report() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(OpeningReportMessage::MergeGames);
    assert_eq!(rig.notice(), Some(OpeningReportNotice::NoReport));
    assert!(rig.state.merge_view().is_none());
}
