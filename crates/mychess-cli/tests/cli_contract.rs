//! Contract tests of `mychess-cli` written from `docs/specs/m3-5-mychess-cli.md`
//! (rules 1-16, decisions D621-D627), beyond the smoke flow of `cli.rs`:
//! usage errors, missing and damaged files, litter after failures, round
//! trips, encodings, cancellation through the library entry point, output
//! hygiene and (ignored) compatibility with the databases bundled with Scid.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mychess_cli::{Io, run as lib_run};
use mychess_db::CancelToken;
use mychess_db::si4::{FLAG_LETTERS, FlagOp, GameFlags, Si4Database, Si4Writer};
use mychess_pgn::write::{WriteOptions, game_to_string};
use pretty_assertions::assert_eq;

// ------------------------------------------------------------------ helpers

const FOUR: &str = "\
[Event \"Club night\"]
[Site \"Paris FRA\"]
[Date \"2021.03.05\"]
[Round \"10\"]
[White \"Delta\"]
[Black \"Echo\"]
[Result \"1-0\"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0

[Event \"Club night\"]
[Site \"Rome ITA\"]
[Date \"2020.11.20\"]
[Round \"2\"]
[White \"Alpha\"]
[Black \"Bravo\"]
[Result \"0-1\"]

1. d4 d5 2. c4 e6 0-1

[Event \"Open\"]
[Site \"Oslo\"]
[Date \"2022.01.01\"]
[Round \"1\"]
[White \"Charlie\"]
[Black \"Foxtrot\"]
[Result \"1/2-1/2\"]

1. c4 c5 2. Nc3 Nc6 1/2-1/2

[Event \"Open\"]
[Site \"Oslo\"]
[Date \"2019.07.07\"]
[Round \"3\"]
[White \"Bravo\"]
[Black \"Delta\"]
[Result \"1-0\"]

1. Nf3 Nf6 1-0
";

/// Games with comments, variations, NAGs and embedded commands.
const RICH: &str = "\
[Event \"Rich\"]
[Site \"Lima PER\"]
[Date \"2018.05.06\"]
[Round \"1\"]
[White \"Rich One\"]
[Black \"Rich Two\"]
[Result \"1-0\"]
[WhiteElo \"2100\"]
[BlackElo \"1990\"]
[ECO \"C42\"]

1. e4 {[%clk 0:10:00] opening comment} e5 $1 (1... c5 2. Nf3 (2. c3) 2... d6) 2. Nf3 Nf6 $14 3. Nxe5 d6 1-0

[Event \"Rich\"]
[Site \"Lima PER\"]
[Date \"2018.05.07\"]
[Round \"2\"]
[White \"Rich Three\"]
[Black \"Rich Four\"]
[Result \"0-1\"]

1. d4 Nf6 2. c4 e6 {a comment} 3. Nc3 Bb4 0-1

[Event \"Rich\"]
[Site \"Oslo\"]
[Date \"2018.??.??\"]
[Round \"3\"]
[White \"Rich Five\"]
[Black \"Rich Six\"]
[Result \"*\"]

1. g3 *
";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mychess-cli"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run mychess-cli")
}

fn run_os(args: &[&Path]) -> Output {
    bin().args(args).output().expect("run mychess-cli")
}

fn run_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).ok();
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("exit code (not a signal)")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// A panic ends a Rust process with 101; the contract says never.
fn assert_no_panic(o: &Output, what: &str) {
    assert_ne!(code(o), 101, "{what}: panicked: {}", err(o));
    assert!(!err(o).contains("panicked"), "{what}: {}", err(o));
}

fn write(dir: &Path, name: &str, content: impl AsRef<[u8]>) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

fn base_from(dir: &Path, name: &str, pgn: &str) -> PathBuf {
    let file = write(dir, &format!("{name}.pgn"), pgn);
    let base = dir.join(name);
    let o = run(&["-q", "import", p(&file), p(&base)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    base
}

fn files_in(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn db_bytes(base: &Path) -> Vec<Vec<u8>> {
    ["si4", "sn4", "sg4"]
        .iter()
        .map(|e| fs::read(base.with_extension(e)).unwrap_or_default())
        .collect()
}

/// `(game as PGN, editable flags)` for every game, in index order.
fn signature(base: &Path) -> Vec<(String, u32)> {
    let mut db = Si4Database::open(base).unwrap();
    (1..=db.len())
        .map(|n| {
            let flags = db.entry(n).unwrap().flags & GameFlags::EDITABLE;
            let game = db.game(n).unwrap();
            (
                game_to_string(&game, &WriteOptions::default()).unwrap(),
                flags.bits(),
            )
        })
        .collect()
}

fn exported(dir: &Path, base: &Path) -> String {
    let pgn = dir.join("export-tmp.pgn");
    fs::remove_file(&pgn).ok();
    let o = run(&["-q", "export", p(base), p(&pgn)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = fs::read_to_string(&pgn).unwrap();
    fs::remove_file(&pgn).unwrap();
    text
}

fn many_games(n: usize) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    for i in 0..n {
        let result = ["1-0", "0-1", "1/2-1/2", "*"][i % 4];
        write!(text,
            "[Event \"Ev{}\"]\n[Site \"Town{} FRA\"]\n[Date \"{}.01.01\"]\n[Round \"{}\"]\n[White \"P{i:04}\"]\n[Black \"Q{i:04}\"]\n[Result \"{result}\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 {result}\n\n",
            i % 7,
            i % 5,
            1990 + i % 30,
            i % 12 + 1,
        )
        .unwrap();
    }
    text
}

fn set_flags(base: &Path, number: u32, letters: &str) {
    // Another test thread may be between `fork` and `exec` of a child process
    // that inherited a previous (already dropped) writer's locked descriptor
    // for an instant; retry the lock a few times.
    let mut attempts = 0;
    let mut w = loop {
        match Si4Writer::open(base) {
            Ok(w) => break w,
            Err(mychess_db::si4::Si4Error::AlreadyOpen { .. }) if attempts < 100 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => panic!("{e}"),
        }
    };
    for c in letters.chars() {
        w.set_flag(number, GameFlags::from_letter(c).unwrap(), FlagOp::Set)
            .unwrap();
    }
}

/// Runs the library entry point with injected streams.
fn lib(
    args: &[OsString],
    input: &[u8],
    cancel: &CancelToken,
    err_sink: &mut dyn Write,
) -> (u8, String) {
    let mut out = Vec::new();
    let mut input = input;
    let code = lib_run(
        args,
        Io {
            out: &mut out,
            err: err_sink,
            input: &mut input,
            cancel,
        },
    );
    (code, String::from_utf8_lossy(&out).into_owned())
}

fn argv(parts: &[&str]) -> Vec<OsString> {
    std::iter::once("mychess-cli")
        .chain(parts.iter().copied())
        .map(OsString::from)
        .collect()
}

/// Cancels the token when the text written to it contains `needle`.
struct CancelOn<'a> {
    token: &'a CancelToken,
    needle: String,
    seen: Vec<u8>,
}

impl Write for CancelOn<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.seen.extend_from_slice(bytes);
        if String::from_utf8_lossy(&self.seen).contains(&self.needle) {
            self.token.cancel();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ------------------------------------------------------ usage errors (code 2)

#[test]
fn every_subcommand_with_missing_arguments_exits_two_with_empty_stdout() {
    let cases: &[&[&str]] = &[
        &["import"],
        &["import", "-f"],
        &["merge"],
        &["merge", "target"],
        &["info"],
        &["names"],
        &["names", "base"],
        &["describe"],
        &["describe", "base"],
        &["sort"],
        &["sort", "date"],
        &["compact"],
        &["export"],
        &["export", "base"],
    ];
    for args in cases {
        let o = run(args);
        assert_eq!(code(&o), 2, "{args:?}: {}", err(&o));
        assert_eq!(out(&o), "", "{args:?}");
        assert!(!err(&o).is_empty(), "{args:?}");
    }
}

#[test]
fn unknown_options_and_extra_arguments_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let b = p(&base);
    let cases: &[&[&str]] = &[
        &["--bogus", "info", b],
        &["info", "--bogus", b],
        &["import", "-z", "a.pgn"],
        &["info", b, "extra", "args"],
        &["sort", "-q2", "date", b],
        &["compact", "--nope", b],
        &["names", b, "player", "B", "extra"],
        &["export", b, "a.pgn", "extra"],
    ];
    for args in cases {
        let o = run(args);
        assert_eq!(code(&o), 2, "{args:?}: {}", err(&o));
    }
}

#[test]
fn invalid_sort_fields_exit_two_before_touching_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let before = db_bytes(&base);
    let many = vec!["date"; 17].join(",");
    for fields in [
        "w",
        "e",
        "c",
        "",
        "date;year",
        "d4te",
        "nope",
        &many,
        "date,,",
    ] {
        let o = run(&["sort", fields, p(&base)]);
        let expected = if fields == "date,," { 0 } else { 2 };
        assert_eq!(code(&o), expected, "{fields:?}: {}", err(&o));
    }
    // A missing database with an invalid field is still a usage error.
    let ghost = dir.path().join("ghost");
    assert_eq!(code(&run(&["sort", "nope", p(&ghost)])), 2);
    assert!(!dir.path().join("ghost.si4").exists());
    // `date,,` sorted the database once; everything else left it alone.
    let after = db_bytes(&base);
    assert_eq!(after.len(), before.len());
}

#[test]
fn help_and_version_exit_zero_for_the_tool_and_every_subcommand() {
    for sub in [
        "import", "merge", "info", "names", "describe", "sort", "compact", "export",
    ] {
        let o = run(&[sub, "--help"]);
        assert_eq!(code(&o), 0, "{sub}: {}", err(&o));
        assert!(out(&o).to_lowercase().contains("usage"), "{sub}");
        assert_eq!(err(&o), "", "{sub}");
    }
    let o = run(&["-V"]);
    assert_eq!(code(&o), 0);
    assert!(out(&o).contains("mychess-cli"), "{}", out(&o));
}

#[test]
fn sort_accepts_all_19_fields_prefixes_separators_and_reverse() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    for field in [
        "date",
        "year",
        "event",
        "site",
        "round",
        "white",
        "black",
        "eco",
        "result",
        "length",
        "rating",
        "welo",
        "belo",
        "country",
        "month",
        "deleted",
        "eventdate",
        "variations",
        "comments",
        "DATE",
        "ra",
        "wh",
        "date,year event.round white",
        "date.year",
    ] {
        for flags in [&["-q", "sort"][..], &["-q", "sort", "-r"][..]] {
            let mut args = flags.to_vec();
            args.push(field);
            args.push(p(&base));
            let o = run(&args);
            assert_eq!(code(&o), 0, "{field}: {}", err(&o));
        }
    }
    assert_eq!(signature(&base).len(), 4);
}

#[test]
fn names_kind_accepts_the_name_and_a_unique_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let full = run(&["names", p(&base), "player"]);
    assert_eq!(code(&full), 0, "{}", err(&full));
    for kind in ["pl", "p", "PLAYER", "Pla"] {
        let o = run(&["names", p(&base), kind]);
        assert_eq!(code(&o), 0, "{kind}: {}", err(&o));
        assert_eq!(out(&o), out(&full), "{kind}");
    }
    for kind in ["e", "s", "r", "ro", "ev", "si"] {
        let o = run(&["names", p(&base), kind]);
        assert_eq!(code(&o), 0, "{kind}: {}", err(&o));
    }
    assert_eq!(code(&run(&["names", p(&base), "x"])), 2);
}

// ------------------------------------------------- names / describe contract

#[test]
fn names_prefix_is_case_sensitive_and_empty_prefix_lists_all() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let o = run(&["names", p(&base), "player", "b"]);
    assert_eq!(code(&o), 0);
    assert_eq!(out(&o), "[0 names matched the supplied prefix string]\n");
    let o = run(&["names", p(&base), "player", "zzz"]);
    assert_eq!(out(&o), "[0 names matched the supplied prefix string]\n");
    let o = run(&["names", p(&base), "player"]);
    let text = out(&o);
    assert!(text.ends_with("[6 names matched the supplied prefix string]\n"));
    // `%6d\t%s` and one line per name, in the order of the .sn4.
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 7);
    for line in &lines[..6] {
        let (freq, name) = line.split_once('\t').unwrap();
        assert_eq!(freq.len(), 6, "{line:?}");
        assert!(freq.trim().parse::<u32>().is_ok());
        assert_ne!(name, "", "{line:?}");
    }
    let freqs: u32 = lines[..6]
        .iter()
        .map(|l| l.split_once('\t').unwrap().0.trim().parse::<u32>().unwrap())
        .sum();
    assert_eq!(freqs, 8, "each of the 4 games uses two players");
    assert_eq!(err(&o), "");
}

#[test]
fn describe_truncates_at_107_bytes_and_survives_multibyte_text() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let long = "a".repeat(200);
    let o = run(&["describe", p(&base), &long]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains(&"a".repeat(107)), "{}", out(&o));
    assert!(!out(&o).contains(&"a".repeat(108)), "{}", out(&o));
    let info = out(&run(&["info", p(&base)]));
    assert!(info.contains(&"a".repeat(107)) && !info.contains(&"a".repeat(108)));

    // 80 two-byte letters: 160 bytes, cut inside or at a character boundary.
    let accents = "\u{e9}".repeat(80);
    let o = run(&["describe", p(&base), &accents]);
    assert_no_panic(&o, "describe multibyte");
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(code(&run(&["info", p(&base)])), 0);
    assert_eq!(Si4Database::open(&base).unwrap().len(), 4);

    let o = run(&["describe", p(&base), ""]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(code(&run(&["info", p(&base)])), 0);
}

#[test]
fn describe_twice_with_the_same_text_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    assert_eq!(code(&run(&["-q", "describe", p(&base), "Same"])), 0);
    let first = db_bytes(&base);
    assert_eq!(code(&run(&["-q", "describe", p(&base), "Same"])), 0);
    assert_eq!(db_bytes(&base), first);
}

// ------------------------------------------- missing, damaged and locked data

#[test]
fn commands_on_a_missing_database_fail_with_one_and_create_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let ghost = dir.path().join("ghost");
    let g = p(&ghost);
    let pgn = dir.path().join("o.pgn");
    let cases: Vec<Vec<&str>> = vec![
        vec!["info", g],
        vec!["info", "--names", g],
        vec!["info", "--compact", g],
        vec!["names", g, "player"],
        vec!["describe", g, "text"],
        vec!["sort", "date", g],
        vec!["compact", g],
        vec!["compact", "--names", g],
        vec!["export", g, p(&pgn)],
    ];
    for args in cases {
        let o = run(&args);
        assert_eq!(code(&o), 1, "{args:?}: {}", err(&o));
        assert!(err(&o).starts_with("error: "), "{args:?}: {}", err(&o));
        assert!(err(&o).contains("ghost"), "the path is in the message");
        assert!(!err(&o).contains("stack backtrace"));
        assert_eq!(files_in(dir.path()).len(), 0, "{args:?} left files");
    }
}

#[test]
fn a_database_given_by_any_of_its_three_files_or_any_case_works() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    for ext in ["si4", "sn4", "sg4", "SI4", "Sg4"] {
        let path = base.with_extension(ext);
        let o = run(&["info", p(&path)]);
        assert_eq!(code(&o), 0, "{ext}: {}", err(&o));
        assert!(out(&o).contains("4 games"), "{ext}");
    }
}

#[test]
fn damaged_files_make_every_command_fail_cleanly_and_writers_leave_them_alone() {
    type Damage = fn(&Path);
    let damages: &[(&str, Damage)] = &[
        ("empty si4", |b| {
            fs::write(b.with_extension("si4"), b"").unwrap();
        }),
        ("garbage si4", |b| {
            let junk: Vec<u8> = (0..700u32).map(|i| (i * 37 % 251) as u8).collect();
            fs::write(b.with_extension("si4"), junk).unwrap();
        }),
        ("short header", |b| {
            let bytes = fs::read(b.with_extension("si4")).unwrap();
            fs::write(b.with_extension("si4"), &bytes[..100]).unwrap();
        }),
        ("truncated entry", |b| {
            let bytes = fs::read(b.with_extension("si4")).unwrap();
            fs::write(b.with_extension("si4"), &bytes[..bytes.len() - 7]).unwrap();
        }),
        ("empty sn4", |b| {
            fs::write(b.with_extension("sn4"), b"").unwrap();
        }),
        ("truncated sn4", |b| {
            let bytes = fs::read(b.with_extension("sn4")).unwrap();
            fs::write(b.with_extension("sn4"), &bytes[..bytes.len() / 2]).unwrap();
        }),
        ("missing sn4", |b| {
            fs::remove_file(b.with_extension("sn4")).unwrap();
        }),
    ];
    for (name, damage) in damages {
        let dir = tempfile::tempdir().unwrap();
        let base = base_from(dir.path(), "x", FOUR);
        damage(&base);
        let before = db_bytes(&base);
        let pgn = dir.path().join("o.pgn");
        let cases: Vec<Vec<&str>> = vec![
            vec!["info", p(&base)],
            vec!["info", "--names", p(&base)],
            vec!["names", p(&base), "player"],
            vec!["export", p(&base), p(&pgn)],
            vec!["sort", "date", p(&base)],
            vec!["compact", p(&base)],
            vec!["compact", "--names", p(&base)],
            vec!["describe", p(&base), "x"],
        ];
        for args in cases {
            let o = run(&args);
            assert_no_panic(&o, &format!("{name} {args:?}"));
            assert_eq!(code(&o), 1, "{name} {args:?}: {}", err(&o));
            assert!(err(&o).starts_with("error: "), "{name} {args:?}");
        }
        assert_eq!(db_bytes(&base), before, "{name}: files were modified");
        assert!(!pgn.exists(), "{name}: a partial export was left");
        assert_eq!(
            files_in(dir.path()),
            ["x.pgn", "x.si4", "x.sn4", "x.sg4"]
                .iter()
                .filter(|f| dir.path().join(f).exists())
                .map(|f| (*f).to_owned())
                .collect::<BTreeSet<_>>(),
            "{name}: litter (journal or temporaries)"
        );
    }
}

#[test]
fn a_truncated_game_file_never_panics_any_command() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(30));
    let sg4 = base.with_extension("sg4");
    let bytes = fs::read(&sg4).unwrap();
    fs::write(&sg4, &bytes[..bytes.len() / 3]).unwrap();
    let pgn = dir.path().join("o.pgn");
    for args in [
        vec!["info", p(&base)],
        vec!["info", "--compact", p(&base)],
        vec!["export", p(&base), p(&pgn)],
        vec!["compact", p(&base)],
    ] {
        let o = run(&args);
        assert_no_panic(&o, &format!("{args:?}"));
        assert!(matches!(code(&o), 0 | 1), "{args:?}: {}", err(&o));
    }
    let target = dir.path().join("merged");
    let o = run(&["merge", p(&target), p(&base)]);
    assert_no_panic(&o, "merge of truncated games");
}

#[cfg(unix)]
#[test]
fn a_read_only_index_fails_the_writers_and_keeps_the_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let si4 = base.with_extension("si4");
    fs::set_permissions(&si4, fs::Permissions::from_mode(0o444)).unwrap();
    if fs::OpenOptions::new().write(true).open(&si4).is_ok() {
        return; // root ignores the permission bits: nothing to test
    }
    let before = db_bytes(&base);
    for args in [
        vec!["sort", "date", p(&base)],
        vec!["compact", p(&base)],
        vec!["describe", p(&base), "x"],
    ] {
        let o = run(&args);
        assert_no_panic(&o, &format!("{args:?}"));
        assert_eq!(code(&o), 1, "{args:?}: {}", err(&o));
    }
    assert_eq!(db_bytes(&base), before);
    // Readers still work.
    assert_eq!(code(&run(&["info", p(&base)])), 0);
    fs::set_permissions(&si4, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn a_locked_database_is_not_changed_by_a_blocked_writer_command() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let holder = Si4Writer::open(&base).unwrap();
    let before = db_bytes(&base);
    for args in [
        vec!["sort", "-r", "date", p(&base)],
        vec!["compact", "--names", p(&base)],
        vec!["describe", p(&base), "blocked"],
    ] {
        let o = run(&args);
        assert_eq!(code(&o), 1, "{args:?}");
        assert_eq!(out(&o), "", "{args:?}: nothing on stdout on failure");
    }
    assert_eq!(db_bytes(&base), before);
    // Readers and the export work while the writer is open; merge reads it too.
    let pgn = dir.path().join("o.pgn");
    assert_eq!(code(&run(&["-q", "export", p(&base), p(&pgn)])), 0);
    let target = dir.path().join("m");
    assert_eq!(code(&run(&["-q", "merge", p(&target), p(&base)])), 0);
    drop(holder);
}

#[test]
fn merge_into_a_locked_target_fails_with_one() {
    let dir = tempfile::tempdir().unwrap();
    let one = base_from(dir.path(), "one", FOUR);
    let target = dir.path().join("t");
    let holder = Si4Writer::create(&target, 0).unwrap();
    let o = run(&["merge", p(&target), p(&one)]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    drop(holder);
    assert!(
        target.with_extension("si4").exists(),
        "the target was not removed"
    );
}

// ----------------------------------------------------------------- import

#[test]
fn import_with_a_wrong_number_of_games_in_a_garbage_file_reports_zero_games() {
    let dir = tempfile::tempdir().unwrap();
    let junk = write(
        dir.path(),
        "junk.pgn",
        "this is not chess\n\x00\x01\x02 ]]] {{{ \n",
    );
    let o = run(&["import", p(&junk)]);
    assert_no_panic(&o, "garbage pgn");
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("0 games"), "{}", out(&o));
    let base = dir.path().join("junk");
    assert_eq!(Si4Database::open(&base).unwrap().len(), 0);
}

#[test]
fn import_of_an_empty_file_creates_an_empty_valid_database_usable_by_every_command() {
    let dir = tempfile::tempdir().unwrap();
    let empty = write(dir.path(), "none.pgn", "");
    let o = run(&["import", p(&empty)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("0 games"), "{}", out(&o));
    assert!(out(&o).contains("There were no warnings or errors."));
    assert!(!dir.path().join("none.err").exists());
    let base = dir.path().join("none");
    for args in [
        vec!["info", p(&base)],
        vec!["info", "--names", "--compact", p(&base)],
        vec!["names", p(&base), "player"],
        vec!["-q", "sort", "date", p(&base)],
        vec!["-q", "compact", p(&base)],
        vec!["-q", "compact", "--names", p(&base)],
        vec!["describe", p(&base), "empty"],
    ] {
        let o = run(&args);
        assert_eq!(code(&o), 0, "{args:?}: {}", err(&o));
    }
    let pgn = dir.path().join("o.pgn");
    let o = run(&["export", p(&base), p(&pgn)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Exported 0 games"), "{}", out(&o));
    assert_eq!(fs::read(&pgn).unwrap_or_default().len(), 0);
}

#[test]
fn import_handles_crlf_cr_bom_and_latin1_like_plain_lf_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let lf = base_from(dir.path(), "lf", FOUR);
    let want = signature(&lf);

    let crlf = FOUR.replace('\n', "\r\n");
    let cr = FOUR.replace('\n', "\r");
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(FOUR.as_bytes());
    for (name, bytes) in [
        ("crlf", crlf.into_bytes()),
        ("cr", cr.into_bytes()),
        ("bom", bom),
    ] {
        let file = write(dir.path(), &format!("{name}.pgn"), bytes);
        let o = run(&["-q", "import", p(&file)]);
        assert_eq!(code(&o), 0, "{name}: {}", err(&o));
        assert_eq!(signature(&dir.path().join(name)), want, "{name}");
    }

    // Latin-1 and Windows-1252 bytes in names become proper UTF-8.
    let latin1: Vec<u8> = b"[Event \"Torneio\"]\n[Site \"S\xE3o Paulo BRA\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n[White \"Jos\xE9 Andr\xE9\"]\n[Black \"Fran\xE7ois M\xFCller\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n".to_vec();
    let file = write(dir.path(), "lat.pgn", latin1);
    let o = run(&["-q", "import", p(&file)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let mut db = Si4Database::open(&dir.path().join("lat")).unwrap();
    let game = db.game(1).unwrap();
    assert_eq!(game.tags().white, "Jos\u{e9} Andr\u{e9}");
    assert_eq!(game.tags().black, "Fran\u{e7}ois M\u{fc}ller");
    assert_eq!(game.tags().site, "S\u{e3}o Paulo BRA");
}

#[test]
fn import_stores_a_game_without_moves_and_skips_garbage_between_games() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[White \"Empty\"]\n[Black \"Game\"]\n[Result \"*\"]\n\n*\n\n\
         @@@ garbage !!! between games\n%escaped line\n\n{FOUR}\n\n;; stray comment\n"
    );
    let file = write(dir.path(), "g.pgn", text);
    let o = run(&["import", p(&file)]);
    assert_no_panic(&o, "garbage between games");
    assert_eq!(code(&o), 0, "{}", err(&o));
    let mut db = Si4Database::open(&dir.path().join("g")).unwrap();
    assert_eq!(db.len(), 5, "{}", out(&o));
    assert_eq!(db.game(1).unwrap().tags().white, "Empty");
    assert_eq!(db.entry(1).unwrap().num_half_moves, 0);
    assert_eq!(db.game(5).unwrap().tags().white, "Bravo");
}

#[test]
fn import_err_file_exists_only_with_warnings_and_force_replaces_everything_old() {
    let dir = tempfile::tempdir().unwrap();
    let bad = format!(
        "{FOUR}\n[Event \"Broken\"]\n[White \"Zulu\"]\n[Black \"Yankee\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Ke4 Qxx9 1-0\n"
    );
    let bad_file = write(dir.path(), "g.pgn", bad);
    let base = dir.path().join("g");
    let o = run(&["-q", "import", p(&bad_file), p(&base)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let err_file = dir.path().join("g.err");
    assert!(err_file.is_file());
    assert!(out(&o).contains("errors or warnings"));

    // A cache and stale companion files of an old database must go with --force.
    fs::write(dir.path().join("g.stc"), b"stale cache").unwrap();
    let clean = write(dir.path(), "clean.pgn", FOUR);
    let o = run(&["-q", "import", "--force", p(&clean), p(&base)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(!err_file.exists(), "an old .err must be removed when clean");
    assert!(!dir.path().join("g.stc").exists(), "the .stc must go");
    assert_eq!(Si4Database::open(&base).unwrap().len(), 4);
    assert!(out(&o).contains("There were no warnings or errors."));

    // `-f` is the short form; forcing over nothing is fine too.
    let fresh = dir.path().join("fresh");
    let o = run(&["-q", "import", "-f", p(&clean), p(&fresh)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
}

#[test]
fn import_refusal_changes_nothing_and_force_is_needed_for_each_extension_case() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let pgn = dir.path().join("x.pgn");
    let before = db_bytes(&base);
    let listing = files_in(dir.path());
    for target in [
        base.clone(),
        base.with_extension("si4"),
        base.with_extension("SG4"),
    ] {
        let o = run_os(&[Path::new("-q"), Path::new("import"), &pgn, &target]);
        assert_eq!(code(&o), 1, "{}: {}", target.display(), err(&o));
        assert!(err(&o).contains("database already exists"), "{}", err(&o));
        assert!(err(&o).contains("--force"), "the hint: {}", err(&o));
    }
    assert_eq!(db_bytes(&base), before);
    assert_eq!(files_in(dir.path()), listing);
}

#[test]
fn import_into_a_missing_directory_fails_without_litter() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "a.pgn", FOUR);
    let target = dir.path().join("nodir").join("db");
    let o = run(&["import", p(&pgn), p(&target)]);
    assert_no_panic(&o, "import to missing dir");
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(!dir.path().join("nodir").exists());
    assert_eq!(files_in(dir.path()), BTreeSet::from(["a.pgn".to_owned()]));
}

#[cfg(unix)]
#[test]
fn import_of_a_directory_or_an_unreadable_file_fails_without_a_panic() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("folder.pgn");
    fs::create_dir(&sub).unwrap();
    let o = run(&["-q", "import", p(&sub), p(&dir.path().join("fromdir"))]);
    assert_no_panic(&o, "directory as pgn");
    assert_eq!(code(&o), 1, "{}", err(&o));

    let secret = write(dir.path(), "secret.pgn", FOUR);
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&secret).is_err() {
        let o = run(&["-q", "import", p(&secret), p(&dir.path().join("sec"))]);
        assert_no_panic(&o, "unreadable pgn");
        assert_eq!(code(&o), 1, "{}", err(&o));
        assert!(err(&o).contains("secret.pgn"), "{}", err(&o));
        assert!(!dir.path().join("sec.si4").exists(), "nothing is created");
    }
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn import_of_a_truncated_gzip_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(many_games(40).as_bytes()).unwrap();
    let bytes = enc.finish().unwrap();
    let gz = write(dir.path(), "cut.pgn.gz", &bytes[..bytes.len() / 2]);
    let o = run(&["-q", "import", p(&gz)]);
    assert_no_panic(&o, "truncated gzip");
    assert!(matches!(code(&o), 0 | 1), "{}", err(&o));
    // Whatever was read is a valid database.
    if dir.path().join("cut.si4").exists() {
        assert!(Si4Database::open(&dir.path().join("cut")).is_ok());
    }
}

#[test]
fn import_reads_gzip_with_ignore_pregame_and_names_the_base_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!("Preface of the first game\n\n{FOUR}");
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(text.as_bytes()).unwrap();
    let gz = write(dir.path(), "club.pgn.gz", enc.finish().unwrap());
    assert_eq!(code(&run(&["-q", "import", "-x", p(&gz)])), 0);
    let keep = dir.path().join("keep");
    assert_eq!(code(&run(&["-q", "import", p(&gz), p(&keep)])), 0);
    let dropped = exported(dir.path(), &dir.path().join("club"));
    let kept = exported(dir.path(), &keep);
    assert!(kept.contains("Preface of the first game"));
    assert!(!dropped.contains("Preface of the first game"));
}

#[test]
fn import_summary_goes_to_stdout_and_a_base_with_extension_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "a.pgn", FOUR);
    let target = dir.path().join("named.SI4");
    let o = run(&["import", p(&pgn), p(&target)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Database '"), "{}", out(&o));
    assert!(out(&o).contains("4 games, 6 players, 2 events, 3 sites."));
    assert!(dir.path().join("named.si4").exists() || dir.path().join("named.SI4").exists());
    assert!(!dir.path().join("named.si4.si4").exists());
    assert_eq!(
        Si4Database::open(&dir.path().join("named")).unwrap().len(),
        4
    );
}

// ------------------------------------------------------------------ merge

#[test]
fn merge_preserves_every_editable_flag_and_the_content_of_each_game() {
    let dir = tempfile::tempdir().unwrap();
    let rich = base_from(dir.path(), "rich", RICH);
    let four = base_from(dir.path(), "four", FOUR);
    // One game per flag letter on `four` (4 games) and `rich` (3 games); the
    // 19 editable flags in total including `D`.
    for (n, letters) in [(1, "D"), (2, "WB!?"), (3, FLAG_LETTERS), (4, "123456")] {
        set_flags(&four, n, letters);
    }
    set_flags(&rich, 2, "D");
    set_flags(&rich, 3, "MENPTKQU");
    let target = dir.path().join("all");
    let o = run(&["merge", p(&target), p(&rich), p(&four)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Successfully merged 2 files, of 7 games."));
    let mut want = signature(&rich);
    want.extend(signature(&four));
    assert_eq!(signature(&target), want);
    let flags: Vec<u32> = signature(&target).iter().map(|s| s.1).collect();
    assert!(
        flags.contains(&GameFlags::EDITABLE.bits()),
        "all 19 flags kept"
    );
    assert!(flags.iter().any(|f| f & GameFlags::DELETE.bits() != 0));
}

#[test]
fn merge_of_a_database_with_itself_and_with_empty_ones() {
    let dir = tempfile::tempdir().unwrap();
    let one = base_from(dir.path(), "one", FOUR);
    let empty_pgn = write(dir.path(), "e.pgn", "");
    assert_eq!(code(&run(&["-q", "import", p(&empty_pgn)])), 0);
    let empty = dir.path().join("e");
    let target = dir.path().join("t");
    let o = run(&["merge", p(&target), p(&one), p(&one), p(&empty)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Successfully merged 3 files, of 8 games."));
    let only_empty = dir.path().join("t2");
    let o = run(&["merge", p(&only_empty), p(&empty)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("of 0 games"), "{}", out(&o));
    assert_eq!(Si4Database::open(&only_empty).unwrap().len(), 0);
}

#[test]
fn merge_target_may_carry_an_extension_and_is_refused_if_any_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let one = base_from(dir.path(), "one", FOUR);
    let target = dir.path().join("t.si4");
    assert_eq!(code(&run(&["-q", "merge", p(&target), p(&one)])), 0);
    assert!(dir.path().join("t.si4").exists());
    assert!(!dir.path().join("t.si4.si4").exists());

    for ext in ["sn4", "sg4", "si4"] {
        let stray = dir.path().join("only");
        let file = stray.with_extension(ext);
        fs::write(&file, b"keep me").unwrap();
        let o = run(&["merge", p(&stray), p(&one)]);
        assert_eq!(code(&o), 1, "{ext}: {}", err(&o));
        assert!(err(&o).contains("already exists"), "{ext}");
        assert_eq!(fs::read(&file).unwrap(), b"keep me", "{ext}");
        fs::remove_file(&file).unwrap();
    }
}

#[test]
fn merge_failures_remove_the_new_database() {
    let dir = tempfile::tempdir().unwrap();
    let good = base_from(dir.path(), "good", FOUR);
    let broken = base_from(dir.path(), "broken", FOUR);
    fs::write(broken.with_extension("sn4"), b"").unwrap();
    let target = dir.path().join("t");
    // First source fine, second broken: the partial target must vanish.
    let o = run(&["merge", p(&target), p(&good), p(&broken)]);
    assert_no_panic(&o, "merge with a broken source");
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("broken"), "{}", err(&o));
    for ext in ["si4", "sn4", "sg4", "err", "txn"] {
        assert!(
            !target.with_extension(ext).exists(),
            "{ext} left behind: {:?}",
            files_in(dir.path())
        );
    }

    // A source with a missing .sg4.
    fs::remove_file(good.with_extension("sg4")).unwrap();
    let o = run(&["merge", p(&target), p(&good)]);
    assert_no_panic(&o, "merge with a source without games file");
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(!target.with_extension("si4").exists());

    // Target in a directory that does not exist.
    let healthy = base_from(dir.path(), "healthy", FOUR);
    let nested = dir.path().join("nodir").join("t");
    let o = run(&["merge", p(&nested), p(&healthy)]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(!dir.path().join("nodir").exists());
}

#[test]
fn merge_with_unreadable_games_keeps_the_new_database_lists_them_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let source = base_from(dir.path(), "src", &many_games(60));
    let sg4 = source.with_extension("sg4");
    let bytes = fs::read(&sg4).unwrap();
    // Zero a stretch in the middle of the game file: some records become unreadable.
    let mut damaged = bytes.clone();
    let mid = damaged.len() / 2;
    for b in &mut damaged[mid..mid + 150] {
        *b = 0xFF;
    }
    fs::write(&sg4, damaged).unwrap();
    let target = dir.path().join("t");
    let o = run(&["merge", p(&target), p(&source)]);
    assert_no_panic(&o, "merge with damaged games");
    match code(&o) {
        1 => {
            assert!(
                err(&o).contains("warning") || err(&o).contains("error"),
                "{}",
                err(&o)
            );
            assert!(
                target.with_extension("si4").exists(),
                "kept on skipped games"
            );
            let n = Si4Database::open(&target).unwrap().len();
            assert!(n < 60, "{n}");
            assert!(out(&o).contains(&format!("of {n} games")), "{}", out(&o));
        }
        0 => {
            // Damage landed in padding: every game copied.
            assert_eq!(Si4Database::open(&target).unwrap().len(), 60);
        }
        other => panic!("exit {other}: {}", err(&o)),
    }
}

#[test]
fn merge_stdin_ignores_blank_lines_accepts_crlf_and_mixes_with_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let one = base_from(dir.path(), "one", FOUR);
    let two = base_from(dir.path(), "two", FOUR);
    let t = dir.path().join("t");
    let list = format!("\n\r\n{}\r\n   \n{}.sg4", one.display(), two.display());
    // "   " is a name (spaces); a database called "   " does not exist.
    let o = run_stdin(&["-q", "merge", p(&t), "-"], list.as_bytes());
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(!t.with_extension("si4").exists());

    let list = format!("\n\r\n{}\r\n\n{}.sg4\n", one.display(), two.display());
    let o = run_stdin(&["-q", "merge", p(&t), "-"], list.as_bytes());
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(Si4Database::open(&t).unwrap().len(), 8);

    let t2 = dir.path().join("t2");
    let o = run_stdin(
        &["-q", "merge", p(&t2), p(&one), "-"],
        two.display().to_string().as_bytes(),
    );
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(Si4Database::open(&t2).unwrap().len(), 8);

    for (name, input) in [("empty", ""), ("blank", "\n\n\r\n")] {
        let t3 = dir.path().join(format!("t-{name}"));
        let o = run_stdin(&["-q", "merge", p(&t3), "-"], input.as_bytes());
        assert_eq!(code(&o), 1, "{name}: {}", err(&o));
        assert!(!t3.with_extension("si4").exists(), "{name}");
    }
}

// ------------------------------------------------- info / compact / sort ops

#[test]
fn info_reports_the_database_without_writing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", RICH);
    let before = db_bytes(&base);
    let listing = files_in(dir.path());
    let o = run(&["info", p(&base)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("3 games"), "{text}");
    assert!(text.contains("Version: 4.0"), "{text}");
    assert!(
        text.contains("Namebase: 6 players, 1 events, 2 sites, 3 rounds."),
        "{text}"
    );
    assert_eq!(err(&o), "");
    let all = run(&["info", "--compact", "--names", p(&base)]);
    assert_eq!(code(&all), 0);
    for kind in ["PLAYER", "EVENT", "SITE", "ROUND"] {
        assert!(out(&all).contains(kind), "{kind}: {}", out(&all));
    }
    assert_eq!(db_bytes(&base), before);
    assert_eq!(files_in(dir.path()), listing);
}

#[test]
fn compact_is_idempotent_and_matches_the_info_prediction() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(25));
    set_flags(&base, 3, "D");
    set_flags(&base, 10, "D");
    set_flags(&base, 11, "D");
    let predicted = out(&run(&["info", "--compact", p(&base)]));
    assert!(
        predicted.contains("Compacted size:      22 games"),
        "{predicted}"
    );
    let o = run(&["-q", "compact", p(&base)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("22 games kept, 3 deleted"), "{}", out(&o));
    let first = db_bytes(&base);
    let again = run(&["-q", "compact", p(&base)]);
    assert!(
        out(&again).contains("22 games kept, 0 deleted"),
        "{}",
        out(&again)
    );
    assert_eq!(
        db_bytes(&base),
        first,
        "a second compaction changes nothing"
    );
    let names = run(&["-q", "compact", "--names", p(&base)]);
    assert_eq!(code(&names), 0);
    let second_names = db_bytes(&base);
    let names = run(&["-q", "compact", "--names", p(&base)]);
    assert!(
        out(&names).contains("nothing to compact"),
        "{}",
        out(&names)
    );
    assert_eq!(db_bytes(&base), second_names);
    // The remaining games are exactly the undeleted ones, in order.
    let text = exported(dir.path(), &base);
    for gone in [3, 10, 11] {
        assert!(!text.contains(&format!("P{:04}", gone - 1)), "game {gone}");
    }
    assert!(text.contains("P0000") && text.contains("P0024"));
}

#[test]
fn sort_is_idempotent_and_reverse_twice_returns_to_a_stable_state() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(30));
    assert_eq!(code(&run(&["-q", "sort", "date,white", p(&base)])), 0);
    let once = db_bytes(&base);
    let o = run(&["sort", "date,white", p(&base)]);
    assert_eq!(code(&o), 0);
    assert!(out(&o).contains("0 moved"), "{}", out(&o));
    assert_eq!(db_bytes(&base), once);
    // Only the index changes; the games and names files are untouched.
    let original = base_from(dir.path(), "y", &many_games(30));
    let o = db_bytes(&original);
    assert_eq!(once[1], o[1], "sn4");
    assert_eq!(once[2], o[2], "sg4");
}

#[test]
fn sort_by_result_orders_wins_before_the_rest_through_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(8));
    assert_eq!(code(&run(&["-q", "sort", "result", p(&base)])), 0);
    let text = exported(dir.path(), &base);
    let results: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("[Result \""))
        .map(|l| l.trim_end_matches("\"]"))
        .collect();
    // 1-0, 1-0, 0-1, 0-1, then draws and unfinished (any order between them).
    assert_eq!(&results[..2], ["1-0", "1-0"]);
    assert_eq!(&results[2..4], ["0-1", "0-1"]);
    assert!(results[4..].iter().all(|r| *r == "1/2-1/2" || *r == "*"));
}

#[test]
fn sort_preserves_the_flags_of_every_game() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    set_flags(&base, 1, "D");
    set_flags(&base, 3, "WB");
    let before = signature(&base);
    assert_eq!(code(&run(&["-q", "sort", "white", p(&base)])), 0);
    let mut after = signature(&base);
    let mut want = before;
    want.sort();
    after.sort();
    assert_eq!(after, want);
}

// ------------------------------------------------------------- round trips

#[test]
fn full_cycle_import_info_sort_merge_compact_export_reimport_keeps_games_flags_and_names() {
    let dir = tempfile::tempdir().unwrap();
    let a = base_from(dir.path(), "a", &format!("{RICH}\n{FOUR}"));
    assert_eq!(signature(&a).len(), 7);
    set_flags(&a, 2, "D");
    set_flags(&a, 5, "WB!");
    set_flags(&a, 6, "123");
    let original = signature(&a);

    let info = out(&run(&["info", "--names", p(&a)]));
    assert!(info.contains("7 games"), "{info}");
    let names_before = out(&run(&["names", p(&a), "player"]));

    // sort: same multiset of (game, flags).
    assert_eq!(code(&run(&["-q", "sort", "white,black", p(&a)])), 0);
    let mut sorted = signature(&a);
    let mut want = original.clone();
    sorted.sort();
    want.sort();
    assert_eq!(sorted, want, "sort must only permute");
    let whites: Vec<String> = {
        let mut db = Si4Database::open(&a).unwrap();
        (1..=db.len())
            .map(|n| db.game(n).unwrap().tags().white.clone())
            .collect()
    };
    assert!(whites.windows(2).all(|w| w[0] <= w[1]), "{whites:?}");

    // merge keeps order, content and flags (including D).
    let m = dir.path().join("m");
    assert_eq!(code(&run(&["-q", "merge", p(&m), p(&a)])), 0);
    assert_eq!(signature(&m), signature(&a));
    let as_set = |text: String| -> BTreeSet<String> { text.lines().map(str::to_owned).collect() };
    assert_eq!(
        as_set(out(&run(&["names", p(&m), "player"]))),
        as_set(names_before),
        "same names and frequencies; ids (file order) may differ after a merge"
    );

    // compact drops the D game only.
    assert_eq!(code(&run(&["-q", "compact", p(&m)])), 0);
    let compacted = signature(&m);
    assert_eq!(compacted.len(), 6);
    assert!(
        compacted
            .iter()
            .all(|s| s.1 & GameFlags::DELETE.bits() == 0)
    );
    assert_eq!(code(&run(&["-q", "compact", "--names", p(&m)])), 0);
    let remaining: Vec<(String, u32)> = signature(&a)
        .into_iter()
        .filter(|s| s.1 & GameFlags::DELETE.bits() == 0)
        .collect();
    assert_eq!(compacted, remaining);

    // export and reimport: same games; flags are not in PGN.
    let pgn = dir.path().join("round.pgn");
    assert_eq!(code(&run(&["-q", "export", p(&m), p(&pgn)])), 0);
    let r = dir.path().join("r");
    assert_eq!(code(&run(&["-q", "import", p(&pgn), p(&r)])), 0);
    let reimported: Vec<String> = signature(&r).into_iter().map(|s| s.0).collect();
    let expected: Vec<String> = compacted.iter().map(|s| s.0.clone()).collect();
    assert_eq!(reimported, expected);
    assert_eq!(exported(dir.path(), &r), exported(dir.path(), &m));
    // The names after compaction equal the names of the reimported file.
    let strip = |text: String| -> BTreeSet<String> {
        text.lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(f, n)| format!("{}:{n}", f.trim()))
            .collect()
    };
    assert_eq!(
        strip(out(&run(&["names", p(&m), "player"]))),
        strip(out(&run(&["names", p(&r), "player"])))
    );
}

#[test]
fn import_force_twice_is_idempotent_for_the_games() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "a.pgn", RICH);
    let base = dir.path().join("a");
    assert_eq!(code(&run(&["-q", "import", p(&pgn)])), 0);
    let first = signature(&base);
    for _ in 0..2 {
        assert_eq!(code(&run(&["-q", "import", "-f", p(&pgn)])), 0);
    }
    assert_eq!(signature(&base), first);
}

#[test]
fn export_text_is_lf_only_extension_is_added_and_append_adds_games() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", RICH);
    let bare = dir.path().join("dump");
    let o = run(&["export", p(&base), p(&bare)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Exported 3 games to"), "{}", out(&o));
    let file = dir.path().join("dump.pgn");
    assert!(file.is_file(), "{:?}", files_in(dir.path()));
    let bytes = fs::read(&file).unwrap();
    assert!(!bytes.contains(&b'\r'), "export must use \\n only");
    assert!(bytes.ends_with(b"\n"));
    let count = |b: &[u8]| String::from_utf8_lossy(b).matches("[Event ").count();
    assert_eq!(count(&bytes), 3);

    let o = run(&["-q", "export", "--append", p(&base), p(&file)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(count(&fs::read(&file).unwrap()), 6);
    // Without --append the file is replaced.
    let o = run(&["-q", "export", p(&base), p(&file)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(count(&fs::read(&file).unwrap()), 3);
}

#[test]
fn export_to_an_impossible_destination_fails_without_litter() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let listing = files_in(dir.path());
    let nested = dir.path().join("nodir").join("o.pgn");
    let o = run(&["export", p(&base), p(&nested)]);
    assert_no_panic(&o, "export to missing dir");
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert_eq!(files_in(dir.path()), listing);
    let folder = dir.path().join("folder.pgn");
    fs::create_dir(&folder).unwrap();
    let o = run(&["export", p(&base), p(&folder)]);
    assert_no_panic(&o, "export onto a directory");
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(folder.is_dir());
}

// --------------------------------------------------------- quiet and streams

#[test]
fn quiet_leaves_stderr_empty_on_success_for_every_subcommand_before_or_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "a.pgn", many_games(25));
    let base = dir.path().join("a");
    let two = dir.path().join("b");
    let merged = dir.path().join("m");
    let dump = dir.path().join("o.pgn");
    let cases: Vec<Vec<&str>> = vec![
        vec!["import", "-q", p(&pgn), p(&base)],
        vec!["-q", "import", "-f", p(&pgn), p(&two)],
        vec!["info", "-q", p(&base)],
        vec!["info", "--quiet", "--names", "--compact", p(&base)],
        vec!["names", "-q", p(&base), "event"],
        vec!["describe", "-q", p(&base), "d"],
        vec!["sort", "-q", "date", p(&base)],
        vec!["compact", "-q", p(&base)],
        vec!["compact", "-q", "--names", p(&base)],
        vec!["merge", "-q", p(&merged), p(&base), p(&two)],
        vec!["export", "-q", p(&base), p(&dump)],
    ];
    for args in cases {
        let o = run(&args);
        assert_eq!(code(&o), 0, "{args:?}: {}", err(&o));
        assert_eq!(err(&o), "", "{args:?}");
    }
}

#[test]
fn quiet_still_prints_errors_and_the_results() {
    let dir = tempfile::tempdir().unwrap();
    let ghost = dir.path().join("ghost");
    let o = run(&["-q", "info", p(&ghost)]);
    assert_eq!(code(&o), 1);
    assert!(err(&o).starts_with("error: "));
    let base = base_from(dir.path(), "x", FOUR);
    let o = run(&["-q", "info", p(&base)]);
    assert!(out(&o).contains("4 games"));
    let o = run(&["-q", "names", p(&base), "site"]);
    assert!(out(&o).contains("Oslo"));
}

#[test]
fn output_has_no_control_sequences_and_progress_is_plain_lines_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "big.pgn", many_games(300));
    let base = dir.path().join("big");
    let dump = dir.path().join("big-out.pgn");
    let merged = dir.path().join("merged");
    let ghost = dir.path().join("ghost");
    let cases: Vec<Vec<&str>> = vec![
        vec!["import", p(&pgn), p(&base)],
        vec!["info", "--compact", "--names", p(&base)],
        vec!["names", p(&base), "player", "P00"],
        vec!["sort", "-r", "date", p(&base)],
        vec!["describe", p(&base), "big"],
        vec!["compact", p(&base)],
        vec!["compact", "--names", p(&base)],
        vec!["merge", p(&merged), p(&base), p(&base)],
        vec!["export", p(&base), p(&dump)],
        vec!["info", p(&ghost)],
        vec!["sort", "nope", p(&base)],
    ];
    let mut progress_seen = BTreeSet::new();
    for args in cases {
        let o = run(&args);
        for (name, bytes) in [("stdout", &o.stdout), ("stderr", &o.stderr)] {
            assert!(!bytes.contains(&b'\r'), "{args:?} {name} has \\r");
            assert!(!bytes.contains(&0x1b), "{args:?} {name} has ESC");
            assert!(
                std::str::from_utf8(bytes).is_ok(),
                "{args:?} {name} is not UTF-8"
            );
        }
        // Progress lines never go to stdout.
        assert!(
            !out(&o)
                .lines()
                .any(|l| l.contains('%') && l.contains(": ") && l.starts_with("import")),
            "{args:?}"
        );
        for line in err(&o).lines() {
            if let Some((label, pct)) = line.split_once(": ")
                && pct.ends_with('%')
            {
                let n: u32 = pct.trim_end_matches('%').parse().unwrap();
                assert!(n.is_multiple_of(10) && n <= 100, "{line}");
                progress_seen.insert(label.to_string());
            }
        }
    }
    assert!(progress_seen.contains("import"), "{progress_seen:?}");
    assert!(progress_seen.contains("export"), "{progress_seen:?}");
}

#[test]
fn progress_percentages_are_increasing_without_repeats() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "big.pgn", many_games(300));
    let o = run(&["import", p(&pgn)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let steps: Vec<u32> = err(&o)
        .lines()
        .filter_map(|l| l.strip_prefix("import: "))
        .map(|l| l.trim_end_matches('%').parse().unwrap())
        .collect();
    assert!(!steps.is_empty(), "no progress on stderr: {}", err(&o));
    assert!(steps.windows(2).all(|w| w[0] < w[1]), "{steps:?}");
}

#[test]
fn error_messages_are_one_line_with_the_cause_chain_and_no_backtrace() {
    let dir = tempfile::tempdir().unwrap();
    let ghost = dir.path().join("ghost");
    let o = run(&["info", p(&ghost)]);
    let text = err(&o);
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.starts_with("error: "));
    assert!(text.contains(": "), "causes are chained with ': ': {text}");
    assert_eq!(out(&o), "");
}

// ------------------------------------------------------- cancellation (lib)

fn big_pgn_file(dir: &Path, games: usize) -> PathBuf {
    write(dir, "big.pgn", many_games(games))
}

#[test]
fn import_with_a_cancelled_token_stores_what_it_read_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = big_pgn_file(dir.path(), 50);
    let base = dir.path().join("c");
    let token = CancelToken::new();
    token.cancel();
    let mut err_buf = Vec::new();
    let (code, stdout) = lib(
        &argv(&["-q", "import", p(&pgn), p(&base)]),
        b"",
        &token,
        &mut err_buf,
    );
    let stderr = String::from_utf8_lossy(&err_buf);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("interrupted"), "{stderr}");
    let stored = Si4Database::open(&base).unwrap().len();
    assert!(stored < 50, "{stored}");
    assert!(stderr.contains(&format!("{stored} games")), "{stderr}");
    assert!(stdout.contains(&format!("{stored} games")), "{stdout}");
}

#[test]
fn import_cancelled_midway_keeps_a_valid_database_with_a_prefix_of_the_games() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = big_pgn_file(dir.path(), 9000);
    let base = dir.path().join("c");
    let token = CancelToken::new();
    let mut sink = CancelOn {
        token: &token,
        needle: "import: ".into(),
        seen: Vec::new(),
    };
    let (code, stdout) = lib(
        &argv(&["import", p(&pgn), p(&base)]),
        b"",
        &token,
        &mut sink,
    );
    assert_eq!(code, 1, "{}", String::from_utf8_lossy(&sink.seen));
    let seen = String::from_utf8_lossy(&sink.seen).into_owned();
    assert!(seen.contains("interrupted"), "{seen}");
    assert!(!seen.contains("import: 100%"), "{seen}");
    let mut db = Si4Database::open(&base).unwrap();
    let stored = db.len();
    assert!(stored > 0 && stored < 9000, "{stored}");
    assert!(stdout.contains(&format!("{stored} games")), "{stdout}");
    // The games kept are the first ones, whole.
    for n in 1..=stored {
        assert_eq!(db.game(n).unwrap().tags().white, format!("P{:04}", n - 1));
    }
}

#[test]
fn merge_with_a_cancelled_token_removes_the_new_database() {
    let dir = tempfile::tempdir().unwrap();
    let one = base_from(dir.path(), "one", FOUR);
    let target = dir.path().join("t");
    let token = CancelToken::new();
    token.cancel();
    let mut err_buf = Vec::new();
    let (code, _) = lib(
        &argv(&["merge", p(&target), p(&one)]),
        b"",
        &token,
        &mut err_buf,
    );
    assert_eq!(code, 1, "{}", String::from_utf8_lossy(&err_buf));
    assert!(String::from_utf8_lossy(&err_buf).contains("interrupted"));
    for ext in ["si4", "sn4", "sg4", "txn", "err"] {
        assert!(!target.with_extension(ext).exists(), "{ext}");
    }
    assert!(one.with_extension("si4").exists());
}

#[test]
fn merge_cancelled_while_copying_the_second_source_removes_the_partial_target() {
    let dir = tempfile::tempdir().unwrap();
    let one = base_from(dir.path(), "one", &many_games(30));
    let two = base_from(dir.path(), "two", &many_games(30));
    let target = dir.path().join("t");
    let token = CancelToken::new();
    let mut sink = CancelOn {
        token: &token,
        needle: format!("merge: {}", two.display()),
        seen: Vec::new(),
    };
    let (code, _) = lib(
        &argv(&["merge", p(&target), p(&one), p(&two)]),
        b"",
        &token,
        &mut sink,
    );
    assert_eq!(code, 1, "{}", String::from_utf8_lossy(&sink.seen));
    assert!(String::from_utf8_lossy(&sink.seen).contains("interrupted"));
    assert!(token.is_cancelled());
    assert_eq!(
        files_in(dir.path()),
        files_in(dir.path())
            .into_iter()
            .filter(|f| f.starts_with("one") || f.starts_with("two"))
            .collect::<BTreeSet<_>>(),
        "the partial target must be gone"
    );
}

#[test]
fn export_with_a_cancelled_token_fails_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(40));
    let dest = write(dir.path(), "dest.pgn", "OLD CONTENT\n");
    let listing = files_in(dir.path());
    let token = CancelToken::new();
    token.cancel();
    let mut err_buf = Vec::new();
    let (code, _) = lib(
        &argv(&["-q", "export", p(&base), p(&dest)]),
        b"",
        &token,
        &mut err_buf,
    );
    let stderr = String::from_utf8_lossy(&err_buf);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("interrupted"), "{stderr}");
    assert_eq!(files_in(dir.path()), listing, "temporary file left behind");
    let text = fs::read_to_string(&dest).unwrap();
    // The old file is intact, or the new one is whole games up to the stop.
    let whole = text.matches("[Event ").count() == text.matches("\n\n1. ").count();
    assert!(text == "OLD CONTENT\n" || whole, "{text:?}");
}

#[test]
fn export_cancelled_midway_leaves_the_old_file_or_only_whole_games() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(400));
    let dest = write(dir.path(), "dest.pgn", "OLD CONTENT\n");
    let listing = files_in(dir.path());
    let token = CancelToken::new();
    let mut sink = CancelOn {
        token: &token,
        needle: "export: 20%".into(),
        seen: Vec::new(),
    };
    let (code, _) = lib(
        &argv(&["export", p(&base), p(&dest)]),
        b"",
        &token,
        &mut sink,
    );
    let seen = String::from_utf8_lossy(&sink.seen).into_owned();
    assert_eq!(code, 1, "{seen}");
    assert!(seen.contains("interrupted"), "{seen}");
    assert_eq!(files_in(dir.path()), listing, "temporary file left behind");
    let text = fs::read_to_string(&dest).unwrap();
    if text != "OLD CONTENT\n" {
        let events = text.matches("[Event ").count();
        assert!(events > 0 && events < 400, "{events}");
        assert_eq!(events, text.matches("[Result ").count());
        assert!(
            text.trim_end().ends_with("1-0")
                || text.trim_end().ends_with("0-1")
                || text.trim_end().ends_with("1/2-1/2")
                || text.trim_end().ends_with('*'),
            "cut mid game"
        );
    }
}

#[test]
fn sort_and_compact_with_a_cancelled_token_finish_atomically_and_warn() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let token = CancelToken::new();
    token.cancel();
    for args in [
        argv(&["sort", "white", p(&base)]),
        argv(&["compact", p(&base)]),
    ] {
        let mut err_buf = Vec::new();
        let (code, _) = lib(&args, b"", &token, &mut err_buf);
        let stderr = String::from_utf8_lossy(&err_buf).to_lowercase();
        // Spec rule 10: they do not stop in the middle; the signal is read
        // afterwards and, if it arrived, the tool warns.
        assert!(matches!(code, 0 | 1), "{stderr}");
        assert!(
            stderr.contains("interrupt") || stderr.contains("cancel"),
            "no warning about the signal: {stderr}"
        );
        assert!(Si4Database::open(&base).is_ok(), "database damaged");
    }
}

#[test]
fn readers_ignore_the_token_and_the_library_never_panics_on_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let token = CancelToken::new();
    for args in [
        argv(&["info", p(&base)]),
        argv(&["names", p(&base), "player"]),
        argv(&[]),
        argv(&["merge", p(&dir.path().join("t")), "-"]),
        argv(&["--help"]),
    ] {
        let mut sink = Vec::new();
        drop(lib(&args, b"", &token, &mut sink));
    }
}

#[cfg(unix)]
#[test]
fn sigint_stops_a_blocked_process_that_has_no_handler_work_to_do() {
    // `merge ... -` blocks on stdin; the handler cancels the token but the
    // read cannot be interrupted, so the process must at least end once its
    // stdin closes, with no target left behind.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("t");
    let mut child = bin()
        .args(["merge", p(&target), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id().to_string();
    Command::new("kill").args(["-INT", &pid]).status().ok();
    drop(child.stdin.take());
    let o = child.wait_with_output().unwrap();
    assert!(o.status.code().is_some() || !o.status.success());
    assert!(!target.with_extension("si4").exists());
}

// ------------------------------------------------- compatibility with Scid

fn scid_bases() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27/bases");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    dir
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_info_and_names_read_the_bundled_scid_databases_in_place() {
    let bases = scid_bases();
    for (name, games) in [("endings", 67), ("matein1", 1212), ("tactics", 928)] {
        let base = bases.join(name);
        let o = run(&["info", "--names", p(&base)]);
        assert_eq!(code(&o), 0, "{name}: {}", err(&o));
        assert!(
            out(&o).contains(&format!("{games} games")),
            "{name}: {}",
            out(&o)
        );
        let o = run(&["names", p(&base), "player"]);
        assert_eq!(code(&o), 0, "{name}: {}", err(&o));
    }
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_merge_export_reimport_and_sort_of_the_bundled_databases() {
    let bases = scid_bases();
    let dir = tempfile::tempdir().unwrap();
    let merged = dir.path().join("all");
    let o = run(&[
        "-q",
        "merge",
        p(&merged),
        p(&bases.join("endings")),
        p(&bases.join("matein1")),
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(Si4Database::open(&merged).unwrap().len(), 67 + 1212);

    // The merged games are the originals, flags included.
    let mut originals = signature(&bases.join("endings"));
    originals.extend(signature(&bases.join("matein1")));
    assert_eq!(signature(&merged), originals);

    // Sort by ECO, then check the index order with the library.
    assert_eq!(code(&run(&["-q", "sort", "eco,white", p(&merged)])), 0);
    let db = Si4Database::open(&merged).unwrap();
    let codes: Vec<u16> = (1..=db.len())
        .map(|n| db.entry(n).unwrap().eco_code)
        .collect();
    assert!(codes.windows(2).all(|w| w[0] <= w[1]));
    drop(db);

    // Compact, export, reimport: the same games.
    assert_eq!(code(&run(&["-q", "compact", p(&merged)])), 0);
    let pgn = dir.path().join("all.pgn");
    assert_eq!(code(&run(&["-q", "export", p(&merged), p(&pgn)])), 0);
    let again = dir.path().join("again");
    assert_eq!(code(&run(&["-q", "import", p(&pgn), p(&again)])), 0);
    // `import` ignores PlyCount (D349), and the puzzle games of Scid carry one.
    let no_ply = |text: String| -> String {
        text.lines()
            .filter(|l| !l.starts_with("[PlyCount"))
            // An empty tag value reads back as the unknown value "?".
            .map(|l| if l.ends_with(" \"\"]") { l.replace("\"\"", "\"?\"") } else { l.to_owned() })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let left: Vec<String> = signature(&merged)
        .into_iter()
        .map(|s| no_ply(s.0))
        .collect();
    let right: Vec<String> = signature(&again).into_iter().map(|s| no_ply(s.0)).collect();
    assert_eq!(left.len(), right.len());
    if let Some(i) = (0..left.len()).find(|&i| left[i] != right[i]) {
        panic!("game {} differs:\n{}\n---\n{}", i + 1, left[i], right[i]);
    }
}

// ------------------------------------------- review round (D628 to D634)

#[test]
fn export_streams_every_game_in_index_order() {
    // D628: the games reach the exporter one at a time, in index order.
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", &many_games(300));
    let dest = dir.path().join("out.pgn");
    let o = run(&["-q", "export", p(&base), p(&dest)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).starts_with("Exported 300 games to "), "{}", out(&o));
    let text = fs::read_to_string(&dest).unwrap();
    assert_eq!(text.matches("[Event ").count(), 300);
    let whites: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("[White \""))
        .collect();
    let want: Vec<String> = (0..300).map(|i| format!("P{i:04}\"]")).collect();
    assert_eq!(whites, want.iter().map(String::as_str).collect::<Vec<_>>());
}

#[test]
fn sort_and_compact_report_the_late_signal_with_code_one_after_finishing() {
    // D626: the operation is atomic; the signal is only reported afterwards.
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let token = CancelToken::new();
    token.cancel();
    let mut err_buf = Vec::new();
    let (code, stdout) = lib(
        &argv(&["-q", "sort", "white", p(&base)]),
        b"",
        &token,
        &mut err_buf,
    );
    assert_eq!(code, 1);
    assert!(stdout.contains("successfully sorted"), "{stdout}");
    assert!(
        String::from_utf8_lossy(&err_buf).starts_with("interrupted:"),
        "{}",
        String::from_utf8_lossy(&err_buf)
    );
    let whites = exported(dir.path(), &base);
    let order: Vec<&str> = whites
        .lines()
        .filter_map(|l| l.strip_prefix("[White \""))
        .collect();
    assert_eq!(order, ["Alpha\"]", "Bravo\"]", "Charlie\"]", "Delta\"]"]);

    let mut err_buf = Vec::new();
    let (code, stdout) = lib(&argv(&["compact", p(&base)]), b"", &token, &mut err_buf);
    assert_eq!(code, 1);
    assert!(stdout.contains("successfully compacted"), "{stdout}");
    assert!(String::from_utf8_lossy(&err_buf).contains("interrupted:"));
    assert_eq!(Si4Database::open(&base).unwrap().len(), 4);
}

#[cfg(unix)]
#[test]
fn the_first_signal_asks_to_stop_and_the_second_ends_the_process_with_130() {
    // `merge -` blocks reading standard input, so the process is alive and
    // idle while the signals arrive (D629). SIGTERM counts like Ctrl-C.
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("t");
    let mut child = bin()
        .args(["-q", "merge", p(&target), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id().to_string();
    let signal = |name: &str| {
        let status = Command::new("kill")
            .args([name, &pid])
            .status()
            .expect("kill");
        assert!(status.success());
    };
    std::thread::sleep(Duration::from_millis(600)); // the handler is installed
    signal("-TERM");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the first signal must not end the process"
    );
    signal("-INT");
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130));
    assert!(!files_in(dir.path()).iter().any(|f| f.starts_with("t.")));
}

#[cfg(unix)]
#[test]
fn a_database_that_cannot_be_removed_after_a_cancelled_merge_is_reported() {
    // D631: the failure to remove the partial database is a warning, and the
    // message does not claim that the database was not created.
    use std::os::unix::fs::PermissionsExt;
    struct LockDirOnMerge<'a> {
        token: &'a CancelToken,
        dir: PathBuf,
        seen: Vec<u8>,
    }
    impl Write for LockDirOnMerge<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.seen.extend_from_slice(bytes);
            if bytes.starts_with(b"merge:") {
                fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o555)).ok();
                self.token.cancel();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    let target = dir.path().join("t");
    let token = CancelToken::new();
    let mut sink = LockDirOnMerge {
        token: &token,
        dir: dir.path().to_owned(),
        seen: Vec::new(),
    };
    let (code, _) = lib(
        &argv(&["merge", p(&target), p(&base)]),
        b"",
        &token,
        &mut sink,
    );
    let can_write = fs::write(dir.path().join("probe"), b"").is_ok();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if can_write {
        return; // root ignores the permission bits: nothing to test
    }
    let stderr = String::from_utf8_lossy(&sink.seen).into_owned();
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("warning: could not remove the partial database"),
        "{stderr}"
    );
    assert!(!stderr.contains("was not created"), "{stderr}");
    assert!(target.with_extension("si4").exists());
}

#[test]
fn force_lists_the_files_it_removes_even_with_quiet() {
    // D631.
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    write(dir.path(), "x.stc", b"cache");
    let pgn = write(dir.path(), "new.pgn", FOUR);
    let o = run(&["-q", "import", "--force", p(&pgn), p(&base)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let stderr = err(&o);
    let removed: Vec<&str> = stderr
        .lines()
        .filter_map(|l| l.strip_prefix("warning: --force removes "))
        .collect();
    assert_eq!(removed.len(), 4, "{stderr}");
    for ext in ["si4", "sn4", "sg4", "stc"] {
        let want = base.with_extension(ext);
        assert!(removed.contains(&p(&want)), "{ext}: {stderr}");
    }
    // A fresh import has nothing to list.
    let other = dir.path().join("fresh");
    let o = run(&["-q", "import", "--force", p(&pgn), p(&other)]);
    assert_eq!(err(&o), "");
}

#[test]
fn force_works_with_a_relative_base_that_has_no_directory() {
    // Regression of D631: `case_variants` saw `./g.sg4` (from `read_dir(".")`)
    // as a case variant of `g.sg4`, so `--force g.pgn` refused its own database.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "g.pgn", FOUR);
    let at = |args: &[&str]| {
        bin()
            .current_dir(dir.path())
            .args(args)
            .output()
            .expect("run mychess-cli")
    };
    let first = at(&["import", "g.pgn"]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(dir.path().join("g.si4").is_file());
    let refused = at(&["import", "g.pgn"]);
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    let forced = at(&["import", "--force", "g.pgn"]);
    assert_eq!(code(&forced), 0, "{}", err(&forced));
    assert!(!err(&forced).contains("letter case"), "{}", err(&forced));
    assert!(err(&forced).contains("--force removes"), "{}", err(&forced));
    assert!(dir.path().join("g.si4").is_file());
}

#[test]
fn force_never_removes_a_name_that_differs_only_in_case_and_fails_before_deleting() {
    // D631: `X.SI4` is not removed because of `x`, and the refusal comes
    // before the deletion of `x.*` (the writer would refuse to create it).
    let dir = tempfile::tempdir().unwrap();
    let base = base_from(dir.path(), "x", FOUR);
    write(dir.path(), "X.SI4", b"unrelated");
    if fs::read(base.with_extension("si4")).unwrap() == b"unrelated" {
        return; // case-insensitive file system: one name only
    }
    let pgn = write(dir.path(), "new.pgn", FOUR);
    let before = db_bytes(&base);
    let o = run(&["-q", "import", "--force", p(&pgn), p(&base)]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("only in letter case"), "{}", err(&o));
    assert!(!err(&o).contains("--force removes"), "{}", err(&o));
    assert_eq!(fs::read(dir.path().join("X.SI4")).unwrap(), b"unrelated");
    assert_eq!(db_bytes(&base), before);
}
