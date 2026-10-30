//! End-to-end tests of the `mychess-cli` binary
//! (`docs/specs/m3-5-mychess-cli.md`). The PGN fixtures are written here:
//! the moves are facts, the headers are ours.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mychess_db::si4::{FlagOp, GameFlags, Si4Database, Si4Writer};
use pretty_assertions::assert_eq;

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
[PlyCount \"4\"]

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

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mychess-cli"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run mychess-cli")
}

fn run_path(args: &[&Path]) -> Output {
    bin().args(args).output().expect("run mychess-cli")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

fn stdout(output: &Output) -> String {
    text(&output.stdout)
}

fn stderr(output: &Output) -> String {
    text(&output.stderr)
}

fn arg(path: &Path) -> &str {
    path.to_str().expect("utf-8 temp path")
}

fn write_pgn(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write pgn");
    path
}

/// Imports `FOUR` into `<dir>/<name>` and returns the base path.
fn make_base(dir: &Path, name: &str) -> PathBuf {
    let pgn = write_pgn(dir, &format!("{name}.pgn"), FOUR);
    let base = dir.join(name);
    let out = run(&["-q", "import", arg(&pgn), arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    base
}

/// The `White` tags of the games of the PGN produced by `export`.
fn exported_whites(dir: &Path, base: &Path) -> Vec<String> {
    let pgn = dir.join("out.pgn");
    fs::remove_file(&pgn).ok();
    let out = run(&["-q", "export", arg(base), arg(&pgn)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    fs::read_to_string(&pgn)
        .expect("read export")
        .lines()
        .filter_map(|l| l.strip_prefix("[White \""))
        .map(|l| l.trim_end_matches("\"]").to_owned())
        .collect()
}

fn mark_deleted(base: &Path, number: u32) {
    let mut writer = Si4Writer::open(base).expect("open for writing");
    writer
        .set_flag(number, GameFlags::DELETE, FlagOp::Set)
        .expect("set D");
}

#[test]
fn info_names_compact_prints_exactly_this_text() {
    // The whole text, reviewed by hand when the code of `info` moved to
    // `mychess_db::maintenance` (M7-1a1): any change of the output shows up.
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_pgn(dir.path(), "club.pgn", FOUR);
    let base = dir.path().join("club");
    let out = run_path(&[Path::new("import"), &pgn]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let out = run(&["info", "--names", "--compact", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let shown = stdout(&out).replace(&base.display().to_string(), "<BASE>");
    let expected = [
        "Database \"<BASE>\": ",
        "4 games",
        "Version: 4.0",
        "Namebase: 6 players, 2 events, 3 sites, 4 rounds.",
        "Current size:         4 games,       28 bytes =     0 Kb",
        "Compacted size:       4 games,       28 bytes =     0 Kb",
        "",
        "PLAYER section:    6 names, 0 unused",
        "Longest name: \"Charlie\" (7 bytes)",
        "Most frequent name: \"Delta\" (2 times)",
        "",
        "EVENT section:    2 names, 0 unused",
        "Longest name: \"Club night\" (10 bytes)",
        "Most frequent name: \"Club night\" (2 times)",
        "",
        "SITE section:    3 names, 0 unused",
        "Longest name: \"Paris FRA\" (9 bytes)",
        "Most frequent name: \"Oslo\" (2 times)",
        "",
        "ROUND section:    4 names, 0 unused",
        "Longest name: \"10\" (2 bytes)",
        "Most frequent name: \"10\" (1 times)",
        "",
    ]
    .join("\n");
    assert_eq!(shown, expected);
}

#[test]
fn import_info_sort_compact_export_flow() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_pgn(dir.path(), "club.pgn", FOUR);
    let base = dir.path().join("club");

    let out = run_path(&[Path::new("import"), &pgn]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let summary = stdout(&out);
    assert!(
        summary.contains("4 games, 6 players, 2 events, 3 sites."),
        "{summary}"
    );
    assert!(
        summary.contains("There were no warnings or errors."),
        "{summary}"
    );
    assert!(!dir.path().join("club.err").exists());
    assert!(dir.path().join("club.si4").is_file());

    let out = run(&["info", "--names", "--compact", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let info = stdout(&out);
    assert!(info.contains("4 games"), "{info}");
    assert!(
        info.contains("Namebase: 6 players, 2 events, 3 sites, 4 rounds."),
        "{info}"
    );
    assert!(info.contains("Current size:"), "{info}");
    assert!(info.contains("PLAYER section:"), "{info}");
    assert!(
        info.contains("Most frequent name: \"Delta\" (2 times)"),
        "{info}"
    );

    assert_eq!(
        exported_whites(dir.path(), &base),
        ["Delta", "Alpha", "Charlie", "Bravo"]
    );

    let out = run(&["sort", "date", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("successfully sorted"));
    assert_eq!(
        exported_whites(dir.path(), &base),
        ["Bravo", "Alpha", "Delta", "Charlie"]
    );

    let out = run(&["sort", "--reverse", "date", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        exported_whites(dir.path(), &base),
        ["Charlie", "Delta", "Alpha", "Bravo"]
    );

    // Rounds compare as numbers: 1, 2, 3, 10.
    let out = run(&["sort", "round", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        exported_whites(dir.path(), &base),
        ["Charlie", "Alpha", "Bravo", "Delta"]
    );

    mark_deleted(&base, 1);
    let out = run(&["info", "--compact", arg(&base)]);
    let before = stdout(&out);
    assert!(before.contains("Compacted size:       3 games"), "{before}");

    let out = run(&["compact", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("3 games kept, 1 deleted"),
        "{}",
        stdout(&out)
    );
    let out = run(&["info", "--compact", arg(&base)]);
    let after = stdout(&out);
    assert!(after.contains("3 games"), "{after}");
    let sizes: Vec<&str> = after
        .lines()
        .filter(|l| l.contains(" bytes = "))
        .map(|l| l.split_once("games,").expect("size line").1)
        .collect();
    assert_eq!(sizes.len(), 2);
    assert_eq!(
        sizes[0], sizes[1],
        "compacted size must equal the real size"
    );
    assert_eq!(
        exported_whites(dir.path(), &base),
        ["Alpha", "Bravo", "Delta"]
    );
}

#[test]
fn import_reads_gzip_and_derives_the_database_name() {
    let dir = tempfile::tempdir().unwrap();
    let gz = dir.path().join("zipped.pgn.gz");
    let mut encoder =
        flate2::write::GzEncoder::new(fs::File::create(&gz).unwrap(), flate2::Compression::fast());
    encoder.write_all(FOUR.as_bytes()).unwrap();
    encoder.finish().unwrap();
    let out = run_path(&[Path::new("-q"), Path::new("import"), &gz]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(dir.path().join("zipped.si4").is_file());
    let db = Si4Database::open(&dir.path().join("zipped")).unwrap();
    assert_eq!(db.len(), 4);
}

#[test]
fn import_refuses_an_existing_database_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    let base = make_base(dir.path(), "again");
    let pgn = dir.path().join("again.pgn");
    let out = run(&["import", arg(&pgn), arg(&base)]);
    assert_eq!(code(&out), 1);
    assert!(
        stderr(&out).contains("database already exists"),
        "{}",
        stderr(&out)
    );

    let small = write_pgn(
        dir.path(),
        "small.pgn",
        &FOUR[..FOUR.find("[Event \"Club night\"]\n[Site \"Rome").unwrap()],
    );
    let out = run(&["-q", "import", "--force", arg(&small), arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(Si4Database::open(&base).unwrap().len(), 1);
}

#[test]
fn import_of_a_missing_file_fails_without_creating_anything() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nothing.pgn");
    let out = run(&["import", arg(&missing)]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cannot open"), "{}", stderr(&out));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn import_of_a_corrupt_pgn_reports_warnings_in_the_err_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut content = String::from("some preface text\n\n");
    content.push_str(FOUR);
    content.push_str("\n[Event \"Broken\"]\n[White \"Zulu\"]\n[Black \"Yankee\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Ke4 Qxx9 1-0\n");
    let pgn = write_pgn(dir.path(), "bad.pgn", &content);
    let base = dir.path().join("bad");
    let out = run(&["import", arg(&pgn), arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let summary = stdout(&out);
    assert!(
        summary.contains("errors or warnings; examine the file"),
        "{summary}"
    );
    let log = fs::read_to_string(dir.path().join("bad.err")).unwrap();
    assert!(log.contains("(game 5"), "{log}");
    // The good games are there.
    assert!(Si4Database::open(&base).unwrap().len() >= 4);
}

#[test]
fn ignore_pregame_drops_the_text_between_games() {
    let dir = tempfile::tempdir().unwrap();
    let content = format!(
        "{FOUR}\nPreface of the last game\n\n[White \"Last\"]\n[Black \"One\"]\n[Result \"*\"]\n\n1. e4 *\n"
    );
    let pgn = write_pgn(dir.path(), "pre.pgn", &content);
    let kept = dir.path().join("kept");
    let dropped = dir.path().join("dropped");
    assert_eq!(code(&run(&["-q", "import", arg(&pgn), arg(&kept)])), 0);
    assert_eq!(
        code(&run(&["-q", "import", "-x", arg(&pgn), arg(&dropped)])),
        0
    );
    let export = |base: &Path| {
        let out = dir.path().join("e.pgn");
        fs::remove_file(&out).ok();
        assert_eq!(code(&run(&["-q", "export", arg(base), arg(&out)])), 0);
        fs::read_to_string(out).unwrap()
    };
    assert!(export(&kept).contains("Preface of the last game"));
    assert!(!export(&dropped).contains("Preface of the last game"));
}

#[test]
fn merge_sums_games_and_keeps_the_deleted_flag() {
    let dir = tempfile::tempdir().unwrap();
    let one = make_base(dir.path(), "one");
    let two = make_base(dir.path(), "two");
    mark_deleted(&two, 2);
    let target = dir.path().join("all");
    let out = run(&["merge", arg(&target), arg(&one), arg(&two)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Successfully merged 2 files, of 8 games."),
        "{}",
        stdout(&out)
    );
    let db = Si4Database::open(&target).unwrap();
    assert_eq!(db.len(), 8);
    let deleted: Vec<u32> = (1..=8)
        .filter(|&n| db.entry(n).unwrap().flags.contains(GameFlags::DELETE))
        .collect();
    assert_eq!(deleted, [6]);
    drop(db);
    assert_eq!(
        exported_whites(dir.path(), &target),
        [
            "Delta", "Alpha", "Charlie", "Bravo", "Delta", "Alpha", "Charlie", "Bravo"
        ]
    );
}

#[test]
fn merge_reads_the_list_of_databases_from_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let one = make_base(dir.path(), "one");
    let two = make_base(dir.path(), "two");
    let target = dir.path().join("piped");
    let mut child = bin()
        .args(["-q", "merge", arg(&target), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let list = format!("{}\r\n\n{}.si4\n", one.display(), two.display());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(list.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(Si4Database::open(&target).unwrap().len(), 8);
}

#[test]
fn merge_refuses_an_existing_target_and_a_missing_source_without_litter() {
    let dir = tempfile::tempdir().unwrap();
    let one = make_base(dir.path(), "one");
    let out = run(&["merge", arg(&one), arg(&one)]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("already exists"), "{}", stderr(&out));

    let target = dir.path().join("new");
    let missing = dir.path().join("ghost");
    let out = run(&["merge", arg(&target), arg(&one), arg(&missing)]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("cannot open"), "{}", stderr(&out));
    assert!(!dir.path().join("new.si4").exists());
}

#[test]
fn compact_names_removes_unused_names() {
    let dir = tempfile::tempdir().unwrap();
    let base = make_base(dir.path(), "names");
    mark_deleted(&base, 3);
    assert_eq!(code(&run(&["-q", "compact", arg(&base)])), 0);
    let out = run(&["compact", "--names", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stdout(&out).contains("successfully compacted"),
        "{}",
        stdout(&out)
    );
    let info = stdout(&run(&["info", arg(&base)]));
    assert!(info.contains("4 players"), "{info}");
    let out = run(&["compact", "--names", arg(&base)]);
    assert!(
        stdout(&out).contains("nothing to compact"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn names_lists_by_prefix_and_describe_changes_the_description() {
    let dir = tempfile::tempdir().unwrap();
    let base = make_base(dir.path(), "meta");
    let out = run(&["names", arg(&base), "player", "B"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "     2\tBravo\n[1 names matched the supplied prefix string]\n"
    );
    let out = run(&["names", arg(&base), "site"]);
    assert!(stdout(&out).contains("     2\tOslo"), "{}", stdout(&out));

    let out = run(&["describe", arg(&base), "My club games"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let info = stdout(&run(&["info", arg(&base)]));
    assert!(info.contains(": My club games"), "{info}");
}

#[test]
fn a_database_open_for_writing_blocks_writers_but_not_readers() {
    let dir = tempfile::tempdir().unwrap();
    let base = make_base(dir.path(), "busy");
    let holder = Si4Writer::open(&base).unwrap();
    for args in [vec!["sort", "date"], vec!["compact"], vec!["describe"]] {
        let mut full: Vec<&str> = vec![args[0]];
        full.extend(&args[1..]);
        full.push(arg(&base));
        if args[0] == "describe" {
            full.push("text");
        }
        let out = run(&full);
        assert_eq!(code(&out), 1, "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("already open for writing"),
            "{}",
            stderr(&out)
        );
    }
    let out = run(&["info", arg(&base)]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    drop(holder);
    assert_eq!(code(&run(&["-q", "sort", "date", arg(&base)])), 0);
}

#[test]
fn damaged_database_files_fail_with_exit_code_one() {
    let dir = tempfile::tempdir().unwrap();
    let base = make_base(dir.path(), "broken");
    let index = dir.path().join("broken.si4");
    let bytes = fs::read(&index).unwrap();
    fs::write(&index, &bytes[..bytes.len() - 10]).unwrap();
    let out = run(&["info", arg(&base)]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).starts_with("error: "), "{}", stderr(&out));
    let missing = dir.path().join("none");
    let out = run(&["info", arg(&missing)]);
    assert_eq!(code(&out), 1);
}

#[test]
fn usage_errors_exit_with_two_and_help_with_zero() {
    assert_eq!(code(&run(&[])), 2);
    assert_eq!(code(&run(&["bogus"])), 2);
    assert_eq!(code(&run(&["sort", "date,nonsense", "x"])), 2);
    assert_eq!(code(&run(&["names", "x", "planet"])), 2);
    let out = run(&["--help"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("import"));
    let out = run(&["--version"]);
    assert_eq!(code(&out), 0);
}

#[test]
fn quiet_hides_progress_and_banners() {
    let dir = tempfile::tempdir().unwrap();
    let pgn = write_pgn(dir.path(), "q.pgn", FOUR);
    let base = dir.path().join("q");
    let out = run(&["-q", "import", arg(&pgn), arg(&base)]);
    assert_eq!(code(&out), 0);
    assert_eq!(stderr(&out), "");
    assert!(
        !stdout(&out).contains("Converting file"),
        "{}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("4 games"));
}
