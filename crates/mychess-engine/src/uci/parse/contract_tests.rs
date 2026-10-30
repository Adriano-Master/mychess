//! Contract tests of the UCI parser (spec m4-1 rules 2 to 6, D411), derived
//! from the specification and the UCI protocol, not from the implementation.

use super::*;
use crate::wire::decode_line;

fn info(line: &str) -> RawInfo {
    match parse_line(line) {
        UciLine::Info(i) => i,
        other => panic!("not info: {line} -> {other:?}"),
    }
}

fn option(line: &str) -> Result<EngineOption, String> {
    match parse_line(line) {
        UciLine::Option(o) => o,
        other => panic!("not an option: {line} -> {other:?}"),
    }
}

#[test]
fn every_info_field_is_read_in_any_order() {
    let fields = [
        ("depth 18", "depth"),
        ("seldepth 30", "seldepth"),
        ("multipv 2", "multipv"),
        ("score cp -12", "score"),
        ("nodes 5000000", "nodes"),
        ("nps 2500000", "nps"),
        ("hashfull 333", "hashfull"),
        ("tbhits 7", "tbhits"),
        ("cpuload 640", "cpuload"),
        ("time 4242", "time"),
        ("currmove e2e4", "currmove"),
        ("currmovenumber 5", "currmovenumber"),
        ("wdl 10 20 970", "wdl"),
    ];
    // Forward, backward and rotated orders must give the same result.
    let build = |order: Vec<usize>| {
        let mut line = String::from("info");
        for i in order {
            line.push(' ');
            line.push_str(fields[i].0);
        }
        line.push_str(" pv e2e4 e7e5");
        info(&line)
    };
    let n = fields.len();
    let forward = build((0..n).collect());
    for rot in 0..n {
        let order = (0..n).map(|i| (i + rot) % n).collect();
        assert_eq!(build(order), forward, "rotation {rot}");
    }
    assert_eq!(build((0..n).rev().collect()), forward);
    assert_eq!(forward.depth, Some(18));
    assert_eq!(forward.seldepth, Some(30));
    assert_eq!(forward.multipv, Some(2));
    assert_eq!(
        forward.score,
        Some((RawScoreKind::Cp(-12), RawBound::Exact))
    );
    assert_eq!(forward.nodes, Some(5_000_000));
    assert_eq!(forward.nps, Some(2_500_000));
    assert_eq!(forward.hashfull, Some(333));
    assert_eq!(forward.tbhits, Some(7));
    assert_eq!(forward.cpuload, Some(640));
    assert_eq!(forward.time_ms, Some(4242));
    assert_eq!(forward.currmove.as_deref(), Some("e2e4"));
    assert_eq!(forward.currmovenumber, Some(5));
    assert_eq!(forward.wdl, Some((10, 20, 970)));
    assert_eq!(forward.pv, ["e2e4", "e7e5"]);
}

#[test]
fn real_engine_lines_from_several_engines() {
    // Written by hand in the style of Stockfish, Komodo, Lc0, Ethereal, Crafty.
    let cases: [(&str, Option<i32>, usize); 5] = [
        (
            "info depth 24 seldepth 33 multipv 1 score cp 21 nodes 45366733 nps 1520000 hashfull 412 tbhits 0 time 29850 pv e2e4 e7e5 g1f3 b8c6 f1b5",
            Some(21),
            5,
        ),
        (
            "info depth 14 seldepth 22 time 2012 nodes 3004019 nps 1493000 score cp 33 pv d2d4 g8f6 c2c4",
            Some(33),
            3,
        ),
        (
            "info depth 3 seldepth 3 multipv 1 score cp 12 nodes 134 nps 134000 tbhits 0 time 1 pv e2e4 e7e5 g1f3",
            Some(12),
            3,
        ),
        (
            "info depth 11 seldepth 17 score cp 8 time 120 nodes 91234 nps 760000 hashfull 3 tbhits 0 pv g1f3 d7d5",
            Some(8),
            2,
        ),
        ("info depth 9 score cp 0 time 17 nodes 1234 pv", Some(0), 0),
    ];
    for (line, cp, pv) in cases {
        let i = info(line);
        assert_eq!(i.score.map(|s| s.0), cp.map(RawScoreKind::Cp), "{line}");
        assert_eq!(i.pv.len(), pv, "{line}");
    }
}

#[test]
fn score_forms() {
    let mate0 = info("info depth 1 score mate 0");
    assert_eq!(mate0.score, Some((RawScoreKind::Mate(0), RawBound::Exact)));
    let m = info("info score mate -3 lowerbound");
    assert_eq!(m.score, Some((RawScoreKind::Mate(-3), RawBound::Lower)));
    let u = info("info score cp -34 upperbound nodes 5");
    assert_eq!(u.score, Some((RawScoreKind::Cp(-34), RawBound::Upper)));
    assert_eq!(u.nodes, Some(5));
    for bad in [
        "info score",
        "info score cp",
        "info score cp x",
        "info score foo 12",
        "info score 12",
        "info score cp 2147483648",
        "info score cp -2147483649",
        "info score mate 99999999999",
    ] {
        assert_eq!(info(bad).score, None, "{bad}");
    }
    let extreme = info("info score cp -2147483648");
    assert_eq!(
        extreme.score,
        Some((RawScoreKind::Cp(i32::MIN), RawBound::Exact))
    );
}

#[test]
fn huge_negative_and_non_numeric_numbers_lose_only_their_field() {
    let i = info(
        "info depth 99999999999999999999 seldepth -4 nodes 18446744073709551616 nps -1 \
         time 1.5 tbhits 0x10 hashfull 1001 cpuload 65536 multipv 256 currmovenumber -2 \
         wdl 1 2 x pv e2e4",
    );
    assert_eq!(i.depth, None);
    assert_eq!(i.seldepth, None);
    assert_eq!(i.nodes, None);
    assert_eq!(i.nps, None);
    assert_eq!(i.time_ms, None);
    assert_eq!(i.tbhits, None);
    assert_eq!(i.hashfull, None);
    assert_eq!(i.cpuload, None);
    assert_eq!(i.multipv, None);
    assert_eq!(i.currmovenumber, None);
    assert_eq!(i.wdl, None);
    assert_eq!(i.pv, ["e2e4"], "the rest of the line survives");
    let max = info("info nodes 18446744073709551615 depth 4294967295");
    assert_eq!(max.nodes, Some(u64::MAX));
    assert_eq!(max.depth, Some(u32::MAX));
    assert_eq!(info("info hashfull 1000 cpuload 1000").hashfull, Some(1000));
}

#[test]
fn repeated_fields_keep_the_last_valid_value() {
    let i = info("info depth 5 depth 7 nodes 1 nodes bad score cp 1 score cp 2");
    assert_eq!(i.depth, Some(7));
    assert_eq!(i.nodes, Some(1));
    assert_eq!(i.score, Some((RawScoreKind::Cp(2), RawBound::Exact)));
    let p = info("info pv e2e4 e7e5 depth 3 pv d2d4");
    assert_eq!(p.pv, ["d2d4"]);
}

#[test]
fn pv_stops_at_keywords_and_at_anything_that_is_not_a_move() {
    assert_eq!(info("info pv e2e4 e7e5 nodes 5").pv, ["e2e4", "e7e5"]);
    assert_eq!(info("info pv e2e4 wdl 1 2 3").pv, ["e2e4"]);
    assert_eq!(info("info pv e2e4 hello e7e5").pv, ["e2e4"]);
    assert_eq!(
        info("info pv a7a8q b1c3 0000").pv,
        ["a7a8q", "b1c3", "0000"]
    );
    assert_eq!(
        info("info pv E2E4").pv.len(),
        0,
        "upper case is not a coordinate move"
    );
    assert_eq!(info("info pv i2i4 e2e4").pv.len(), 0);
    assert_eq!(info("info pv e2e9").pv.len(), 0);
    assert_eq!(info("info pv e7e8k").pv.len(), 0);
    assert_eq!(info("info pv").pv.len(), 0);
    assert_eq!(
        info("info multipv 1 pv score cp 4").score.map(|s| s.0),
        Some(RawScoreKind::Cp(4))
    );
}

#[test]
fn string_refutation_and_currline_never_produce_a_pv() {
    let s = info("info string depth 5 score cp 3 pv e2e4");
    assert_eq!(s.string.as_deref(), Some("depth 5 score cp 3 pv e2e4"));
    assert!(s.pv.is_empty() && s.depth.is_none() && s.score.is_none());
    let before = info("info depth 4 string hello");
    assert_eq!(before.depth, Some(4));
    assert_eq!(before.string.as_deref(), Some("hello"));
    assert_eq!(info("info refutation e2e4 d7d5 depth 9").pv.len(), 0);
    assert_eq!(info("info refutation e2e4 d7d5 depth 9").depth, None);
    assert_eq!(info("info currline 1 e2e4 e7e5").pv.len(), 0);
    assert_eq!(info("info string").string.as_deref(), Some(""));
}

#[test]
fn degenerate_info_lines() {
    assert_eq!(info("info"), RawInfo::default());
    assert_eq!(info("info    "), RawInfo::default());
    assert_eq!(info("info ???"), RawInfo::default());
    assert_eq!(parse_line("information depth 3"), UciLine::Unknown);
    assert_eq!(
        parse_line("info\tdepth\t6"),
        UciLine::Info(RawInfo {
            depth: Some(6),
            ..RawInfo::default()
        })
    );
}

#[test]
fn empty_blank_and_crlf_lines() {
    assert_eq!(parse_line(""), UciLine::Unknown);
    assert_eq!(parse_line("     "), UciLine::Unknown);
    assert_eq!(parse_line("\t\t"), UciLine::Unknown);
    assert_eq!(parse_line("uciok\r\n"), UciLine::UciOk);
    assert_eq!(parse_line("readyok\r"), UciLine::ReadyOk);
    assert_eq!(parse_line("uciok extra words"), UciLine::UciOk);
}

#[test]
fn id_lines() {
    assert_eq!(
        parse_line("id name Stockfish 17"),
        UciLine::IdName("Stockfish 17".into())
    );
    assert_eq!(parse_line("id   name   X  "), UciLine::IdName("X".into()));
    assert_eq!(
        parse_line("id author the Stockfish developers (see AUTHORS file)"),
        UciLine::IdAuthor("the Stockfish developers (see AUTHORS file)".into())
    );
    assert_eq!(parse_line("id"), UciLine::Unknown);
    assert_eq!(parse_line("id name"), UciLine::IdName(String::new()));
    assert_eq!(parse_line("id version 3"), UciLine::Unknown);
}

#[test]
fn option_of_every_type_with_defaults_and_limits() {
    let o = option("option name Hash type spin default 16 min 1 max 33554432").unwrap();
    assert_eq!(
        o.kind,
        OptionKind::Spin {
            default: 16,
            min: 1,
            max: 33_554_432
        }
    );
    let o = option("option name Ponder type check default true").unwrap();
    assert_eq!(o.kind, OptionKind::Check { default: true });
    let o = option("option name Ponder type check").unwrap();
    assert_eq!(o.kind, OptionKind::Check { default: false });
    let o = option("option name Clear Hash type button").unwrap();
    assert_eq!(
        (o.name.as_str(), o.kind),
        ("Clear Hash", OptionKind::Button)
    );
    let o = option("option name SyzygyPath type string default <empty>").unwrap();
    assert_eq!(
        o.kind,
        OptionKind::String {
            default: String::new()
        }
    );
    let o = option("option name EvalFile type string default nn-1111cefa1111.nnue").unwrap();
    assert_eq!(
        o.kind,
        OptionKind::String {
            default: "nn-1111cefa1111.nnue".into()
        }
    );
    let o = option("option name UCI_Elo type spin default 1320 min 1320 max 3190").unwrap();
    assert!(matches!(o.kind, OptionKind::Spin { default: 1320, .. }));
    let o = option("option name Neg type spin default -5 min -10 max 10").unwrap();
    assert_eq!(
        o.kind,
        OptionKind::Spin {
            default: -5,
            min: -10,
            max: 10
        }
    );
}

#[test]
fn option_names_defaults_and_vars_with_several_words() {
    let o =
        option("option name Skill Level Of The Moon type spin default 20 min 0 max 20").unwrap();
    assert_eq!(o.name, "Skill Level Of The Moon");
    let o = option("option name Book File type string default my opening book.bin").unwrap();
    assert_eq!(
        o.kind,
        OptionKind::String {
            default: "my opening book.bin".into()
        }
    );
    let o = option(
        "option name Playing Style type combo default Very Solid var Very Solid var Normal Style var Risky",
    )
    .unwrap();
    assert_eq!(
        o.kind,
        OptionKind::Combo {
            default: "Very Solid".into(),
            vars: vec!["Very Solid".into(), "Normal Style".into(), "Risky".into()],
        }
    );
    let o = option("option name   Spaced   Name   type   button").unwrap();
    assert_eq!(o.name, "Spaced Name");
}

#[test]
fn dropped_options_carry_a_reason() {
    for bad in [
        "option name Foo type wibble",
        "option name Foo",
        "option type check",
        "option name type check",
        "option name S type spin default 5 min 10 max 1",
        "option name S type spin default x min 1 max 10",
        "option name S type spin min 1 max 10",
        "option name S type spin default 1 max 10",
        "option name S type spin default 1 min 1",
        "option name S type spin default 1 min 1 max 99999999999999999999",
        "option",
    ] {
        let r = option(bad);
        assert!(r.as_ref().is_err_and(|e| !e.is_empty()), "{bad}: {r:?}");
    }
}

#[test]
fn bestmove_forms() {
    let bm = |l: &str| match parse_line(l) {
        UciLine::BestMove(b) => b,
        other => panic!("{l}: {other:?}"),
    };
    assert_eq!(bm("bestmove e2e4").mv.as_deref(), Some("e2e4"));
    assert_eq!(
        bm("bestmove e2e4 ponder e7e5").ponder.as_deref(),
        Some("e7e5")
    );
    assert_eq!(bm("bestmove e2e4 ponder").ponder, None);
    assert_eq!(bm("bestmove e2e4 ponder zzzz").ponder, None);
    assert_eq!(bm("bestmove (none)").mv, None);
    assert_eq!(bm("bestmove 0000").mv, None);
    assert_eq!(
        bm("bestmove e7e8q ponder a1a2").mv.as_deref(),
        Some("e7e8q")
    );
    // C1: a `bestmove` without a move is a violation, kept as "no move".
    for line in ["bestmove", "bestmove   "] {
        let UciLine::BestMove(raw) = parse_line(line) else {
            panic!("`{line}` must parse as a bestmove")
        };
        assert!(raw.missing && raw.mv.is_none() && raw.ponder.is_none());
    }
    assert!(!bm("bestmove (none)").missing);
    assert_eq!(bm("bestmove garbage").mv.as_deref(), Some("garbage"));
}

#[test]
fn looks_like_move_matches_the_documented_pattern() {
    for ok in ["e2e4", "a7a8q", "h1h8n", "0000", "a1h8r", "b7b8b"] {
        assert!(looks_like_move(ok), "{ok}");
    }
    for bad in [
        "",
        "e2",
        "e2e",
        "e2e4x",
        "e2e44",
        "i2e4",
        "e0e4",
        "e2e9",
        "E2E4",
        "0-0",
        "O-O",
        "e2-e4",
        "e2e4qq",
        "\u{e9}2e4",
    ] {
        assert!(!looks_like_move(bad), "{bad:?}");
    }
}

#[test]
fn a_line_of_one_mebibyte_parses_quickly_and_a_pv_is_capped() {
    let long = format!("info depth 3 pv {}", "e2e4 ".repeat(200_000));
    assert!(long.len() > 1_000_000);
    let began = std::time::Instant::now();
    let i = info(&long);
    assert_eq!(i.depth, Some(3));
    assert_eq!(i.pv.len(), MAX_PV_MOVES);
    let junk = format!("info string {}", "A".repeat(1 << 20));
    assert!(matches!(parse_line(&junk), UciLine::Info(_)));
    let words = format!("option name {} type check", "w ".repeat(400_000));
    drop(parse_line(&words));
    assert!(
        began.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
}

#[test]
fn a_hundred_thousand_lines_in_a_row() {
    let mut moves = 0_usize;
    for i in 0..100_000_u32 {
        let line = match i % 5 {
            0 => format!("info depth {} score cp {} pv e2e4 e7e5", i % 90, i % 300),
            1 => "readyok".to_owned(),
            2 => format!("bestmove e2e4 ponder e7e5 {i}"),
            3 => format!("option name N{i} type spin default 1 min 0 max 5"),
            _ => format!("junk {i} \u{1f600}"),
        };
        if matches!(parse_line(&line), UciLine::Info(_)) {
            moves += 1;
        }
    }
    assert_eq!(moves, 20_000);
}

mod prop {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
            let text = decode_line(&bytes);
            drop(parse_line(&text));
            drop(parse_line(&format!("info {text}")));
            drop(parse_line(&format!("option name {text}")));
            drop(parse_line(&format!("bestmove {text}")));
            drop(parse_line(&format!("id name {text}")));
        }

        #[test]
        fn nearly_info_lines_never_panic_and_respect_the_caps(tokens in proptest::collection::vec(
            prop_oneof![
                Just("depth".to_owned()), Just("seldepth".to_owned()), Just("multipv".to_owned()),
                Just("score".to_owned()), Just("cp".to_owned()), Just("mate".to_owned()),
                Just("lowerbound".to_owned()), Just("upperbound".to_owned()), Just("wdl".to_owned()),
                Just("pv".to_owned()), Just("string".to_owned()), Just("refutation".to_owned()),
                Just("currline".to_owned()), Just("currmove".to_owned()), Just("nodes".to_owned()),
                Just("e2e4".to_owned()), Just("a7a8q".to_owned()), Just("0000".to_owned()),
                Just("-1".to_owned()), Just("0".to_owned()), Just("99999999999999999999".to_owned()),
                "[a-z0-9-]{0,8}",
            ], 0..60))
        {
            let i = info(&format!("info {}", tokens.join(" ")));
            prop_assert!(i.pv.len() <= MAX_PV_MOVES);
            prop_assert!(i.hashfull.is_none_or(|v| v <= 1000));
            prop_assert!(i.multipv.is_none_or(|v| v >= 1));
            prop_assert!(i.pv.iter().all(|m| looks_like_move(m)));
        }

        #[test]
        fn a_well_formed_info_line_round_trips(
            depth in 0_u32..200, cp in -100_000_i32..100_000, nodes in any::<u64>(),
            mpv in 1_u8..=255, bound in 0_u8..3,
        ) {
            let b = ["", " lowerbound", " upperbound"][usize::from(bound)];
            let i = info(&format!("info depth {depth} multipv {mpv} score cp {cp}{b} nodes {nodes} pv e2e4 e7e5"));
            prop_assert_eq!(i.depth, Some(depth));
            prop_assert_eq!(i.multipv, Some(mpv));
            prop_assert_eq!(i.nodes, Some(nodes));
            prop_assert_eq!(i.score.map(|s| s.0), Some(RawScoreKind::Cp(cp)));
            prop_assert_eq!(i.pv.len(), 2);
        }
    }
}
