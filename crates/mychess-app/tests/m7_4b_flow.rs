//! M7-4b contract: the flow of the extra formats - the questions about
//! existing files, protection of the open databases, answers by request,
//! Stop, Quit and closing a slot in the middle (spec
//! `docs/specs/m7-4-exportacao.md`, criterion 14, rules A.1 and A.4,
//! D3722, D3728 and `M7-4/D3749`-`D3756`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m7_4b_common;

use m7_4b_common::*;
use mychess_app::{BaseError, ConfirmKind, OpenControl, SlotId};
use tempfile::tempdir;

fn rig_with(dir: &std::path::Path, text: &str) -> Rig {
    let mut rig = Rig::new();
    rig.open_si4(dir, "mybase", text);
    rig.set_assets();
    rig
}

fn confirm_view(rig: &Rig) -> Option<(ConfirmKind, ExportFormat, Vec<std::path::PathBuf>)> {
    rig.state
        .export_docs_view()
        .confirm
        .map(|c| (c.kind, c.format, c.files.to_vec()))
}

const OLD_HTML: &str = "<html>\n<body>\n<p>old games</p>\n</body>\n</html>\n";

// ---- update never writes: the effect does ----

#[test]
fn update_only_asks_for_the_job_and_writes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Latex);
    let dest = dir.path().join("lazy.tex");
    let (effects, _) = rig.gx_choose(&dest);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ExportDocs { .. })),
        "{effects:?}"
    );
    // The update already ran; the file is only written by the effect.
    assert!(!dest.exists());
    assert!(
        std::fs::read_dir(dir.path()).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tex")),
        "no .tex file or temporary yet"
    );
    assert!(matches!(
        rig.state.games_export_view().unwrap().step,
        GamesExportStepView::Running { .. }
    ));
    rig.gx_run(effects);
    assert!(dest.exists());
}

// ---- preferences ----

#[test]
fn the_format_and_the_options_are_remembered_in_the_preferences() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    // Without the dialog open nothing changes.
    rig.format(ExportFormat::Csv);
    assert_eq!(rig.state.prefs.export_docs.format, ExportFormat::Pgn);
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Latex);
    rig.edit(ExportEdit::LatexFlip(true));
    assert_eq!(rig.state.prefs.export_docs.format, ExportFormat::Latex);
    assert!(rig.state.prefs.export_docs.latex.flip);
    let view = rig.state.export_docs_view();
    assert_eq!(view.prefs.format, ExportFormat::Latex);
    assert!(view.has_assets);
}

#[test]
fn numbers_are_limited_to_their_ranges() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    rig.edit(ExportEdit::ViewerPerFile(0));
    assert_eq!(rig.state.prefs.export_docs.viewer.per_file, 1);
    rig.edit(ExportEdit::ViewerPerFile(1_000_000));
    assert_eq!(rig.state.prefs.export_docs.viewer.per_file, 5000);
    rig.edit(ExportEdit::HtmlDiagramPx(60_000));
    assert_eq!(rig.state.prefs.export_docs.html.diagram_px, 1000);
}

#[test]
fn the_options_are_frozen_when_the_job_starts() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    let dest = dir.path().join("frozen.html");
    let (effects, _) = rig.gx_choose(&dest);
    // Another change of the preferences while the job is queued...
    let mut changed = (*rig.state.prefs.export_docs).clone();
    changed.html.comments = false;
    changed.html.diagram_px = 0;
    rig.raw(PrefsMessage::Set(PrefChange::ExportDocs(Box::new(changed))));
    assert!(!rig.state.prefs.export_docs.html.comments);
    rig.gx_run(effects);
    // ...does not reach the job, which wrote with the options it was given.
    let text = text_of(&dest);
    assert!(text.contains("class=\"c\""), "comments kept");
    assert!(text.contains("<svg "), "diagrams kept");
}

#[test]
fn edits_during_the_job_are_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Csv);
    let (effects, _) = rig.gx_choose(&dir.path().join("busy.csv"));
    rig.edit(ExportEdit::CsvBom(true));
    rig.format(ExportFormat::Html);
    assert!(!rig.state.prefs.export_docs.csv.bom);
    assert_eq!(rig.state.prefs.export_docs.format, ExportFormat::Csv);
    rig.gx_run(effects);
}

// ---- the open database is never overwritten (D2013) ----

#[test]
fn the_pgn_next_to_a_viewer_never_replaces_an_open_pgn_database() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    let base = write(dir.path(), "games.pgn", &numbered(2));
    rig.send(BasesMessage::Open(base.clone()));
    let before = std::fs::read(&base).unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    // `games.html` goes with `games.pgn`, which is the open database.
    let (effects, _) = rig.gx_choose(&dir.path().join("games.html"));
    rig.gx_run(effects);
    assert_eq!(std::fs::read(&base).unwrap(), before);
    assert!(
        !dir.path().join("games.html").exists(),
        "nothing was written"
    );
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Failed(BaseError::ExportOverBase))
    );
}

#[test]
fn a_destination_that_is_the_open_file_is_refused_in_any_letter_case() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_assets();
    // A database whose file is an HTML name: exporting HTML over it.
    let base = write(dir.path(), "book.html", &numbered(2));
    rig.send(BasesMessage::Open(base.clone()));
    assert_eq!(
        rig.state.bases.current_slot().and_then(|s| s.path.clone()),
        Some(base.clone()),
        "the premise: the HTML-named file is open as a database"
    );
    let before = std::fs::read(&base).unwrap();
    for name in ["book.html", "BOOK.HTML", "Book"] {
        rig.gx(GamesExportMessage::Open);
        rig.format(ExportFormat::Html);
        let (effects, _) = rig.gx_choose(&dir.path().join(name));
        rig.gx_run(effects);
        assert_eq!(std::fs::read(&base).unwrap(), before, "{name}");
        assert_eq!(
            docs_notice(&rig),
            Some(GamesExportNotice::Failed(BaseError::ExportOverBase)),
            "{name}"
        );
        rig.gx(GamesExportMessage::Close);
    }
}

// ---- confirmation before replacing or appending (D3750) ----

#[test]
fn appending_to_an_existing_file_asks_first_and_adds_before_the_closing_lines() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let dest = dir.path().join("log.html");
    std::fs::write(&dest, OLD_HTML).unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, kind) = rig.gx_choose(&dest);
    assert_eq!(
        kind,
        PickKind::ExportDoc {
            format: ExportFormat::Html,
            append: true
        }
    );
    rig.gx_run(effects);
    // The job found the file and wrote nothing: the user is asked, with the
    // name of the file and what will happen to it.
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), OLD_HTML);
    assert_eq!(
        confirm_view(&rig),
        Some((ConfirmKind::Append, ExportFormat::Html, vec![dest.clone()]))
    );
    assert!(rig.state.modal_open());
    assert!(rig.state.list_job().is_none());
    // Closing the window does not escape the question.
    rig.gx(GamesExportMessage::Close);
    assert!(rig.state.games_export_view().is_some());
    // Yes: the games go in, the old part is kept byte for byte.
    let effects = rig.raw(ExportDocsMessage::Confirm(true));
    assert!(confirm_view(&rig).is_none());
    rig.gx_run(effects);
    let text = text_of(&dest);
    assert!(
        text.starts_with("<html>\n<body>\n<p>old games</p>\n"),
        "{text}"
    );
    assert_eq!(text.matches("<article").count(), 2);
    assert!(text.trim_end().ends_with("</html>"));
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Exported {
            count: 2,
            name: "log.html".to_owned()
        })
    );
}

#[test]
fn answering_no_leaves_the_file_alone_and_starts_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let dest = dir.path().join("keep.tex");
    std::fs::write(&dest, "\\begin{document}\nold\n\\end{document}\n").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Latex);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    assert!(confirm_view(&rig).is_some());
    let effects = rig.raw(ExportDocsMessage::Confirm(false));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(confirm_view(&rig).is_none());
    assert!(rig.state.list_job().is_none());
    assert_eq!(
        std::fs::read_to_string(&dest).unwrap(),
        "\\begin{document}\nold\n\\end{document}\n"
    );
    // An answer with no question open does nothing.
    assert!(rig.raw(ExportDocsMessage::Confirm(true)).is_empty());
}

#[test]
fn a_name_fixed_by_the_extension_that_exists_is_asked_about_before_replacing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let existing = dir.path().join("out.csv");
    std::fs::write(&existing, "old").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Csv);
    // The dialog confirmed `out`, not `out.csv`.
    let (effects, _) = rig.gx_choose(&dir.path().join("out"));
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "old");
    assert_eq!(
        confirm_view(&rig),
        Some((
            ConfirmKind::Replace,
            ExportFormat::Csv,
            vec![existing.clone()]
        ))
    );
    let effects = rig.raw(ExportDocsMessage::Confirm(true));
    rig.gx_run(effects);
    assert!(text_of(&existing).contains("W1 & Co"));
}

#[test]
fn a_file_the_dialog_confirmed_is_replaced_without_a_second_question() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let dest = dir.path().join("same.csv");
    std::fs::write(&dest, "old").unwrap();
    rig.export_as(ExportFormat::Csv, &dest);
    assert!(confirm_view(&rig).is_none());
    assert!(text_of(&dest).contains("W1 & Co"));
}

#[test]
fn the_viewer_asks_before_replacing_the_pgn_next_to_it() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let pgn = dir.path().join("show.pgn");
    std::fs::write(&pgn, "an unrelated file").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    let (effects, _) = rig.gx_choose(&dir.path().join("show.html"));
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&pgn).unwrap(), "an unrelated file");
    assert!(!dir.path().join("show.html").exists());
    assert_eq!(
        confirm_view(&rig),
        Some((
            ConfirmKind::Replace,
            ExportFormat::Viewer,
            vec![pgn.clone()]
        ))
    );
    let effects = rig.raw(ExportDocsMessage::Confirm(true));
    rig.gx_run(effects);
    assert!(dir.path().join("show.html").exists());
    assert_eq!(whites_in(&text_of(&pgn)).len(), 2);
}

#[test]
fn the_viewer_asks_before_replacing_the_other_volumes_too() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let two = dir.path().join("show_2.html");
    let three = dir.path().join("show_3.html");
    std::fs::write(&two, "old two").unwrap();
    std::fs::write(&three, "old three").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    let (effects, _) = rig.gx_choose(&dir.path().join("show.html"));
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&two).unwrap(), "old two");
    assert_eq!(
        confirm_view(&rig),
        Some((
            ConfirmKind::Replace,
            ExportFormat::Viewer,
            vec![two.clone(), three.clone()]
        ))
    );
    let effects = rig.raw(ExportDocsMessage::Confirm(false));
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&three).unwrap(), "old three");
}

#[test]
fn the_viewer_pgn_question_names_the_file_the_writer_sanitizes() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    // The writer turns `a:b` into `a_b`.
    let pgn = dir.path().join("a_b.pgn");
    std::fs::write(&pgn, "unrelated").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    let (effects, _) = rig.gx_choose(&dir.path().join("a:b.html"));
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&pgn).unwrap(), "unrelated");
    assert_eq!(
        confirm_view(&rig),
        Some((ConfirmKind::Replace, ExportFormat::Viewer, vec![pgn]))
    );
}

#[test]
fn appending_to_a_file_without_the_closing_line_changes_nothing_and_says_why() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    let dest = dir.path().join("plain.html");
    std::fs::write(&dest, "just some text\n").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    rig.gx(GamesExportMessage::SetAppend(true));
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    let effects = rig.raw(ExportDocsMessage::Confirm(true));
    rig.gx_run(effects);
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "just some text\n");
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Problem(ExportProblem::NoAppendMarker))
    );
    assert!(!has_temp(dir.path()));
    assert!(rig.state.games_export_view().is_some(), "the window stays");
}

#[test]
fn appending_to_a_file_that_does_not_exist_needs_no_question() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Epd);
    rig.edit(ExportEdit::EpdFinalPosition(true));
    rig.gx(GamesExportMessage::SetAppend(true));
    let dest = dir.path().join("new.epd");
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    assert!(confirm_view(&rig).is_none());
    assert_eq!(text_of(&dest).lines().count(), 1);
}

// ---- answers by request, Stop, Quit, slots ----

#[test]
fn an_answer_for_another_request_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Html);
    let dest = dir.path().join("late.html");
    let (effects, _) = rig.gx_choose(&dest);
    let wrong = Message::Bases(BasesMessage::List(ListMessage::GamesExported {
        request: mychess_app::RequestId(987_654),
        result: Err(BaseError::Cancelled),
    }));
    rig.raw(wrong);
    // Still running: the stale answer neither closed the window nor ended
    // the job.
    assert!(rig.state.list_job().is_some());
    assert!(matches!(
        rig.state.games_export_view().unwrap().step,
        GamesExportStepView::Running { .. }
    ));
    rig.gx_run(effects);
    assert!(dest.exists());
}

#[test]
fn stop_before_the_job_runs_writes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Viewer);
    let dest = dir.path().join("stopped.html");
    let (effects, _) = rig.gx_choose(&dest);
    rig.raw(Message::Bases(BasesMessage::List(ListMessage::CancelJob)));
    assert!(docs_control(&effects).is_cancelled());
    rig.gx_run(effects);
    assert!(!dest.exists());
    assert!(!dir.path().join("stopped.pgn").exists());
    assert_eq!(docs_notice(&rig), Some(GamesExportNotice::Stopped));
    assert!(rig.state.games_export_view().is_none());
}

#[test]
fn stop_in_the_middle_leaves_the_old_file_and_no_temporary() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &numbered(30_000));
    let dest = dir.path().join("mid.tex");
    std::fs::write(&dest, "old").unwrap();
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Latex);
    let (effects, _) = rig.gx_choose(&dest);
    let control = docs_control(&effects);
    let watcher = {
        let control = control.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            while control.progress().0 == 0 && started.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            control.cancel();
        })
    };
    rig.gx_run(effects);
    watcher.join().unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
    assert!(!has_temp(dir.path()));
    assert_eq!(docs_notice(&rig), Some(GamesExportNotice::Stopped));
}

#[test]
fn progress_never_goes_back_and_ends_at_the_total() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &numbered(20_000));
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Epd);
    rig.edit(ExportEdit::EpdFinalPosition(true));
    let (effects, _) = rig.gx_choose(&dir.path().join("p.epd"));
    let control = docs_control(&effects);
    let sampler = {
        let control: OpenControl = control.clone();
        std::thread::spawn(move || {
            let mut samples = Vec::new();
            let started = std::time::Instant::now();
            while started.elapsed().as_secs() < 60 {
                let (done, total) = control.progress();
                samples.push((done, total));
                if total > 0 && done >= total {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
            samples
        })
    };
    rig.gx_run(effects);
    let samples = sampler.join().unwrap();
    assert!(
        samples.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {samples:?}"
    );
    let (done, total) = *samples.last().unwrap();
    assert_eq!((done, total), (20_000, 20_000));
}

#[test]
fn quitting_during_the_export_cancels_it_and_the_quit_goes_on_after_the_answer() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(dir.path(), &rich_pgn());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Latex);
    let dest = dir.path().join("q.tex");
    let (effects, _) = rig.gx_choose(&dest);
    let control = docs_control(&effects);
    let quit = rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    assert!(
        quit.iter()
            .any(|e| matches!(e, Effect::QuitDeadline { .. }))
    );
    assert!(!quit.iter().any(|e| matches!(e, Effect::Exit)));
    let mut exits = false;
    for effect in effects {
        let answer = rig.perform(effect);
        exits |= rig.raw(answer).iter().any(|e| matches!(e, Effect::Exit));
    }
    assert!(exits, "the quit goes on once the job answered");
    assert!(!dest.exists());
    assert!(!has_temp(dir.path()));
}

/// Two databases open; the first one is the current one.
fn two_bases(dir: &std::path::Path) -> (Rig, SlotId, SlotId) {
    let mut rig = rig_with(dir, &rich_pgn());
    let first = rig.state.bases.current();
    let other = write(dir, "other.pgn", &numbered(5));
    rig.send(BasesMessage::Open(other));
    let second = rig.state.bases.current();
    assert_ne!(first, second);
    rig.send(BasesMessage::Select(first));
    assert_eq!(rig.state.bases.current(), first);
    (rig, first, second)
}

#[test]
fn the_window_keeps_the_slot_of_the_job_from_being_closed_or_left() {
    // The dialog is modal: while it is open (and so while the job runs)
    // neither selecting another database nor closing this one has any
    // effect, so the job never loses the games it reads. (A slot closed by
    // other means cancels the job through `PendingListJob::closing`, covered
    // by the tests of M3-4.)
    let dir = tempdir().unwrap();
    let (mut rig, first, second) = two_bases(dir.path());
    rig.gx(GamesExportMessage::Open);
    rig.format(ExportFormat::Csv);
    let dest = dir.path().join("closing.csv");
    let (effects, _) = rig.gx_choose(&dest);
    let control = docs_control(&effects);
    assert!(rig.raw(BasesMessage::Select(second)).is_empty());
    assert_eq!(rig.state.bases.current(), first, "the slot did not change");
    assert!(rig.raw(BasesMessage::Close(first)).is_empty());
    assert!(
        rig.state.bases.slot(first).is_some(),
        "the slot is still open"
    );
    assert!(!control.is_cancelled());
    rig.gx_run(effects);
    let text = text_of(&dest);
    assert_eq!(
        text.lines().count(),
        3,
        "header and the two games of the first base: {text}"
    );
    assert!(text.contains("W1 & Co") && !text.contains("P1"));
    assert_eq!(
        docs_notice(&rig),
        Some(GamesExportNotice::Exported {
            count: 2,
            name: "closing.csv".to_owned()
        })
    );
}

#[test]
fn opening_the_browser_is_asked_only_when_the_user_chose_it() {
    for open_after in [false, true] {
        let dir = tempdir().unwrap();
        let mut rig = rig_with(dir.path(), &rich_pgn());
        rig.gx(GamesExportMessage::Open);
        rig.format(ExportFormat::Viewer);
        rig.edit(ExportEdit::ViewerOpenAfter(open_after));
        let dest = dir.path().join(format!("open{open_after}.html"));
        let (effects, _) = rig.gx_choose(&dest);
        let job = effects
            .into_iter()
            .find(|e| matches!(e, Effect::ExportDocs { .. }))
            .unwrap();
        let answer = rig.perform(job);
        let after = rig.raw(answer);
        let opens: Vec<_> = after
            .iter()
            .filter_map(|e| match e {
                Effect::OpenExported { path } => Some(path.clone()),
                _ => None,
            })
            .collect();
        if open_after {
            assert_eq!(opens, std::slice::from_ref(&dest));
        } else {
            assert!(opens.is_empty());
        }
        assert!(dest.exists());
    }
}
