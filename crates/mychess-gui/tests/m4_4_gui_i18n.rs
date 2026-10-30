//! Texts of the analysis panel in both languages (spec M4-4, "Textos de
//! interface", D815, D817): parity of `analysis.ftl`, no duplicated ids, the
//! keys the code uses, the remarks with arguments under hostile input, and
//! the status bar. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_4_gui_common;

use std::collections::{BTreeMap, BTreeSet};

use fluent_bundle::FluentArgs;
use fluent_syntax::ast::Entry;
use fluent_syntax::parser;
use m4_4_gui_common::*;
use mychess_app::{AnalysisNotice, AppNotice};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::analysis_text::notice_text;
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
            && let Some((id, _)) = line.split_once(" = ")
        {
            current = Some(id.to_owned());
            out.entry(id.to_owned()).or_default();
        }
        if let Some(id) = &current {
            let set = out.get_mut(id).unwrap();
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

fn all_notices() -> Vec<AnalysisNotice> {
    // `AnalysisNotice` is `#[non_exhaustive]`: the list below is checked
    // against the application's source by `every_notice_key_in_the_app_is_listed`.
    vec![
        AnalysisNotice::AtGameEnd,
        AnalysisNotice::LineTruncated(0),
        AnalysisNotice::LineTruncated(1),
        AnalysisNotice::LineTruncated(2),
        AnalysisNotice::LineTruncated(1_000_000),
        AnalysisNotice::LineTruncated(usize::MAX),
        AnalysisNotice::ExcludeInvalid("Zz9".to_owned()),
        AnalysisNotice::ExcludeAll,
        AnalysisNotice::ExcludeUnsupported,
        AnalysisNotice::MultiPvUnsupported,
        AnalysisNotice::Locked,
        AnalysisNotice::LogSaved,
    ]
}

// ---- files ----

#[test]
fn both_languages_define_exactly_the_same_analysis_ids_with_the_same_placeholders() {
    let en = messages("en-US", "analysis.ftl");
    let pt = messages("pt-BR", "analysis.ftl");
    let only_en: Vec<_> = en.keys().filter(|k| !pt.contains_key(*k)).collect();
    let only_pt: Vec<_> = pt.keys().filter(|k| !en.contains_key(*k)).collect();
    assert!(only_en.is_empty(), "only in en-US: {only_en:?}");
    assert!(only_pt.is_empty(), "only in pt-BR: {only_pt:?}");
    for (id, a) in &en {
        assert_eq!(a, &pt[id], "{id}: placeholders differ");
    }
    assert!(en.len() > 50, "the file is not empty ({} ids)", en.len());
}

#[test]
fn analysis_ftl_has_valid_syntax_without_junk_and_no_empty_values() {
    for language in LANGUAGES {
        let source = ftl_text(language, "analysis.ftl");
        assert!(!source.starts_with('\u{feff}'), "{language}: BOM");
        assert!(!source.contains('\r'), "{language}: CRLF");
        let resource = parser::parse(source.as_str())
            .unwrap_or_else(|(_, errors)| panic!("{language}: {errors:?}"));
        for entry in &resource.body {
            if let Entry::Junk { content } = entry {
                panic!("{language}: junk: {content}");
            }
            if let Entry::Message(m) = entry {
                assert!(m.value.is_some(), "{language}: {} has no value", m.id.name);
            }
        }
    }
}

#[test]
fn no_id_is_defined_twice_across_the_files_of_a_language() {
    for language in LANGUAGES {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/i18n")
            .join(language);
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "ftl"))
            .collect();
        entries.sort();
        for path in entries {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            for id in messages(language, &file).keys() {
                if let Some(first) = seen.insert(id.clone(), file.clone()) {
                    panic!("{language}: {id} is in {first} and in {file}");
                }
            }
        }
    }
}

#[test]
fn the_menu_entries_of_the_panel_exist_in_both_languages() {
    for language in LANGUAGES {
        let menu = messages(language, "menu.ftl");
        assert!(
            menu.contains_key("menu-tools-analysis-window"),
            "{language}"
        );
        let analysis = messages(language, "analysis.ftl");
        for id in [
            "menu-view-pane-analysis",
            "pane-analysis",
            "action-toggle-analysis-pane",
        ] {
            assert!(
                analysis.contains_key(id) || menu.contains_key(id),
                "{language}: {id}"
            );
        }
    }
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    assert_eq!(en.tr("menu-tools-analysis-window"), "Analysis window");
    assert_eq!(
        pt.tr("menu-tools-analysis-window"),
        "Janela de an\u{e1}lise"
    );
    assert_ne!(en.tr("pane-analysis"), "pane-analysis");
    assert_ne!(pt.tr("pane-analysis"), "pane-analysis");
}

#[test]
fn the_portuguese_texts_are_translated_not_copied_from_english() {
    let en = messages("en-US", "analysis.ftl");
    let en_text = ftl_text("en-US", "analysis.ftl");
    let pt_text = ftl_text("pt-BR", "analysis.ftl");
    let value = |text: &str, id: &str| -> String {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{id} = ")))
            .unwrap_or_default()
            .to_owned()
    };
    // Words that must differ (not names or symbols).
    for id in [
        "analysis-empty",
        "analysis-state-stopped",
        "analysis-btn-lock",
        "analysis-btn-exclude",
        "analysis-tip-lock",
        "analysis-log-empty",
        "analysis-notice-exclude-all",
        "analysis-click-to-see",
    ] {
        assert!(en.contains_key(id), "{id}");
        assert_ne!(
            value(&en_text, id),
            value(&pt_text, id),
            "{id}: untranslated"
        );
    }
}

#[test]
fn the_english_tooltips_keep_the_scid_phrases() {
    // Equivalences of the spec (english.tcl): the tips are the Scid's.
    let en = I18n::new(Language::EnUs);
    for (key, phrase) in [
        ("analysis-tip-add-move", "add move"),
        ("analysis-tip-add-line", "add variation"),
        ("analysis-tip-add-all", "add all variations"),
        ("analysis-tip-lock", "lock engine to current position"),
        ("analysis-tip-exclude", "exclude move"),
        ("analysis-tip-priority", "low cpu priority"),
        ("analysis-tip-info", "show engine info"),
        ("analysis-click-to-see", "click here to see the moves"),
        ("analysis-lines", "lines"),
    ] {
        assert!(
            en.tr(key).to_lowercase().contains(phrase),
            "{key}: {:?} lacks {phrase:?}",
            en.tr(key)
        );
    }
}

// ---- keys used by the code ----

fn source(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every `"analysis-..."` literal of a source text.
fn analysis_literals(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("\"analysis-") {
        rest = &rest[at + 1..];
        let key: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        if key.len() > "analysis-".len() && !key.ends_with('-') {
            out.insert(key);
        }
    }
    out
}

#[test]
fn every_analysis_key_the_gui_code_uses_exists_in_both_languages() {
    let mut used = analysis_literals(&source("src/windows/analysis.rs"));
    used.extend(analysis_literals(&source("src/windows/analysis_text.rs")));
    used.extend(analysis_literals(&source("src/shell/analysis_glue.rs")));
    used.extend(analysis_literals(&source("src/host.rs")));
    assert!(used.len() > 30, "the scan found keys ({})", used.len());
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in &used {
            // `analysis-` prefixes that are not keys (such as ids of widgets).
            if key.ends_with("-input") || key == "analysis-pane" || key == "analysis-log-search" {
                continue;
            }
            assert!(i18n.has(key), "{language:?}: missing key {key}");
        }
    }
}

#[test]
fn every_notice_key_in_the_app_is_listed_here() {
    let app = source("../mychess-app/src/analysis/message.rs");
    let in_source = analysis_literals(&app);
    let listed: BTreeSet<String> = all_notices()
        .iter()
        .map(|n| n.message_key().to_owned())
        .collect();
    let notice_keys: BTreeSet<String> = in_source
        .into_iter()
        .filter(|k| k.starts_with("analysis-notice-"))
        .collect();
    assert_eq!(
        notice_keys, listed,
        "a notice was added or removed: update all_notices() and the .ftl files (D817)"
    );
    for language in LANGUAGES {
        let defined = messages(language, "analysis.ftl");
        for key in &listed {
            assert!(defined.contains_key(key), "{language}: {key}");
        }
        let notice_ids: BTreeSet<_> = defined
            .keys()
            .filter(|k| k.starts_with("analysis-notice-"))
            .cloned()
            .collect();
        assert_eq!(
            notice_ids, listed,
            "{language}: ids of notices without a variant"
        );
    }
}

#[test]
fn the_dialog_titles_of_the_log_exist() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in ["analysis-log-save-title", "analysis-log-filter"] {
            assert!(i18n.has(key), "{key}");
            assert_ne!(i18n.tr(key), key);
        }
    }
}

// ---- the remarks ----

#[test]
fn every_remark_has_text_in_both_languages_and_is_not_the_key() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in all_notices() {
            let text = notice_text(&i18n, &notice);
            assert!(!text.trim().is_empty(), "{notice:?}");
            assert_ne!(
                text,
                notice.message_key(),
                "{language:?} {notice:?}: missing key"
            );
            assert!(
                !text.contains("$count") && !text.contains("$token"),
                "{text}"
            );
            // The status bar uses the same text.
            assert_eq!(
                app_notice_text(&i18n, &AppNotice::Analysis(notice.clone())),
                text
            );
        }
    }
}

#[test]
fn remarks_carry_their_arguments() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for count in [0usize, 1, 2, 7, 1_000_000, usize::MAX] {
            let text = notice_text(&i18n, &AnalysisNotice::LineTruncated(count));
            // usize::MAX does not fit an i64: the text still has digits.
            assert!(text.chars().any(|c| c.is_ascii_digit()), "{count}: {text}");
            if count < 1_000_000 {
                assert!(text.contains(&count.to_string()), "{count}: {text}");
            }
        }
        let text = notice_text(&i18n, &AnalysisNotice::ExcludeInvalid("Zz9".to_owned()));
        assert!(text.contains("Zz9"), "{text}");
    }
}

#[test]
fn hostile_fluent_syntax_in_a_token_is_shown_verbatim() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for token in [
            "{ $token }",
            "{ $count }",
            "}{",
            "{",
            "\"",
            "-term",
            "{ -brand }",
            "{ NUMBER(1) }",
            "\u{2068}\u{2069}",
            "\u{202e}e4",
            "a\nb",
            "\u{0}",
            "\u{1F600}",
            "",
        ] {
            let text = notice_text(&i18n, &AnalysisNotice::ExcludeInvalid(token.to_owned()));
            assert!(text.contains(token), "{token:?} -> {text:?}");
        }
        let long = "x".repeat(100_000);
        let text = notice_text(&i18n, &AnalysisNotice::ExcludeInvalid(long.clone()));
        assert!(text.contains(&long));
    }
}

#[test]
fn texts_with_arguments_show_hostile_values_verbatim() {
    let i18n = I18n::new(Language::EnUs);
    let hostile = [
        "{ $depth }",
        "{ $moves }",
        "}",
        "{ NUMBER($x) }",
        "\u{202e}",
        "a\r\nb",
    ];
    let cases: &[(&str, &[&str])] = &[
        ("analysis-history-line", &["moves"]),
        ("analysis-lock-file-game", &["file", "move"]),
        ("analysis-lock-game", &["move"]),
        ("analysis-tip-unlock", &["description"]),
        ("analysis-locked-hint", &["description"]),
        ("analysis-tip-excluded", &["moves"]),
        ("analysis-state-failed", &["reason"]),
        ("analysis-info-main", &["score", "depth", "nodes", "time"]),
        (
            "analysis-info-extra",
            &["nps", "hash", "load", "tb", "current"],
        ),
    ];
    for (key, names) in cases {
        for h in hostile {
            let mut args = FluentArgs::new();
            for name in *names {
                args.set(*name, h);
            }
            // The numeric-looking placeholders too.
            args.set("depth", h);
            let text = i18n.tr_args(key, &args);
            assert!(text.contains(h), "{key} with {h:?}: {text:?}");
        }
    }
}

#[test]
fn the_history_line_follows_the_scid_layout_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        let mut args = FluentArgs::new();
        args.set("depth", 12_i64);
        args.set("score", "+0.35");
        args.set("moves", "1.e4 e5");
        args.set("time", "1.23");
        assert_eq!(
            i18n.tr_args("analysis-history-line", &args),
            "12 [+0.35]  1.e4 e5 (1.23 s)"
        );
    }
}

// ---- the status bar in the real window ----

#[test]
fn an_analysis_remark_reaches_the_status_bar_of_the_window() {
    let (mut rig, run) = uci_run_with_lines();
    rig.msg(aui(AnalysisUiMessage::OpenExclude(run)));
    rig.msg(aui(AnalysisUiMessage::ExcludeInput("Zz9".to_owned())));
    rig.msg(aui(AnalysisUiMessage::ExcludeSubmit(run)));
    assert!(
        has(&rig, "\"Zz9\" is not a legal move here."),
        "the remark is in the window"
    );
}

#[test]
fn the_remark_follows_the_language_of_the_window() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
    );
    rig.msg(aui(AnalysisUiMessage::OpenExclude(run)));
    rig.msg(aui(AnalysisUiMessage::ExcludeInput("Zz9".to_owned())));
    rig.msg(aui(AnalysisUiMessage::ExcludeSubmit(run)));
    let s = rig
        .sim(SIZE)
        .find("\"Zz9\" n\u{e3}o \u{e9} um lance legal aqui.")
        .is_ok();
    assert!(s, "in Portuguese");
}

#[test]
fn the_panel_is_drawn_in_both_languages_with_every_state() {
    for language in [
        mychess_app::LanguagePref::EnUs,
        mychess_app::LanguagePref::PtBr,
    ] {
        let mut rig = Rig::with_language(language);
        let run = uci_run(&mut rig);
        three_lines(&mut rig, run);
        for message in [
            AnalysisMessage::ToggleInfo(run),
            AnalysisMessage::ToggleLock(run),
            AnalysisMessage::OpenExclude(run),
            AnalysisMessage::CloseExclude(run),
            AnalysisMessage::ToggleLock(run),
            AnalysisMessage::OpenLog(run),
            AnalysisMessage::CloseLog(run),
            AnalysisMessage::Toggle(run),
        ] {
            rig.app_msg(AppMessage::Analysis(message));
            drop(rig.sim(SIZE).find("Fake"));
        }
    }
}
