//! M5-2a GUI contract: what takes the place of the table (spec
//! `docs/specs/m5-2-janela-da-arvore.md`, D968, D972, D973, behaviour 6):
//! loading, unavailable base, empty tree, engine issues, the unreadable-games
//! warning and the search in progress, in both languages.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use m5_2_gui_common::*;
use mychess_app::LanguagePref;
use mychess_core::Position;
use tempfile::tempdir;

/// 218 legal moves for White.
const MANY_MOVES_FEN: &str = "R6R/3Q4/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp1Q4/kBNN1KB1 w - - 0 1";

fn many_first_moves(n: usize) -> String {
    let pos = Position::from_fen(MANY_MOVES_FEN).unwrap();
    let mut text = String::new();
    for mv in pos.legal_moves().into_iter().take(n) {
        let san = pos.san(mv).unwrap();
        text.push_str(&game_pgn(&format!("1. {san}"), "1-0", Some(MANY_MOVES_FEN)));
    }
    text
}

#[test]
fn before_the_first_answer_the_panel_says_it_is_searching_the_games() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    key_without_running(&mut rig, command_shift(Key::Char('t')));
    assert_eq!(rig.view().body, TreeBody::Loading);
    assert!(pane_shows(&rig, "Searching the games..."));
    assert!(pane_shows(&rig, "Searching..."), "the discreet indicator");
    assert!(!pane_shows(&rig, "TOTAL"), "no table yet");
}

#[test]
fn loading_is_in_portuguese_with_a_portuguese_session() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_language(LanguagePref::PtBr);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    key_without_running(&mut rig, command_shift(Key::Char('t')));
    assert!(pane_shows(&rig, "Buscando nas partidas..."));
    assert!(pane_shows(&rig, "Buscando..."));
}

#[test]
fn with_no_base_the_panel_says_the_base_is_not_available_in_both_languages() {
    for (language, text) in [
        (LanguagePref::EnUs, "Base not available"),
        (LanguagePref::PtBr, "Base não disponível"),
    ] {
        let mut rig = TreeRig::with_language(language);
        rig.open_panel();
        assert_eq!(rig.view().body, TreeBody::Unavailable);
        assert!(pane_shows(&rig, text), "{text}");
        assert!(!pane_shows(&rig, "TOTAL"));
    }
}

#[test]
fn a_position_no_game_reaches_shows_the_empty_text_and_no_table() {
    let dir = tempdir().unwrap();
    for (language, text, header) in [
        (LanguagePref::EnUs, "No game reaches this position", "Move"),
        (
            LanguagePref::PtBr,
            "Nenhuma partida chega a esta posição",
            "Lance",
        ),
    ] {
        let mut rig = TreeRig::with_language(language);
        rig.open_pgn(dir.path(), "std.pgn", STD);
        rig.open_panel();
        rig.play("a2a3");
        assert_eq!(rig.view().body, TreeBody::Empty);
        assert!(pane_shows(&rig, text), "{text}");
        assert!(!pane_shows(&rig, header), "no header without the table");
        assert!(!pane_shows(&rig, "TOTAL"));
        // The toolbar stays so the user can change the options.
        assert!(pane_shows(
            &rig,
            if header == "Move" {
                "Short display"
            } else {
                "Exibição curta"
            }
        ));
    }
}

#[test]
fn more_than_sixty_moves_shows_the_message_of_the_issue() {
    let dir = tempdir().unwrap();
    for (language, text) in [
        (
            LanguagePref::EnUs,
            "More than 60 moves have been played from this position",
        ),
        (
            LanguagePref::PtBr,
            "Mais de 60 lances foram jogados a partir desta posição",
        ),
    ] {
        let mut rig = TreeRig::with_language(language);
        rig.open_si4(
            dir.path(),
            &format!("many{}", language as u8),
            &many_first_moves(61),
        );
        let slot = rig.app().bases.current();
        rig.app_msg(BasesMessage::LoadGame { slot, number: 1 });
        assert_eq!(rig.app().session.position().fen(), MANY_MOVES_FEN);
        rig.open_panel();
        assert_eq!(rig.view().body, TreeBody::Issue(TreeIssue::TooManyMoves));
        assert!(pane_shows(&rig, text), "{text}");
        assert!(!pane_shows(&rig, "TOTAL"));
        rig.app_msg(BasesMessage::Close(slot));
        rig.app_msg(TreeMessage::Close);
    }
}

const HEADER_LEN: usize = 182;
const ENTRY_LEN: usize = 47;

fn break_offset(si4: &std::path::Path, n: usize) {
    let mut bytes = std::fs::read(si4).unwrap();
    let at = HEADER_LEN + ENTRY_LEN * (n - 1);
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FF00_u32.to_be_bytes());
    std::fs::write(si4, bytes).unwrap();
}

#[test]
fn unreadable_games_add_a_discreet_warning_with_the_count_and_keep_the_table() {
    let dir = tempdir().unwrap();
    let mut text = String::new();
    for _ in 0..4 {
        text.push_str(&game_pgn("1. e4 e5", "1-0", None));
    }
    for (language, warning) in [
        (LanguagePref::EnUs, "2 games could not be read"),
        (LanguagePref::PtBr, "2 partidas não puderam ser lidas"),
    ] {
        let name = format!("bad-{}", language as u8);
        let si4 = make_si4(dir.path(), &name, &text);
        break_offset(&si4, 2);
        break_offset(&si4, 3);
        let mut rig = TreeRig::with_language(language);
        rig.app_msg(BasesMessage::Open(si4));
        rig.open_panel();
        let view = rig.view();
        assert_eq!(view.body, TreeBody::Rows, "{view:?}");
        assert_eq!(view.unreadable, 2);
        assert!(pane_shows(&rig, warning), "{warning}");
        assert!(
            pane_shows(&rig, "e4"),
            "the readable games still make the table"
        );
        assert!(pane_shows(&rig, "TOTAL"));
    }
}

#[test]
fn a_healthy_base_shows_no_unreadable_warning() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    assert!(!pane_shows(&rig, "could not be read"));
}

#[test]
fn while_searching_the_previous_table_stays_with_the_indicator_and_ignores_clicks() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    assert!(!pane_shows(&rig, "Searching..."), "idle: no indicator");
    // A move arrives; the search for the new position is still in flight.
    let mv = rig.app().session.position().parse_uci("e2e4").unwrap();
    send_without_running(
        &mut rig,
        GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        },
    );
    assert!(rig.view().searching);
    assert_eq!(rig.view().body, TreeBody::Rows, "the old rows stay");
    let mut sim = pane_sim(&rig);
    for text in ["TOTAL", "Nf3", "d4", "Searching..."] {
        assert!(sim.find(text).is_ok(), "{text}");
    }
    // Rows of the old table are not clickable now.
    sim.click("Nf3").expect("text");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        !sent.iter().any(|m| dbg(m).contains("RowClicked")),
        "{sent:?}"
    );
}

#[test]
fn the_indicator_goes_away_when_the_answer_arrives() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    rig.play("e2e4");
    assert!(!rig.view().searching);
    assert!(!pane_shows(&rig, "Searching..."));
    assert!(pane_shows(&rig, "c5") && pane_shows(&rig, "e5"));
    assert!(
        !pane_shows(&rig, "Nf3"),
        "the new table replaced the old one"
    );
}

#[test]
fn the_texts_of_every_issue_exist_in_both_languages_and_the_read_one_names_the_reason() {
    use fluent_bundle::FluentArgs;
    for (i18n, language) in [(en(), "en-US"), (pt(), "pt-BR")] {
        for key in [
            "tree-issue-too-many-moves",
            "tree-issue-unsupported",
            "tree-issue-filter-size",
            "tree-issue-internal",
            "tree-unavailable",
            "tree-empty",
            "tree-loading",
            "tree-searching",
        ] {
            assert!(i18n.has(key), "{language}: {key}");
            let text = i18n.tr(key);
            assert!(!text.is_empty() && text != key, "{language}: {key}");
        }
        let mut args = FluentArgs::new();
        args.set("reason", "disk gone".to_owned());
        let text = i18n.tr_args("tree-issue-read", &args);
        assert!(text.contains("disk gone"), "{language}: {text}");
        let mut args = FluentArgs::new();
        args.set("count", "7".to_owned());
        assert!(i18n.tr_args("tree-unreadable", &args).contains('7'));
    }
    // Different texts per language, not the same string twice.
    assert_ne!(
        en().tr("tree-issue-unsupported"),
        pt().tr("tree-issue-unsupported")
    );
    assert_ne!(
        en().tr("tree-issue-filter-size"),
        pt().tr("tree-issue-filter-size")
    );
    assert_ne!(
        en().tr("tree-issue-internal"),
        pt().tr("tree-issue-internal")
    );
}
