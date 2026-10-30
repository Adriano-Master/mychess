//! M7-2a: properties of the opening report (acceptance criteria 18 and 19):
//! small random databases never panic, give the same report with one thread
//! and with several, keep the table coherent, and no document has a `NUL`;
//! the `reports.dat` reader accepts any text.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m7_2a_support.rs"]
mod support;

use std::collections::HashSet;
use std::fmt::Write as _;

use mychess_core::Position;
use mychess_db::report::{
    ReportLabels, ReportOptions, ReportPart, parse_reports_dat, render_html, render_latex,
    render_text,
};
use proptest::prelude::*;
use support::{open, pgn_game, report};

/// A random game as SAN words, from the choices of the strategy.
fn random_game(choices: &[u8]) -> Vec<String> {
    let mut pos = Position::new();
    let mut out = Vec::new();
    for &c in choices {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        let mv = moves[usize::from(c) % moves.len()];
        out.push(pos.san(mv).unwrap());
        pos.play(mv).unwrap();
    }
    out
}

fn numbered(sans: &[String]) -> String {
    sans.iter()
        .enumerate()
        .map(|(i, m)| {
            if i % 2 == 0 {
                format!("{}. {m}", i / 2 + 1)
            } else {
                m.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn uci_of(sans: &[String]) -> String {
    let mut pos = Position::new();
    let mut out = Vec::new();
    for san in sans {
        let mv = pos
            .parse_san(san, mychess_core::SanMode::Strict)
            .unwrap()
            .mv;
        out.push(pos.uci(mv));
        pos.play(mv).unwrap();
    }
    out.join(" ")
}

#[derive(Debug, Clone)]
struct Spec {
    games: Vec<(Vec<u8>, u16, u16, u8, u8)>,
    depth: usize,
    max_games: u32,
    max_table: u32,
    extra: u8,
    rows: u8,
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        prop::collection::vec(
            (
                prop::collection::vec(any::<u8>(), 0..24),
                0u16..2800,
                0u16..2800,
                0u8..4,
                0u8..30,
            ),
            1..10,
        ),
        0usize..4,
        0u32..6,
        0u32..6,
        0u8..4,
        0u8..8,
    )
        .prop_map(|(games, depth, max_games, max_table, extra, rows)| Spec {
            games,
            depth,
            max_games,
            max_table,
            extra,
            rows,
        })
}

fn build(spec: &Spec) -> (String, String, ReportOptions) {
    let results = ["1-0", "0-1", "1/2-1/2", "*"];
    let mut pgn = String::new();
    let mut first: Vec<String> = Vec::new();
    for (i, (choices, w, b, r, year)) in spec.games.iter().enumerate() {
        let sans = random_game(choices);
        if i == 0 {
            first.clone_from(&sans);
        }
        let name = format!("P{}, X", i % 3);
        pgn.push_str(&pgn_game(
            &name,
            "Q, Y",
            results[usize::from(*r)],
            &format!("{}.01.01", 1990 + u16::from(*year)),
            "Site & <1>",
            (*w, *b),
            &numbered(&sans),
        ));
    }
    let depth = spec.depth.min(first.len());
    let ucis = uci_of(&first[..depth]);
    let opts = ReportOptions {
        max_games: spec.max_games,
        max_table_games: spec.max_table,
        extra_moves: spec.extra,
        table_rows: spec.rows,
        ..ReportOptions::default()
    };
    (pgn, ucis, opts)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn a_random_database_gives_a_coherent_report(spec in spec()) {
        let (pgn, ucis, opts) = build(&spec);
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let many = rayon::ThreadPoolBuilder::new().num_threads(3).build().unwrap();
        let a = one.install(|| report(&mut open(&pgn), &ucis, &opts));
        let b = many.install(|| report(&mut open(&pgn), &ucis, &opts));
        prop_assert_eq!(&a.core, &b.core, "one thread and many agree");
        let c = &a.core;
        prop_assert!(!c.interrupted);
        let normalized = opts.normalized();
        // Lines: at most max_games, no repeats, by game number.
        prop_assert!(c.lines.len() <= usize::try_from(normalized.max_games).unwrap());
        prop_assert!(u32::try_from(c.lines.len()).unwrap() <= c.report_games());
        prop_assert!(c.lines.windows(2).all(|w| w[0].game < w[1].game));
        prop_assert_eq!(c.theory_games(), c.report_games());
        // The table: each chosen line exactly once, counts add up.
        let mut seen = HashSet::new();
        for row in &c.rows {
            prop_assert!(seen.insert(row.line));
            for &m in &row.merged {
                prop_assert!(seen.insert(m));
            }
            prop_assert_eq!(row.count as usize, 1 + row.merged.len());
        }
        prop_assert_eq!(seen.len(), c.table_order.len());
        prop_assert!(c.table_order.len() <= usize::try_from(normalized.max_table_games).unwrap());
        for note in &c.notes {
            prop_assert!(note.row < c.rows.len() && note.cell < 16);
            for e in &note.entries {
                prop_assert_eq!(c.lines[e.line].note, note.number);
            }
        }
        for row in &c.rows {
            prop_assert!(c.lines[row.line].note > 0);
        }
        // Percentages never exceed their bounds.
        for t in 0..10 {
            prop_assert!(c.theme_percent(t) <= 100);
        }
        // Documents: no NUL, in every part and format.
        let labels = ReportLabels::default();
        for part in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
            for doc in [
                render_text(&a, &opts, &labels, part),
                render_html(&a, &opts, &labels, part),
                render_latex(&a, &opts, &labels, part),
            ] {
                prop_assert!(!doc.contains('\0'));
                prop_assert!(!doc.contains('\r'));
            }
        }
    }

    #[test]
    fn the_reports_dat_reader_accepts_any_text(text in ".{0,300}") {
        let r = parse_reports_dat(&text);
        prop_assert!(r.favorites.iter().all(|f| !f.name.is_empty()));
    }

    #[test]
    fn the_reports_dat_reader_accepts_tcl_like_noise(
        text in prop::collection::vec(
            prop::sample::select(vec![
                "set", " ", "reportFavorites", "{", "}", "\"", "\\", "\n", ";", "#", "[", "]", "$x",
                "a", "1.e4", "\t",
            ]),
            0..80,
        )
    ) {
        let joined: String = text.concat();
        drop(parse_reports_dat(&joined));
    }

    #[test]
    fn well_formed_favorites_round_trip(
        names in prop::collection::vec("[A-Za-z0-9 ,]{1,12}", 1..6),
    ) {
        let entries = names.iter().fold(String::new(), |mut acc, n| {
            let _ = write!(acc, "{{{{{n}}} {{1.e4 e5}}}} ");
            acc
        });
        let r = parse_reports_dat(&format!("set reportFavorites {{{entries}}}\n"));
        let wanted: Vec<String> = names.iter().map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect();
        let got: Vec<String> = r.favorites.iter().map(|f| f.name.clone()).collect();
        prop_assert_eq!(got, wanted);
    }
}
