//! M2-3 localization behavior (rule 12), shortcut labels and key translation
//! (rules 14 and 59), and the layout options of the board (rule 18).

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use fluent_bundle::FluentArgs;
use iced::keyboard::{self, Key as IcedKey, key::Named};
use mychess_app::{Action, Binding, Key, KeyInput, Modifiers, NamedKey, Scope};
use mychess_core::Square;
use mychess_gui::i18n::{I18n, Language, LanguagePref, language_from_locale};
use mychess_gui::keymap::{Platform, binding_label, key_input};
use mychess_gui::widgets::board::BoardOptions;
use mychess_gui::widgets::board::geometry::CoordsMode;
use mychess_gui::widgets::board::moves::legal_destinations;
use mychess_gui::{Message, Shell, UiMessage};
use rstest::rstest;

// ---------------------------------------------------------------------------
// i18n
// ---------------------------------------------------------------------------

#[rstest]
#[case("board-promote-queen", "Queen", "Dama")]
#[case("board-promote-rook", "Rook", "Torre")]
#[case("board-promote-bishop", "Bishop", "Bispo")]
#[case("board-promote-knight", "Knight", "Cavalo")]
fn m2_3_promotion_pieces_use_the_chess_terms_of_each_language(
    #[case] key: &str,
    #[case] en: &str,
    #[case] pt: &str,
) {
    assert_eq!(I18n::new(Language::EnUs).tr(key), en);
    assert_eq!(I18n::new(Language::PtBr).tr(key), pt);
}

#[test]
fn m2_3_every_spec_key_has_a_non_empty_text_that_is_not_the_key() {
    let keys = [
        "app-name",
        "window-untitled",
        "menu-file",
        "menu-edit",
        "menu-game",
        "menu-view",
        "menu-help",
        "menu-view-theme-system",
        "menu-view-theme-light",
        "menu-view-theme-dark",
        "menu-view-coords-none",
        "menu-view-coords-outside",
        "menu-view-coords-both",
        "menu-view-coords-inside",
        "menu-view-material-off",
        "menu-view-material-difference",
        "menu-view-material-captured",
        "menu-view-language-system",
        "menu-view-language-en-us",
        "menu-view-language-pt-br",
        "pane-board",
        "pane-info",
        "pane-notation",
        "notation-placeholder",
        "board-flip",
        "status-ready",
        "status-check",
        "status-checkmate",
        "status-stalemate",
        "status-modified",
        "info-unknown",
        "dialog-ok",
        "dialog-cancel",
        "about-title",
        "about-license",
    ];
    for lang in Language::ALL {
        let i18n = I18n::new(lang);
        for key in keys {
            let text = i18n.tr(key);
            assert!(!text.trim().is_empty(), "{key} empty in {lang:?}");
            assert_ne!(text, key, "{key} missing in {lang:?}");
            assert!(i18n.has(key));
        }
    }
}

#[test]
fn m2_3_portuguese_translates_the_visible_labels() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for key in [
        "menu-file",
        "menu-edit",
        "menu-game",
        "menu-view",
        "menu-help",
        "pane-board",
        "pane-notation",
        "status-ready",
        "status-check",
        "status-checkmate",
        "status-stalemate",
        "board-flip",
        "window-untitled",
    ] {
        assert_ne!(en.tr(key), pt.tr(key), "{key} is not translated");
    }
}

#[test]
fn m2_3_missing_keys_return_the_key_in_every_language_without_panicking() {
    for lang in Language::ALL {
        let i18n = I18n::new(lang);
        for key in [
            "",
            "no-such-key",
            "menu_file",
            "MENU-FILE",
            "ünï-ćødé",
            "a b c",
        ] {
            assert_eq!(i18n.tr(key), key);
            assert_eq!(i18n.tr(key), key, "twice (warns once, answers always)");
        }
        let mut args = FluentArgs::new();
        args.set("x", 1);
        assert_eq!(i18n.tr_args("no-such-key", &args), "no-such-key");
    }
}

#[test]
fn m2_3_window_title_has_a_modified_selector_in_both_languages() {
    for lang in Language::ALL {
        let i18n = I18n::new(lang);
        let title = |modified: &str| {
            let mut args = FluentArgs::new();
            args.set("name", "game.pgn");
            args.set("modified", modified);
            i18n.tr_args("window-title", &args)
        };
        assert_eq!(title("yes"), "*game.pgn - MyChess", "{lang:?}");
        assert_eq!(title("no"), "game.pgn - MyChess", "{lang:?}");
    }
}

#[test]
fn m2_3_turn_texts_carry_the_move_number_and_no_isolation_marks() {
    for (lang, white, black) in [
        (Language::EnUs, "White to move (7)", "Black to move (7)"),
        (Language::PtBr, "Brancas jogam (7)", "Pretas jogam (7)"),
    ] {
        let i18n = I18n::new(lang);
        let mut args = FluentArgs::new();
        args.set("move", 7);
        assert_eq!(i18n.tr_args("status-turn-white", &args), white);
        assert_eq!(i18n.tr_args("status-turn-black", &args), black);
        assert!(!white.contains('\u{2068}'));
    }
}

#[test]
fn m2_3_missing_arguments_do_not_panic() {
    for lang in Language::ALL {
        let i18n = I18n::new(lang);
        let text = i18n.tr("status-turn-white");
        assert!(!text.is_empty());
        let text = i18n.tr_args("status-turn-white", &FluentArgs::new());
        assert!(!text.is_empty());
    }
}

#[rstest]
#[case(Some("pt_BR.UTF-8"), Language::PtBr)]
#[case(Some("pt_BR"), Language::PtBr)]
#[case(Some("pt-BR"), Language::PtBr)]
#[case(Some("pt_PT.UTF-8"), Language::PtBr)]
#[case(Some("pt"), Language::PtBr)]
#[case(Some("PT_br"), Language::PtBr)]
#[case(Some("pt_BR.UTF-8@euro"), Language::PtBr)]
#[case(Some("en_US.UTF-8"), Language::EnUs)]
#[case(Some("en-GB"), Language::EnUs)]
#[case(Some("de_DE.UTF-8"), Language::EnUs)]
#[case(Some("es"), Language::EnUs)]
#[case(Some("C"), Language::EnUs)]
#[case(Some("C.UTF-8"), Language::EnUs)]
#[case(Some("POSIX"), Language::EnUs)]
#[case(Some(""), Language::EnUs)]
#[case(Some("@@@"), Language::EnUs)]
#[case(Some("ptolemaic"), Language::EnUs)]
#[case(Some("🙂"), Language::EnUs)]
#[case(None, Language::EnUs)]
fn m2_3_system_locale_maps_portuguese_to_pt_br_and_the_rest_to_en_us(
    #[case] locale: Option<&str>,
    #[case] expected: Language,
) {
    assert_eq!(language_from_locale(locale), expected, "{locale:?}");
}

#[test]
fn m2_3_explicit_language_preferences_ignore_the_system() {
    assert_eq!(LanguagePref::EnUs.resolve(), Language::EnUs);
    assert_eq!(LanguagePref::PtBr.resolve(), Language::PtBr);
    // `System` depends on the machine: it must at least resolve to a language.
    assert!(Language::ALL.contains(&LanguagePref::System.resolve()));
}

// ---------------------------------------------------------------------------
// Shortcuts (rules 14 and 59)
// ---------------------------------------------------------------------------

fn binding(key: Key, command: bool, shift: bool, alt: bool) -> Binding {
    Binding {
        key,
        modifiers: Modifiers {
            command,
            shift,
            alt,
        },
        scope: Scope::Always,
    }
}

#[test]
fn m2_3_flip_shortcut_label_per_platform() {
    let flip = binding(Key::Char('f'), true, false, false);
    assert_eq!(binding_label(&flip, Platform::Linux), "Ctrl+F");
    assert_eq!(binding_label(&flip, Platform::Windows), "Ctrl+F");
    assert_eq!(binding_label(&flip, Platform::MacOs), "⌘F");
}

#[test]
fn m2_3_labels_with_more_modifiers() {
    let b = binding(Key::Char('n'), true, true, false);
    assert_eq!(binding_label(&b, Platform::Linux), "Ctrl+Shift+N");
    assert_eq!(binding_label(&b, Platform::MacOs), "⇧⌘N");
    let plain = binding(Key::Char('x'), false, false, false);
    assert_eq!(binding_label(&plain, Platform::Linux), "X");
    assert_eq!(binding_label(&plain, Platform::MacOs), "X");
}

#[test]
fn m2_3_current_platform_matches_the_build_target() {
    let expected = if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    };
    assert_eq!(Platform::current(), expected);
}

#[test]
fn m2_3_ctrl_or_cmd_f_is_the_flip_shortcut() {
    let shortcuts = mychess_app::AppState::default().shortcuts;
    // `Modifiers::COMMAND` is Ctrl on Linux and Windows and Cmd on macOS.
    for ch in ["f", "F"] {
        let input = key_input(&IcedKey::Character(ch.into()), keyboard::Modifiers::COMMAND)
            .expect("a character key");
        assert_eq!(
            shortcuts.lookup(&input),
            Some(Action::FlipBoard),
            "{ch} with Command"
        );
    }
}

#[test]
fn m2_3_f_without_command_is_not_the_flip_shortcut() {
    let shortcuts = mychess_app::AppState::default().shortcuts;
    for mods in [keyboard::Modifiers::empty(), keyboard::Modifiers::ALT] {
        let input = key_input(&IcedKey::Character("f".into()), mods).expect("character key");
        assert_ne!(
            shortcuts.lookup(&input),
            Some(Action::FlipBoard),
            "{mods:?}"
        );
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn m2_3_the_logo_key_is_not_command_outside_macos() {
    let input = key_input(&IcedKey::Character("f".into()), keyboard::Modifiers::LOGO)
        .expect("character key");
    assert!(!input.modifiers.command);
    let ctrl = key_input(&IcedKey::Character("f".into()), keyboard::Modifiers::CTRL)
        .expect("character key");
    assert!(
        ctrl.modifiers.command,
        "Ctrl is Command on Linux and Windows"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn m2_3_the_control_key_is_not_command_on_macos() {
    let ctrl = key_input(&IcedKey::Character("f".into()), keyboard::Modifiers::CTRL)
        .expect("character key");
    assert!(!ctrl.modifiers.command);
    let logo = key_input(&IcedKey::Character("f".into()), keyboard::Modifiers::LOGO)
        .expect("character key");
    assert!(logo.modifiers.command, "Cmd is Command on macOS");
}

#[test]
fn m2_3_key_input_rejects_what_the_map_cannot_name() {
    let none = keyboard::Modifiers::empty();
    assert_eq!(key_input(&IcedKey::Unidentified, none), None);
    assert_eq!(key_input(&IcedKey::Character("".into()), none), None);
    assert_eq!(key_input(&IcedKey::Character("ab".into()), none), None);
    assert_eq!(key_input(&IcedKey::Named(Named::MediaPlay), none), None);
    assert_eq!(key_input(&IcedKey::Named(Named::Shift), none), None);
}

#[test]
fn m2_3_key_input_maps_named_keys_and_space() {
    let none = keyboard::Modifiers::empty();
    let cases = [
        (Named::Escape, NamedKey::Escape),
        (Named::Enter, NamedKey::Enter),
        (Named::ArrowLeft, NamedKey::Left),
        (Named::ArrowRight, NamedKey::Right),
        (Named::Home, NamedKey::Home),
        (Named::End, NamedKey::End),
        (Named::F1, NamedKey::F(1)),
        (Named::F12, NamedKey::F(12)),
    ];
    for (iced, app) in cases {
        let input = key_input(&IcedKey::Named(iced), none).expect("named key");
        assert_eq!(
            input,
            KeyInput {
                key: Key::Named(app),
                modifiers: Modifiers::NONE
            }
        );
    }
    let space = key_input(&IcedKey::Character(" ".into()), none).expect("space");
    assert_eq!(space.key, Key::Named(NamedKey::Space));
}

#[test]
fn m2_3_command_f_through_the_shell_flips_the_board_and_twice_restores_it() {
    use mychess_gui::widgets::board::geometry::Orientation;
    let mut shell = Shell::new(None);
    let input = key_input(
        &IcedKey::Character("f".into()),
        keyboard::Modifiers::COMMAND,
    )
    .expect("character key");
    let press = |shell: &mut Shell| {
        drop(shell.update(Message::Ui(UiMessage::Key {
            input,
            text_focused: false,
        })));
    };
    press(&mut shell);
    assert_eq!(shell.ui.orientation, Orientation::Black);
    press(&mut shell);
    assert_eq!(shell.ui.orientation, Orientation::White);
}

// ---------------------------------------------------------------------------
// Options and destinations
// ---------------------------------------------------------------------------

#[test]
fn m2_3_layout_options_carry_the_coordinate_mode() {
    for coords in [
        CoordsMode::None,
        CoordsMode::Outside,
        CoordsMode::OutsideBothSides,
        CoordsMode::Inside,
    ] {
        let o = BoardOptions {
            coords,
            ..BoardOptions::default()
        };
        assert_eq!(o.layout_options().coords, coords);
    }
}

#[test]
fn m2_3_board_option_defaults_follow_the_spec() {
    let o = BoardOptions::default();
    assert_eq!(o.coords, CoordsMode::Outside);
    assert!(o.highlight_last_move);
    assert!(o.show_legal_hints && o.show_marks);
    assert_eq!(
        o.theme,
        mychess_gui::theme::board_theme::BoardThemeId::MyChessGreen
    );
}

#[test]
fn m2_3_chess960_castling_that_keeps_the_king_in_place_does_not_hint_the_origin() {
    // King g1 with the rook on h1: the castling move keeps the king on g1.
    // Showing a legal-destination dot on the selected piece's own square
    // would be wrong (clicking it deselects instead of castling).
    let pos = mychess_core::Position::from_fen("4k3/8/8/8/8/8/8/6KR w H - 0 1").expect("fen");
    let g1: Square = "g1".parse().expect("square");
    let dests = legal_destinations(&pos, g1);
    assert!(
        dests.iter().all(|d| d.square != g1),
        "the origin square is listed as a destination: {dests:?}"
    );
}
