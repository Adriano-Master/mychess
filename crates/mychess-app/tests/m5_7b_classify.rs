//! M5-7b contract: the "Classify games" dialog (spec
//! `docs/specs/m5-7-eco-e-novidade.md`, D1710-D1716).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_7b_common;

use m5_7b_common::*;
use mychess_app::ListColumn;
use mychess_db::GameSource;
use tempfile::tempdir;

/// Dates and tags chosen so every selection picks a different set:
/// g1 2020.01.01 no code (B20), g2 2020.07.01 `C20` (C20), g3 2021.03.01 no
/// code (A00a), g4 2021.05.20 wrong code `E99` (B20).
fn games() -> String {
    let mut pgn = String::new();
    pgn.push_str(&game_pgn("1. e4 c5", "1-0", "2020.01.01", None));
    pgn.push_str(&game_pgn("1. e4 e5", "0-1", "2020.07.01", Some("C20")));
    pgn.push_str(&game_pgn("1. d4 d5", "1/2-1/2", "2021.03.01", None));
    pgn.push_str(&game_pgn(
        "1. e4 c5 2. Nf3",
        "1-0",
        "2021.05.20",
        Some("E99"),
    ));
    pgn
}

const BEFORE: [&str; 4] = ["", "C20", "", "E99"];

struct Si4Rig {
    rig: Rig,
    slot: SlotId,
    path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

fn si4_rig() -> Si4Rig {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "g", &games());
    rig.set_book(BOOK);
    assert_eq!(codes_on_disk(&path), BEFORE);
    Si4Rig {
        rig,
        slot,
        path,
        _dir: dir,
    }
}

impl Si4Rig {
    fn choose(&mut self, choice: ClassifyChoice) {
        self.rig.raw(eco(EcoMessage::SetClassifyChoice(choice)));
    }

    fn result(&self) -> mychess_app::ClassifyResult {
        self.rig
            .state
            .eco_classify_view()
            .expect("dialog")
            .result
            .cloned()
            .expect("a result")
    }

    fn preview(&mut self) -> Vec<Effect> {
        self.rig.send(eco(EcoMessage::ClassifyPreview))
    }

    fn run(&mut self) {
        self.rig.send(eco(EcoMessage::ClassifyRun));
    }

    fn slot_codes(&self) -> Vec<String> {
        let slot = self.rig.state.bases.slot(self.slot).unwrap();
        let db = slot.source.read();
        (1..=GameSource::len(&*db))
            .map(|n| {
                db.entry(n)
                    .unwrap()
                    .eco()
                    .map(|e| e.to_string())
                    .unwrap_or_default()
            })
            .collect()
    }
}

#[test]
fn the_dialog_opens_on_the_current_base_with_the_documented_defaults() {
    let mut s = si4_rig();
    let effects = s.rig.raw(eco(EcoMessage::OpenClassify));
    assert_eq!(
        count(&effects, |e| matches!(e, Effect::QueryToday { .. })),
        1
    );
    let view = s.rig.state.eco_classify_view().unwrap();
    assert_eq!(view.slot, s.slot);
    assert_eq!(view.choice, ClassifyChoice::NoCode, "MyChess default");
    assert!(view.extended, "extensions by default (D1711)");
    assert!(!view.today_known);
    assert!(view.can_write && view.book_loaded && !view.running);
    assert_eq!((view.total_games, view.filter_games), (4, 4));
    assert!(view.result.is_none() && !view.invalid_date);
}

#[test]
fn only_games_without_a_code_leaves_the_others_alone() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::NoCode);
    s.run();
    let r = s.result();
    assert!(!r.preview && r.error.is_none());
    assert_eq!(
        (r.report.classified, r.report.changed, r.report.written),
        (2, 2, 2)
    );
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00a", "E99"]);
}

#[test]
fn all_games_overwrites_the_old_codes() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    let r = s.result();
    assert_eq!(
        (r.report.classified, r.report.changed, r.report.written),
        (4, 3, 3)
    );
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00a", "B20"]);
}

#[test]
fn only_the_filter_touches_the_games_of_the_filter() {
    let mut s = si4_rig();
    s.rig.keep(s.slot, &[1, 4]);
    s.rig.open_classify(None);
    assert_eq!(s.rig.state.eco_classify_view().unwrap().filter_games, 2);
    s.choose(ClassifyChoice::Filter);
    s.run();
    let r = s.result();
    assert_eq!(r.report.classified, 2);
    // g3 is outside the filter and stays without a code.
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "", "B20"]);
}

#[test]
fn since_a_date_skips_the_older_games() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::Since);
    s.rig
        .raw(eco(EcoMessage::SetClassifySince("2021.01.01".to_owned())));
    s.run();
    assert_eq!(s.result().report.classified, 2);
    // g4 is inside the range, so its wrong code is replaced.
    assert_eq!(codes_on_disk(&s.path), ["", "C20", "A00a", "B20"]);
}

#[test]
fn since_accepts_a_lenient_date_with_only_the_year() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::Since);
    s.rig
        .raw(eco(EcoMessage::SetClassifySince("2021".to_owned())));
    s.preview();
    assert_eq!(s.result().report.classified, 2);
}

#[test]
fn last_year_and_last_month_keep_the_day_of_today() {
    let mut s = si4_rig();
    s.rig.open_classify(Some(date(2021, 6, 15)));
    assert!(s.rig.state.eco_classify_view().unwrap().today_known);
    // Since 2020.06.15: g2, g3, g4.
    s.choose(ClassifyChoice::LastYear);
    s.preview();
    assert_eq!(s.result().report.classified, 3);
    // Since 2021.05.15: only g4.
    s.choose(ClassifyChoice::LastMonth);
    s.preview();
    assert_eq!(s.result().report.classified, 1);
}

#[test]
fn last_month_of_january_goes_to_december_of_the_year_before() {
    let mut s = si4_rig();
    // Since 2020.12.31: g3 and g4 only (g1 and g2 are older).
    s.rig.open_classify(Some(date(2021, 1, 31)));
    s.choose(ClassifyChoice::LastMonth);
    s.preview();
    assert_eq!(s.result().report.classified, 2);
}

#[test]
fn last_year_before_the_clock_answers_runs_nothing() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    for choice in [ClassifyChoice::LastYear, ClassifyChoice::LastMonth] {
        s.choose(choice);
        let effects = s.rig.raw(eco(EcoMessage::ClassifyPreview));
        assert_eq!(count(&effects, is_classify), 0);
        assert!(s.rig.state.eco_classify_view().unwrap().result.is_none());
    }
}

#[test]
fn an_invalid_date_runs_nothing_and_flags_the_dialog_until_the_next_edit() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::Since);
    for text in ["", "   ", "abc", "????.??.??", "??.12.31", "-"] {
        s.rig
            .raw(eco(EcoMessage::SetClassifySince(text.to_owned())));
        for message in [EcoMessage::ClassifyPreview, EcoMessage::ClassifyRun] {
            let effects = s.rig.raw(eco(message));
            assert_eq!(count(&effects, is_classify), 0, "{text:?}");
            let view = s.rig.state.eco_classify_view().unwrap();
            assert!(view.invalid_date, "{text:?}");
            assert!(view.result.is_none());
        }
        // Editing clears the warning.
        s.rig
            .raw(eco(EcoMessage::SetClassifySince("2020".to_owned())));
        assert!(!s.rig.state.eco_classify_view().unwrap().invalid_date);
    }
    assert_eq!(codes_on_disk(&s.path), BEFORE, "nothing was written");
}

#[test]
fn the_preview_counts_without_writing_and_keeps_the_database_as_it_was() {
    let mut s = si4_rig();
    let bytes = std::fs::read(&s.path).unwrap();
    let source = s.rig.state.bases.slot(s.slot).unwrap().source.clone();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.preview();
    let r = s.result();
    assert!(r.preview);
    assert_eq!(
        (r.report.classified, r.report.changed, r.report.written),
        (4, 3, 0)
    );
    assert_eq!(
        std::fs::read(&s.path).unwrap(),
        bytes,
        "the file is untouched"
    );
    assert_eq!(codes_on_disk(&s.path), BEFORE);
    let slot = s.rig.state.bases.slot(s.slot).unwrap();
    assert!(slot.source == source, "the base was not reopened");
    assert_eq!(
        s.rig.notice(),
        Some(EcoNotice::Classified {
            classified: 4,
            changed: 3,
            written: 0,
            preview: true,
            stopped: false
        })
    );
}

#[test]
fn classifying_writes_reopens_the_base_and_shows_the_new_codes() {
    let mut s = si4_rig();
    let source = s.rig.state.bases.slot(s.slot).unwrap().source.clone();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00a", "B20"]);
    let slot = s.rig.state.bases.slot(s.slot).unwrap();
    assert!(slot.source != source, "the base was opened again");
    assert!(
        s.rig.state.eco_classify_view().unwrap().can_write,
        "needs_reload stays off"
    );
    assert_eq!(s.slot_codes(), ["B20", "C20", "A00a", "B20"]);
    assert_eq!(slot.total(), 4, "the number of games does not change");
    assert_eq!(
        s.rig.notice(),
        Some(EcoNotice::Classified {
            classified: 4,
            changed: 3,
            written: 3,
            preview: false,
            stopped: false
        })
    );
}

#[test]
fn basic_codes_drop_the_extension() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.rig.raw(eco(EcoMessage::SetClassifyExtended(false)));
    s.choose(ClassifyChoice::NoCode);
    s.run();
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00", "E99"]);
}

#[test]
fn classifying_twice_in_a_row_works_and_the_second_run_changes_nothing() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    s.run();
    let r = s.result();
    assert_eq!((r.report.changed, r.report.written), (0, 0));
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00a", "B20"]);
}

#[test]
fn an_order_by_eco_is_dropped_after_writing_and_another_order_stays() {
    let mut s = si4_rig();
    s.rig.send(BasesMessage::List(ListMessage::SortBy {
        slot: s.slot,
        column: ListColumn::Eco,
    }));
    assert!(matches!(
        s.rig.state.bases.slot(s.slot).unwrap().list.sort,
        Some((ListColumn::Eco, _))
    ));
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    assert_eq!(s.rig.state.bases.slot(s.slot).unwrap().list.sort, None);

    // Another column keeps its order.
    let mut s = si4_rig();
    s.rig.send(BasesMessage::List(ListMessage::SortBy {
        slot: s.slot,
        column: ListColumn::Date,
    }));
    let sort = s.rig.state.bases.slot(s.slot).unwrap().list.sort;
    assert!(matches!(sort, Some((ListColumn::Date, _))));
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    assert_eq!(s.rig.state.bases.slot(s.slot).unwrap().list.sort, sort);
}

#[test]
fn the_filter_and_the_number_of_games_survive_a_classification() {
    let mut s = si4_rig();
    s.rig.keep(s.slot, &[2, 3]);
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    let slot = s.rig.state.bases.slot(s.slot).unwrap();
    assert_eq!((slot.total(), slot.filtered()), (4, 2));
}

// ---- who can write ----

#[test]
fn a_pgn_base_only_has_the_preview() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_pgn(dir.path(), "g.pgn", &games());
    rig.set_book(BOOK);
    let bytes = std::fs::read(dir.path().join("g.pgn")).unwrap();
    rig.raw(eco(EcoMessage::OpenClassify));
    assert!(!rig.state.eco_classify_view().unwrap().can_write);
    let effects = rig.raw(eco(EcoMessage::ClassifyRun));
    assert_eq!(count(&effects, is_classify), 0);
    assert_eq!(rig.notice(), Some(EcoNotice::ReadOnly));
    rig.raw(eco(EcoMessage::SetClassifyChoice(ClassifyChoice::All)));
    rig.send(eco(EcoMessage::ClassifyPreview));
    let view = rig.state.eco_classify_view().unwrap();
    let result = view.result.unwrap();
    assert!(result.preview && result.report.classified == 4);
    assert_eq!(std::fs::read(dir.path().join("g.pgn")).unwrap(), bytes);
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 4);
}

#[test]
fn the_clipbase_only_has_the_preview() {
    let mut rig = Rig::new();
    rig.set_book(BOOK);
    assert!(rig.state.bases.current().is_clipbase());
    rig.raw(eco(EcoMessage::OpenClassify));
    assert!(!rig.state.eco_classify_view().unwrap().can_write);
    let effects = rig.raw(eco(EcoMessage::ClassifyRun));
    assert_eq!(count(&effects, is_classify), 0);
    assert_eq!(rig.notice(), Some(EcoNotice::ReadOnly));
    rig.send(eco(EcoMessage::ClassifyPreview));
    assert!(rig.state.eco_classify_view().unwrap().result.is_some());
}

#[test]
fn a_read_only_si4_only_has_the_preview_and_is_not_touched() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let path = make_si4(dir.path(), "ro", &games());
    let files: Vec<_> = ["si4", "sn4", "sg4"]
        .iter()
        .map(|e| path.with_extension(e))
        .collect();
    for f in &files {
        let mut perms = std::fs::metadata(f).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(f, perms).unwrap();
    }
    rig.send(BasesMessage::Open(path.clone()));
    let slot = rig.state.bases.current();
    assert!(rig.state.bases.slot(slot).unwrap().read_only);
    rig.set_book(BOOK);
    rig.raw(eco(EcoMessage::OpenClassify));
    assert!(!rig.state.eco_classify_view().unwrap().can_write);
    let effects = rig.raw(eco(EcoMessage::ClassifyRun));
    assert_eq!(count(&effects, is_classify), 0);
    assert_eq!(rig.notice(), Some(EcoNotice::ReadOnly));
    rig.raw(eco(EcoMessage::SetClassifyChoice(ClassifyChoice::All)));
    rig.send(eco(EcoMessage::ClassifyPreview));
    assert_eq!(
        rig.state
            .eco_classify_view()
            .unwrap()
            .result
            .unwrap()
            .report
            .changed,
        3
    );
    assert_eq!(codes_on_disk(&path), BEFORE);
    for f in &files {
        let mut perms = std::fs::metadata(f).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(f, perms).unwrap();
    }
}

#[test]
fn without_a_book_neither_button_starts_anything() {
    let mut s = si4_rig();
    s.rig.state.eco_book = None;
    s.rig.open_classify(None);
    assert!(!s.rig.state.eco_classify_view().unwrap().book_loaded);
    for message in [EcoMessage::ClassifyPreview, EcoMessage::ClassifyRun] {
        let effects = s.rig.raw(eco(message));
        assert_eq!(count(&effects, is_classify), 0);
        assert_eq!(s.rig.notice(), Some(EcoNotice::NoBook));
        s.rig.clear_notice();
    }
    assert_eq!(codes_on_disk(&s.path), BEFORE);
}

// ---- jobs: progress, stop, one at a time ----

fn classify_effect(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(is_classify)
        .expect("an EcoClassify effect")
}

#[test]
fn classifying_is_a_job_of_the_list_with_progress_and_one_at_a_time() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    assert!(s.rig.state.list_job().is_none());
    let effects = s.rig.raw(eco(EcoMessage::ClassifyRun));
    let effect = classify_effect(effects);
    let (kind, control) = s.rig.state.list_job().expect("a job");
    assert_eq!(kind, ListJobKind::EcoClassify);
    assert!(!control.is_cancelled());
    assert!(s.rig.state.eco_classify_view().unwrap().running);
    // One job at a time: neither button starts a second one.
    for message in [EcoMessage::ClassifyPreview, EcoMessage::ClassifyRun] {
        assert_eq!(count(&s.rig.raw(eco(message)), is_classify), 0);
    }
    let answer = s.rig.perform(effect);
    s.rig.send(answer);
    assert!(s.rig.state.list_job().is_none());
    assert!(!s.rig.state.eco_classify_view().unwrap().running);
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00a", "B20"]);
}

#[test]
fn stop_before_the_first_game_leaves_the_file_as_it_was_and_the_base_usable() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    let effect = classify_effect(s.rig.raw(eco(EcoMessage::ClassifyRun)));
    s.rig
        .raw(Message::Bases(BasesMessage::List(ListMessage::CancelJob)));
    assert!(s.rig.state.list_job().unwrap().1.is_cancelled());
    let answer = s.rig.perform(effect);
    s.rig.send(answer);
    assert!(s.rig.state.list_job().is_none(), "the job is over");
    assert!(!s.rig.state.eco_classify_view().unwrap().running);
    assert_eq!(codes_on_disk(&s.path), BEFORE);
    assert!(s.rig.state.eco_classify_view().unwrap().can_write);
    // The base can be classified again.
    s.run();
    assert_eq!(codes_on_disk(&s.path), ["B20", "C20", "A00a", "B20"]);
}

/// A base big enough for the run to take several batches of 1000 games.
fn big_games(n: usize) -> String {
    let mut pgn = String::new();
    for i in 0..n {
        let moves = if i % 2 == 0 { "1. e4 c5" } else { "1. e4 e5" };
        pgn.push_str(&game_pgn(moves, "1-0", "2020.01.01", None));
    }
    pgn
}

#[test]
fn stopping_a_long_run_keeps_what_was_written_and_the_slot_shows_the_disk() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "big", &big_games(3000));
    rig.set_book(BOOK);
    rig.open_classify(None);
    rig.raw(eco(EcoMessage::SetClassifyChoice(ClassifyChoice::All)));
    let effect = classify_effect(rig.raw(eco(EcoMessage::ClassifyRun)));
    // Cancel from another thread as soon as the run reports progress. The
    // run may or may not still be going; both ends must be coherent.
    let answer = std::thread::scope(|scope| {
        let control = rig.state.list_job().unwrap().1;
        let stopper = scope.spawn(|| {
            let start = std::time::Instant::now();
            while control.progress().0 == 0 && start.elapsed().as_secs() < 20 {
                std::thread::yield_now();
            }
            control.cancel();
        });
        let answer = rig.perform(effect);
        stopper.join().unwrap();
        answer
    });
    rig.send(answer);
    let codes = codes_on_disk(&path);
    let written_on_disk = codes.iter().filter(|c| !c.is_empty()).count();
    let view = rig.state.eco_classify_view().unwrap();
    match view.result {
        Some(result) => {
            assert_eq!(
                result.report.written as usize, written_on_disk,
                "the report says what is on disk (cancelled: {})",
                result.report.cancelled
            );
            if result.report.cancelled {
                assert!(written_on_disk < 3000);
            } else {
                assert_eq!(written_on_disk, 3000);
            }
        }
        None => assert_eq!(written_on_disk, 0, "stopped before any game"),
    }
    // Whatever happened the slot shows the file and the game count is intact.
    let slot_ref = rig.state.bases.slot(slot).unwrap();
    assert_eq!(slot_ref.total(), 3000);
    let db = slot_ref.source.read();
    let in_slot = (1..=GameSource::len(&*db))
        .filter(|n| db.entry(*n).unwrap().eco().is_some())
        .count();
    assert_eq!(in_slot, written_on_disk);
}

#[test]
fn closing_the_dialog_cancels_a_run_in_flight() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    let effect = classify_effect(s.rig.raw(eco(EcoMessage::ClassifyRun)));
    s.rig.raw(eco(EcoMessage::CloseClassify));
    assert_eq!(s.rig.state.eco_window(), None);
    assert!(s.rig.state.list_job().unwrap().1.is_cancelled());
    // The late answer is absorbed without opening anything.
    let answer = s.rig.perform(effect);
    s.rig.send(answer);
    assert_eq!(s.rig.state.eco_window(), None);
    assert!(s.rig.state.list_job().is_none());
}

#[test]
fn closing_a_dialog_that_is_not_the_job_owner_does_not_cancel_the_job_of_another_kind() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    let effect = classify_effect(s.rig.raw(eco(EcoMessage::ClassifyRun)));
    // Closing the novelty dialog (not open) must not cancel the classify job.
    s.rig.raw(eco(EcoMessage::CloseNovelty));
    assert!(!s.rig.state.list_job().unwrap().1.is_cancelled());
    let answer = s.rig.perform(effect);
    s.rig.send(answer);
}

#[test]
fn choices_change_the_dialog_and_clear_the_last_result() {
    let mut s = si4_rig();
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.preview();
    assert!(s.rig.state.eco_classify_view().unwrap().result.is_some());
    s.choose(ClassifyChoice::NoCode);
    assert!(s.rig.state.eco_classify_view().unwrap().result.is_none());
}

#[test]
fn classify_messages_with_no_dialog_open_do_nothing() {
    let mut rig = Rig::new();
    for m in [
        EcoMessage::CloseClassify,
        EcoMessage::SetClassifyChoice(ClassifyChoice::All),
        EcoMessage::SetClassifySince("2020".to_owned()),
        EcoMessage::SetClassifyExtended(false),
        EcoMessage::ClassifyPreview,
        EcoMessage::ClassifyRun,
    ] {
        assert!(rig.raw(eco(m)).is_empty());
    }
    assert_eq!(rig.state.eco_window(), None);
}

#[test]
fn a_browser_opened_over_the_classified_base_rebuilds_its_table_when_the_base_is_reopened() {
    let mut s = si4_rig();
    s.rig.open_browser();
    assert_eq!(s.rig.browser().classified, 2, "C20 and E99 before");
    // Classify from the dialog (opening it closes the browser) and reopen.
    s.rig.open_classify(None);
    s.choose(ClassifyChoice::All);
    s.run();
    s.rig.send(eco(EcoMessage::CloseClassify));
    s.rig.open_browser();
    assert_eq!(s.rig.browser().classified, 4);
}
