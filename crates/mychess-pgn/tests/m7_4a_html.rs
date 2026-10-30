//! M7-4a, criterion 1 and rules 7-9 of `docs/specs/m7-4-exportacao.md`: the
//! HTML writer.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4a_support;

use m7_4a_support::{FakeDiagrams, GAME_ONE, check_balanced, fixtures, read_games};
use mychess_pgn::export::{
    HtmlOptions, NoDiagrams, game_to_html_string, write_game_html, write_html_document,
};

fn document(opts: &HtmlOptions) -> String {
    let games = fixtures();
    let refs: Vec<&_> = games.iter().collect();
    let mut out = Vec::new();
    write_html_document(&mut out, &refs, opts, &mut FakeDiagrams::default()).unwrap();
    String::from_utf8(out).unwrap()
}

fn symbolic() -> HtmlOptions {
    HtmlOptions {
        symbols: true,
        ..HtmlOptions::default()
    }
}

#[test]
fn document_is_complete_and_balanced() {
    let html = document(&symbolic());
    assert!(html.starts_with("<!DOCTYPE html>\n"));
    assert!(html.contains("<meta charset=\"utf-8\">"));
    assert!(html.ends_with("</body>\n</html>\n"));
    assert_eq!(html.matches("<article").count(), 2);
    check_balanced(&html).unwrap();
}

#[test]
fn text_from_the_game_is_escaped() {
    let html = document(&symbolic());
    assert!(
        html.contains("A&amp;B &lt;Open&gt; &quot;Q&quot; &#39;x&#39;"),
        "{html}"
    );
    assert!(html.contains("Smith &amp; &lt;Co&gt;"));
    assert!(html.contains("&lt;b&gt;move&lt;/b&gt;"));
    assert!(html.contains("&lt;/script&gt;"));
    assert!(!html.contains("<b>move</b>"));
    assert!(!html.to_lowercase().contains("</script>"));
}

#[test]
fn comment_is_in_braces_and_marks_are_stripped() {
    let html = document(&symbolic());
    assert!(html.contains("<span class=\"c\">{Good "), "{html}");
    assert!(!html.contains("[%cal"), "{html}");
    assert!(html.contains("ok}</span>"));
}

#[test]
fn variations_are_nested_divs_and_only_the_main_line_is_bold() {
    let html = document(&symbolic());
    assert_eq!(html.matches("<div class=\"var ind\">(").count(), 2);
    // The nested variation (2. f4) is inside the first one.
    let first = html.find("<div class=\"var ind\">(").unwrap();
    let inner = html[first + 5..].find("<div class=\"var ind\">(").unwrap() + first + 5;
    let close = html[first..].find(")</div>").unwrap() + first;
    assert!(inner < close, "nested div is inside the outer one");
    // Moves of the variation are not bold; main line moves are.
    assert!(html.contains("<b>1.e4!</b>"), "{html}");
    assert!(!html.contains("<b>1...e5"));
    // After a variation the black move shows its number again.
    assert!(html.contains("<b>2.Nf3</b>"));
}

#[test]
fn nags_are_symbols_or_numbers() {
    let sym = document(&symbolic());
    assert!(sym.contains("e4!"));
    assert!(sym.contains("f4?!"));
    let num = document(&HtmlOptions::default());
    assert!(num.contains("e4$1"), "{num}");
    assert!(num.contains("f4$6"));
}

#[test]
fn diagram_comes_with_the_fen_title() {
    let games = read_games(GAME_ONE);
    let mut fake = FakeDiagrams::default();
    let html = game_to_html_string(&games[0], &symbolic(), &mut fake).unwrap();
    // One diagram for the NAG D after 1...c5.
    assert_eq!(fake.fens.len(), 1);
    assert_eq!(
        fake.fens[0],
        "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
    );
    assert!(html.contains(&format!("<title>{}</title>", fake.fens[0])));
    assert!(html.contains("<figure class=\"dg\"><svg"));
    assert!(!html.contains("(D)"));
}

#[test]
fn start_position_of_a_fen_game_gets_a_fen_line_and_a_diagram() {
    let games = read_games(m7_4a_support::GAME_TWO);
    let mut fake = FakeDiagrams::default();
    let html = game_to_html_string(&games[0], &HtmlOptions::default(), &mut fake).unwrap();
    assert!(html.contains("<p class=\"fen\">FEN: 4k3/8/8/8/8/8/4P3/4K3 w - - 0 1</p>"));
    assert_eq!(fake.fens.len(), 1);
    assert!(html.contains("<b>1.e4</b>"));
    assert!(html.contains("<b>*</b>"));
}

#[test]
fn zero_diagram_width_turns_diagrams_off() {
    let games = read_games(GAME_ONE);
    let opts = HtmlOptions {
        diagram_px: 0,
        ..symbolic()
    };
    let html = game_to_html_string(&games[0], &opts, &mut NoDiagrams).unwrap();
    assert!(html.contains("<i>(D)</i>"));
    assert!(!html.contains("<figure"));
}

#[test]
fn turning_comments_and_variations_off_removes_the_blocks() {
    let opts = HtmlOptions {
        comments: false,
        variations: false,
        ..symbolic()
    };
    let html = document(&opts);
    assert!(!html.contains("class=\"c\""), "{html}");
    assert!(!html.contains("class=\"var"));
    assert!(!html.contains("Nf3 ("));
    check_balanced(&html).unwrap();
}

#[test]
fn zero_games_gives_just_the_start_and_end() {
    let mut out = Vec::new();
    write_html_document(&mut out, &[], &HtmlOptions::default(), &mut NoDiagrams).unwrap();
    let html = String::from_utf8(out).unwrap();
    assert_eq!(html.matches("<article").count(), 0);
    check_balanced(&html).unwrap();
}

#[test]
fn write_game_html_writes_exactly_the_article() {
    let games = read_games(GAME_ONE);
    let mut out = Vec::new();
    write_game_html(&mut out, &games[0], &symbolic(), &mut NoDiagrams).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("<article class=\"game\">\n<header class=\"gh\">"));
    assert!(text.ends_with("</article>\n"));
}

#[test]
fn output_is_deterministic_and_has_no_nul() {
    let a = document(&symbolic());
    let b = document(&symbolic());
    assert_eq!(a, b);
    assert!(!a.contains('\0'));
}

#[test]
fn unicode_and_emoji_names_pass_through() {
    let pgn = "[Event \"\u{1F600} Torneio S\u{e3}o Paulo\"]\n[White \"Jos\u{e9} \u{4e2d}\"]\n[Result \"*\"]\n\n1. e4 *\n";
    let games = read_games(pgn);
    let html = game_to_html_string(&games[0], &symbolic(), &mut NoDiagrams).unwrap();
    assert!(html.contains("\u{1F600} Torneio S\u{e3}o Paulo"));
    assert!(html.contains("Jos\u{e9} \u{4e2d}"));
}

#[test]
fn snapshot_document() {
    insta::assert_snapshot!(document(&symbolic()));
}
