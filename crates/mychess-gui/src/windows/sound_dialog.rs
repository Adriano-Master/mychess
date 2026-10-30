//! The "Sounds" window: the sounds of the moves and their announcement (spec
//! M8-11, D2814).
//!
//! Every control changes the preferences at once (no draft): the window sends
//! the whole `[sound]` table with the one field that changed.

use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, pick_list, row, scrollable, slider, text, text_input};
use iced::{Alignment, Element, Length, Padding};
use mychess_app::sound::FolderStatus;
use mychess_app::sound::{
    AnnounceLang, PackError, PackId, PackInfo, PackOrigin, PackStatus, SoundMessage, SoundPrefs,
    announce_text,
};
use mychess_app::{AppState, Message as AppMessage};

use crate::i18n::{I18n, Language};
use crate::shell::Message;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::check;
use crate::windows::header_dialog::Labeled;

/// Id of the box.
pub const SOUND_BOX_ID: &str = "sound-box";

/// An event of the window that the application does not handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundUiMessage {
    /// "Choose..." for the sounds folder.
    PickFolder,
    /// The answer of the folder dialog (`None`: cancelled).
    FolderPicked(Option<PathBuf>),
    /// Typing in the read-only box that shows the sounds folder (it only
    /// exists so that the path can be selected and copied).
    PathTyped,
}

/// The application message "Close".
pub fn cancel_message() -> AppMessage {
    AppMessage::Sound(SoundMessage::Close)
}

fn set(prefs: SoundPrefs) -> Message {
    Message::App(AppMessage::Sound(SoundMessage::Set(Box::new(prefs))))
}

fn language(i18n: &I18n) -> AnnounceLang {
    match i18n.language() {
        Language::EnUs => AnnounceLang::EnUs,
        Language::PtBr => AnnounceLang::PtBr,
    }
}

/// The line about the folder.
fn folder_status(app: &AppState, i18n: &I18n) -> String {
    match app.sound_folder_status() {
        FolderStatus::None => i18n.tr("sound-folder-none"),
        FolderStatus::Loading => i18n.tr("sound-folder-loading"),
        FolderStatus::Loaded { found, skipped } => {
            let mut args = FluentArgs::new();
            args.set("found", i64::try_from(*found).unwrap_or(i64::MAX));
            let mut line = i18n.tr_args("sound-folder-found", &args);
            if *skipped > 0 {
                let mut args = FluentArgs::new();
                args.set("skipped", i64::try_from(*skipped).unwrap_or(i64::MAX));
                line.push_str("; ");
                line.push_str(&i18n.tr_args("sound-folder-skipped", &args));
            }
            line
        }
        FolderStatus::Failed(_) => i18n.tr("sound-folder-failed"),
    }
}

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The text a pack has in the list (D2907).
fn pack_label(info: &PackInfo, i18n: &I18n) -> String {
    match info.origin() {
        PackOrigin::Builtin => i18n.tr("sound-pack-default"),
        PackOrigin::Bundled => info.name.clone(),
        PackOrigin::User => {
            let mut args = FluentArgs::new();
            args.set("name", info.name.clone());
            i18n.tr_args("sound-pack-user", &args)
        }
    }
}

/// The error of a pack, from the variant (never the message).
fn pack_error(error: &PackError, i18n: &I18n) -> String {
    i18n.tr(match error {
        PackError::NotFound(_) => "sound-pack-error-missing",
        PackError::InvalidId => "sound-pack-error-invalid",
        PackError::Io(_) => "sound-pack-error-io",
        _ => "sound-pack-error",
    })
}

/// The message of choosing a pack.
fn select_message(id: PackId) -> Message {
    Message::App(AppMessage::Sound(SoundMessage::SelectPack(id)))
}

/// The entries of the list and the one shown as chosen. A chosen pack that
/// is not listed (a user pack before the scan ends) is added to the list.
fn pack_options(
    app: &AppState,
    i18n: &I18n,
    chosen: &PackId,
) -> (Vec<Labeled<PackId>>, Labeled<PackId>) {
    let labeled = |info: &PackInfo| Labeled {
        value: info.id.clone(),
        label: pack_label(info, i18n),
    };
    let packs = app.sound_packs();
    let mut options: Vec<Labeled<PackId>> = packs.iter().map(labeled).collect();
    let current = packs.iter().find(|p| &p.id == chosen).map_or_else(
        || {
            let label = match chosen {
                PackId::BuiltinSynth => i18n.tr("sound-pack-default"),
                PackId::Bundled(name) | PackId::User(name) => name.clone(),
            };
            let current = Labeled {
                value: chosen.clone(),
                label,
            };
            options.push(current.clone());
            current
        },
        labeled,
    );
    (options, current)
}

/// The chooser of the pack and what is known about the one chosen.
fn pack_section<'a>(app: &AppState, i18n: &I18n, chosen: &PackId) -> Element<'a, Message> {
    let (options, current) = pack_options(app, i18n, chosen);
    let info = app.sound_packs().iter().find(|p| &p.id == chosen);
    let mut section = column![
        text(i18n.tr("sound-pack")).size(13),
        pick_list(options, Some(current), |choice: Labeled<PackId>| {
            select_message(choice.value)
        })
        .text_size(13)
        .padding(4),
    ]
    .spacing(4);
    if let Some(info) = info.filter(|i| i.origin() != PackOrigin::Builtin) {
        if !info.author.is_empty() {
            let mut args = FluentArgs::new();
            args.set("author", info.author.clone());
            section = section.push(text(i18n.tr_args("sound-pack-author", &args)).size(12));
        }
        if !info.license.is_empty() {
            let mut args = FluentArgs::new();
            args.set("license", info.license.clone());
            section = section.push(text(i18n.tr_args("sound-pack-license", &args)).size(12));
        }
        if !info.description.is_empty() {
            section = section.push(text(info.description.clone()).size(12));
        }
        let mut args = FluentArgs::new();
        args.set("count", count(info.covered));
        args.set("total", count(PackInfo::TOTAL));
        let key = if info.covered >= PackInfo::TOTAL {
            "sound-pack-complete"
        } else {
            "sound-pack-covered"
        };
        section = section.push(text(i18n.tr_args(key, &args)).size(12));
    }
    match app.sound_pack_status() {
        PackStatus::Loading => {
            section = section.push(text(i18n.tr("sound-pack-loading")).size(12));
        }
        PackStatus::Failed(error) => {
            section = section.push(text(pack_error(error, i18n)).size(12));
        }
        _ => {}
    }
    // Remarks found while listing and, once loaded, while decoding.
    let warnings = app
        .sound_loaded_pack()
        .filter(|p| &p.info().id == chosen)
        .map_or_else(
            || info.map_or(0, |i| i.warnings.len()),
            |p| p.info().warnings.len(),
        );
    if warnings > 0 {
        let mut args = FluentArgs::new();
        args.set("count", count(warnings));
        section = section.push(text(i18n.tr_args("sound-pack-warnings", &args)).size(12));
    }
    let ignored = app.sound_packs_warnings().len();
    if ignored > 0 {
        let mut args = FluentArgs::new();
        args.set("count", count(ignored));
        section = section.push(text(i18n.tr_args("sound-packs-ignored", &args)).size(12));
    }
    section = section.push(
        button(text(i18n.tr("sound-open-folder")).size(13))
            .on_press(Message::App(AppMessage::Sound(
                SoundMessage::OpenUserFolderRequested,
            )))
            .style(button::secondary),
    );
    if let Some(folder) = app.sound_user_folder() {
        // The path to copy the packs to, in case the file manager did not
        // open (or there is none).
        section = section
            .push(text(i18n.tr("sound-user-folder-hint")).size(12))
            .push(
                text_input("", &folder.display().to_string())
                    .on_input(|_| Message::SoundUi(SoundUiMessage::PathTyped))
                    .size(12),
            );
    }
    section.into()
}

/// The box.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Element<'a, Message> {
    let prefs = app.sound_prefs().clone();
    let on = |label: &str, value: bool, apply: fn(&mut SoundPrefs, bool)| {
        let base = prefs.clone();
        check(i18n.tr(label), value, true, move |v| {
            let mut next = base.clone();
            apply(&mut next, v);
            set(next)
        })
    };
    let volume = {
        let base = prefs.clone();
        row![
            text(i18n.tr("sound-volume")).size(13),
            slider(0..=100u8, prefs.volume, move |v| {
                let mut next = base.clone();
                next.volume = v;
                set(next)
            })
            .width(Length::Fixed(180.0)),
            text(format!("{}%", prefs.volume)).size(13),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };
    let folder_text = if prefs.folder.is_empty() {
        i18n.tr("sound-folder-builtin")
    } else {
        prefs.folder.clone()
    };
    let remove = {
        let mut next = prefs.clone();
        next.folder.clear();
        button(text(i18n.tr("sound-folder-remove")).size(13))
            .on_press_maybe((!prefs.folder.is_empty()).then(|| set(next)))
            .style(button::secondary)
    };
    let folder = column![
        text(i18n.tr("sound-folder")).size(13),
        text(folder_text).size(12),
        row![
            button(text(i18n.tr("sound-folder-choose")).size(13))
                .on_press(Message::SoundUi(SoundUiMessage::PickFolder))
                .style(button::secondary),
            remove,
        ]
        .spacing(6),
        text(folder_status(app, i18n)).size(12),
        text(i18n.tr("sound-folder-help")).size(12),
    ]
    .spacing(4);
    let advanced = column![
        text(i18n.tr("sound-advanced")).size(14),
        on("sound-announce", prefs.announce, |p, v| p.announce = v),
        folder,
    ]
    .spacing(8);
    let mut body = column![
        pack_section(app, i18n, &prefs.pack),
        on("sound-enabled", prefs.enabled, |p, v| p.enabled = v),
        volume,
        on("sound-on-new", prefs.on_new_move, |p, v| p.on_new_move = v),
        on("sound-on-forward", prefs.on_forward, |p, v| p.on_forward =
            v),
        on("sound-on-back", prefs.on_back, |p, v| p.on_back = v),
        on("sound-tick-only", prefs.tick_only, |p, v| p.tick_only = v),
    ]
    .spacing(8);
    if let Some(san) = app.sound_last_announcement() {
        let mut args = FluentArgs::new();
        args.set("text", announce_text(san, language(i18n)));
        body = body.push(text(i18n.tr_args("sound-last", &args)).size(12));
    }
    body = body.push(advanced);
    modal(Card {
        id: SOUND_BOX_ID,
        title: i18n.tr("sound-title"),
        body: scrollable(body.padding(Padding {
            right: 12.0,
            ..Padding::ZERO
        }))
        .into(),
        buttons: vec![
            button(text(i18n.tr("sound-test")))
                .on_press(Message::App(AppMessage::Sound(SoundMessage::Test)))
                .style(button::secondary)
                .into(),
            button(text(i18n.tr("sound-close")))
                .on_press(Message::App(cancel_message()))
                .into(),
        ],
        width: 440.0,
    })
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;

    use super::*;

    fn sim(app: &AppState, language: Language) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(700.0, 600.0),
            view(app, &I18n::new(language)),
        )
    }

    /// The messages as `Debug` text: the shell `Message` is not `PartialEq`.
    fn sent(sim: Simulator<'static, Message>) -> Vec<String> {
        sim.into_messages().map(|m| format!("{m:?}")).collect()
    }

    fn shown(message: &Message) -> String {
        format!("{message:?}")
    }

    #[test]
    fn the_switch_sends_the_whole_table_with_the_change() {
        let app = AppState::default();
        let mut s = sim(&app, Language::EnUs);
        s.click("Play sounds").expect("switch");
        let want = SoundPrefs {
            enabled: true,
            ..SoundPrefs::default()
        };
        assert_eq!(sent(s), [shown(&set(want))]);
    }

    #[test]
    fn test_and_close_send_their_messages() {
        let app = AppState::default();
        let mut s = sim(&app, Language::EnUs);
        s.click("Test").expect("test");
        assert_eq!(
            sent(s),
            [shown(&Message::App(AppMessage::Sound(SoundMessage::Test)))]
        );
        let mut s = sim(&app, Language::EnUs);
        s.click("Close").expect("close");
        assert_eq!(sent(s), [shown(&Message::App(cancel_message()))]);
    }

    #[test]
    fn choose_asks_for_the_folder_and_remove_is_off_without_one() {
        let app = AppState::default();
        let mut s = sim(&app, Language::EnUs);
        s.click("Choose...").expect("choose");
        assert_eq!(
            sent(s),
            [shown(&Message::SoundUi(SoundUiMessage::PickFolder))]
        );
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find("Built-in sounds only").is_ok());
        s.click("Remove").expect("remove");
        assert_eq!(sent(s), Vec::<String>::new());
    }

    use mychess_app::sound::{PackError, SoundEffect};
    use mychess_app::{Effect, update};

    /// The state after choosing `id`, with the request of its load.
    fn choose(app: &mut AppState, id: PackId) -> mychess_app::RequestId {
        update(app, AppMessage::Sound(SoundMessage::SelectPack(id)))
            .into_iter()
            .find_map(|e| match e {
                Effect::Sound(SoundEffect::LoadPack { request, .. }) => Some(request),
                _ => None,
            })
            .expect("a load request")
    }

    #[test]
    fn the_list_shows_the_packs_and_choosing_sends_the_id() {
        let app = AppState::default();
        let i18n = I18n::new(Language::EnUs);
        let (options, current) = pack_options(&app, &i18n, &PackId::BuiltinSynth);
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels[0], "Default (synthesized)");
        assert!(labels.contains(&"Lichess SFX"), "{labels:?}");
        assert_eq!(current.value, PackId::BuiltinSynth);
        let sfx = options
            .iter()
            .find(|o| o.label == "Lichess SFX")
            .expect("listed");
        assert_eq!(
            shown(&select_message(sfx.value.clone())),
            shown(&Message::App(AppMessage::Sound(SoundMessage::SelectPack(
                PackId::Bundled("sfx".into())
            ))))
        );
        // The picker is on the screen, with the chosen entry.
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find("Sound pack").is_ok());
    }

    #[test]
    fn a_user_pack_is_marked_and_a_missing_choice_is_still_listed() {
        let mut app = AppState::default();
        let user = PackId::User("mine".into());
        choose(&mut app, user.clone());
        let (options, current) = pack_options(&app, &I18n::new(Language::PtBr), &user);
        assert_eq!(current.label, "mine");
        assert!(options.iter().any(|o| o.value == user));
        let info = PackInfo {
            id: user,
            name: "Mine".into(),
            author: String::new(),
            license: String::new(),
            description: String::new(),
            covered: 3,
            warnings: Vec::new(),
        };
        assert_eq!(
            pack_label(&info, &I18n::new(Language::PtBr)),
            "Mine (do usuário)"
        );
    }

    #[test]
    fn the_chosen_pack_shows_author_license_and_coverage() {
        let mut app = AppState::default();
        choose(&mut app, PackId::Bundled("sfx".into()));
        let info = app
            .sound_packs()
            .iter()
            .find(|p| p.id == PackId::Bundled("sfx".into()))
            .expect("listed")
            .clone();
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find(format!("Author: {}", info.author)).is_ok());
        assert!(s.find(format!("License: {}", info.license)).is_ok());
        let line = format!(
            "{} of {} sounds; the rest use the default sound",
            info.covered,
            PackInfo::TOTAL
        );
        assert!(info.covered < PackInfo::TOTAL, "the pack is partial");
        assert!(s.find(line).is_ok());
        // Loading: the status line.
        assert!(s.find("Loading...").is_ok());
        let mut s = sim(&app, Language::PtBr);
        assert!(
            s.find(format!(
                "{} de {} sons; o resto usa o som padrão",
                info.covered,
                PackInfo::TOTAL
            ))
            .is_ok()
        );
        assert!(s.find("Pacote de sons").is_ok());
    }

    #[test]
    fn a_failed_pack_shows_a_translated_error_not_the_message() {
        let mut app = AppState::default();
        let id = PackId::User("gone".into());
        let request = choose(&mut app, id.clone());
        update(
            &mut app,
            AppMessage::Sound(SoundMessage::PackLoaded {
                request,
                id: id.clone(),
                result: Err(PackError::Io(std::io::ErrorKind::PermissionDenied)),
            }),
        );
        assert!(matches!(app.sound_pack_status(), PackStatus::Failed(_)));
        let mut s = sim(&app, Language::EnUs);
        assert!(s.find("The sound pack could not be read.").is_ok());
        let mut s = sim(&app, Language::PtBr);
        assert!(s.find("Não foi possível ler o pacote de sons.").is_ok());
    }

    #[test]
    fn open_folder_sends_its_request_and_the_path_is_shown_once_known() {
        let app = AppState::default();
        let mut s = sim(&app, Language::EnUs);
        s.click("Open sounds folder").expect("button");
        assert_eq!(
            sent(s),
            [shown(&Message::App(AppMessage::Sound(
                SoundMessage::OpenUserFolderRequested
            )))]
        );
        let mut app = AppState::default();
        let dir = PathBuf::from("some-sounds-dir");
        update(
            &mut app,
            AppMessage::Sound(SoundMessage::UserFolderReady(Ok(dir))),
        );
        let mut s = sim(&app, Language::EnUs);
        assert!(
            s.find("Copy the sound packs (one folder each) into this folder:")
                .is_ok()
        );
    }

    #[test]
    fn the_window_speaks_portuguese() {
        let app = AppState::default();
        let mut s = sim(&app, Language::PtBr);
        assert!(s.find("Tocar sons").is_ok());
        assert!(s.find("Fechar").is_ok());
    }
}
