//! M5-2b contract tests, app side: the *Best Games* section of the tree
//! (spec `m5-2-janela-da-arvore.md`, D1600-D1612; Scid `tree.tcl` `best`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::{
    AppNotice, BEST_DEFAULT_MAX, BEST_MAX_CHOICES, BasesNotice, BestColumn, BestMessage, BestOrder,
    BestResults, PickMode,
};
use tempfile::tempdir;

fn best(rig: &mut Rig, message: BestMessage) {
    rig.send(TreeMessage::Best(message));
}

fn numbers(rig: &Rig) -> Vec<u32> {
    rig.view()
        .best
        .map(|b| b.rows.iter().map(|r| r.game.number).collect())
        .unwrap_or_default()
}

fn selected(rig: &Rig) -> Vec<u32> {
    rig.view()
        .best
        .map(|b| b.selected.iter().copied().collect())
        .unwrap_or_default()
}

fn with_best(dir: &std::path::Path) -> Rig {
    let mut rig = Rig::std_with_tree(dir);
    best(&mut rig, BestMessage::Toggle);
    rig
}

fn notice(rig: &Rig) -> Option<&BasesNotice> {
    match &rig.state.notice {
        Some(AppNotice::Bases(n)) => Some(n),
        _ => None,
    }
}

/// Four rated games from the start position.
const RATED: [(&str, &str, u16, u16); 4] = [
    ("1. e4 e5", "1-0", 2000, 2000),
    ("1. e4 c5", "0-1", 2700, 2600),
    ("1. d4 d5", "1/2-1/2", 2500, 2500),
    ("1. e4 e6", "1-0", 2300, 2400),
];

fn rated_pgn() -> String {
    RATED
        .iter()
        .map(|(m, r, w, b)| game_pgn(m, r, *w, *b, "2001.01.01", None))
        .collect()
}

#[test]
fn the_section_is_closed_by_default_and_toggles_open_and_closed() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    assert!(rig.view().best.is_none());
    best(&mut rig, BestMessage::Toggle);
    assert!(rig.view().best.is_some());
    assert!(rig.state.tree().best().is_open());
    best(&mut rig, BestMessage::Toggle);
    assert!(rig.view().best.is_none());
}

#[test]
fn the_defaults_are_every_result_best_order_and_a_hundred_games() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    let view = rig.view().best.unwrap();
    assert_eq!(view.results, BestResults::All);
    assert_eq!(view.order, BestOrder::Rating);
    assert_eq!(view.max, 100);
    assert_eq!(BEST_DEFAULT_MAX, 100);
    assert_eq!(BEST_MAX_CHOICES, [20, 50, 100, 250, 500, 1000]);
    assert!(!view.columns_open);
    assert_eq!(view.columns.len(), 10);
    assert!(!view.columns.contains(&BestColumn::Event));
    assert_eq!(view.columns.first(), Some(&BestColumn::Number));
}

#[test]
fn the_list_has_every_game_that_reaches_the_position() {
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    let mut got = numbers(&rig);
    got.sort_unstable();
    assert_eq!(got, [1, 2, 3, 4, 5]);
}

#[test]
fn the_list_follows_the_position_of_the_game() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.play("e2e4");
    let mut got = numbers(&rig);
    got.sort_unstable();
    assert_eq!(got, [1, 3, 5]);
    let rows = rig.view().best.unwrap().rows;
    assert!(rows.iter().all(|r| r.ply == 1));
}

#[test]
fn rows_carry_the_next_moves_in_san_up_to_six() {
    let dir = tempdir().unwrap();
    let pgn = game_pgn(
        "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O",
        "1-0",
        0,
        0,
        "2001.01.01",
        None,
    );
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "one.pgn", &pgn);
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    rig.play("e2e4");
    let rows = rig.view().best.unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].next_moves, ["e5", "Nf3", "Nc6", "Bb5", "a6", "Ba4"]);
}

#[test]
fn the_result_filter_chooses_the_games() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    for (choice, expected) in [
        (BestResults::White, vec![1, 4]),
        (BestResults::Black, vec![2]),
        (BestResults::Decisive, vec![1, 2, 4]),
        (BestResults::Draw, vec![3]),
        (BestResults::Unfinished, vec![5]),
        (BestResults::All, vec![1, 2, 3, 4, 5]),
    ] {
        best(&mut rig, BestMessage::SetResults(choice));
        let mut got = numbers(&rig);
        got.sort_unstable();
        assert_eq!(got, expected, "{choice:?}");
        assert_eq!(rig.view().best.unwrap().results, choice);
    }
}

#[test]
fn changing_the_results_searches_again_and_closing_the_section_does_not() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    let before = rig.started();
    best(&mut rig, BestMessage::SetResults(BestResults::Draw));
    assert_eq!(rig.started(), before + 1);
    // The same choice again changes nothing.
    best(&mut rig, BestMessage::SetResults(BestResults::Draw));
    assert_eq!(rig.started(), before + 1);
    best(&mut rig, BestMessage::Toggle);
    assert_eq!(rig.started(), before + 1);
    assert!(rig.view().best.is_none());
}

#[test]
fn opening_the_section_searches_again_to_get_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let before = rig.started();
    best(&mut rig, BestMessage::Toggle);
    assert_eq!(rig.started(), before + 1);
}

#[test]
fn the_rating_order_puts_the_highest_average_first() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated", &rated_pgn());
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    // Averages: g1 2000, g2 2650, g3 2500, g4 2350.
    assert_eq!(numbers(&rig), [2, 3, 4, 1]);
    let rows = rig.view().best.unwrap().rows;
    assert_eq!(rows[0].avg_elo, 2650);
}

#[test]
fn the_database_order_keeps_the_last_games_of_the_filter() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated", &rated_pgn());
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    best(&mut rig, BestMessage::SetOrder(BestOrder::Database));
    best(&mut rig, BestMessage::SetMax(2));
    let mut got = numbers(&rig);
    got.sort_unstable();
    assert_eq!(got, [3, 4], "the last two games of the base");
}

#[test]
fn the_maximum_cuts_the_list_and_is_clamped_to_the_engine_limits() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    best(&mut rig, BestMessage::SetMax(2));
    assert_eq!(numbers(&rig).len(), 2);
    best(&mut rig, BestMessage::SetMax(0));
    assert_eq!(rig.view().best.unwrap().max, 1);
    assert_eq!(numbers(&rig).len(), 1);
    best(&mut rig, BestMessage::SetMax(5000));
    assert_eq!(rig.view().best.unwrap().max, 1000);
    assert_eq!(numbers(&rig).len(), 5);
}

#[test]
fn the_best_list_takes_the_rating_order_from_the_engine_for_unrated_games_too() {
    // All ratings zero: the list is complete, whatever the tie order.
    let dir = tempdir().unwrap();
    let rig = with_best(dir.path());
    assert!(rig.view().best.unwrap().rows.iter().all(|r| r.avg_elo == 0));
}

// ---- columns (D1603) ----

#[test]
fn a_column_is_turned_on_and_off_and_the_display_order_is_kept() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    best(&mut rig, BestMessage::ToggleColumn(BestColumn::Event));
    let columns = rig.view().best.unwrap().columns;
    assert_eq!(columns.len(), 11);
    assert_eq!(columns, BestColumn::ALL);
    best(&mut rig, BestMessage::ToggleColumn(BestColumn::White));
    let columns = rig.view().best.unwrap().columns;
    assert!(!columns.contains(&BestColumn::White));
    best(&mut rig, BestMessage::ToggleColumn(BestColumn::White));
    assert_eq!(rig.view().best.unwrap().columns, BestColumn::ALL);
}

#[test]
fn the_last_column_cannot_be_turned_off() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    for column in BestColumn::ALL {
        best(&mut rig, BestMessage::ToggleColumn(column));
    }
    let columns = rig.view().best.unwrap().columns;
    assert_eq!(columns.len(), 1, "one column is always left: {columns:?}");
    // Turning the one that is left "off" again changes nothing.
    let last = columns[0];
    best(&mut rig, BestMessage::ToggleColumn(last));
    assert_eq!(rig.view().best.unwrap().columns, [last]);
}

#[test]
fn columns_do_not_search_and_the_menu_toggles() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    let before = rig.started();
    best(&mut rig, BestMessage::ToggleColumn(BestColumn::Eco));
    best(&mut rig, BestMessage::ToggleColumnMenu);
    assert!(rig.view().best.unwrap().columns_open);
    best(&mut rig, BestMessage::ToggleColumnMenu);
    assert!(!rig.view().best.unwrap().columns_open);
    assert_eq!(rig.started(), before);
}

#[test]
fn closing_the_section_closes_the_column_menu_and_keeps_the_columns() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    best(&mut rig, BestMessage::ToggleColumn(BestColumn::Event));
    best(&mut rig, BestMessage::ToggleColumnMenu);
    best(&mut rig, BestMessage::Toggle);
    best(&mut rig, BestMessage::Toggle);
    let view = rig.view().best.unwrap();
    assert!(!view.columns_open);
    assert!(view.columns.contains(&BestColumn::Event));
}

// ---- selection (D1604) ----

fn pick(rig: &mut Rig, number: u32, mode: PickMode) {
    best(rig, BestMessage::Pick { number, mode });
}

#[test]
fn a_plain_click_selects_only_that_row() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 2, PickMode::Only);
    pick(&mut rig, 4, PickMode::Only);
    assert_eq!(selected(&rig), [4]);
}

#[test]
fn toggle_adds_and_removes_rows() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 2, PickMode::Only);
    pick(&mut rig, 4, PickMode::Toggle);
    assert_eq!(selected(&rig), [2, 4]);
    pick(&mut rig, 2, PickMode::Toggle);
    assert_eq!(selected(&rig), [4]);
    pick(&mut rig, 4, PickMode::Toggle);
    assert!(selected(&rig).is_empty());
}

#[test]
fn a_range_follows_the_order_of_the_list_not_the_numbers() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated", &rated_pgn());
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    assert_eq!(numbers(&rig), [2, 3, 4, 1]);
    pick(&mut rig, 3, PickMode::Only);
    pick(&mut rig, 1, PickMode::Range);
    assert_eq!(selected(&rig), [1, 3, 4]);
    // From the anchor upwards too.
    pick(&mut rig, 2, PickMode::Range);
    assert_eq!(selected(&rig), [2, 3]);
}

#[test]
fn a_range_without_an_anchor_selects_the_clicked_row() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 3, PickMode::Range);
    assert_eq!(selected(&rig), [3]);
}

#[test]
fn clicking_a_game_that_is_not_in_the_list_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 1, PickMode::Only);
    pick(&mut rig, 99, PickMode::Only);
    pick(&mut rig, 0, PickMode::Toggle);
    assert_eq!(selected(&rig), [1]);
}

#[test]
fn the_selection_keeps_only_the_games_that_stay_in_a_new_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 1, PickMode::Only);
    pick(&mut rig, 2, PickMode::Toggle);
    best(&mut rig, BestMessage::SetResults(BestResults::White));
    assert_eq!(selected(&rig), [1], "game 2 (0-1) left the list");
}

#[test]
fn the_selection_follows_the_position_through_the_new_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 2, PickMode::Only);
    pick(&mut rig, 3, PickMode::Toggle);
    rig.play("e2e4");
    assert_eq!(selected(&rig), [3], "game 2 does not reach 1.e4");
}

// ---- life cycle (D1609) ----

#[test]
fn closing_the_panel_erases_the_list_and_the_selection() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    pick(&mut rig, 1, PickMode::Only);
    rig.send(TreeMessage::Close);
    assert!(numbers(&rig).is_empty());
    assert!(selected(&rig).is_empty());
    // Reopening the panel and the section: a fresh list, nothing selected.
    rig.send(TreeMessage::Open);
    if rig.view().best.is_none() {
        best(&mut rig, BestMessage::Toggle);
    }
    assert_eq!(numbers(&rig).len(), 5);
    assert!(selected(&rig).is_empty());
}

#[test]
fn a_base_without_games_gives_an_empty_list() {
    let mut rig = Rig::new();
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    assert!(numbers(&rig).is_empty());
}

#[test]
fn a_position_nobody_reached_gives_an_empty_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.play("a2a3");
    assert!(numbers(&rig).is_empty());
}

#[test]
fn a_stale_answer_never_replaces_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    // The search of 1.e4 flies; the user goes back to the start meanwhile.
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    let mut first = jobs(rig.raw(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }));
    assert_eq!(first.len(), 1);
    let old = first.remove(0);
    rig.raw(GameMessage::Navigate(Nav::Back(1)));
    // The old answer (the list of 1.e4: games 1, 3, 5) arrives first.
    let answer = rig.run_job(old);
    let mut queue = rig.raw(answer);
    assert!(
        numbers(&rig).len() != 3,
        "the list of the old position is not shown"
    );
    while let Some(effect) = queue.pop() {
        if effect.runs_on_host() {
            continue;
        }
        let message = rig.perform(effect);
        queue.extend(rig.raw(message));
    }
    let mut got = numbers(&rig);
    got.sort_unstable();
    assert_eq!(got, [1, 2, 3, 4, 5], "the list of the start position");
}

#[test]
fn an_answer_delivered_twice_does_not_duplicate_the_list() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let mut sent = jobs(rig.raw(TreeMessage::Best(BestMessage::Toggle)));
    assert_eq!(sent.len(), 1);
    let answer = rig.run_job(sent.remove(0));
    rig.raw(answer.clone());
    let count = numbers(&rig).len();
    rig.raw(answer);
    assert_eq!(numbers(&rig).len(), count);
}

#[test]
fn while_searching_the_old_list_is_still_visible() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    let before = numbers(&rig);
    let sent = jobs(rig.raw(TreeMessage::Best(BestMessage::SetResults(
        BestResults::Draw,
    ))));
    assert_eq!(sent.len(), 1);
    assert!(rig.view().searching);
    assert_eq!(
        numbers(&rig),
        before,
        "the list is only replaced by the answer"
    );
}

// ---- copy and browse (D1606, D1608) ----

#[test]
fn copy_without_selection_notifies_and_touches_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    let effects = rig.raw(TreeMessage::Best(BestMessage::Copy));
    assert!(effects.is_empty());
    assert_eq!(notice(&rig), Some(&BasesNotice::NothingSelected));
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 0);
}

#[test]
fn browse_without_selection_notifies_and_opens_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.send(TreeMessage::Best(BestMessage::Browse));
    assert_eq!(notice(&rig), Some(&BasesNotice::NothingSelected));
    assert!(rig.state.browser.is_none());
}

#[test]
fn copy_sends_the_selected_games_to_the_clipbase_and_keeps_the_list_selection() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    let slot = rig.state.bases.current();
    // The list of games has its own selection (game 5).
    rig.send(BasesMessage::List(ListMessage::SelectNumbers {
        slot,
        numbers: vec![5],
    }));
    let before = rig.state.bases.slot(slot).unwrap().list.selection.clone();
    pick(&mut rig, 1, PickMode::Only);
    pick(&mut rig, 4, PickMode::Toggle);
    rig.send(TreeMessage::Best(BestMessage::Copy));
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).unwrap();
    assert_eq!(clip.total(), 2);
    assert_eq!(
        rig.state.bases.slot(slot).unwrap().list.selection,
        before,
        "the selection of the list is restored"
    );
    assert_eq!(selected(&rig), [1, 4], "and the one of the section is kept");
}

#[test]
fn copy_when_the_target_is_the_clipbase_says_so() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let source = rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(Message::Prefs(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::ConfirmCopy(false),
    )));
    rig.send(BasesMessage::List(ListMessage::CopyGames {
        from: source,
        to: SlotId::CLIPBASE,
        scope: mychess_app::CopyScope::Filter,
    }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    assert!(rig.view().best.unwrap().in_clipbase);
    pick(&mut rig, 1, PickMode::Only);
    rig.send(TreeMessage::Best(BestMessage::Copy));
    assert_eq!(notice(&rig), Some(&BasesNotice::AlreadyInClipbase));
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 5);
}

#[test]
fn browse_opens_the_browser_with_the_selection_in_list_order() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated", &rated_pgn());
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    pick(&mut rig, 1, PickMode::Only);
    pick(&mut rig, 2, PickMode::Toggle);
    pick(&mut rig, 4, PickMode::Toggle);
    rig.send(TreeMessage::Best(BestMessage::Browse));
    let browser = rig.state.browser.as_ref().expect("browser");
    let shown: Vec<u32> = browser.games.iter().map(|g| g.number).collect();
    assert_eq!(shown, [2, 4, 1], "the order of the list, not of the clicks");
}

// ---- load (D1605) ----

#[test]
fn double_click_loads_the_game_at_the_ply_of_the_position() {
    let dir = tempdir().unwrap();
    let pgn = format!(
        "{}{}",
        game_pgn(
            "1. Nf3 Nf6 2. Ng1 Ng8 3. e4 e5 4. Nf3",
            "1-0",
            0,
            0,
            "2001.01.01",
            None
        ),
        game_pgn("1. e4 c5", "0-1", 0, 0, "2001.01.01", None),
    );
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "t.pgn", &pgn);
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    rig.play("e2e4");
    let rows = rig.view().best.unwrap().rows;
    let plies: std::collections::BTreeMap<u32, u8> =
        rows.iter().map(|r| (r.game.number, r.ply)).collect();
    assert_eq!(plies.get(&1), Some(&5), "reached by transposition");
    assert_eq!(plies.get(&2), Some(&1));
    best(&mut rig, BestMessage::Load(1));
    // The game was edited: the usual question comes first.
    rig.send(Message::AnswerDiscard(mychess_app::DiscardChoice::DontSave));
    assert_eq!(rig.ply(), 5);
    assert_eq!(
        rig.state.session.position().fen().split(' ').next(),
        Some("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR")
    );
}

#[test]
fn load_activates_the_target_base() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let first = rig.open_pgn(dir.path(), "a.pgn", STD);
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    rig.send(TreeMessage::SetLocked(true));
    rig.open_pgn(dir.path(), "b.pgn", "[Result \"1-0\"]\n\n1. c4 c5 1-0\n\n");
    assert_ne!(rig.state.bases.current(), first);
    best(&mut rig, BestMessage::Load(2));
    assert_eq!(rig.state.bases.current(), first);
}

#[test]
fn load_of_a_game_outside_the_list_or_out_of_range_does_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    best(&mut rig, BestMessage::SetResults(BestResults::Draw));
    let nodes = rig.nodes();
    best(&mut rig, BestMessage::Load(1));
    best(&mut rig, BestMessage::Load(0));
    best(&mut rig, BestMessage::Load(99));
    assert_eq!(rig.nodes(), nodes);
    assert_eq!(rig.ply(), 0);
}

// ---- more edges ----

#[test]
fn a_big_base_is_cut_at_the_maximum_and_the_default_is_a_hundred() {
    let dir = tempdir().unwrap();
    let pgn: String = (0..120)
        .map(|_| game_pgn("1. e4 e5", "1-0", 0, 0, "2001.01.01", None))
        .collect();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "big.pgn", &pgn);
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    assert_eq!(numbers(&rig).len(), 100);
    best(&mut rig, BestMessage::SetMax(20));
    assert_eq!(numbers(&rig).len(), 20);
    best(&mut rig, BestMessage::SetMax(250));
    assert_eq!(numbers(&rig).len(), 120);
}

#[test]
fn the_list_works_while_training_hides_the_moves() {
    let dir = tempdir().unwrap();
    let mut rig = with_best(dir.path());
    rig.send(TreeMessage::SetTraining(true));
    assert!(rig.state.tree().training());
    let mut got = numbers(&rig);
    got.sort_unstable();
    assert_eq!(got, [1, 2, 3, 4, 5]);
}

#[test]
fn actions_on_a_list_of_a_slot_that_closed_do_not_panic_or_act() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_pgn(dir.path(), "a.pgn", STD);
    let b = rig.open_pgn(dir.path(), "b.pgn", "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n");
    rig.send(BasesMessage::Select(a));
    rig.open_tree();
    rig.send(TreeMessage::SetLocked(true));
    best(&mut rig, BestMessage::Toggle);
    pick(&mut rig, 1, PickMode::Only);
    rig.send(BasesMessage::Select(b));
    // Closing `a` starts a new search; the old list is still on screen.
    let sent = rig.raw(BasesMessage::Close(a));
    let nodes = rig.nodes();
    rig.raw(TreeMessage::Best(BestMessage::Load(1)));
    rig.raw(TreeMessage::Best(BestMessage::Copy));
    rig.raw(TreeMessage::Best(BestMessage::Browse));
    assert_eq!(rig.nodes(), nodes);
    assert!(rig.state.browser.is_none());
    assert_eq!(rig.state.bases.slot(SlotId::CLIPBASE).unwrap().total(), 0);
    // Finish the search: the list is the one of `b`.
    let mut queue = sent;
    while let Some(effect) = queue.pop() {
        if effect.runs_on_host() {
            continue;
        }
        let message = rig.perform(effect);
        queue.extend(rig.raw(message));
    }
    assert_eq!(numbers(&rig), [1]);
}

#[test]
fn the_selection_does_not_cross_to_another_database_with_the_same_numbers() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_pgn(dir.path(), "a.pgn", STD);
    let b = rig.open_pgn(dir.path(), "b.pgn", "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n");
    rig.send(BasesMessage::Select(a));
    rig.open_tree();
    best(&mut rig, BestMessage::Toggle);
    pick(&mut rig, 1, PickMode::Only);
    // The tree follows the current base: the list is now the one of `b`,
    // whose game 1 is another game.
    rig.send(BasesMessage::Select(b));
    assert_eq!(numbers(&rig), [1]);
    assert!(
        selected(&rig).is_empty(),
        "game 1 of `a` was selected, not game 1 of `b`"
    );
}
