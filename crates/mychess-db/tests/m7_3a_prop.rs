//! M7-3a: properties of the player report (acceptance criterion 9): small
//! random databases never panic, give the same report with one thread and
//! with several, keep the figures coherent, and no document has a `NUL` or a
//! name that is not escaped. The databases are tiny on purpose (the cases
//! run under a memory limit).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m7_3a_support.rs"]
mod support;

use mychess_core::Color;
use mychess_core::Position;
use mychess_db::report::{
    NameMatch, PlayerLabels, PlayerReportOptions, ReportPart, render_player_html,
    render_player_latex, render_player_text,
};
use proptest::prelude::*;
use support::{Ask, game, open, run};

const MARK: &str = "Mar&<zz>_k#{}%$~^\\";

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

/// Moves, Elos, result, year, name slot and ECO slot of a game.
type RandomGame = (Vec<u8>, u16, u16, u8, u8, u8, u8);

#[derive(Debug, Clone)]
struct Spec {
    games: Vec<RandomGame>,
    depth: usize,
    black: bool,
    matching: u8,
    options: (u8, u8, u8, u8, u32, u8),
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        prop::collection::vec(
            (
                prop::collection::vec(any::<u8>(), 0..20),
                0u16..2800,
                0u16..2800,
                0u8..4,
                0u8..30,
                0u8..3,
                0u8..3,
            ),
            1..8,
        ),
        0usize..4,
        any::<bool>(),
        0u8..3,
        (0u8..12, 0u8..12, 0u8..12, 0u8..12, 0u32..6, 0u8..12),
    )
        .prop_map(|(games, depth, black, matching, options)| Spec {
            games,
            depth,
            black,
            matching,
            options,
        })
}

fn build(spec: &Spec) -> (String, Ask<'static>, String) {
    let results = ["1-0", "0-1", "1/2-1/2", "*"];
    let ecos = [None, Some("C44"), Some("B90")];
    let mut pgn = String::new();
    let mut first: Vec<String> = Vec::new();
    for (i, (choices, w, b, r, year, wn, eco)) in spec.games.iter().enumerate() {
        let sans = random_game(choices);
        if i == 0 {
            first.clone_from(&sans);
        }
        let name = |n: u8| format!("{MARK}{}, X", n % 3);
        pgn.push_str(&game(
            &name(*wn),
            &name(wn.wrapping_add(1)),
            results[usize::from(*r)],
            &format!("{}.01.01", 1990 + u16::from(*year)),
            "Site & <1>",
            (*w, *b),
            ecos[usize::from(*eco)],
            &numbered(&sans),
        ));
    }
    let depth = spec.depth.min(first.len());
    let ucis = uci_of(&first[..depth]);
    let (oldest, newest, opponents, eco_codes, table, extra) = spec.options;
    let ask = Ask {
        name: Box::leak(format!("{MARK}0, X").into_boxed_str()),
        color: if spec.black {
            Color::Black
        } else {
            Color::White
        },
        matching: [NameMatch::Exact, NameMatch::Prefix, NameMatch::Contains]
            [usize::from(spec.matching)],
        ucis: None,
        options: PlayerReportOptions {
            oldest,
            newest,
            opponents,
            eco_codes,
            max_table_games: table,
            extra_moves: extra,
            ..PlayerReportOptions::default()
        },
    };
    (pgn, ask, ucis)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn a_random_database_gives_a_coherent_player_report(spec in spec()) {
        let (pgn, mut ask, ucis) = build(&spec);
        if spec.depth > 0 {
            ask.ucis = Some(Box::leak(ucis.into_boxed_str()));
        }
        let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let many = rayon::ThreadPoolBuilder::new().num_threads(3).build().unwrap();
        let a = one.install(|| run(&mut open(&pgn), &ask));
        let b = many.install(|| run(&mut open(&pgn), &ask));
        prop_assert_eq!(&a.core, &b.core, "one thread and many agree");
        prop_assert_eq!(&a.opponents, &b.opponents);
        prop_assert_eq!(&a.eco_groups, &b.eco_groups);
        prop_assert_eq!(a.player_score, b.player_score);
        let c = &a.core;
        prop_assert!(!c.interrupted);
        if ask.ucis.is_none() && spec.matching == 0 {
            // Exact name from the beginning: the games of the player with
            // that colour that have at least one move.
            let expected = spec
                .games
                .iter()
                .filter(|g| {
                    let slot = if spec.black { g.5.wrapping_add(1) } else { g.5 };
                    slot % 3 == 0 && !random_game(&g.0).is_empty()
                })
                .count();
            prop_assert_eq!(usize::try_from(c.report_games()).unwrap(), expected);
        }
        // The score of the player adds up to the games of the report.
        let s = a.player_score;
        prop_assert_eq!(s.wins + s.draws + s.losses + s.unknown, c.report_games());
        prop_assert!(s.score_percent() <= 100);
        // The player is the colour asked in every game kept.
        for l in &c.lines {
            let name = if spec.black { &l.black } else { &l.white };
            prop_assert!(name.starts_with(MARK) || spec.matching != 0, "{name}");
        }
        // The lists are bounded and sorted as promised.
        prop_assert!(a.opponents.len() <= 10 && a.eco_groups.len() <= 10);
        prop_assert!(a.opponents.windows(2).all(|w| w[0].games >= w[1].games));
        prop_assert!(a.eco_groups.windows(2).all(|w| w[0].games >= w[1].games));
        prop_assert!(a.opponents.iter().all(|p| p.score_percent <= 100));
        prop_assert!(a.eco_groups.iter().all(|g| g.score_percent <= 100));
        prop_assert!(c.lines.windows(2).all(|w| w[0].game < w[1].game));
        prop_assert!(c.table_order.len() <= usize::try_from(a_table_limit(&ask)).unwrap());

        // Documents: no NUL or CR, names escaped, in every part and format.
        let labels = PlayerLabels::default();
        for part in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
            let html = render_player_html(&a, &ask.options, &labels, part);
            let tex = render_player_latex(&a, &ask.options, &labels, part);
            let txt = render_player_text(&a, &ask.options, &labels, part);
            for doc in [&html, &tex, &txt] {
                prop_assert!(!doc.contains('\0') && !doc.contains('\r'));
            }
            prop_assert!(!html.contains("<zz>") && !html.contains("Site & <1>"));
            prop_assert!(!html.contains("Mar&<"));
            // Every `_` of a name is escaped in LaTeX: none is left bare.
            prop_assert!(!tex.contains("Mar&") && !tex.contains("<zz>_"));
        }
    }

    #[test]
    fn names_of_any_text_never_panic(name in ".{0,40}", prefix in any::<bool>()) {
        let mut ask = support::ask(&name, Color::White);
        ask.matching = if prefix { NameMatch::Prefix } else { NameMatch::Contains };
        let r = support::run_with(
            &mut support::fixture(),
            &ask,
            &mut mychess_db::NoProgress,
            &mychess_db::CancelToken::new(),
        );
        // Either the name is refused or the report is made.
        match r {
            Ok(r) => prop_assert!(r.game_count() <= 8),
            Err(e) => prop_assert!(matches!(e, mychess_db::report::ReportError::InvalidName)),
        }
        prop_assert!(!mychess_db::report::natural_name(&name).contains('\0') || name.contains('\0'));
    }
}

fn a_table_limit(ask: &Ask<'_>) -> u32 {
    ask.options.normalized().max_table_games
}
