//! M6-4a: end-to-end tables from PGN fixtures (spec 2.1 and 3.1). The
//! outputs were derived by hand from the format strings of `crosstab.cpp`
//! and checked line by line (D3366, D3395: no Scid to compare with).
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m6_4a_support.rs"]
mod support;

use mychess_db::{
    CancelToken, CrosstableError, CrosstableFormat, CrosstableLabels, CrosstableOptions,
    CrosstableRequest, NoProgress, TableMode, TournamentReference, build_crosstable, render_html,
    render_latex, render_text,
};
use pretty_assertions::assert_eq;
use support::{Game, game, open};

fn table(games: &[Game], reference: u32, options: CrosstableOptions) -> mychess_db::Crosstable {
    let db = open(games);
    let request = CrosstableRequest {
        reference: TournamentReference::from_game(&db, reference).unwrap(),
        options,
    };
    build_crosstable(&db, &request, None, &mut NoProgress, &CancelToken::new()).unwrap()
}

/// Compares `actual` with the reviewed file `tests/fixtures/m6_4a/<name>`.
/// Run with `MYCHESS_UPDATE_GOLDEN=1` to write the file, then read the diff:
/// the files are the expected output, never accepted blindly.
fn golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m6_4a")
        .join(name);
    if std::env::var_os("MYCHESS_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(actual, expected, "golden file {name}");
}

fn labels() -> CrosstableLabels {
    CrosstableLabels::default()
}

/// Four rated players, all-play-all, one draw and one unfinished game.
fn rated_round_robin() -> Vec<Game> {
    vec![
        game("Ana", "Bia", "1-0").elo(2500, 2400),
        game("Cid", "Ana", "1/2-1/2").elo(2300, 2500),
        game("Bia", "Cid", "0-1").elo(2400, 2300),
        game("Ana", "Dan", "1-0").elo(2500, 0),
        game("Dan", "Bia", "*").elo(0, 2400),
    ]
}

#[test]
fn rated_all_play_all_in_text() {
    let t = table(&rated_round_robin(), 1, CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::AllPlayAll);
    golden("rated_apa.txt", &render_text(&t, &labels()));
}

#[test]
fn rated_all_play_all_in_html_and_latex() {
    let t = table(&rated_round_robin(), 1, CrosstableOptions::default());
    golden("rated_apa.html", &render_html(&t, &labels()));
    golden("rated_apa.tex", &render_latex(&t, &labels()));
}

#[test]
fn rated_table_as_swiss_without_rounds() {
    let options = CrosstableOptions {
        format: CrosstableFormat::Swiss,
        ..CrosstableOptions::default()
    };
    let t = table(&rated_round_robin(), 1, options);
    golden("rated_swiss.txt", &render_text(&t, &labels()));
    golden("rated_swiss.html", &render_html(&t, &labels()));
    golden("rated_swiss.tex", &render_latex(&t, &labels()));
}

fn swiss_games() -> Vec<Game> {
    let pairs: [(&str, &str, &str, &str); 12] = [
        ("Ana", "Bia", "1-0", "1"),
        ("Cid", "Dan", "1/2-1/2", "1"),
        ("Eva", "Fred", "0-1", "1"),
        ("Gil", "Hugo", "1-0", "1"),
        ("Fred", "Ana", "0-1", "2"),
        ("Gil", "Cid", "1/2-1/2", "2"),
        ("Dan", "Eva", "1-0", "2"),
        ("Hugo", "Bia", "*", "2"),
        ("Ana", "Gil", "1/2-1/2", "3"),
        ("Cid", "Fred", "1-0", "3"),
        ("Dan", "Hugo", "0-1", "3"),
        ("Bia", "Eva", "1-0", "?"),
    ];
    pairs
        .iter()
        .map(|&(w, b, r, round)| game(w, b, r).round(round))
        .collect()
}

#[test]
fn swiss_with_missing_rounds_and_an_unnumbered_game() {
    // Auto: eight players are at most 12, so all-play-all.
    let auto = table(&swiss_games(), 1, CrosstableOptions::default());
    assert_eq!(auto.mode, TableMode::AllPlayAll);
    let options = CrosstableOptions {
        format: CrosstableFormat::Swiss,
        show_tiebreaks: true,
        ..CrosstableOptions::default()
    };
    let t = table(&swiss_games(), 1, options);
    assert_eq!(t.max_round, 3);
    golden("swiss.txt", &render_text(&t, &labels()));
    golden("swiss.tex", &render_latex(&t, &labels()));
    golden("swiss.html", &render_html(&t, &labels()));
}

#[test]
fn knockout_from_dotted_rounds() {
    let games = vec![
        game("Ana", "Bia", "1-0").round("1.1"),
        game("Cid", "Dan", "0-1").round("1.2"),
        game("Dan", "Ana", "1/2-1/2").round("2.1"),
        game("Ana", "Dan", "1-0").round("2.1"),
    ];
    let t = table(&games, 1, CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::Knockout);
    golden("knockout.txt", &render_text(&t, &labels()));
    golden("knockout.tex", &render_latex(&t, &labels()));
    golden("knockout.html", &render_html(&t, &labels()));
}

#[test]
fn names_with_special_characters_are_escaped() {
    let games = vec![
        game("Smith & Sons <A>", "50% \"B\"_1", "1-0").event("R&D"),
        game("50% \"B\"_1", "C\\D", "1/2-1/2").event("R&D"),
    ];
    let t = table(&games, 1, CrosstableOptions::default());
    let html = render_html(&t, &labels());
    assert!(html.contains("Smith &amp; Sons &lt;A&gt; "));
    assert!(html.contains("50% &quot;B&quot;_1"));
    assert!(!html.contains("<A>"));
    let latex = render_latex(&t, &labels());
    assert!(latex.contains("Smith \\& Sons \\textless{}A\\textgreater{}"));
    assert!(latex.contains("50\\% \"B\"\\_1"));
    assert!(latex.contains("C\\textbackslash{}D"));
    assert!(!latex.contains("<A>"));
    golden("escapes.html", &html);
    golden("escapes.tex", &latex);
    golden("escapes.txt", &render_text(&t, &labels()));
}

#[test]
fn round_robin_with_round_and_board_numbers_is_not_a_knockout() {
    // TWIC and ChessBase write Round as `round.board`.
    let games = vec![
        game("Ana", "Bia", "1-0").round("1.1"),
        game("Cid", "Dan", "1/2-1/2").round("1.2"),
        game("Ana", "Cid", "1/2-1/2").round("2.1"),
        game("Bia", "Dan", "0-1").round("2.2"),
        game("Ana", "Dan", "1-0").round("3.1"),
        game("Bia", "Cid", "1/2-1/2").round("3.2"),
    ];
    let t = table(&games, 1, CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::AllPlayAll);
    assert_eq!(t.knockout, vec![]);
    assert!(render_text(&t, &labels()).contains("Ana"));
}

#[test]
fn rounds_that_are_dates_never_give_an_empty_knockout() {
    let games = vec![
        game("Ana", "Bia", "1-0").round("0.5"),
        game("Cid", "Dan", "0-1").round("2020.05.01"),
    ];
    let t = table(&games, 1, CrosstableOptions::default());
    assert_ne!(t.mode, TableMode::Knockout);
    assert!(render_text(&t, &labels()).contains("Dan"));
}

#[test]
fn error_when_there_is_no_table() {
    let db = open(&[game("Ana", "Ana", "1-0")]);
    let request = CrosstableRequest {
        reference: TournamentReference::from_game(&db, 1).unwrap(),
        options: CrosstableOptions {
            format: CrosstableFormat::Swiss,
            ..CrosstableOptions::default()
        },
    };
    let result = build_crosstable(&db, &request, None, &mut NoProgress, &CancelToken::new());
    assert!(matches!(result, Err(CrosstableError::NoTable)));
}

#[test]
fn the_filter_does_not_matter_and_names_come_from_the_whole_base() {
    // The selection reads the whole database: there is no filter parameter.
    let mut games = rated_round_robin();
    games.push(game("Zed", "Yan", "1-0").event("Other"));
    let t = table(&games, 1, CrosstableOptions::default());
    assert_eq!(t.total_players, 4);
    assert_eq!(t.game_count, 5);
}
