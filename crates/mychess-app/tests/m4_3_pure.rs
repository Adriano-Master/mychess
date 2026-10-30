//! M4-3 contract tests of the pure parts: `split_args`/`join_args` (D428,
//! D691), the Scid `engines.dat` reader (D432, D692), the installed-engine
//! search (D427, D727, rule 11), the keys (D724) and the stable message keys.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_3_common;

use std::path::PathBuf;

use m4_3_common::*;
use mychess_app::engines::{
    ArgsError, EngineFailure, EngineList, EngineNotice, FailureKind, Hotkey, ImportWarning,
    KNOWN_ENGINES, MAX_BRACE_DEPTH, MAX_SCID_ENGINES, ScidWarning, find_installed_engines,
    join_args, parse_scid_engines_dat, split_args,
};
use mychess_app::{Action, Binding, Key, KeyInput, Modifiers, NamedKey, Shortcuts};
use mychess_engine::{Phase, Protocol};
use proptest::prelude::*;

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ---- args ----

#[test]
fn split_args_table() {
    let cases: &[(&str, Result<Vec<String>, ArgsError>)] = &[
        ("", Ok(v(&[]))),
        ("   \t\n ", Ok(v(&[]))),
        ("-t 4", Ok(v(&["-t", "4"]))),
        ("  a   b  ", Ok(v(&["a", "b"]))),
        (r#""my file" x"#, Ok(v(&["my file", "x"]))),
        ("'my file' x", Ok(v(&["my file", "x"]))),
        (r#"a"b c"d"#, Ok(v(&["ab cd"]))),
        (r#""""#, Ok(v(&[""]))),
        ("'' x", Ok(v(&["", "x"]))),
        (r"a\ b", Ok(v(&["a b"]))),
        (r#"say \"hi\""#, Ok(v(&["say", "\"hi\""]))),
        (r"C:\engines\sf.exe", Ok(v(&[r"C:\engines\sf.exe"]))),
        (
            r#""C:\Program Files\sf.exe""#,
            Ok(v(&[r"C:\Program Files\sf.exe"])),
        ),
        (r"\\server\share", Ok(v(&[r"\server\share"]))),
        (r"trailing\", Ok(v(&[r"trailing\"]))),
        ("'a\\b'", Ok(v(&["a\\b"]))),
        ("ünï çödé", Ok(v(&["ünï", "çödé"]))),
        ("日本 語", Ok(v(&["日本", "語"]))),
        ("\"open", Err(ArgsError::UnclosedQuote)),
        ("'open", Err(ArgsError::UnclosedQuote)),
        ("a \"b 'c\"", Ok(v(&["a", "b 'c"]))),
        ("\"a\\\"", Err(ArgsError::UnclosedQuote)),
        (
            "--hash=$HOME `id` $(id) *",
            Ok(v(&["--hash=$HOME", "`id`", "$(id)", "*"])),
        ),
    ];
    for (input, want) in cases {
        assert_eq!(&split_args(input), want, "input {input:?}");
    }
}

#[test]
fn join_args_quotes_only_when_needed() {
    assert_eq!(join_args(&[]), "");
    assert_eq!(join_args(&v(&["-t", "4"])), "-t 4");
    assert_eq!(join_args(&v(&["a b"])), r#""a b""#);
    assert_eq!(join_args(&v(&[""])), r#""""#);
    assert_eq!(join_args(&v(&[r"C:\engines\sf"])), r"C:\engines\sf");
}

#[test]
fn split_args_survives_hostile_sizes() {
    let long = "a ".repeat(200_000);
    assert_eq!(split_args(&long).unwrap().len(), 200_000);
    let quotes = "\"".repeat(100_001);
    assert_eq!(split_args(&quotes), Err(ArgsError::UnclosedQuote));
    let slashes = "\\".repeat(100_000);
    assert!(split_args(&slashes).is_ok());
}

proptest! {
    #[test]
    fn join_then_split_is_exact(args in proptest::collection::vec("[ a\\\\\"'\\t\\n\u{e9}\u{a0}\u{2003}日-]{0,8}", 0..6)) {
        let line = join_args(&args);
        prop_assert_eq!(split_args(&line).unwrap(), args);
    }

    #[test]
    fn split_join_split_is_stable(text in "[ a\\\\\"'\\t\u{e9}-]{0,24}") {
        if let Ok(first) = split_args(&text) {
            let second = split_args(&join_args(&first)).unwrap();
            prop_assert_eq!(first, second);
        }
    }

    #[test]
    fn split_args_never_panics(text in any::<String>()) {
        drop(split_args(&text));
    }
}

// ---- Scid engines.dat ----

const DAT: &str = r#"# Analysis Engines configuration for a Scid-like program

engine {
  Name {Stockfish 16}
  Cmd  /opt/sf/stockfish
  Args {--threads 4 "my file" 'it s'}
  Dir  /opt/sf
  Elo  3600
  Time 1700000000
  URL  https://stockfishchess.org
  UCI  1
  UCIoptions {{Hash 256} {{Skill Level} 20} {BookFile {/a b/book.bin}} {Empty {}}}
}

engine {
  Name Crafty
  Cmd  ./crafty
  Args {}
  Dir  .
  Elo  0
  Time 0
  URL  {}
  UCI  0
  UCIoptions {}
}

engine {
  Name NoDir
  Cmd  nodir
}

engine {
  Name {}
  Cmd  c
  Dir  d
}

set x [exec echo hi]
exec touch /nonexistent/never
"#;

#[test]
fn scid_dat_maps_every_key_and_pairs_of_uci_options() {
    let import = parse_scid_engines_dat(DAT);
    assert_eq!(import.engines.len(), 2, "{import:?}");
    assert_eq!(import.skipped, 2, "NoDir and the empty-name block");
    assert!(!import.incomplete);
    let sf = &import.engines[0];
    assert_eq!(sf.name, "Stockfish 16");
    assert_eq!(sf.command, "/opt/sf/stockfish");
    assert_eq!(sf.args_text, "--threads 4 \"my file\" 'it s'");
    assert_eq!(sf.dir, "/opt/sf");
    assert_eq!(sf.elo, Some(3600));
    assert_eq!(sf.url, "https://stockfishchess.org");
    assert!(sf.uci);
    assert_eq!(
        sf.options,
        [
            ("Hash".to_owned(), "256".to_owned()),
            ("Skill Level".to_owned(), "20".to_owned()),
            ("BookFile".to_owned(), "/a b/book.bin".to_owned()),
            ("Empty".to_owned(), String::new()),
        ]
    );
    let crafty = &import.engines[1];
    assert_eq!(
        (crafty.elo, crafty.uci),
        (None, false),
        "UCI 0 means XBoard"
    );
}

#[test]
fn importing_into_a_list_converts_protocol_args_and_warns_about_relative_paths() {
    let mut list = EngineList::new();
    let report = list.import_scid(&parse_scid_engines_dat(DAT));
    assert_eq!(report.added.len(), 2);
    assert_eq!(report.duplicates, 0);
    let sf = list.get(report.added[0]).unwrap();
    assert_eq!(sf.protocol, Protocol::Uci);
    assert_eq!(sf.args, ["--threads", "4", "my file", "it s"]);
    assert_eq!(sf.command, PathBuf::from("/opt/sf/stockfish"));
    assert_eq!(sf.working_dir, Some(PathBuf::from("/opt/sf")));
    assert_eq!(sf.elo, Some(3600));
    assert_eq!(sf.options.len(), 4);
    let crafty = list.get(report.added[1]).unwrap();
    assert_eq!(crafty.protocol, Protocol::XBoard);
    assert!(
        report
            .warnings
            .iter()
            .any(|w: &ImportWarning| w.engine == "Crafty" && w.kind == ScidWarning::RelativePath),
        "{:?}",
        report.warnings
    );
    assert!(!report.warnings.iter().any(|w| w.engine == "Stockfish 16"));
    // The import never assigns keys.
    for k in Hotkey::ALL {
        assert_eq!(list.hotkey_owner(k), None);
    }
    // Importing again skips both as duplicates (same name and command).
    let again = list.import_scid(&parse_scid_engines_dat(DAT));
    assert_eq!((again.added.len(), again.duplicates), (0, 2));
    assert_eq!(list.len(), 2);
}

#[test]
fn a_file_without_engine_blocks_reports_nothing_found() {
    for text in [
        "",
        "# only a comment\n",
        "set a 1\nputs hi\n",
        "engines {Name x Cmd y Dir z}",
    ] {
        let i = parse_scid_engines_dat(text);
        assert!(
            i.engines.is_empty() && i.skipped == 0 && !i.incomplete,
            "{text:?}: {i:?}"
        );
    }
}

#[test]
fn tcl_code_in_the_file_is_treated_as_text_and_never_evaluated() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("pwned");
    let marker_text = marker.to_string_lossy().replace('\\', "/");
    let dat = format!(
        "exec touch {marker_text}\nengine {{\n  Name {{[exec touch {marker_text}]}}\n  Cmd  {{$env(HOME)/bin/x}}\n  Dir  {{[file join a b]}}\n  Args {{[exec touch {marker_text}] $x}}\n}}\nengine {{ Name \"a [exec touch {marker_text}] b\" Cmd c Dir d }}\nsource {marker_text}\n",
    );
    let import = parse_scid_engines_dat(&dat);
    assert!(!marker.exists(), "the file must never be evaluated");
    assert_eq!(import.engines.len(), 2);
    assert!(import.engines[0].name.starts_with("[exec touch"));
    assert_eq!(import.engines[0].command, "$env(HOME)/bin/x");
    assert_eq!(import.engines[0].dir, "[file join a b]");
    assert!(import.engines[1].name.contains("[exec touch"));
}

#[test]
fn uci_flag_accepts_the_usual_truthy_words_and_rejects_the_rest() {
    for (word, want) in [
        ("1", true),
        ("true", true),
        ("yes", true),
        ("on", true),
        ("0", false),
        ("no", false),
        ("maybe", false),
        ("", false),
    ] {
        let dat = format!("engine {{ Name n Cmd /c Dir /d UCI {{{word}}} }}");
        let i = parse_scid_engines_dat(&dat);
        assert_eq!(i.engines[0].uci, want, "UCI {word:?}");
    }
}

#[test]
fn elo_outside_the_range_or_not_a_number_is_unknown() {
    for (word, want) in [
        ("2500", Some(2500)),
        ("4000", Some(4000)),
        ("4001", None),
        ("0", None),
        ("-3", None),
        ("abc", None),
        ("99999999999999", None),
    ] {
        let dat = format!("engine {{ Name n Cmd /c Dir /d Elo {word} }}");
        assert_eq!(
            parse_scid_engines_dat(&dat).engines[0].elo,
            want,
            "Elo {word}"
        );
    }
}

#[test]
fn unknown_keys_are_ignored_and_an_odd_number_of_words_skips_the_block() {
    let i = parse_scid_engines_dat("engine { Name n Cmd /c Dir /d Colour red Extra {a b c} }");
    assert_eq!(i.engines.len(), 1);
    let i = parse_scid_engines_dat("engine { Name n Cmd /c Dir /d Lonely }");
    assert!(i.engines.is_empty());
    assert_eq!(i.skipped, 1);
}

#[test]
fn unbalanced_braces_or_quotes_stop_the_reader_and_mark_it_incomplete() {
    for text in [
        "engine { Name n Cmd /c Dir /d",
        "engine { Name {unclosed Cmd /c Dir /d }",
    ] {
        let i = parse_scid_engines_dat(text);
        assert!(i.incomplete, "{text:?}");
        assert!(i.engines.is_empty(), "{text:?}");
    }
    // A bad quote inside a closed block only skips that block (Tcl would
    // raise a list error for it) and the next block is still read.
    let i = parse_scid_engines_dat(
        "engine { Name \"unclosed Cmd /c Dir /d }\nengine { Name b Cmd c Dir d }",
    );
    assert_eq!((i.engines.len(), i.skipped), (1, 1));
    // Blocks before the damage are kept.
    let i = parse_scid_engines_dat("engine { Name a Cmd /a Dir /d }\nengine { Name b");
    assert_eq!(i.engines.len(), 1);
    assert!(i.incomplete);
}

#[test]
fn brace_nesting_deeper_than_the_limit_does_not_overflow_the_stack() {
    assert_eq!(MAX_BRACE_DEPTH, 32);
    let deep = |n: usize| {
        format!(
            "engine {{ Name n Cmd /c Dir /d UCIoptions {}{} }}",
            "{".repeat(n),
            "}".repeat(n)
        )
    };
    // Within the limit the block is read.
    let ok = parse_scid_engines_dat(&deep(10));
    assert_eq!(ok.engines.len(), 1);
    // Far beyond it, in a thread with a small stack: stops, no panic.
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let text = format!(
                "engine {{ Name n Cmd /c Dir /d UCIoptions {}",
                "{".repeat(200_000)
            );
            let a = parse_scid_engines_dat(&text);
            let text = "{".repeat(500_000);
            let b = parse_scid_engines_dat(&text);
            let text = format!("engine {}", "{".repeat(100_000) + &"}".repeat(100_000));
            let c = parse_scid_engines_dat(&text);
            (a.incomplete, b.incomplete, c.engines.len())
        })
        .unwrap();
    let (a, _b, c) = handle.join().expect("no stack overflow or panic");
    assert!(a);
    assert_eq!(c, 0);
}

#[test]
fn more_than_1024_blocks_are_cut_and_reported_incomplete() {
    assert_eq!(MAX_SCID_ENGINES, 1024);
    let mut dat = String::new();
    for i in 0..1500 {
        dat.push_str(&format!("engine {{ Name e{i} Cmd /c{i} Dir /d }}\n"));
    }
    let i = parse_scid_engines_dat(&dat);
    assert_eq!(i.engines.len(), 1024);
    assert!(i.incomplete);
    // The list itself still limits itself to 256.
    let mut list = EngineList::new();
    let report = list.import_scid(&i);
    assert_eq!(list.len(), 256);
    assert_eq!(report.added.len(), 256);
    assert!(report.over_limit > 0);
}

#[test]
fn a_very_large_input_is_bounded() {
    let big = format!("# {}\n", "x".repeat(10 << 20));
    let started = std::time::Instant::now();
    let i = parse_scid_engines_dat(&big);
    assert!(i.engines.is_empty());
    assert!(started.elapsed().as_secs() < 20);
}

#[test]
fn hostile_bytes_never_panic_the_reader() {
    for text in [
        "\\",
        "engine {\\",
        "engine { Name \\\n n Cmd \\x41 Dir \\u00e9 }",
        "engine {Name n;Cmd c;Dir d}",
        "\u{0}\u{0}engine\u{0}{",
        "engine { Name {a}b Cmd c Dir d }",
        "engine {{}}",
        "engine",
        "engine {} {} {}",
        "#\\\nengine { Name n Cmd c Dir d }",
    ] {
        drop(parse_scid_engines_dat(text));
    }
}

proptest! {
    #[test]
    fn scid_reader_never_panics(text in "[ engine{}\"\\\\#\\[\\]\\$;\\nNameCmdDir]{0,200}") {
        let i = parse_scid_engines_dat(&text);
        prop_assert!(i.engines.len() <= MAX_SCID_ENGINES);
    }
}

// ---- finding installed engines ----

#[cfg(unix)]
mod find_unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn file(dir: &std::path::Path, name: &str, mode: u32, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn lists_known_executables_only_and_never_runs_them() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let script = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
        let sf = file(dir.path(), "stockfish", 0o755, &script);
        let sf16 = file(dir.path(), "Stockfish_16", 0o755, &script);
        let lc0 = file(dir.path(), "lc0-v0.31", 0o755, &script);
        file(dir.path(), "stockfish-not-exec", 0o644, &script);
        file(dir.path(), "stockfishx", 0o755, &script);
        file(dir.path(), "unknown-engine", 0o755, &script);
        file(dir.path(), "xstockfish", 0o755, &script);
        std::fs::create_dir(dir.path().join("crafty")).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        file(&dir.path().join("sub"), "komodo", 0o755, &script);
        let found = find_installed_engines(&[
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            dir.path().join("does-not-exist"),
        ]);
        let paths: Vec<_> = found.iter().map(|f| f.path.clone()).collect();
        assert!(!marker.exists(), "the search must never execute anything");
        assert_eq!(paths.len(), 3, "{found:?}");
        for want in [&sf, &sf16, &lc0] {
            assert!(paths.contains(want), "{want:?} in {paths:?}");
        }
        // Order follows KNOWN_ENGINES (stockfish before lc0 in the list? no:
        // the list order is stockfish, lc0, ...).
        let pos = |name: &str| KNOWN_ENGINES.iter().position(|k| *k == name).unwrap();
        assert!(pos("stockfish") < pos("lc0"));
        assert_eq!(found.last().unwrap().path, lc0);
    }

    #[test]
    fn an_empty_or_unreadable_search_path_gives_nothing() {
        assert!(find_installed_engines(&[]).is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert!(find_installed_engines(&[dir.path().to_path_buf()]).is_empty());
    }
}

#[test]
fn known_engine_names_match_the_spec_list() {
    for name in [
        "stockfish",
        "lc0",
        "komodo",
        "dragon",
        "berserk",
        "ethereal",
        "rubichess",
        "igel",
        "koivisto",
        "fruit",
        "toga",
        "crafty",
        "gnuchess",
        "phalanx",
    ] {
        assert!(KNOWN_ENGINES.contains(&name), "{name}");
    }
}

// ---- keys (D724) ----

fn press(key: Key, modifiers: Modifiers) -> KeyInput {
    KeyInput { key, modifiers }
}

#[test]
fn f2_f3_f4_start_engines_even_while_a_text_field_has_focus() {
    let shortcuts = Shortcuts::defaults();
    for (n, f) in [(1u8, 2u8), (2, 3), (3, 4)] {
        let input = press(Key::Named(NamedKey::F(f)), Modifiers::NONE);
        assert_eq!(shortcuts.lookup(&input), Some(Action::StartEngine(n)));
        assert_eq!(
            shortcuts.lookup_in(&input, true),
            Some(Action::StartEngine(n))
        );
    }
    for f in [1u8, 5, 6] {
        let input = press(Key::Named(NamedKey::F(f)), Modifiers::NONE);
        assert!(!matches!(
            shortcuts.lookup(&input),
            Some(Action::StartEngine(_))
        ));
    }
}

#[test]
fn space_toggles_the_analysis_but_not_while_typing() {
    let shortcuts = Shortcuts::defaults();
    let space = press(Key::Named(NamedKey::Space), Modifiers::NONE);
    assert_eq!(
        shortcuts.lookup_in(&space, false),
        Some(Action::ToggleEngineAnalysis)
    );
    assert_eq!(shortcuts.lookup_in(&space, true), None);
    // With a modifier it is not the analysis toggle.
    let with_shift = press(
        Key::Named(NamedKey::Space),
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
    );
    assert_ne!(
        shortcuts.lookup(&with_shift),
        Some(Action::ToggleEngineAnalysis)
    );
}

#[test]
fn command_shift_e_opens_the_dialog_and_command_shift_a_stays_select_all_games() {
    let shortcuts = Shortcuts::defaults();
    let e = press(Key::Char('e'), Modifiers::COMMAND_SHIFT);
    assert_eq!(shortcuts.lookup(&e), Some(Action::ManageEngines));
    assert_eq!(shortcuts.lookup_in(&e, true), Some(Action::ManageEngines));
    // Shift reported with the upper-case letter, as some platforms do.
    assert_eq!(
        shortcuts.lookup(&press(Key::Char('E'), Modifiers::COMMAND_SHIFT)),
        Some(Action::ManageEngines)
    );
    assert_eq!(
        shortcuts.lookup(&press(Key::Char('a'), Modifiers::COMMAND_SHIFT)),
        Some(Action::ListSelectAll)
    );
    assert_ne!(
        shortcuts.lookup(&press(Key::Char('e'), Modifiers::COMMAND)),
        Some(Action::ManageEngines)
    );
}

#[test]
fn engine_actions_are_not_configurable_but_have_stable_names_and_keys() {
    let all: Vec<Action> = Action::all().collect();
    for a in [
        Action::StartEngine(1),
        Action::StartEngine(2),
        Action::StartEngine(3),
        Action::ManageEngines,
        Action::ToggleEngineAnalysis,
    ] {
        assert!(!all.contains(&a), "{a:?} must stay out of Action::all()");
    }
    assert_eq!(Action::StartEngine(2).name(), "StartEngine");
    assert_eq!(Action::StartEngine(2).message_key(), "action-start-engine");
    assert_eq!(Action::ManageEngines.message_key(), "action-manage-engines");
    assert_eq!(
        Action::ToggleEngineAnalysis.message_key(),
        "action-toggle-engine-analysis"
    );
    assert_eq!(
        Action::from_name("ManageEngines"),
        Some(Action::ManageEngines)
    );
    assert_eq!(
        Action::from_name("ToggleEngineAnalysis"),
        Some(Action::ToggleEngineAnalysis)
    );
}

#[test]
fn no_default_binding_is_shared_by_two_actions() {
    let shortcuts = Shortcuts::defaults();
    let mut seen: std::collections::HashMap<(Key, Modifiers), Action> =
        std::collections::HashMap::new();
    for (action, bindings) in shortcuts.iter() {
        for b in bindings {
            let b: &Binding = b;
            if let Some(prev) = seen.insert((b.key, b.modifiers), action) {
                assert_eq!(prev, action, "{:?}+{:?} bound twice", b.modifiers, b.key);
            }
        }
    }
}

#[test]
fn the_engine_dialog_action_opens_the_dialog() {
    use mychess_app::{Message, update};
    let mut state = mychess_app::AppState::default();
    update(&mut state, Message::Action(Action::ManageEngines));
    assert!(state.engine_dialog.is_some());
    // The Space action without a running engine only warns.
    update(&mut state, Message::Action(Action::ToggleEngineAnalysis));
    assert_eq!(notice(&state), Some(&EngineNotice::NoEngineRunning));
}

// ---- message keys ----

fn failure(kind: FailureKind) -> EngineFailure {
    EngineFailure {
        kind,
        exit_code: Some(1),
        stderr_tail: vec![],
    }
}

#[test]
fn failure_and_notice_keys_are_stable_prefixed_and_unique_per_meaning() {
    let kinds = [
        (FailureKind::CannotStart, "engines-failure-cannot-start"),
        (FailureKind::ExitedEarly, "engines-failure-exited-early"),
        (FailureKind::Crashed, "engines-failure-crashed"),
        (
            FailureKind::Timeout(Phase::Uciok),
            "engines-failure-timeout",
        ),
        (FailureKind::Timeout(Phase::Pong), "engines-failure-timeout"),
        (FailureKind::StopTimeout, "engines-failure-stop-timeout"),
        (FailureKind::NotAnEngine, "engines-failure-not-an-engine"),
        (FailureKind::Io, "engines-failure-io"),
        (FailureKind::Closed, "engines-failure-closed"),
        (FailureKind::Other, "engines-failure-other"),
    ];
    let mut unique = std::collections::HashSet::new();
    for (kind, key) in kinds {
        assert_eq!(failure(kind).message_key(), key);
        unique.insert(key);
    }
    assert_eq!(unique.len(), 9, "one key per failure kind");

    let f = failure(FailureKind::Crashed);
    let notices = [
        EngineNotice::FileUnreadable,
        EngineNotice::NewerVersion,
        EngineNotice::FileFixed { count: 1 },
        EngineNotice::SaveFailed,
        EngineNotice::NoEngineOnKey(Hotkey::F2),
        EngineNotice::NoEngineRunning,
        EngineNotice::ListFull,
        EngineNotice::NotFound,
        EngineNotice::RestartToApply,
        EngineNotice::ManyRunning { count: 4 },
        EngineNotice::CommandRefused,
        EngineNotice::Imported {
            added: 1,
            duplicates: 0,
            invalid: 0,
        },
        EngineNotice::NothingToImport,
    ];
    let keys: Vec<&str> = notices.iter().map(EngineNotice::message_key).collect();
    let set: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(set.len(), keys.len(), "{keys:?}");
    assert!(keys.iter().all(|k| k.starts_with("engines-notice-")));
    // Notices that wrap a failure reuse the key of the failure.
    for n in [
        EngineNotice::StartFailed {
            name: "x".into(),
            failure: f.clone(),
        },
        EngineNotice::Crashed {
            name: "x".into(),
            failure: f.clone(),
        },
        EngineNotice::FoundRejected {
            path: "p".into(),
            failure: f.clone(),
        },
    ] {
        assert_eq!(n.message_key(), f.message_key());
    }
}
