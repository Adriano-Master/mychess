//! Contract tests of the files and the words of the player report (spec
//! `m7-3-relatorio-de-jogador.md`, criteria 14, 15 and 18): save, view, the
//! options and the Fluent keys.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod m7_3b_common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use m7_3b_common::*;
use mychess_app::player_report::{PlayerLabels, player_label_keys, player_labels};
use mychess_app::{load_preferences, render_preferences};

use PlayerColor::White;
use PlayerStart::{Beginning, Position};

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
    rig.report("Alfa, A", White, Position);
    assert_eq!(rig.state.player_report_view().phase, PlayerPhase::Ready);
    (rig, slot, dir)
}

fn save(rig: &mut Rig, path: PathBuf, part: ReportPart, format: ReportFormat) {
    rig.send(PlayerReportMessage::Save { path, part, format });
}

// ---- save (criterion 14) ----

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
            save(
                &mut rig,
                out.path().join(format!("{tag}-{ext}")),
                part,
                format,
            );
            assert_eq!(rig.notice(), Some(PlayerReportNotice::Saved), "{tag} {ext}");
            let file = out.path().join(format!("{tag}-{ext}.{ext}"));
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(!text.is_empty() && !text.contains('\0') && !text.contains('\r'));
            assert!(
                text.contains("A Alfa"),
                "{tag} {ext}: the name in natural order"
            );
            match ext {
                "html" => assert!(text.contains("</html>"), "a complete document"),
                "tex" => assert!(text.contains("\\end{document}")),
                _ => {
                    text_of.insert(tag, text);
                }
            }
        }
    }
    assert_eq!(names(out.path()).len(), 9, "no stray temporary files");
    // The parts: the point of view legend and the player's lines are in
    // all of them; the statistics only without "table only"; the theory
    // table (its heading `+V =E -D`) in the full one and the table-only one.
    let has_table = |tag: &str| text_of[tag].contains("+3 =0 -0");
    assert!(has_table("full") && has_table("table") && !has_table("compact"));
    assert!(text_of["full"].contains("Most frequent opponents"));
    assert!(!text_of["table"].contains("Most frequent opponents"));
    assert!(text_of["compact"].contains("Most frequent opponents"));
    assert!(text_of["full"].len() > text_of["table"].len());
    assert!(text_of["full"].contains("Database: report.pgn"));
    // What was chosen is remembered.
    let prefs = &rig.state.prefs.player_report;
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
        save(&mut rig, out.path().join(name), ReportPart::Compact, format);
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
    save(
        &mut rig,
        target.clone(),
        ReportPart::Compact,
        ReportFormat::Text,
    );
    assert_ne!(std::fs::read_to_string(&target).unwrap(), "old");
    assert_eq!(names(out.path()), ["r.txt"], "no temporary file is left");
    // A folder that does not exist: an error, nothing written anywhere.
    let missing = out.path().join("nope").join("r.txt");
    save(
        &mut rig,
        missing.clone(),
        ReportPart::Compact,
        ReportFormat::Text,
    );
    assert_eq!(rig.notice(), Some(PlayerReportNotice::SaveFailed));
    assert!(!missing.exists());
    assert_eq!(names(out.path()), ["r.txt"]);
}

#[test]
fn an_extension_added_over_an_existing_file_asks_before_replacing_it() {
    let (mut rig, _slot, _dir) = with_report();
    let out = tempfile::tempdir().unwrap();
    let existing = out.path().join("r.txt");
    std::fs::write(&existing, "old").unwrap();
    // The dialog confirmed `r`, not `r.txt`.
    save(
        &mut rig,
        out.path().join("r"),
        ReportPart::Compact,
        ReportFormat::Text,
    );
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "old");
    assert_eq!(
        rig.state.player_report_view().confirm_overwrite,
        Some(existing.as_path())
    );
    // "No" keeps the file and closes the question.
    rig.send(PlayerReportMessage::ConfirmSave(false));
    assert!(rig.state.player_report_view().confirm_overwrite.is_none());
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "old");
    // "Yes" writes it.
    save(
        &mut rig,
        out.path().join("r"),
        ReportPart::Compact,
        ReportFormat::Text,
    );
    rig.send(PlayerReportMessage::ConfirmSave(true));
    assert_ne!(std::fs::read_to_string(&existing).unwrap(), "old");
    assert_eq!(rig.notice(), Some(PlayerReportNotice::Saved));
    assert!(rig.state.player_report_view().confirm_overwrite.is_none());
}

#[test]
fn closing_the_window_drops_a_save_still_in_flight() {
    let (mut rig, _slot, _dir) = with_report();
    let out = tempfile::tempdir().unwrap();
    let existing = out.path().join("r.txt");
    std::fs::write(&existing, "old").unwrap();
    let effects = rig.raw(PlayerReportMessage::Save {
        path: out.path().join("r"),
        part: ReportPart::Compact,
        format: ReportFormat::Text,
    });
    let late = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(PlayerReportMessage::Close);
    rig.raw(late);
    assert!(
        rig.state.player_report_view().confirm_overwrite.is_none(),
        "an abandoned save must not ask to overwrite later"
    );
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "old");
}

#[test]
fn the_open_database_is_never_overwritten() {
    let (mut rig, _slot, dir) = sample();
    // Any file is read as PGN, so a database can have the extension of a
    // format and a save to its own name would destroy it.
    open_in(&mut rig, dir.path(), "games.txt");
    rig.report("Alfa, A", White, Beginning);
    let base = dir.path().join("games.txt");
    let original = std::fs::read(&base).unwrap();
    save(&mut rig, base.clone(), ReportPart::Full, ReportFormat::Text);
    assert_eq!(rig.notice(), Some(PlayerReportNotice::SaveProtected));
    assert_eq!(std::fs::read(&base).unwrap(), original);
    // The same name in another case is the same file.
    save(
        &mut rig,
        dir.path().join("GAMES.TXT"),
        ReportPart::Full,
        ReportFormat::Text,
    );
    assert_eq!(rig.notice(), Some(PlayerReportNotice::SaveProtected));
    assert_eq!(std::fs::read(&base).unwrap(), original);
    // Another name is written.
    save(
        &mut rig,
        dir.path().join("other.txt"),
        ReportPart::Full,
        ReportFormat::Text,
    );
    assert_eq!(rig.notice(), Some(PlayerReportNotice::Saved));
}

#[test]
fn saving_needs_a_report_and_one_save_at_a_time() {
    let (mut rig, _slot, _dir) = sample();
    let out = tempfile::tempdir().unwrap();
    let effects = rig.raw(PlayerReportMessage::Save {
        path: out.path().join("x"),
        part: ReportPart::Full,
        format: ReportFormat::Text,
    });
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::NoReport));
    rig.report("Alfa, A", White, Beginning);
    let first = rig.raw(PlayerReportMessage::Save {
        path: out.path().join("one"),
        part: ReportPart::Full,
        format: ReportFormat::Text,
    });
    assert_eq!(first.len(), 1);
    let second = rig.raw(PlayerReportMessage::Save {
        path: out.path().join("two"),
        part: ReportPart::Full,
        format: ReportFormat::Text,
    });
    assert!(second.is_empty());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::Busy));
    let answer = rig.perform(first.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(rig.notice(), Some(PlayerReportNotice::Saved));
    assert_eq!(names(out.path()), ["one.txt"]);
}

#[test]
fn the_documents_use_the_words_the_front_end_sent() {
    let pt = ftl("pt-BR");
    let lookup = |key: &str, args: &[(&str, &str)]| {
        pt.get(key).map(|v| {
            let mut v = v.clone();
            for (name, value) in args {
                v = v.replace(&format!("{{ ${name} }}"), value);
            }
            v
        })
    };
    let (mut rig, _slot, _dir) = sample();
    rig.send(PlayerReportMessage::SetLabels(Box::new(player_labels(
        &|k, a| lookup(k, a).or_else(|| ftl_of("pt-BR", "oprep.ftl").get(k).cloned()),
    ))));
    rig.report("Alfa, A", White, Position);
    let out = tempfile::tempdir().unwrap();
    save(
        &mut rig,
        out.path().join("pt"),
        ReportPart::Full,
        ReportFormat::Text,
    );
    let text = std::fs::read_to_string(out.path().join("pt.txt")).unwrap();
    assert!(
        text.contains("Jogador: A Alfa com as peças brancas depois de"),
        "{text}"
    );
    assert!(text.contains("Banco: report.pgn (8 partidas)"), "{text}");
    assert!(text.contains("do ponto de vista de A Alfa"), "{text}");
    assert!(text.contains("Oponentes mais frequentes"), "{text}");
    assert!(!text.contains("variação"), "{text}");
}

#[test]
fn view_html_writes_a_temporary_file_and_asks_to_open_it() {
    let (mut rig, _slot, _dir) = with_report();
    let host = rig.send(PlayerReportMessage::ViewHtml);
    assert!(host.is_empty());
    let open: Vec<PathBuf> = rig
        .seen
        .iter()
        .filter_map(|e| match e {
            Effect::PlayerReport(PlayerReportEffect::OpenFile { path }) => Some(path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(open.len(), 1, "{:?}", rig.seen);
    assert!(
        open[0]
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("html"))
    );
    let html = std::fs::read_to_string(&open[0]).unwrap();
    assert!(html.contains("</html>") && html.contains("A Alfa"));
    // The same folder is reused by a second view.
    rig.send(PlayerReportMessage::ViewHtml);
    let again: Vec<&Effect> = rig
        .seen
        .iter()
        .filter(|e| matches!(e, Effect::PlayerReport(PlayerReportEffect::OpenFile { .. })))
        .collect();
    assert_eq!(again.len(), 2);
    // Without a report there is nothing to view.
    let (mut rig, _slot, _dir) = sample();
    let effects = rig.raw(PlayerReportMessage::ViewHtml);
    assert!(effects.is_empty());
    assert_eq!(rig.notice(), Some(PlayerReportNotice::NoReport));
}

#[test]
fn a_late_answer_of_the_view_is_ignored() {
    let (mut rig, _slot, _dir) = with_report();
    let effects = rig.raw(PlayerReportMessage::ViewHtml);
    let stale = rig.perform(effects.into_iter().next().unwrap());
    // A newer view replaces the pending one.
    let newer = rig.raw(PlayerReportMessage::ViewHtml);
    let out = rig.raw(stale);
    assert!(out.is_empty(), "the old answer asks for nothing: {out:?}");
    let answer = rig.perform(newer.into_iter().next().unwrap());
    let out = rig.raw(answer);
    assert!(matches!(
        out.as_slice(),
        [Effect::PlayerReport(PlayerReportEffect::OpenFile { .. })]
    ));
}

// ---- the options (criterion 15) ----

#[test]
fn the_options_are_changed_restored_and_feed_the_report() {
    let (mut rig, _slot, _dir) = sample();
    let custom = PlayerReportPrefs {
        opponents: 1,
        eco_codes: 2,
        max_table_games: 0,
        stats: false,
        ..PlayerReportPrefs::default()
    };
    rig.send(PlayerReportMessage::SetPrefs(Box::new(custom.clone())));
    assert_eq!(*rig.state.prefs.player_report, custom);
    rig.report("Alfa, A", White, Beginning);
    let view = rig.state.player_report_view();
    let options = view.options.unwrap();
    assert_eq!(
        (
            options.opponents,
            options.eco_codes,
            options.max_table_games,
            options.stats
        ),
        (1, 2, 0, false)
    );
    // Out-of-range values go back to the default of their field.
    rig.send(PlayerReportMessage::SetPrefs(Box::new(PlayerReportPrefs {
        oldest: 200,
        ..custom
    })));
    assert_eq!(rig.state.prefs.player_report.oldest, 5);
    // Defaults come back, but what the dialog and the save remember stays.
    rig.send(PlayerReportMessage::ResetPrefs);
    let p = &rig.state.prefs.player_report;
    assert_eq!(
        (p.opponents, p.eco_codes, p.max_table_games, p.stats),
        (6, 6, 500, true)
    );
    assert_eq!(p.last_color, White);
}

#[test]
fn the_options_survive_the_preferences_file() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(PlayerReportMessage::SetPrefs(Box::new(PlayerReportPrefs {
        newest: 9,
        matching: NameMatching::Prefix,
        copy_to_clipbase: true,
        last_dir: Some(PathBuf::from("out")),
        ..PlayerReportPrefs::default()
    })));
    let text = render_preferences(&rig.state.prefs);
    assert!(text.contains("[player_report]"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.player_report, *rig.state.prefs.player_report);
}

// ---- the words (criterion 18, the part of the application) ----

fn ftl_of(language: &str, file: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn ftl(language: &str) -> BTreeMap<String, String> {
    ftl_of(language, "preport.ftl")
}

#[test]
fn every_label_and_notice_key_exists_in_both_languages() {
    use PlayerReportNotice as N;
    let notices = [
        N::Unavailable,
        N::NoGames,
        N::InvalidName,
        N::NoReport,
        N::Stale,
        N::Busy,
        N::Failed,
        N::UnsupportedPosition,
        N::BadLine,
        N::Interrupted,
        N::FilterSet,
        N::Saved,
        N::SaveFailed,
        N::SaveProtected,
        N::ViewFailed,
        N::ClipbaseSelf,
        N::NoGameName,
    ];
    for language in ["en-US", "pt-BR"] {
        let words = ftl(language);
        let shared = ftl_of(language, "oprep.ftl");
        for key in player_label_keys() {
            assert!(
                words.contains_key(&key) || shared.contains_key(&key),
                "{language}: {key}"
            );
        }
        for notice in notices {
            assert!(
                words.contains_key(notice.message_key()),
                "{language}: {notice:?}"
            );
        }
        for key in [
            "menu-tools-player-report",
            "preport-title",
            "preport-dialog-title",
            "preport-player",
            "preport-color",
            "preport-color-white",
            "preport-color-black",
            "preport-start",
            "preport-start-beginning",
            "preport-start-position",
            "preport-beginning",
            "preport-after",
            "preport-openings",
            "preport-clipbase",
            "preport-clipbase-confirm",
            "preport-change",
            "preport-matching",
            "preport-matching-exact",
            "preport-matching-prefix",
            "preport-matching-contains",
            "preport-none",
            "preport-pov",
            "preport-no-games",
        ] {
            assert!(words.contains_key(key), "{language}: {key}");
        }
        for (key, value) in &words {
            assert!(!value.trim().is_empty(), "{language}: {key} is empty");
            assert!(
                !value.to_lowercase().contains("variação"),
                "{language}: {key}"
            );
        }
    }
    let (en, pt) = (ftl("en-US"), ftl("pt-BR"));
    assert_eq!(en.keys().collect::<Vec<_>>(), pt.keys().collect::<Vec<_>>());
    assert_ne!(en["preport-label-player"], pt["preport-label-player"]);
}

#[test]
fn the_legend_says_that_all_games_is_the_whole_database() {
    for language in ["en-US", "pt-BR"] {
        let all = ftl_of(language, "oprep.ftl")["oprep-label-all-games"].clone();
        let pov = &ftl(language)["preport-label-pov"];
        assert!(pov.contains(&format!("\"{all}\"")), "{language}: {pov}");
    }
}

#[test]
fn the_labels_default_to_english_without_a_lookup() {
    assert_eq!(player_labels(&|_, _| None), PlayerLabels::default());
}
