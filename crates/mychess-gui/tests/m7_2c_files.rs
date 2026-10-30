//! M7-2c (GUI, spec M7-2): saving the report (three parts in three formats,
//! with the file dialog simulated by its answer), the favorites, the batch of
//! reports and the translations of the window.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::assert_is_empty
)]

mod m7_2c_common;

use std::collections::BTreeSet;
use std::path::Path;

use m7_2c_common::*;

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn favorite_names(rig: &Rig) -> Vec<String> {
    rig.shell
        .app
        .opening_report_view()
        .favorites
        .iter()
        .map(|f| f.name.clone())
        .collect()
}

#[test]
fn saving_three_parts_in_three_formats_through_the_form() {
    let mut rig = Rig::sample();
    rig.open_window();
    let out = tempfile::tempdir().unwrap();
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
            rig.ui(OpRepUiMessage::OpenSave);
            // The form proposes what was saved last; the buttons change it.
            let mut sim = rig.sim(1400.0, 900.0);
            for name in ["oprep-save-box", "oprep-save-choose", "oprep-save-cancel"] {
                assert!(sim.find(id(name)).is_ok(), "{name}");
            }
            drop(sim);
            rig.ui(OpRepUiMessage::SavePart(part));
            rig.ui(OpRepUiMessage::SaveFormat(format));
            // The native dialog is replaced by its answer; the name has no
            // extension, the program adds the one of the format.
            let base = out.path().join(format!("{tag}-{ext}"));
            rig.ui(OpRepUiMessage::SavePicked(Some(base)));
            assert!(rig.shell.ui.dialogs.oprep.form.is_none(), "{tag} {ext}");
            assert_eq!(
                rig.notice(),
                Some(OpeningReportNotice::Saved),
                "{tag} {ext}"
            );
            let file = out.path().join(format!("{tag}-{ext}.{ext}"));
            let text = std::fs::read_to_string(&file).unwrap();
            let lower = text.to_lowercase();
            match part {
                ReportPart::Full => {
                    assert!(lower.contains("theory table"), "{tag} {ext}");
                    assert!(lower.contains("statistics and history"), "{tag} {ext}");
                }
                ReportPart::Compact => {
                    assert!(!lower.contains("theory table"), "{tag} {ext}");
                    assert!(lower.contains("statistics and history"), "{tag} {ext}");
                }
                ReportPart::TableOnly => {
                    assert!(lower.contains("theory table") || lower.contains("+3 =1 -1"));
                    assert!(!lower.contains("statistics and history"), "{tag} {ext}");
                }
            }
        }
    }
    assert_eq!(names(out.path()).len(), 9);
    // The last choice is remembered for the next time.
    let prefs = &rig.shell.app.prefs.opening_report;
    assert_eq!(prefs.last_part, ReportPart::TableOnly);
    assert_eq!(prefs.last_format, ReportFormat::Latex);
    rig.ui(OpRepUiMessage::OpenSave);
    let Some(OpRepForm::Save(draft)) = rig.shell.ui.dialogs.oprep.form else {
        panic!("the form");
    };
    assert_eq!(
        (draft.part, draft.format),
        (ReportPart::TableOnly, ReportFormat::Latex)
    );
}

#[test]
fn a_cancelled_dialog_writes_nothing() {
    let mut rig = Rig::sample();
    rig.open_window();
    let out = tempfile::tempdir().unwrap();
    rig.ui(OpRepUiMessage::OpenSave);
    rig.ui(OpRepUiMessage::SavePicked(None));
    assert!(rig.shell.ui.dialogs.oprep.form.is_some(), "the form stays");
    assert!(names(out.path()).is_empty());
}

#[test]
fn the_save_button_asks_for_the_form() {
    let mut rig = Rig::sample();
    rig.open_window();
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-save");
    assert!(debug_of(&messages).iter().any(|m| m.contains("OpenSave")));
}

#[test]
fn favorites_are_added_renamed_reordered_and_removed_on_the_form() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::OpenFavorites);
    let mut sim = rig.sim(1400.0, 900.0);
    for name in [
        "oprep-favorites-box",
        "oprep-fav-add",
        "oprep-fav-import",
        "oprep-fav-generate",
    ] {
        assert!(sim.find(id(name)).is_ok(), "{name}");
    }
    assert!(sim.find("There are no favorite reports").is_ok());
    drop(sim);
    // An empty name does nothing: the button is disabled and Enter too.
    assert!(click_id(rig.sim(1400.0, 900.0), "oprep-fav-add").is_empty());
    rig.ui(OpRepUiMessage::FavAdd);
    assert!(favorite_names(&rig).is_empty());
    rig.ui(OpRepUiMessage::FavName("   ".to_owned()));
    assert!(click_id(rig.sim(1400.0, 900.0), "oprep-fav-add").is_empty());
    // A name is added with the position of the board.
    rig.ui(OpRepUiMessage::FavName("Ruy".to_owned()));
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-fav-add");
    assert_eq!(messages.len(), 1);
    for m in messages {
        rig.msg(m);
    }
    assert_eq!(favorite_names(&rig), ["Ruy"]);
    let Some(OpRepForm::Favorites(draft)) = &rig.shell.ui.dialogs.oprep.form else {
        panic!("the form");
    };
    assert!(draft.name.is_empty(), "the field is cleared");
    // A name that is used is refused with the notice.
    rig.ui(OpRepUiMessage::FavName("Ruy".to_owned()));
    rig.ui(OpRepUiMessage::FavAdd);
    assert_eq!(favorite_names(&rig), ["Ruy"]);
    assert_eq!(rig.notice(), Some(OpeningReportNotice::FavoriteNameTaken));
    rig.ui(OpRepUiMessage::FavName("Italiana".to_owned()));
    rig.ui(OpRepUiMessage::FavAdd);
    assert_eq!(favorite_names(&rig), ["Ruy", "Italiana"]);
    // Order: the first cannot go up and the last cannot go down.
    assert!(click_id(rig.sim(1400.0, 900.0), "oprep-fav-up-0").is_empty());
    assert!(click_id(rig.sim(1400.0, 900.0), "oprep-fav-down-1").is_empty());
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-fav-up-1");
    for m in messages {
        rig.msg(m);
    }
    assert_eq!(favorite_names(&rig), ["Italiana", "Ruy"]);
    rig.ui(OpRepUiMessage::FavDown(0));
    assert_eq!(favorite_names(&rig), ["Ruy", "Italiana"]);
    // Rename.
    rig.ui(OpRepUiMessage::FavRenameStart(1));
    rig.ui(OpRepUiMessage::FavRenameInput("Giuoco".to_owned()));
    rig.ui(OpRepUiMessage::FavRenameGo);
    assert_eq!(favorite_names(&rig), ["Ruy", "Giuoco"]);
    let Some(OpRepForm::Favorites(draft)) = &rig.shell.ui.dialogs.oprep.form else {
        panic!("the form");
    };
    assert!(draft.rename.is_none());
    // Delete.
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-fav-delete-0");
    for m in messages {
        rig.msg(m);
    }
    assert_eq!(favorite_names(&rig), ["Giuoco"]);
    // The file on disk holds the list (the write is serialized: wait it).
    rig.ui(OpRepUiMessage::CloseForm);
    let path =
        mychess_app::opening_report::favorites_path(&rig.dir.path().join("preferences.toml"));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("Giuoco") && !text.contains("Ruy"), "{text}");
}

#[test]
fn a_favorite_makes_its_report_and_the_form_closes() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::OpenFavorites);
    rig.ui(OpRepUiMessage::FavName("Ruy".to_owned()));
    rig.ui(OpRepUiMessage::FavAdd);
    // Another line in the editor, the favorite is still the first.
    rig.app(GameMessage::Navigate(Nav::GoToMainlinePly(1)));
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-fav-open-0");
    for m in messages {
        rig.msg(m);
    }
    assert!(rig.shell.ui.dialogs.oprep.form.is_none());
    assert_eq!(rig.view_phase(), ReportPhase::Ready);
    assert_eq!(rig.model().header.title, "1.e4 e5 2.Nf3");
    // The game of the editor was not touched (D3650).
    assert_eq!(rig.shell.app.session.position().ply(), 1);
}

#[test]
fn reports_dat_of_the_scid_is_imported_with_the_counts_of_the_notice() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::OpenFavorites);
    let dat = rig.dir.path().join("reports.dat");
    std::fs::write(
        &dat,
        "set reportFavorites {{Ruy {1.e4 e5 2.Nf3}} {Nada {1.e4 e4}} {Dama {1.d4 d5}}}",
    )
    .unwrap();
    rig.ui(OpRepUiMessage::FavDatPicked(Some(dat)));
    assert_eq!(favorite_names(&rig), ["Ruy", "Nada", "Dama"]);
    assert!(matches!(
        rig.notice(),
        Some(OpeningReportNotice::FavoritesImported { added: 3, .. })
    ));
    // The notice is on the window with its numbers.
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("Favorites imported: 3; left out: 0").is_ok());
    drop(sim);
    // A cancelled dialog or a file that is not there changes nothing.
    rig.ui(OpRepUiMessage::FavDatPicked(None));
    rig.ui(OpRepUiMessage::FavDatPicked(Some(
        rig.dir.path().join("nada.dat"),
    )));
    assert_eq!(rig.notice(), Some(OpeningReportNotice::ImportFailed));
    assert_eq!(favorite_names(&rig).len(), 3);
}

#[test]
fn the_batch_writes_a_file_per_favorite_and_the_window_says_so() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::OpenFavorites);
    for name in ["Um", "Dois"] {
        rig.ui(OpRepUiMessage::FavName(name.to_owned()));
        rig.ui(OpRepUiMessage::FavAdd);
    }
    rig.ui(OpRepUiMessage::FavPart(ReportPart::Compact));
    rig.ui(OpRepUiMessage::FavFormat(ReportFormat::Text));
    let out = tempfile::tempdir().unwrap();
    rig.ui(OpRepUiMessage::FavDirPicked(Some(out.path().to_path_buf())));
    assert!(rig.shell.ui.dialogs.oprep.form.is_none());
    assert_eq!(names(out.path()), ["Dois.txt", "Um.txt"]);
    assert_eq!(
        rig.notice(),
        Some(OpeningReportNotice::BatchDone {
            written: 2,
            failed: 0
        })
    );
    assert!(rig.shell.app.opening_report_view().batch.is_none());
    assert!(
        rig.sim(1400.0, 900.0)
            .find("Reports written: 2; failed: 0")
            .is_ok()
    );
}

#[test]
fn the_batch_shows_its_progress_and_stop_ends_it_and_closing_stops_it() {
    let mut rig = Rig::sample();
    rig.open_window();
    rig.ui(OpRepUiMessage::OpenFavorites);
    for name in ["Um", "Dois", "Tres"] {
        rig.ui(OpRepUiMessage::FavName(name.to_owned()));
        rig.ui(OpRepUiMessage::FavAdd);
    }
    let out = tempfile::tempdir().unwrap();
    // The first item is asked for but not run: the batch is in flight.
    let task = rig.once(Message::OpRep(OpRepUiMessage::FavDirPicked(Some(
        out.path().to_path_buf(),
    ))));
    drop(task);
    let view = rig.shell.app.opening_report_view();
    let batch = view.batch.clone().expect("the batch runs");
    drop(view);
    assert_eq!(batch.total, 3);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find(id("oprep-batch-stop")).is_ok());
    assert!(
        sim.find("Generating reports: 1 / 3: Um").is_ok(),
        "{batch:?}"
    );
    drop(sim);
    // Generating is not offered again while it runs.
    rig.ui(OpRepUiMessage::OpenFavorites);
    assert!(click_id(rig.sim(1400.0, 900.0), "oprep-fav-generate").is_empty());
    rig.ui(OpRepUiMessage::CloseForm);
    let messages = click_id(rig.sim(1400.0, 900.0), "oprep-batch-stop");
    let sent = debug_of(&messages);
    assert!(sent.iter().any(|m| m.contains("StopBatch")), "{sent:?}");
    // Closing the window with the batch in flight stops it.
    drop(rig.once(Message::OpRep(OpRepUiMessage::Close)));
    assert!(!rig.is_open());
    assert!(names(out.path()).is_empty(), "nothing was written");
}

fn ftl_keys(language: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("oprep.ftl");
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (key, _) = l.split_once(" = ")?;
            let key = key.trim();
            (!key.starts_with('#') && !key.contains(' ')).then(|| key.to_owned())
        })
        .collect()
}

#[test]
fn every_key_of_the_window_exists_in_both_languages() {
    let en = ftl_keys("en-US");
    let pt = ftl_keys("pt-BR");
    assert_eq!(en, pt, "the two files have the same keys");
    // Every `oprep-` key the code of the window names exists.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/windows/opening_report");
    let mut used = BTreeSet::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.push(dir.with_extension("rs"));
    for file in files {
        if file.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for chunk in text.split('"').skip(1).step_by(2) {
            if chunk.starts_with("oprep-")
                && !chunk.starts_with("oprep-link-")
                && !chunk.starts_with("oprep-note-")
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !chunk.ends_with('-')
            {
                used.insert(chunk.to_owned());
            }
        }
    }
    // Ids of widgets share the prefix: keep the ones that are messages.
    let ids = [
        "oprep-window",
        "oprep-find",
        "oprep-fav-name",
        "oprep-fav-rename",
        "oprep-options-box",
        "oprep-save-box",
        "oprep-exclude-box",
        "oprep-favorites-box",
        "oprep-board",
        "oprep-close",
        "oprep-stop",
        "oprep-stale-update",
        "oprep-batch-stop",
        "oprep-footer",
        "oprep-update",
        "oprep-options",
        "oprep-exclude",
        "oprep-favorites",
        "oprep-save",
        "oprep-view-html",
        "oprep-view-latex",
        "oprep-merge",
        "oprep-board-smaller",
        "oprep-board-bigger",
        "oprep-theory",
        "oprep-scroll",
        "oprep-body",
        "oprep-options-defaults",
        "oprep-options-cancel",
        "oprep-options-ok",
        "oprep-save-cancel",
        "oprep-save-choose",
        "oprep-exclude-none",
        "oprep-exclude-cancel",
        "oprep-fav-rename-ok",
        "oprep-fav-rename-cancel",
        "oprep-fav-add",
        "oprep-fav-import",
        "oprep-fav-generate",
        "oprep-favorites-close",
    ];
    let missing: Vec<&String> = used
        .iter()
        .filter(|k| !en.contains(*k) && !ids.contains(&k.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "keys without a translation: {missing:?}"
    );
    assert!(used.iter().any(|k| k.starts_with("oprep-ui-")));
}

#[test]
fn portuguese_says_variante_never_variacao_and_has_no_english_window_texts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/pt-BR/oprep.ftl");
    let text = std::fs::read_to_string(path).unwrap().to_lowercase();
    assert!(!text.contains("variaç"), "the word is variante");
    let en = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/en-US/oprep.ftl"),
    )
    .unwrap();
    // No `oprep-ui-` text of the Portuguese file equals the English one,
    // except words that are the same in both (HTML, LaTeX, the format names).
    let pairs = |t: &str| -> Vec<(String, String)> {
        t.lines()
            .filter_map(|l| l.split_once(" = "))
            .filter(|(k, _)| k.starts_with("oprep-ui-") || k.starts_with("oprep-opt-"))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    };
    let pt_text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/pt-BR/oprep.ftl"),
    )
    .unwrap();
    let en_pairs = pairs(&en);
    for (key, value) in pairs(&pt_text) {
        let same = en_pairs.iter().any(|(k, v)| *k == key && *v == value);
        assert!(!same, "{key} is not translated: {value}");
    }
}

#[test]
fn the_window_texts_come_from_fluent_in_both_languages() {
    let mut rig = Rig::sample();
    rig.open_window();
    for language in [
        mychess_app::LanguagePref::EnUs,
        mychess_app::LanguagePref::PtBr,
    ] {
        rig.set_language(language);
        let mut sim = rig.sim(1400.0, 900.0);
        for key in [
            "oprep-ui-update",
            "oprep-ui-options",
            "oprep-ui-favorites",
            "oprep-ui-save",
            "oprep-ui-view-html",
            "oprep-ui-view-latex",
            "oprep-ui-merge",
            "oprep-ui-close",
        ] {
            let text = rig.shell.i18n.tr(key);
            assert!(sim.find(text.as_str()).is_ok(), "{key} = {text}");
        }
    }
}
