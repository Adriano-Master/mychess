//! The "Mask" menu of the tree panel and the line that shows the job that
//! runs (spec M5-4c, D1830, D1826).

use fluent_bundle::FluentArgs;
use iced::widget::{button, container, progress_bar, row, text};
use iced::{Alignment, Element, Length};
use iced_aw::menu::{Item, MenuBar};
use mychess_app::{FillSide, MaskBusy, MaskJobKind, MaskMessage};

use super::cells::MaskCtx;
use super::mask_message;
use super::state::MaskUiMessage;
use crate::i18n::I18n;
use crate::keymap::Platform;
use crate::shell::Message;
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::menu_bar::{Label, MenuEntry, build_menu, recent_labels};

fn ui(message: MaskUiMessage) -> Message {
    Message::MaskUi(message)
}

fn item(key: &'static str, action: Option<Message>) -> MenuEntry {
    MenuEntry::Item {
        label: Label::Key(key),
        shortcut: None,
        action,
        checked: None,
        icon: None,
    }
}

fn check(key: &'static str, on: bool, message: MaskMessage) -> MenuEntry {
    MenuEntry::Item {
        label: Label::Key(key),
        shortcut: None,
        action: Some(mask_message(message)),
        checked: Some(on),
        icon: None,
    }
}

/// The entries of the menu for `ctx` (D1830): what needs a mask is
/// disabled without one. Outside standard chess (D1835) the application
/// refuses what reads or changes the position (the fills and the outline);
/// the files, the search and the options keep working (D1875).
pub fn entries(ctx: &MaskCtx) -> Vec<MenuEntry> {
    let on = ctx.enabled;
    let with_mask = |message: Message| ctx.open.then_some(message);
    let with_standard_mask = |message: Message| (on && ctx.open).then_some(message);
    let plain = |message: MaskMessage| Some(mask_message(message));
    let names = recent_labels(&ctx.recent);
    let mut recent: Vec<MenuEntry> = ctx
        .recent
        .iter()
        .zip(&names)
        .map(|(path, name)| MenuEntry::Item {
            label: Label::Text(name.clone()),
            shortcut: None,
            action: plain(MaskMessage::Open(Some(path.clone()))),
            checked: None,
            icon: None,
        })
        .collect();
    let recent_entry = if recent.is_empty() {
        item("tree-mask-open-recent", None)
    } else {
        let forget = ctx
            .recent
            .iter()
            .zip(&names)
            .map(|(path, name)| MenuEntry::Item {
                label: Label::Text(name.clone()),
                shortcut: None,
                action: Some(mask_message(MaskMessage::RemoveRecent(path.clone()))),
                checked: None,
                icon: None,
            })
            .collect();
        recent.push(MenuEntry::Separator);
        recent.push(MenuEntry::Submenu {
            label: Label::Key("tree-mask-remove-recent"),
            entries: forget,
            icon: None,
        });
        MenuEntry::Submenu {
            label: Label::Key("tree-mask-open-recent"),
            entries: recent,
            icon: None,
        }
    };
    vec![
        item("tree-mask-new", plain(MaskMessage::New)),
        item("tree-mask-open", plain(MaskMessage::Open(None))),
        recent_entry,
        item("tree-mask-save", with_mask(mask_message(MaskMessage::Save))),
        item(
            "tree-mask-save-as",
            with_mask(mask_message(MaskMessage::SaveAs)),
        ),
        item(
            "tree-mask-close",
            with_mask(mask_message(MaskMessage::Close)),
        ),
        MenuEntry::Separator,
        item(
            "tree-mask-display",
            with_standard_mask(mask_message(MaskMessage::BuildOutline)),
        ),
        item("tree-mask-search", with_mask(ui(MaskUiMessage::OpenSearch))),
        MenuEntry::Separator,
        item(
            "tree-mask-fill-line",
            with_standard_mask(ui(MaskUiMessage::OpenFillLine)),
        ),
        item(
            "tree-mask-fill-game",
            with_standard_mask(mask_message(MaskMessage::FillGame)),
        ),
        item(
            "tree-mask-fill-filter",
            with_standard_mask(mask_message(MaskMessage::FillFilter(FillSide::All))),
        ),
        MenuEntry::Separator,
        check(
            "tree-mask-show-markers",
            ctx.show_markers,
            MaskMessage::SetShowMarkers(!ctx.show_markers),
        ),
        check(
            "tree-mask-show-tooltips",
            ctx.tooltips,
            MaskMessage::SetTooltips(!ctx.tooltips),
        ),
        check(
            "tree-mask-auto-load",
            ctx.auto_load,
            MaskMessage::SetAutoLoad(!ctx.auto_load),
        ),
    ]
}

/// The "Mask" entry of the toolbar of the panel: a button-like title that
/// opens the dropdown.
pub fn bar<'a>(ctx: &MaskCtx, i18n: &I18n, platform: Platform) -> Element<'a, Message> {
    let title = container(
        row![
            icon(Icon::Database, 14.0, IconTone::Text),
            text(i18n.tr("tree-mask-menu")).size(12),
            icon(Icon::SortDown, 12.0, IconTone::Muted),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding([3, 8])
    .style(|theme| {
        let t = theme::Tokens::of(theme);
        container::Style {
            border: iced::Border {
                color: t.border,
                width: 1.0,
                radius: t.radius.control.into(),
            },
            ..container::Style::default()
        }
    });
    let menu = build_menu(entries(ctx), i18n, platform);
    crate::widgets::menu_guard::guard(MenuBar::new(vec![Item::with_menu(title, menu)]))
}

/// The text of the job that runs.
pub fn busy_text(i18n: &I18n, busy: &MaskBusy) -> String {
    if busy.cancelling {
        return i18n.tr("tree-mask-stopping");
    }
    let mut args = FluentArgs::new();
    let key = match busy.kind {
        MaskJobKind::Open => "tree-mask-progress-open",
        MaskJobKind::Save => "tree-mask-progress-save",
        MaskJobKind::Search => "tree-mask-progress-search",
        MaskJobKind::Outline => "tree-mask-progress-outline",
        _ => {
            args.set("done", i64::try_from(busy.done).unwrap_or(i64::MAX));
            if busy.total > 0 {
                args.set("total", i64::try_from(busy.total).unwrap_or(i64::MAX));
                "tree-mask-progress-fill"
            } else {
                "tree-mask-progress-fill-unknown"
            }
        }
    };
    i18n.tr_args(key, &args)
}

/// The fraction of a fill that is done (`None` while the total is unknown).
pub fn busy_fraction(busy: &MaskBusy) -> Option<f32> {
    if busy.kind != MaskJobKind::Fill || busy.total == 0 {
        return None;
    }
    // Game counts of a database: well inside what `f32` tells apart for a
    // progress bar.
    #[allow(clippy::cast_precision_loss)]
    let fraction = busy.done as f32 / busy.total as f32;
    Some(fraction.clamp(0.0, 1.0))
}

/// The line with the job that runs: the text, the bar (when the end is
/// known) and "Stop" for a fill (D1826).
pub fn progress<'a>(busy: &MaskBusy, i18n: &I18n) -> Element<'a, Message> {
    let mut line = row![text(busy_text(i18n, busy)).size(12)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(fraction) = busy_fraction(busy) {
        line = line.push(
            container(progress_bar(0.0..=1.0, fraction))
                .width(Length::Fixed(160.0))
                .height(Length::Fixed(8.0)),
        );
    }
    if busy.kind == MaskJobKind::Fill && !busy.cancelling {
        line = line.push(
            button(text(i18n.tr("tree-mask-stop")).size(12))
                .padding([2, 8])
                .style(theme::command_button(ButtonLook::Secondary, false))
                .on_press(mask_message(MaskMessage::CancelFill)),
        );
    }
    line.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[MenuEntry]) -> Vec<(String, bool)> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item { label, action, .. } => Some((
                    match label {
                        Label::Key(key) => (*key).to_owned(),
                        Label::Text(text) => text.clone(),
                        _ => String::new(),
                    },
                    action.is_some(),
                )),
                MenuEntry::Submenu { label, entries, .. } => Some((
                    match label {
                        Label::Key(key) => (*key).to_owned(),
                        _ => String::new(),
                    },
                    !entries.is_empty(),
                )),
                _ => None,
            })
            .collect()
    }

    fn enabled(entries: &[MenuEntry], key: &str) -> bool {
        labels(entries)
            .into_iter()
            .find(|(k, _)| k == key)
            .is_some_and(|(_, on)| on)
    }

    #[test]
    fn without_a_mask_only_the_file_and_option_items_work() {
        let ctx = MaskCtx {
            enabled: true,
            show_markers: true,
            ..MaskCtx::default()
        };
        let menu = entries(&ctx);
        assert!(enabled(&menu, "tree-mask-new"));
        assert!(enabled(&menu, "tree-mask-open"));
        assert!(!enabled(&menu, "tree-mask-open-recent"), "no recent file");
        for key in [
            "tree-mask-save",
            "tree-mask-save-as",
            "tree-mask-close",
            "tree-mask-display",
            "tree-mask-search",
            "tree-mask-fill-line",
            "tree-mask-fill-game",
            "tree-mask-fill-filter",
        ] {
            assert!(!enabled(&menu, key), "{key}");
        }
        assert!(enabled(&menu, "tree-mask-show-markers"));
    }

    #[test]
    fn with_a_mask_everything_works_and_chess960_keeps_only_files_search_and_options() {
        let open = MaskCtx {
            open: true,
            enabled: true,
            recent: vec!["/m/a.stm".into()],
            ..MaskCtx::default()
        };
        let menu = entries(&open);
        for (key, on) in labels(&menu) {
            assert!(on, "{key} should be enabled");
        }
        let variant = MaskCtx {
            enabled: false,
            ..open
        };
        let menu = entries(&variant);
        for key in [
            "tree-mask-display",
            "tree-mask-fill-line",
            "tree-mask-fill-game",
            "tree-mask-fill-filter",
        ] {
            assert!(!enabled(&menu, key), "{key} needs a standard game");
        }
        // The files, the search and the options are not about the game.
        for key in [
            "tree-mask-new",
            "tree-mask-save",
            "tree-mask-close",
            "tree-mask-search",
            "tree-mask-auto-load",
        ] {
            assert!(enabled(&menu, key), "{key}");
        }
    }

    #[test]
    fn the_fraction_is_known_only_for_a_fill_with_a_total() {
        let mut busy = MaskBusy {
            kind: MaskJobKind::Fill,
            done: 25,
            total: 100,
            cancelling: false,
        };
        assert_eq!(busy_fraction(&busy), Some(0.25));
        busy.total = 0;
        assert_eq!(busy_fraction(&busy), None);
        busy.kind = MaskJobKind::Open;
        busy.total = 10;
        assert_eq!(busy_fraction(&busy), None);
    }
}
