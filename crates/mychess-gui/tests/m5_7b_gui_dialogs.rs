//! M5-7b GUI contract: the "Classify games" and "Find novelty" boxes, the
//! remarks of the status bar and the keys of the boxes (spec
//! `docs/specs/m5-7-eco-e-novidade.md`, D1701, D1710-D1720, D1724).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_7b_gui_common;

use m5_7b_gui_common::*;
use mychess_app::NamedKey;
use tempfile::tempdir;

fn app(rig: &mut TreeRig, message: EcoMessage) {
    rig.app_msg(AppMessage::Eco(message));
}

fn new_game(rig: &mut TreeRig) {
    rig.app_msg(mychess_app::Command::NewGame);
}

fn classify_games() -> String {
    let mut pgn = String::new();
    pgn.push_str(&game_pgn("1. e4 c5", "1-0", "2020.01.01", None));
    pgn.push_str(&game_pgn("1. e4 e5", "0-1", "2020.07.01", Some("C20")));
    pgn.push_str(&game_pgn("1. d4 d5", "1/2-1/2", "2021.03.01", None));
    pgn.push_str(&game_pgn(
        "1. e4 c5 2. Nf3",
        "1-0",
        "2021.05.20",
        Some("E99"),
    ));
    pgn
}

fn si4_rig(language: LanguagePref, dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_language(language);
    rig.open_si4(dir, "g", &classify_games());
    rig.rig.shell.app.eco_book = Some(book(BOOK));
    rig
}

fn codes(path: &std::path::Path) -> Vec<String> {
    use mychess_db::GameSource;
    let db = mychess_db::si4::Si4Database::open(path).unwrap();
    (1..=GameSource::len(&db))
        .map(|n| {
            db.entry(n)
                .unwrap()
                .eco()
                .map(|e| e.to_string())
                .unwrap_or_default()
        })
        .collect()
}

// ---- classify ----

#[test]
fn the_classify_box_offers_every_choice_and_each_radio_sends_its_choice() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::EnUs, dir.path());
    app(&mut rig, EcoMessage::OpenClassify);
    assert_eq!(window(&rig), Some(EcoWindow::Classify));
    for text in [
        "ECO-classify games",
        "Games to classify",
        "ECO codes to use",
        "Basic codes only (\"B12\")",
        "Extensions (\"B12j\")",
        "Preview",
        "Classify",
        "Close",
    ] {
        assert!(box_shows(&rig, EcoBox::Classify, text), "{text}");
    }
    // The six radios, in the order of the Scid dialog's choices.
    for (index, choice) in CLASSIFY_RADIOS.into_iter().enumerate() {
        let point = classify_radio(&rig, index);
        let messages = box_click_at(&rig, EcoBox::Classify, point);
        assert!(
            sent(&messages, &eco(EcoMessage::SetClassifyChoice(choice))),
            "radio {index}: {messages:?}"
        );
    }
    let messages = box_click(&rig, EcoBox::Classify, "Basic codes only (\"B12\")");
    assert!(sent(
        &messages,
        &eco(EcoMessage::SetClassifyExtended(false))
    ));
    let messages = box_click(&rig, EcoBox::Classify, "Extensions (\"B12j\")");
    assert!(sent(&messages, &eco(EcoMessage::SetClassifyExtended(true))));
}

#[test]
fn preview_does_not_write_and_classify_writes_and_the_result_is_shown() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::EnUs, dir.path());
    let path = dir.path().join("g.si4");
    assert_eq!(codes(&path), ["", "C20", "", "E99"]);
    app(&mut rig, EcoMessage::OpenClassify);
    pick_classify(&mut rig, ClassifyChoice::All);
    assert_eq!(
        rig.app().eco_classify_view().unwrap().choice,
        ClassifyChoice::All
    );
    box_press(&mut rig, EcoBox::Classify, "Preview");
    assert!(box_shows(
        &rig,
        EcoBox::Classify,
        "4 games examined: 3 would change."
    ));
    assert_eq!(
        codes(&path),
        ["", "C20", "", "E99"],
        "a preview writes nothing"
    );
    assert!(rig.shows("ECO preview: 4 games examined, 3 would change."));
    box_press(&mut rig, EcoBox::Classify, "Classify");
    assert!(box_shows(
        &rig,
        EcoBox::Classify,
        "4 games classified: 3 changed, 3 written."
    ));
    assert_eq!(codes(&path), ["B20", "C20", "A00a", "B20"]);
    assert!(rig.shows("ECO classification: 4 games examined, 3 changed, 3 written."));
}

#[test]
fn a_pgn_base_only_gets_the_preview_button_and_says_why() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    app(&mut rig, EcoMessage::OpenClassify);
    assert!(box_shows(
        &rig,
        EcoBox::Classify,
        "This database cannot be written: only the preview is available."
    ));
    assert!(
        box_click(&rig, EcoBox::Classify, "Classify").is_empty(),
        "the button is off"
    );
    let messages = box_click(&rig, EcoBox::Classify, "Preview");
    assert!(sent(&messages, &eco(EcoMessage::ClassifyPreview)));
    // A run asked for anyway is refused with a remark.
    app(&mut rig, EcoMessage::ClassifyRun);
    assert!(rig.shows("This database cannot be written: only the preview is possible."));
}

#[test]
fn without_a_book_the_classify_buttons_are_off_and_the_box_says_so() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::EnUs, dir.path());
    rig.rig.shell.app.eco_book = None;
    app(&mut rig, EcoMessage::OpenClassify);
    assert!(box_shows(
        &rig,
        EcoBox::Classify,
        "No ECO file is loaded. Load one in the ECO browser first."
    ));
    assert!(box_click(&rig, EcoBox::Classify, "Preview").is_empty());
    assert!(box_click(&rig, EcoBox::Classify, "Classify").is_empty());
}

#[test]
fn an_invalid_date_shows_a_warning_in_the_box_and_nothing_runs() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::EnUs, dir.path());
    app(&mut rig, EcoMessage::OpenClassify);
    app(
        &mut rig,
        EcoMessage::SetClassifyChoice(ClassifyChoice::Since),
    );
    app(&mut rig, EcoMessage::SetClassifySince("abc".to_owned()));
    box_press(&mut rig, EcoBox::Classify, "Preview");
    assert!(box_shows(
        &rig,
        EcoBox::Classify,
        "Type a date that starts with the year, as 2020.01.31."
    ));
    assert!(rig.app().list_job().is_none());
    assert!(rig.app().eco_classify_view().unwrap().result.is_none());
}

#[test]
fn while_it_runs_the_box_shows_progress_and_stop_and_hides_the_other_buttons() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::EnUs, dir.path());
    app(&mut rig, EcoMessage::OpenClassify);
    // Start the job but do not run its task: the job stays in flight.
    let _held = rig.rig.shell.update(eco(EcoMessage::ClassifyRun));
    assert!(rig.app().list_job().is_some());
    assert!(box_shows(&rig, EcoBox::Classify, "Stop"));
    assert!(box_shows(&rig, EcoBox::Classify, "Classifying: 0%"));
    assert!(!box_shows(&rig, EcoBox::Classify, "Preview"));
    assert!(!box_shows(&rig, EcoBox::Classify, "Classify"));
    let messages = box_click(&rig, EcoBox::Classify, "Stop");
    assert!(
        messages.iter().any(|m| dbg(m).contains("CancelJob")),
        "{messages:?}"
    );
}

#[test]
fn enter_does_not_classify_and_escape_closes_the_box() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::EnUs, dir.path());
    let path = dir.path().join("g.si4");
    app(&mut rig, EcoMessage::OpenClassify);
    rig.key(named(NamedKey::Enter));
    assert!(rig.app().list_job().is_none());
    assert!(rig.app().eco_classify_view().unwrap().result.is_none());
    assert_eq!(codes(&path), ["", "C20", "", "E99"]);
    assert_eq!(window(&rig), Some(EcoWindow::Classify));
    rig.key(named(NamedKey::Escape));
    assert_eq!(window(&rig), None);
}

#[test]
fn the_classify_box_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = si4_rig(LanguagePref::PtBr, dir.path());
    app(&mut rig, EcoMessage::OpenClassify);
    for text in [
        "Classificar partidas por ECO",
        "Partidas a classificar",
        "Códigos ECO a usar",
        "Prévia",
        "Classificar",
        "Fechar",
    ] {
        assert!(box_shows(&rig, EcoBox::Classify, text), "{text}");
    }
    box_press(&mut rig, EcoBox::Classify, "Prévia");
    assert!(box_shows(
        &rig,
        EcoBox::Classify,
        "2 partidas examinadas: 2 mudariam."
    ));
}

// ---- novelty ----

const RUY_NF6: [&str; 6] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "g8f6"];

fn novelty_rig(language: LanguagePref, dir: &std::path::Path) -> TreeRig {
    let mut rig = TreeRig::with_language(language);
    rig.open_pgn(
        dir,
        "ref.pgn",
        &game_pgn("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6", "1-0", "2020.01.01", None),
    );
    new_game(&mut rig);
    rig
}

#[test]
fn the_novelty_box_lists_the_bases_with_the_clipbase_and_finds_3_nf6_with_enter() {
    let dir = tempdir().unwrap();
    let mut rig = novelty_rig(LanguagePref::EnUs, dir.path());
    for uci in RUY_NF6 {
        rig.play(uci);
    }
    app(&mut rig, EcoMessage::OpenNovelty);
    assert_eq!(window(&rig), Some(EcoWindow::Novelty));
    for text in ["Find novelty", "Database", "ref.pgn", "Clipbase", "Close"] {
        assert!(box_shows(&rig, EcoBox::Novelty, text), "{text}");
    }
    // Enter (focus outside a field) searches.
    rig.key(named(NamedKey::Enter));
    assert!(box_shows(&rig, EcoBox::Novelty, "Novelty: 3...Nf6"));
    assert!(rig.shows("Novelty: 3...Nf6"), "the status bar says it too");
    // The board is at the position before the move.
    let ply = {
        let s = &rig.app().session;
        s.game().ply(s.cursor()).unwrap()
    };
    assert_eq!(ply, 5);
}

#[test]
fn choosing_a_base_or_older_only_sends_the_choice() {
    let dir = tempdir().unwrap();
    let mut rig = novelty_rig(LanguagePref::EnUs, dir.path());
    rig.play("e2e4");
    app(&mut rig, EcoMessage::OpenNovelty);
    let slot = rig.app().bases.current();
    let messages = box_click(&rig, EcoBox::Novelty, "ref.pgn");
    assert!(
        sent(&messages, &eco(EcoMessage::SetNoveltyReference(slot))),
        "{messages:?}"
    );
    let messages = box_click(&rig, EcoBox::Novelty, "Clipbase");
    assert!(
        messages
            .iter()
            .any(|m| dbg(m).contains("SetNoveltyReference")),
        "{messages:?}"
    );
    let point = older_checkbox(&rig, "ref.pgn");
    let messages = box_click_at(&rig, EcoBox::Novelty, point);
    assert!(sent(&messages, &eco(EcoMessage::SetNoveltyOlderOnly(true))));
}

#[test]
fn a_game_without_moves_says_so_and_enter_reports_no_novelty() {
    let dir = tempdir().unwrap();
    let mut rig = novelty_rig(LanguagePref::EnUs, dir.path());
    app(&mut rig, EcoMessage::OpenNovelty);
    assert!(box_shows(&rig, EcoBox::Novelty, "The game has no moves."));
    rig.key(named(NamedKey::Enter));
    assert!(box_shows(
        &rig,
        EcoBox::Novelty,
        "No novelty was found for this game."
    ));
}

#[test]
fn escape_closes_the_novelty_box_and_stops_a_search() {
    let dir = tempdir().unwrap();
    let mut rig = novelty_rig(LanguagePref::EnUs, dir.path());
    rig.play("e2e4");
    app(&mut rig, EcoMessage::OpenNovelty);
    let _held = rig.rig.shell.update(eco(EcoMessage::FindNovelty));
    assert!(rig.app().list_job().is_some());
    assert!(box_shows(&rig, EcoBox::Novelty, "Stop"));
    assert!(box_shows(&rig, EcoBox::Novelty, "Searching: 0%"));
    rig.key(named(NamedKey::Escape));
    assert_eq!(window(&rig), None);
    assert!(rig.app().list_job().unwrap().1.is_cancelled());
}

#[test]
fn the_novelty_box_in_portuguese() {
    let dir = tempdir().unwrap();
    let mut rig = novelty_rig(LanguagePref::PtBr, dir.path());
    for uci in RUY_NF6 {
        rig.play(uci);
    }
    app(&mut rig, EcoMessage::OpenNovelty);
    for text in ["Encontrar novidade", "Base", "Fechar"] {
        assert!(box_shows(&rig, EcoBox::Novelty, text), "{text}");
    }
    rig.key(named(NamedKey::Enter));
    assert!(box_shows(&rig, EcoBox::Novelty, "Novidade: 3...Nf6"));
}

// ---- identify opening and the status bar ----

#[test]
fn identify_opening_tells_the_code_and_the_name_in_the_status_bar() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    new_game(&mut rig);
    rig.play("e2e4");
    rig.play("c7c5");
    rig.play("g1f3");
    app(&mut rig, EcoMessage::Identify);
    assert!(rig.shows("Opening B20: Sicilian Defence (after half-move 2)"));
    let ply = {
        let s = &rig.app().session;
        s.game().ply(s.cursor()).unwrap()
    };
    assert_eq!(ply, 2);
}

#[test]
fn identify_opening_in_portuguese_translates_the_name() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::PtBr, dir.path());
    new_game(&mut rig);
    rig.play("e2e4");
    rig.play("c7c5");
    app(&mut rig, EcoMessage::Identify);
    assert!(rig.shows("Abertura B20: Defesa Siciliana (depois do meio-lance 2)"));
}

#[test]
fn identify_without_a_book_tells_so() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    rig.rig.shell.app.eco_book = None;
    app(&mut rig, EcoMessage::Identify);
    assert!(rig.shows("No ECO file is loaded."));
    rig.rig.shell.app.notice = None;
    rig.rig.shell.app.eco_book = Some(book(BOOK));
    new_game(&mut rig);
    rig.play("a2a3");
    app(&mut rig, EcoMessage::Identify);
    // The start entry matches every game at ply 0.
    assert!(rig.shows("Opening A00a: Start (after half-move 0)"));
}
