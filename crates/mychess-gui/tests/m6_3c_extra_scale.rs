//! M6-3c extra (GUI): the windows with huge lists, hostile names, both
//! themes, small sizes and both languages (spec M6-3, "Textos de interface",
//! criteria 40 to 45).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_3c_common;

use std::fmt::Write as _;

use iced::Size;
use iced::widget::Id;
use m6_3c_common::*;
use mychess_app::{
    Action, FinderMessage, NameKind, PlayerField, PlayerInfoMessage, SpellingMessage,
};
use mychess_gui::theme::{AccentId, ThemeMode, resolve};
use mychess_gui::windows::player_tools::finder_view as fv;

fn themes() -> [iced::Theme; 2] {
    [
        resolve(
            ThemeMode::Dark,
            iced::theme::Mode::Dark,
            AccentId::default(),
        ),
        resolve(
            ThemeMode::Light,
            iced::theme::Mode::Light,
            AccentId::default(),
        ),
    ]
}

fn pgn_of(names: &[(String, String)]) -> String {
    let mut out = String::new();
    for (white, black) in names {
        let _ = write!(
            out,
            "[Event \"E\"]\n[Site \"S\"]\n[Date \"2020.01.01\"]\n[Round \"1\"]\n\
             [White \"{white}\"]\n[Black \"{black}\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n"
        );
    }
    out
}

fn open_pgn_as_si4(t: &mut Tools, text: &str) {
    let path = base::make_si4(t.dir.path(), "big", text);
    t.rig.app_msg(BasesMessage::Open(path));
}

#[test]
fn thousands_of_players_render_only_the_visible_rows() {
    let names: Vec<(String, String)> = (0..2500)
        .map(|n| (format!("P{n:05}, X"), format!("Q{n:05}, Y")))
        .collect();
    let mut t = Tools::new();
    open_pgn_as_si4(&mut t, &pgn_of(&names));
    t.action(Action::PlayerFinder);
    let total = t.app().player_finder_view().expect("open").rows.len();
    // The finder lists at most 1000 players by default (the "Max" field).
    assert_eq!(total, 1000, "players listed");
    let mut sim = t.sim_default();
    assert!(sim.find(fv::player_row_id(0)).is_ok());
    // Virtualised: the last row is not a widget of the first screen.
    assert!(sim.find(fv::player_row_id(total - 1)).is_err());
    drop(sim);
    // End jumps to the last row and brings it into view.
    t.named(NamedKey::End);
    assert_eq!(
        t.shell().ui.dialogs.player_tools.players.cursor,
        Some(total - 1)
    );
    for theme in themes() {
        assert!(t.sim(Size::new(640.0, 400.0)).snapshot(&theme).is_ok());
    }
    // Page keys never go past the ends.
    for _ in 0..3 {
        t.named(NamedKey::PageDown);
    }
    assert_eq!(
        t.shell().ui.dialogs.player_tools.players.cursor,
        Some(total - 1)
    );
    t.named(NamedKey::Home);
    t.named(NamedKey::PageUp);
    assert_eq!(t.shell().ui.dialogs.player_tools.players.cursor, Some(0));
}

#[test]
fn thousands_of_corrections_render_in_the_name_checker() {
    let mut text = String::from("@PLAYER \"., -_*\"\n");
    let mut names = Vec::new();
    for n in 0..1500 {
        let _ = writeln!(text, "Good{n:04}, G\n  = Bad{n:04}, G");
        names.push((format!("Bad{n:04}, G"), format!("Good{n:04}, G")));
    }
    let mut t = Tools::new();
    open_pgn_as_si4(&mut t, &pgn_of(&names));
    let path = t.dir.path().join("big.ssp");
    std::fs::write(&path, text).expect("write");
    t.rig.app_msg(SpellingMessage::Load(path));
    t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
    let view = t.app().spell_view().expect("open");
    assert_eq!(view.total, 1500);
    let mut sim = t.sim(Size::new(1000.0, 600.0));
    assert!(sim.find(Id::new("spell-check-box")).is_ok());
    drop(sim);
    for theme in themes() {
        assert!(t.sim(Size::new(640.0, 400.0)).snapshot(&theme).is_ok());
    }
    t.named(NamedKey::End);
    t.named(NamedKey::Home);
}

fn hostile_names() -> Vec<(String, String)> {
    vec![
        ("X".repeat(3000), "Short, S".into()),
        ("\u{202E}Evil, E".into(), "Short, S".into()),
        (
            "Mix \u{202B}ed \u{2067}bidi\u{202C}, M".into(),
            "Short, S".into(),
        ),
        ("Nul\0Name, N".into(), "Short, S".into()),
        ("Tab\tName, T".into(), "Short, S".into()),
        ("Zero\u{200B}width\u{FEFF}, Z".into(), "Short, S".into()),
        ("日本語, 太郎".into(), "Short, S".into()),
        ("{braces} [x] \"q\" %s {0}, B".into(), "Short, S".into()),
    ]
}

#[test]
fn hostile_names_draw_in_the_finders_the_information_and_the_checker() {
    let mut t = Tools::new();
    open_pgn_as_si4(&mut t, &pgn_of(&hostile_names()));
    let ssp = "@PLAYER \"., -_*\"\nShort, S #GM XXX [2500] 1980.01.01\n  = \u{202E}Alias, A\n  = AliasNul\0, N\n";
    let path = t.dir.path().join("h.ssp");
    std::fs::write(&path, ssp).expect("write");
    t.rig.app_msg(SpellingMessage::Load(path));
    let sizes = [
        Size::new(300.0, 200.0),
        Size::new(640.0, 400.0),
        Size::new(1300.0, 900.0),
    ];
    for theme in themes() {
        t.action(Action::PlayerFinder);
        t.finder(FinderMessage::SetPlayerField(
            PlayerField::Country,
            "yes".into(),
        ));
        t.finder(FinderMessage::UpdatePlayers);
        for size in sizes {
            assert!(t.sim(size).snapshot(&theme).is_ok(), "finder {size:?}");
        }
        t.named(NamedKey::End);
        t.named(NamedKey::Enter);
        for size in sizes {
            assert!(t.sim(size).snapshot(&theme).is_ok(), "info {size:?}");
        }
        t.named(NamedKey::Escape);
        t.named(NamedKey::Escape);
        t.action(Action::TournamentFinder);
        for size in sizes {
            assert!(t.sim(size).snapshot(&theme).is_ok(), "tournaments {size:?}");
        }
        t.named(NamedKey::Escape);
        t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
        for size in sizes {
            assert!(t.sim(size).snapshot(&theme).is_ok(), "checker {size:?}");
        }
        t.named(NamedKey::Escape);
        assert_eq!(t.top(), None);
    }
    // The information of every hostile name opens without a panic.
    for (white, _) in hostile_names() {
        t.rig.app_msg(PlayerInfoMessage::Open { name: white });
        drop(t.sim_default());
        t.rig.app_msg(PlayerInfoMessage::Close);
    }
}

#[test]
fn the_windows_snapshot_in_both_themes_and_languages_at_all_sizes() {
    for language in [LanguagePref::EnUs, LanguagePref::PtBr] {
        let mut t = Tools::with_language(language);
        t.open_base(&misspelled());
        t.load_ssp();
        for theme in themes() {
            for size in [Size::new(300.0, 200.0), Size::new(640.0, 400.0)] {
                t.action(Action::PlayerFinder);
                assert!(t.sim(size).snapshot(&theme).is_ok());
                t.action(Action::PlayerFinder);
                t.action(Action::TournamentFinder);
                assert!(t.sim(size).snapshot(&theme).is_ok());
                t.action(Action::TournamentFinder);
                t.rig.app_msg(SpellingMessage::OpenCheck(NameKind::Player));
                assert!(t.sim(size).snapshot(&theme).is_ok());
                t.rig.app_msg(SpellingMessage::CloseCheck);
                t.rig.app_msg(SpellingMessage::OpenAddElo);
                assert!(t.sim(size).snapshot(&theme).is_ok());
                t.rig.app_msg(SpellingMessage::CloseAddElo);
            }
        }
    }
}

#[test]
fn the_language_can_change_while_a_window_is_open() {
    let mut t = Tools::new();
    t.open_base(&six());
    t.action(Action::PlayerFinder);
    let mut sim = t.sim_default();
    assert!(sim.find("Games").is_ok());
    drop(sim);
    t.rig.rig.shell.app.prefs.general.language = LanguagePref::PtBr;
    // Any message makes the shell reload the translations.
    t.named(NamedKey::Down);
    let mut sim = t.sim_default();
    assert!(sim.find("Partidas").is_ok(), "now in Portuguese");
    assert!(sim.find("Games").is_err());
    drop(sim);
    t.rig.rig.shell.app.prefs.general.language = LanguagePref::EnUs;
    t.named(NamedKey::Down);
    assert!(t.sim_default().find("Games").is_ok());
}

#[test]
fn every_fluent_key_the_player_tools_use_exists_in_both_files() {
    // Every `message_key()` of the notices, plus the keys of the labels the
    // windows build, exist in en-US and pt-BR with the same set of ids.
    let en = ftl_ids("en-US");
    let pt = ftl_ids("pt-BR");
    for id in en.difference(&pt) {
        if id.contains("spell")
            || id.contains("finder")
            || id.contains("player")
            || id.contains("elo")
        {
            panic!("{id} exists in en-US only");
        }
    }
    for id in pt.difference(&en) {
        if id.contains("spell")
            || id.contains("finder")
            || id.contains("player")
            || id.contains("elo")
        {
            panic!("{id} exists in pt-BR only");
        }
    }
}

fn ftl_ids(language: &str) -> std::collections::BTreeSet<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language);
    let mut ids = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "ftl") {
            for line in std::fs::read_to_string(&path).expect("read").lines() {
                if let Some((id, _)) = line.split_once(" =")
                    && !id.starts_with([' ', '#', '-', '.'])
                    && !id.is_empty()
                {
                    ids.insert(id.trim().to_owned());
                }
            }
        }
    }
    ids
}
