//! Texts of the engine management in both languages (spec M4-3, D746,
//! "Textos de interface"): parity of the Fluent files, the keys the code
//! uses, and the texts with arguments under hostile input. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_3_gui_common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use m4_3_gui_common::*;
use mychess_app::engines::{
    ArgsError, CommandWarning, EditorError, EngineFailure, EngineNotice, FailureKind, Hotkey,
    ImportWarning, LoadIssue, Phase, ScidWarning, ValidationError,
};
use mychess_app::{AppNotice, FileError, FileErrorKind};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::engine_text::{
    STDERR_CHARS, command_warning_text, editor_error_text, engine_notice_text, failure_text,
    import_warning_text, load_issue_text, option_error_text, option_label,
};
use mychess_gui::windows::status_bar::app_notice_text;

fn i18n_dir(language: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
}

/// `id -> placeholders` of every message of a file.
fn messages(language: &str, file: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = std::fs::read_to_string(i18n_dir(language).join(file)).unwrap();
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

fn plain(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

fn failure_of(kind: FailureKind, code: Option<i32>, tail: &[&str]) -> EngineFailure {
    EngineFailure {
        kind,
        exit_code: code,
        stderr_tail: tail.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn all_kinds() -> Vec<FailureKind> {
    vec![
        FailureKind::CannotStart,
        FailureKind::ExitedEarly,
        FailureKind::Crashed,
        FailureKind::Timeout(Phase::Uciok),
        FailureKind::Timeout(Phase::Readyok),
        FailureKind::Timeout(Phase::Bestmove),
        FailureKind::Timeout(Phase::Features),
        FailureKind::Timeout(Phase::Pong),
        FailureKind::StopTimeout,
        FailureKind::NotAnEngine,
        FailureKind::Io,
        FailureKind::Closed,
        FailureKind::Other,
    ]
}

fn all_notices() -> Vec<EngineNotice> {
    let mut notices = vec![
        EngineNotice::FileUnreadable,
        EngineNotice::NewerVersion,
        EngineNotice::FileFixed { count: 3 },
        EngineNotice::SaveFailed,
        EngineNotice::NoEngineOnKey(Hotkey::F2),
        EngineNotice::NoEngineOnKey(Hotkey::F3),
        EngineNotice::NoEngineOnKey(Hotkey::F4),
        EngineNotice::NoEngineRunning,
        EngineNotice::ListFull,
        EngineNotice::NotFound,
        EngineNotice::EntryInvalid,
        EngineNotice::RestartToApply,
        EngineNotice::ManyRunning { count: 5 },
        EngineNotice::CommandRefused,
        EngineNotice::Imported {
            added: 4,
            duplicates: 5,
            invalid: 6,
        },
        EngineNotice::NothingToImport,
    ];
    for kind in [
        FileErrorKind::NotFound,
        FileErrorKind::PermissionDenied,
        FileErrorKind::TooLarge { limit: 4 << 20 },
        FileErrorKind::UnsupportedEncoding("UTF-16"),
        FileErrorKind::CorruptGzip,
        FileErrorKind::Io(std::io::ErrorKind::Other),
    ] {
        notices.push(EngineNotice::ImportFailed(FileError::new(kind, None)));
    }
    for kind in all_kinds() {
        let failure = failure_of(kind, Some(2), &["oops"]);
        notices.push(EngineNotice::StartFailed {
            name: "E".to_owned(),
            failure: failure.clone(),
        });
        notices.push(EngineNotice::Crashed {
            name: "E".to_owned(),
            failure: failure.clone(),
        });
        notices.push(EngineNotice::FoundRejected {
            path: PathBuf::from("bin").join("fish"),
            failure,
        });
    }
    notices
}

fn all_editor_errors() -> Vec<EditorError> {
    vec![
        EditorError::Invalid(ValidationError::EmptyName),
        EditorError::Invalid(ValidationError::EmptyCommand),
        EditorError::Invalid(ValidationError::NonUtf8Path),
        EditorError::Invalid(ValidationError::EloOutOfRange(4001)),
        EditorError::Invalid(ValidationError::TextTooLong {
            field: "name",
            limit: 4096,
        }),
        EditorError::Invalid(ValidationError::TooManyItems {
            field: "args",
            limit: 512,
        }),
        EditorError::Args(ArgsError::UnclosedQuote),
        EditorError::EloNotANumber,
    ]
}

fn all_warnings() -> [CommandWarning; 3] {
    [
        CommandWarning::CommandNotFound,
        CommandWarning::NotExecutable,
        CommandWarning::WorkingDirMissing,
    ]
}

// ---- the files ----

#[test]
fn engines_ftl_defines_the_same_ids_with_the_same_placeholders_in_both_languages() {
    let en = messages("en-US", "engines.ftl");
    let pt = messages("pt-BR", "engines.ftl");
    assert_eq!(
        en.keys().collect::<Vec<_>>(),
        pt.keys().collect::<Vec<_>>(),
        "ids in only one language"
    );
    for (id, vars) in &en {
        assert_eq!(
            vars, &pt[id],
            "{id}: placeholders differ between en-US and pt-BR"
        );
    }
}

#[test]
fn no_id_is_defined_twice_across_the_ftl_files_of_a_language() {
    for language in ["en-US", "pt-BR"] {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut files: Vec<_> = std::fs::read_dir(i18n_dir(language))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| {
                Path::new(n)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("ftl"))
            })
            .collect();
        files.sort();
        for file in files {
            for id in ftl_ids(language, &file) {
                if let Some(first) = seen.insert(id.clone(), file.clone()) {
                    panic!("{language}: `{id}` is in {first} and in {file}");
                }
            }
        }
    }
}

#[test]
fn every_message_of_engines_ftl_has_text_in_both_languages_and_pt_br_is_translated() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for id in ftl_ids("en-US", "engines.ftl") {
            let text = i18n.tr(&id);
            assert!(!text.trim().is_empty() && text != id, "{language:?} {id}");
        }
    }
    // Spot checks: the Portuguese is not the English copied.
    let pt = pt();
    let en = en();
    for key in [
        "engines-btn-new",
        "engines-btn-delete",
        "engines-empty",
        "engines-delete-message",
        "engines-error-empty-name",
        "engines-options-save",
        "analysis-pref-title",
    ] {
        assert_ne!(pt.tr(key), en.tr(key), "{key} is not translated to pt-BR");
    }
}

#[test]
fn every_engines_key_named_in_the_gui_and_the_app_exists_in_the_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![
        root.join("crates/mychess-gui/src/windows/engines_dialog.rs"),
        root.join("crates/mychess-gui/src/windows/engine_editor.rs"),
        root.join("crates/mychess-gui/src/windows/engine_options.rs"),
        root.join("crates/mychess-gui/src/windows/engine_text.rs"),
        root.join("crates/mychess-gui/src/windows/menu_bar.rs"),
        root.join("crates/mychess-gui/src/windows/status_bar.rs"),
        root.join("crates/mychess-app/src/engines/message.rs"),
        root.join("crates/mychess-app/src/engines/editor.rs"),
        root.join("crates/mychess-app/src/engines/run.rs"),
    ];
    files.retain(|p| p.exists());
    let known: BTreeSet<String> = ftl_ids("en-US", "engines.ftl")
        .into_iter()
        .chain(ftl_ids("en-US", "menu.ftl"))
        .chain(ftl_ids("en-US", "dialogs.ftl"))
        .chain(ftl_ids("en-US", "common.ftl"))
        .chain(ftl_ids("en-US", "status.ftl"))
        // M4-6/M4-7: the Tools menu also names the graph and annotate items.
        .chain(ftl_ids("en-US", "graph.ftl"))
        .chain(ftl_ids("en-US", "annotate.ftl"))
        // M8-2b: the Tools menu also names the tournament items.
        .chain(ftl_ids("en-US", "tournament.ftl"))
        // M5-6: the Tools menu also names the opening books item.
        .chain(ftl_ids("en-US", "book.ftl"))
        // M5-7b: and the ECO browser and classify items.
        .chain(ftl_ids("en-US", "eco.ftl"))
        // M8-8: the tablebase item. M8-10c: the electronic board item.
        .chain(ftl_ids("en-US", "tablebase.ftl"))
        .chain(ftl_ids("en-US", "eboard.ftl"))
        // M6-3c: the players, tournaments and spelling group of the Tools menu.
        .chain(ftl_ids("en-US", "player_tools.ftl"))
        // M6-4d: the statistics and graphs group of the Tools menu.
        .chain(ftl_ids("en-US", "crosstab.ftl"))
        .chain(ftl_ids("en-US", "stats.ftl"))
        .chain(ftl_ids("en-US", "ptracker.ftl"))
        // M7-5c: the EPD window item. M7-2c: the opening report item.
        .chain(ftl_ids("en-US", "epd.ftl"))
        .chain(ftl_ids("en-US", "oprep.ftl"))
        // M7-3c: the player report item.
        .chain(ftl_ids("en-US", "preport.ftl"))
        // M7-1c: the maintenance item.
        .chain(ftl_ids("en-US", "maint.ftl"))
        .collect();
    let mut checked = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // The tests of the file itself are not the contract.
        let body = text.split("#[cfg(test)]").next().unwrap();
        for part in body.split('"').skip(1).step_by(2) {
            let is_key = (part.starts_with("engines-")
                || part.starts_with("analysis-pref-")
                || part.starts_with("menu-tools"))
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if is_key && !part.ends_with('-') && !part.ends_with("-box") {
                checked += 1;
                assert!(
                    known.contains(part),
                    "{} names `{part}`, which no .ftl defines",
                    file.display()
                );
            }
        }
    }
    assert!(
        checked > 80,
        "only {checked} keys found: the scan is broken"
    );
}

// ---- every variant, both languages ----

#[test]
fn every_variant_of_the_enums_has_a_key_that_resolves_in_both_languages() {
    let en_ids = ftl_ids("en-US", "engines.ftl");
    let pt_ids = ftl_ids("pt-BR", "engines.ftl");
    let mut keys: Vec<(String, &'static str)> = Vec::new();
    for n in all_notices() {
        if !matches!(n, EngineNotice::ImportFailed(_)) {
            keys.push((format!("{n:?}"), n.message_key()));
        }
    }
    for e in all_editor_errors() {
        keys.push((format!("{e:?}"), e.message_key()));
    }
    for kind in all_kinds() {
        keys.push((
            format!("{kind:?}"),
            failure_of(kind, None, &[]).message_key(),
        ));
    }
    for (what, key) in keys {
        assert!(en_ids.contains(key), "{what}: `{key}` missing in en-US");
        assert!(pt_ids.contains(key), "{what}: `{key}` missing in pt-BR");
    }
}

fn clean(text: &str, language: Language, what: &str) {
    assert!(!text.trim().is_empty(), "{language:?} {what}: empty");
    for bad in ["engines-", "analysis-pref", "{", "}", "$", "undefined"] {
        assert!(
            !text.contains(bad),
            "{language:?} {what}: `{bad}` in {text:?}"
        );
    }
}

#[test]
fn every_notice_error_warning_and_issue_renders_clean_text_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for notice in all_notices() {
            let text = plain(&engine_notice_text(&i18n, &notice));
            clean(&text, language, &format!("{notice:?}"));
            // The same text reaches the status bar through the application's notice.
            let bar = plain(&app_notice_text(&i18n, &AppNotice::Engine(notice.clone())));
            assert_eq!(bar, text, "{notice:?}");
        }
        for error in all_editor_errors() {
            clean(
                &editor_error_text(&i18n, &error),
                language,
                &format!("{error:?}"),
            );
        }
        for warning in all_warnings() {
            clean(
                &command_warning_text(&i18n, warning),
                language,
                &format!("{warning:?}"),
            );
        }
        for kind in all_kinds() {
            for code in [None, Some(0), Some(-1), Some(i32::MAX), Some(i32::MIN)] {
                clean(
                    &plain(&failure_text(&i18n, &failure_of(kind, code, &[]))),
                    language,
                    &format!("{kind:?} {code:?}"),
                );
            }
        }
        let issues = [
            LoadIssue::FieldReset {
                entry: 0,
                field: "elo",
            },
            LoadIssue::EntryDropped {
                entry: 1,
                reason: ValidationError::EmptyName,
            },
            LoadIssue::EntryNotATable { entry: 2 },
            LoadIssue::DuplicateHotkey { entry: 3 },
            LoadIssue::TooManyEngines { limit: 256 },
        ];
        for issue in &issues {
            clean(
                &plain(&load_issue_text(&i18n, issue)),
                language,
                &format!("{issue:?}"),
            );
        }
        for kind in [ScidWarning::RelativePath, ScidWarning::ArgsUnparsable] {
            let w = ImportWarning {
                engine: "E".to_owned(),
                kind,
            };
            clean(
                &plain(&import_warning_text(&i18n, &w)),
                language,
                &format!("{kind:?}"),
            );
        }
        for error in [
            mychess_app::engines::OptionError::OutOfRange {
                name: "H".into(),
                value: 0,
                min: 1,
                max: 9,
            },
            mychess_app::engines::OptionError::NotInList {
                name: "H".into(),
                value: "x".into(),
            },
            mychess_app::engines::OptionError::WrongType {
                name: "H".into(),
                expected: "an integer",
            },
            mychess_app::engines::OptionError::ControlCharacter { name: "H".into() },
        ] {
            clean(
                &option_error_text(&i18n, &error),
                language,
                &format!("{error:?}"),
            );
        }
        for name in [
            "MultiPV",
            "Hash",
            "OwnBook",
            "BookFile",
            "UCI_LimitStrength",
        ] {
            assert_ne!(option_label(&i18n, name), name, "{language:?} {name}");
        }
    }
}

#[test]
fn the_analysis_preference_labels_exist_in_both_languages_and_show_in_the_dialog() {
    let keys = [
        "analysis-pref-title",
        "analysis-pref-log-size",
        "analysis-pref-max-ply",
        "analysis-pref-add-name",
        "analysis-pref-low-priority",
        "analysis-pref-wrap-pv",
        "analysis-pref-show-info",
    ];
    for language in ["en-US", "pt-BR"] {
        let ids = ftl_ids(language, "engines.ftl");
        for key in keys {
            assert!(ids.contains(key), "{language} {key}");
        }
    }
    let (rig, _) = trio();
    let pt = pt();
    let mut s = sim_of(&rig.shell.app, true, &pt);
    for key in keys {
        assert!(s.find(pt.tr(key).as_str()).is_ok(), "{key}: {}", pt.tr(key));
    }
}

#[test]
fn the_dialogs_draw_in_portuguese_with_the_translated_buttons() {
    let (mut rig, ids) = trio();
    let pt = pt();
    {
        let mut s = sim_of(&rig.shell.app, false, &pt);
        for t in [
            "Novo",
            "Editar",
            "Copiar",
            "Excluir",
            "Fechar",
            "Iniciar",
            "Nome",
            "Tecla",
            "Tipo",
            "Último uso",
        ] {
            assert!(s.find(t).is_ok(), "{t}");
        }
    }
    send(&mut rig, EngineMessage::Edit(ids[0]));
    let mut s = sim_of(&rig.shell.app, false, &pt);
    for t in ["Testar engine", "Detectar protocolo", "Salvar", "Cancelar"] {
        assert!(s.find(t).is_ok(), "{t}");
    }
}

use mychess_app::engines::EngineMessage;

// ---- arguments under hostile input ----

#[test]
fn an_engine_name_with_fluent_syntax_is_shown_verbatim() {
    let hostile = [
        "{ $name }",
        "{ -brand }",
        "{ NUMBER(5) }",
        "{\"x\"}",
        "a } b { c",
        "\u{2068}isolate",
        "名前 \u{1F600} e\u{0301}",
        "line1\nline2",
    ];
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for name in hostile {
            let notice = EngineNotice::StartFailed {
                name: name.to_owned(),
                failure: failure_of(FailureKind::CannotStart, None, &[]),
            };
            let text = plain(&engine_notice_text(&i18n, &notice));
            assert!(
                text.starts_with(&format!("{}: ", plain(name))),
                "{language:?} {name:?} -> {text:?}"
            );
        }
    }
}

#[test]
fn an_engine_name_with_fluent_syntax_survives_the_deletion_question() {
    let (mut rig, ids) = rig_with(&[("{ $command } \u{2068} { -x }", "{ $name }")]);
    send(&mut rig, EngineMessage::Delete(ids[0]));
    assert!(has(
        &rig,
        "Delete the engine { $command } \u{2068} { -x } ({ $name })?"
    ));
}

fn stderr_part(text: &str) -> String {
    let (_, tail) = text.split_once("The engine said: ").expect("a stderr part");
    plain(tail)
}

#[test]
fn stderr_is_cut_at_160_characters_with_an_ellipsis_and_never_inside_a_character() {
    let en = en();
    for (line, cut) in [
        ("x".repeat(160), false),
        ("x".repeat(161), true),
        ("\u{1F600}".repeat(300), true),
        ("e\u{0301}".repeat(200), true),
        ("日".repeat(160), false),
    ] {
        let text = failure_text(&en, &failure_of(FailureKind::Crashed, Some(1), &[&line]));
        let tail = stderr_part(&text);
        assert!(
            tail.chars().count() <= STDERR_CHARS,
            "{} chars",
            tail.chars().count()
        );
        assert_eq!(
            tail.ends_with('\u{2026}'),
            cut,
            "{:?}",
            tail.chars().take(10).collect::<String>()
        );
        if !cut {
            assert_eq!(tail, line);
        }
    }
}

#[test]
fn the_tail_lines_are_joined_blank_ones_dropped_and_a_huge_tail_is_still_cut() {
    let en = en();
    let text = failure_text(
        &en,
        &failure_of(
            FailureKind::ExitedEarly,
            Some(1),
            &["one", "", "   ", "two "],
        ),
    );
    assert_eq!(stderr_part(&text), "one | two");
    let many: Vec<String> = (0..5000).map(|n| format!("line {n}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let text = failure_text(&en, &failure_of(FailureKind::ExitedEarly, None, &refs));
    assert!(stderr_part(&text).chars().count() <= STDERR_CHARS);
    let none = failure_text(&en, &failure_of(FailureKind::ExitedEarly, None, &["", " "]));
    assert!(!none.contains("The engine said"), "{none}");
}

#[test]
fn stderr_with_fluent_syntax_or_control_characters_does_not_break_the_message() {
    let en = en();
    for line in [
        "{ $detail } { -x } }",
        "\u{0007}\u{001b}[31mred\u{001b}[0m",
        "tab\there",
        "nul\u{0000}byte",
    ] {
        let text = failure_text(&en, &failure_of(FailureKind::Crashed, Some(9), &[line]));
        assert!(
            text.starts_with("The engine ended by itself (exit code 9)."),
            "{text:?}"
        );
        // Control characters become blanks (D751), the rest stays.
        let part = stderr_part(&text);
        assert!(!part.chars().any(char::is_control), "{text:?}");
        for word in line
            .split(|c: char| c.is_control())
            .filter(|w| !w.trim().is_empty())
        {
            assert!(part.contains(word.trim()), "{word:?} in {text:?}");
        }
    }
}

#[test]
fn the_text_of_a_failure_stays_on_one_line_even_if_stderr_has_line_breaks() {
    // The status bar and the dialog lines are single-line text. A tail line
    // with `\r` (a progress bar) or an embedded `\n` must not split them.
    let en = en();
    let text = failure_text(
        &en,
        &failure_of(FailureKind::Crashed, None, &["loading\r50%\r100%", "a\nb"]),
    );
    assert!(!text.contains('\n') && !text.contains('\r'), "{text:?}");
}

#[test]
fn the_exit_code_is_shown_with_its_sign() {
    let en = en();
    assert!(
        failure_text(&en, &failure_of(FailureKind::Crashed, Some(-1), &[]))
            .contains("exit code -1")
    );
    assert!(
        failure_text(&en, &failure_of(FailureKind::Crashed, Some(0), &[])).contains("exit code 0")
    );
    assert!(!failure_text(&en, &failure_of(FailureKind::Crashed, None, &[])).contains("exit code"));
}

#[test]
fn a_found_engine_is_named_by_its_file_even_when_the_path_is_odd() {
    let en = en();
    for path in [
        PathBuf::from("/opt/my engines/stock fish"),
        PathBuf::from("rel").join("{ $x }"),
        PathBuf::from("日本語"),
        PathBuf::from("/"),
        PathBuf::new(),
    ] {
        let n = EngineNotice::FoundRejected {
            path: path.clone(),
            failure: failure_of(FailureKind::NotAnEngine, None, &[]),
        };
        let text = plain(&engine_notice_text(&en, &n));
        assert!(!text.trim().is_empty());
        if let Some(name) = path.file_name() {
            assert!(
                text.starts_with(&format!("{}: ", name.to_string_lossy())),
                "{text}"
            );
        }
    }
}
