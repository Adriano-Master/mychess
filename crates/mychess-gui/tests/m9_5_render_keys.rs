//! M9-5c (GUI): every chapter renders, no table or image reaches the screen,
//! the themes, the fallback of the language, the keys behind the window and
//! the translations (spec M9-5, criteria 2, 11, 12 and 14).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m9_5_common;

use m9_5_common::*;
use mychess_app::manual::{ManualIndex, SharedIndex};
use mychess_app::{BasesMessage, GameMessage, Nav};
use mychess_gui::windows::manual::Rendered;

#[test]
fn every_chapter_renders_with_a_title_and_an_id_for_every_heading() {
    let mut rig = Rig::pt();
    rig.open();
    #[allow(clippy::needless_range_loop)] // the chapter number is also the index of the model
    for chapter in 0..15 {
        rig.goto(chapter);
        assert_eq!(rig.chapter(), chapter);
        let index = rig.shell.app.manual().index().unwrap().clone();
        {
            let rendered = rig
                .shell
                .ui
                .dialogs
                .manual
                .rendered()
                .unwrap_or_else(|| panic!("chapter {chapter} was not parsed"));
            assert_eq!(rendered.chapter, chapter);
            assert_eq!(
                format!("{:?}", rendered.first_heading_level()),
                "Some(H1)",
                "{} starts with a level 1 heading",
                CHAPTER_FILES[chapter]
            );
            assert_eq!(
                rendered.heading_count(),
                index.headings(chapter).len(),
                "{}: every heading of the text is a heading item",
                CHAPTER_FILES[chapter]
            );
            assert!(
                !rendered.has_table_or_image(),
                "{} produced a table or an image",
                CHAPTER_FILES[chapter]
            );
            assert!(rendered.item_count() > 3, "{}", CHAPTER_FILES[chapter]);
        }
        let mut sim = rig.sim(1300.0, 900.0);
        assert!(
            sim.find(id(MANUAL_TEXT_ID)).is_ok(),
            "{}",
            CHAPTER_FILES[chapter]
        );
        for heading in index.headings(chapter) {
            assert!(
                sim.find(id(&heading_id(&heading.slug))).is_ok(),
                "{}: no id for '{}'",
                CHAPTER_FILES[chapter],
                heading.title
            );
        }
    }
}

#[test]
fn the_detector_sees_tables_and_images_when_a_text_has_them() {
    // Non-vacuity of the check above.
    for text in [
        "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
        "# T\n\n![alt](picture.png)\n",
    ] {
        let index = ManualIndex::build(&[text], false);
        let rendered = Rendered::build(text, &index, 0, Language::PtBr);
        assert!(rendered.has_table_or_image(), "{text}");
    }
    let plain = "# T\n\nText with `code` and [a link](x.md).\n";
    let index = ManualIndex::build(&[plain], false);
    assert!(!Rendered::build(plain, &index, 0, Language::PtBr).has_table_or_image());
}

#[test]
fn the_window_draws_in_the_light_and_in_the_dark_theme() {
    let mut rig = Rig::pt();
    rig.open();
    rig.goto(1);
    let [dark, light] = themes();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dark.hash");
    {
        let mut sim = rig.sim(1100.0, 800.0);
        let snapshot = sim.snapshot(&dark).expect("the dark window draws");
        // The first call records the hash; the second compares with it.
        assert!(snapshot.matches_hash(&path).unwrap());
        assert!(snapshot.matches_hash(&path).unwrap());
    }
    let mut sim = rig.sim(1100.0, 800.0);
    let snapshot = sim.snapshot(&light).expect("the light window draws");
    assert!(
        !snapshot.matches_hash(&path).unwrap(),
        "the light window differs from the dark one"
    );
}

#[test]
fn portuguese_shows_no_notice_and_english_falls_back_with_one() {
    let mut rig = Rig::pt();
    rig.open();
    assert!(!rig.shell.app.manual().is_fallback());
    assert!(!has_id(&rig, "manual-no-translation"));

    let mut rig = Rig::en();
    rig.open();
    assert!(rig.shell.app.manual().is_fallback());
    assert!(has_id(&rig, "manual-no-translation"));
    let mut sim = rig.sim(1300.0, 900.0);
    assert!(
        sim.find("The manual is available only in Portuguese.")
            .is_ok()
    );
    // The text itself is the Portuguese one.
    assert_eq!(
        rig.shell.app.manual().index().unwrap().title(4),
        Some("Buscas")
    );
}

#[test]
fn changing_the_language_with_the_manual_open_keeps_the_chapter_and_the_heading() {
    let mut rig = Rig::en();
    rig.open();
    rig.link("03-bancos-de-dados.md#abrir-uma-base");
    assert!(rig.shell.app.manual().is_fallback());
    rig.shell.app.prefs.general.language = LanguagePref::PtBr;
    rig.msg(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    assert_eq!(
        rig.shell.i18n.language(),
        Language::PtBr,
        "the language changed"
    );
    assert!(
        !rig.shell.app.manual().is_fallback(),
        "the index follows the language"
    );
    assert_eq!(rig.chapter(), 3);
    assert_eq!(
        rig.shell.app.manual().place().anchor.as_deref(),
        Some("abrir-uma-base")
    );
    assert!(!has_id(&rig, "manual-no-translation"));
}

fn rig_with_a_game() -> Rig {
    let mut rig = Rig::en();
    rig.shell.app.open_pgn_in_slots = true;
    let path = rig.dir.path().join("game.pgn");
    std::fs::write(
        &path,
        "[Event \"?\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *\n\n",
    )
    .unwrap();
    rig.msg(Message::App(AppMessage::Bases(BasesMessage::Open(path))));
    let slot = rig.shell.app.bases.current();
    rig.msg(Message::App(AppMessage::Bases(BasesMessage::LoadGame {
        slot,
        number: 1,
    })));
    rig.msg(Message::App(AppMessage::Game(GameMessage::Navigate(
        Nav::GoToMainlinePly(3),
    ))));
    rig
}

#[test]
fn keys_do_not_reach_the_game_behind_the_open_manual() {
    let mut rig = rig_with_a_game();
    let cursor = rig.shell.app.session.cursor();
    let nodes = rig.shell.app.session.game().mainline().count();
    rig.open();
    assert!(
        rig.shell.modal_layer_open(),
        "the manual is a modal layer for the keys"
    );
    let command = Modifiers {
        command: true,
        ..Modifiers::NONE
    };
    for (key, modifiers) in [
        (Key::Named(NamedKey::Left), Modifiers::NONE),
        (Key::Named(NamedKey::Right), Modifiers::NONE),
        (Key::Named(NamedKey::Up), Modifiers::NONE),
        (Key::Named(NamedKey::Down), Modifiers::NONE),
        (Key::Named(NamedKey::Home), Modifiers::NONE),
        (Key::Named(NamedKey::End), Modifiers::NONE),
        (Key::Named(NamedKey::Delete), Modifiers::NONE),
        (Key::Named(NamedKey::Enter), Modifiers::NONE),
        (Key::Char('e'), Modifiers::NONE),
        (Key::Char('4'), Modifiers::NONE),
        (Key::Char('d'), Modifiers::NONE),
        (Key::Char('!'), Modifiers::NONE),
        (Key::Char('z'), command),
        (Key::Char('y'), command),
        (Key::Char('f'), Modifiers::NONE),
    ] {
        rig.key(key, modifiers);
        assert!(
            rig.shell.app.manual().is_open(),
            "{key:?} must not close the manual"
        );
        assert_eq!(
            rig.shell.app.session.cursor(),
            cursor,
            "{key:?} moved the game"
        );
        assert_eq!(
            rig.shell.app.session.game().mainline().count(),
            nodes,
            "{key:?} changed the game"
        );
        assert!(
            rig.shell.app.session.move_entry().text().is_empty(),
            "{key:?} typed a move"
        );
    }
    // With the manual closed the same key moves the game: the check above
    // is not vacuous.
    rig.named(NamedKey::Escape);
    assert!(!rig.shell.app.manual().is_open());
    rig.named(NamedKey::Left);
    assert_ne!(
        rig.shell.app.session.cursor(),
        cursor,
        "Left moves the game when no layer is open"
    );
}

#[test]
fn closing_the_manual_zeroes_the_focus_of_the_search_field() {
    let mut rig = Rig::pt();
    rig.open();
    rig.ui(ManualUiMessage::SearchFocus(true));
    assert!(rig.shell.ui.dialogs.manual.search_focused());
    // A key typed with the field focused is not used as a shortcut.
    rig.ui(ManualUiMessage::Plain(ManualMessage::Close));
    assert!(!rig.shell.ui.dialogs.manual.search_focused());
    assert!(!rig.shell.modal_layer_open());
    rig.open();
    assert!(
        !rig.shell.ui.dialogs.manual.search_focused(),
        "a new window starts unfocused"
    );
}

#[test]
fn the_arrows_do_not_close_or_navigate_the_manual_and_scroll_keys_are_taken() {
    let mut rig = Rig::pt();
    rig.open();
    rig.link("04-buscas.md");
    for key in [
        NamedKey::Up,
        NamedKey::Down,
        NamedKey::PageUp,
        NamedKey::PageDown,
        NamedKey::Home,
        NamedKey::End,
    ] {
        rig.named(key);
        assert!(rig.shell.app.manual().is_open());
        assert_eq!(rig.chapter(), 4, "{key:?} only scrolls");
    }
}

#[test]
fn the_manual_texts_exist_in_both_languages_and_use_the_right_term() {
    let keys = [
        "menu-help-manual",
        "manual-title",
        "manual-contents",
        "manual-back",
        "manual-forward",
        "manual-home",
        "manual-close",
        "manual-back-tip",
        "manual-forward-tip",
        "manual-home-tip",
        "manual-close-tip",
        "manual-search-placeholder",
        "manual-search-empty",
        "manual-no-translation",
        "manual-link-not-found",
        "manual-link-ignored",
        "manual-url-refused",
        "manual-external-title",
        "manual-external-body",
        "manual-external-open",
        "manual-external-cancel",
        "manual-text-smaller",
        "manual-text-larger",
        "manual-text-smaller-tip",
        "manual-text-larger-tip",
    ];
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in keys {
            assert!(i18n.has(key), "{language:?} lacks {key}");
            let text = i18n.tr(key);
            assert!(
                !text.trim().is_empty() && text != key,
                "{language:?} {key}: {text}"
            );
            assert!(!text.to_lowercase().contains("variaç"), "{key}");
        }
    }
    let pt = I18n::new(Language::PtBr);
    assert_eq!(
        pt.tr("manual-no-translation"),
        "Manual disponível apenas em português"
    );
    assert_eq!(pt.tr("manual-back"), "Voltar");
    assert_eq!(I18n::new(Language::EnUs).tr("manual-back"), "Back");
}

#[test]
fn the_shared_index_type_compares_by_identity() {
    let a = SharedIndex::new(ManualIndex::build(&["# A\n"], false));
    let b = SharedIndex::new(ManualIndex::build(&["# A\n"], false));
    assert_eq!(a, a.clone());
    assert_ne!(a, b);
}
