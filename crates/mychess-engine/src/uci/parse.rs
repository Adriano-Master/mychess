//! Tolerant parsing of the lines a UCI engine writes.
//!
//! Everything here is a pure function from text to plain data, with no
//! knowledge of the position: moves stay as text until `convert` validates
//! them. Nothing panics on any input (spec m4-1, D411, rule 10): a malformed
//! field is skipped on its own and never discards the rest of the line.

use crate::option::{EngineOption, OptionKind};

/// The most PV moves kept from one `info` line.
pub(crate) const MAX_PV_MOVES: usize = 256;
/// The longest text kept from `id name`, `id author` and `info string`.
pub(crate) const MAX_TEXT_BYTES: usize = 4096;
/// The longest option name accepted; longer ones drop the option.
pub(crate) const MAX_OPTION_NAME_BYTES: usize = 256;
/// The most `var` entries kept for one `combo` option.
pub(crate) const MAX_COMBO_VARS: usize = 1024;

/// Cuts `text` to at most `max` bytes at a character boundary.
pub(crate) fn truncate_utf8(text: &str, max: usize) -> String {
    let mut cut = text.len().min(max);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text[..cut].to_owned()
}

/// One parsed engine line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UciLine {
    /// `uciok`.
    UciOk,
    /// `readyok`.
    ReadyOk,
    /// `id name ...`.
    IdName(String),
    /// `id author ...`.
    IdAuthor(String),
    /// `option name ...`; `Err` carries the reason the option was dropped.
    Option(Result<EngineOption, String>),
    /// `info ...`.
    Info(RawInfo),
    /// `bestmove ...`.
    BestMove(RawBestMove),
    /// Anything else.
    Unknown,
}

/// The score kind of an `info` line, from the engine's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawScoreKind {
    /// `cp n`.
    Cp(i32),
    /// `mate n`.
    Mate(i32),
}

/// Bound marker after a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RawBound {
    #[default]
    Exact,
    Lower,
    Upper,
}

/// An `info` line before it is checked against a position.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RawInfo {
    pub(crate) depth: Option<u32>,
    pub(crate) seldepth: Option<u32>,
    pub(crate) time_ms: Option<u64>,
    pub(crate) nodes: Option<u64>,
    pub(crate) nps: Option<u64>,
    pub(crate) hashfull: Option<u16>,
    pub(crate) cpuload: Option<u16>,
    pub(crate) tbhits: Option<u64>,
    pub(crate) multipv: Option<u8>,
    pub(crate) score: Option<(RawScoreKind, RawBound)>,
    pub(crate) wdl: Option<(u32, u32, u32)>,
    pub(crate) pv: Vec<String>,
    pub(crate) currmove: Option<String>,
    pub(crate) currmovenumber: Option<u32>,
    pub(crate) string: Option<String>,
}

/// A `bestmove` line before it is checked against a position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBestMove {
    /// The move text, `None` for `(none)` and `0000`.
    pub(crate) mv: Option<String>,
    /// The `ponder` move text.
    pub(crate) ponder: Option<String>,
    /// The line had no move at all (`bestmove` alone): `mv` is `None` too,
    /// but this is a protocol violation, not `(none)` (spec m4-1, D411).
    pub(crate) missing: bool,
}

/// Parses one engine line (already decoded, without line terminator).
pub(crate) fn parse_line(line: &str) -> UciLine {
    let line = line.trim();
    let mut words = line.split_whitespace();
    match words.next() {
        Some("uciok") => UciLine::UciOk,
        Some("readyok") => UciLine::ReadyOk,
        Some("id") => parse_id(line),
        Some("option") => UciLine::Option(parse_option(line)),
        Some("info") => UciLine::Info(parse_info(line)),
        Some("bestmove") => UciLine::BestMove(parse_bestmove(line)),
        _ => UciLine::Unknown,
    }
}

fn parse_id(line: &str) -> UciLine {
    let rest = line.strip_prefix("id").unwrap_or("").trim_start();
    if let Some(name) = rest.strip_prefix("name") {
        if name.is_empty() || name.starts_with(char::is_whitespace) {
            return UciLine::IdName(truncate_utf8(name.trim(), MAX_TEXT_BYTES));
        }
    } else if let Some(author) = rest.strip_prefix("author")
        && (author.is_empty() || author.starts_with(char::is_whitespace))
    {
        return UciLine::IdAuthor(truncate_utf8(author.trim(), MAX_TEXT_BYTES));
    }
    UciLine::Unknown
}

const OPTION_KEYS: [&str; 6] = ["name", "type", "default", "min", "max", "var"];

/// Parses an `option name ... type ...` line. Names and defaults may have
/// several words (Glaurung); `var` repeats to build a combo list.
pub(crate) fn parse_option(line: &str) -> Result<EngineOption, String> {
    parse_option_noted(line).map(|(option, _)| option)
}

/// Like [`parse_option`], with a note when the option was accepted after a
/// repair (a `spin` default outside its range is clamped).
pub(crate) fn parse_option_noted(line: &str) -> Result<(EngineOption, Option<String>), String> {
    let mut words = line.split_whitespace();
    if words.next() != Some("option") {
        return Err("not an option line".to_owned());
    }
    let mut name: Option<String> = None;
    let mut ty: Option<String> = None;
    let mut default: Option<String> = None;
    let mut min: Option<String> = None;
    let mut max: Option<String> = None;
    let mut vars: Vec<String> = Vec::new();
    let mut key: Option<&str> = None;
    let mut buf: Vec<&str> = Vec::new();

    let mut flush = |key: Option<&str>, buf: &mut Vec<&str>| {
        let Some(key) = key else {
            buf.clear();
            return;
        };
        let text = buf.join(" ");
        buf.clear();
        // A hostile engine could send megabytes per field.
        let text = match key {
            "default" | "var" => truncate_utf8(&text, MAX_TEXT_BYTES),
            _ => text,
        };
        match key {
            "name" => name = Some(text),
            "type" => ty = Some(text),
            "default" => default = Some(text),
            "min" => min = Some(text),
            "max" => max = Some(text),
            _ => {
                if vars.len() < MAX_COMBO_VARS {
                    vars.push(text);
                }
            }
        }
    };
    for word in words {
        if OPTION_KEYS.contains(&word) && !(key.is_none() && word != "name") {
            flush(key, &mut buf);
            key = Some(word);
        } else {
            buf.push(word);
        }
    }
    flush(key, &mut buf);

    let name = name
        .filter(|n| !n.is_empty())
        .ok_or("option without a name")?;
    if name.len() > MAX_OPTION_NAME_BYTES {
        return Err(format!(
            "option name longer than {MAX_OPTION_NAME_BYTES} bytes"
        ));
    }
    let mut note = None;
    let ty = ty.ok_or_else(|| format!("option `{name}` has no type"))?;
    let kind = match ty.as_str() {
        "check" => OptionKind::Check {
            default: default.as_deref() == Some("true"),
        },
        "spin" => {
            let num = |v: &Option<String>, what: &str| {
                v.as_deref()
                    .and_then(|s| s.parse::<i64>().ok())
                    .ok_or_else(|| format!("spin `{name}` has no valid {what}"))
            };
            let (default, min, max) = (
                num(&default, "default")?,
                num(&min, "min")?,
                num(&max, "max")?,
            );
            if min > max {
                return Err(format!(
                    "spin `{name}` has an inconsistent range {min}..={max}"
                ));
            }
            let clamped = default.clamp(min, max);
            if clamped != default {
                note = Some(format!(
                    "spin `{name}`: default {default} is outside {min}..={max}; using {clamped}"
                ));
            }
            let default = clamped;
            OptionKind::Spin { default, min, max }
        }
        "combo" => OptionKind::Combo {
            default: default.unwrap_or_default(),
            vars,
        },
        "button" => OptionKind::Button,
        "string" => OptionKind::String {
            default: match default.as_deref() {
                Some("<empty>") | None => String::new(),
                Some(d) => d.to_owned(),
            },
        },
        other => return Err(format!("option `{name}` has the unknown type `{other}`")),
    };
    Ok((EngineOption { name, kind }, note))
}

/// Whether `word` looks like a move in coordinate notation
/// (`^[a-h][1-8][a-h][1-8][qrbn]?$` or `0000`).
pub(crate) fn looks_like_move(word: &str) -> bool {
    let b = word.as_bytes();
    if word == "0000" {
        return true;
    }
    if !(b.len() == 4 || b.len() == 5) {
        return false;
    }
    let file = |c: u8| (b'a'..=b'h').contains(&c);
    let rank = |c: u8| (b'1'..=b'8').contains(&c);
    file(b[0])
        && rank(b[1])
        && file(b[2])
        && rank(b[3])
        && b.get(4)
            .is_none_or(|p| matches!(p, b'q' | b'r' | b'b' | b'n'))
}

fn num<T: std::str::FromStr>(word: Option<&&str>) -> Option<T> {
    word.and_then(|w| w.parse::<T>().ok())
}

/// Parses an `info` line. Tokens may come in any order; `pv` stops at the
/// first word that is not a move or is an `info` keyword.
pub(crate) fn parse_info(line: &str) -> RawInfo {
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut info = RawInfo::default();
    let mut i = 1; // words[0] is "info"
    while i < words.len() {
        let next = words.get(i + 1);
        match words[i] {
            "depth" => info.depth = num(next).or(info.depth),
            "seldepth" => info.seldepth = num(next).or(info.seldepth),
            "time" => info.time_ms = num(next).or(info.time_ms),
            "nodes" => info.nodes = num(next).or(info.nodes),
            "nps" => info.nps = num(next).or(info.nps),
            "tbhits" => info.tbhits = num(next).or(info.tbhits),
            "hashfull" => info.hashfull = num::<u16>(next).filter(|v| *v <= 1000).or(info.hashfull),
            "cpuload" => info.cpuload = num::<u16>(next).filter(|v| *v <= 1000).or(info.cpuload),
            "multipv" => info.multipv = num::<u8>(next).filter(|v| *v >= 1).or(info.multipv),
            "currmovenumber" => info.currmovenumber = num(next).or(info.currmovenumber),
            "currmove" => {
                if let Some(w) = next.filter(|w| looks_like_move(w)) {
                    info.currmove = Some((*w).to_owned());
                }
            }
            "score" => {
                if let Some(parsed) = parse_score(&words[i + 1..]) {
                    info.score = Some(parsed);
                }
            }
            "wdl" => {
                if let (Some(w), Some(d), Some(l)) =
                    (num(next), num(words.get(i + 2)), num(words.get(i + 3)))
                {
                    info.wdl = Some((w, d, l));
                }
            }
            "pv" => {
                let mut moves = Vec::new();
                let mut j = i + 1;
                while let Some(w) = words.get(j) {
                    // No `info` keyword looks like a move, so this ends the
                    // list at the next keyword too.
                    if !looks_like_move(w) {
                        break;
                    }
                    if moves.len() < MAX_PV_MOVES {
                        moves.push((*w).to_owned());
                    }
                    j += 1;
                }
                info.pv = moves;
                i = j;
                continue;
            }
            "string" => {
                info.string = Some(truncate_utf8(&words[i + 1..].join(" "), MAX_TEXT_BYTES));
                break;
            }
            "refutation" | "currline" => break,
            _ => {}
        }
        i += 1;
    }
    info
}

/// `cp n` or `mate n`, optionally followed by `lowerbound`/`upperbound`.
/// Anything else (Prodeo writes `score` with neither) yields `None`.
fn parse_score(words: &[&str]) -> Option<(RawScoreKind, RawBound)> {
    let n = words.get(1).and_then(|w| w.parse::<i32>().ok())?;
    let kind = match *words.first()? {
        "cp" => RawScoreKind::Cp(n),
        "mate" => RawScoreKind::Mate(n),
        _ => return None,
    };
    let bound = match words.get(2).copied() {
        Some("lowerbound") => RawBound::Lower,
        Some("upperbound") => RawBound::Upper,
        _ => RawBound::Exact,
    };
    Some((kind, bound))
}

/// Parses `bestmove <move> [ponder <move>]`.
pub(crate) fn parse_bestmove(line: &str) -> RawBestMove {
    let words: Vec<&str> = line.split_whitespace().collect();
    let Some(first) = words.get(1).copied() else {
        return RawBestMove {
            mv: None,
            ponder: None,
            missing: true,
        };
    };
    let none = |w: &str| w == "(none)" || w == "0000" || w == "none";
    let mv = (!none(first)).then(|| first.to_owned());
    let ponder = words
        .iter()
        .position(|w| *w == "ponder")
        .and_then(|p| words.get(p + 1))
        .filter(|w| looks_like_move(w))
        .map(|w| (*w).to_owned());
    RawBestMove {
        mv,
        ponder,
        missing: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(line: &str) -> RawInfo {
        match parse_line(line) {
            UciLine::Info(i) => i,
            other => panic!("not an info line: {other:?}"),
        }
    }

    #[test]
    fn option_default_and_combo_vars_are_capped() {
        let huge = "x".repeat(10 * MAX_TEXT_BYTES);
        let line = format!("option name Style type combo default {huge} var {huge} var b");
        let Ok(EngineOption {
            kind: OptionKind::Combo { default, vars },
            ..
        }) = parse_option(&line)
        else {
            panic!("combo expected")
        };
        assert_eq!(default.len(), MAX_TEXT_BYTES);
        assert_eq!(vars[0].len(), MAX_TEXT_BYTES);
        assert_eq!(vars[1], "b");
        let line = format!("option name S type string default {huge}");
        let Ok(EngineOption {
            kind: OptionKind::String { default },
            ..
        }) = parse_option(&line)
        else {
            panic!("string expected")
        };
        assert_eq!(default.len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn parses_a_stockfish_info_line() {
        let i = info(
            "info depth 20 seldepth 28 multipv 2 score cp -34 nodes 1234567 nps 987654 \
             hashfull 120 tbhits 0 time 1250 pv e7e5 g1f3 b8c6",
        );
        assert_eq!(i.depth, Some(20));
        assert_eq!(i.seldepth, Some(28));
        assert_eq!(i.multipv, Some(2));
        assert_eq!(i.score, Some((RawScoreKind::Cp(-34), RawBound::Exact)));
        assert_eq!(i.nodes, Some(1_234_567));
        assert_eq!(i.hashfull, Some(120));
        assert_eq!(i.pv, ["e7e5", "g1f3", "b8c6"]);
    }

    #[test]
    fn pv_in_the_middle_ends_at_the_next_keyword() {
        let i = info("info pv e2e4 e7e5 score cp 20 depth 9");
        assert_eq!(i.pv, ["e2e4", "e7e5"]);
        assert_eq!(i.score, Some((RawScoreKind::Cp(20), RawBound::Exact)));
        assert_eq!(i.depth, Some(9));
    }

    #[test]
    fn bounds_mate_and_wdl() {
        let i = info("info depth 5 score cp 12 lowerbound wdl 400 500 100");
        assert_eq!(i.score, Some((RawScoreKind::Cp(12), RawBound::Lower)));
        assert_eq!(i.wdl, Some((400, 500, 100)));
        let m = info("info score mate -3 upperbound pv a1a2");
        assert_eq!(m.score, Some((RawScoreKind::Mate(-3), RawBound::Upper)));
    }

    #[test]
    fn string_refutation_and_currline_consume_the_rest() {
        let s = info("info string NNUE evaluation using nn-1.nnue pv e2e4");
        assert_eq!(
            s.string.as_deref(),
            Some("NNUE evaluation using nn-1.nnue pv e2e4")
        );
        assert_eq!(s.pv.len(), 0);
        assert_eq!(info("info refutation d1h5 g6h5").pv.len(), 0);
        assert_eq!(info("info currline 1 e2e4 e7e5").pv.len(), 0);
    }

    #[test]
    fn bad_field_is_skipped_but_the_line_survives() {
        let i = info("info depth abc score foo 12 nodes 99 multipv 0 hashfull 5000 pv e2e4");
        assert_eq!(i.depth, None);
        assert_eq!(i.score, None);
        assert_eq!(i.nodes, Some(99));
        assert_eq!(i.multipv, None);
        assert_eq!(i.hashfull, None);
        assert_eq!(i.pv, ["e2e4"]);
    }

    #[test]
    fn pv_is_capped() {
        let line = format!("info pv {}", "e2e4 ".repeat(MAX_PV_MOVES + 50));
        assert_eq!(info(&line).pv.len(), MAX_PV_MOVES);
    }

    #[test]
    fn options_of_every_type() {
        let cases = [
            (
                "option name Hash type spin default 16 min 1 max 33554432",
                "Hash",
            ),
            ("option name Ponder type check default false", "Ponder"),
            ("option name Clear Hash type button", "Clear Hash"),
            (
                "option name SyzygyPath type string default <empty>",
                "SyzygyPath",
            ),
        ];
        for (line, name) in cases {
            let UciLine::Option(Ok(o)) = parse_line(line) else {
                panic!("{line}")
            };
            assert_eq!(o.name, name);
        }
        let UciLine::Option(Ok(combo)) = parse_line(
            "option name Playing Style type combo default Normal var Very Solid var Normal var Risky",
        ) else {
            panic!()
        };
        assert_eq!(
            combo.kind,
            OptionKind::Combo {
                default: "Normal".into(),
                vars: vec!["Very Solid".into(), "Normal".into(), "Risky".into()]
            }
        );
    }

    #[test]
    fn multi_word_default_and_bad_options() {
        let UciLine::Option(Ok(o)) =
            parse_line("option name Book File type string default my book.bin")
        else {
            panic!()
        };
        assert_eq!(
            o.kind,
            OptionKind::String {
                default: "my book.bin".into()
            }
        );
        for bad in [
            "option name X type spin default 5 min 10 max 1",
            "option name X type wibble",
            "option type check",
        ] {
            assert!(matches!(parse_line(bad), UciLine::Option(Err(_))), "{bad}");
        }
        // A default outside the range is repaired, not rejected, with a note.
        let (o, note) = parse_option_noted("option name X type spin default 50 min 1 max 10")
            .expect("repaired");
        assert_eq!(
            o.kind,
            OptionKind::Spin {
                default: 10,
                min: 1,
                max: 10
            }
        );
        assert!(note.is_some());
    }

    #[test]
    fn bestmove_forms() {
        let bm = |l: &str| match parse_line(l) {
            UciLine::BestMove(b) => b,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            bm("bestmove e2e4 ponder e7e5").ponder.as_deref(),
            Some("e7e5")
        );
        assert_eq!(bm("bestmove (none)").mv, None);
        assert_eq!(bm("bestmove 0000").mv, None);
        assert_eq!(bm("bestmove a7a8q").mv.as_deref(), Some("a7a8q"));
        assert!(matches!(
            parse_line("bestmove"),
            UciLine::BestMove(RawBestMove { missing: true, .. })
        ));
    }

    #[test]
    fn simple_lines_and_unknown_text() {
        assert_eq!(parse_line("uciok"), UciLine::UciOk);
        assert_eq!(parse_line("  readyok  "), UciLine::ReadyOk);
        assert_eq!(
            parse_line("id name Stock Fish 17"),
            UciLine::IdName("Stock Fish 17".into())
        );
        assert_eq!(
            parse_line("id author The Authors"),
            UciLine::IdAuthor("The Authors".into())
        );
        assert_eq!(parse_line("id nameless"), UciLine::Unknown);
        assert_eq!(parse_line("hello world"), UciLine::Unknown);
        assert_eq!(parse_line(""), UciLine::Unknown);
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn arbitrary_text_never_panics(s in ".*") {
                drop(parse_line(&s));
                drop(parse_line(&format!("info {s}")));
                drop(parse_line(&format!("option name {s}")));
                drop(parse_line(&format!("bestmove {s}")));
            }

            #[test]
            fn shuffled_info_tokens_never_panic(tokens in proptest::collection::vec(
                prop_oneof![
                    Just("depth".to_owned()), Just("score".to_owned()), Just("cp".to_owned()),
                    Just("mate".to_owned()), Just("pv".to_owned()), Just("e2e4".to_owned()),
                    Just("string".to_owned()), Just("-5".to_owned()), Just("wdl".to_owned()),
                    Just("multipv".to_owned()), "[a-z0-9]{0,6}",
                ], 0..40))
            {
                drop(parse_line(&format!("info {}", tokens.join(" "))));
            }
        }
    }
}

#[cfg(test)]
mod contract_tests;
