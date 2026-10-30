//! M7-4a, criterion 2 and rules 14-18 of `docs/specs/m7-4-exportacao.md`: the
//! LaTeX writer.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4a_support;

use m7_4a_support::{GAME_ONE, GAME_TWO, fixtures, read_games};
use mychess_core::{Game, Move};
use mychess_pgn::export::{
    LatexOptions, game_to_latex_string, write_game_latex, write_latex_document,
};

fn body() -> String {
    fixtures()
        .iter()
        .map(|g| game_to_latex_string(g, &LatexOptions::default()).unwrap())
        .collect()
}

fn document() -> String {
    let games = fixtures();
    let refs: Vec<&Game> = games.iter().collect();
    let mut out = Vec::new();
    write_latex_document(&mut out, &refs, &LatexOptions::default()).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn document_has_matching_begin_and_end() {
    let tex = document();
    assert!(tex.starts_with("\\documentclass{scrreprt}\n"));
    assert!(tex.ends_with("\\end{document}\n"));
    for env in [
        "document",
        "addmargin",
        "longfbox",
        "tabularx",
        "center",
        "@twocolumnfalse",
    ] {
        assert_eq!(
            tex.matches(&format!("\\begin{{{env}}}")).count(),
            tex.matches(&format!("\\end{{{env}}}")).count(),
            "{env}"
        );
    }
    assert_eq!(tex.matches("\\twocolumn[").count(), 2);
    assert!(
        !tex.contains("pgfplots}\n\\begin{tikzpicture"),
        "no evaluation graph"
    );
    assert!(!tex.contains("axis"));
}

#[test]
fn braces_are_balanced_outside_escapes() {
    let tex = body();
    let mut depth = 0i64;
    let mut prev = '\0';
    for c in tex.chars() {
        match (prev, c) {
            ('\\', _) => {}
            (_, '{') => depth += 1,
            (_, '}') => depth -= 1,
            _ => {}
        }
        assert!(depth >= 0, "closing brace without opening");
        prev = if prev == '\\' { '\0' } else { c };
    }
    assert_eq!(depth, 0);
}

#[test]
fn header_texts_are_escaped() {
    let tex = body();
    assert!(tex.contains("Smith \\& $<$Co$>$"), "{tex}");
    assert!(tex.contains("Rio\\_de \\$Janeiro \\% \\#"));
    assert!(tex.contains("(2500)"));
    assert!(tex.contains("Opening ECO: & B20"));
    assert!(tex.contains("2024.05.01\\\\"));
}

#[test]
fn comment_text_escapes_every_special_character() {
    let tex = body();
    assert!(tex.contains("Good $<$b$>$move$<$/b$>$ \\& \\textasciitilde{} \\textasciicircum{} \\textbackslash{} ok\\\\[1ex]"), "{tex}");
    assert!(!tex.contains("[%cal"));
}

#[test]
fn mainline_and_variations_have_the_xskak_structure() {
    let tex = body();
    assert!(tex.contains("\\mainline{1. e4!"), "{tex}");
    assert!(tex.contains("\\textcolor{VariationColor}{ \\variation{1... e5"));
    // Second level uses the invar option and a nested longfbox.
    assert!(tex.contains("\\variation[invar]{2. f4?!"));
    assert_eq!(tex.matches("\\begin{longfbox}").count(), 2);
    assert!(tex.contains("\\textbf{1-0}"));
}

#[test]
fn nag_without_symbol_uses_xskakcomment_and_diagram_uses_chessboard() {
    let pgn = "[Event \"T\"]\n[Result \"*\"]\n\n1. e4 $14 e5 $201 2. Nf3 $136 *\n";
    let g = &read_games(pgn)[0];
    let tex = game_to_latex_string(g, &LatexOptions::default()).unwrap();
    assert!(tex.contains("e4 \\xskakcomment{\\wbetter}"), "{tex}");
    assert!(tex.contains("\\xskakcomment{\\timelimit}"));
    assert!(tex.contains("\\chessboard[showmover=true,lastmoveid"));
    assert!(
        !tex.contains("tinyboard"),
        "main line diagrams are full size"
    );
}

#[test]
fn diagram_inside_a_variation_restores_the_main_line_position() {
    let pgn = "[Event \"T\"]\n[Result \"*\"]\n\n1. e4 e5 (1... c5 $201) 2. Nf3 *\n";
    let g = &read_games(pgn)[0];
    let tex = game_to_latex_string(g, &LatexOptions::default()).unwrap();
    let new_games: Vec<&str> = tex
        .lines()
        .filter(|l| l.starts_with("\\newchessgame["))
        .collect();
    assert_eq!(new_games.len(), 2, "{tex}");
    assert!(
        new_games[0].contains(
            "moveid=2w,setfen=rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
        )
    );
    // The main line was after 1...e5 when the variation was written.
    assert!(
        new_games[1].contains(
            "moveid=2w,setfen=rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
        )
    );
    assert!(tex.contains(",tinyboard]"));
    assert!(
        !tex.contains("lastmoveid"),
        "no last-move mark in a variation"
    );
}

#[test]
fn start_position_adds_the_fen_line_and_setfen() {
    let g = &read_games(GAME_TWO)[0];
    let tex = game_to_latex_string(g, &LatexOptions::default()).unwrap();
    assert!(tex.contains("Fen: 4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\n\\newchessgame[moveid=1w,setfen=4k3/8/8/8/8/8/4P3/4K3 w - - 0 1]"), "{tex}");
    assert!(tex.contains("\\textbf{*}"));
}

#[test]
fn flip_adds_inverse_to_diagrams() {
    let g = &read_games(GAME_ONE)[0];
    let off = game_to_latex_string(g, &LatexOptions::default()).unwrap();
    assert!(!off.contains("inverse"));
    let on = game_to_latex_string(
        g,
        &LatexOptions {
            flip: true,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    assert!(on.contains(",inverse"));
}

#[test]
fn marks_in_a_diagram_comment_become_chessboard_keys() {
    let pgn = "[Event \"T\"]\n[Result \"*\"]\n\n1. e4 $201 {[%cal Ge2e4] [%csl Rd4]} *\n";
    let g = &read_games(pgn)[0];
    let tex = game_to_latex_string(g, &LatexOptions::default()).unwrap();
    assert!(
        tex.contains(",pgfstyle=straightmove,color=green,markmoves=e2-e4"),
        "{tex}"
    );
    assert!(
        tex.contains(",pgfstyle=circle,padding=-0.1em,color=red,backfields=d4"),
        "{tex}"
    );
}

#[test]
fn castling_move_has_no_last_move_mark() {
    let pgn = "[Event \"T\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O $201 *\n";
    let g = &read_games(pgn)[0];
    let tex = game_to_latex_string(g, &LatexOptions::default()).unwrap();
    assert!(tex.contains("\\chessboard[showmover=true"));
    assert!(!tex.contains("lastmoveid"));
}

#[test]
fn null_move_becomes_a_comment_by_default() {
    let mut g = Game::new();
    let e4 = g
        .add_move(g.root(), g.start_position().parse_uci("e2e4").unwrap())
        .unwrap();
    g.add_move(e4, Move::Null).unwrap();
    let tex = game_to_latex_string(&g, &LatexOptions::default()).unwrap();
    assert!(tex.contains("null move"), "{tex}");
    assert!(!tex.contains("--"));
    let raw = game_to_latex_string(
        &g,
        &LatexOptions {
            convert_null_moves: false,
            ..LatexOptions::default()
        },
    )
    .unwrap();
    assert!(raw.contains("--"));
}

#[test]
fn comments_and_variations_off_drop_the_blocks() {
    let opts = LatexOptions {
        comments: false,
        variations: false,
        ..LatexOptions::default()
    };
    let tex = game_to_latex_string(&read_games(GAME_ONE)[0], &opts).unwrap();
    assert!(!tex.contains("longfbox"));
    assert!(!tex.contains("Good"));
}

#[test]
fn zero_games_gives_the_start_and_end_only() {
    let mut out = Vec::new();
    write_latex_document(&mut out, &[], &LatexOptions::default()).unwrap();
    let tex = String::from_utf8(out).unwrap();
    assert!(tex.contains("\\begin{document}\n\\end{document}\n"));
}

#[test]
fn write_game_latex_matches_the_string_form() {
    let g = &read_games(GAME_ONE)[0];
    let mut out = Vec::new();
    write_game_latex(&mut out, g, &LatexOptions::default()).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        game_to_latex_string(g, &LatexOptions::default()).unwrap()
    );
}

#[test]
fn snapshot_document() {
    insta::assert_snapshot!(document());
}
