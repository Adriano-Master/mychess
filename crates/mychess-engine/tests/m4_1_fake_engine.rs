//! M4-1 contract tests of the fake engine itself: every flag of the
//! `mychess-fake-engine` binary does what the spec says, the output is
//! deterministic, and the in-memory `run_fake` speaks exactly like the binary.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mychess_engine::testing::{FakeBehavior, run_fake};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FAKE: &str = env!("CARGO_BIN_EXE_mychess-fake-engine");

struct Run {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
}

impl Run {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).replace('\r', "")
    }
    fn lines(&self) -> Vec<String> {
        self.text().lines().map(str::to_owned).collect()
    }
}

/// Feeds `input` to the binary, closes its stdin and collects everything.
fn run(args: &[&str], input: &str) -> Run {
    let mut child = Command::new(FAKE)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("io");
    let mut stdin = child.stdin.take().expect("io");
    let mut stdout = child.stdout.take().expect("io");
    let mut stderr = child.stderr.take().expect("io");
    let out = std::thread::spawn(move || {
        let mut v = Vec::new();
        stdout.read_to_end(&mut v).ok();
        v
    });
    let err = std::thread::spawn(move || {
        let mut v = Vec::new();
        stderr.read_to_end(&mut v).ok();
        v
    });
    stdin.write_all(input.as_bytes()).ok();
    drop(stdin);
    let began = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().expect("io") {
            break status.code();
        }
        if began.elapsed() > Duration::from_secs(10) {
            child.kill().ok();
            child.wait().ok();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Run {
        stdout: out.join().expect("io"),
        stderr: err.join().expect("io"),
        code,
    }
}

fn analyse(args: &[&str], fen: &str, go: &str, setup: &str) -> Run {
    run(
        args,
        &format!("uci\n{setup}isready\nposition fen {fen}\n{go}\nquit\n"),
    )
}

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[test]
fn announces_the_options_of_the_spec() {
    let out = run(&[], "uci\nquit\n").lines();
    assert_eq!(out.first().map(String::as_str), Some("id name FakeFish"));
    for opt in [
        "option name Hash type spin default 16 min 1 max 1024",
        "option name Threads type spin default 1 min 1 max 64",
        "option name MultiPV type spin default 1 min 1 max 256",
        "option name Ponder type check default false",
        "option name UCI_LimitStrength type check default false",
        "option name UCI_Elo type spin default 1500 min 1320 max 3190",
        "option name UCI_ShowWDL type check default false",
        "option name UCI_Chess960 type check default false",
        "option name UCI_AnalyseMode type check default false",
        "option name Style type combo default Normal var Solid var Normal var Risky",
        "option name Skill Level type spin default 20 min 0 max 20",
        "option name SyzygyPath type string default <empty>",
        "option name Clear Hash type button",
    ] {
        assert!(out.iter().any(|l| l == opt), "missing: {opt}");
    }
    assert_eq!(out.last().map(String::as_str), Some("uciok"));
}

#[test]
fn name_no_multipv_and_chess960_flags_change_the_announcement() {
    let out = run(&["--name", "Other One"], "uci\nquit\n").lines();
    assert!(out.iter().any(|l| l == "id name Other One"));
    let out = run(&["--no-multipv"], "uci\nquit\n").text();
    assert!(!out.contains("MultiPV"));
    let out = run(&["--no-chess960"], "uci\nquit\n").text();
    assert!(!out.contains("UCI_Chess960"));
    let out = run(&["--chess960"], "uci\nquit\n").text();
    assert!(out.contains("UCI_Chess960"));
}

#[test]
fn crlf_ends_every_line_with_carriage_return_and_newline() {
    let r = run(&["--crlf"], "uci\nisready\nquit\n");
    let text = String::from_utf8_lossy(&r.stdout).into_owned();
    assert!(text.ends_with("readyok\r\n"));
    assert!(
        !text.replace("\r\n", "").contains('\n'),
        "a bare newline slipped in"
    );
}

#[test]
fn unknown_flags_and_missing_values_exit_with_code_two() {
    for args in [&["--bogus"][..], &["--name"], &["--exit-code", "x"]] {
        let r = run(args, "");
        assert_eq!(r.code, Some(2), "{args:?}");
        assert_ne!(r.stderr.len(), 0);
    }
}

#[test]
fn exit_code_is_used_on_quit_and_on_end_of_input() {
    assert_eq!(run(&["--exit-code", "5"], "quit\n").code, Some(5));
    assert_eq!(run(&["--exit-code", "9"], "").code, Some(9));
    assert_eq!(run(&[], "quit\n").code, Some(0));
}

#[test]
fn crash_after_go_exits_one_by_default_with_a_message_and_no_bestmove() {
    let r = analyse(&["--crash-after-go"], START, "go depth 3", "");
    assert_eq!(r.code, Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("crashing on purpose"));
    assert!(!r.text().contains("bestmove"));
    let r = analyse(
        &["--crash-after-go", "--exit-code", "7"],
        START,
        "go depth 3",
        "",
    );
    assert_eq!(r.code, Some(7));
}

#[test]
fn silent_never_answers_uci() {
    let mut child = Command::new(FAKE)
        .arg("--silent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "uci").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "still alive");
    child.kill().unwrap();
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert!(out.is_empty(), "{out:?}");
    child.wait().unwrap();
}

#[test]
fn uciok_delay_postpones_uciok() {
    let began = Instant::now();
    let r = run(&["--uciok-delay-ms", "300"], "uci\nquit\n");
    assert!(r.text().contains("uciok"));
    assert!(
        began.elapsed() >= Duration::from_millis(280),
        "{:?}",
        began.elapsed()
    );
}

#[test]
fn ignore_quit_survives_quit_and_end_of_input() {
    let mut child = Command::new(FAKE)
        .arg("--ignore-quit")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "quit").unwrap();
    drop(stdin);
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "must ignore quit");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn stderr_flood_writes_far_more_than_a_pipe_holds() {
    let mut child = Command::new(FAKE)
        .arg("--stderr-flood")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let err = std::thread::spawn(move || {
        let mut v = Vec::new();
        stderr.read_to_end(&mut v).ok();
        v
    });
    writeln!(stdin, "uci").unwrap();
    stdin.flush().unwrap();
    // Give the flood time to finish before the engine quits.
    std::thread::sleep(Duration::from_millis(1500));
    writeln!(stdin, "quit").unwrap();
    drop(stdin);
    let mut out = String::new();
    stdout.read_to_string(&mut out).unwrap();
    child.wait().unwrap();
    assert!(err.join().unwrap().len() > 65_536);
    assert!(out.contains("uciok"), "the flood must not block stdout");
}

#[test]
fn garbage_mixes_junk_bad_bytes_and_a_two_mebibyte_line() {
    let r = analyse(&["--garbage"], START, "go depth 2", "");
    assert!(r.text().contains("this is not a UCI line"));
    assert!(
        r.stdout.windows(2).any(|w| w == [0xff, 0xfe]),
        "non UTF-8 bytes"
    );
    assert!(r.stdout.len() > 2 * 1024 * 1024);
    assert!(r.text().contains("bestmove"));
}

#[test]
fn depth_search_is_deterministic_and_follows_the_greedy_rules() {
    let a = analyse(&[], START, "go depth 3", "").lines();
    let b = analyse(&[], START, "go depth 3", "").lines();
    assert_eq!(a, b, "same input, same output");
    let infos: Vec<_> = a.iter().filter(|l| l.starts_with("info depth")).collect();
    assert_eq!(infos.len(), 3, "one report per depth 1..=3");
    for (i, l) in infos.iter().enumerate() {
        assert!(l.starts_with(&format!("info depth {} ", i + 1)), "{l}");
        assert!(l.contains("score cp 0"), "{l}");
    }
    // All moves tie at 0: the smallest UCI text wins; replies are greedy.
    assert!(infos[0].ends_with("pv a2a3"), "{}", infos[0]);
    assert!(infos[1].ends_with("pv a2a3 a7a5"), "{}", infos[1]);
    assert!(infos[2].ends_with("pv a2a3 a7a5 a1a2"), "{}", infos[2]);
    assert_eq!(a.last().map(String::as_str), Some("bestmove a2a3"));
}

#[test]
fn the_pv_is_capped_at_three_moves() {
    let l = analyse(&[], START, "go depth 9", "").lines();
    let last_info = l.iter().rfind(|l| l.starts_with("info depth")).unwrap();
    let pv = last_info.split(" pv ").nth(1).unwrap();
    assert_eq!(pv.split_whitespace().count(), 3);
}

#[test]
fn captures_rank_first_and_mate_in_one_is_announced() {
    let r = analyse(&[], "4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1", "go depth 1", "").lines();
    assert!(
        r.iter()
            .any(|l| l.contains("score cp 900") && l.ends_with("pv d1d5")),
        "{r:?}"
    );
    assert_eq!(r.last().unwrap(), "bestmove d1d5");
    let r = analyse(&[], "6k1/5ppp/8/8/8/8/8/R3K3 w - - 0 1", "go depth 1", "").lines();
    assert!(r.iter().any(|l| l.contains("score mate 1")), "{r:?}");
    assert_eq!(r.last().unwrap(), "bestmove a1a8");
}

#[test]
fn positions_without_moves_answer_mate_zero_or_zero_centipawns() {
    let mate = analyse(
        &[],
        "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
        "go depth 3",
        "",
    )
    .lines();
    assert!(
        mate.iter().any(|l| l == "info depth 0 score mate 0"),
        "{mate:?}"
    );
    assert_eq!(mate.last().unwrap(), "bestmove (none)");
    let stale = analyse(&[], "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", "go infinite", "").lines();
    assert!(
        stale.iter().any(|l| l == "info depth 0 score cp 0"),
        "{stale:?}"
    );
    assert_eq!(stale.last().unwrap(), "bestmove (none)");
}

#[test]
fn searchmoves_restricts_the_root_moves() {
    let r = analyse(&[], START, "go depth 2 searchmoves e2e4 d2d4", "").lines();
    assert_eq!(r.last().unwrap(), "bestmove d2d4");
    let r = analyse(&[], START, "go depth 2 searchmoves h2h4", "").lines();
    assert_eq!(r.last().unwrap(), "bestmove h2h4");
}

#[test]
fn multipv_emits_k_lines_per_depth_in_order() {
    let setup = "setoption name MultiPV value 3\n";
    let r = analyse(&[], START, "go depth 2", setup).lines();
    let infos: Vec<_> = r.iter().filter(|l| l.starts_with("info depth")).collect();
    assert_eq!(infos.len(), 6);
    for (i, l) in infos.iter().enumerate() {
        assert!(l.contains(&format!(" multipv {} ", i % 3 + 1)), "{l}");
    }
}

#[test]
fn show_wdl_adds_a_thousand_sum_wdl() {
    let setup = "setoption name UCI_ShowWDL value true\n";
    let r = analyse(&[], START, "go depth 1", setup).lines();
    let l = r.iter().find(|l| l.starts_with("info depth 1")).unwrap();
    let wdl: Vec<u32> = l
        .split(" wdl ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .take(3)
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(wdl.iter().sum::<u32>(), 1000);
    let plain = analyse(&[], START, "go depth 1", "").text();
    assert!(!plain.contains(" wdl "));
}

#[test]
fn ponder_option_adds_a_ponder_move_to_bestmove() {
    let on = analyse(
        &[],
        START,
        "go depth 2",
        "setoption name Ponder value true\n",
    )
    .lines();
    assert!(on.last().unwrap().contains(" ponder "), "{on:?}");
    let off = analyse(&[], START, "go depth 2", "").lines();
    assert!(!off.last().unwrap().contains("ponder"));
}

#[test]
fn movetime_ends_by_itself_and_infinite_needs_a_stop() {
    let mut child = Command::new(FAKE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "uci\nposition startpos\ngo movetime 100").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(600));
    writeln!(stdin, "quit").unwrap();
    drop(stdin);
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    child.wait().unwrap();
    assert!(
        out.lines()
            .last()
            .is_some_and(|l| l.starts_with("bestmove ")),
        "{out}"
    );

    // `go infinite` then `stop`: no bestmove before the stop.
    let mut child = Command::new(FAKE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "uci\nisready\nposition startpos\ngo infinite").unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    writeln!(stdin, "stop\nquit").unwrap();
    drop(stdin);
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    child.wait().unwrap();
    let lines: Vec<_> = out.lines().collect();
    let best = lines
        .iter()
        .position(|l| l.starts_with("bestmove"))
        .unwrap();
    assert_eq!(best, lines.len() - 1);
    assert!(
        lines[..best]
            .iter()
            .filter(|l| l.starts_with("info depth"))
            .count()
            >= 2
    );
}

#[test]
fn hang_on_stop_and_no_bestmove_after_stop_never_print_bestmove() {
    for flag in ["--hang-on-stop", "--no-bestmove-after-stop"] {
        let mut child = Command::new(FAKE)
            .arg(flag)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, "uci\nisready\nposition startpos\ngo infinite").unwrap();
        stdin.flush().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        writeln!(stdin, "stop\nisready\nquit").unwrap();
        drop(stdin);
        let mut out = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        child.wait().unwrap();
        assert!(!out.contains("bestmove"), "{flag}");
        assert!(
            out.lines().last() == Some("readyok") || out.contains("readyok"),
            "{flag}"
        );
    }
}

#[test]
fn ponderhit_ends_a_ponder_search_at_once() {
    let mut child = Command::new(FAKE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "uci\nposition startpos\ngo ponder\nponderhit\nquit").unwrap();
    drop(stdin);
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    child.wait().unwrap();
    assert!(out.lines().any(|l| l.starts_with("bestmove ")), "{out}");
}

#[tokio::test]
async fn the_in_memory_fake_speaks_exactly_like_the_binary() {
    let input = format!(
        "uci\nsetoption name MultiPV value 2\nisready\nposition fen {START}\ngo depth 3\nquit\n"
    );
    let from_binary = run(&[], &input).stdout;
    let (engine_side, test_side) = tokio::io::duplex(1 << 20);
    let (er, ew) = tokio::io::split(engine_side);
    let task = tokio::spawn(run_fake(er, ew, FakeBehavior::default(), None));
    let (mut tr, mut tw) = tokio::io::split(test_side);
    tw.write_all(input.as_bytes()).await.unwrap();
    let mut from_memory = Vec::new();
    tr.read_to_end(&mut from_memory).await.unwrap();
    task.await.unwrap();
    assert_eq!(
        String::from_utf8_lossy(&from_memory),
        String::from_utf8_lossy(&from_binary)
    );
}

#[test]
fn from_args_parses_the_documented_flags() {
    let args = [
        "--name",
        "X",
        "--silent",
        "--uciok-delay-ms",
        "5",
        "--hang-on-stop",
        "--no-bestmove-after-stop",
        "--ignore-quit",
        "--crash-after-go",
        "--garbage",
        "--no-multipv",
        "--no-chess960",
        "--crlf",
        "--stderr-flood",
        "--exit-code",
        "3",
    ];
    let (b, flood) = FakeBehavior::from_args(args.iter().map(|s| (*s).to_owned())).unwrap();
    assert!(flood);
    assert_eq!(b.name, "X");
    assert!(b.silent && b.hang_on_stop && b.no_bestmove_after_stop && b.ignore_quit);
    assert!(b.crash_after_go && b.garbage && b.crlf);
    assert!(!b.multipv && !b.chess960);
    assert_eq!(b.uciok_delay, Duration::from_millis(5));
    assert_eq!(b.exit_code, 3);
    assert!(FakeBehavior::from_args(["--nope".to_owned()]).is_err());
}
