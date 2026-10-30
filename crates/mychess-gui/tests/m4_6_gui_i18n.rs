//! Texts of the annotation and of the engine modes in both languages (spec
//! M4-6 "Textos de interface", D913; spec M4-8 D917): parity of the files,
//! no duplicated ids, every variant of the notices with its arguments, the
//! vocabulary of the Scid in Portuguese, and the status bar saying the same
//! as the dialog. Headless.
#![allow(
    clippy::too_many_lines,
    clippy::redundant_closure_for_method_calls,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::implicit_clone,
    clippy::assert_is_empty
)]

mod m4_68_gui_common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use fluent_syntax::ast::Entry;
use fluent_syntax::parser;
use m4_68_gui_common::*;
use mychess_app::{AnnotationProgress, ModeKind};
use mychess_gui::i18n::Language;
use mychess_gui::windows::annotate_dialog::{notice_text, progress_text};
use mychess_gui::windows::status_bar::app_notice_text;

const LANGUAGES: [&str; 2] = ["en-US", "pt-BR"];

/// `id -> placeholders` of every message of one file.
fn messages(language: &str, file: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = ftl_text(language, file);
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ')
            && let Some((id, _)) = line.split_once(" =")
        {
            let id = id.trim();
            current = Some(id.to_owned());
            out.entry(id.to_owned()).or_default();
        }
        if let Some(id) = &current {
            let set = out.get_mut(id).expect("entry");
            let mut rest = line;
            while let Some(at) = rest.find('$') {
                rest = &rest[at + 1..];
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                set.insert(name);
            }
        }
    }
    out
}

fn ftl_files(language: &str) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language);
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .expect("the i18n directory")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".ftl"))
        .collect();
    files.sort();
    files
}

fn every_annotate_notice() -> Vec<AnnotateNotice> {
    vec![
        AnnotateNotice::AlreadyRunning,
        AnnotateNotice::NoEngine,
        AnnotateNotice::TrialMode,
        AnnotateNotice::NothingToAnnotate,
        AnnotateNotice::Busy,
        AnnotateNotice::Finished {
            plies: 31,
            variations: 4,
        },
        AnnotateNotice::Cancelled { plies: 31 },
        AnnotateNotice::GameChanged { plies: 31 },
        AnnotateNotice::EngineFailed { plies: 31 },
        AnnotateNotice::NoScore { plies: 31 },
    ]
}

fn every_stop_reason() -> [StopReason; 9] {
    [
        StopReason::Checkmate,
        StopReason::Stalemate,
        StopReason::InsufficientMaterial,
        StopReason::FiftyMoves,
        StopReason::Repetition,
        StopReason::NoMove,
        StopReason::EngineError,
        StopReason::UserStopped,
        StopReason::GameChanged,
    ]
}

fn every_mode_notice() -> Vec<ModeNotice> {
    let mut all = vec![
        ModeNotice::NullMoveInCheck,
        ModeNotice::Conflict,
        ModeNotice::TrialMode,
        ModeNotice::ShootOutBusy,
    ];
    for kind in [ModeKind::Threat, ModeKind::ShootOut, ModeKind::Automove] {
        for reason in every_stop_reason() {
            all.push(ModeNotice::Ended { kind, reason });
        }
    }
    all
}

// ---- files ----

#[test]
fn annotate_ftl_has_the_same_ids_and_placeholders_in_both_languages() {
    let en = messages("en-US", "annotate.ftl");
    let pt = messages("pt-BR", "annotate.ftl");
    assert!(en.len() > 60, "only {} ids: the scan is broken", en.len());
    assert_eq!(
        en.keys().collect::<Vec<_>>(),
        pt.keys().collect::<Vec<_>>(),
        "ids differ between the languages"
    );
    for (id, vars) in &en {
        assert_eq!(vars, &pt[id], "{id}: placeholders differ");
    }
}

#[test]
fn analysis_ftl_has_the_same_ids_and_placeholders_in_both_languages() {
    let en = messages("en-US", "analysis.ftl");
    let pt = messages("pt-BR", "analysis.ftl");
    assert_eq!(en.keys().collect::<Vec<_>>(), pt.keys().collect::<Vec<_>>());
    for (id, vars) in &en {
        assert_eq!(vars, &pt[id], "{id}: placeholders differ");
    }
}

#[test]
fn both_languages_ship_the_same_ftl_files() {
    assert_eq!(ftl_files("en-US"), ftl_files("pt-BR"));
    assert!(ftl_files("en-US").contains(&"annotate.ftl".to_owned()));
}

#[test]
fn the_files_parse_without_errors() {
    for language in LANGUAGES {
        for file in ["annotate.ftl", "analysis.ftl", "menu.ftl", "status.ftl"] {
            let text = ftl_text(language, file);
            if let Err((_, errors)) = parser::parse(text.clone()) {
                panic!("{language}/{file}: {errors:?}");
            }
        }
    }
}

#[test]
fn no_id_is_defined_twice_across_the_ftl_files_of_a_language() {
    for language in LANGUAGES {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for file in ftl_files(language) {
            let text = ftl_text(language, &file);
            let Ok(resource) = parser::parse(text) else {
                panic!("{language}/{file} does not parse");
            };
            for entry in resource.body {
                if let Entry::Message(m) = entry {
                    let id = m.id.name.to_owned();
                    if let Some(other) = seen.insert(id.clone(), file.clone()) {
                        panic!("{language}: `{id}` is in {other} and in {file}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_new_menu_item_has_both_texts() {
    let en = messages("en-US", "menu.ftl");
    let pt = messages("pt-BR", "menu.ftl");
    assert!(en.contains_key("menu-tools-annotate"));
    assert!(pt.contains_key("menu-tools-annotate"));
}

#[test]
fn portuguese_says_variante_never_variacao_in_the_new_files() {
    for file in ["annotate.ftl", "analysis.ftl"] {
        let text = ftl_text("pt-BR", file).to_lowercase();
        assert!(
            !text.contains("variaç") && !text.contains("variac"),
            "{file} says variacao (D913): use variante"
        );
    }
    assert!(ftl_text("pt-BR", "annotate.ftl").contains("variante"));
}

#[test]
fn portuguese_is_translated_not_copied() {
    let en = messages("en-US", "annotate.ftl");
    let en_text = ftl_text("en-US", "annotate.ftl");
    let pt_text = ftl_text("pt-BR", "annotate.ftl");
    let value = |text: &str, id: &str| -> String {
        text.lines()
            .find(|l| l.starts_with(&format!("{id} = ")))
            .map(|l| l.split_once(" = ").unwrap().1.to_owned())
            .unwrap_or_default()
    };
    let same: Vec<&String> = en
        .keys()
        .filter(|id| {
            let (e, p) = (value(&en_text, id), value(&pt_text, id));
            !e.is_empty() && e == p
        })
        .collect();
    // Only symbols (the score formats) may be identical.
    for id in &same {
        assert!(
            id.starts_with("annotate-format-") || **id == "annotate-engine",
            "{id} was not translated"
        );
    }
}

// ---- the keys of the code exist ----

#[test]
fn every_key_the_dialog_and_the_buttons_name_exists_in_both_languages() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for language in LANGUAGES {
        for file in ftl_files(language) {
            ids.extend(messages(language, &file).into_keys());
        }
    }
    let mut checked = 0;
    for file in [
        "src/windows/annotate_dialog.rs",
        "src/windows/analysis.rs",
        "src/windows/status_bar.rs",
        "src/windows/menu_bar.rs",
        "../mychess-app/src/annotate/message.rs",
        "../mychess-app/src/training/message.rs",
        "../mychess-app/src/training/mode.rs",
    ] {
        let text = std::fs::read_to_string(root.join(file)).expect("source");
        let body = text.split("#[cfg(test)]").next().expect("body");
        for part in body.split('"').skip(1).step_by(2) {
            let is_key = [
                "annotate-",
                "analysis-btn-",
                "analysis-tip-",
                "analysis-mode-",
                "analysis-training-",
                "analysis-threat-",
                "menu-tools-annotate",
            ]
            .iter()
            .any(|p| part.starts_with(p))
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if is_key
                && !part.ends_with('-')
                && !part.ends_with("-box")
                && !part.ends_with("-input")
            {
                checked += 1;
                assert!(
                    ids.contains(part),
                    "{file} names `{part}`: no .ftl defines it"
                );
            }
        }
    }
    assert!(
        checked > 30,
        "only {checked} keys found: the scan is broken"
    );
}

// ---- every variant, both languages, with arguments ----

fn assert_clean(text: &str, what: &str) {
    assert!(!text.trim().is_empty(), "{what}: empty");
    assert!(!text.contains('{') && !text.contains('}'), "{what}: {text}");
    assert!(!text.contains('$'), "{what}: {text}");
    // A missing id would be shown as the id itself.
    assert!(
        !text.starts_with("annotate-") && !text.starts_with("analysis-"),
        "{what}: untranslated {text}"
    );
    assert!(
        !text.contains('\u{2068}') && !text.contains('\u{2069}'),
        "{what}: isolation marks leaked into {text:?}"
    );
}

#[test]
fn every_annotate_notice_is_clean_text_with_its_numbers_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in every_annotate_notice() {
            let text = notice_text(&i18n, &notice);
            assert_clean(&text, &format!("{language:?} {notice:?}"));
            if let AnnotateNotice::Finished { .. } = notice {
                assert!(text.contains("31") && text.contains('4'), "{text}");
            } else if !matches!(
                notice,
                AnnotateNotice::AlreadyRunning
                    | AnnotateNotice::NoEngine
                    | AnnotateNotice::TrialMode
                    | AnnotateNotice::NothingToAnnotate
                    | AnnotateNotice::Busy
            ) {
                assert!(text.contains("31"), "{language:?} {notice:?}: {text}");
            }
        }
    }
}

#[test]
fn the_finished_remark_has_singular_and_plural_in_both_languages() {
    let at = |language, plies, variations| {
        notice_text(
            &I18n::new(language),
            &AnnotateNotice::Finished { plies, variations },
        )
    };
    assert_eq!(
        at(Language::EnUs, 1, 1),
        "Annotation finished: 1 move, 1 variation."
    );
    assert_eq!(
        at(Language::EnUs, 2, 0),
        "Annotation finished: 2 moves, 0 variations."
    );
    assert_eq!(
        at(Language::PtBr, 1, 1),
        "Anotação concluída: 1 lance, 1 variante."
    );
    assert_eq!(
        at(Language::PtBr, 5, 3),
        "Anotação concluída: 5 lances, 3 variantes."
    );
}

#[test]
fn the_remarks_survive_extreme_counts() {
    for plies in [0, 1, u32::MAX] {
        for language in [Language::EnUs, Language::PtBr] {
            let i18n = I18n::new(language);
            let text = notice_text(
                &i18n,
                &AnnotateNotice::Finished {
                    plies,
                    variations: plies,
                },
            );
            assert_clean(&text, &format!("{plies}"));
            assert!(text.contains(&plies.to_string()), "{text}");
        }
    }
}

#[test]
fn every_mode_notice_and_stop_reason_is_clean_text_in_both_languages() {
    let mut seen = BTreeSet::new();
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in every_mode_notice() {
            let text = app_notice_text(&i18n, &AppNotice::Mode(notice.clone()));
            assert_clean(&text, &format!("{language:?} {notice:?}"));
            seen.insert((language == Language::PtBr, notice.message_key()));
        }
        for reason in every_stop_reason() {
            assert!(
                i18n.has(reason.message_key()),
                "{reason:?}: key {} missing",
                reason.message_key()
            );
        }
    }
    // Every key exists in the English and Portuguese files themselves.
    let en = messages("en-US", "analysis.ftl");
    let pt = messages("pt-BR", "analysis.ftl");
    let status = messages("en-US", "status.ftl");
    let status_pt = messages("pt-BR", "status.ftl");
    for notice in every_mode_notice() {
        let key = notice.message_key();
        assert!(
            (en.contains_key(key) && pt.contains_key(key))
                || (status.contains_key(key) && status_pt.contains_key(key)),
            "{key} is not in both languages"
        );
    }
}

#[test]
fn the_stop_reasons_have_different_texts() {
    let i18n = I18n::new(Language::EnUs);
    let texts: BTreeSet<String> = every_stop_reason()
        .iter()
        .map(|r| i18n.tr(r.message_key()))
        .collect();
    assert_eq!(texts.len(), 9);
}

#[test]
fn the_status_bar_says_what_the_dialog_module_says() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in every_annotate_notice() {
            assert_eq!(
                app_notice_text(&i18n, &AppNotice::Annotate(notice)),
                notice_text(&i18n, &notice),
                "{notice:?}"
            );
        }
    }
}

#[test]
fn the_remark_reaches_the_window_in_the_language_of_the_rig() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    rig.set_game(game_of(&SCHOLAR));
    rig.app_msg(AppMessage::Annotate(AnnotateMessage::Open));
    rig.app_msg(AppMessage::Annotate(AnnotateMessage::Start));
    tick_status(&mut rig);
    assert!(window_has(
        &rig,
        "Nenhuma engine está em execução e nenhuma está definida para F2."
    ));
}

// ---- the progress ----

#[test]
fn the_progress_text_has_its_words_and_numbers_in_both_languages() {
    let at = |done, total| AnnotationProgress {
        done_plies: done,
        total_plies: total,
        game_index: 1,
        game_total: 1,
    };
    let (en, pt) = (I18n::new(Language::EnUs), I18n::new(Language::PtBr));
    assert_eq!(progress_text(&en, &at(0, 40)), "Annotating... move 1 of 40");
    assert_eq!(progress_text(&pt, &at(0, 40)), "Anotando... lance 1 de 40");
    assert_eq!(
        progress_text(&en, &at(39, 40)),
        "Annotating... move 40 of 40"
    );
    assert_eq!(
        progress_text(&en, &at(40, 40)),
        "Annotating... move 40 of 40"
    );
    assert_eq!(
        progress_text(&en, &at(u32::MAX, u32::MAX)),
        format!("Annotating... move {0} of {0}", u32::MAX)
    );
    assert!(!progress_text(&pt, &at(0, 0)).is_empty());
    assert_clean(&progress_text(&pt, &at(0, 0)), "starting");
}

// ---- the texts of the dialog ----

#[test]
fn the_estimate_texts_have_their_numbers_and_plural_in_portuguese() {
    let (mut rig, _run) = rig_with_engine(&["e2e4"]);
    rig.shell.app.annotate_options.control = Control::Depth;
    ann(&mut rig, AnnotateMessage::Open);
    assert!(
        dialog_sim(&rig, false, &pt())
            .find("1 lance a anotar")
            .is_ok()
    );
    rig.set_game(game_of(&SCHOLAR));
    let o = dialog_options(&rig);
    ann(&mut rig, AnnotateMessage::SetOptions(o));
    let mut s = dialog_sim(&rig, false, &pt());
    assert!(s.find("7 lances a anotar").is_ok());
}

#[test]
fn the_names_of_the_engines_are_not_translated_or_altered() {
    let mut rig = Rig::new();
    rig.set_game(game_of(&SCHOLAR));
    let name = "Stockfish 17 \u{2068}x\u{2069}";
    let id = add_engine(&mut rig, name, Protocol::Uci);
    start_with(&mut rig, id, identity(name, Some(1..=5), true));
    ann(&mut rig, AnnotateMessage::Open);
    let mut s = dialog_sim(&rig, false, &pt());
    assert!(s.find(name).is_ok());
}
