//! Contract tests of the single-game browser (spec M3-4, slice 4e: D595,
//! D597-D599, D602, D603; Scid `::gbrowser::new` 7, `load` 240, `LoadGame`
//! 379).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4e_common;

use m3_4e_common::*;
use mychess_app::{DiscardChoice, Inline};
use rstest::rstest;

fn board_part(fen: &str) -> &str {
    fen.split(' ').next().unwrap()
}

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";

// ---- opening ----

#[test]
fn opening_asks_for_the_read_as_an_effect_and_update_reads_nothing() {
    let (mut rig, slot, _d) = setup(5);
    let effects = rig.raw(open_msg(slot, &[3]));
    let reads: Vec<_> = effects
        .iter()
        .filter(|e| matches!(e, Effect::ReadBrowserGames { .. }))
        .collect();
    assert_eq!(reads.len(), 1, "{effects:?}");
    let Effect::ReadBrowserGames { numbers, .. } = reads[0] else {
        unreachable!()
    };
    assert_eq!(numbers, &[3]);
    // Nothing was read yet: the board waits.
    let browser = br(&rig);
    assert_eq!(browser.mode, BrowseMode::Single);
    assert!(browser.is_loading());
    assert!(matches!(browser.games[0].content, BrowserContent::Loading));
    assert!(rig.state.modal_open());
}

#[test]
fn the_single_browser_shows_header_board_and_main_line_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(
        dir.path(),
        "g.pgn",
        &[
            six(1),
            six(2)
                .eco("B90")
                .date("1992.??.??")
                .event("Linares")
                .result("1/2-1/2"),
        ],
    );
    open_browser(&mut rig, slot, &[2]);
    let ready = br(&rig).games[0].ready().expect("read");
    assert_eq!(ready.header.white, "W2");
    assert_eq!(ready.header.black, "B2");
    assert_eq!(ready.header.result.as_str(), "1/2-1/2");
    assert_eq!(ready.header.event, "Linares");
    assert_eq!(ready.header.date, "1992.??.??");
    assert_eq!(ready.header.eco, "B90");
    assert_eq!(board_part(&fen_of(&rig, 0)), START);
    let doc = ready.doc.as_ref().expect("text of the single browser");
    let sans: Vec<String> = doc
        .blocks
        .iter()
        .flat_map(|b| &b.inlines)
        .filter_map(|i| match i {
            Inline::Move { san, .. } => Some(san.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(sans, ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
}

#[rstest]
#[case("????.??.??")]
#[case("2020.??.??")]
#[case("2020.05.??")]
fn unknown_parts_of_the_date_keep_their_question_marks(#[case] date: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "g.pgn", &[six(1).date(date), six(2)]);
    open_browser(&mut rig, slot, &[1]);
    assert_eq!(br(&rig).games[0].ready().unwrap().header.date, date);
}

#[test]
fn a_game_without_eco_has_an_empty_code() {
    let (mut rig, slot, _d) = setup(2);
    open_browser(&mut rig, slot, &[1]);
    assert_eq!(br(&rig).games[0].ready().unwrap().header.eco, "");
}

#[test]
fn a_single_game_opens_at_the_start_when_the_filter_has_no_ply() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[2]);
    assert_eq!(plies(&rig), [0]);
}

#[test]
fn a_number_above_the_total_opens_the_last_game_zero_opens_nothing() {
    let (mut rig, slot, _d) = setup(4);
    open_browser(&mut rig, slot, &[999]);
    assert_eq!(numbers_shown(&rig), [4]);
    send(&mut rig, BrowseMessage::Close);
    open_browser(&mut rig, slot, &[0]);
    assert!(rig.state.browser.is_none(), "zero is not a game");
    assert!(!rig.state.modal_open());
}

#[test]
fn opening_a_slot_that_is_not_open_or_a_game_of_an_empty_base_does_nothing() {
    let (mut rig, _slot, _d) = setup(2);
    let effects = rig.raw(open_msg(sel(7), &[1]));
    assert!(effects.is_empty());
    assert!(rig.state.browser.is_none());
    // The Clipbase is empty.
    let effects = rig.raw(open_msg(SlotId::CLIPBASE, &[1]));
    assert!(effects.is_empty());
    assert!(rig.state.browser.is_none());
}

#[test]
fn the_browser_opens_on_a_slot_that_is_not_the_current_one() {
    let (mut rig, first, dir) = setup(3);
    let second = rig.open_games(dir.path(), "h.pgn", &sixes(2));
    assert_ne!(first, second);
    open_browser(&mut rig, first, &[2]);
    assert_eq!(br(&rig).slot, first);
    assert_eq!(numbers_shown(&rig), [2]);
}

// ---- stepping ----

#[test]
fn the_board_follows_the_ply() {
    let (mut rig, slot, _d) = setup(2);
    open_browser(&mut rig, slot, &[1]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(0),
            delta: 2,
        },
    );
    assert_eq!(plies(&rig), [2]);
    assert_eq!(
        board_part(&fen_of(&rig, 0)),
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR"
    );
    assert!(
        br(&rig).games[0].ready().unwrap().last_move().is_some(),
        "the move that led here is known"
    );
    send(&mut rig, BrowseMessage::GoToEnd { game: Some(0) });
    assert_eq!(plies(&rig), [6]);
    send(&mut rig, BrowseMessage::GoToStart { game: Some(0) });
    assert_eq!(plies(&rig), [0]);
    assert!(br(&rig).games[0].ready().unwrap().last_move().is_none());
    assert_eq!(board_part(&fen_of(&rig, 0)), START);
}

#[rstest]
#[case(0, -1, 0)]
#[case(0, 1, 1)]
#[case(0, -10, 0)]
#[case(0, 10, 6)]
#[case(3, 10, 6)]
#[case(3, -10, 0)]
#[case(6, 1, 6)]
#[case(6, -1, 5)]
#[case(6, i32::MAX, 6)]
#[case(0, i32::MIN, 0)]
#[case(6, i32::MIN, 0)]
fn steps_are_clamped_to_the_game(#[case] from: u32, #[case] delta: i32, #[case] expected: usize) {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    send(&mut rig, BrowseMessage::GoToPly { game: 0, ply: from });
    send(
        &mut rig,
        BrowseMessage::Step {
            game: Some(0),
            delta,
        },
    );
    assert_eq!(plies(&rig), [expected]);
}

#[test]
fn going_to_a_ply_out_of_range_clamps_to_the_end() {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    send(&mut rig, BrowseMessage::GoToPly { game: 0, ply: 4 });
    assert_eq!(plies(&rig), [4]);
    send(&mut rig, BrowseMessage::GoToPly { game: 0, ply: 7 });
    assert_eq!(plies(&rig), [6]);
    send(
        &mut rig,
        BrowseMessage::GoToPly {
            game: 0,
            ply: u32::MAX,
        },
    );
    assert_eq!(plies(&rig), [6]);
    send(&mut rig, BrowseMessage::GoToPly { game: 5, ply: 1 });
    assert_eq!(plies(&rig), [6], "a board that does not exist is ignored");
}

#[test]
fn flip_toggles_the_orientation_and_leaves_the_ply() {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 3,
        },
    );
    assert!(!br(&rig).games[0].flipped);
    send(&mut rig, BrowseMessage::Flip { game: None });
    assert!(br(&rig).games[0].flipped);
    send(&mut rig, BrowseMessage::Flip { game: Some(0) });
    assert!(!br(&rig).games[0].flipped);
    assert_eq!(plies(&rig), [3]);
}

#[test]
fn messages_without_a_browser_do_nothing() {
    let (mut rig, _slot, _d) = setup(2);
    for message in [
        BrowseMessage::Step {
            game: None,
            delta: 1,
        },
        BrowseMessage::GoToEnd { game: None },
        BrowseMessage::ToggleAutoplay,
        BrowseMessage::GoToGame(GameStep::Next),
        BrowseMessage::Focus(0),
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
        BrowseMessage::ToggleDeleted { game: 0 },
        BrowseMessage::AutoplayTick { generation: 3 },
        BrowseMessage::Close,
    ] {
        let effects = rig.raw(browse(message.clone()));
        assert!(effects.is_empty(), "{message:?}: {effects:?}");
        assert!(rig.state.browser.is_none());
    }
}

// ---- autoplay ----

#[test]
fn autoplay_advances_on_ticks_stops_at_the_end_and_ignores_stale_ticks() {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    let effects = rig.raw(browse(BrowseMessage::ToggleAutoplay));
    let ticks: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::BrowserTick { generation, .. } => Some(*generation),
            _ => None,
        })
        .collect();
    assert_eq!(ticks.len(), 1, "only one timer: {effects:?}");
    assert!(br(&rig).autoplay);
    let mut generation = ticks[0];
    // A tick of another run does nothing.
    let before = plies(&rig);
    let stale = rig.raw(browse(BrowseMessage::AutoplayTick {
        generation: generation.wrapping_add(5),
    }));
    assert!(stale.is_empty());
    assert_eq!(plies(&rig), before);
    for _ in 0..10 {
        let next = rig.raw(browse(BrowseMessage::AutoplayTick { generation }));
        let timers: Vec<u64> = next
            .iter()
            .filter_map(|e| match e {
                Effect::BrowserTick { generation, .. } => Some(*generation),
                _ => None,
            })
            .collect();
        assert!(timers.len() <= 1);
        match timers.first() {
            Some(g) => generation = *g,
            None => break,
        }
    }
    assert_eq!(plies(&rig), [6], "stops at the end");
    assert!(!br(&rig).autoplay);
    // A tick after the stop does nothing.
    let late = rig.raw(browse(BrowseMessage::AutoplayTick { generation }));
    assert!(late.is_empty());
    assert_eq!(plies(&rig), [6]);
}

#[test]
fn toggling_autoplay_twice_stops_it_and_a_pending_tick_is_dead() {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    let first = rig.raw(browse(BrowseMessage::ToggleAutoplay));
    let Some(Effect::BrowserTick { generation, .. }) = first.into_iter().next() else {
        panic!("a timer");
    };
    rig.raw(browse(BrowseMessage::ToggleAutoplay));
    assert!(!br(&rig).autoplay);
    let ply = plies(&rig);
    let tick = rig.raw(browse(BrowseMessage::AutoplayTick { generation }));
    assert!(tick.is_empty());
    assert_eq!(plies(&rig), ply);
    // Starting again gives a timer the old one cannot be mistaken for.
    let again = rig.raw(browse(BrowseMessage::ToggleAutoplay));
    let Some(Effect::BrowserTick { generation: g2, .. }) = again.into_iter().next() else {
        panic!("a timer");
    };
    assert_ne!(g2, generation);
}

#[test]
fn closing_the_browser_kills_the_autoplay() {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    let first = rig.raw(browse(BrowseMessage::ToggleAutoplay));
    let Some(Effect::BrowserTick { generation, .. }) = first.into_iter().next() else {
        panic!("a timer");
    };
    send(&mut rig, BrowseMessage::Close);
    let tick = rig.raw(browse(BrowseMessage::AutoplayTick { generation }));
    assert!(tick.is_empty());
    assert!(rig.state.browser.is_none());
}

#[test]
fn autoplay_at_the_end_of_the_game_does_not_loop() {
    let (mut rig, slot, _d) = setup(1);
    open_browser(&mut rig, slot, &[1]);
    send(&mut rig, BrowseMessage::GoToEnd { game: None });
    let effects = rig.raw(browse(BrowseMessage::ToggleAutoplay));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::BrowserTick { .. })),
        "{effects:?}"
    );
    assert!(!br(&rig).autoplay);
}

// ---- other games of the filter ----

fn remove_from_filter(rig: &mut Rig, slot: SlotId, numbers: &[u32]) {
    rig.select_all(slot, numbers);
    rig.send(l(ListMessage::RemoveSelected { slot }));
}

#[test]
fn the_game_keys_follow_the_filter_and_stop_at_the_extremes() {
    let (mut rig, slot, _d) = setup(6);
    remove_from_filter(&mut rig, slot, &[1, 3, 4]);
    assert_eq!(rig.filter_numbers(slot), [2, 5, 6]);
    open_browser(&mut rig, slot, &[5]);
    let shown = |rig: &Rig| numbers_shown(rig)[0];
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Previous));
    assert_eq!(shown(&rig), 2, "skips games out of the filter");
    // At the first game of the filter nothing changes, no effect.
    let effects = rig.raw(browse(BrowseMessage::GoToGame(GameStep::Previous)));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(shown(&rig), 2);
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Last));
    assert_eq!(shown(&rig), 6);
    let effects = rig.raw(browse(BrowseMessage::GoToGame(GameStep::Next)));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(shown(&rig), 6);
    send(&mut rig, BrowseMessage::GoToGame(GameStep::First));
    assert_eq!(shown(&rig), 2);
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Next));
    assert_eq!(shown(&rig), 5);
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Next));
    assert_eq!(shown(&rig), 6);
}

#[test]
fn a_game_outside_the_filter_can_be_browsed_and_next_goes_to_the_following_one() {
    let (mut rig, slot, _d) = setup(5);
    remove_from_filter(&mut rig, slot, &[2, 3]);
    open_browser(&mut rig, slot, &[3]);
    assert_eq!(numbers_shown(&rig), [3]);
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Next));
    assert_eq!(numbers_shown(&rig), [4]);
}

#[test]
fn an_empty_filter_game_keys_do_nothing() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert!(rig.filter_numbers(slot).is_empty());
    open_browser(&mut rig, slot, &[2]);
    for step in [
        GameStep::First,
        GameStep::Last,
        GameStep::Next,
        GameStep::Previous,
    ] {
        let effects = rig.raw(browse(BrowseMessage::GoToGame(step)));
        assert!(effects.is_empty(), "{step:?}");
        assert_eq!(numbers_shown(&rig), [2]);
    }
}

#[test]
fn changing_game_keeps_the_browser_a_single_one_and_reads_by_effect() {
    let (mut rig, slot, _d) = setup(3);
    open_browser(&mut rig, slot, &[1]);
    let effects = rig.raw(browse(BrowseMessage::GoToGame(GameStep::Next)));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ReadBrowserGames { numbers, .. } if numbers == &[2])),
        "{effects:?}"
    );
    assert!(br(&rig).is_loading());
    assert_eq!(br(&rig).mode, BrowseMode::Single);
}

// ---- loading into the main board ----

#[test]
fn load_puts_the_game_on_the_main_board_at_the_ply_shown_and_closes() {
    let (mut rig, slot, _d) = setup(3);
    open_browser(&mut rig, slot, &[2]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 3,
        },
    );
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert!(rig.state.browser.is_none(), "closed once the game is in");
    assert_eq!(editor_white(&rig), "W2");
    assert_eq!(editor_ply(&rig), 3);
    assert!(!rig.state.is_dirty());
}

#[test]
fn ctrl_load_keeps_the_browser_open_and_it_can_load_again_at_another_ply() {
    let (mut rig, slot, _d) = setup(3);
    open_browser(&mut rig, slot, &[3]);
    send(&mut rig, BrowseMessage::GoToEnd { game: None });
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: true,
        },
    );
    assert!(rig.state.browser.is_some());
    assert!(rig.state.modal_open());
    assert_eq!((editor_white(&rig), editor_ply(&rig)), ("W3".to_owned(), 6));
    send(&mut rig, BrowseMessage::GoToStart { game: None });
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: true,
        },
    );
    assert_eq!(editor_ply(&rig), 0);
}

#[test]
fn load_at_the_start_lands_at_the_start() {
    let (mut rig, slot, _d) = setup(2);
    open_browser(&mut rig, slot, &[2]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert_eq!(editor_ply(&rig), 0);
    assert_eq!(editor_white(&rig), "W2");
}

#[test]
fn a_game_loaded_from_an_si4_gets_the_new_origin_and_from_a_pgn_the_file_origin() {
    // D519: an `.si4` game never remembers the database (Save would write
    // over it); a PGN game remembers its file and index like a load from
    // the list does.
    let dir = tempfile::tempdir().unwrap();
    let path = make_si4(dir.path(), "db", &pgn(&sixes(3)));
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    open_browser(&mut rig, slot, &[2]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert_eq!(rig.state.origin, GameOrigin::New);
    assert_eq!(editor_white(&rig), "W2");

    let (mut rig, slot, _d) = setup(2);
    open_browser(&mut rig, slot, &[2]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert!(matches!(
        rig.state.origin,
        GameOrigin::PgnFile {
            index: 1,
            total: 2,
            ..
        }
    ));
    // The same as loading from the list.
    let browser_origin = rig.state.origin.clone();
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    assert_eq!(rig.state.origin, browser_origin);
}

#[test]
fn load_activates_the_slot_of_the_game() {
    let (mut rig, first, dir) = setup(3);
    let second = rig.open_games(dir.path(), "h.pgn", &[six(7), six(8)]);
    assert_eq!(rig.state.bases.current(), second);
    open_browser(&mut rig, first, &[1]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert_eq!(rig.state.bases.current(), first);
    assert_eq!(editor_white(&rig), "W1");
}

#[test]
fn load_of_a_board_that_does_not_exist_does_nothing() {
    let (mut rig, slot, _d) = setup(2);
    open_browser(&mut rig, slot, &[1]);
    let effects = rig.raw(browse(BrowseMessage::LoadIntoBoard {
        game: 9,
        keep_open: false,
    }));
    assert!(effects.is_empty());
    assert!(rig.state.browser.is_some());
}

#[test]
fn canceling_the_discard_question_keeps_the_browser_and_the_altered_game() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play(&mut rig, "d2d3");
    assert!(rig.state.is_dirty());
    let plies_before = editor_ply(&rig);
    open_browser(&mut rig, slot, &[3]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 2,
        },
    );
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert!(rig.state.discard_prompt.is_some(), "asks before replacing");
    assert!(
        rig.state.browser.is_some(),
        "the browser waits under the question"
    );
    assert_eq!(editor_white(&rig), "W1");
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(rig.state.browser.is_some(), "cancel keeps the browser open");
    assert_eq!(editor_white(&rig), "W1");
    assert!(rig.state.is_dirty());
    assert_eq!(editor_ply(&rig), plies_before);
    assert_eq!(plies(&rig), [2], "the browser keeps its place");
}

#[test]
fn discarding_loads_the_game_at_the_ply_and_then_closes_the_browser() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play(&mut rig, "d2d3");
    open_browser(&mut rig, slot, &[3]);
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 4,
        },
    );
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(rig.state.browser.is_none());
    assert_eq!(editor_white(&rig), "W3");
    assert_eq!(editor_ply(&rig), 4);
    assert!(!rig.state.is_dirty());
}

#[test]
fn keys_go_to_the_discard_question_while_it_is_open_over_the_browser() {
    let (mut rig, slot, _d) = setup(3);
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    play(&mut rig, "d2d3");
    open_browser(&mut rig, slot, &[3]);
    send(
        &mut rig,
        BrowseMessage::LoadIntoBoard {
            game: 0,
            keep_open: false,
        },
    );
    assert!(rig.state.discard_prompt.is_some());
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 1,
        },
    );
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Next));
    assert_eq!(
        plies(&rig),
        [0],
        "the browser does not move under the question"
    );
    assert_eq!(numbers_shown(&rig), [3]);
    // The ways out still work.
    send(&mut rig, BrowseMessage::Close);
    assert!(rig.state.browser.is_none());
}

// ---- unreadable game ----

/// An `.si4` whose game 2 cannot be decoded: the record bytes are
/// overwritten before the base is opened.
fn si4_with_broken_game(dir: &std::path::Path) -> (std::path::PathBuf, bool) {
    use mychess_db::GameSource;
    use mychess_db::si4::Si4Database;
    let path = make_si4(dir, "db", &pgn(&sixes(3)));
    let (offset, length) = {
        let db = Si4Database::open(&path).unwrap();
        let entry = db.entry(2).unwrap();
        (entry.offset as usize, entry.length as usize)
    };
    let sg4 = path.with_extension("sg4");
    let mut bytes = std::fs::read(&sg4).unwrap();
    for b in &mut bytes[offset..offset + length] {
        *b = 0xFF;
    }
    std::fs::write(&sg4, &bytes).unwrap();
    let mut db = Si4Database::open(&path).unwrap();
    let broken = GameSource::game(&mut db, 2).is_err();
    (path, broken)
}

#[test]
fn an_unreadable_game_is_reported_not_drawn_and_navigation_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let (path, broken) = si4_with_broken_game(dir.path());
    assert!(broken, "the fixture must make game 2 unreadable");
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    open_browser(&mut rig, slot, &[2]);
    assert!(matches!(
        br(&rig).games[0].content,
        BrowserContent::Unreadable(BaseError::GameUnreadable { number: 2 })
    ));
    assert!(br(&rig).games[0].ready().is_none());
    assert!(!br(&rig).is_loading());
    // Moving on an unreadable board is harmless.
    send(
        &mut rig,
        BrowseMessage::Step {
            game: None,
            delta: 3,
        },
    );
    send(&mut rig, BrowseMessage::GoToEnd { game: None });
    send(&mut rig, BrowseMessage::Flip { game: None });
    // The next game opens normally.
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Next));
    assert_eq!(numbers_shown(&rig), [3]);
    assert!(br(&rig).games[0].ready().is_some());
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Previous));
    send(&mut rig, BrowseMessage::GoToGame(GameStep::Previous));
    assert_eq!(numbers_shown(&rig), [1]);
    assert!(br(&rig).games[0].ready().is_some());
}

#[test]
fn loading_an_unreadable_game_into_the_board_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (path, broken) = si4_with_broken_game(dir.path());
    assert!(broken);
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    open_browser(&mut rig, slot, &[2]);
    let before = editor_white(&rig);
    let effects = rig.raw(browse(BrowseMessage::LoadIntoBoard {
        game: 0,
        keep_open: false,
    }));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(rig.state.browser.is_some());
    assert_eq!(editor_white(&rig), before);
}

#[test]
fn an_unreadable_game_in_a_grid_does_not_stop_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let (path, broken) = si4_with_broken_game(dir.path());
    assert!(broken);
    let mut rig = Rig::new();
    let slot = rig.open(&path);
    open_browser(&mut rig, slot, &[1, 2, 3]);
    let ok: Vec<bool> = br(&rig).games.iter().map(|g| g.ready().is_some()).collect();
    assert_eq!(ok, [true, false, true]);
    send(&mut rig, BrowseMessage::GoToStart { game: None });
    assert_eq!(plies(&rig), [0, usize::MAX, 0]);
}
