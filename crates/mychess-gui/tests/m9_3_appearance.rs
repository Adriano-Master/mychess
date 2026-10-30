//! M9-3 (GUI): the appearance window (spec
//! `docs/specs/m9-3-janela-de-aparencia.md`, D1700-D1706). Headless.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::path::Path;

use iced::widget::Id;
use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{
    AccentPref, Key, KeyInput, LanguagePref, Message as AppMessage, Modifiers, NamedKey,
    PrefChange, PrefsMessage, ThemePref,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::theme::board_theme::BoardThemeId;
use mychess_gui::theme::tokens::Tokens;
use mychess_gui::theme::{AccentId, ThemeMode};
use mychess_gui::widgets::board::geometry::CoordsMode;
use mychess_gui::widgets::board::pieces::PieceSetId;
use mychess_gui::windows::appearance::{
    AppearanceDraft, AppearanceMessage, AppearanceTab, PlayerSwitch,
};
use mychess_gui::windows::appearance_view::{APPEARANCE_BOX_ID, PREVIEW_ID, box_size};
use mychess_gui::windows::dialog_ui::{ActiveDialog, DialogMessage};
use mychess_gui::windows::main_window;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::{Message, Shell};

/// A shell in English (the machine's language must not matter).
fn english() -> Shell {
    let mut shell = Shell::new(None);
    send(
        &mut shell,
        Message::App(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Language(
            LanguagePref::EnUs,
        )))),
    );
    shell
}

fn open() -> Shell {
    let mut shell = english();
    send(&mut shell, appearance(AppearanceMessage::Open));
    shell
}

fn appearance(message: AppearanceMessage) -> Message {
    Message::Dialog(DialogMessage::Appearance(message))
}

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

fn key(named: NamedKey) -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(named),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

fn draft(shell: &Shell) -> AppearanceDraft {
    shell
        .ui
        .dialogs
        .appearance
        .as_ref()
        .expect("window open")
        .draft
}

fn is_open(shell: &Shell) -> bool {
    shell.ui.dialogs.appearance.is_some()
}

fn sim(shell: &Shell, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), size, main_window::view(shell))
}

fn click(shell: &mut Shell, id: &'static str) {
    let messages: Vec<Message> = {
        let mut s = sim(shell, Size::new(1000.0, 700.0));
        s.click(Id::new(id))
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        s.into_messages().collect()
    };
    for message in messages {
        send(shell, message);
    }
}

fn shown(shell: &Shell, id: &'static str) -> bool {
    sim(shell, Size::new(1000.0, 700.0))
        .find(Id::new(id))
        .is_ok()
}

// ---- opening, the layer and the keys ----

#[test]
fn the_view_menu_item_opens_the_window_over_the_page() {
    let mut shell = english();
    assert!(!shell.modal_layer_open());
    let specs = menu_specs(&shell.app, &shell.ui, shell.options);
    let view = specs.iter().find(|s| s.label_key == "menu-view").unwrap();
    let Some(MenuEntry::Item {
        action: Some(message),
        ..
    }) = view.entries.iter().find(
        |e| matches!(e, MenuEntry::Item { label: Label::Key(k), .. } if *k == "menu-view-theme"),
    )
    else {
        panic!("the Theme command");
    };
    send(&mut shell, message.clone());
    assert!(is_open(&shell));
    assert!(shell.modal_layer_open());
    assert_eq!(
        ActiveDialog::of(&shell.app, &shell.ui.dialogs),
        Some(ActiveDialog::Appearance)
    );
    assert!(shown(&shell, APPEARANCE_BOX_ID));
    assert!(shown(&shell, "board-pane"), "the page stays below");
    assert!(shown(&shell, PREVIEW_ID));
}

#[test]
fn the_draft_starts_from_the_current_preferences() {
    let mut shell = Shell::new(None);
    send(
        &mut shell,
        Message::App(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Accent(
            AccentPref::Orange,
        )))),
    );
    send(
        &mut shell,
        Message::App(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Theme(
            ThemePref::Dark,
        )))),
    );
    send(&mut shell, appearance(AppearanceMessage::Open));
    let d = draft(&shell);
    assert_eq!(d.accent, AccentId::Orange);
    assert_eq!(d.mode, ThemeMode::Dark);
}

#[test]
fn escape_cancels_and_enter_applies_without_leaking() {
    let mut shell = open();
    send(
        &mut shell,
        appearance(AppearanceMessage::SetAccent(AccentId::Purple)),
    );
    send(&mut shell, key(NamedKey::Escape));
    assert!(!is_open(&shell));
    assert_ne!(
        Tokens::of(&shell.theme()).accent,
        AccentId::Purple.color(false)
    );
    assert!(!shell.modal_layer_open());

    let mut shell = open();
    send(
        &mut shell,
        appearance(AppearanceMessage::SetMode(ThemeMode::Light)),
    );
    send(
        &mut shell,
        appearance(AppearanceMessage::SetAccent(AccentId::Purple)),
    );
    let games_before = shell
        .ui
        .is_open(mychess_gui::windows::layout::PaneKind::GameList);
    send(&mut shell, key(NamedKey::Enter));
    assert!(!is_open(&shell));
    assert_eq!(
        Tokens::of(&shell.theme()).accent,
        AccentId::Purple.color(false)
    );
    // Enter did not reach the board's move entry nor the panes.
    assert_eq!(
        shell
            .ui
            .is_open(mychess_gui::windows::layout::PaneKind::GameList),
        games_before
    );
}

#[test]
fn the_arrows_change_the_tab() {
    let mut shell = open();
    send(&mut shell, key(NamedKey::Down));
    assert_eq!(
        shell.ui.dialogs.appearance.as_ref().unwrap().tab,
        AppearanceTab::Board
    );
    send(&mut shell, key(NamedKey::Up));
    assert_eq!(
        shell.ui.dialogs.appearance.as_ref().unwrap().tab,
        AppearanceTab::Theme
    );
}

// ---- the draft does not touch the application until Apply ----

#[test]
fn choices_change_the_draft_but_not_the_application_until_apply() {
    let mut shell = open();
    let before = shell.theme();
    let board_before = shell.app.prefs.board.clone();
    for message in [
        AppearanceMessage::SetMode(ThemeMode::Dark),
        AppearanceMessage::SetAccent(AccentId::Red),
        AppearanceMessage::SetBoardTheme(BoardThemeId::ScidBrown),
        AppearanceMessage::SetCoords(CoordsMode::Outside),
        AppearanceMessage::SetPieceSet(PieceSetId::Merida),
        AppearanceMessage::SetSwitch(PlayerSwitch::Clocks, false),
    ] {
        send(&mut shell, appearance(message));
    }
    assert_eq!(
        Tokens::of(&shell.theme()).accent,
        Tokens::of(&before).accent
    );
    assert_eq!(shell.app.prefs.board, board_before);
    assert!(shell.app.prefs.players.show_clocks);

    send(&mut shell, appearance(AppearanceMessage::Apply));
    assert!(!is_open(&shell));
    assert_eq!(shell.app.prefs.board.theme, "scid-brown");
    assert_eq!(shell.app.prefs.board.piece_set, "merida");
    assert!(!shell.app.prefs.players.show_clocks);
    assert_eq!(Tokens::of(&shell.theme()).accent, AccentId::Red.color(true));
    // The coordinates went through UiMessage::Coords.
    let reopened = {
        send(&mut shell, appearance(AppearanceMessage::Open));
        draft(&shell)
    };
    assert_eq!(reopened.coords, CoordsMode::Outside);
}

#[test]
fn cancel_discards_the_draft() {
    let mut shell = open();
    send(
        &mut shell,
        appearance(AppearanceMessage::SetBoardTheme(BoardThemeId::ScidTan)),
    );
    click(&mut shell, "appearance-cancel");
    assert!(!is_open(&shell));
    assert_ne!(shell.app.prefs.board.theme, "scid-tan");
}

#[test]
fn apply_sends_only_what_changed() {
    let mut shell = open();
    send(&mut shell, appearance(AppearanceMessage::Apply));
    assert!(!is_open(&shell));
    // Nothing differed: the preferences were not touched.
    let fresh = Shell::new(None);
    assert_eq!(shell.app.prefs.board, fresh.app.prefs.board);
    assert_eq!(
        shell.app.prefs.general.accent,
        fresh.app.prefs.general.accent
    );
}

// ---- tabs, choices and previews ----

#[test]
fn every_tab_shows_its_choices_and_clicking_them_changes_the_draft() {
    let mut shell = open();
    // Theme tab.
    click(&mut shell, "appearance-mode-dark");
    assert_eq!(draft(&shell).mode, ThemeMode::Dark);
    for id in AccentId::ALL {
        click(&mut shell, id.name_key());
        assert_eq!(draft(&shell).accent, id);
    }
    // Board tab.
    click(&mut shell, "appearance-tab-board");
    for id in BoardThemeId::ALL {
        // The cards below the fold are reached by scrolling: here they
        // only have to exist; the first one is clicked.
        assert!(shown(&shell, id.name_key()), "{id:?}");
        send(&mut shell, appearance(AppearanceMessage::SetBoardTheme(id)));
        assert_eq!(draft(&shell).board_theme, id);
    }
    click(&mut shell, BoardThemeId::ALL[1].name_key());
    assert_eq!(draft(&shell).board_theme, BoardThemeId::ALL[1]);
    click(&mut shell, "appearance-coords-outside");
    assert_eq!(draft(&shell).coords, CoordsMode::Outside);
    // Pieces tab.
    click(&mut shell, "appearance-tab-pieces");
    for id in PieceSetId::ALL {
        assert!(shown(&shell, id.name_key()), "{id:?}");
        send(&mut shell, appearance(AppearanceMessage::SetPieceSet(id)));
        assert_eq!(draft(&shell).piece_set, id);
    }
    click(&mut shell, PieceSetId::ALL[1].name_key());
    assert_eq!(draft(&shell).piece_set, PieceSetId::ALL[1]);
    send(
        &mut shell,
        appearance(AppearanceMessage::SetMaterial(
            mychess_app::MaterialMode::Off,
        )),
    );
    assert_eq!(
        draft(&shell).players.material,
        mychess_app::MaterialMode::Off
    );
    // Players tab: a switch is a checkbox with its label.
    click(&mut shell, "appearance-tab-players");
    let messages: Vec<Message> = {
        let mut s = sim(&shell, Size::new(1000.0, 700.0));
        s.click("Clocks").expect("switch");
        s.into_messages().collect()
    };
    for message in messages {
        send(&mut shell, message);
    }
    assert!(!draft(&shell).players.show_clocks);
}

#[test]
fn the_tab_that_differs_gets_a_dot() {
    let mut shell = open();
    let mut s = sim(&shell, Size::new(1000.0, 700.0));
    assert!(s.find("\u{25cf}").is_err());
    drop(s);
    send(
        &mut shell,
        appearance(AppearanceMessage::SetAccent(AccentId::Pink)),
    );
    s = sim(&shell, Size::new(1000.0, 700.0));
    assert!(s.find("\u{25cf}").is_ok());
}

#[test]
fn the_theme_preview_follows_the_draft_and_every_tab_renders() {
    for tab in AppearanceTab::ALL {
        for (mode, accent) in [
            (ThemeMode::Dark, AccentId::Green),
            (ThemeMode::Light, AccentId::Pink),
            (ThemeMode::System, AccentId::Gray),
        ] {
            let mut shell = open();
            send(&mut shell, appearance(AppearanceMessage::SetMode(mode)));
            send(&mut shell, appearance(AppearanceMessage::SetAccent(accent)));
            send(&mut shell, appearance(AppearanceMessage::Tab(tab)));
            assert!(shown(&shell, PREVIEW_ID), "{tab:?} {mode:?}");
        }
    }
}

#[test]
fn every_piece_set_and_every_board_theme_renders_in_the_previews() {
    for id in PieceSetId::ALL {
        let mut shell = open();
        send(
            &mut shell,
            appearance(AppearanceMessage::Tab(AppearanceTab::Pieces)),
        );
        send(&mut shell, appearance(AppearanceMessage::SetPieceSet(id)));
        assert!(shown(&shell, PREVIEW_ID), "{id:?}");
    }
    for id in BoardThemeId::ALL {
        let mut shell = open();
        send(
            &mut shell,
            appearance(AppearanceMessage::Tab(AppearanceTab::Board)),
        );
        send(&mut shell, appearance(AppearanceMessage::SetBoardTheme(id)));
        assert!(shown(&shell, PREVIEW_ID), "{id:?}");
    }
}

#[test]
fn the_players_tab_previews_the_switches() {
    let mut shell = open();
    send(
        &mut shell,
        appearance(AppearanceMessage::Tab(AppearanceTab::Players)),
    );
    send(
        &mut shell,
        appearance(AppearanceMessage::SetSwitch(PlayerSwitch::Strips, false)),
    );
    let mut s = sim(&shell, Size::new(1000.0, 700.0));
    assert!(s.find("The player strips are hidden.").is_ok());
    drop(s);
    send(
        &mut shell,
        appearance(AppearanceMessage::SetSwitch(PlayerSwitch::Strips, true)),
    );
    let mut s = sim(&shell, Size::new(1000.0, 700.0));
    assert!(s.find("The player strips are hidden.").is_err());
    assert!(s.find("White player").is_ok());
}

// ---- size ----

#[test]
fn the_window_size_follows_the_application_window() {
    assert_eq!(box_size(Size::new(1920.0, 1080.0)), Size::new(880.0, 600.0));
    let small = box_size(Size::new(800.0, 560.0));
    assert!((small.width - 720.0).abs() < 0.01 && (small.height - 504.0).abs() < 0.01);
}

#[test]
fn the_window_fits_in_the_minimum_application_window_on_every_tab() {
    let window = Size::new(800.0, 560.0);
    for tab in AppearanceTab::ALL {
        let mut shell = open();
        send(&mut shell, appearance(AppearanceMessage::Tab(tab)));
        let mut s = sim(&shell, window);
        let target = s.find(Id::new(APPEARANCE_BOX_ID)).expect("box");
        let bounds = target.bounds();
        assert!(bounds.width <= window.width * 0.9 + 0.5, "{tab:?}");
        assert!(bounds.height <= window.height * 0.9 + 0.5, "{tab:?}");
        assert!(bounds.x >= 0.0 && bounds.y >= 0.0, "{tab:?}");
        assert!(bounds.x + bounds.width <= window.width, "{tab:?}");
        assert!(bounds.y + bounds.height <= window.height, "{tab:?}");
        let preview = s.find(Id::new(PREVIEW_ID)).expect("preview").bounds();
        assert!(
            preview.y + preview.height <= bounds.y + bounds.height,
            "{tab:?}"
        );
        drop(s);
    }
}

// ---- Fluent ----

fn ids(language: &str, file: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter_map(|l| {
            let (id, _) = l.split_once(" = ")?;
            (!id.starts_with(['#', ' ', '-'])).then(|| id.to_owned())
        })
        .collect()
}

#[test]
fn the_appearance_keys_exist_in_both_languages_with_the_same_ids() {
    let (en, pt) = (
        ids("en-US", "appearance.ftl"),
        ids("pt-BR", "appearance.ftl"),
    );
    assert_eq!(en, pt);
    assert!(en.len() >= 10);
    assert!(en.iter().all(|k| k.starts_with("appearance-")));
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in &en {
            assert_ne!(&i18n.tr(key), key, "{key} in {language:?}");
        }
    }
}

#[test]
fn pt_br_never_says_variacao_in_the_appearance_texts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/pt-BR/appearance.ftl");
    let text = std::fs::read_to_string(path).unwrap().to_lowercase();
    assert!(!text.contains("variaç") && !text.contains("variac"));
}

#[test]
fn the_theme_command_ends_with_an_ellipsis_in_both_languages() {
    // Spec M9-7 D4610: three dots, like every other command that opens a window.
    assert_eq!(I18n::new(Language::EnUs).tr("menu-view-theme"), "Theme...");
    assert_eq!(I18n::new(Language::PtBr).tr("menu-view-theme"), "Tema...");
}

#[test]
fn the_tabs_have_their_names() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    for (tab, e, p) in [
        (AppearanceTab::Theme, "Theme", "Tema"),
        (AppearanceTab::Board, "Board", "Tabuleiro"),
        (AppearanceTab::Pieces, "Pieces", "Peças"),
        (AppearanceTab::Players, "Players", "Jogadores"),
    ] {
        assert_eq!(en.tr(tab.name_key()), e);
        assert_eq!(pt.tr(tab.name_key()), p);
    }
}

// ---- review fixes (D1711 to D1713) ----

fn key_with(key: Key, modifiers: Modifiers, typed: Option<char>) -> Message {
    Message::Dialog(DialogMessage::Key {
        input: KeyInput { key, modifiers },
        text_focused: false,
        typed,
    })
}

fn pref(change: PrefChange) -> Message {
    Message::App(AppMessage::Prefs(PrefsMessage::Set(change)))
}

#[test]
fn enter_and_typed_keys_do_not_leak_to_the_game_under_the_window() {
    let mut shell = open();
    let before = shell.app.session.position().zobrist();
    send(
        &mut shell,
        key_with(Key::Char('e'), Modifiers::NONE, Some('e')),
    );
    send(
        &mut shell,
        key_with(Key::Char('4'), Modifiers::NONE, Some('4')),
    );
    assert!(is_open(&shell), "typing does not close the window");
    assert_eq!(shell.app.session.move_entry().text(), "");
    send(&mut shell, key(NamedKey::Enter));
    assert!(!is_open(&shell));
    assert_eq!(shell.app.session.move_entry().text(), "");
    assert_eq!(shell.app.session.position().zobrist(), before);
}

#[test]
fn shift_enter_and_ctrl_enter_do_not_apply() {
    for modifiers in [
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        Modifiers::COMMAND,
    ] {
        let mut shell = open();
        send(
            &mut shell,
            appearance(AppearanceMessage::SetAccent(AccentId::Pink)),
        );
        send(
            &mut shell,
            key_with(Key::Named(NamedKey::Enter), modifiers, None),
        );
        assert!(is_open(&shell), "{modifiers:?}");
        assert_ne!(shell.app.prefs.general.accent, AccentPref::Pink);
    }
}

#[test]
fn enter_with_the_window_closed_applies_nothing() {
    let mut shell = open();
    send(
        &mut shell,
        appearance(AppearanceMessage::SetAccent(AccentId::Pink)),
    );
    send(&mut shell, key(NamedKey::Escape));
    assert!(!is_open(&shell));
    send(&mut shell, key(NamedKey::Enter));
    assert!(!is_open(&shell));
    assert_ne!(shell.app.prefs.general.accent, AccentPref::Pink);
}

#[test]
fn apply_keeps_what_changed_elsewhere_while_the_window_was_open() {
    let mut shell = open();
    // Something else changes the accent and a strip switch meanwhile.
    send(&mut shell, pref(PrefChange::Accent(AccentPref::Blue)));
    let mut players = shell.app.prefs.players;
    players.show_clocks = !players.show_clocks;
    let clocks = players.show_clocks;
    send(&mut shell, pref(PrefChange::Players(players)));
    // The user touches only the coordinates and the ratings switch.
    send(
        &mut shell,
        appearance(AppearanceMessage::SetCoords(CoordsMode::Outside)),
    );
    let ratings = !shell.app.prefs.players.show_ratings;
    send(
        &mut shell,
        appearance(AppearanceMessage::SetSwitch(PlayerSwitch::Ratings, ratings)),
    );
    send(&mut shell, appearance(AppearanceMessage::Apply));
    assert_eq!(shell.app.prefs.general.accent, AccentPref::Blue);
    assert_eq!(shell.app.prefs.players.show_clocks, clocks);
    assert_eq!(shell.app.prefs.players.show_ratings, ratings);
    send(&mut shell, appearance(AppearanceMessage::Open));
    assert_eq!(draft(&shell).coords, CoordsMode::Outside);
}

/// Clicks the widget `id` after scrolling the choices to their end.
fn click_after_scrolling(shell: &mut Shell, id: &'static str) {
    use iced::{Event, Point, mouse};
    let messages: Vec<Message> = {
        let mut s = sim(shell, Size::new(1000.0, 700.0));
        s.point_at(Point::new(600.0, 560.0));
        let wheel = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -60.0 },
        });
        drop(s.simulate([wheel]));
        s.click(Id::new(id))
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
        s.into_messages().collect()
    };
    for message in messages {
        send(shell, message);
    }
}

#[test]
fn the_last_card_of_each_grid_and_every_tab_can_be_really_clicked() {
    let mut shell = open();
    click(&mut shell, "appearance-tab-board");
    let last = *BoardThemeId::ALL.last().unwrap();
    click_after_scrolling(&mut shell, last.name_key());
    assert_eq!(draft(&shell).board_theme, last);
    click(&mut shell, "appearance-tab-pieces");
    let last = *PieceSetId::ALL.last().unwrap();
    click_after_scrolling(&mut shell, last.name_key());
    assert_eq!(draft(&shell).piece_set, last);
    for tab in AppearanceTab::ALL {
        click(&mut shell, tab.button_id());
        assert_eq!(shell.ui.dialogs.appearance.as_ref().unwrap().tab, tab);
    }
}
