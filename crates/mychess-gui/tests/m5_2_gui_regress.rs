//! M5-2a review regression of the GUI: the plural of `tree-unreadable`
//! (D989) in both languages, through the i18n API and through the rendered
//! panel with exactly one unreadable game.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use fluent_bundle::FluentArgs;
use m5_2_gui_common::*;
use mychess_app::{BasesMessage, LanguagePref};
use tempfile::tempdir;

const HEADER_LEN: usize = 182;
const ENTRY_LEN: usize = 47;

fn break_offset(si4: &std::path::Path, n: usize) {
    let mut bytes = std::fs::read(si4).unwrap();
    let at = HEADER_LEN + ENTRY_LEN * (n - 1);
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FF00_u32.to_be_bytes());
    std::fs::write(si4, bytes).unwrap();
}

fn unreadable_text(i18n: &mychess_gui::i18n::I18n, count: i64) -> String {
    let mut args = FluentArgs::new();
    args.set("count", count);
    i18n.tr_args("tree-unreadable", &args)
}

#[test]
fn the_unreadable_warning_has_a_distinct_singular_in_both_languages() {
    let cases = [
        (
            en(),
            "1 game could not be read",
            "2 games could not be read",
        ),
        (
            pt(),
            "1 partida não pôde ser lida",
            "2 partidas não puderam ser lidas",
        ),
    ];
    for (i18n, one, two) in cases {
        let (one_text, two_text) = (unreadable_text(&i18n, 1), unreadable_text(&i18n, 2));
        // Fluent wraps numbers in isolation marks in some builds; compare
        // without the invisible control characters.
        let clean = |s: &str| {
            s.chars()
                .filter(|c| !c.is_control() && !matches!(c, '\u{2066}'..='\u{2069}'))
                .collect::<String>()
        };
        assert_eq!(clean(&one_text), one);
        assert_eq!(clean(&two_text), two);
        assert_ne!(one_text, two_text);
        assert!(!one_text.contains("1 games") && !one_text.contains("1 partidas"));
        assert!(!two_text.contains("2 game ") && !two_text.contains("2 partida "));
        // 0 is not asserted: CLDR puts 0 in the singular for pt-BR.
        assert!(unreadable_text(&i18n, 0).contains('0'));
        assert!(clean(&unreadable_text(&i18n, 21)).starts_with("21 "));
    }
}

#[test]
fn a_panel_with_one_unreadable_game_shows_the_singular_text() {
    let dir = tempdir().unwrap();
    let mut text = String::new();
    for _ in 0..3 {
        text.push_str(&game_pgn("1. e4 e5", "1-0", None));
    }
    for (language, warning, wrong) in [
        (LanguagePref::EnUs, "1 game could not be read", "1 games"),
        (
            LanguagePref::PtBr,
            "1 partida não pôde ser lida",
            "1 partidas",
        ),
    ] {
        let si4 = make_si4(dir.path(), &format!("one-{}", language as u8), &text);
        break_offset(&si4, 2);
        let mut rig = TreeRig::with_language(language);
        rig.app_msg(BasesMessage::Open(si4));
        rig.open_panel();
        assert_eq!(rig.view().unreadable, 1);
        assert!(pane_shows(&rig, warning), "{warning}");
        assert!(!pane_shows(&rig, wrong), "{wrong}");
        assert!(pane_shows(&rig, "e4"), "the table is still there");
    }
}
