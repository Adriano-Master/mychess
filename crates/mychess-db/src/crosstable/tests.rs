//! Unit tests of the table: the examples of the spec (section 2.1) and the
//! rules T1 to T29 that do not need a database.

use std::collections::HashMap;

use mychess_core::{Color, GameResult, PartialDate};
use pretty_assertions::assert_eq;

use super::*;
use crate::progress::CancelToken;

const W: GameResult = GameResult::White;
const B: GameResult = GameResult::Black;
const D: GameResult = GameResult::Draw;
const N: GameResult = GameResult::Unknown;

fn date(text: &str) -> PartialDate {
    PartialDate::parse_lenient(text)
}

fn reference() -> TournamentReference {
    TournamentReference {
        event: "Club".into(),
        site: "Rio".into(),
        event_raw: Vec::new(),
        site_raw: Vec::new(),
        date: date("2020.05.01"),
        event_date: PartialDate::UNKNOWN,
    }
}

/// A game between players given by ID; the number is set by [`data`].
fn g(white: u32, black: u32, result: GameResult, round: u32) -> TournamentGame {
    TournamentGame {
        number: 0,
        white,
        black,
        result,
        round,
        sub_round: false,
        date: date("2020.05.01"),
        white_elo: 0,
        black_elo: 0,
    }
}

fn elo(mut game: TournamentGame, white: u16, black: u16) -> TournamentGame {
    game.white_elo = white;
    game.black_elo = black;
    game
}

/// Names the players `A`, `B`... by ID (A = 0) and numbers the games.
fn data(games: Vec<TournamentGame>) -> TournamentGames {
    let mut names = HashMap::new();
    let games = games
        .into_iter()
        .zip(1u32..)
        .map(|(mut game, number)| {
            game.number = number;
            for id in [game.white, game.black] {
                let letter = u8::try_from(id % 26).unwrap();
                let name = if id < 26 {
                    char::from(b'A' + letter).to_string()
                } else {
                    format!("P{id}")
                };
                names.insert(id, name);
            }
            game
        })
        .collect();
    TournamentGames {
        reference: reference(),
        games,
        names,
        unreadable: 0,
        truncated: false,
    }
}

fn build(games: Vec<TournamentGame>, options: &CrosstableOptions) -> Crosstable {
    Crosstable::from_games(&data(games), options, None).unwrap()
}

fn text(t: &Crosstable) -> String {
    render_text(t, &CrosstableLabels::default())
}

/// The all-play-all of the spec: A-B 1-0, A-C 1/2, B-C 1-0.
fn apa() -> Vec<TournamentGame> {
    vec![g(0, 1, W, 0), g(0, 2, D, 0), g(1, 2, W, 0)]
}

/// The Swiss of the spec: R1 A-B 1-0, C-D 1/2; R2 A-C 1/2, B-D 1-0.
fn swiss() -> Vec<TournamentGame> {
    vec![g(0, 1, W, 1), g(2, 3, D, 1), g(0, 2, D, 2), g(1, 3, W, 2)]
}

fn swiss_options() -> CrosstableOptions {
    CrosstableOptions {
        format: CrosstableFormat::Swiss,
        ..CrosstableOptions::default()
    }
}

#[test]
fn all_play_all_text_matches_the_spec_example() {
    let t = build(apa(), &CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::AllPlayAll);
    let expected = "Club\n\
Rio, 2020.05.01\n\
\x20        Score     A B C\n\
-------------------------\n\
\x201: A   1.5 /  2   . 1 ½  (+1 -0 =1)\n\
\x202: B   1.0 /  2   0 . 1  (+1 -1 =0)\n\
\x203: C   0.5 /  2   ½ 0 .  (+0 -1 =1)\n\
-------------------------\n\
3 games: +2 -0 =1\n";
    assert_eq!(text(&t), expected);
}

#[test]
fn swiss_text_matches_the_spec_example() {
    let t = build(swiss(), &swiss_options());
    // B and C tie on points, games and Buchholz: first appearance wins.
    let expected = "Club\n\
Rio, 2020.05.01\n\
\x20        Score       1    2 \n\
-----------------------------\n\
\x201: A   1.5 /  2    2w+  3w=  (+1 -0 =1)\n\
\x202: B   1.0 /  2    1b-  4w+  (+1 -1 =0)\n\
\x203: C   1.0 /  2    4w=  1b=  (+0 -0 =2)\n\
\x204: D   0.5 /  2    3b=  2b-  (+0 -1 =1)\n\
-----------------------------\n\
4 games: +2 -0 =2\n";
    assert_eq!(text(&t), expected);
}

#[test]
fn sonneborn_berger_and_buchholz_of_the_spec() {
    let options = CrosstableOptions {
        show_tiebreaks: true,
        ..CrosstableOptions::default()
    };
    let t = build(apa(), &options);
    // A: beat B (1.0 = 2 half points) -> 2*2, drew C (1 half point) -> 1.
    assert_eq!(t.rows[0].tiebreak, 5);
    assert!(text(&t).contains(" 1: A   1.5 /  2    1.25  . 1 ½"));
    let s = build(
        swiss(),
        &CrosstableOptions {
            show_tiebreaks: true,
            ..swiss_options()
        },
    );
    // A: B (1.0) + C (1.0) = 2.0 points = 4 half points.
    assert_eq!(s.rows[0].tiebreak, 4);
    assert!(text(&s).contains(" 1: A   1.5 /  2   2.0   2w+  3w="));
}

#[test]
fn html_matches_the_spec_example() {
    let t = build(apa(), &CrosstableOptions::default());
    let html = render_html(&t, &CrosstableLabels::default());
    let expected = "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
<title>Club</title>\n</head>\n<body>\n\
Club<br>\nRio, 2020.05.01<br>\n\
\n<p><table border=1 cellspacing=0 cellpadding=4>\n\
<tr><th></th> <th>Player</th>    <th> Score </th>    <th>A</th> <th>B</th> <th>C</th>   <th>+/-/=</th></tr>\n\
<tr><td align=right> 1: </td><td>A </td><td align=right>  1.5 /  2  </td> <td align=right>.</td> <td align=right>1</td> <td align=right>½</td><td>  (+1 -0 =1)</td></tr>\n\
<tr><td align=right> 2: </td><td>B </td><td align=right>  1.0 /  2  </td> <td align=right>0</td> <td align=right>.</td> <td align=right>1</td><td>  (+1 -1 =0)</td></tr>\n\
<tr><td align=right> 3: </td><td>C </td><td align=right>  0.5 /  2  </td> <td align=right>½</td> <td align=right>0</td> <td align=right>.</td><td>  (+0 -1 =1)</td></tr>\n\
</table></p>\n\
3 games: +2 -0 =1\n\
</body>\n</html>\n";
    assert_eq!(html, expected);
}

#[test]
fn latex_matches_the_spec_example() {
    let t = build(apa(), &CrosstableOptions::default());
    let latex = render_latex(&t, &CrosstableLabels::default());
    let expected = "\\documentclass{scrartcl}\n\n\
\\usepackage[T1]{fontenc}\n\
\\usepackage[utf8]{inputenc}\n\
\\usepackage{booktabs}% provides toprule, midrule, bottomrule\n\
\\usepackage{longtable}\n\n\
\\begin{document}\n\n\
\\setlength{\\parindent}{0cm}\n\
\\small\n\
Club\\\\\n\
Rio, 2020.05.01\\\\\n\
\\begin{longtable}{rlr@{/}rc@{ }c@{ }cr}\n\
\\toprule\n\
\x20 & \\bfseries Player &  \\multicolumn{2}{c}{\\bfseries Score} &  \\ttfamily A & \\ttfamily B & \\ttfamily C & \\\\\n\
\\midrule\n\
\\endhead\n\
\x201:  & A  &   1.5  &  2   & $\\bullet$ & 1 & ½ &  (+1 -0 =1) \\\\\n\
\x202:  & B  &   1.0  &  2   & 0 & $\\bullet$ & 1 &  (+1 -1 =0) \\\\\n\
\x203:  & C  &   0.5  &  2   & ½ & 0 & $\\bullet$ &  (+0 -1 =1) \\\\\n\
\\bottomrule\n\
\\end{longtable}\n\
3 games: +2 -0 =1\n\
\n\\end{document}\n";
    assert_eq!(latex, expected);
}

#[test]
fn swiss_latex_has_no_extra_ampersand_at_the_end_of_a_row() {
    let t = build(swiss(), &swiss_options());
    let latex = render_latex(&t, &CrosstableLabels::default());
    assert!(
        latex.contains("\\begin{longtable}{rlr@{ / }rr@{ }rr}\n"),
        "{latex}"
    );
    assert!(latex.contains(" 1:  &"));
    assert!(
        latex.contains("3w=   (+1 -0 =1) \\\\\n") || latex.contains("3w= &  (+1 -0 =1) \\\\\n")
    );
    assert!(!latex.contains("=1) &"));
}

#[test]
fn auto_picks_the_format_by_best_mode() {
    let all_play_all = |players: u32, games: u32| {
        // `games` games between consecutive players, cycling.
        let list = (0..games)
            .map(|i| {
                let a = i % players;
                let b = (i + 1 + i / players) % players;
                let b = if a == b { (b + 1) % players } else { b };
                g(a, b, D, 0)
            })
            .collect();
        build(list, &CrosstableOptions::default()).mode
    };
    // 12 players: all-play-all, whatever the games.
    assert_eq!(all_play_all(12, 12), TableMode::AllPlayAll);
    // 31 players: Swiss.
    assert_eq!(all_play_all(31, 62), TableMode::Swiss);
    // 20 players and 3 games per player (60 games): Swiss.
    assert_eq!(all_play_all(20, 60), TableMode::Swiss);
    // 8 players: <= 12 so all-play-all; BestMode needs more than 12 players
    // for the rule of "less than half": 13 players, 5 per player, 20 games
    // is below half of 78 = 39/2.
    assert_eq!(all_play_all(13, 65), TableMode::AllPlayAll);
}

#[test]
fn best_mode_rules_for_more_than_twelve_players() {
    // 14 players: complete all-play-all is 91 games. 5 per player = 70.
    let games: Vec<_> = (0..14u32)
        .flat_map(|a| (a + 1..14).map(move |b| (a, b)))
        .map(|(a, b)| g(a, b, D, 0))
        .collect();
    assert_eq!(games.len(), 91);
    let t = build(games.clone(), &CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::AllPlayAll);
    // Only 40 games (< 45): "less than half" gives Swiss, and 40/14 = 2 < 5
    // gives Swiss too.
    let few: Vec<_> = games.into_iter().take(40).collect();
    assert_eq!(
        build(few, &CrosstableOptions::default()).mode,
        TableMode::Swiss
    );
}

#[test]
fn order_points_then_fewer_games_then_tiebreak() {
    // A beat C, D beat A, B beat D: B has 1.0 in one game; A and D have 1.0
    // in two games and D wins on Sonneborn-Berger (it beat A, who has 1.0).
    let games = vec![g(0, 2, W, 0), g(0, 3, B, 0), g(1, 3, W, 0)];
    let t = build(games, &CrosstableOptions::default());
    let order: Vec<&str> = t.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(order, ["B", "D", "A", "C"]);
}

fn names(t: &Crosstable) -> Vec<String> {
    t.rows.iter().map(|r| r.name.clone()).collect()
}

#[test]
fn head_to_head_option_goes_before_the_tiebreak() {
    // A beat B, X beat A, B beat Z, Z beat W and V. A and B have 1.0 in two
    // games; B has the better Sonneborn-Berger (it beat Z, with 2.0), but A
    // beat B and scores against players on its own points.
    let games = vec![
        g(0, 1, W, 0),
        g(2, 0, W, 0),
        g(1, 3, W, 0),
        g(3, 4, W, 0),
        g(3, 5, W, 0),
    ];
    let plain = build(games.clone(), &CrosstableOptions::default());
    assert_eq!(names(&plain), ["D", "C", "B", "A", "E", "F"]);
    let head = build(
        games,
        &CrosstableOptions {
            tiebreak_head_to_head: true,
            ..CrosstableOptions::default()
        },
    );
    assert_eq!(names(&head), ["D", "C", "A", "B", "E", "F"]);
}

#[test]
fn wins_option_goes_before_the_tiebreak() {
    // P (A) draws twice; Q (B) beats R (C) and loses to T (E). Both have
    // 1.0 in two games and the same Sonneborn-Berger, so first appearance
    // decides unless wins count.
    let games = vec![g(0, 2, D, 0), g(0, 3, D, 0), g(1, 2, W, 0), g(4, 1, W, 0)];
    let plain = build(games.clone(), &CrosstableOptions::default());
    assert_eq!(names(&plain), ["E", "A", "B", "D", "C"]);
    let wins = build(
        games,
        &CrosstableOptions {
            tiebreak_wins: true,
            ..CrosstableOptions::default()
        },
    );
    assert_eq!(names(&wins), ["E", "B", "A", "D", "C"]);
}

#[test]
fn three_points_for_a_win_changes_only_the_score() {
    let options = CrosstableOptions {
        three_points: true,
        ..CrosstableOptions::default()
    };
    let t = build(apa(), &options);
    assert_eq!(t.rows[0].score_half, 6 + 2);
    assert_eq!(t.rows[1].score_half, 6);
    let out = text(&t);
    assert!(out.contains(" 1: A   4.0  "), "{out}");
    assert!(out.contains(" 2: B   3.0  "), "{out}");
    assert!(!out.contains(" / "));
}

#[test]
fn games_without_result_count_as_games_but_not_points() {
    let t = build(
        vec![g(0, 1, N, 0), g(0, 1, W, 0)],
        &CrosstableOptions::default(),
    );
    assert_eq!(t.game_count, 2);
    assert_eq!(t.rows[0].games, 2);
    assert_eq!(t.rows[0].score_half, 2);
    assert_eq!(t.rows[0].won + t.rows[0].lost + t.rows[0].drawn, 1);
    assert_eq!(t.max_clashes, 2);
    let out = text(&t);
    assert!(out.ends_with("2 games: +1 -0 =0 *1\n"), "{out}");
    // Two columns for the two games between the pair.
    assert!(out.contains(" 1: A   1.0 /  2   *1  (+1 -0 =0)"), "{out}");
}

#[test]
fn two_players_have_a_single_column_with_digits() {
    let t = build(
        vec![g(0, 1, W, 0), g(1, 0, D, 0)],
        &CrosstableOptions::default(),
    );
    let out = text(&t);
    let expected = "Club\nRio, 2020.05.01\n\
\x20        Score     12\n\
----------------------\n\
\x201: A   1.5 /  2   1½  (+1 -0 =1)\n\
\x202: B   0.5 /  2   0½  (+0 -1 =1)\n\
----------------------\n\
2 games: +1 -0 =1\n";
    assert_eq!(out, expected);
}

#[test]
fn repeated_games_between_the_same_pair_fill_the_cells_in_game_order() {
    let t = build(
        vec![g(0, 1, W, 0), g(1, 0, W, 0), g(0, 1, D, 0)],
        &CrosstableOptions::default(),
    );
    let a = &t.rows[0];
    let versus: Vec<char> = a.versus[1]
        .iter()
        .map(|c| c.outcome.result_char())
        .collect();
    assert_eq!(versus, ['1', '0', '=']);
    assert_eq!(a.versus[1][1].color, Color::Black);
}

#[test]
fn performance_and_rating_change_with_the_margin_of_350() {
    // A (2500) draws... A scores 1.5/2 against B (2400) and C (2900).
    let games = vec![
        elo(g(0, 1, W, 0), 2500, 2400),
        elo(g(0, 2, D, 0), 2500, 2900),
    ];
    let t = build(games, &CrosstableOptions::default());
    let a = &t.rows[0];
    // OpponentElo(2500, 2400) = 2400; OpponentElo(2500, 2900) = 2850.
    // average (2400 + 2850) / 2 = 2625; score 3 of 4 half points: 75%.
    let perf = a.performance.unwrap();
    assert_eq!(perf.average_opponent, 2625);
    assert_eq!(perf.percentage, 75);
    assert_eq!(perf.rating, 2625 + 193);
    assert_eq!(perf.change, Some(rating_change(2500, 2625, 75, 2)));
    assert_eq!(t.average_elo, (2500 + 2400 + 2900) / 3);
    assert_eq!(t.category, fide_category(t.average_elo));
    let out = text(&t);
    assert!(
        out.contains("Average Rating: 2600  (Category 14)\n"),
        "{out}"
    );
    assert!(out.contains("Perf Chg"));
}

#[test]
fn player_without_elo_gets_performance_without_change() {
    let t = build(
        vec![elo(g(0, 1, W, 0), 0, 2000)],
        &CrosstableOptions::default(),
    );
    let a = &t.rows[0];
    let perf = a.performance.unwrap();
    assert_eq!(perf.average_opponent, 2000);
    assert_eq!(perf.change, None);
    assert!(text(&t).contains(&format!("{:4}    ", perf.rating)));
}

#[test]
fn no_rated_opponent_gives_blank_performance_cells() {
    // B has Elo, A does not; the game is not decided for C.
    let games = vec![elo(g(0, 1, W, 0), 0, 1800), elo(g(2, 0, N, 0), 0, 0)];
    let t = build(games, &CrosstableOptions::default());
    let c = t.rows.iter().find(|r| r.name == "C").unwrap();
    assert_eq!(c.rated_games, 0);
    assert!(c.performance.is_none());
    // `*` does not count toward the performance of A either.
    let a = t.rows.iter().find(|r| r.name == "A").unwrap();
    assert_eq!(a.rated_games, 1);
}

#[test]
fn elo_is_the_highest_of_the_tournament_games() {
    let games = vec![elo(g(0, 1, W, 0), 2000, 0), elo(g(0, 2, W, 0), 2100, 0)];
    let t = build(games, &CrosstableOptions::default());
    assert_eq!(t.rows[0].elo, 2100);
}

#[test]
fn swiss_missing_round_and_non_numeric_round() {
    // Round 1 and 3 only, plus a game with round 0 that still scores.
    let games = vec![g(0, 1, W, 1), g(0, 2, W, 3), g(1, 2, D, 0)];
    let t = build(games, &swiss_options());
    assert_eq!(t.max_round, 3);
    let out = text(&t);
    assert!(out.contains("2w+ ....  3w+"), "{out}");
    let a = &t.rows[0];
    assert!(a.by_round[1].is_none());
    // The unnumbered game counts for the points and the total.
    let b = t.rows.iter().find(|r| r.name == "B").unwrap();
    assert_eq!(b.games, 2);
    assert_eq!(b.score_half, 1);
    assert_eq!(t.game_count, 3);
    assert_eq!(b.by_round.iter().flatten().count(), 1);
}

#[test]
fn swiss_two_games_in_a_round_show_the_last_but_both_score() {
    let games = vec![g(0, 1, W, 1), g(1, 0, W, 1)];
    let t = build(games, &swiss_options());
    let a = &t.rows[0];
    assert_eq!(a.score_half, 2);
    assert_eq!(a.games, 2);
    // Both are 1.0 and tied; A appears first. Its round-1 cell is the last
    // game (A lost with Black... A had Black and lost).
    let cell = a.by_round[0].unwrap();
    assert_eq!(cell.game, 2);
    assert_eq!(cell.outcome, Outcome::Loss);
}

#[test]
fn swiss_without_numeric_rounds_has_no_round_columns() {
    let t = build(vec![g(0, 1, W, 0), g(1, 2, W, 0)], &swiss_options());
    assert_eq!(t.max_round, 0);
    assert!(t.rows.iter().all(|r| r.by_round.is_empty()));
}

#[test]
fn round_60_or_more_is_not_in_the_grid() {
    let t = build(vec![g(0, 1, W, 60), g(0, 2, W, 59)], &swiss_options());
    assert_eq!(t.max_round, 59);
}

#[test]
fn colour_information_can_be_turned_off() {
    let options = CrosstableOptions {
        swiss_colors: false,
        ..swiss_options()
    };
    let out = text(&build(swiss(), &options));
    assert!(
        out.contains(" 1: A   1.5 /  2    2+  3=  (+1 -0 =1)"),
        "{out}"
    );
    let head = "         Score      1   2 ";
    assert!(out.contains(head), "{out}");
}

#[test]
fn numbered_columns_replace_the_names() {
    let options = CrosstableOptions {
        numbered_columns: true,
        ..CrosstableOptions::default()
    };
    let out = text(&build(apa(), &options));
    assert!(out.contains("Score     1 2 3\n"), "{out}");
}

#[test]
fn separate_score_groups_adds_blank_lines() {
    let options = CrosstableOptions {
        separate_score_groups: true,
        ..CrosstableOptions::default()
    };
    let out = text(&build(apa(), &options));
    // Three different scores: two blank lines between the rows.
    assert_eq!(out.matches("\n\n").count(), 2, "{out}");
}

#[test]
fn sort_by_name_rating_and_country() {
    let games = vec![
        elo(g(2, 1, W, 0), 2000, 2100),
        elo(g(1, 0, W, 0), 2100, 1900),
    ];
    let by = |sort| {
        let t = build(
            games.clone(),
            &CrosstableOptions {
                sort,
                ..CrosstableOptions::default()
            },
        );
        t.rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(by(CrosstableSort::Name), ["A", "B", "C"]);
    assert_eq!(by(CrosstableSort::Rating), ["B", "C", "A"]);
    // Equal (empty) countries: first appearance (C, B, A).
    assert_eq!(by(CrosstableSort::Country), ["C", "B", "A"]);
}

#[test]
fn sort_is_stable_for_complete_ties() {
    let t = build(
        vec![g(3, 2, D, 0), g(1, 0, D, 0)],
        &CrosstableOptions::default(),
    );
    let order: Vec<&str> = t.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(order, ["D", "C", "B", "A"]);
}

#[test]
fn same_player_on_both_sides_is_ignored() {
    let t = build(
        vec![g(0, 0, W, 0), g(0, 1, W, 0)],
        &CrosstableOptions::default(),
    );
    assert_eq!(t.game_count, 1);
}

#[test]
fn fewer_than_two_players_is_no_table() {
    let none = Crosstable::from_games(&data(Vec::new()), &CrosstableOptions::default(), None);
    assert!(matches!(none, Err(CrosstableError::NoTable)));
    let same = Crosstable::from_games(
        &data(vec![g(0, 0, W, 0)]),
        &CrosstableOptions::default(),
        None,
    );
    assert!(matches!(same, Err(CrosstableError::NoTable)));
}

#[test]
fn more_than_1000_players_drops_games_and_warns_once() {
    // 1001 players: game i is (2i, 2i+1) giving 1002 players for 501 games;
    // use chained games instead.
    let games: Vec<_> = (0..1001u32).map(|i| g(i, i + 1, D, 0)).collect();
    let t = build(games, &swiss_options());
    assert_eq!(t.total_players, MAX_PLAYERS);
    assert_eq!(t.notices, [Notice::PlayerLimit]);
    assert!(text(&t).starts_with("Player limit of 1000 reached. Table is incomplete\n\nClub\n"));
}

#[test]
fn all_play_all_shows_at_most_300_players_and_warns() {
    let games: Vec<_> = (0..350u32).map(|i| g(i, i + 1, W, 0)).collect();
    let t = build(
        games,
        &CrosstableOptions {
            format: CrosstableFormat::AllPlayAll,
            ..CrosstableOptions::default()
        },
    );
    assert_eq!(t.total_players, 351);
    assert_eq!(t.rows.len(), ALL_PLAY_ALL_LIMIT);
    assert_eq!(t.notices, [Notice::TooManyPlayers]);
    assert_eq!(t.number_width, 3);
    assert!(text(&t).starts_with("Warning: Too many players for all-play-all;"));
}

fn knockout_games() -> Vec<TournamentGame> {
    let mut games = vec![
        // Round 1: A-B, C-D (A and C with White).
        g(0, 1, W, 1),
        g(2, 3, B, 1),
        // Round 2: D-A, two games, Black in the first game of the pair.
        g(3, 0, D, 2),
        g(0, 3, W, 2),
    ];
    for game in &mut games {
        game.sub_round = true;
    }
    games
}

#[test]
fn auto_chooses_knockout_only_when_every_round_is_dotted() {
    let t = build(knockout_games(), &CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::Knockout);
    let mut mixed = knockout_games();
    if let Some(first) = mixed.first_mut() {
        first.sub_round = false;
    }
    assert_ne!(
        build(mixed, &CrosstableOptions::default()).mode,
        TableMode::Knockout
    );
}

#[test]
fn knockout_prints_one_line_per_match_with_the_white_player_on_the_left() {
    let t = build(knockout_games(), &CrosstableOptions::default());
    assert_eq!(t.knockout.len(), 2);
    assert_eq!(t.knockout[0].matches.len(), 2);
    let second_round = &t.knockout[1].matches;
    assert_eq!(second_round.len(), 1);
    // D had White in the first game of the pair D-A: D on the left.
    let m = &second_round[0];
    assert_eq!(t.rows[m.first].name, "D");
    assert_eq!(t.rows[m.second].name, "A");
    // D drew, then lost: 1 half point of 2 games.
    assert_eq!(m.score_half, 1);
    let out = text(&t);
    assert!(out.contains("1:\n"), "{out}");
    assert!(out.contains("2:\n"), "{out}");
    assert!(out.contains("D "), "{out}");
}

#[test]
fn knockout_score_counts_a_game_without_result_as_half_a_point() {
    let mut games = vec![g(0, 1, N, 1)];
    games[0].sub_round = true;
    let t = build(games, &CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::Knockout);
    assert_eq!(t.knockout[0].matches[0].score_half, 1);
}

#[test]
fn knockout_has_no_tiebreaks_or_tallies() {
    let t = build(
        knockout_games(),
        &CrosstableOptions {
            show_tiebreaks: true,
            ..CrosstableOptions::default()
        },
    );
    assert!(!t.columns.tiebreaks);
    assert!(
        t.rows
            .iter()
            .all(|r| r.tiebreak == 0 && r.won == 0 && r.lost == 0)
    );
    assert!(!text(&t).contains("(+"));
}

#[test]
fn names_are_escaped_in_html_and_latex() {
    let mut data = data(vec![g(0, 1, W, 0)]);
    data.names.insert(0, "A&B <x> \"q\"".into());
    data.names.insert(1, "C_D \\ 50% #1".into());
    data.reference.event = "R&D <Open>".into();
    let t = Crosstable::from_games(&data, &CrosstableOptions::default(), None).unwrap();
    let html = render_html(&t, &CrosstableLabels::default());
    assert!(html.contains("A&amp;B &lt;x&gt; &quot;q&quot;"));
    assert!(html.contains("<title>R&amp;D &lt;Open&gt;</title>"));
    assert!(!html.contains("<x>"));
    let latex = render_latex(&t, &CrosstableLabels::default());
    assert!(latex.contains("C\\_D \\textbackslash{} 50\\% \\#1"));
    assert!(latex.contains("A\\&B \\textless{}x\\textgreater{} \"q\""));
    assert!(latex.contains("R\\&D \\textless{}Open\\textgreater{}\\\\\n"));
    assert!(!latex.contains('<') && !latex.contains('>'));
    assert!(latex.contains("\\usepackage[T1]{fontenc}\n\\usepackage[utf8]{inputenc}\n"));
    // The plain text keeps the names as they are.
    assert!(text(&t).contains("A&B <x> \"q\""));
}

#[test]
fn header_dates_use_the_range_of_the_games_seen() {
    let mut d = data(vec![g(0, 1, W, 0), g(0, 2, W, 0)]);
    d.games[1].date = date("2020.07.??");
    let t = Crosstable::from_games(&d, &CrosstableOptions::default(), None).unwrap();
    assert!(text(&t).starts_with("Club\nRio, 2020.05.01 - 2020.07\n"));
    // An unknown date shows as ????.
    d.games[0].date = PartialDate::UNKNOWN;
    let t = Crosstable::from_games(&d, &CrosstableOptions::default(), None).unwrap();
    assert!(
        text(&t).starts_with("Club\nRio, ???? - 2020.07\n"),
        "{}",
        text(&t)
    );
}

struct Bios;

impl PlayerBioProvider for Bios {
    fn bio(&self, name: &str) -> Option<PlayerBio> {
        (name == "A").then(|| PlayerBio {
            title: "gm".into(),
            country: "BRA".into(),
            birth: date("2000.06.15"),
        })
    }
}

#[test]
fn bio_adds_title_country_and_age_columns() {
    let t =
        Crosstable::from_games(&data(apa()), &CrosstableOptions::default(), Some(&Bios)).unwrap();
    assert!(t.columns.titles && t.columns.countries && t.columns.ages);
    let a = &t.rows[0];
    assert_eq!((a.title.as_str(), a.country.as_str()), ("gm", "BRA"));
    // 2020 - 2000, minus one: the birth month (6) is after May.
    assert_eq!(a.age, Some(19));
    let out = text(&t);
    assert!(out.contains("  Ti Age Nat"), "{out}");
    assert!(out.contains(" 1: A  "), "{out}");
    assert!(out.contains(" gm  19 BRA "), "{out}");
}

#[test]
fn options_can_hide_the_columns() {
    let options = CrosstableOptions {
        show_titles: false,
        show_countries: false,
        show_ages: false,
        show_tallies: false,
        ..CrosstableOptions::default()
    };
    let t = Crosstable::from_games(&data(apa()), &options, Some(&Bios)).unwrap();
    assert!(!t.columns.titles && !t.columns.tallies);
    assert!(!text(&t).contains("(+"));
}

#[test]
fn numbered_header_digit_for_hundreds_is_right() {
    assert_eq!(format::render_numbered_for_tests(123, 3), "123");
    assert_eq!(format::render_numbered_for_tests(7, 3), "  7");
}

#[test]
fn opponent_elo_uses_the_rating_known_when_the_game_is_added() {
    // D3395: as in the Scid, the Elo of a player grows with the games read.
    // In game 1 A has no Elo yet: the opponent counts in full (2800); in
    // game 2 A has 2400: the opponent is limited to 2750.
    let games = vec![elo(g(0, 1, W, 0), 0, 2800), elo(g(0, 2, W, 0), 2400, 2800)];
    let t = build(games, &CrosstableOptions::default());
    let a = t.rows.iter().find(|r| r.name == "A").unwrap();
    assert_eq!(a.elo, 2400);
    assert_eq!(a.performance.unwrap().average_opponent, 2775);
}

/// A knockout game: Round `round.board`.
fn kg(white: u32, black: u32, result: GameResult, round: u32) -> TournamentGame {
    let mut game = g(white, black, result, round);
    game.sub_round = true;
    game
}

fn auto(games: Vec<TournamentGame>) -> Crosstable {
    build(games, &CrosstableOptions::default())
}

#[test]
fn swiss_with_round_and_board_numbers_is_not_a_knockout() {
    // 1.1, 1.2, 2.1, 2.2: the same four players in both rounds.
    let games = vec![
        kg(0, 1, W, 1),
        kg(2, 3, D, 1),
        kg(0, 2, D, 2),
        kg(1, 3, W, 2),
    ];
    let t = auto(games);
    assert_ne!(t.mode, TableMode::Knockout);
    assert_eq!(t.knockout, vec![]);
    assert!(text(&t).contains("(+1 -0 =1)"));
}

#[test]
fn round_robin_with_round_and_board_numbers_is_all_play_all() {
    let games = vec![
        kg(0, 1, W, 1),
        kg(2, 3, D, 1),
        kg(0, 2, D, 2),
        kg(1, 3, W, 2),
        kg(0, 3, W, 3),
        kg(1, 2, D, 3),
    ];
    assert_eq!(auto(games).mode, TableMode::AllPlayAll);
}

#[test]
fn a_player_with_two_opponents_in_a_round_is_not_a_knockout() {
    let games = vec![kg(0, 1, W, 1), kg(0, 2, W, 1), kg(0, 3, W, 2)];
    assert_ne!(auto(games).mode, TableMode::Knockout);
}

#[test]
fn a_match_over_several_rounds_is_not_a_knockout() {
    // The same pair in every round: the round-two players are not a strict
    // subset of round one's.
    let games = vec![kg(0, 1, W, 1), kg(0, 1, D, 2), kg(1, 0, B, 3)];
    assert_ne!(auto(games).mode, TableMode::Knockout);
}

#[test]
fn dates_and_round_zero_are_never_taken_for_a_knockout() {
    // "0.5" reads as round 0; "2020.05.01" as round 2020 (out of the grid).
    for round in [0, 2020, MAX_ROUNDS] {
        let t = auto(vec![kg(0, 1, W, round), kg(2, 3, D, round)]);
        assert_ne!(t.mode, TableMode::Knockout, "round {round}");
        assert_eq!(t.knockout, vec![]);
        let out = text(&t);
        assert!(out.contains("1: "), "round {round}: {out}");
    }
}

#[test]
fn a_real_knockout_is_recognised() {
    // Eight players: four matches, two, then the final.
    let games = vec![
        kg(0, 1, W, 1),
        kg(2, 3, W, 1),
        kg(4, 5, B, 1),
        kg(6, 7, W, 1),
        kg(0, 2, W, 2),
        kg(5, 6, D, 2),
        kg(0, 6, W, 3),
    ];
    let t = auto(games);
    assert_eq!(t.mode, TableMode::Knockout);
    assert_eq!(t.knockout.len(), 3);
    assert_eq!(t.knockout[0].matches.len(), 4);
    assert_eq!(t.knockout[2].matches.len(), 1);
}

#[test]
fn counters_saturate_instead_of_wrapping() {
    // 50 000 draws between two players with three points for a win: each has
    // 100 000 half points and a Sonneborn-Berger of 50 000 * 100 000, which
    // does not fit in 32 bits.
    let games: Vec<_> = (0..50_000).map(|_| g(0, 1, D, 0)).collect();
    let options = CrosstableOptions {
        three_points: true,
        ..CrosstableOptions::default()
    };
    let t = build(games, &options);
    assert_eq!(t.game_count, 50_000);
    assert_eq!(t.results.draws, 50_000);
    for row in &t.rows {
        assert_eq!(row.score_half, 100_000);
        assert_eq!(row.games, 50_000);
        assert_eq!(row.drawn, 50_000);
        assert_eq!(row.tiebreak, u32::MAX);
    }
}

#[test]
fn many_rated_games_keep_the_performance_exact() {
    let games: Vec<_> = (0..20_000)
        .map(|_| elo(g(0, 1, D, 0), 2400, 2500))
        .collect();
    let t = build(games, &CrosstableOptions::default());
    let a = t.rows.iter().find(|r| r.name == "A").unwrap();
    assert_eq!(a.rated_games, 20_000);
    let perf = a.performance.unwrap();
    assert_eq!(perf.average_opponent, 2500);
    assert_eq!(perf.percentage, 50);
}

#[test]
fn cancelling_stops_the_building_of_the_table() {
    let games: Vec<_> = (0..6_000).map(|i| g(i % 2, 1 - i % 2, D, 0)).collect();
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = Crosstable::from_games_cancellable(
        &data(games.clone()),
        &CrosstableOptions::default(),
        None,
        &cancel,
    );
    assert!(matches!(result, Err(CrosstableError::Cancelled)));
    // Without the flag the same games give a table.
    assert!(Crosstable::from_games(&data(games), &CrosstableOptions::default(), None).is_ok());
}

#[test]
fn unreadable_entries_and_the_game_limit_become_notices() {
    let mut d = data(apa());
    d.unreadable = 3;
    d.truncated = true;
    let t = Crosstable::from_games(&d, &CrosstableOptions::default(), None).unwrap();
    assert_eq!(t.notices, [Notice::GameLimit, Notice::Unreadable(3)]);
    let out = text(&t);
    assert!(out.starts_with("Game limit of 100000 reached. Table is incomplete\n\n"));
    assert!(out.contains("Warning: 3 unreadable games were left out."));
}

#[test]
fn a_very_large_all_play_all_grid_warns() {
    // 300 players, 3 games per pair on one line would be 270 000 cells; here
    // two players with a long series: 1 column x 2 rows x 100 001 games.
    let games: Vec<_> = (0..100_001).map(|_| g(0, 1, D, 0)).collect();
    let t = build(games, &CrosstableOptions::default());
    assert_eq!(t.mode, TableMode::AllPlayAll);
    assert_eq!(t.notices, [Notice::LargeOutput]);
    // A small table has no notice.
    assert_eq!(
        build(apa(), &CrosstableOptions::default()).notices,
        Vec::<Notice>::new()
    );
}

#[test]
fn text_with_non_ascii_names_aligns_by_characters() {
    let mut d = data(vec![g(0, 1, W, 0), g(1, 2, D, 0), g(0, 2, W, 0)]);
    d.names.insert(0, "Müller".into());
    d.names.insert(1, "José".into());
    d.names.insert(2, "Zoë Åström".into());
    let t = Crosstable::from_games(&d, &CrosstableOptions::default(), None).unwrap();
    assert_eq!(t.longest_name, 10);
    let out = text(&t);
    let lines: Vec<&str> = out.lines().collect();
    // Every table line has the same number of characters up to the tallies,
    // and the dashes are `line_width` characters long.
    let dash_lines: Vec<_> = lines.iter().filter(|l| l.starts_with("---")).collect();
    assert_eq!(dash_lines.len(), 2);
    assert!(dash_lines.iter().all(|l| l.chars().count() == t.line_width));
    let row = lines.iter().find(|l| l.contains("Müller")).unwrap();
    // " 1: " + name padded to 10 characters + one space.
    assert!(row.starts_with(" 1: Müller     "), "{row:?}");
    let row = lines.iter().find(|l| l.contains("José")).unwrap();
    assert!(row.contains("José       "), "{row:?}");
    // The column headers are the names cut to the width of the cells (one
    // column per pair: the longest series is 1), by characters.
    let header = lines.iter().find(|l| l.contains("Score")).unwrap();
    assert!(header.ends_with(" M J Z"), "{header:?}");
}

#[test]
fn latex_tallies_stay_in_the_chg_column_when_chg_is_not_written() {
    // Dan has a rated opponent but no Elo of his own: Perf without Chg.
    let games = vec![
        elo(g(0, 1, W, 0), 2500, 0),
        elo(g(1, 2, D, 0), 0, 2300),
        elo(g(0, 2, D, 0), 2500, 2300),
    ];
    let t = build(games, &CrosstableOptions::default());
    let latex = render_latex(&t, &CrosstableLabels::default());
    let b = latex
        .lines()
        .find(|l| l.starts_with(" 3:"))
        .unwrap_or_else(|| panic!("{latex}"));
    // Perf cell, empty Chg cell, then the tallies: two ampersands after Perf.
    assert!(b.contains(" &  (+"), "{latex}");
}

#[test]
fn bio_providers_can_cross_threads() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn PlayerBioProvider>();
}
