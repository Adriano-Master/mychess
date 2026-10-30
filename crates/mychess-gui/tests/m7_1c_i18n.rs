//! M7-1c (GUI, spec M7-1, criteria 31 and 33): the words of the maintenance
//! window exist in both languages, the window follows the language and the
//! theme, and the Portuguese says "variante".
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m7_1c_common;

use m7_1c_common::*;
use mychess_app::maintenance::MAINT_KEYS;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::maintenance::text::{MARK_FLAGS, flag_label};

#[test]
fn every_key_of_the_application_and_every_dynamic_key_exists_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in MAINT_KEYS {
            assert!(i18n.has(key), "{key} missing in {language:?}");
        }
        for criterion in SortCriterion::ALL {
            let key = format!("maint-sort-key-{}", criterion.name());
            assert!(i18n.has(&key), "{key} missing in {language:?}");
        }
        for n in 0..32 {
            assert!(i18n.has(&format!("maint-type-{n}")), "{n} in {language:?}");
        }
        for flag in MARK_FLAGS {
            let label = flag_label(&i18n, flag, &Default::default());
            assert!(!label.starts_with("maint-"), "{label}");
        }
    }
}

#[test]
fn the_portuguese_texts_say_variante_and_never_variacao() {
    let i18n = I18n::new(Language::PtBr);
    for key in ["maint-strip-variations", "maint-twins-keep-variations"] {
        let text = i18n.tr(key);
        assert!(text.contains("variantes"), "{text}");
        assert!(!text.to_lowercase().contains("variaç"), "{text}");
    }
}

#[test]
fn the_window_is_drawn_in_portuguese_and_in_both_themes() {
    for mode in [iced::theme::Mode::Light, iced::theme::Mode::Dark] {
        let mut m = Maint::six();
        m.rig.rig.shell.app.prefs.general.language = mychess_app::LanguagePref::PtBr;
        m.rig.msg(Message::Ui(UiMessage::SystemTheme(mode)));
        m.open();
        m.ui(MaintUiMessage::OpenForm(MaintForm::Twins));
        let mut sim = m.sim();
        assert!(sim.find("Manutenção do banco").is_ok(), "{mode:?}");
        assert!(sim.find("Critérios").is_ok(), "{mode:?}");
        assert!(sim.find("Mesmas cores").is_ok(), "{mode:?}");
    }
}

#[test]
fn the_tools_menu_has_the_three_entries() {
    let i18n = I18n::new(Language::EnUs);
    for key in [
        "menu-tools-maintenance",
        "menu-tools-name-editor",
        "menu-tools-repair-base",
    ] {
        assert!(i18n.has(key), "{key}");
    }
}
