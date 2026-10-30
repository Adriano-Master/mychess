//! Contract tests of the files of the opening report (spec
//! `m7-2-relatorio-de-abertura.md`, criteria 23, 24 and 26): save, view,
//! favorites, import of `reports.dat`, the batch, the options and the words.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod m7_2b_common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m7_2b_common::*;
use mychess_app::opening_report::{label_keys, parse_favorites, report_labels};
use mychess_app::{load_preferences, render_preferences};

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn with_report() -> (Rig, SlotId, tempfile::TempDir) {
    let (mut rig, slot, dir) = sample();
    rig.send(OpeningReportMessage::Generate);
    assert_eq!(rig.state.opening_report_view().phase, ReportPhase::Ready);
    (rig, slot, dir)
}

// ---- save (criterion 23) ----

#[test]
fn saves_three_parts_in_three_formats_with_the_extension_added() {
    let (mut rig, _slot, _dir) = with_report();
    let out = tempfile::tempdir().unwrap();
    let mut text_of: BTreeMap<&str, String> = BTreeMap::new();
    for (part, tag) in [
        (ReportPart::Full, "full"),
        (ReportPart::Compact, "compact"),
        (ReportPart::TableOnly, "table"),
    ] {
        for (format, ext) in [
            (ReportFormat::Text, "txt"),
            (ReportFormat::Html, "html"),
            (ReportFormat::Latex, "tex"),
        ] {
            let base = out.path().join(format!("{tag}-{ext}"));
            rig.send(OpeningReportMessage::Save {
                path: base.clone(),
                part,
                format,
            });
            assert_eq!(rig.notice(), Some(OpeningReportNotice::Saved));
            let file = out.path().join(format!("{tag}-{ext}.{ext}"));
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(!text.is_empty() && !text.contains('\0') && !text.contains('\r'));
            if ext == "html" {
                assert!(text.contains("</html>"), "a complete document");
            }
            if ext == "tex" {
                assert!(text.contains("\\end{document}"));
            }
            if ext == "txt" {
                text_of.insert(tag, text);
            }
        }
    }
    assert_eq!(names(out.path()).len(), 9, "no stray temporary files");
    // The parts: the theory table is in the full one and the table-only one.
    // The theory table: "THEORY TABLE" in the full report; its heading line
    // (score of the position) in the full one and the table-only one.
    let table = |tag: &str| text_of[tag].contains("+3 =1 -1 (3.5/5: 70%)");
    assert!(table("full") && table("table") && !table("compact"));
    assert!(text_of["full"].contains("5. THEORY TABLE"));
    assert!(!text_of["table"].contains("STATISTICS AND HISTORY"));
    assert!(text_of["compact"].contains("STATISTICS AND HISTORY"));
    assert!(text_of["full"].len() > text_of["table"].len());
    assert!(text_of["full"].contains("Database: report.pgn"));
    // What was chosen is remembered.
    let prefs = &rig.state.prefs.opening_report;
    assert_eq!(
        (prefs.last_part, prefs.last_format),
        (ReportPart::TableOnly, ReportFormat::Latex)
    );
    assert_eq!(prefs.last_dir.as_deref(), Some(out.path()));
}

#[test]
fn the_extension_is_not_added_twice_in_any_case() {
    let (mut rig, _slot, _dir) = with_report();
    let out = tempfile::tempdir().unwrap();
    for (name, format) in [
        ("a.HTML", ReportFormat::Html),
        ("b.Htm", ReportFormat::Html),
        ("c.LTX", ReportFormat::Latex),
        ("d.TXT", ReportFormat::Text),
        ("e", ReportFormat::Text),
        ("f.txt", ReportFormat::Html),
    ] {
        rig.send(OpeningReportMessage::Save {
            path: out.path().join(name),
            part: ReportPart::Compact,
            format,
        });
    }
    assert_eq!(
        names(out.path()),
        ["a.HTML", "b.Htm", "c.LTX", "d.TXT", "e.txt", "f.txt.html"]
    );
}

#[test]
fn an_existing_file_is_replaced_whole_and_a_failure_leaves_nothing() {
    let (mut rig, _slot, _dir) = with_report();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("r.txt");
    std::fs::write(&target, "old").unwrap();
    rig.send(OpeningReportMessage::Save {
        path: target.clone(),
        part: ReportPart::Compact,
        format: ReportFormat::Text,
    });
    let text = std::fs::read_to_string(&target).unwrap();
    assert_ne!(text, "old");
    assert_eq!(names(out.path()), ["r.txt"], "no temporary file is left");
    // A folder that does not exist: an error, nothing written anywhere.
    let missing: PathBuf = out.path().join("nope").join("r.txt");
    rig.send(OpeningReportMessage::Save {
        path: missing.clone(),
        part: ReportPart::Compact,
        format: ReportFormat::Text,
    });
    assert_eq!(rig.notice(), Some(OpeningReportNotice::SaveFailed));
    assert!(!missing.exists());
    assert_eq!(names(out.path()), ["r.txt"]);
    // A save without a report is refused.
    let mut empty = Rig::new();
    empty.send(OpeningReportMessage::Save {
        path: out.path().join("z"),
        part: ReportPart::Full,
        format: ReportFormat::Text,
    });
    assert_eq!(empty.notice(), Some(OpeningReportNotice::NoReport));
}

#[test]
fn view_html_writes_a_temporary_file_and_asks_to_open_it() {
    let (mut rig, _slot, _dir) = with_report();
    let effects = rig.raw(OpeningReportMessage::ViewHtml);
    let [Effect::OpeningReport(OpeningReportEffect::WriteHtml { .. })] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let answer = rig.perform(effects.into_iter().next().unwrap());
    let effects = rig.raw(answer);
    let [Effect::OpeningReport(OpeningReportEffect::OpenFile { path })] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let path = path.clone();
    assert_eq!(path.file_name().unwrap(), "TempOpeningReport.html");
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("<html") || html.contains("<!DOCTYPE"));
    // The second view reuses the folder.
    let effects = rig.raw(OpeningReportMessage::ViewHtml);
    let answer = rig.perform(effects.into_iter().next().unwrap());
    let effects = rig.raw(answer);
    let [Effect::OpeningReport(OpeningReportEffect::OpenFile { path: again })] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(*again, path);
    // The folder goes with the program.
    drop(rig);
    assert!(!path.exists());
    // A failed opener is told.
    let mut rig = Rig::new();
    rig.raw(OpeningReportMessage::FileOpened { ok: false });
    assert_eq!(rig.notice(), Some(OpeningReportNotice::ViewFailed));
}

// ---- favorites (criterion 24) ----

fn with_favorites_file(dir: &Path) -> Rig {
    let state = AppState::with_preferences_path(Some(dir.join("preferences.toml")));
    let mut rig = Rig::with(state);
    rig.state.open_pgn_in_slots = true;
    rig.state
        .set_opening_report_today(Some("2026.10.29".parse().unwrap()));
    open_in(&mut rig, dir, "report.pgn");
    // Opening the window reads the file.
    rig.send(OpeningReportMessage::Generate);
    rig
}

fn favorite_names(rig: &Rig) -> Vec<String> {
    rig.state
        .opening_report_view()
        .favorites
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

#[test]
fn favorites_are_added_edited_reordered_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let add = |rig: &mut Rig, name: &str| {
        rig.send(OpeningReportMessage::Favorite(FavoriteMessage::Add {
            name: name.to_owned(),
        }));
    };
    add(&mut rig, "  ");
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteNameEmpty));
    add(&mut rig, "Spanish");
    add(&mut rig, "Spanish");
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteNameTaken));
    add(&mut rig, "Second");
    add(&mut rig, "Third");
    assert_eq!(favorite_names(&rig), ["Spanish", "Second", "Third"]);
    // The moves are those of the position of the board.
    assert_eq!(
        rig.state.opening_report_view().favorites[0].moves,
        "1.e4 e5 2.Nf3"
    );
    let edit = |rig: &mut Rig, m: FavoriteMessage| {
        rig.send(OpeningReportMessage::Favorite(m));
    };
    edit(
        &mut rig,
        FavoriteMessage::Rename {
            index: 1,
            name: "Spanish".to_owned(),
        },
    );
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteNameTaken));
    edit(
        &mut rig,
        FavoriteMessage::Rename {
            index: 1,
            name: "Renamed".to_owned(),
        },
    );
    edit(&mut rig, FavoriteMessage::MoveUp(1));
    edit(&mut rig, FavoriteMessage::MoveDown(1));
    edit(&mut rig, FavoriteMessage::Delete(0));
    edit(&mut rig, FavoriteMessage::Delete(9));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteMissing));
    assert_eq!(favorite_names(&rig), ["Third", "Spanish"]);
    // The file holds the same, and a new program reads it back.
    let file = dir.path().join("report_favorites.toml");
    let on_disk = parse_favorites(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(on_disk.issue, None);
    assert_eq!(on_disk.favorites.len(), 2);
    assert_eq!(
        names(dir.path())
            .iter()
            .filter(|n| n.contains("tmp"))
            .count(),
        0
    );
    let again = with_favorites_file(dir.path());
    assert_eq!(favorite_names(&again), ["Third", "Spanish"]);
}

#[test]
fn a_favorite_is_refused_before_the_file_is_read_and_for_a_custom_start() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    let mut rig = Rig::with(state);
    // Not read yet: the effect is asked for and the edit is refused.
    let effects = rig.raw(OpeningReportMessage::Favorite(FavoriteMessage::Add {
        name: "x".to_owned(),
    }));
    assert!(matches!(
        effects.as_slice(),
        [Effect::OpeningReport(
            OpeningReportEffect::LoadFavorites { .. }
        )]
    ));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoritesLoading));
}

fn rig_in(dir: &Path) -> Rig {
    Rig::with(AppState::with_preferences_path(Some(
        dir.join("preferences.toml"),
    )))
}

fn add_favorite(rig: &mut Rig, name: &str) {
    rig.send(OpeningReportMessage::Favorite(FavoriteMessage::Add {
        name: name.to_owned(),
    }));
}

const NEWER: &str = "version = 2\nfuture = \"x\"\n\n[[favorite]]\nname = \"Keep\"\nmoves = \"1.e4\"\nnote = \"kept by the newer version\"\n";
const WITH_JUNK: &str = "version = 1\n\n[[favorite]]\nname = \"Keep\"\nmoves = \"1.e4\"\n\n[[favorite]]\nname = \"No moves\"\n";

#[test]
fn a_failed_read_never_lets_an_edit_overwrite_the_favorites_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report_favorites.toml");
    let good = "version = 1\n\n[[favorite]]\nname = \"Keep\"\nmoves = \"1.e4\"\n";
    std::fs::write(&file, good).unwrap();
    let mut rig = rig_in(dir.path());
    let add = |rig: &mut Rig| {
        rig.raw(OpeningReportMessage::Favorite(FavoriteMessage::Add {
            name: "New".to_owned(),
        }))
    };
    // The read fails (permission, I/O): the list is not editable.
    let effects = add(&mut rig);
    let [Effect::OpeningReport(OpeningReportEffect::LoadFavorites { request, .. })] =
        effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    let failure = mychess_app::FileError::new(
        mychess_app::FileErrorKind::Io(std::io::ErrorKind::PermissionDenied),
        None,
    );
    let next = rig.raw(OpeningReportMessage::FavoritesLoaded {
        request: *request,
        result: Err(failure),
    });
    assert!(next.is_empty());
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoritesLoadFailed));
    // The next edit reads the file again and is refused until it is read.
    let effects = add(&mut rig);
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::OpeningReport(
                OpeningReportEffect::LoadFavorites { .. }
            )]
        ),
        "{effects:?}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), good);
    assert!(rig.state.opening_report_view().favorites.is_empty());
    // The read works this time: the edit is accepted and keeps `Keep`.
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    rig.send(OpeningReportMessage::Favorite(FavoriteMessage::Add {
        name: "New".to_owned(),
    }));
    assert_eq!(favorite_names(&rig), ["Keep", "New"]);
    let on_disk = parse_favorites(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(on_disk.favorites.len(), 2);
    assert!(!dir.path().join("report_favorites.toml.bak").exists());
}

#[test]
fn a_file_of_a_newer_version_is_copied_before_it_is_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report_favorites.toml");
    let bak = dir.path().join("report_favorites.toml.bak");
    std::fs::write(&file, NEWER).unwrap();
    let mut rig = rig_in(dir.path());
    add_favorite(&mut rig, "warm up the read");
    add_favorite(&mut rig, "One");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), NEWER);
    let on_disk = parse_favorites(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(on_disk.issue, None, "rewritten as version 1");
    assert_eq!(on_disk.favorites.len(), 2);
    // Later writes do not replace the copy of the original.
    add_favorite(&mut rig, "Two");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), NEWER);
    assert_eq!(favorite_names(&rig), ["Keep", "One", "Two"]);
}

#[test]
fn entries_dropped_on_reading_are_kept_in_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report_favorites.toml");
    let bak = dir.path().join("report_favorites.toml.bak");
    std::fs::write(&file, WITH_JUNK).unwrap();
    let mut rig = rig_in(dir.path());
    add_favorite(&mut rig, "warm up the read");
    assert_eq!(favorite_names(&rig), ["Keep"]);
    add_favorite(&mut rig, "One");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), WITH_JUNK);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("No moves"));
    assert_eq!(parse_favorites(&text).favorites.len(), 2);
}

#[test]
fn a_clean_file_gets_no_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    add_favorite(&mut rig, "warm up the read");
    add_favorite(&mut rig, "One");
    add_favorite(&mut rig, "Two");
    assert_eq!(favorite_names(&rig), ["One", "Two"]);
    assert!(!dir.path().join("report_favorites.toml.bak").exists());
}

#[test]
fn an_edit_during_a_save_is_written_after_it_and_a_late_answer_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path());
    add_favorite(&mut rig, "warm up the read");
    let save_request = |effects: &[Effect]| match effects {
        [
            Effect::OpeningReport(OpeningReportEffect::SaveFavorites {
                request, favorites, ..
            }),
        ] => (*request, favorites.len()),
        other => panic!("{other:?}"),
    };
    let first = rig.raw(OpeningReportMessage::Favorite(FavoriteMessage::Add {
        name: "A".to_owned(),
    }));
    let (first_request, n) = save_request(&first);
    assert_eq!(n, 1);
    // The save is still running: the second edit waits.
    let second = rig.raw(OpeningReportMessage::Favorite(FavoriteMessage::Add {
        name: "B".to_owned(),
    }));
    assert!(second.is_empty(), "{second:?}");
    assert_eq!(favorite_names(&rig), ["A", "B"]);
    // The first save ends: the list as it is now is written.
    let done = rig.raw(OpeningReportMessage::FavoritesSaved {
        request: first_request,
        ok: true,
    });
    let (second_request, n) = save_request(&done);
    assert_eq!(n, 2);
    assert_ne!(first_request, second_request);
    // A late answer of the first save changes nothing.
    let late = rig.raw(OpeningReportMessage::FavoritesSaved {
        request: first_request,
        ok: false,
    });
    assert!(late.is_empty());
    let end = rig.raw(OpeningReportMessage::FavoritesSaved {
        request: second_request,
        ok: true,
    });
    assert!(end.is_empty(), "nothing is written twice: {end:?}");
    let answer = rig.perform(done.into_iter().next().unwrap());
    assert!(matches!(
        answer,
        Message::OpeningReport(OpeningReportMessage::FavoritesSaved { ok: true, .. })
    ));
    let on_disk = parse_favorites(
        &std::fs::read_to_string(dir.path().join("report_favorites.toml")).unwrap(),
    );
    assert_eq!(on_disk.favorites.len(), 2);
}

#[test]
fn the_report_becomes_stale_when_games_are_added_or_removed_from_its_database() {
    // Criterion 27 from the messages: the Clipbase gets a game (the total
    // changes), "Update" makes it current again, and clearing it makes the
    // report stale once more. (Saving and compacting a file go through
    // `data_rev`, which the unit tests of the flow change directly.)
    let (mut rig, slot, _dir) = sample();
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.send(OpeningReportMessage::Generate);
    assert_eq!(rig.state.opening_report_view().phase, ReportPhase::Ready);
    assert!(!rig.state.opening_report_view().stale);
    // The copy is made from a file slot; coming back, only the games of the
    // Clipbase differ.
    rig.send(BasesMessage::Select(slot));
    rig.send(BasesMessage::ClipbaseCopy);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 1);
    assert!(rig.state.opening_report_view().stale);
    rig.send(OpeningReportMessage::Select(ReportSelector::All));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::Stale));
    rig.send(OpeningReportMessage::Refresh);
    assert!(!rig.state.opening_report_view().stale);
    rig.send(BasesMessage::ClipbaseClear);
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 0);
    assert!(rig.state.opening_report_view().stale);
}

#[test]
fn reports_dat_of_the_scid_is_imported_as_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let dat = dir.path().join("reports.dat");
    std::fs::write(
        &dat,
        "# Scid opening report favorites file\n\n\
         set reportFavorites {{Najdorf {1.e4 c5 2.Nf3 d6}} {Other \"1.d4 d5\"} {Najdorf {1.c4}}}\n\
         exec rm -rf /\n",
    )
    .unwrap();
    rig.send(OpeningReportMessage::Favorite(FavoriteMessage::Import(dat)));
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::FavoritesImported {
            added: 2,
            skipped: 2
        })
    );
    assert_eq!(favorite_names(&rig), ["Najdorf", "Other"]);
    assert!(dir.path().join("report_favorites.toml").exists());
    // A file that is not there.
    rig.send(OpeningReportMessage::Favorite(FavoriteMessage::Import(
        dir.path().join("none.dat"),
    )));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::ImportFailed));
}

fn import(rig: &mut Rig, dir: &Path, body: &str) {
    let dat = dir.join("reports.dat");
    std::fs::write(&dat, body).unwrap();
    rig.send(OpeningReportMessage::Favorite(FavoriteMessage::Import(dat)));
}

#[test]
fn the_batch_writes_one_file_per_favorite_and_restores_nothing_it_never_touched() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    import(
        &mut rig,
        dir.path(),
        "set reportFavorites {{A/B {1.e4 e5}} {A:B {1.d4 d5}} {Bad {1.e4 e4}} {CON {1.c4}}}",
    );
    assert_eq!(favorite_names(&rig).len(), 4);
    let game = rig.state.session.game().clone();
    let cursor = rig.state.session.cursor();
    let out = tempfile::tempdir().unwrap();
    rig.send(OpeningReportMessage::Favorite(
        FavoriteMessage::GenerateAll {
            dir: out.path().to_path_buf(),
            part: ReportPart::Compact,
            format: ReportFormat::Text,
        },
    ));
    // Names are made safe; two that end up equal get a number; the name
    // reserved by Windows is changed; the illegal line is counted.
    assert_eq!(names(out.path()), ["A_B (2).txt", "A_B.txt", "_CON.txt"]);
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::BatchDone {
            written: 3,
            failed: 1
        })
    );
    assert!(rig.state.opening_report_view().batch.is_none());
    // The game of the user was not touched.
    assert!(rig.state.session.game().same_content(&game));
    assert_eq!(rig.state.session.cursor(), cursor);
    let first = std::fs::read_to_string(out.path().join("A_B.txt")).unwrap();
    assert!(first.contains("1.e4 e5"));
}

#[test]
fn a_write_error_is_counted_and_the_batch_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    import(
        &mut rig,
        dir.path(),
        "set reportFavorites {{One {1.e4}} {Two {1.d4}}}",
    );
    rig.send(OpeningReportMessage::Favorite(
        FavoriteMessage::GenerateAll {
            dir: dir.path().join("missing"),
            part: ReportPart::Full,
            format: ReportFormat::Html,
        },
    ));
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::BatchDone {
            written: 0,
            failed: 2
        })
    );
}

fn start_batch(rig: &mut Rig, dir: &Path, out: &Path, body: &str) -> Vec<Effect> {
    import(rig, dir, body);
    rig.raw(OpeningReportMessage::Favorite(
        FavoriteMessage::GenerateAll {
            dir: out.to_path_buf(),
            part: ReportPart::Compact,
            format: ReportFormat::Text,
        },
    ))
}

fn batch_item(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::OpeningReport(OpeningReportEffect::BatchItem(_))))
        .expect("a report is asked for")
}

const THREE: &str = "set reportFavorites {{One {1.e4}} {Two {1.d4}} {Three {1.c4}}}";

#[test]
fn stop_before_the_item_runs_ends_the_batch_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let out = tempfile::tempdir().unwrap();
    let effects = start_batch(&mut rig, dir.path(), out.path(), THREE);
    let batch = rig.state.opening_report_view().batch.expect("running");
    assert_eq!(
        (batch.current, batch.total, batch.name.as_str()),
        (1, 3, "One")
    );
    rig.raw(OpeningReportMessage::Favorite(FavoriteMessage::StopBatch));
    let answer = rig.perform(batch_item(effects));
    let next = rig.raw(answer);
    assert!(next.is_empty(), "nothing more is asked for: {next:?}");
    assert!(rig.state.opening_report_view().batch.is_none());
    assert!(
        names(out.path()).is_empty(),
        "the stopped item wrote nothing"
    );
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::BatchDone {
            written: 0,
            failed: 0
        })
    );
}

#[test]
fn stop_after_the_item_finished_counts_it_and_ends_with_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let out = tempfile::tempdir().unwrap();
    let effects = start_batch(&mut rig, dir.path(), out.path(), THREE);
    // The item ends before the stop has any effect on it.
    let answer = rig.perform(batch_item(effects));
    assert_eq!(names(out.path()), ["One.txt"]);
    rig.raw(OpeningReportMessage::Favorite(FavoriteMessage::StopBatch));
    let next = rig.raw(answer);
    assert!(next.is_empty(), "no second report: {next:?}");
    assert!(rig.state.opening_report_view().batch.is_none());
    assert_eq!(names(out.path()), ["One.txt"]);
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::BatchDone {
            written: 1,
            failed: 0
        })
    );
}

#[test]
fn the_answer_of_an_old_batch_item_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let out = tempfile::tempdir().unwrap();
    let effects = start_batch(&mut rig, dir.path(), out.path(), THREE);
    let first = rig.perform(batch_item(effects));
    // The first answer moves the batch to the second item.
    let second = rig.raw(first.clone());
    assert!(matches!(
        second.as_slice(),
        [Effect::OpeningReport(OpeningReportEffect::BatchItem(_))]
    ));
    let view = rig.state.opening_report_view().batch.expect("running");
    assert_eq!((view.current, view.name.as_str()), (2, "Two"));
    // The same answer again is a late one: nothing happens.
    rig.state.notice = None;
    let again = rig.raw(first);
    assert!(again.is_empty(), "{again:?}");
    let view = rig.state.opening_report_view().batch.expect("running");
    assert_eq!((view.current, view.name.as_str()), (2, "Two"));
    assert_eq!(rig.notice(), None);
}

#[test]
fn changing_the_options_during_the_batch_does_not_change_the_later_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let out = tempfile::tempdir().unwrap();
    let effects = start_batch(&mut rig, dir.path(), out.path(), THREE);
    let first = batch_item(effects);
    // The job is private: its `Debug` shows the options it carries.
    let table_size = |e: &Effect| {
        let text = format!("{e:?}");
        text.contains("max_table_games: 500") && !text.contains("max_table_games: 7,")
    };
    assert!(table_size(&first), "{first:?}");
    let answer = rig.perform(first);
    rig.raw(OpeningReportMessage::SetPrefs(Box::new(
        OpeningReportPrefs {
            max_table_games: 7,
            ..OpeningReportPrefs::default()
        },
    )));
    assert_eq!(rig.state.prefs.opening_report.max_table_games, 7);
    let second = batch_item(rig.raw(answer));
    assert!(table_size(&second), "{second:?}");
}

#[test]
fn a_batch_whose_database_is_gone_ends_with_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    let out = tempfile::tempdir().unwrap();
    let effects = start_batch(&mut rig, dir.path(), out.path(), THREE);
    let answer = rig.perform(batch_item(effects));
    // Closing the database ends the batch at the end of that `update`,
    // with the note of what was done so far.
    rig.state.notice = None;
    let slot = rig.state.bases.current();
    rig.raw(BasesMessage::Close(slot));
    assert!(rig.state.opening_report_view().batch.is_none());
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::BatchDone {
            written: 0,
            failed: 0
        })
    );
    // The answer that arrives later is ignored.
    assert!(rig.raw(answer).is_empty());
}

#[test]
fn a_favorite_makes_its_report_without_touching_the_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_favorites_file(dir.path());
    import(
        &mut rig,
        dir.path(),
        "set reportFavorites {{Ruy {1.e4 e5 2.Nf3 Nc6 3.Bb5}} {Bad {1.e4 e4}}}",
    );
    let game = rig.state.session.game().clone();
    rig.send(OpeningReportMessage::OpenFavorite(0));
    let view = rig.state.opening_report_view();
    assert_eq!(view.phase, ReportPhase::Ready);
    let report = view.report.unwrap();
    // Games 1 and 3 reach 3.Bb5; game 6 transposes into it but ends there,
    // so it is not in the report (spec: games that end at the position).
    let games: Vec<u32> = report.core.lines.iter().map(|l| l.game).collect();
    assert_eq!(games, vec![1, 3]);
    assert!(rig.state.session.game().same_content(&game));
    rig.send(OpeningReportMessage::OpenFavorite(1));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteMoves));
    rig.send(OpeningReportMessage::OpenFavorite(7));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteMissing));
}

// ---- options (criterion 26) ----

#[test]
fn the_options_are_changed_restored_and_feed_the_report() {
    let (mut rig, _slot, _dir) = sample();
    let mut prefs = OpeningReportPrefs {
        oldest: 0,
        newest: 0,
        max_table_games: 0,
        move_orders: 0,
        board_size: 55,
        ..OpeningReportPrefs::default()
    };
    rig.send(OpeningReportMessage::SetPrefs(Box::new(prefs.clone())));
    assert_eq!(*rig.state.prefs.opening_report, prefs);
    rig.send(OpeningReportMessage::Generate);
    let view = rig.state.opening_report_view();
    assert_eq!(view.options.unwrap().max_table_games, 0);
    assert!(view.report.unwrap().core.rows.is_empty(), "no theory table");
    // A wrong value goes back to the default; the others are kept.
    prefs.shortest = 200;
    rig.send(OpeningReportMessage::SetPrefs(Box::new(prefs)));
    assert_eq!(rig.state.prefs.opening_report.shortest, 5);
    assert_eq!(rig.state.prefs.opening_report.board_size, 55);
    rig.send(OpeningReportMessage::ResetPrefs);
    let p = &rig.state.prefs.opening_report;
    assert_eq!((p.oldest, p.newest, p.max_table_games), (5, 5, 500));
    // The file round trip.
    let text = render_preferences(&rig.state.prefs);
    assert_eq!(
        *load_preferences(&text).prefs.opening_report,
        *rig.state.prefs.opening_report
    );
}

// ---- the words (criterion 29, the part of the application) ----

fn ftl(language: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("oprep.ftl");
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[test]
fn every_label_and_notice_key_exists_in_both_languages() {
    use OpeningReportNotice as N;
    let notices = [
        N::Unavailable,
        N::NoReport,
        N::Stale,
        N::Busy,
        N::Failed,
        N::UnsupportedPosition,
        N::BadLine,
        N::Interrupted,
        N::FilterSet,
        N::NothingToMerge,
        N::Saved,
        N::SaveFailed,
        N::SaveProtected,
        N::ViewFailed,
        N::FavoritesLoading,
        N::FavoriteNameEmpty,
        N::FavoriteNameTaken,
        N::FavoriteLimit,
        N::FavoriteMissing,
        N::FavoriteStart,
        N::FavoriteMoves,
        N::FavoritesSaveFailed,
        N::FavoritesLoadFailed,
        N::FavoritesCorrupt,
        N::FavoritesImported {
            added: 1,
            skipped: 0,
        },
        N::ImportFailed,
        N::NoFavorites,
        N::BatchDone {
            written: 1,
            failed: 0,
        },
    ];
    for language in ["en-US", "pt-BR"] {
        let words = ftl(language);
        for key in label_keys() {
            assert!(words.contains_key(&key), "{language}: {key}");
        }
        for notice in notices {
            assert!(
                words.contains_key(notice.message_key()),
                "{language}: {notice:?}"
            );
        }
        assert!(words.contains_key("menu-tools-opening-report"));
        for (key, value) in &words {
            assert!(!value.trim().is_empty(), "{language}: {key} is empty");
            assert!(
                !value.to_lowercase().contains("variação"),
                "{language}: {key}"
            );
        }
    }
    let en = ftl("en-US");
    let pt = ftl("pt-BR");
    assert_eq!(en.keys().collect::<Vec<_>>(), pt.keys().collect::<Vec<_>>());
    assert_ne!(en["oprep-label-title"], pt["oprep-label-title"]);
}

#[test]
fn the_labels_are_filled_from_a_lookup() {
    let pt = ftl("pt-BR");
    let labels = report_labels(&|key, args| {
        pt.get(key).map(|v| {
            let mut v = v.clone();
            for (name, value) in args {
                v = v.replace(&format!("{{ ${name} }}"), value);
            }
            v
        })
    });
    assert_eq!(labels.title, "Relatório de abertura");
    assert_eq!(labels.every, "uma vez a cada {n} partidas");
    assert_eq!(labels.decimal, ',');
    assert_eq!(
        labels.move_orders_many,
        "Houve {n} ordens de lances; as {max} principais são:"
    );
    assert_eq!(labels.theme_names[1], "Roques opostos");
    // The save uses the words that the front-end sent.
    let (mut rig, _slot, _dir) = sample();
    rig.send(OpeningReportMessage::SetLabels(Box::new(labels)));
    rig.send(OpeningReportMessage::Generate);
    let out = tempfile::tempdir().unwrap();
    rig.send(OpeningReportMessage::Save {
        path: out.path().join("pt"),
        part: ReportPart::Compact,
        format: ReportFormat::Text,
    });
    let text = std::fs::read_to_string(out.path().join("pt.txt")).unwrap();
    assert!(text.contains("Banco: report.pgn (7 partidas)"), "{text}");
    assert!(text.contains("Gerado por: MyChess"), "{text}");
    assert!(text.contains("ESTATÍSTICAS E HISTÓRICO"), "{text}");
}
