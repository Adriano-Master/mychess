//! M7-2a: the text, HTML and LaTeX documents of the opening report
//! (acceptance criterion 15). The snapshots were read by hand against the
//! rules of the spec (see the numbers in `m7_2a_report.rs`).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m7_2a_support.rs"]
mod support;

use mychess_db::report::{
    OpeningReport, ReportLabels, ReportOptions, ReportPart, SectionSet, render_html, render_latex,
    render_text,
};
use support::{E4_E5_NF3, fixture, open, pgn_game, report};

fn labels() -> ReportLabels {
    ReportLabels {
        database_name: "report.pgn".into(),
        generator: "MyChess 0.1.0".into(),
        date: "2026-10-29".into(),
        ..ReportLabels::default()
    }
}

fn fixture_report() -> OpeningReport {
    report(&mut fixture(), E4_E5_NF3, &ReportOptions::default())
}

#[test]
fn text_of_the_fixture() {
    let r = fixture_report();
    insta::assert_snapshot!(render_text(
        &r,
        &ReportOptions::default(),
        &labels(),
        ReportPart::Full
    ));
}

#[test]
fn html_of_the_fixture() {
    let r = fixture_report();
    insta::assert_snapshot!(render_html(
        &r,
        &ReportOptions::default(),
        &labels(),
        ReportPart::Full
    ));
}

#[test]
fn latex_of_the_fixture() {
    let r = fixture_report();
    insta::assert_snapshot!(render_latex(
        &r,
        &ReportOptions::default(),
        &labels(),
        ReportPart::Full
    ));
}

/// Thirty games after `1.e4 e5 2.Nf3` that differ in the next moves, for a
/// table that must merge rows. Each entry is the rest of the game.
const THIRTY: [&str; 30] = [
    "Nc6 Bb5 a6",
    "Nc6 Bb5 Nf6",
    "Nc6 Bb5 f5",
    "Nc6 Bb5 d6",
    "Nc6 Bb5 Bc5",
    "Nc6 Bb5 a6 Ba4",
    "Nc6 Bb5 a6 Bxc6",
    "Nc6 Bb5 Nf6 O-O",
    "Nc6 Bc4 Bc5",
    "Nc6 Bc4 Nf6",
    "Nc6 Bc4 Be7",
    "Nc6 Bc4 Bc5 c3",
    "Nc6 d4 exd4",
    "Nc6 d4 exd4 Nxd4",
    "Nc6 Nc3 Nf6",
    "Nc6 Nc3 Bc5",
    "Nf6 Nxe5 d6",
    "Nf6 Nxe5 Nxe4",
    "Nf6 Nxe5 d6 Nf3",
    "Nf6 d4 Nxe4",
    "Nf6 d4 exd4",
    "Nf6 Nc3 Nc6",
    "d6 d4 exd4",
    "d6 d4 Nf6",
    "d6 Bc4 Be7",
    "d6 Nc3 Nf6",
    "Nc6 Bb5 a6 Ba4 Nf6",
    "Nc6 Bb5 a6 Ba4 Nf6 O-O",
    "Nc6 Bb5 a6 Ba4 Nf6 O-O Be7",
    "Qe7 d4",
];

fn many_lines_pgn() -> String {
    THIRTY
        .iter()
        .enumerate()
        .map(|(i, rest)| {
            let elo = 2000 + u16::try_from(i).unwrap();
            let result = ["1-0", "0-1", "1/2-1/2"][i % 3];
            let all: Vec<&str> = ["e4", "e5", "Nf3"]
                .into_iter()
                .chain(rest.split_whitespace())
                .collect();
            let moves: Vec<String> = all
                .iter()
                .enumerate()
                .map(|(k, m)| {
                    if k % 2 == 0 {
                        format!("{}. {m}", k / 2 + 1)
                    } else {
                        (*m).to_owned()
                    }
                })
                .collect();
            pgn_game(
                "W, W",
                "B, B",
                result,
                "2001.01.01",
                "x",
                (elo, 2000),
                &moves.join(" "),
            )
        })
        .collect()
}

fn merged_report(rows: u8) -> (OpeningReport, ReportOptions) {
    let mut db = open(&many_lines_pgn());
    let opts = ReportOptions {
        table_rows: rows,
        ..ReportOptions::default()
    };
    (report(&mut db, E4_E5_NF3, &opts), opts)
}

#[test]
fn a_table_of_thirty_lines_merges_to_eight_rows() {
    let (r, opts) = merged_report(8);
    let c = &r.core;
    assert_eq!(c.rows.len(), 8);
    assert_eq!(c.rows.iter().map(|x| x.count).sum::<u32>(), 30);
    let mut seen: Vec<usize> = c
        .rows
        .iter()
        .flat_map(|x| std::iter::once(x.line).chain(x.merged.iter().copied()))
        .collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..30).collect::<Vec<_>>(), "each line once");
    // Each merged line is in the note that its row carries.
    let noted = c.lines.iter().filter(|l| l.note > 0).count();
    assert_eq!(
        noted,
        c.notes.iter().map(|n| n.entries.len()).sum::<usize>()
    );
    for row in &c.rows {
        assert!(c.lines[row.line].note > 0, "the main line has its note");
    }
    insta::assert_snapshot!(
        "text_of_a_table_that_merges_to_eight_rows",
        render_text(&r, &opts, &labels(), ReportPart::TableOnly)
    );
}

#[test]
fn a_table_of_thirty_lines_with_the_automatic_number_of_rows() {
    let (r, opts) = merged_report(0);
    // isqrt(30 * 3 / 4) + 3 = 7 rows.
    assert!(r.core.rows.len() <= 7, "{}", r.core.rows.len());
    assert!(r.core.rows.len() >= 5);
    insta::assert_snapshot!(
        "text_of_a_table_with_automatic_rows",
        render_text(&r, &opts, &labels(), ReportPart::TableOnly)
    );
}

#[test]
fn parts_include_and_omit_their_blocks() {
    let r = fixture_report();
    let o = ReportOptions::default();
    let l = labels();
    for render in [render_text, render_html, render_latex] {
        let full = render(&r, &o, &l, ReportPart::Full);
        let compact = render(&r, &o, &l, ReportPart::Compact);
        let table = render(&r, &o, &l, ReportPart::TableOnly);
        assert!(full.contains("Theory Table") || full.contains("THEORY TABLE"));
        assert!(!compact.contains("Theory Table") && !compact.contains("THEORY TABLE"));
        assert!(!table.to_lowercase().contains("statistics and history"));
        assert!(full.contains("(3.5/5: 70%)") || full.contains("3.5/5: 70"));
        let flat = table.replace('\\', "");
        assert!(flat.contains("+3 =1 -1 (3.5/5: 70%)"), "{flat}");
        assert!(flat.contains("1.e4 e5 2.Nf3"));
        assert!(compact.len() < full.len());
    }
}

#[test]
fn the_defect_of_the_trends_header_is_not_reproduced() {
    // Only "shortest wins of White" on, results off: the section stays.
    let o = ReportOptions {
        sections: SectionSet::empty(),
        shortest_white: true,
        shortest_black: false,
        oldest: 0,
        newest: 0,
        high_rating: 0,
        move_orders: 0,
        most_frequent: 0,
        max_table_games: 0,
        ..ReportOptions::default()
    };
    let r = report(&mut fixture(), E4_E5_NF3, &o);
    let text = render_text(&r, &o, &labels(), ReportPart::Full);
    assert!(text.contains("1. RESULT TRENDS"), "{text}");
    assert!(text.contains("1.1  Shortest wins (White)"));
    assert!(!text.contains("(Black)"));
}

#[test]
fn an_empty_report_has_only_the_header() {
    let r = report(
        &mut fixture(),
        "d2d4 d7d5 c2c4 e7e6",
        &ReportOptions::default(),
    );
    assert_eq!(r.core.report_games(), 0);
    let text = render_text(&r, &ReportOptions::default(), &labels(), ReportPart::Full);
    assert!(text.contains("(0 games)"));
    assert!(!text.contains("STATISTICS"));
    let html = render_html(&r, &ReportOptions::default(), &labels(), ReportPart::Full);
    assert!(html.ends_with("</html>\n"));
}

fn tricky_report() -> OpeningReport {
    let pgn = [
        pgn_game(
            "Smith & Co <b>, \"Q\" 'x'",
            "100% $_{}#~^\\ \u{df}, Z",
            "1-0",
            "2020.01.01",
            "Rio & <Sao>",
            (2500, 2400),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5",
        ),
        pgn_game(
            "Zed, A",
            "Y, B",
            "0-1",
            "2020.01.01",
            "Lima",
            (2500, 2400),
            "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Bxc6 dxc6 5. Nxe5",
        ),
    ]
    .concat();
    report(&mut open(&pgn), E4_E5_NF3, &ReportOptions::default())
}

#[test]
fn html_escapes_names_and_is_balanced() {
    let r = tricky_report();
    let html = render_html(&r, &ReportOptions::default(), &labels(), ReportPart::Full);
    assert!(html.contains("Smith &amp; Co &lt;b&gt;"));
    assert!(html.contains("Rio &amp; &lt;Sao&gt;"));
    assert!(!html.contains("Smith & Co"));
    assert!(!html.contains("<b>, "));
    for tag in [
        "table", "ol", "li", "span", "pre", "h2", "h3", "p", "center", "b",
    ] {
        let open =
            html.matches(&format!("<{tag}>")).count() + html.matches(&format!("<{tag} ")).count();
        let close = html.matches(&format!("</{tag}>")).count();
        assert_eq!(open, close, "unbalanced <{tag}>");
    }
}

#[test]
fn latex_escapes_the_special_characters_and_has_a_document() {
    let r = tricky_report();
    let tex = render_latex(&r, &ReportOptions::default(), &labels(), ReportPart::Full);
    assert!(tex.contains("\\begin{document}") && tex.trim_end().ends_with("\\end{document}"));
    assert!(tex.contains("Smith \\& Co"));
    assert!(tex.contains("Rio \\& <Sao>"));
    assert!(tex.contains(
        "100\\% \\$\\_\\{\\}\\#\\textasciitilde{}\\textasciicircum{}\\textbackslash{} {\\ss}"
    ));
    assert!(!tex.contains("Smith & Co"));
}

#[test]
fn no_document_contains_a_nul() {
    let pgn = pgn_game(
        "Nul\u{0}Name, A",
        "B\u{1}, B",
        "1-0",
        "2020.01.01",
        "S\u{0}",
        (0, 0),
        "1. e4 e5 2. Nf3 Nc6",
    );
    let r = report(&mut open(&pgn), E4_E5_NF3, &ReportOptions::default());
    let mut l = labels();
    l.database_name = "a\u{0}b".into();
    for part in [ReportPart::Full, ReportPart::Compact, ReportPart::TableOnly] {
        for doc in [
            render_text(&r, &ReportOptions::default(), &l, part),
            render_html(&r, &ReportOptions::default(), &l, part),
            render_latex(&r, &ReportOptions::default(), &l, part),
        ] {
            assert!(!doc.contains('\0'));
            assert!(!doc.contains('\r'));
        }
    }
}

#[test]
fn the_defect_of_unsafe_markup_in_labels_is_not_reproduced() {
    let r = fixture_report();
    let mut l = labels();
    l.database_name = "<script>alert(1)</script>".into();
    let html = render_html(&r, &ReportOptions::default(), &l, ReportPart::Full);
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}
