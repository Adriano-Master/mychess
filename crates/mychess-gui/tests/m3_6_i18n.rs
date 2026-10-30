//! Fluent keys and notice texts of M3-6 (spec "Plano de testes": pt-BR and
//! en-US complete; D680 notices with arguments). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_6_common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use m3_6_common::*;
use mychess_app::{AppNotice, BookmarkNotice};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::status_bar::app_notice_text;

const SOURCE_KEYS: &[&str] = &[
    "bookmarks-title",
    "bookmarks-empty",
    "bookmarks-title-placeholder",
    "bookmarks-up",
    "bookmarks-down",
    "bookmarks-indent",
    "bookmarks-outdent",
    "bookmarks-new-folder",
    "bookmarks-add",
    "bookmarks-delete",
    "bookmarks-go",
    "menu-file-bookmarks",
    "menu-file-bookmarks-add",
    "menu-file-bookmarks-manage",
    "menu-file-bookmarks-empty",
    "menu-file-open-recent",
    "menu-file-open-recent-none",
    "menu-file-open-recent-clear",
    "menu-file-read-only",
    "dialog-ok",
    "dialog-cancel",
];

fn all_notices() -> Vec<BookmarkNotice> {
    vec![
        BookmarkNotice::Added,
        BookmarkNotice::CannotAdd,
        BookmarkNotice::PathNotUtf8,
        BookmarkNotice::TreeFull,
        BookmarkNotice::BaseUnavailable {
            title: "My game".to_owned(),
        },
        BookmarkNotice::GameNotFound {
            title: "My game".to_owned(),
        },
        BookmarkNotice::Dropped { count: 1 },
        BookmarkNotice::Dropped { count: 3 },
        BookmarkNotice::FileUnreadable,
        BookmarkNotice::NewerVersion,
        BookmarkNotice::SaveFailed,
    ]
}

/// Removes the Unicode isolation marks Fluent wraps around arguments.
fn plain(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

fn text(language: Language, notice: BookmarkNotice) -> String {
    plain(&app_notice_text(
        &I18n::new(language),
        &AppNotice::Bookmark(notice),
    ))
}

fn ftl_ids(language: &str, file: &str) -> BTreeSet<String> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter_map(|l| {
            let (id, _) = l.split_once(" = ")?;
            (!id.starts_with(' ') && !id.starts_with('#') && !id.starts_with('-'))
                .then(|| id.to_owned())
        })
        .collect()
}

#[test]
fn every_key_the_gui_uses_for_bookmarks_recent_and_read_only_exists_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in SOURCE_KEYS {
            assert!(i18n.has(key), "{language:?} lacks {key}");
            let t = i18n.tr(key);
            assert!(
                !t.trim().is_empty() && t != *key,
                "{language:?} {key}: {t:?}"
            );
        }
    }
}

#[test]
fn the_new_files_define_the_same_ids_in_both_languages() {
    for file in ["bookmarks.ftl", "menu.ftl", "status.ftl", "keys.ftl"] {
        let en = ftl_ids("en-US", file);
        let pt = ftl_ids("pt-BR", file);
        assert_eq!(
            en.symmetric_difference(&pt).collect::<Vec<_>>(),
            Vec::<&String>::new(),
            "{file}: ids in only one language"
        );
    }
}

#[test]
fn every_notice_has_its_key_in_both_languages_and_a_distinct_readable_text() {
    let mut seen = BTreeSet::new();
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in all_notices() {
            assert!(i18n.has(notice.message_key()), "{language:?} {notice:?}");
            let t = text(language, notice.clone());
            assert!(!t.trim().is_empty(), "{notice:?}");
            assert!(
                !t.contains("$title") && !t.contains("$count"),
                "unresolved: {t}"
            );
            assert!(!t.contains('{'), "unresolved placeholder in {t:?}");
            seen.insert((language.tag(), t));
        }
    }
    // 9 distinct messages per language (the two `Dropped` forms and the two
    // titled notices differ by their arguments).
    assert!(seen.len() >= 18, "{seen:#?}");
}

#[test]
fn the_notices_with_a_title_name_the_bookmark_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        for notice in [
            BookmarkNotice::BaseUnavailable {
                title: "Najdorf 1972".to_owned(),
            },
            BookmarkNotice::GameNotFound {
                title: "Najdorf 1972".to_owned(),
            },
        ] {
            let t = text(language, notice);
            assert!(t.contains("\"Najdorf 1972\""), "{language:?}: {t}");
        }
    }
}

#[test]
fn a_title_with_fluent_syntax_or_unicode_is_shown_verbatim_and_never_breaks_the_text() {
    for title in [
        "{ $count } and { -term } { NUMBER(1) }",
        "\u{4e2d}\u{6587} \u{1f3c1} \u{e9}\u{e7}",
        "line\nbreak \"quoted\"",
        "",
        &"x".repeat(500),
    ] {
        for language in [Language::EnUs, Language::PtBr] {
            let t = text(
                language,
                BookmarkNotice::BaseUnavailable {
                    title: title.to_owned(),
                },
            );
            assert!(t.contains(title), "{language:?}: {t:?} lacks {title:?}");
        }
    }
}

#[test]
fn the_dropped_notice_pluralizes_and_survives_extreme_counts() {
    let one = text(Language::EnUs, BookmarkNotice::Dropped { count: 1 });
    let three = text(Language::EnUs, BookmarkNotice::Dropped { count: 3 });
    assert!(one.contains("One") && !one.contains('1'), "{one}");
    assert!(three.contains('3'), "{three}");
    let pt_one = text(Language::PtBr, BookmarkNotice::Dropped { count: 1 });
    let pt_three = text(Language::PtBr, BookmarkNotice::Dropped { count: 3 });
    assert_ne!(pt_one, pt_three);
    assert!(pt_three.contains('3'), "{pt_three}");
    for count in [0, usize::MAX] {
        for language in [Language::EnUs, Language::PtBr] {
            let t = text(language, BookmarkNotice::Dropped { count });
            assert!(!t.trim().is_empty(), "{count}");
        }
    }
}

#[test]
fn a_missing_database_names_the_bookmark_in_the_status_bar_and_keeps_the_list() {
    // Acceptance: missing database, notice with the name of the bookmark,
    // list intact (spec "Regras e casos de borda").
    for (pref, language) in [
        (mychess_app::LanguagePref::EnUs, Language::EnUs),
        (mychess_app::LanguagePref::PtBr, Language::PtBr),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut rig = rig_in(dir.path(), None);
        rig.app_msg(mychess_app::Message::Prefs(mychess_app::PrefsMessage::Set(
            mychess_app::PrefChange::Language(pref),
        )));
        let gone = dir.path().join("gone.si4");
        let id = add(&mut rig, None, game("Najdorf 1972", &gone, 1));
        let before = titles(&rig);
        rig.send(mychess_app::Message::Bookmarks(
            mychess_app::BookmarksMessage::Go(id),
        ));
        assert_eq!(
            notice(&rig),
            Some(AppNotice::Bookmark(BookmarkNotice::BaseUnavailable {
                title: "Najdorf 1972".to_owned()
            })),
            "{language:?}"
        );
        assert_eq!(titles(&rig), before, "nothing is removed");
        assert!(
            rig.shell.app.prefs.files.recent.is_empty(),
            "no recent entry for a failure"
        );
        // Let the shell move the notice to the status bar.
        rig.app_msg(mychess_app::Message::Bases(
            mychess_app::BasesMessage::Select(mychess_app::SlotId::CLIPBASE),
        ));
        let expected = app_notice_text(
            &I18n::new(language),
            &AppNotice::Bookmark(BookmarkNotice::BaseUnavailable {
                title: "Najdorf 1972".to_owned(),
            }),
        );
        assert!(plain(&expected).contains("Najdorf 1972"));
        assert!(
            rig.sim(SIZE).find(expected.as_str()).is_ok(),
            "{expected:?}"
        );
    }
}

#[test]
fn the_menu_labels_come_out_translated_in_portuguese() {
    let pt = I18n::new(Language::PtBr);
    assert_eq!(pt.tr("menu-file-read-only"), "Somente leitura");
    assert_eq!(pt.tr("menu-file-open-recent-clear"), "Limpar lista");
    assert_eq!(pt.tr("menu-file-bookmarks"), "Marcadores");
    let en = I18n::new(Language::EnUs);
    assert_eq!(en.tr("menu-file-read-only"), "Read-only");
    assert_eq!(en.tr("menu-file-bookmarks-empty"), "(empty)");
}
