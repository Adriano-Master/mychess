//! M4-1 contract tests: option model, validation at every boundary and what
//! reaches the engine as `setoption` (D409, D410).

#[path = "m4_1_common/helpers.rs"]
mod common;

use common::*;
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{
    EngineCommand, EngineError, EngineEvent, EngineOption, EngineWarning, OptionError, OptionKind,
    OptionValue, is_managed_option,
};

fn spin(min: i64, max: i64, default: i64) -> EngineOption {
    EngineOption {
        name: "Skill Level".into(),
        kind: OptionKind::Spin { default, min, max },
    }
}

fn text_option(name: &str) -> EngineOption {
    EngineOption {
        name: name.into(),
        kind: OptionKind::String {
            default: String::new(),
        },
    }
}

fn combo() -> EngineOption {
    EngineOption {
        name: "Playing Style".into(),
        kind: OptionKind::Combo {
            default: "Normal".into(),
            vars: vec!["Very Solid".into(), "Normal".into(), "Risky".into()],
        },
    }
}

fn check() -> EngineOption {
    EngineOption {
        name: "UCI_ShowWDL".into(),
        kind: OptionKind::Check { default: false },
    }
}

fn button() -> EngineOption {
    EngineOption {
        name: "Clear Hash".into(),
        kind: OptionKind::Button,
    }
}

// ---------------------------------------------------------------------------
// validate: spin
// ---------------------------------------------------------------------------

#[test]
fn spin_accepts_min_and_max_and_refuses_one_beyond_each() {
    let o = spin(0, 20, 20);
    assert_eq!(o.validate(&OptionValue::Int(0)), Ok(OptionValue::Int(0)));
    assert_eq!(o.validate(&OptionValue::Int(20)), Ok(OptionValue::Int(20)));
    assert_eq!(o.validate(&OptionValue::Int(10)), Ok(OptionValue::Int(10)));
    for bad in [-1, 21, i64::MIN, i64::MAX] {
        assert!(
            matches!(
                o.validate(&OptionValue::Int(bad)),
                Err(OptionError::OutOfRange { value, min: 0, max: 20, .. }) if value == bad
            ),
            "{bad}"
        );
    }
}

#[test]
fn spin_with_the_full_i64_range_accepts_the_extremes() {
    let o = spin(i64::MIN, i64::MAX, 0);
    assert!(o.validate(&OptionValue::Int(i64::MIN)).is_ok());
    assert!(o.validate(&OptionValue::Int(i64::MAX)).is_ok());
}

#[test]
fn spin_reads_numeric_text_and_refuses_anything_else() {
    let o = spin(0, 100, 50);
    assert_eq!(
        o.validate(&OptionValue::Text(" 42 ".into())),
        Ok(OptionValue::Int(42))
    );
    assert_eq!(
        o.validate(&OptionValue::Text("-0".into())),
        Ok(OptionValue::Int(0))
    );
    for bad in [
        "",
        "4 2",
        "4.5",
        "1e2",
        "ten",
        "0x10",
        "99999999999999999999",
    ] {
        assert!(
            matches!(
                o.validate(&OptionValue::Text(bad.into())),
                Err(OptionError::WrongType { .. })
            ),
            "{bad:?}"
        );
    }
    assert!(matches!(
        o.validate(&OptionValue::Bool(true)),
        Err(OptionError::WrongType { .. })
    ));
    assert!(matches!(
        o.validate(&OptionValue::Text("101".into())),
        Err(OptionError::OutOfRange { .. })
    ));
}

// ---------------------------------------------------------------------------
// validate: combo, check, string, button
// ---------------------------------------------------------------------------

#[test]
fn combo_accepts_only_listed_values_exactly_and_keeps_words_with_spaces() {
    let o = combo();
    assert_eq!(
        o.validate(&OptionValue::Text("Very Solid".into())),
        Ok(OptionValue::Text("Very Solid".into()))
    );
    for bad in ["Wild", "", "very solid", "Very  Solid", "Very", "Risky "] {
        assert!(
            matches!(
                o.validate(&OptionValue::Text(bad.into())),
                Err(OptionError::NotInList { .. })
            ),
            "{bad:?}"
        );
    }
    assert!(matches!(
        o.validate(&OptionValue::Int(1)),
        Err(OptionError::WrongType { .. })
    ));
    assert!(matches!(
        o.validate(&OptionValue::Bool(true)),
        Err(OptionError::WrongType { .. })
    ));
}

#[test]
fn combo_refuses_line_breaks_even_if_a_var_were_to_contain_them() {
    let o = EngineOption {
        name: "Bad".into(),
        kind: OptionKind::Combo {
            default: "a".into(),
            vars: vec!["a\nquit".into()],
        },
    };
    assert!(matches!(
        o.validate(&OptionValue::Text("a\nquit".into())),
        Err(OptionError::ControlCharacter { .. })
    ));
}

#[test]
fn check_accepts_booleans_and_the_words_true_and_false_only() {
    let o = check();
    assert_eq!(
        o.validate(&OptionValue::Bool(true)),
        Ok(OptionValue::Bool(true))
    );
    assert_eq!(
        o.validate(&OptionValue::Bool(false)),
        Ok(OptionValue::Bool(false))
    );
    assert_eq!(
        o.validate(&OptionValue::Text("true".into())),
        Ok(OptionValue::Bool(true))
    );
    assert_eq!(
        o.validate(&OptionValue::Text("False".into())),
        Ok(OptionValue::Bool(false))
    );
    for bad in ["yes", "1", "0", "", "truee", "on"] {
        assert!(
            matches!(
                o.validate(&OptionValue::Text(bad.into())),
                Err(OptionError::WrongType { .. })
            ),
            "{bad:?}"
        );
    }
    assert!(matches!(
        o.validate(&OptionValue::Int(1)),
        Err(OptionError::WrongType { .. })
    ));
}

#[test]
fn string_refuses_line_breaks_nul_and_control_characters() {
    let o = text_option("SyzygyPath");
    for bad in [
        "a\nquit",
        "a\rquit",
        "a\r\nquit",
        "a\0b",
        "\n",
        "x\u{1b}[31m",
        "setoption\u{85}name",
    ] {
        assert!(
            matches!(
                o.validate(&OptionValue::Text(bad.into())),
                Err(OptionError::ControlCharacter { .. })
            ),
            "{bad:?}"
        );
    }
    for good in [
        "",
        "/tmp/tb",
        "C:\\tb;D:\\tb",
        "caf\u{e9} \u{4e2d}\u{6587}",
        "  padded  ",
    ] {
        assert_eq!(
            o.validate(&OptionValue::Text(good.into())),
            Ok(OptionValue::Text(good.into())),
            "{good:?}"
        );
    }
    assert!(matches!(
        o.validate(&OptionValue::Int(1)),
        Err(OptionError::WrongType { .. })
    ));
}

#[test]
fn button_accepts_any_value_and_ignores_it() {
    let o = button();
    for v in [
        OptionValue::Bool(false),
        OptionValue::Int(7),
        OptionValue::Text("junk".into()),
    ] {
        assert!(o.validate(&v).is_ok(), "{v:?}");
    }
}

#[test]
fn managed_options_are_recognised_without_regard_to_case() {
    for name in [
        "UCI_Chess960",
        "uci_chess960",
        "UCI_AnalyseMode",
        "UCI_Opponent",
        "UCI_ShowCurrLine",
        "UCI_ShowRefutations",
        "Ponder",
        "PONDER",
    ] {
        assert!(is_managed_option(name), "{name}");
    }
    for name in [
        "Hash",
        "Threads",
        "MultiPV",
        "UCI_Elo",
        "UCI_LimitStrength",
        "UCI_ShowWDL",
        "Ponder Time",
        "",
    ] {
        assert!(!is_managed_option(name), "{name}");
    }
    assert!(
        EngineOption {
            name: "Ponder".into(),
            kind: OptionKind::Check { default: false }
        }
        .is_managed()
    );
}

// ---------------------------------------------------------------------------
// what reaches the engine
// ---------------------------------------------------------------------------

async fn started() -> (
    ScriptedEngine,
    mychess_engine::Engine,
    mychess_engine::EngineEvents,
) {
    let fake = ScriptedEngine::builder().build();
    let (engine, events) = fake.start(config()).await.expect("start");
    (fake, engine, events)
}

#[tokio::test(start_paused = true)]
async fn setoption_lines_have_the_standard_shape() {
    let (fake, engine, mut events) = started().await;
    let cases = [
        (
            "Hash",
            OptionValue::Int(256),
            "setoption name Hash value 256",
        ),
        (
            "Skill Level",
            OptionValue::Int(0),
            "setoption name Skill Level value 0",
        ),
        (
            "Skill Level",
            OptionValue::Text("7".into()),
            "setoption name Skill Level value 7",
        ),
        (
            "UCI_ShowWDL",
            OptionValue::Bool(true),
            "setoption name UCI_ShowWDL value true",
        ),
        (
            "UCI_ShowWDL",
            OptionValue::Text("false".into()),
            "setoption name UCI_ShowWDL value false",
        ),
        (
            "Style",
            OptionValue::Text("Risky".into()),
            "setoption name Style value Risky",
        ),
        (
            "SyzygyPath",
            OptionValue::Text(String::new()),
            "setoption name SyzygyPath value <empty>",
        ),
        (
            "SyzygyPath",
            OptionValue::Text("/a b/c".into()),
            "setoption name SyzygyPath value /a b/c",
        ),
        (
            "Clear Hash",
            OptionValue::Bool(true),
            "setoption name Clear Hash",
        ),
        (
            "Clear Hash",
            OptionValue::Int(5),
            "setoption name Clear Hash",
        ),
    ];
    for (name, value, line) in cases {
        let before = fake.received().len();
        engine
            .send(EngineCommand::SetOption {
                name: name.into(),
                value,
            })
            .unwrap();
        settle().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let sent = fake.received();
        assert_eq!(sent[before], line);
        // One `isready` after each applied option (the barrier).
        assert_eq!(sent[before + 1], "isready", "{line}");
    }
    assert!(
        drain(&mut events)
            .iter()
            .all(|e| !matches!(e, EngineEvent::Warning(_)))
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn the_name_is_matched_ignoring_case_and_sent_as_the_engine_spelled_it() {
    let (fake, engine, _events) = started().await;
    engine
        .send(EngineCommand::SetOption {
            name: "skill level".into(),
            value: OptionValue::Int(3),
        })
        .unwrap();
    settle().await;
    assert!(
        fake.received()
            .contains(&"setoption name Skill Level value 3".to_owned()),
        "{:?}",
        fake.received()
    );
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn refused_values_never_reach_the_engine() {
    let (fake, engine, _events) = started().await;
    let before = fake.received();
    let bad: Vec<(&str, OptionValue)> = vec![
        ("Hash", OptionValue::Int(0)),
        ("Hash", OptionValue::Int(1025)),
        ("Hash", OptionValue::Text("lots".into())),
        ("Style", OptionValue::Text("Wild".into())),
        ("Style", OptionValue::Text("Solid\nquit".into())),
        ("SyzygyPath", OptionValue::Text("x\nquit".into())),
        ("SyzygyPath", OptionValue::Text("x\ry".into())),
        ("SyzygyPath", OptionValue::Text("x\0y".into())),
        ("UCI_ShowWDL", OptionValue::Text("maybe".into())),
        ("NoSuchOption", OptionValue::Int(1)),
        ("Hash value 3 name Threads", OptionValue::Int(1)),
        ("MultiPV", OptionValue::Int(257)),
        ("MultiPV", OptionValue::Int(0)),
    ];
    for (name, value) in bad {
        let result = engine.send(EngineCommand::SetOption {
            name: name.into(),
            value: value.clone(),
        });
        assert!(
            matches!(result, Err(EngineError::InvalidOption(_))),
            "{name} = {value:?}: {result:?}"
        );
    }
    settle().await;
    assert_eq!(fake.received(), before);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn managed_options_are_refused_through_setoption() {
    let (fake, engine, _events) = started().await;
    let before = fake.received();
    for name in ["Ponder", "UCI_Chess960", "UCI_AnalyseMode", "uci_opponent"] {
        let result = engine.send(EngineCommand::SetOption {
            name: name.into(),
            value: OptionValue::Bool(true),
        });
        assert!(
            matches!(
                result,
                Err(EngineError::InvalidOption(OptionError::Managed(_)))
            ),
            "{name}: {result:?}"
        );
    }
    settle().await;
    assert_eq!(fake.received(), before);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn the_error_names_the_option_and_the_limits() {
    let (_fake, engine, _events) = started().await;
    let err = engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(5000),
        })
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("Hash") && text.contains("5000"), "{text}");
    assert!(text.contains("1..=1024"), "{text}");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn no_option_value_with_a_line_break_ever_reaches_the_engine_not_even_saved_ones() {
    let mut cfg = config();
    cfg.options = vec![
        ("SyzygyPath".into(), OptionValue::Text("a\nquit".into())),
        ("Style".into(), OptionValue::Text("Solid\r\nquit".into())),
    ];
    let fake = ScriptedEngine::builder().build();
    let (engine, mut events) = fake.start(cfg).await.unwrap();
    let sent = fake.received();
    assert!(
        sent.iter()
            .all(|l| l != "quit" && !l.contains("SyzygyPath")),
        "{sent:?}"
    );
    let warnings = drain(&mut events)
        .into_iter()
        .filter(|e| matches!(e, EngineEvent::Warning(EngineWarning::OptionSkipped { .. })))
        .count();
    assert_eq!(warnings, 2);
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn saved_options_work_with_multi_word_names_and_buttons() {
    let mut cfg = config();
    cfg.options = vec![
        ("Skill Level".into(), OptionValue::Int(5)),
        ("Clear Hash".into(), OptionValue::Bool(true)),
    ];
    let fake = ScriptedEngine::builder().build();
    let (engine, _events) = fake.start(cfg).await.unwrap();
    let sent = fake.received();
    assert!(sent.contains(&"setoption name Skill Level value 5".to_owned()));
    assert!(sent.contains(&"setoption name Clear Hash".to_owned()));
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn setoption_during_a_search_stops_it_and_restarts_it() {
    let (fake, engine, mut events) = started().await;
    engine.send(EngineCommand::Search(infinite(KPK))).unwrap();
    let a = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    next_info(&mut events).await;
    engine
        .send(EngineCommand::SetOption {
            name: "Hash".into(),
            value: OptionValue::Int(32),
        })
        .unwrap();
    // Old search ends, a new one with a greater id starts.
    assert_eq!(next_best(&mut events).await.search, a);
    let b = until(&mut events, |e| match e {
        EngineEvent::SearchStarted(id) => Some(id),
        _ => None,
    })
    .await;
    assert!(b > a);
    let sent = fake.received();
    let stop = sent.iter().rposition(|l| l == "stop").unwrap();
    let set = sent
        .iter()
        .rposition(|l| l == "setoption name Hash value 32")
        .unwrap();
    let go = sent.iter().rposition(|l| l.starts_with("go")).unwrap();
    assert!(stop < set && set < go, "{sent:?}");
    engine.shutdown().await;
}
