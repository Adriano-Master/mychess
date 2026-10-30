//! M8-7 contract tests of the fake engine: the options of strength it
//! declares follow the flags (spec M8-7, D2452). The tactical game plans the
//! strength of an engine from exactly these lines.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

const FAKE: &str = env!("CARGO_BIN_EXE_mychess-fake-engine");

/// The `option name ...` lines the binary prints for `uci`.
fn declared(args: &[&str]) -> Vec<String> {
    let mut child = Command::new(FAKE)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the fake engine");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");
    stdin.write_all(b"uci\nquit\n").expect("write");
    drop(stdin);
    let mut text = String::new();
    stdout.read_to_string(&mut text).expect("read");
    child.wait().expect("wait");
    text.lines()
        .filter(|l| l.starts_with("option name "))
        .map(|l| l.replace('\r', ""))
        .collect()
}

fn has(lines: &[String], name: &str) -> bool {
    let prefix = format!("option name {name} ");
    lines.iter().any(|l| l.starts_with(&prefix))
}

#[test]
fn by_default_it_declares_every_option_of_strength() {
    let lines = declared(&[]);
    assert!(has(&lines, "UCI_LimitStrength"));
    assert!(has(&lines, "UCI_Elo"));
    assert!(has(&lines, "Skill Level"));
    assert!(
        lines
            .iter()
            .any(|l| l == "option name UCI_Elo type spin default 1500 min 1320 max 3190")
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "option name Skill Level type spin default 20 min 0 max 20")
    );
}

#[test]
fn without_limit_strength_only_skill_level_is_left() {
    let lines = declared(&["--no-limit-strength"]);
    assert!(!has(&lines, "UCI_LimitStrength"));
    assert!(!has(&lines, "UCI_Elo"));
    assert!(has(&lines, "Skill Level"));
    assert!(has(&lines, "Hash"), "the other options stay");
}

#[test]
fn with_no_strength_none_of_the_three_is_declared() {
    let lines = declared(&["--no-strength"]);
    assert!(!has(&lines, "UCI_LimitStrength"));
    assert!(!has(&lines, "UCI_Elo"));
    assert!(!has(&lines, "Skill Level"));
    assert!(has(&lines, "Ponder"));
    assert!(has(&lines, "Clear Hash"));
}

#[test]
fn the_declaration_is_deterministic() {
    assert_eq!(declared(&["--no-strength"]), declared(&["--no-strength"]));
}
