//! M7-3a: the text, HTML and LaTeX documents of the player report
//! (acceptance criterion 10 of `docs/specs/m7-3-relatorio-de-jogador.md`).
//! The snapshots were read by hand against the numbers worked out in
//! `m7_3a_report.rs`.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m7_3a_support.rs"]
mod support;

use mychess_core::Color::{Black, White};
use mychess_db::report::{
    PlayerLabels, PlayerReport, PlayerReportOptions, ReportLabels, ReportPart, render_player_html,
    render_player_latex, render_player_text,
};
use support::{E4_E5_NF3, ask, fixture, game, open, run};

fn labels() -> PlayerLabels {
    PlayerLabels {
        base: ReportLabels {
            database_name: "report.pgn".into(),
            generator: "MyChess 0.1.0".into(),
            date: "2026-10-29".into(),
            ..ReportLabels::default()
        },
        ..PlayerLabels::default()
    }
}

fn alfa_white() -> PlayerReport {
    run(&mut fixture(), &ask("Alfa, A", White))
}

fn text(r: &PlayerReport) -> String {
    render_player_text(
        r,
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::Full,
    )
}

#[test]
fn text_of_alfa_with_white() {
    insta::assert_snapshot!(text(&alfa_white()));
}

#[test]
fn html_of_alfa_with_white() {
    insta::assert_snapshot!(render_player_html(
        &alfa_white(),
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::Full
    ));
}

#[test]
fn latex_of_alfa_with_white() {
    insta::assert_snapshot!(render_player_latex(
        &alfa_white(),
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::Full
    ));
}

#[test]
fn text_of_alfa_with_black_is_seen_from_alfa() {
    let r = run(&mut fixture(), &ask("Alfa, A", Black));
    insta::assert_snapshot!(text(&r));
}

#[test]
fn text_of_alfa_at_the_current_position() {
    let mut a = ask("Alfa, A", White);
    a.ucis = Some(E4_E5_NF3);
    let r = run(&mut fixture(), &a);
    let doc = text(&r);
    assert!(
        doc.contains("with the White pieces after 1.e4 e5 2.Nf3"),
        "{doc}"
    );
    insta::assert_snapshot!(doc);
}

#[test]
fn the_legend_of_the_point_of_view_is_in_the_three_formats() {
    let r = alfa_white();
    let o = PlayerReportOptions::default();
    for doc in [
        render_player_text(&r, &o, &labels(), ReportPart::Full),
        render_player_html(&r, &o, &labels(), ReportPart::Full),
        render_player_latex(&r, &o, &labels(), ReportPart::Full),
    ] {
        assert!(
            doc.contains("from the point of view of A Alfa"),
            "natural order in the legend: {doc}"
        );
    }
}

#[test]
fn the_title_has_the_name_in_the_natural_order() {
    let doc = text(&alfa_white());
    assert!(doc.contains("A ALFA"), "{doc}");
    let html = render_player_html(
        &alfa_white(),
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::Full,
    );
    assert!(html.contains("<title>A Alfa</title>"));
    assert!(html.contains("<h2><center>A Alfa</center></h2>"));
}

fn tricky() -> PlayerReport {
    let pgn = game(
        "Smith & Co, A_B",
        "O'Hara <b>, \"Q\"",
        "1-0",
        "2000.01.01",
        "R&D_#1 {x}",
        (2000, 2100),
        Some("C44"),
        "1. e4 e5 2. Nf3",
    );
    run(
        &mut open(&(pgn.clone() + &pgn)),
        &ask("Smith & Co, A_B", White),
    )
}

#[test]
fn html_escapes_names() {
    let r = tricky();
    let html = render_player_html(
        &r,
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::Full,
    );
    assert!(html.contains("A_B Smith &amp; Co"), "{html}");
    assert!(html.contains("O&#39;Hara &lt;b&gt;"));
    assert!(!html.contains("<b>,"), "raw tag from a name");
    assert!(!html.contains("Smith & Co"), "raw ampersand");
}

#[test]
fn latex_escapes_names() {
    let r = tricky();
    let tex = render_player_latex(
        &r,
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::Full,
    );
    assert!(tex.contains("A\\_B Smith \\& Co"), "{tex}");
    assert!(tex.contains("R\\&D\\_\\#1 \\{x\\}"));
    assert!(tex.contains("\\begin{document}") && tex.contains("\\end{document}"));
}

#[test]
fn no_document_has_a_nul_or_a_carriage_return() {
    let r = alfa_white();
    let o = PlayerReportOptions::default();
    for part in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
        for doc in [
            render_player_text(&r, &o, &labels(), part),
            render_player_html(&r, &o, &labels(), part),
            render_player_latex(&r, &o, &labels(), part),
        ] {
            assert!(!doc.contains('\0') && !doc.contains('\r'));
        }
    }
}

#[test]
fn parts_include_and_omit_the_expected_blocks() {
    let mut a = ask("Alfa, A", White);
    a.ucis = Some(E4_E5_NF3);
    let r = run(&mut fixture(), &a);
    let o = PlayerReportOptions::default();
    let full = render_player_text(&r, &o, &labels(), ReportPart::Full);
    let compact = render_player_text(&r, &o, &labels(), ReportPart::Compact);
    let table = render_player_text(&r, &o, &labels(), ReportPart::TableOnly);
    assert!(full.contains("THEORY TABLE") && full.contains("STATISTICS AND HISTORY"));
    assert!(!compact.contains("THEORY TABLE") && compact.contains("STATISTICS AND HISTORY"));
    assert!(!table.contains("STATISTICS AND HISTORY") && table.contains("+3 =0 -0"));
}

#[test]
fn sections_follow_the_options() {
    let r = alfa_white();
    let off = PlayerReportOptions {
        stats: false,
        oldest: 0,
        newest: 0,
        opponents: 0,
        results: false,
        avg_perf: false,
        high_rating: 0,
        eco_codes: 0,
        themes: false,
        endgames: false,
        max_table_games: 0,
        extra_moves: 1,
    };
    let doc = render_player_text(&r, &off, &labels(), ReportPart::Full);
    for section in ["STATISTICS", "RATINGS", "MOVES AND THEMES", "THEORY TABLE"] {
        assert!(!doc.contains(section), "{section} in {doc}");
    }
    let only_eco = PlayerReportOptions {
        eco_codes: 2,
        ..off
    };
    let doc = render_player_text(&r, &only_eco, &labels(), ReportPart::Full);
    assert!(doc.contains("1. MOVES AND THEMES"), "{doc}");
    assert!(doc.contains("C40-C49") && doc.contains("C60-C69") && !doc.contains("C70-C79"));
}

#[test]
fn an_empty_report_says_there_are_no_games_of_the_player() {
    let r = run(&mut fixture(), &ask("Zeta, Z", White));
    let doc = text(&r);
    assert!(doc.contains("No games of Z Zeta"), "{doc}");
    assert!(!doc.contains("STATISTICS"));
    let table = render_player_text(
        &r,
        &PlayerReportOptions::default(),
        &labels(),
        ReportPart::TableOnly,
    );
    assert!(table.contains("No games"));
}

#[test]
fn an_interrupted_report_is_marked() {
    let cancel = mychess_db::CancelToken::new();
    cancel.cancel();
    let r = support::run_with(
        &mut fixture(),
        &ask("Alfa, A", White),
        &mut mychess_db::NoProgress,
        &cancel,
    )
    .unwrap();
    assert!(text(&r).contains("The report was interrupted"));
}

#[test]
fn translated_labels_are_used() {
    let mut l = labels();
    l.pov = "do ponto de vista de {n}".into();
    l.with_white = "com as pecas brancas".into();
    l.wins = "Vitorias".into();
    let doc = render_player_text(
        &alfa_white(),
        &PlayerReportOptions::default(),
        &l,
        ReportPart::Full,
    );
    assert!(doc.contains("do ponto de vista de A Alfa"));
    assert!(doc.contains("com as pecas brancas"));
    assert!(doc.contains("Vitorias"));
}

#[test]
fn the_header_has_no_after_for_a_position_without_moves() {
    // The standard start position as the reference: an empty line.
    let mut a = ask("Alfa, A", White);
    a.ucis = Some("");
    let r = run(&mut fixture(), &a);
    assert!(r.position.is_some() && r.stem.is_empty());
    let l = PlayerLabels {
        after: "after {n}".into(),
        ..labels()
    };
    let out = render_player_text(&r, &PlayerReportOptions::default(), &l, ReportPart::Full);
    assert!(!out.contains("after"), "{out}");
    // With moves it is there.
    let mut b = ask("Alfa, A", White);
    b.ucis = Some(E4_E5_NF3);
    let r = run(&mut fixture(), &b);
    let out = render_player_text(&r, &PlayerReportOptions::default(), &l, ReportPart::Full);
    assert!(out.contains("after 1.e4 e5 2.Nf3"), "{out}");
}
