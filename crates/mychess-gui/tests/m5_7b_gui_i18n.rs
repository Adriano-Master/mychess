//! M5-7b GUI contract: the texts (spec `docs/specs/m5-7-eco-e-novidade.md`,
//! D1721, D1725 and the i18n criterion): en-US and pt-BR complete, no
//! "variação" in pt-BR, the glossary, the notices, and the ids of the boxes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_7b_gui_common;

use m5_7b_gui_common::*;
use mychess_core::Color;
use mychess_gui::windows::eco_text::{glossary, notice_text, translated_name};
use mychess_gui::windows::{eco_browser, eco_dialogs};
use tempfile::tempdir;

/// The message ids of a Fluent file (a line that starts with `id =`).
fn ids(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && l.contains(" ="))
        .map(|l| l.split(" =").next().unwrap().trim().to_owned())
        .collect()
}

#[test]
fn both_languages_define_the_same_messages() {
    let en_ids = ids(&ftl_text("en-US", "eco.ftl"));
    let pt_ids = ids(&ftl_text("pt-BR", "eco.ftl"));
    assert!(en_ids.len() > 60, "the file was read ({})", en_ids.len());
    let mut a = en_ids.clone();
    let mut b = pt_ids.clone();
    a.sort();
    b.sort();
    assert_eq!(a, b, "en-US and pt-BR have the same ids");
    let (en, pt) = (en(), pt());
    for id in &en_ids {
        assert!(en.has(id), "en-US lacks {id}");
        assert!(pt.has(id), "pt-BR lacks {id}");
    }
}

fn ftl_text(language: &str, file: &str) -> String {
    base::ftl_text(language, file)
}

#[test]
fn portuguese_never_says_variacao_and_says_variante() {
    let text = ftl_text("pt-BR", "eco.ftl");
    let lower = text.to_lowercase();
    // The comments of the file may mention the rule; the messages and the
    // glossary may not use the word.
    for line in lower.lines().filter(|l| !l.trim_start().starts_with('#')) {
        assert!(!line.contains("variaç"), "{line}");
        assert!(!line.contains("variac"), "{line}");
    }
    assert!(text.contains("Variation => Variante"));
    let pt = pt();
    for id in ids(&text) {
        let rendered = pt.tr(&id).to_lowercase();
        assert!(!rendered.contains("variaç"), "{id}: {rendered}");
    }
    assert_eq!(
        translated_name(&pt, "Alekhine Variation"),
        "Alekhine Variante"
    );
    assert_eq!(
        translated_name(&pt, "Sicilian: Najdorf Variation"),
        "Siciliana: Najdorf Variante"
    );
}

#[test]
fn the_glossary_is_a_list_of_pairs_in_portuguese_and_empty_in_english() {
    assert!(glossary(&en()).is_empty());
    let pairs = glossary(&pt());
    assert!(pairs.len() > 20);
    assert!(pairs.iter().all(|(a, b)| !a.is_empty() && !b.is_empty()));
    // The generic words come first and the phrases that fix the order last
    // are the ones applied first (the Scid order); here the phrases lead the
    // file, so "Sicilian Defence" ends up as "Defesa Siciliana".
    assert_eq!(
        translated_name(&pt(), "Sicilian Defence"),
        "Defesa Siciliana"
    );
    assert_eq!(translated_name(&pt(), "French Defence"), "Defesa Francesa");
    assert_eq!(
        translated_name(&en(), "Sicilian Defence"),
        "Sicilian Defence"
    );
}

#[test]
fn the_glossary_survives_odd_names() {
    let pt = pt();
    for name in [
        "",
        " ",
        "=>",
        "Defence Defence Defence",
        "Ünïcode Variation",
        "Variation Variation",
    ] {
        let out = translated_name(&pt, name);
        assert!(
            !out.to_lowercase().contains("variação"),
            "{name:?} -> {out:?}"
        );
    }
}

#[test]
fn every_remark_has_text_in_both_languages_and_the_numbers_are_filled() {
    let notices = [
        EcoNotice::NoBook,
        EcoNotice::Opening {
            eco: "B90a2".to_owned(),
            name: "Sicilian Defence".to_owned(),
            ply: 11,
        },
        EcoNotice::NoOpening,
        EcoNotice::Classified {
            classified: 12,
            changed: 5,
            written: 4,
            preview: true,
            stopped: false,
        },
        EcoNotice::Classified {
            classified: 12,
            changed: 5,
            written: 5,
            preview: false,
            stopped: false,
        },
        EcoNotice::Classified {
            classified: 12,
            changed: 5,
            written: 2,
            preview: false,
            stopped: true,
        },
        EcoNotice::ReadOnly,
        EcoNotice::Novelty {
            move_number: 12,
            color: Color::Black,
            san: "Nf6".to_owned(),
        },
        EcoNotice::Novelty {
            move_number: 3,
            color: Color::White,
            san: "Bc4".to_owned(),
        },
        EcoNotice::NoNovelty,
        EcoNotice::NoveltyStopped,
        EcoNotice::NoveltyNonStandard,
        EcoNotice::NoveltyFailed,
        EcoNotice::LineFailed,
    ];
    for language in [en(), pt()] {
        for notice in &notices {
            let key = notice.message_key();
            assert!(language.has(key), "{key}");
            let text = notice_text(&language, notice);
            assert_ne!(text, key, "{key} was not translated");
            assert!(!text.contains('$'), "{key}: {text}");
            assert!(!text.is_empty());
        }
    }
    // The move label: one dot for White, three for Black.
    let n = |color, san: &str| EcoNotice::Novelty {
        move_number: 12,
        color,
        san: san.to_owned(),
    };
    assert_eq!(
        notice_text(&en(), &n(Color::Black, "Nf6")),
        "Novelty: 12...Nf6"
    );
    assert_eq!(
        notice_text(&en(), &n(Color::White, "Nf3")),
        "Novelty: 12.Nf3"
    );
    assert_eq!(
        notice_text(&pt(), &n(Color::Black, "Nf6")),
        "Novidade: 12...Nf6"
    );
    // The opening remark translates the name with the glossary.
    let opening = EcoNotice::Opening {
        eco: "B20".to_owned(),
        name: "Sicilian Defence".to_owned(),
        ply: 2,
    };
    assert!(notice_text(&pt(), &opening).contains("Defesa Siciliana"));
    assert!(notice_text(&en(), &opening).contains("Sicilian Defence"));
}

#[test]
fn the_menu_entries_exist_in_both_languages() {
    for key in [
        "menu-game-identify-opening",
        "menu-game-novelty",
        "menu-tools-eco-browser",
        "menu-tools-eco-classify",
    ] {
        assert!(en().has(key) && pt().has(key), "{key}");
    }
    assert_eq!(en().tr("menu-tools-eco-browser"), "ECO browser...");
    assert_eq!(pt().tr("menu-tools-eco-browser"), "Navegador ECO...");
}

#[test]
fn the_boxes_have_ids_for_the_focus_and_the_search_field_is_in_the_browser() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    open_browser(&mut rig);
    {
        let mut sim = box_sim(&rig, EcoBox::Browser);
        assert!(
            sim.find(iced::widget::Id::from(eco_browser::ECO_BROWSER_BOX_ID))
                .is_ok()
        );
        assert!(
            sim.find(iced::widget::Id::from(eco_browser::ECO_FIND_ID))
                .is_ok()
        );
    }
    rig.app_msg(AppMessage::Eco(EcoMessage::OpenClassify));
    {
        let mut sim = box_sim(&rig, EcoBox::Classify);
        assert!(
            sim.find(iced::widget::Id::from(eco_dialogs::ECO_CLASSIFY_BOX_ID))
                .is_ok()
        );
        assert!(
            sim.find(iced::widget::Id::from(eco_dialogs::ECO_SINCE_ID))
                .is_ok()
        );
    }
    rig.app_msg(AppMessage::Eco(EcoMessage::OpenNovelty));
    let mut sim = box_sim(&rig, EcoBox::Novelty);
    assert!(
        sim.find(iced::widget::Id::from(eco_dialogs::ECO_NOVELTY_BOX_ID))
            .is_ok()
    );
}

#[test]
fn the_three_boxes_are_modal_and_exclusive() {
    let dir = tempdir().unwrap();
    let mut rig = rig_with(LanguagePref::EnUs, dir.path());
    for (open, expected) in [
        (EcoMessage::OpenClassify, EcoWindow::Classify),
        (EcoMessage::OpenNovelty, EcoWindow::Novelty),
        (
            EcoMessage::OpenBrowser { glossary: vec![] },
            EcoWindow::Browser,
        ),
    ] {
        rig.app_msg(AppMessage::Eco(open));
        assert_eq!(window(&rig), Some(expected));
        assert!(rig.app().modal_open());
    }
}
