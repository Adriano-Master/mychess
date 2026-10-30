//! The MultiPV lines, the history, the information lines and the states of
//! a section (spec M4-4, "Comportamento observavel" 2-5, regras 3, 11, 12,
//! D449, D450, D812, D814). Headless, on the real window.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod m4_4_gui_common;
mod render_common;

use m4_4_gui_common::*;
use mychess_app::engines::Protocol;
use mychess_app::{Display, GameMessage, Nav, PlayOptions, format_score, format_seconds};
use mychess_core::Game;
use mychess_engine::{Bound, Score};
use render_common::{SCALE, render};

fn play_ucis(rig: &mut Rig, ucis: &[&str]) {
    for u in ucis {
        let mv = rig.shell.app.session.position().parse_uci(u).expect("uci");
        rig.app_msg(AppMessage::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
    }
}

fn mainline(rig: &Rig) -> Vec<String> {
    let game = rig.game();
    let mut pos = game.start_position().clone();
    game.mainline()
        .filter(|id| *id != game.root())
        .map(|id| {
            let mv = game.mv(id).expect("mv");
            let u = pos.uci(mv);
            pos.play(mv).expect("legal");
            u
        })
        .collect()
}

// ---- the lines ----

#[test]
fn every_line_shows_its_number_score_and_depth() {
    let (rig, _run) = uci_run_with_lines();
    // Scores 40 - 10 n: +0.30, +0.20, +0.10 (the header repeats the first).
    for score in ["+0.30", "+0.20", "+0.10"] {
        assert!(has(&rig, score), "{score}");
    }
    assert_eq!(count_text(&rig, "+0.30"), 2, "header and line 1");
    for n in ["1", "2", "3"] {
        assert!(has(&rig, n), "line number {n}");
    }
    assert!(has(&rig, "12"), "the depth of the lines");
}

#[test]
fn lines_are_numbered_by_their_multipv_even_when_they_arrive_out_of_order() {
    // D794: line 3 before line 2; the GUI numbers by `PvLine::multipv`.
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 3)));
    started(&mut rig, run, 1);
    let start = Position::new();
    info(&mut rig, run, report(&start, 1, 3, 12, 10, &["g1f3"]));
    info(&mut rig, run, report(&start, 1, 1, 12, 30, &["e2e4"]));
    info(&mut rig, run, report(&start, 1, 2, 12, 20, &["d2d4"]));
    let order: Vec<u8> = view(&rig, run).lines.iter().map(|l| l.multipv).collect();
    assert_eq!(order, [1, 2, 3]);
    // The three "+" buttons of the lines send their own numbers.
    for n in 1..=3u8 {
        // Index 0 is the stepper's "+".
        let b = nth_text_bounds(&rig, "+", usize::from(n));
        let messages = click_at(&rig, b.center());
        assert!(
            is_am(&messages, &AnalysisMessage::AddLineNumber(run, n)),
            "line {n}: {messages:?}"
        );
    }
}

#[test]
fn the_plus_of_a_line_is_dead_when_the_position_is_locked() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
    for n in 1..=3 {
        let b = nth_text_bounds(&rig, "+", n);
        assert!(click_at(&rig, b.center()).is_empty(), "locked, line {n}");
    }
}

#[test]
fn the_plus_of_a_line_is_dead_when_the_analysis_is_stopped() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::Toggle(run)));
    // Rule 11: the lines stay visible while stopped.
    for n in 1..=3 {
        let b = nth_text_bounds(&rig, "+", n);
        assert!(click_at(&rig, b.center()).is_empty(), "stopped, line {n}");
    }
}

#[test]
fn the_first_line_is_drawn_in_bold() {
    // Two lines with the very same score and depth: only the weight differs.
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 2)));
    started(&mut rig, run, 1);
    let start = Position::new();
    info(&mut rig, run, report(&start, 1, 1, 12, 30, &["e2e4"]));
    info(&mut rig, run, report(&start, 1, 2, 12, 30, &["e2e4"]));
    // Texts "+0.30": header, line 1, line 2.
    assert_eq!(count_text(&rig, "+0.30"), 3);
    let shot = render(rig.shell.view(), SIZE);
    let ink = |r: iced::Rectangle| -> u32 {
        let (x0, y0) = ((r.x * SCALE) as u32, (r.y * SCALE) as u32);
        let (x1, y1) = (
            ((r.x + r.width) * SCALE) as u32,
            ((r.y + r.height) * SCALE) as u32,
        );
        let mut count = 0u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = shot.pixel(x, y);
                // The section's background is light; text is darker.
                if u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 3 * 128 {
                    count += 1;
                }
            }
        }
        count
    };
    let line1 = nth_text_bounds(&rig, "+0.30", 1);
    let line2 = nth_text_bounds(&rig, "+0.30", 2);
    // The text is left aligned in a fixed-width box: compare the box of the glyphs.
    let (b1, b2) = (ink(line1), ink(line2));
    assert!(b1 > 0 && b2 > 0, "both have ink ({b1}, {b2})");
    assert!(b1 > b2, "the first line has more ink (bold): {b1} vs {b2}");
}

// ---- clicking a move of the PV (D812) ----

/// The row of line `n`: y of its score text and the x where its moves start.
fn pv_origin(rig: &Rig, score: &str, nth: usize) -> (f32, f32) {
    let s = nth_text_bounds(rig, score, nth);
    // score box (64) + depth box (30) + two gaps of 6.
    (s.center_y(), s.x + 64.0 + 6.0 + 30.0 + 6.0)
}

#[test]
fn every_move_of_a_pv_is_a_link_to_its_index_and_clicks_grow_with_x() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    info(
        &mut rig,
        run,
        report(&start, 1, 1, 12, 30, &["e2e4", "e7e5", "g1f3", "b8c6"]),
    );
    assert_eq!(view(&rig, run).lines[0].text, "1.e4 e5 2.Nf3 Nc6");
    let (y, x0) = pv_origin(&rig, "+0.30", 1);
    let mut seen = std::collections::BTreeSet::new();
    let mut last = 0usize;
    let mut x = x0;
    while x < x0 + 240.0 {
        for m in click_at(&rig, iced::Point::new(x, y)) {
            if let Message::Ui(UiMessage::Analysis(AnalysisUiMessage::PlayPv {
                run: r,
                line,
                index,
            })) = m
            {
                assert_eq!((r, line), (run, 1));
                assert!(index >= last, "indexes grow with x ({index} after {last})");
                last = index;
                seen.insert(index);
            }
        }
        x += 2.0;
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        [0, 1, 2, 3],
        "four moves, four links; the move numbers are not links"
    );
}

#[test]
fn clicking_a_move_plays_the_pv_up_to_it_on_the_board() {
    let (mut rig, run) = uci_run_with_lines();
    // Replace the lines with deeper ones.
    started(&mut rig, run, 2);
    let start = Position::new();
    info(
        &mut rig,
        run,
        report(&start, 2, 1, 14, 30, &["e2e4", "e7e5", "g1f3", "b8c6"]),
    );
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 2,
    }));
    assert_eq!(mainline(&rig), ["e2e4", "e7e5", "g1f3"]);
    assert_eq!(rig.shell.ui.analysis.focus, Some(run));
}

#[test]
fn clicking_the_first_move_plays_only_that_move() {
    let (mut rig, run) = uci_run_with_lines();
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 2,
        index: 0,
    }));
    assert_eq!(mainline(&rig), ["d2d4"], "line 2's first move");
}

#[test]
fn a_click_with_an_index_or_line_that_does_not_exist_does_nothing() {
    let (mut rig, run) = uci_run_with_lines();
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 9,
        index: 0,
    }));
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 99,
    }));
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run: mychess_app::engines::RunId(999),
        line: 1,
        index: 0,
    }));
    assert_eq!(mainline(&rig), Vec::<String>::new());
    assert!(
        notice(&rig).is_none()
            || !matches!(notice(&rig), Some(mychess_app::AppNotice::Analysis(_)))
    );
}

#[test]
fn lines_of_another_position_than_the_boards_are_not_played() {
    // Lines of the start position; the board moves on by two plies and the
    // engine has not answered yet. g1f3 and g8f6 are legal after 1.a3 a6, so
    // a missing check would put them in the game.
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    info(
        &mut rig,
        run,
        report(&start, 1, 1, 12, 30, &["g1f3", "g8f6"]),
    );
    play_ucis(&mut rig, &["a2a3", "a7a6"]);
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 1,
    }));
    assert_eq!(mainline(&rig), ["a2a3", "a7a6"], "D812: nothing was added");
}

#[test]
fn a_locked_analysis_does_not_play_its_lines_elsewhere() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLock(run)));
    play_ucis(&mut rig, &["a2a3", "a7a6"]);
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 0,
    }));
    assert_eq!(mainline(&rig), ["a2a3", "a7a6"]);
}

#[test]
fn a_pv_that_diverges_from_the_game_asks_before_replacing_and_does_not_go_on() {
    // The game has 1.e4 e5 and the cursor goes back to the start: the first
    // move of the PV (1.d4) is not the game's next move, so M2-1 asks.
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    play_ucis(&mut rig, &["e2e4", "e7e5"]);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Start)));
    started(&mut rig, run, 5);
    let start = Position::new();
    info(
        &mut rig,
        run,
        report(&start, 5, 1, 12, 30, &["d2d4", "d7d5", "c2c4"]),
    );
    assert!(!view(&rig, run).lines.is_empty());
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 2,
    }));
    assert!(
        rig.shell.app.session.prompt().is_some(),
        "the replacement question of the first move is open"
    );
    assert_eq!(
        mainline(&rig),
        ["e2e4", "e7e5"],
        "nothing changed before the answer"
    );
    assert_eq!(
        rig.shell.app.session.notice(),
        Some(&mychess_app::Notice::PromptPending),
        "the following moves got PromptPending"
    );
}

#[test]
fn a_pv_that_follows_the_game_only_moves_the_cursor() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    play_ucis(&mut rig, &["e2e4", "e7e5"]);
    rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Start)));
    started(&mut rig, run, 5);
    let start = Position::new();
    info(
        &mut rig,
        run,
        report(&start, 5, 1, 12, 30, &["e2e4", "e7e5"]),
    );
    rig.msg(aui(AnalysisUiMessage::PlayPv {
        run,
        line: 1,
        index: 1,
    }));
    assert_eq!(mainline(&rig), ["e2e4", "e7e5"], "no duplicates");
    assert!(rig.shell.app.session.prompt().is_none());
    assert_eq!(rig.game().node_count(), 3);
}

// ---- display modes (D449) ----

#[test]
fn the_middle_click_on_the_lines_cycles_the_display() {
    let (rig, run) = uci_run_with_lines();
    let messages = middle_click(&rig, "+0.20");
    assert!(
        is_am(&messages, &AnalysisMessage::CycleDisplay(run)),
        "{messages:?}"
    );
}

#[test]
fn the_hidden_mode_shows_only_the_hint_and_the_hint_brings_the_lines_back() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetDisplay(
        run,
        Display::Hidden,
    )));
    assert!(has(&rig, "Click here to see the moves"));
    assert!(!has(&rig, "+0.20"), "the lines are hidden");
    let messages = click_msgs(&rig, "Click here to see the moves");
    assert!(is_am(&messages, &AnalysisMessage::CycleDisplay(run)));
    apply(&mut rig, messages);
    assert_ne!(view(&rig, run).display, Display::Hidden);
    assert!(has(&rig, "+0.20"));
}

#[test]
fn the_wrap_mode_draws_the_same_lines() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetDisplay(
        run,
        Display::Wrap,
    )));
    for score in ["+0.30", "+0.20", "+0.10"] {
        assert!(has(&rig, score));
    }
    assert!(has(&rig, "Wrap"));
}

// ---- history (one line) ----

fn history_text(depth: u32, cp: i32, san: &str, ms: u64) -> String {
    format!(
        "{depth} [{}]  {san} ({} s)",
        format_score(Score::Cp(cp), Bound::Exact),
        format_seconds(ms)
    )
}

#[test]
fn the_history_of_a_single_line_lists_each_new_pv_in_the_scid_format() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    let mut a = report(&start, 1, 1, 10, 20, &["e2e4", "e7e5"]);
    a.time_ms = Some(1230);
    info(&mut rig, run, a);
    let mut b = report(&start, 1, 1, 11, 25, &["d2d4", "d7d5"]);
    b.time_ms = Some(2000);
    info(&mut rig, run, b);
    // The same PV again: not a new history line (regra 3).
    info(
        &mut rig,
        run,
        report(&start, 1, 1, 12, 26, &["d2d4", "d7d5"]),
    );
    assert_eq!(view(&rig, run).history.len(), 2);
    assert!(has(&rig, &history_text(10, 20, "1.e4 e5", 1230)));
    assert!(has(&rig, &history_text(11, 25, "1.d4 d5", 2000)));
}

#[test]
fn the_history_is_not_shown_with_more_than_one_line_and_comes_back_at_one() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    info(&mut rig, run, report(&start, 1, 1, 10, 20, &["e2e4"]));
    let entry = history_text(10, 20, "1.e4", 1000);
    assert!(has(&rig, &entry));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 2)));
    assert!(!has(&rig, &entry), "MultiPV 2: no history");
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::SetMultiPv(run, 1)));
    // Going back to one line clears it (regra 2).
    assert!(view(&rig, run).history.is_empty());
    assert!(!has(&rig, &entry));
}

#[test]
fn only_the_last_sixty_of_the_two_hundred_history_lines_are_drawn() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    let pvs = ["e2e4", "d2d4", "g1f3", "c2c4"];
    let san = ["1.e4", "1.d4", "1.Nf3", "1.c4"];
    for i in 0..250u32 {
        let k = (i % 4) as usize;
        info(&mut rig, run, report(&start, 1, 1, 100 + i, 10, &[pvs[k]]));
    }
    let kept = view(&rig, run).history.len();
    assert_eq!(kept, 200, "the application keeps 200");
    // Entries 190..250 are the newest 60 (depth 100 + i).
    let drawn = |i: u32| {
        has(
            &rig,
            &history_text(100 + i, 10, san[(i % 4) as usize], 1000),
        )
    };
    assert!(drawn(249), "the newest");
    assert!(drawn(190), "the 60th newest");
    assert!(!drawn(189), "the 61st is not drawn (D814)");
    assert!(!drawn(60), "old ones are not drawn");
}

// ---- information lines ----

#[test]
fn the_information_lines_show_depth_hash_load_and_the_current_move() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    // The root move list had 32 moves in the Scid's example; here 20.
    let nf3 = start.parse_uci("g1f3").unwrap();
    let mut cur = report(&start, 1, 1, 12, 35, &["e2e4"]);
    cur.score = None;
    cur.pv = Vec::new();
    cur.depth = None;
    cur.currmove = Some(nf3);
    cur.currmovenumber = Some(18);
    info(&mut rig, run, cur.clone());
    let mut main = report(&start, 1, 1, 12, 35, &["e2e4", "e7e5"]);
    main.seldepth = Some(20);
    main.nps = Some(987_000);
    main.hashfull_permille = Some(123);
    main.cpuload_permille = Some(396);
    main.tbhits = Some(0);
    info(&mut rig, run, main);
    cur.currmovenumber = Some(3);
    info(&mut rig, run, cur);
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleInfo(run)));
    let (first, second) =
        mychess_gui::windows::analysis_text::info_lines(&en(), view(&rig, run), true);
    let second = second.expect("UCI has two lines");
    assert!(first.starts_with("+0.35"), "{first}");
    assert!(first.contains("Depth: 12/20"), "{first}");
    assert!(second.contains("NPS: 987000"), "{second}");
    assert!(
        second.contains("Hash: 12%"),
        "hashfull/10 rounded: {second}"
    );
    assert!(second.contains("Load: 40%"), "cpuload/10 rounded: {second}");
    assert!(second.contains("TB: 0"), "{second}");
    // `maxmovenumber` is the largest seen, 18, not the last.
    assert!(second.contains("Current: Nf3 (3/18)"), "{second}");
    assert!(has(&rig, &first), "the first line is in the panel");
    assert!(has(&rig, &second), "the second line is in the panel");
}

#[test]
fn unknown_numbers_of_the_information_lines_are_dashes() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    let start = Position::new();
    let mut a = report(&start, 1, 1, 8, 10, &["e2e4"]);
    a.nps = None;
    info(&mut rig, run, a);
    let (_, second) = mychess_gui::windows::analysis_text::info_lines(&en(), view(&rig, run), true);
    let second = second.unwrap();
    assert!(second.contains("Hash: ----"), "{second}");
    assert!(second.contains("Load: ----"), "{second}");
    assert!(second.contains("Current: ----"), "{second}");
}

#[test]
fn an_xboard_engine_shows_only_depth_nodes_score_and_time() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::XBoard);
    let run = start_with(&mut rig, id, identity("Fake", None, false));
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
    );
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleInfo(run)));
    let (first, second) =
        mychess_gui::windows::analysis_text::info_lines(&en(), view(&rig, run), false);
    assert!(second.is_none());
    assert!(has(&rig, &first));
    let (_, uci_second) =
        mychess_gui::windows::analysis_text::info_lines(&en(), view(&rig, run), true);
    assert!(
        !has(&rig, &uci_second.unwrap()),
        "no NPS/Hash line for XBoard"
    );
}

#[test]
fn the_information_lines_in_portuguese_use_the_scid_words() {
    let mut rig = Rig::with_language(mychess_app::LanguagePref::PtBr);
    let run = uci_run(&mut rig);
    started(&mut rig, run, 1);
    info(
        &mut rig,
        run,
        report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
    );
    let (first, second) =
        mychess_gui::windows::analysis_text::info_lines(&pt(), view(&rig, run), true);
    assert!(first.contains("Prof.:"), "{first}");
    assert!(first.contains("N\u{f3}s:"), "{first}");
    assert!(first.contains("Tempo:"), "{first}");
    assert!(second.unwrap().contains("Carga:"));
}

// ---- states ----

#[test]
fn a_position_without_moves_shows_checkmate_or_stalemate_and_the_synthetic_score() {
    for (fen, word, score) in [
        ("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1", "(checkmate)", "+327.00"),
        ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", "(stalemate)", "+0.00"),
        ("8/8/8/8/8/6k1/6q1/7K w - - 0 1", "(checkmate)", "-327.00"),
    ] {
        let mut rig = Rig::new();
        let run = uci_run(&mut rig);
        let position = Position::from_fen(fen).expect("fen");
        let game = Game::from_position(position);
        rig.set_game(game);
        // Any message of the run makes the application look at the board.
        rig.app_msg(AppMessage::Game(GameMessage::Navigate(Nav::Start)));
        rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleInfo(run)));
        rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleInfo(run)));
        assert!(has(&rig, word), "{fen}: {word}");
        let zero = format_score(Score::Cp(0), Bound::Exact);
        let score = if score == "ZERO" {
            zero.as_str()
        } else {
            score
        };
        assert!(
            has(&rig, score),
            "{fen}: the score of getScoreNoMoves {score}"
        );
        assert!(
            !has(&rig, "stopped"),
            "{fen}: it is not 'stopped': the analysis is on"
        );
    }
}

#[test]
fn a_restarting_engine_says_so() {
    let (mut rig, run) = uci_run_with_lines();
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleLowPriority(
        run,
    )));
    assert!(view(&rig, run).restarting);
    assert!(has(&rig, "restarting..."));
    // The edit buttons are dead while restarting.
    for label in ["+ Move", "+ Line", "Lock"] {
        assert!(click_msgs(&rig, label).is_empty(), "{label}");
    }
}

#[test]
fn stopping_the_engine_shows_stopping_and_nothing_else_acts() {
    let mut rig = Rig::new();
    let id = add_engine(&mut rig, "Fake", Protocol::Uci);
    let run = start_with(&mut rig, id, identity("Fake", Some(1..=5), true));
    rig.app_msg(AppMessage::Engine(
        mychess_app::engines::EngineMessage::Stop(run),
    ));
    let state = &rig.shell.app.engine_runs.get(run).expect("run").state;
    assert_eq!(*state, mychess_app::engines::EngineRunState::Stopping);
    assert!(has(&rig, "stopping..."));
    assert!(click_msgs(&rig, "+ Move").is_empty());
}
