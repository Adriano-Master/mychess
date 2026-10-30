//! Commands embedded in comments (spec m1-2, rules 17-20).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::float_cmp,
    let_underscore_drop
)]
use std::borrow::Cow;
use std::time::{Duration, Instant};

use mychess_core::{
    BoardMark, EmbeddedCommand, MATE_SCORE, MarkKind, Square, chessbase_color, clock_of,
    embedded_commands, emt_of, eval_of, strip_mark_codes, toggle_mark,
};
use proptest::prelude::*;
use rstest::rstest;

fn sq(name: &str) -> Square {
    Square::from_ascii(name.as_bytes()).unwrap()
}

fn mark(kind: MarkKind, from: &str, to: Option<&str>, color: &str) -> BoardMark {
    BoardMark {
        kind,
        from: sq(from),
        to: to.map(sq),
        color: color.to_owned(),
    }
}

fn cmd(kind: MarkKind, from: &str, to: Option<&str>, color: &str) -> EmbeddedCommand {
    EmbeddedCommand::Mark(mark(kind, from, to, color))
}

fn other(name: &str, args: &str) -> EmbeddedCommand {
    EmbeddedCommand::Other {
        name: name.to_owned(),
        args: args.to_owned(),
    }
}

/// Generous bound: only catches super-linear behavior, not slow machines.
const TIME_LIMIT: Duration = Duration::from_secs(20);

// ---------- rule 17: strip_mark_codes ----------

#[rstest]
#[case("[%clk 0:01:00] text", "text")]
#[case("text [%clk 0:01:00]", "text ")]
#[case("a [%clk 1:00]  b", "a b")]
#[case("[%eval 0.3]", "")]
#[case("[%mark e4 green]Good move", "Good move")]
#[case("[%csl Ge4][%cal Gd2d4] x", "x")]
#[case("[%anything at all] y", "y")]
#[case("[%foo]\n\t y", "y")]
#[case("[%]", "")]
#[case("[%] x", "x")]
#[case("keep [not a code] this", "keep [not a code] this")]
#[case("100% sure [%x]", "100% sure ")]
#[case("", "")]
#[case("plain", "plain")]
#[case("é[%clk 1:00]é", "éé")]
#[case("[%é] ü", "ü")]
fn strips_any_percent_block_and_following_space(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(strip_mark_codes(input), expected);
}

#[rstest]
#[case("abc[%")]
#[case("abc[%clk 1:00")]
#[case("[%clk 1:00 and more text")]
#[case("[")]
#[case("%")]
#[case("[%clk [%eval 1")]
fn unclosed_block_stays_as_text(#[case] input: &str) {
    assert_eq!(strip_mark_codes(input), input);
}

#[test]
fn block_interrupted_by_another_bracket_stays_but_the_inner_one_goes() {
    assert_eq!(strip_mark_codes("[%clk [%eval 1] x"), "[%clk x");
    assert_eq!(strip_mark_codes("[%a[%b]]"), "[%a]");
    assert_eq!(strip_mark_codes("[%a [b] [%c]"), "[%a [b] ");
}

#[test]
fn text_after_a_stray_close_bracket_is_kept() {
    assert_eq!(strip_mark_codes("] [%x] y ]"), "] y ]");
}

#[test]
fn only_ascii_white_space_after_a_block_is_swallowed() {
    // Scid uses C `isspace`: the no-break space (U+00A0) is not white space.
    assert_eq!(strip_mark_codes("[%x]\u{a0}y"), "\u{a0}y");
    assert_eq!(strip_mark_codes("[%x]\r\n y"), "y");
    assert_eq!(strip_mark_codes("[%x]\u{b}\u{c}y"), "y");
}

#[test]
fn borrows_when_nothing_is_removed() {
    assert!(matches!(
        strip_mark_codes("nothing here [x] [%open"),
        Cow::Borrowed(_)
    ));
    assert!(matches!(strip_mark_codes("a [%x] b"), Cow::Owned(_)));
}

#[test]
fn consecutive_blocks_are_all_removed() {
    assert_eq!(strip_mark_codes("[%a][%b] [%c]\n[%d]z"), "z");
}

#[test]
fn nested_looking_brackets_do_not_swallow_following_text() {
    assert_eq!(strip_mark_codes("[%a [b] c] tail"), "[%a [b] c] tail");
}

// ---------- rule 18: marks ----------

#[rstest]
#[case("[%mark e4]", mark(MarkKind::Full, "e4", None, "firebrick"))]
#[case("[%mark e4 green]", mark(MarkKind::Full, "e4", None, "green"))]
#[case("[%mark a1  blue]", mark(MarkKind::Full, "a1", None, "blue"))]
#[case("[%arrow e2 e4]", mark(MarkKind::Arrow, "e2", Some("e4"), "firebrick"))]
#[case("[%arrow c4 f7 blue]", mark(MarkKind::Arrow, "c4", Some("f7"), "blue"))]
#[case(
    "[%arrow c4 f7 #ff0000]",
    mark(MarkKind::Arrow, "c4", Some("f7"), "#ff0000")
)]
fn old_scid_mark_and_arrow_formats(#[case] text: &str, #[case] expected: BoardMark) {
    assert_eq!(
        embedded_commands(text),
        vec![EmbeddedCommand::Mark(expected)],
        "{text}"
    );
}

#[rstest]
#[case("[%draw e4]", mark(MarkKind::Full, "e4", None, "firebrick"))]
#[case("[%draw e4,green]", mark(MarkKind::Full, "e4", None, "green"))]
#[case("[%draw e2,e4]", mark(MarkKind::Arrow, "e2", Some("e4"), "firebrick"))]
#[case(
    "[%draw e2,e4,green]",
    mark(MarkKind::Arrow, "e2", Some("e4"), "green")
)]
#[case("[%draw full,e4,green]", mark(MarkKind::Full, "e4", None, "green"))]
#[case("[%draw full,e4]", mark(MarkKind::Full, "e4", None, "firebrick"))]
#[case("[%draw square,e4,red]", mark(MarkKind::Square, "e4", None, "red"))]
#[case("[%draw circle,f7,blue]", mark(MarkKind::Circle, "f7", None, "blue"))]
#[case("[%draw circle,f7]", mark(MarkKind::Circle, "f7", None, "firebrick"))]
#[case("[%draw disk,a8,yellow]", mark(MarkKind::Disk, "a8", None, "yellow"))]
#[case("[%draw tux,h1,red]", mark(MarkKind::Tux, "h1", None, "red"))]
#[case(
    "[%draw arrow,e2,e4,green]",
    mark(MarkKind::Arrow, "e2", Some("e4"), "green")
)]
#[case(
    "[%draw arrow,e2,e4]",
    mark(MarkKind::Arrow, "e2", Some("e4"), "firebrick")
)]
#[case("[%draw 1,e4]", mark(MarkKind::Text('1'), "e4", None, "firebrick"))]
#[case("[%draw x,e4,blue]", mark(MarkKind::Text('x'), "e4", None, "blue"))]
#[case("[%draw A,c3,green]", mark(MarkKind::Text('A'), "c3", None, "green"))]
#[case("[%draw +,d4]", mark(MarkKind::Text('+'), "d4", None, "firebrick"))]
#[case("[%draw -,d4]", mark(MarkKind::Text('-'), "d4", None, "firebrick"))]
#[case("[%draw =,d4]", mark(MarkKind::Text('='), "d4", None, "firebrick"))]
#[case("[%draw ?,d4]", mark(MarkKind::Text('?'), "d4", None, "firebrick"))]
#[case("[%draw !,d4]", mark(MarkKind::Text('!'), "d4", None, "firebrick"))]
#[case(
    "[%draw circle,e4,#00ff00]",
    mark(MarkKind::Circle, "e4", None, "#00ff00")
)]
fn draw_formats(#[case] text: &str, #[case] expected: BoardMark) {
    assert_eq!(
        embedded_commands(text),
        vec![EmbeddedCommand::Mark(expected)],
        "{text}"
    );
}

#[rstest]
#[case("[%csl Ge4]", vec![("e4", "green")])]
#[case("[%csl Re4]", vec![("e4", "firebrick")])]
#[case("[%csl Ye5,Bd4,Oa1,Ch8,Gh1]", vec![("e5", "yellow2"), ("d4", "blue"), ("a1", "darkOrange1"), ("h8", "turquoise"), ("h1", "green")])]
fn csl_makes_circles(#[case] text: &str, #[case] expected: Vec<(&str, &str)>) {
    let want: Vec<_> = expected
        .into_iter()
        .map(|(s, c)| cmd(MarkKind::Circle, s, None, c))
        .collect();
    assert_eq!(embedded_commands(text), want);
}

#[rstest]
#[case("[%cal Gd2d4]", vec![("d2", "d4", "green")])]
#[case("[%cal Rd5e4,Yb1c3,Bb1d2]", vec![("d5", "e4", "firebrick"), ("b1", "c3", "yellow2"), ("b1", "d2", "blue")])]
#[case("[%cal Oa1a8,Cb2b7]", vec![("a1", "a8", "darkOrange1"), ("b2", "b7", "turquoise")])]
fn cal_makes_arrows(#[case] text: &str, #[case] expected: Vec<(&str, &str, &str)>) {
    let want: Vec<_> = expected
        .into_iter()
        .map(|(a, b, c)| cmd(MarkKind::Arrow, a, Some(b), c))
        .collect();
    assert_eq!(embedded_commands(text), want);
}

#[test]
fn chessbase_color_table_matches_scid() {
    let table = [
        ('G', "green"),
        ('R', "firebrick"),
        ('Y', "yellow2"),
        ('B', "blue"),
        ('O', "darkOrange1"),
        ('C', "turquoise"),
    ];
    for (letter, name) in table {
        assert_eq!(chessbase_color(letter), Some(name));
    }
    for letter in ['g', 'r', 'X', 'W', ' ', '1', 'é', '\0'] {
        assert_eq!(chessbase_color(letter), None, "{letter:?}");
    }
}

#[rstest]
#[case("[%draw]", "draw", "")]
#[case("[%draw ]", "draw", "")]
#[case("[%draw i9]", "draw", "i9")]
#[case("[%draw e9]", "draw", "e9")]
#[case("[%draw circle]", "draw", "circle")]
#[case("[%draw circle,i9]", "draw", "circle,i9")]
#[case("[%draw blob,e4]", "draw", "blob,e4")]
#[case("[%draw ab,e4]", "draw", "ab,e4")]
#[case("[%draw 1,e4,e5]", "draw", "1,e4,e5")]
#[case("[%draw circle,e4,e5]", "draw", "circle,e4,e5")]
#[case("[%draw arrow,e4]", "draw", "arrow,e4")]
#[case("[%draw @,e4]", "draw", "@,e4")]
#[case("[%csl]", "csl", "")]
#[case("[%csl Xe4]", "csl", "Xe4")]
#[case("[%csl ge4]", "csl", "ge4")]
#[case("[%csl Ge4,]", "csl", "Ge4,")]
#[case("[%csl Ge4,Rz9]", "csl", "Ge4,Rz9")]
#[case("[%csl Gd2d4]", "csl", "Gd2d4")]
#[case("[%cal Gd2]", "cal", "Gd2")]
#[case("[%cal Gd2d9]", "cal", "Gd2d9")]
#[case("[%cal Gd2d4,Xa1a2]", "cal", "Gd2d4,Xa1a2")]
#[case("[%mark]", "mark", "")]
#[case("[%mark z9]", "mark", "z9")]
#[case("[%arrow e2]", "arrow", "e2")]
#[case("[%eval abc]", "eval", "abc")]
#[case("[%clk soon]", "clk", "soon")]
#[case("[%emt 5]", "emt", "5")]
fn malformed_known_blocks_become_other(#[case] text: &str, #[case] name: &str, #[case] args: &str) {
    assert_eq!(embedded_commands(text), vec![other(name, args)], "{text}");
}

#[test]
fn unknown_block_is_other_with_trimmed_arguments() {
    assert_eq!(embedded_commands("[%ct 1]"), vec![other("ct", "1")]);
    assert_eq!(
        embedded_commands("[%foo   bar baz  ]"),
        vec![other("foo", "bar baz")]
    );
    assert_eq!(embedded_commands("[%foo]"), vec![other("foo", "")]);
}

#[test]
fn commands_come_in_order_of_appearance_with_text_between() {
    let got = embedded_commands(
        "1. e4 [%clk 0:03:00] great [%csl Ge4] [%eval 0.3] [%emt 0:00:05] [%cal Gd2d4]",
    );
    assert_eq!(
        got,
        vec![
            EmbeddedCommand::Clk(180),
            cmd(MarkKind::Circle, "e4", None, "green"),
            EmbeddedCommand::Eval(0.3),
            EmbeddedCommand::Emt(5),
            cmd(MarkKind::Arrow, "d2", Some("d4"), "green"),
        ]
    );
}

#[test]
fn unclosed_blocks_are_not_commands() {
    assert!(embedded_commands("[%clk 1:00").is_empty());
    assert!(embedded_commands("[%draw e4").is_empty());
    assert!(embedded_commands("no commands [here]").is_empty());
    assert!(embedded_commands("").is_empty());
    assert_eq!(
        embedded_commands("[%clk [%eval 1]"),
        vec![EmbeddedCommand::Eval(1.0)]
    );
}

// ---------- rule 19: toggle_mark ----------

#[test]
fn toggle_adds_a_draw_block_at_the_end() {
    let m = mark(MarkKind::Circle, "e4", None, "red");
    assert_eq!(toggle_mark("hello", &m), "hello [%draw circle,e4,red]");
    assert_eq!(toggle_mark("", &m), "[%draw circle,e4,red]");
    assert_eq!(toggle_mark("   ", &m), "[%draw circle,e4,red]");
    assert_eq!(
        toggle_mark("  padded  ", &m),
        "padded [%draw circle,e4,red]"
    );
}

#[test]
fn toggle_twice_is_identity_on_a_trimmed_comment() {
    let m = mark(MarkKind::Full, "f4", None, "green");
    let once = toggle_mark("good", &m);
    assert_eq!(once, "good [%draw full,f4,green]");
    assert_eq!(toggle_mark(&once, &m), "good");
    assert_eq!(toggle_mark(&toggle_mark("  x ", &m), &m), "x");
    assert_eq!(toggle_mark(&toggle_mark("", &m), &m), "");
}

#[test]
fn toggle_arrow_writes_both_squares() {
    let m = mark(MarkKind::Arrow, "e2", Some("e4"), "green");
    assert_eq!(toggle_mark("t", &m), "t [%draw arrow,e2,e4,green]");
    assert_eq!(toggle_mark("t [%draw arrow,e2,e4,green]", &m), "t");
}

#[test]
fn arrow_in_the_opposite_direction_is_replaced_not_duplicated() {
    let m = mark(MarkKind::Arrow, "e2", Some("e4"), "green");
    let got = toggle_mark("t [%draw arrow,e4,e2,green]", &m);
    assert_eq!(got, "t [%draw arrow,e2,e4,green]");
}

#[test]
fn arrow_with_another_color_is_replaced() {
    let m = mark(MarkKind::Arrow, "e2", Some("e4"), "green");
    assert_eq!(
        toggle_mark("[%draw arrow,e2,e4,red]", &m),
        "[%draw arrow,e2,e4,green]"
    );
    assert_eq!(
        toggle_mark("[%draw arrow,e4,e2,red] z", &m),
        "z [%draw arrow,e2,e4,green]"
    );
}

#[test]
fn arrows_with_other_squares_are_kept() {
    let m = mark(MarkKind::Arrow, "e2", Some("e4"), "green");
    let got = toggle_mark("[%draw arrow,e2,e5,green]", &m);
    assert_eq!(got, "[%draw arrow,e2,e5,green] [%draw arrow,e2,e4,green]");
}

#[test]
fn arrow_from_a_square_to_itself_is_ignored() {
    let m = mark(MarkKind::Arrow, "e4", Some("e4"), "green");
    assert_eq!(toggle_mark("abc ", &m), "abc ");
}

#[test]
fn mark_with_inconsistent_destination_is_ignored() {
    let arrow_without_to = mark(MarkKind::Arrow, "e2", None, "green");
    let circle_with_to = mark(MarkKind::Circle, "e2", Some("e4"), "green");
    assert_eq!(toggle_mark("abc", &arrow_without_to), "abc");
    assert_eq!(toggle_mark("abc", &circle_with_to), "abc");
}

#[test]
fn same_square_marks_of_the_replaceable_kinds_replace_each_other() {
    let circle = mark(MarkKind::Circle, "e4", None, "red");
    let got = toggle_mark("[%draw full,e4,green] note", &circle);
    assert_eq!(got, "note [%draw circle,e4,red]");
    for kind in [
        "tux", "circle", "disk", "full", "+", "-", "=", "?", "!", "1", "5", "9",
    ] {
        let c = format!("[%draw {kind},e4,blue]");
        assert_eq!(
            toggle_mark(&c, &circle),
            "[%draw circle,e4,red]",
            "kind {kind}"
        );
    }
}

#[test]
fn text_digit_marks_replace_each_other_on_one_square() {
    let two = mark(MarkKind::Text('2'), "e4", None, "red");
    assert_eq!(toggle_mark("[%draw 1,e4,red]", &two), "[%draw 2,e4,red]");
}

#[test]
fn marks_on_other_squares_are_kept() {
    let circle = mark(MarkKind::Circle, "e4", None, "red");
    let got = toggle_mark("[%draw full,d4,green]", &circle);
    assert_eq!(got, "[%draw full,d4,green] [%draw circle,e4,red]");
}

#[test]
fn kinds_outside_the_replace_list_coexist_on_a_square() {
    let circle = mark(MarkKind::Circle, "e4", None, "red");
    let got = toggle_mark("[%draw square,e4,red]", &circle);
    assert_eq!(got, "[%draw square,e4,red] [%draw circle,e4,red]");
    let got = toggle_mark("[%draw x,e4,red]", &circle);
    assert_eq!(got, "[%draw x,e4,red] [%draw circle,e4,red]");
}

#[test]
fn identical_mark_of_a_non_replaceable_kind_toggles_off() {
    let sqr = mark(MarkKind::Square, "e4", None, "red");
    assert_eq!(toggle_mark("a [%draw square,e4,red]", &sqr), "a");
    let tx = mark(MarkKind::Text('x'), "e4", None, "red");
    assert_eq!(toggle_mark("a [%draw x,e4,red]", &tx), "a");
}

#[test]
fn same_kind_with_other_color_is_recolored_not_removed() {
    let m = mark(MarkKind::Circle, "e4", None, "blue");
    assert_eq!(
        toggle_mark("[%draw circle,e4,red]", &m),
        "[%draw circle,e4,blue]"
    );
}

#[test]
fn empty_color_means_the_default_color() {
    let m = mark(MarkKind::Circle, "e4", None, "");
    assert_eq!(toggle_mark("", &m), "[%draw circle,e4,firebrick]");
}

#[test]
fn invalid_color_or_text_char_leaves_the_comment_unchanged() {
    let bad_color = mark(MarkKind::Circle, "e4", None, "re d]");
    let bracket = mark(MarkKind::Circle, "e4", None, "red,blue");
    let bad_char = mark(MarkKind::Text('@'), "e4", None, "red");
    let bad_unicode = mark(MarkKind::Text('é'), "e4", None, "red");
    for m in [bad_color, bracket, bad_char, bad_unicode] {
        assert_eq!(toggle_mark(" keep ", &m), " keep ", "{m:?}");
    }
}

#[test]
fn toggle_output_is_understood_by_embedded_commands() {
    let marks = [
        mark(MarkKind::Full, "a1", None, "green"),
        mark(MarkKind::Square, "b2", None, "red"),
        mark(MarkKind::Circle, "c3", None, "blue"),
        mark(MarkKind::Disk, "d4", None, "yellow"),
        mark(MarkKind::Tux, "e5", None, "red"),
        mark(MarkKind::Text('7'), "f6", None, "red"),
        mark(MarkKind::Text('Q'), "g7", None, "green"),
        mark(MarkKind::Arrow, "h8", Some("a1"), "blue"),
    ];
    for m in marks {
        let c = toggle_mark("text", &m);
        assert_eq!(
            embedded_commands(&c),
            vec![EmbeddedCommand::Mark(m.clone())],
            "{c}"
        );
    }
}

#[test]
fn toggling_does_not_touch_other_embedded_commands() {
    let m = mark(MarkKind::Circle, "e4", None, "red");
    let c = "[%clk 0:01:00] [%eval 0.5] words";
    let added = toggle_mark(c, &m);
    assert!(added.starts_with(c));
    assert_eq!(toggle_mark(&added, &m), c);
}

fn square_strategy() -> impl Strategy<Value = Square> {
    (0u8..8, 0u8..8)
        .prop_map(|(f, r)| sq(&format!("{}{}", char::from(b'a' + f), char::from(b'1' + r))))
}

fn mark_strategy() -> impl Strategy<Value = BoardMark> {
    let kind = prop_oneof![
        Just(MarkKind::Full),
        Just(MarkKind::Square),
        Just(MarkKind::Circle),
        Just(MarkKind::Disk),
        Just(MarkKind::Tux),
        prop::sample::select(vec!['1', '5', '9', '+', '-', '=', '?', '!', 'a', 'Z', '0'])
            .prop_map(MarkKind::Text),
    ];
    // A color spelled like a square (`e4`) is ambiguous in the `[%draw]` grammar.
    let color = prop_oneof![
        Just(String::new()),
        "[A-Za-z0-9]{1,8}".prop_filter("not a square", |c| {
            let b = c.as_bytes();
            !(b.len() == 2 && (b'a'..=b'h').contains(&b[0]) && (b'1'..=b'8').contains(&b[1]))
        })
    ];
    let plain =
        (kind, square_strategy(), color.clone()).prop_map(|(kind, from, color)| BoardMark {
            kind,
            from,
            to: None,
            color,
        });
    let arrow = (square_strategy(), square_strategy(), color)
        .prop_filter("distinct squares", |(a, b, _)| a != b)
        .prop_map(|(from, to, color)| BoardMark {
            kind: MarkKind::Arrow,
            from,
            to: Some(to),
            color,
        });
    prop_oneof![plain, arrow]
}

fn comment_without_draw() -> impl Strategy<Value = String> {
    "[ -~\n\té\\[\\]%]{0,60}".prop_filter("no draw block", |s| !s.contains("[%draw"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn toggling_twice_gives_back_the_trimmed_comment(m in mark_strategy(), c in comment_without_draw()) {
        let once = toggle_mark(&c, &m);
        let twice = toggle_mark(&once, &m);
        prop_assert_eq!(twice, c.trim().to_owned());
    }

    #[test]
    fn toggling_a_fresh_mark_makes_it_the_last_parsed_command(m in mark_strategy(), c in comment_without_draw()) {
        let once = toggle_mark(&c, &m);
        let last = embedded_commands(&once).into_iter().rev().find_map(|e| match e {
            EmbeddedCommand::Mark(b) => Some(b),
            _ => None,
        });
        let mut expected = m.clone();
        if expected.color.is_empty() {
            expected.color = "firebrick".into();
        }
        prop_assert_eq!(last, Some(expected));
    }

    #[test]
    fn toggle_never_panics_on_arbitrary_comments_and_colors(
        c in any::<String>(), color in any::<String>(), m in mark_strategy(),
    ) {
        let m = BoardMark { color, ..m };
        let _ = toggle_mark(&c, &m);
    }

    #[test]
    fn toggle_with_draw_soup_never_panics(
        c in "(\\[%draw [a-z0-9,]{0,16}\\]|[ a-z\\[\\]%,]){0,12}", m in mark_strategy(),
    ) {
        let once = toggle_mark(&c, &m);
        let _ = toggle_mark(&once, &m);
    }
}

// ---------- rule 20: eval, clk, emt ----------

#[rstest]
#[case("[%eval 0.35]", Some(0.35))]
#[case("[%eval -1.2]", Some(-1.2))]
#[case("[%eval +2.5]", Some(2.5))]
#[case("[%eval 0]", Some(0.0))]
#[case("[%eval 15]", Some(15.0))]
#[case("[%eval -327]", Some(-327.0))]
#[case("[%eval #3]", Some(327.0))]
#[case("[%eval #-3]", Some(-327.0))]
#[case("[%eval #0]", Some(327.0))]
#[case("[%eval #]", Some(327.0))]
#[case("[%eval abc]", None)]
#[case("[%eval .5]", None)]
#[case("[%eval -]", None)]
#[case("[%eval]", None)]
#[case("[%eval ]", None)]
#[case("[%eval 0.3] and +5 later", Some(0.3))]
#[case("[%clk 0:01:00] [%eval -0.5]", Some(-0.5))]
#[case("+0.35", Some(0.35))]
#[case("White is better -0.5 here", Some(-0.5))]
#[case("-1.25: Black stands better", Some(-1.25))]
#[case("good +3 move", Some(3.0))]
#[case("1-0", None)]
#[case("0-1 resigned", None)]
#[case("1-0 but +1.5", Some(1.5))]
#[case("", None)]
#[case("no score", None)]
#[case("+", None)]
#[case("-", None)]
#[case("+ 5", None)]
#[case("a - b", None)]
#[case("x+y", None)]
fn eval_of_cases(#[case] text: &str, #[case] expected: Option<f32>) {
    match (eval_of(text), expected) {
        (Some(got), Some(want)) => assert!((got - want).abs() < 1e-6, "{text:?}: {got} != {want}"),
        (got, want) => assert_eq!(got, want, "{text:?}"),
    }
}

#[test]
fn eval_command_wins_over_a_score_elsewhere_in_the_text() {
    assert_eq!(eval_of("-9.9 [%eval 1.5]"), Some(1.5));
    assert_eq!(eval_of("[%eval abc] +2.0"), None);
}

#[test]
fn mate_score_constant_is_327() {
    assert_eq!(MATE_SCORE, 327.0);
}

#[test]
fn eval_is_not_clamped_to_ten() {
    assert_eq!(eval_of("[%eval 123.5]"), Some(123.5));
    assert_eq!(eval_of("[%eval -50]"), Some(-50.0));
}

#[test]
fn eval_with_absurd_digits_gives_none_or_a_finite_number() {
    let huge = format!("[%eval {}]", "9".repeat(400));
    assert!(eval_of(&huge).is_none_or(f32::is_finite));
    let long_fraction = format!("[%eval 0.{}]", "1".repeat(400));
    assert!(eval_of(&long_fraction).is_none_or(f32::is_finite));
}

#[test]
fn eval_command_and_embedded_commands_agree() {
    assert_eq!(
        embedded_commands("[%eval 0.35]"),
        vec![EmbeddedCommand::Eval(0.35)]
    );
    assert_eq!(
        embedded_commands("[%eval #-2]"),
        vec![EmbeddedCommand::Eval(-MATE_SCORE)]
    );
}

#[rstest]
#[case("[%clk 1:23:45]", Some(5025))]
#[case("[%clk 0:00:05]", Some(5))]
#[case("[%clk 0:00:00]", Some(0))]
#[case("[%clk 00:00:00]", Some(0))]
#[case("[%clk 12:34]", Some(754))]
#[case("[%clk 0:59]", Some(59))]
#[case("[%clk 10:00:00]", Some(36_000))]
#[case("[%clk  1:00:00 ]", Some(3600))]
#[case("text [%clk 0:01:00] more", Some(60))]
#[case("[%clk 0:00:10] [%clk 0:00:20]", Some(10))]
#[case("[%clk 1:23:45.0]", Some(5025))]
#[case("[%clk 1193046:28:15]", Some(u32::MAX))]
#[case("[%clk 1193046:28:16]", None)]
#[case("[%clk 99999999999:00:00]", None)]
#[case("[%clk 4000000:00:00]", None)]
#[case("[%clk 5]", None)]
#[case("[%clk ]", None)]
#[case("[%clk]", None)]
#[case("[%clk abc]", None)]
#[case("[%clk 1:2:3:4]", None)]
#[case("[%clk -1:00]", None)]
#[case("[%clk 1:-5]", None)]
#[case("[%clk 1::5]", None)]
#[case("[%clk :5]", None)]
#[case("[%clk 1:]", None)]
#[case("[%clk 1:00", None)]
#[case("clk 1:00", None)]
#[case("[%emt 0:00:05]", None)]
#[case("", None)]
fn clock_of_cases(#[case] text: &str, #[case] expected: Option<u32>) {
    assert_eq!(clock_of(text), expected, "{text:?}");
}

#[test]
fn clock_decimals_do_not_change_the_whole_seconds_by_more_than_one() {
    let got = clock_of("[%clk 1:23:45.6]").unwrap();
    assert!((5025..=5026).contains(&got), "{got}");
    let got = clock_of("[%clk 0:00:09.99]").unwrap();
    assert!((9..=10).contains(&got), "{got}");
}

#[rstest]
#[case("[%emt 0:00:05]", Some(5))]
#[case("[%emt 1:02]", Some(62))]
#[case("[%emt 1:02:03]", Some(3723))]
#[case("[%emt 0:00:00]", Some(0))]
#[case("[%clk 0:03:00] [%emt 0:00:07]", Some(7))]
#[case("[%emt 0:00:07] [%emt 0:00:09]", Some(7))]
#[case("[%emt 7]", None)]
#[case("[%emt x:y]", None)]
#[case("[%emt 99999999999:00]", None)]
#[case("[%clk 0:03:00]", None)]
#[case("", None)]
fn emt_of_cases(#[case] text: &str, #[case] expected: Option<u32>) {
    assert_eq!(emt_of(text), expected, "{text:?}");
}

#[test]
fn clk_and_emt_in_one_comment_are_independent() {
    let c = "[%clk 0:03:00] [%emt 0:00:07]";
    assert_eq!((clock_of(c), emt_of(c)), (Some(180), Some(7)));
    assert_eq!(
        embedded_commands(c),
        vec![EmbeddedCommand::Clk(180), EmbeddedCommand::Emt(7)]
    );
}

// ---------- hostile input ----------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn no_function_panics_on_arbitrary_text(s in any::<String>()) {
        let stripped = strip_mark_codes(&s);
        prop_assert!(stripped.len() <= s.len());
        let _ = embedded_commands(&s);
        let _ = eval_of(&s);
        let _ = clock_of(&s);
        let _ = emt_of(&s);
    }

    #[test]
    fn no_function_panics_on_bracket_soup(s in "(\\[%|\\[|\\]|%|clk|eval|emt|draw|csl|cal|mark|arrow| |,|:|\\.|#|-|\\+|[0-9a-h]|é){0,40}") {
        let stripped = strip_mark_codes(&s);
        prop_assert!(stripped.len() <= s.len());
        if !s.contains("[%") {
            prop_assert_eq!(stripped.as_ref(), s.as_str());
        }
        let _ = embedded_commands(&s);
        let _ = eval_of(&s);
        let _ = clock_of(&s);
        let _ = emt_of(&s);
    }

    #[test]
    fn stripped_text_has_no_complete_block_when_input_blocks_are_flat(
        parts in proptest::collection::vec(("[a-z ]{0,5}", "[a-z0-9 :.,#+-]{0,8}"), 0..6),
    ) {
        let mut text = String::new();
        for (plain, body) in &parts {
            text.push_str(plain);
            text.push_str("[%");
            text.push_str(body);
            text.push(']');
        }
        let out = strip_mark_codes(&text);
        prop_assert!(!out.contains("[%"), "{out:?}");
        let expected: String = parts.iter().map(|(p, _)| p.as_str()).collect::<String>();
        // Spaces that follow a block are swallowed, so compare without spaces.
        let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        prop_assert_eq!(squash(&out), squash(&expected));
    }
}

#[track_caller]
fn assert_fast(label: &str, start: Instant) {
    let took = start.elapsed();
    assert!(took < TIME_LIMIT, "{label} took {took:?}");
}

#[test]
fn one_megabyte_of_open_brackets_is_processed_in_linear_time() {
    let text = "[%".repeat(512 * 1024);
    let t = Instant::now();
    assert_eq!(strip_mark_codes(&text), text.as_str());
    assert_fast("strip", t);
    let t = Instant::now();
    assert!(embedded_commands(&text).is_empty());
    assert_fast("embedded_commands", t);
    let t = Instant::now();
    assert_eq!(eval_of(&text), None);
    assert_eq!(clock_of(&text), None);
    assert_eq!(emt_of(&text), None);
    assert_fast("eval/clk/emt", t);
    let t = Instant::now();
    let m = mark(MarkKind::Circle, "e4", None, "red");
    assert!(toggle_mark(&text, &m).ends_with(" [%draw circle,e4,red]"));
    assert_fast("toggle", t);
}

#[test]
fn one_megabyte_of_unclosed_command_with_long_body_is_linear() {
    let text = format!("[%draw {}", "a,".repeat(512 * 1024));
    let t = Instant::now();
    assert_eq!(strip_mark_codes(&text), text.as_str());
    assert!(embedded_commands(&text).is_empty());
    assert_eq!(eval_of(&text), None);
    assert_fast("unclosed", t);
}

#[test]
fn one_megabyte_of_adjacent_blocks_is_linear() {
    let text = "[%a]".repeat(256 * 1024);
    let t = Instant::now();
    assert_eq!(strip_mark_codes(&text), "");
    assert_eq!(embedded_commands(&text).len(), 256 * 1024);
    assert_fast("adjacent", t);
    let text = "[%clk 0:00:01]".repeat(80_000);
    let t = Instant::now();
    assert_eq!(clock_of(&text), Some(1));
    assert_fast("clk", t);
}

#[test]
fn many_draw_blocks_toggle_in_linear_time() {
    let text = "[%draw full,a1,red] ".repeat(50_000);
    let t = Instant::now();
    let m = mark(MarkKind::Circle, "a1", None, "red");
    let out = toggle_mark(&text, &m);
    assert_eq!(out, "[%draw circle,a1,red]");
    assert_fast("toggle many", t);
}

#[test]
fn long_runs_of_signs_and_digits_do_not_slow_the_eval_fallback() {
    let t = Instant::now();
    assert_eq!(eval_of(&"1-".repeat(512 * 1024)), None);
    assert_eq!(eval_of(&"-a".repeat(512 * 1024)), None);
    assert_eq!(eval_of(&"+".repeat(1 << 20)), None);
    assert!(eval_of(&format!("[%eval {}]", "9".repeat(1 << 20))).is_none_or(f32::is_finite));
    assert_fast("eval fallback", t);
}

#[test]
fn deeply_repeated_brackets_inside_a_block_do_not_overflow_the_stack() {
    let text = format!("[%x {}]", "[".repeat(200_000));
    assert_eq!(strip_mark_codes(&text), text.as_str());
    let text = format!("[%x {}]", "]".repeat(200_000));
    assert_eq!(strip_mark_codes(&text), "]".repeat(200_000));
}

// ---------- observations ----------

/// Scid reads `%eval ` even without the closing `]`; this crate only reads
/// closed blocks (so an unclosed one falls back to the first `+n`/`-n`).
#[test]
fn observation_unclosed_eval_block_is_not_a_command() {
    assert_eq!(eval_of("[%eval 1.5"), None);
}

/// Scid stops its fallback scan at the first `+`/`-` and fails when it is not
/// followed by a digit (or is a `1-0` result); this crate keeps searching.
#[test]
fn observation_eval_fallback_keeps_searching_after_a_result() {
    assert_eq!(eval_of("a - b +1.5"), Some(1.5));
    assert_eq!(eval_of("1-0 then +2.0"), Some(2.0));
}
