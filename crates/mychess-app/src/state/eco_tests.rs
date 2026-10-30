//! Sanity tests of the flow of the ECO tools (spec M5-7b). The full contract
//! suite is the tester's.
#![allow(clippy::large_futures)] // the test rig owns the whole `AppState` across awaits

use std::collections::VecDeque;

use mychess_core::{Color, EcoBook, Game, SanMode};

use super::AppState;
use crate::bases::{BasesMessage, ListJobKind, ListMessage};
use crate::eco_ui::{
    BrowserScope, ClassifyChoice, EcoMessage, EcoNotice, EcoWindow, NoveltyOutcome,
};
use crate::effect::{Effect, perform};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::shared::SharedEco;
use crate::state::update;

const BOOK: &str = "A00a \"Start\" *\n\
    B20 \"Sicilian Defence\" 1.e4 c5 *\n\
    C20 \"King's Pawn\" 1.e4 e5 *\n\
    C40 \"King's Knight\" 1.e4 e5 2.Nf3 *\n";

/// Five games: three with ECO tags (two B20, one C40), one without a tag and
/// one that only has `1.d4` (the start entry of the book matches it).
const GAMES: &str = "[Result \"1-0\"]\n[ECO \"B20\"]\n\n1. e4 c5 2. Nf3 1-0\n\n\
    [Result \"0-1\"]\n[ECO \"B20\"]\n\n1. e4 c5 0-1\n\n\
    [Result \"1/2-1/2\"]\n[ECO \"C40\"]\n\n1. e4 e5 2. Nf3 Nc6 1/2-1/2\n\n\
    [Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nf6 1-0\n\n\
    [Result \"*\"]\n\n1. d4 *\n\n";

async fn drive(state: &mut AppState, message: impl Into<Message>) {
    let mut queue: VecDeque<Effect> = update(state, message.into()).into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = perform(effect).await;
        queue.extend(update(state, answer));
    }
}

fn eco(message: EcoMessage) -> Message {
    Message::Eco(message)
}

fn book() -> SharedEco {
    SharedEco::new(EcoBook::parse(BOOK).expect("book"))
}

#[allow(clippy::field_reassign_with_default)] // private fields forbid the struct update
async fn opened_pgn() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("g.pgn");
    std::fs::write(&path, GAMES).expect("write");
    let mut state = AppState::default();
    state.open_pgn_in_slots = true;
    state.eco_book = Some(book());
    drive(&mut state, BasesMessage::Open(path)).await;
    assert_eq!(
        state
            .bases
            .current_slot()
            .map(super::super::bases::BaseSlot::total),
        Some(5)
    );
    (state, dir)
}

fn game_of(sans: &[&str]) -> Game {
    let mut game = Game::new();
    let mut node = game.root();
    for san in sans {
        let position = game.position_at(node).expect("position").clone();
        let parsed = position.parse_san(san, SanMode::Strict).expect("san");
        node = game.add_move(node, parsed.mv).expect("move");
    }
    game
}

fn notice(state: &AppState) -> Option<EcoNotice> {
    match &state.notice {
        Some(AppNotice::Eco(notice)) => Some(notice.clone()),
        _ => None,
    }
}

#[test]
fn only_nodes_of_the_eco_tree_are_valid_prefixes() {
    use super::eco_flow::valid_prefix;
    for ok in ["", "A", "E", "B9", "B90", "B90a", "C42z"] {
        assert!(valid_prefix(ok), "{ok}");
    }
    for bad in ["F", "b", "B9a", "B90A", "B90a1", "B9 0", "9"] {
        assert!(!valid_prefix(bad), "{bad}");
    }
}

#[test]
fn the_dates_of_last_year_and_last_month() {
    use mychess_core::PartialDate;

    use super::eco_flow::date_back;
    let today = PartialDate::new(Some(2026), Some(1), Some(15)).expect("date");
    let year = date_back(today, true).expect("year");
    assert_eq!(
        (year.year(), year.month(), year.day()),
        (Some(2025), Some(1), Some(15))
    );
    let month = date_back(today, false).expect("month");
    assert_eq!((month.year(), month.month()), (Some(2025), Some(12)));
}

#[test]
fn a_month_back_from_the_31st_keeps_the_day_without_clamping() {
    use mychess_core::PartialDate;

    use super::eco_flow::date_back;
    let today = PartialDate::new(Some(2026), Some(3), Some(31)).expect("date");
    let month = date_back(today, false).expect("month");
    assert_eq!(
        (month.year(), month.month(), month.day()),
        (Some(2026), Some(2), Some(31))
    );
}

#[tokio::test]
async fn the_browser_counts_by_code_and_navigates() {
    let (mut state, _dir) = opened_pgn().await;
    drive(
        &mut state,
        eco(EcoMessage::OpenBrowser { glossary: vec![] }),
    )
    .await;
    assert_eq!(state.eco_window(), Some(EcoWindow::Browser));
    let view = state.eco_browser_view().expect("view");
    assert!(!view.building);
    assert_eq!(view.classified, 3);
    let letters: Vec<&str> = view.bars.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(letters, ["A", "B", "C", "D", "E"]);
    assert_eq!(view.bars[1].stats.count, 2);
    assert_eq!(view.bars[1].height, 1000);
    assert_eq!(view.bars[1].parts, [500, 0, 500]);
    assert_eq!(view.entries.len(), 3, "the book thinned like Scid does");

    drive(&mut state, eco(EcoMessage::BrowseTo("B".to_owned()))).await;
    let view = state.eco_browser_view().expect("view");
    assert_eq!(view.stats.count, 2);
    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.entries[0].code, "B20");
    drive(&mut state, eco(EcoMessage::BrowseTo("B2".to_owned()))).await;
    drive(&mut state, eco(EcoMessage::BrowseTo("B20".to_owned()))).await;
    let view = state.eco_browser_view().expect("view");
    assert_eq!(view.bars.len(), 26);
    assert_eq!(view.bars[0].label, "a");
    drive(&mut state, eco(EcoMessage::BrowseUp)).await;
    assert_eq!(state.eco_browser_view().expect("view").prefix, "B2");
    // Not a node of the tree: ignored.
    drive(&mut state, eco(EcoMessage::BrowseTo("Z".to_owned()))).await;
    assert_eq!(state.eco_browser_view().expect("view").prefix, "B2");
}

#[tokio::test]
async fn the_search_of_the_browser_ignores_case() {
    let (mut state, _dir) = opened_pgn().await;
    drive(
        &mut state,
        eco(EcoMessage::OpenBrowser { glossary: vec![] }),
    )
    .await;
    drive(&mut state, eco(EcoMessage::SetFind("KING'S".to_owned()))).await;
    drive(&mut state, eco(EcoMessage::SubmitFind)).await;
    let view = state.eco_browser_view().expect("view");
    assert_eq!(view.finding, Some("KING'S"));
    assert_eq!(view.entries.len(), 2);
    drive(&mut state, eco(EcoMessage::SetFind("  ".to_owned()))).await;
    drive(&mut state, eco(EcoMessage::SubmitFind)).await;
    assert_eq!(state.eco_browser_view().expect("view").finding, None);
}

#[tokio::test]
async fn a_stale_table_is_ignored_and_the_scope_counts_again() {
    let (mut state, _dir) = opened_pgn().await;
    let effects = update(
        &mut state,
        eco(EcoMessage::OpenBrowser { glossary: vec![] }),
    );
    let [Effect::EcoStats { request, .. }] = effects.as_slice() else {
        panic!("one stats effect: {effects:?}");
    };
    let old = *request;
    // The scope changes before the first answer: the old one is dropped.
    let effects = update(&mut state, eco(EcoMessage::SetScope(BrowserScope::Filter)));
    let [
        Effect::EcoStats {
            request, filter, ..
        },
    ] = effects.as_slice()
    else {
        panic!("one stats effect: {effects:?}");
    };
    assert!(filter.is_some());
    assert_ne!(*request, old);
    let table = crate::eco_ui::SharedStats::new(mychess_db::EcoStatsTable::default());
    update(
        &mut state,
        eco(EcoMessage::StatsBuilt {
            request: old,
            table,
        }),
    );
    assert!(state.eco_browser_view().expect("view").building);
}

#[tokio::test]
async fn importing_a_line_replaces_the_game() {
    let (mut state, _dir) = opened_pgn().await;
    drive(
        &mut state,
        eco(EcoMessage::OpenBrowser { glossary: vec![] }),
    )
    .await;
    drive(&mut state, eco(EcoMessage::BrowseTo("C".to_owned()))).await;
    let view = state.eco_browser_view().expect("view");
    let index = view
        .entries
        .iter()
        .position(|e| e.code == "C40")
        .expect("the C40 entry");
    drive(&mut state, eco(EcoMessage::ImportLine(index))).await;
    assert_eq!(state.eco_window(), None);
    assert_eq!(state.session.game().mainline().count(), 3);
    assert_eq!(state.session.game().ply(state.session.cursor()), Ok(3));
}

#[tokio::test]
async fn identify_goes_to_the_deepest_book_position() {
    let (mut state, _dir) = opened_pgn().await;
    state
        .session
        .replace_game(game_of(&["e4", "e5", "Nf3", "Nc6", "Bb5"]));
    drive(&mut state, eco(EcoMessage::Identify)).await;
    assert_eq!(
        notice(&state),
        Some(EcoNotice::Opening {
            eco: "C40".to_owned(),
            name: "King's Knight".to_owned(),
            ply: 3
        })
    );
    assert_eq!(state.session.game().ply(state.session.cursor()), Ok(3));

    state.eco_book = None;
    drive(&mut state, eco(EcoMessage::Identify)).await;
    assert_eq!(notice(&state), Some(EcoNotice::NoBook));
}

#[tokio::test]
async fn novelty_is_searched_in_the_chosen_slot() {
    let (mut state, _dir) = opened_pgn().await;
    // Not in the base: 1.e4 e5 2.Nf3 Nc6 3.Bb5, where the base has 2...Nc6
    // and 2...Nf6.
    state
        .session
        .replace_game(game_of(&["e4", "e5", "Nf3", "Nc6", "Bb5"]));
    drive(&mut state, eco(EcoMessage::OpenNovelty)).await;
    let view = state.eco_novelty_view().expect("view");
    assert_eq!(view.reference, state.bases.current());
    assert!(view.references.contains(&crate::bases::SlotId::CLIPBASE));
    drive(&mut state, eco(EcoMessage::FindNovelty)).await;
    let view = state.eco_novelty_view().expect("view");
    assert_eq!(
        view.result,
        Some(&NoveltyOutcome::Found {
            ply: 5,
            san: "Bb5".to_owned(),
            move_number: 3,
            color: Color::White,
        })
    );
    // The cursor stops before the novel move.
    assert_eq!(state.session.game().ply(state.session.cursor()), Ok(4));
    assert!(matches!(notice(&state), Some(EcoNotice::Novelty { .. })));
    assert!(state.list_job().is_none());
}

#[tokio::test]
async fn a_game_already_in_the_base_has_no_novelty() {
    let (mut state, _dir) = opened_pgn().await;
    state
        .session
        .replace_game(game_of(&["e4", "e5", "Nf3", "Nc6"]));
    drive(&mut state, eco(EcoMessage::OpenNovelty)).await;
    drive(&mut state, eco(EcoMessage::FindNovelty)).await;
    assert_eq!(
        state.eco_novelty_view().expect("view").result,
        Some(&NoveltyOutcome::NoNovelty)
    );
    // An empty game never starts a job.
    state.session.replace_game(Game::new());
    let effects = update(&mut state, eco(EcoMessage::FindNovelty));
    assert!(effects.is_empty());
}

#[tokio::test]
async fn the_preview_counts_without_writing_and_a_pgn_cannot_be_written() {
    let (mut state, _dir) = opened_pgn().await;
    drive(&mut state, eco(EcoMessage::OpenClassify)).await;
    assert!(!state.eco_classify_view().expect("view").can_write);
    drive(
        &mut state,
        eco(EcoMessage::SetClassifyChoice(ClassifyChoice::All)),
    )
    .await;
    drive(&mut state, eco(EcoMessage::ClassifyPreview)).await;
    let view = state.eco_classify_view().expect("view");
    let result = view.result.expect("a result");
    assert!(result.preview);
    assert_eq!(result.report.classified, 5);
    assert_eq!(result.report.written, 0);
    assert!(result.report.changed > 0);
    // Writing is refused.
    let effects = update(&mut state, eco(EcoMessage::ClassifyRun));
    assert!(effects.is_empty());
    assert_eq!(notice(&state), Some(EcoNotice::ReadOnly));
}

#[tokio::test]
async fn a_date_that_is_not_a_date_runs_nothing() {
    let (mut state, _dir) = opened_pgn().await;
    drive(&mut state, eco(EcoMessage::OpenClassify)).await;
    drive(
        &mut state,
        eco(EcoMessage::SetClassifyChoice(ClassifyChoice::Since)),
    )
    .await;
    drive(
        &mut state,
        eco(EcoMessage::SetClassifySince("soon".to_owned())),
    )
    .await;
    let effects = update(&mut state, eco(EcoMessage::ClassifyPreview));
    assert!(effects.is_empty());
    assert!(state.eco_classify_view().expect("view").invalid_date);
    drive(
        &mut state,
        eco(EcoMessage::SetClassifySince("2020".to_owned())),
    )
    .await;
    assert!(!state.eco_classify_view().expect("view").invalid_date);
}

#[tokio::test]
async fn without_a_book_classifying_only_complains() {
    let (mut state, _dir) = opened_pgn().await;
    state.eco_book = None;
    drive(&mut state, eco(EcoMessage::OpenClassify)).await;
    assert!(update(&mut state, eco(EcoMessage::ClassifyPreview)).is_empty());
    assert_eq!(notice(&state), Some(EcoNotice::NoBook));
}

#[tokio::test]
async fn classifying_an_si4_file_writes_the_codes_and_reopens_it() {
    use mychess_db::si4::{Si4Database, Si4Writer};
    use mychess_db::{CancelToken, GameSource, ImportOptions, NoProgress, import_pgn};

    let dir = tempfile::tempdir().expect("dir");
    let base = dir.path().join("db");
    let mut writer = Si4Writer::create(&base, 0).expect("create");
    import_pgn(
        GAMES.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    let path = base.with_extension("si4");
    let mut state = AppState {
        open_pgn_in_slots: true,
        eco_book: Some(book()),
        ..AppState::default()
    };
    drive(&mut state, BasesMessage::Open(path.clone())).await;
    let slot = state.bases.current();
    let before = state.bases.slot(slot).expect("slot").source.clone();

    drive(&mut state, eco(EcoMessage::OpenClassify)).await;
    assert!(state.eco_classify_view().expect("view").can_write);
    assert!(state.eco_classify_view().expect("view").today_known);
    drive(
        &mut state,
        eco(EcoMessage::SetClassifyChoice(ClassifyChoice::NoCode)),
    )
    .await;
    drive(&mut state, eco(EcoMessage::ClassifyRun)).await;

    let view = state.eco_classify_view().expect("view");
    let result = view.result.expect("a result");
    assert!(!result.preview);
    assert_eq!(result.error, None);
    // The game without a code (4) and the one with `d4` (5) were classified:
    // the three with a tag keep it.
    assert_eq!(result.report.classified, 2);
    assert_eq!(result.report.written, 2);
    assert!(state.list_job().is_none());
    let now = &state.bases.slot(slot).expect("slot").source;
    assert!(*now != before, "the slot holds the reopened database");

    let db = Si4Database::open(&path).expect("reader");
    let codes: Vec<String> = (1..=GameSource::len(&db))
        .map(|n| {
            db.entry(n)
                .expect("entry")
                .eco()
                .map(|e| e.to_string())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(codes, ["B20", "B20", "C40", "C40", "A00a"]);
}

#[tokio::test]
async fn closing_a_dialog_cancels_its_job() {
    let (mut state, _dir) = opened_pgn().await;
    drive(&mut state, eco(EcoMessage::OpenClassify)).await;
    let effects = update(&mut state, eco(EcoMessage::ClassifyPreview));
    assert!(matches!(effects.as_slice(), [Effect::EcoClassify { .. }]));
    assert_eq!(
        state.list_job().map(|(kind, _)| kind),
        Some(ListJobKind::EcoClassify)
    );
    update(&mut state, eco(EcoMessage::CloseClassify));
    assert_eq!(state.eco_window(), None);
    assert!(
        state
            .list_job()
            .is_some_and(|(_, control)| control.is_cancelled())
    );
    // The answer of the cancelled job ends it quietly.
    for effect in effects {
        let answer = perform(effect).await;
        update(&mut state, answer);
    }
    assert!(state.list_job().is_none());
}

#[test]
fn the_answers_of_the_jobs_are_list_messages() {
    let message = Message::Bases(BasesMessage::List(ListMessage::EcoNovelty {
        request: crate::error::RequestId(1),
        result: Ok(NoveltyOutcome::NoNovelty),
    }));
    // A stale answer (no job of this request) changes nothing.
    let mut state = AppState::default();
    assert!(update(&mut state, message).is_empty());
    assert_eq!(notice(&state), None);
}
