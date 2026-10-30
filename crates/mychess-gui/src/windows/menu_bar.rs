//! The menu bar: a declarative [`MenuSpec`] tree and its rendering with
//! `iced_aw::menu`.
//!
//! [`menu_specs`] is pure (state in, data out), so the menus are tested
//! without opening any overlay; [`view`] only walks the specification.
//! Items whose feature belongs to another slice have `action: None` and are
//! drawn disabled (spec D138).

use iced::widget::{button, container, row, rule, text};
use iced::{Element, Length, Padding};
use iced_aw::menu::{Item, Menu, MenuBar};
use std::path::{Path, PathBuf};

use mychess_app::engines::{EngineMessage, Hotkey};
use mychess_app::{
    Action, AnnotateMessage, AppState, BasesMessage, Binding, BookmarkNode, BookmarksMessage,
    Command, EcoMessage, ImportMessage, ListMessage, Message as AppMessage, Nav, NodeKind,
    NotationLayout, SlotId, StripKind,
};

use mychess_app::{ArrowPrefs, EVAL_SCALE_RANGE, EvalBarPrefs, MenuStylePref, SEQUENCE_LEN_RANGE};

use crate::app_api::{self, ViewChange, ViewOptions};
use crate::i18n::{I18n, LanguagePref};
use crate::keymap::{Platform, binding_label};
use crate::shell::{Message, UiMessage, UiState};
use crate::theme;
use crate::widgets::icon::Icon;
use crate::windows::appearance::AppearanceMessage;
use crate::windows::data_panels::DataPanelsMessage;
use crate::windows::dialog_ui::DialogMessage;
use crate::windows::layout::PaneKind;
use crate::windows::maintenance::{MaintForm, MaintUiMessage};
use crate::windows::notation::{NotationMessage, OptionChange};
use crate::windows::online_ui::{OnlineDialog, OnlineUiMessage};
use crate::windows::tournament_ui::TournamentUiMessage;

/// Animation times offered (Scid `menus.tcl:1195-1198`, plus off).
pub const ANIMATION_CHOICES_MS: [u32; 11] = [0, 100, 150, 200, 250, 300, 400, 500, 600, 800, 1000];

/// How an entry is labeled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// A Fluent key.
    Key(&'static str),
    /// A duration in milliseconds (`menu-view-animation-ms`).
    Milliseconds(u32),
    /// A Fluent key whose text takes a `$number` (`menu-tools-start-engine`).
    Numbered(&'static str, u8),
    /// Text that is not translated (a file name).
    Text(String),
}

/// One entry of a menu.
// Built per frame and consumed at once; no need to box the message.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum MenuEntry {
    /// A command.
    Item {
        /// The text.
        label: Label,
        /// The shortcut shown at the right.
        shortcut: Option<Binding>,
        /// What it does; `None` draws it disabled.
        action: Option<Message>,
        /// `Some` draws a check mark when `true` (checkable and radio items).
        checked: Option<bool>,
        /// The icon of the command (spec M9-2 D1624): drawn by the ribbon,
        /// not by the dropdown. Filled by `icon_for` in [`menu_specs`];
        /// `None` for the entries that inherit it (spec M9-2 D1632).
        icon: Option<Icon>,
    },
    /// A divider line.
    Separator,
    /// The start of a named group (spec M9-2 2.2). The dropdown draws it as a
    /// divider (the title is optional there); the ribbon uses the key as the
    /// title of the group.
    Group(&'static str),
    /// A nested menu.
    Submenu {
        /// The text.
        label: Label,
        /// The nested entries.
        entries: Vec<MenuEntry>,
        /// The icon of the submenu (see [`MenuEntry::Item::icon`]).
        icon: Option<Icon>,
    },
}

/// A top-level menu.
#[derive(Debug, Clone)]
pub struct MenuSpec {
    /// Fluent key of the title.
    pub label_key: &'static str,
    /// The entries.
    pub entries: Vec<MenuEntry>,
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

/// An item whose keyboard shortcut (the first binding of `action` in the
/// user's map) is shown at its right.
fn keyed(
    app: &AppState,
    key: &'static str,
    action: Action,
    message: impl Into<Option<Message>>,
) -> MenuEntry {
    MenuEntry::Item {
        label: Label::Key(key),
        shortcut: app.shortcuts.bindings(action).first().copied(),
        action: message.into(),
        checked: None,
        icon: None,
    }
}

fn command(command: Command) -> Message {
    Message::App(app_api::command(command))
}

fn choice(label: Label, selected: bool, message: Message) -> MenuEntry {
    MenuEntry::Item {
        label,
        shortcut: None,
        action: Some(message),
        checked: Some(selected),
        icon: None,
    }
}

fn toggle(key: &'static str, on: bool, message: impl FnOnce(bool) -> Message) -> MenuEntry {
    choice(Label::Key(key), on, message(!on))
}

fn view_toggle(key: &'static str, on: bool, change: impl FnOnce(bool) -> ViewChange) -> MenuEntry {
    toggle(key, on, |value| Message::Ui(UiMessage::View(change(value))))
}

/// A checkable item with the shortcut of `action` at its right.
fn keyed_toggle(
    app: &AppState,
    key: &'static str,
    action: Action,
    on: bool,
    message: Message,
) -> MenuEntry {
    MenuEntry::Item {
        label: Label::Key(key),
        shortcut: app.shortcuts.bindings(action).first().copied(),
        action: Some(message),
        checked: Some(on),
        icon: None,
    }
}

/// The evaluation bar, the traffic lights and the engine arrows (spec
/// M4-5, D852): the Scid has them in `Options > Score Bar` and in the
/// analysis window; the menu bar here has no Options menu, so they live in
/// View.
fn engine_overlay_entries(app: &AppState) -> Vec<MenuEntry> {
    let prefs = app_api::engine_view_prefs(app);
    let (bar, arrows) = (prefs.eval_bar, prefs.arrows);
    let command = |m: app_api::EvalBarMessage| Message::App(app_api::eval_bar_command(m));
    let scales = (EVAL_SCALE_RANGE.0..=EVAL_SCALE_RANGE.1).map(|n| {
        choice(
            Label::Text(n.to_string()),
            bar.scale == n,
            Message::App(app_api::set_eval_bar(EvalBarPrefs { scale: n, ..bar })),
        )
    });
    let bar_options = vec![
        submenu("menu-options-evalbar-scale", scales.collect()),
        toggle("menu-options-evalbar-log", bar.log, |on| {
            Message::App(app_api::set_eval_bar(EvalBarPrefs { log: on, ..bar }))
        }),
        toggle("menu-options-evalbar-ticks", bar.ticks, |on| {
            Message::App(app_api::set_eval_bar(EvalBarPrefs { ticks: on, ..bar }))
        }),
        toggle("menu-options-evalbar-label", bar.label, |on| {
            Message::App(app_api::set_eval_bar(EvalBarPrefs { label: on, ..bar }))
        }),
    ];
    let modes = [
        ("analysis-arrow-mode-arrows", app_api::ArrowMode::Arrows),
        ("analysis-arrow-mode-sequence", app_api::ArrowMode::Sequence),
    ]
    .map(|(key, mode)| {
        choice(
            Label::Key(key),
            arrows.mode == mode,
            Message::App(app_api::set_arrows(ArrowPrefs { mode, ..arrows })),
        )
    });
    let sides = [
        ("analysis-arrow-seq-both", app_api::SequenceSide::Both),
        ("analysis-arrow-seq-white", app_api::SequenceSide::White),
        ("analysis-arrow-seq-black", app_api::SequenceSide::Black),
    ]
    .map(|(key, side)| {
        choice(
            Label::Key(key),
            arrows.sequence_side == side,
            Message::App(app_api::set_arrows(ArrowPrefs {
                sequence_side: side,
                ..arrows
            })),
        )
    });
    let lengths = (SEQUENCE_LEN_RANGE.0..=SEQUENCE_LEN_RANGE.1).map(|n| {
        choice(
            Label::Text(n.to_string()),
            arrows.sequence_len == n,
            Message::App(app_api::set_arrows(ArrowPrefs {
                sequence_len: n,
                ..arrows
            })),
        )
    });
    let mut arrow_options: Vec<MenuEntry> = modes.into();
    arrow_options.push(MenuEntry::Separator);
    arrow_options.extend(sides);
    arrow_options.push(submenu("analysis-arrow-seq-len", lengths.collect()));
    arrow_options.push(MenuEntry::Separator);
    arrow_options.push(toggle(
        "menu-options-arrows-click",
        arrows.click_to_play,
        |on| {
            Message::App(app_api::set_arrows(ArrowPrefs {
                click_to_play: on,
                ..arrows
            }))
        },
    ));
    vec![
        keyed_toggle(
            app,
            "menu-view-evalbar",
            Action::ToggleEvalBar,
            bar.show,
            command(app_api::EvalBarMessage::ToggleBar),
        ),
        keyed_toggle(
            app,
            "menu-view-traffic-lights",
            Action::ToggleTrafficLights,
            prefs.traffic_lights.show,
            command(app_api::EvalBarMessage::ToggleTrafficLights),
        ),
        keyed_toggle(
            app,
            "menu-view-arrow-numbers",
            Action::ToggleArrowNumbers,
            arrows.number_lines,
            command(app_api::EvalBarMessage::ToggleArrowNumbers),
        ),
        submenu("menu-options-evalbar", bar_options),
        submenu("menu-options-arrows", arrow_options),
    ]
}

fn submenu(key: &'static str, entries: Vec<MenuEntry>) -> MenuEntry {
    MenuEntry::Submenu {
        label: Label::Key(key),
        entries,
        icon: None,
    }
}

/// The labels of the recent files, `n: name` (spec M3-6, D681). A file name
/// that appears more than once gets ` - folder/` (its parent) so the
/// entries can be told apart.
pub(crate) fn recent_labels(files: &[PathBuf]) -> Vec<String> {
    /// How many ancestors a label may show.
    const MAX_LEVELS: usize = 3;
    let name_of = |path: &PathBuf| {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned()
    };
    // The folders above the file, nearest first.
    let ancestors: Vec<Vec<String>> = files
        .iter()
        .map(|path| {
            path.ancestors()
                .skip(1)
                .filter_map(Path::file_name)
                .take(MAX_LEVELS)
                .map(|f| f.to_string_lossy().into_owned())
                .collect()
        })
        .collect();
    let names: Vec<String> = files.iter().map(name_of).collect();
    let key = |index: usize, levels: usize| {
        let up = &ancestors[index];
        (&names[index], &up[..levels.min(up.len())])
    };
    files
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let name = &names[index];
            let repeated = |levels: usize| {
                (0..files.len())
                    .any(|other| other != index && key(other, levels) == key(index, levels))
            };
            let max = ancestors[index].len();
            // The shortest suffix that tells the entry apart (from the
            // parent up); none when the name alone is unique.
            let levels = (names.iter().filter(|n| *n == name).count() > 1)
                .then(|| (1..=max).find(|&levels| !repeated(levels)).unwrap_or(max));
            match levels.filter(|&l| l > 0) {
                Some(levels) => {
                    let up = &ancestors[index][..levels];
                    let folder: Vec<&str> = up.iter().rev().map(String::as_str).collect();
                    format!("{}: {name} \u{2014} {}/", index + 1, folder.join("/"))
                }
                None => format!("{}: {name}", index + 1),
            }
        })
        .collect()
}

fn recent_entries(app: &AppState) -> Vec<MenuEntry> {
    let files = app_api::recent_files(app);
    if files.is_empty() {
        return vec![item("menu-file-open-recent-none", None)];
    }
    let mut entries: Vec<MenuEntry> = recent_labels(&files)
        .into_iter()
        .enumerate()
        .map(|(index, label)| MenuEntry::Item {
            label: Label::Text(label),
            shortcut: u8::try_from(index + 1).ok().and_then(|n| {
                app.shortcuts
                    .bindings(Action::OpenRecent(n))
                    .first()
                    .copied()
            }),
            action: Some(command(Command::OpenRecent(index))),
            checked: None,
            icon: None,
        })
        .collect();
    entries.push(MenuEntry::Separator);
    entries.push(item(
        "menu-file-open-recent-clear",
        Some(command(Command::ClearRecent)),
    ));
    entries
}

/// The entries of one level of the bookmark tree: a folder is a submenu
/// (disabled "(empty)" when it has nothing), a game an item that goes to it
/// (D679).
fn bookmark_tree_entries(nodes: &[BookmarkNode]) -> Vec<MenuEntry> {
    nodes
        .iter()
        .map(|node| match &node.kind {
            NodeKind::Game(game) => MenuEntry::Item {
                label: Label::Text(game.title.clone()),
                shortcut: None,
                action: Some(Message::App(AppMessage::Bookmarks(BookmarksMessage::Go(
                    node.id,
                )))),
                checked: None,
                icon: None,
            },
            NodeKind::Folder(folder) => MenuEntry::Submenu {
                label: Label::Text(folder.title.clone()),
                icon: None,
                entries: if folder.children.is_empty() {
                    vec![item("menu-file-bookmarks-empty", None)]
                } else {
                    bookmark_tree_entries(&folder.children)
                },
            },
        })
        .collect()
}

/// File > Bookmarks: add the current game, manage, then the tree.
fn bookmark_entries(app: &AppState) -> Vec<MenuEntry> {
    let add = app
        .can_bookmark_current()
        .then(|| Message::App(AppMessage::Bookmarks(BookmarksMessage::AddCurrent)));
    let mut entries = vec![
        keyed(app, "menu-file-bookmarks-add", Action::AddBookmark, add),
        keyed(
            app,
            "menu-file-bookmarks-manage",
            Action::ManageBookmarks,
            Some(Message::App(AppMessage::Bookmarks(
                BookmarksMessage::OpenEditor,
            ))),
        ),
    ];
    if !app.bookmarks.is_empty() {
        entries.push(MenuEntry::Separator);
        entries.extend(bookmark_tree_entries(app.bookmarks.roots()));
    }
    entries
}

/// File > Read-only for the current database (D682): off for the Clipbase,
/// checked and off for a database that can never be written.
fn read_only_entry(app: &AppState) -> MenuEntry {
    let id = app.bases.current();
    let slot = app.bases.slot(id);
    let on = slot.is_some_and(|slot| slot.read_only);
    let can_change = !id.is_clipbase() && slot.is_some_and(|slot| !slot.is_locked_read_only());
    MenuEntry::Item {
        label: Label::Key("menu-file-read-only"),
        shortcut: None,
        action: can_change.then(|| {
            bases_message(BasesMessage::List(ListMessage::SetReadOnly {
                slot: id,
                read_only: !on,
            }))
        }),
        checked: Some(on),
        icon: None,
    }
}

fn bases_message(message: BasesMessage) -> Message {
    Message::App(AppMessage::Bases(message))
}

/// The entries of "Databases": the eight slots and the Clipbase, with the
/// current one checked and the empty ones disabled (spec M3-4, D504).
fn database_entries(app: &AppState) -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = SlotId::all()
        .map(|id| {
            let slot = app.bases.slot(id);
            let label = match slot {
                Some(_) if id.is_clipbase() => Label::Key("games-clipbase"),
                Some(slot) => Label::Text(
                    slot.path
                        .as_deref()
                        .and_then(|path| path.file_name())
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
                ),
                None => Label::Text(format!("({})", id.number())),
            };
            let action = u8::try_from(id.number()).ok().map(Action::SelectBase);
            MenuEntry::Item {
                label,
                shortcut: action.and_then(|a| app.shortcuts.bindings(a).first().copied()),
                action: slot.map(|_| bases_message(BasesMessage::Select(id))),
                checked: Some(app.bases.current() == id),
                icon: None,
            }
        })
        .collect();
    entries.push(MenuEntry::Separator);
    entries.push(keyed(
        app,
        "menu-file-next-base",
        Action::NextBase,
        bases_message(BasesMessage::Next),
    ));
    entries.push(keyed(
        app,
        "menu-file-previous-base",
        Action::PreviousBase,
        bases_message(BasesMessage::Previous),
    ));
    entries
}

/// File > EPD (spec M7-5c): open, create, save and the recent files.
fn epd_file_entries(app: &AppState) -> Vec<MenuEntry> {
    use mychess_app::epd_window::EpdMessage;
    let send = |m: EpdMessage| Some(Message::App(AppMessage::Epd(m)));
    let view = app.epd_view();
    let can_open = view.tabs.len() < view.max_files && !view.opening;
    let active = view
        .active
        .and_then(|id| view.tabs.iter().find(|t| t.id == id))
        .or_else(|| view.tabs.first());
    let can_save = active.is_some_and(|t| !t.read_only && !t.busy && t.run.is_none());
    let can_save_as = active.is_some_and(|t| !t.busy && t.run.is_none());
    let mut recent: Vec<MenuEntry> = view
        .recent
        .iter()
        .map(|path| MenuEntry::Item {
            label: Label::Text(path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            )),
            shortcut: None,
            action: can_open
                .then(|| Message::App(AppMessage::Epd(EpdMessage::OpenPath(path.clone())))),
            checked: None,
            icon: None,
        })
        .collect();
    if recent.is_empty() {
        recent.push(item("menu-file-epd-recent-none", None));
    } else {
        recent.push(MenuEntry::Separator);
        recent.push(item(
            "menu-file-epd-recent-clear",
            send(EpdMessage::ClearRecent),
        ));
    }
    vec![
        item(
            "menu-file-epd-open",
            can_open.then(|| Message::App(AppMessage::Epd(EpdMessage::Open))),
        ),
        item(
            "menu-file-epd-new",
            can_open.then(|| Message::App(AppMessage::Epd(EpdMessage::New))),
        ),
        item(
            "menu-file-epd-save",
            can_save.then(|| Message::App(AppMessage::Epd(EpdMessage::Save))),
        ),
        item(
            "menu-file-epd-save-as",
            can_save_as.then(|| Message::App(AppMessage::Epd(EpdMessage::SaveAs))),
        ),
        submenu("menu-file-epd-recent", recent),
    ]
}

#[allow(clippy::too_many_lines)] // one entry per line
fn file_menu(app: &AppState) -> MenuSpec {
    MenuSpec {
        label_key: "menu-file",
        entries: vec![
            MenuEntry::Group("menu-group-file-open"),
            keyed(
                app,
                "menu-file-new",
                Action::NewGame,
                command(Command::NewGame),
            ),
            keyed(
                app,
                "menu-file-open",
                Action::OpenFile,
                command(Command::Open),
            ),
            submenu("menu-file-open-recent", recent_entries(app)),
            submenu("menu-file-bookmarks", bookmark_entries(app)),
            submenu("menu-file-epd", epd_file_entries(app)),
            item(
                if app.bases.current().is_clipbase() {
                    "menu-file-clear-clipbase"
                } else {
                    "menu-file-close-base"
                },
                Some(bases_message(BasesMessage::Close(app.bases.current()))),
            ),
            MenuEntry::Group("menu-group-file-save"),
            keyed(app, "menu-file-save", Action::Save, command(Command::Save)),
            keyed(
                app,
                "menu-file-save-as",
                Action::SaveAs,
                command(Command::SaveAs),
            ),
            keyed(
                app,
                "menu-file-append",
                Action::AppendToPgn,
                command(Command::Append),
            ),
            MenuEntry::Group("menu-group-file-exchange"),
            keyed(
                app,
                "menu-file-import-pgn-file",
                Action::ImportPgnFile,
                Some(Message::App(AppMessage::FileImport(
                    mychess_app::FileImportMessage::Open,
                ))),
            ),
            keyed(
                app,
                "menu-file-export-games",
                Action::ExportGames,
                Some(Message::App(AppMessage::GamesExport(
                    mychess_app::GamesExportMessage::Open,
                ))),
            ),
            // Spec M7-4c: the image of the position shown (no default key).
            item(
                "menu-file-export-image",
                Some(Message::App(AppMessage::ExportDocs(
                    mychess_app::ExportDocsMessage::OpenImage,
                ))),
            ),
            MenuEntry::Separator,
            keyed(
                app,
                "menu-file-quit",
                Action::Quit,
                Some(Message::Ui(UiMessage::CloseRequested)),
            ),
        ],
    }
}

/// Database > Names: the spelling file (the Scid's Options menu); reload,
/// unload and cancel the load (D3499).
fn spelling_file_entries(app: &AppState) -> Vec<MenuEntry> {
    let spelling = app.spelling_status_view();
    vec![
        item(
            "menu-file-load-spelling",
            Some(Message::App(AppMessage::Spelling(
                mychess_app::SpellingMessage::PickFile,
            ))),
        ),
        item(
            "menu-file-reload-spelling",
            (spelling.path.is_some() && spelling.loading.is_none()).then_some(Message::App(
                AppMessage::Spelling(mychess_app::SpellingMessage::Reload),
            )),
        ),
        item(
            "menu-file-unload-spelling",
            (spelling.loaded || spelling.loading.is_some()).then_some(Message::App(
                AppMessage::Spelling(mychess_app::SpellingMessage::Unload),
            )),
        ),
        item(
            "menu-file-cancel-spelling-load",
            spelling
                .loading
                .is_some()
                .then_some(Message::App(AppMessage::Spelling(
                    mychess_app::SpellingMessage::CancelLoad,
                ))),
        ),
    ]
}

/// Spec M9-7: the Database menu, everything that changes or describes the
/// database. Opens the forms of the maintenance window directly.
#[allow(clippy::too_many_lines)] // one entry per line
fn database_menu(app: &AppState) -> MenuSpec {
    let maint = |form: MaintForm| Some(Message::Maint(MaintUiMessage::OpenForm(form)));
    let spell = |kind: mychess_app::NameKind| {
        Some(Message::App(AppMessage::Spelling(
            mychess_app::SpellingMessage::OpenCheck(kind),
        )))
    };
    MenuSpec {
        label_key: "menu-database",
        entries: vec![
            MenuEntry::Group("menu-group-db-open"),
            submenu("menu-file-databases", database_entries(app)),
            read_only_entry(app),
            MenuEntry::Group("menu-group-db-clipbase"),
            item(
                "menu-edit-clipbase-copy",
                (!app.bases.current().is_clipbase())
                    .then(|| bases_message(BasesMessage::ClipbaseCopy)),
            ),
            item(
                "menu-edit-clipbase-paste",
                (!app.bases.current().is_clipbase()
                    && app
                        .bases
                        .slot(SlotId::CLIPBASE)
                        .is_some_and(|slot| slot.total() > 0))
                .then(|| bases_message(BasesMessage::ClipbasePaste)),
            ),
            item(
                "menu-edit-clipbase-clear",
                Some(bases_message(BasesMessage::ClipbaseClear)),
            ),
            MenuEntry::Group("menu-group-db-maintenance"),
            // Spec M7-1c: the maintenance window and the repair.
            item(
                "menu-tools-maintenance",
                Some(Message::Maint(MaintUiMessage::Open)),
            ),
            item("menu-db-compact", maint(MaintForm::Compact)),
            item("menu-db-sort", maint(MaintForm::Sort)),
            item("menu-db-twins", maint(MaintForm::Twins)),
            item("menu-db-cleaner", maint(MaintForm::Cleaner)),
            item("menu-db-check", maint(MaintForm::Check)),
            item("menu-tools-repair-base", maint(MaintForm::Repair)),
            MenuEntry::Group("menu-group-db-properties"),
            item("menu-db-type", maint(MaintForm::BaseType)),
            item("menu-db-description", maint(MaintForm::Description)),
            item("menu-db-flags", maint(MaintForm::CustomFlags)),
            item("menu-db-tags", maint(MaintForm::Tags)),
            MenuEntry::Group("menu-group-db-names"),
            item("menu-tools-name-editor", maint(MaintForm::Names)),
            // Spec M6-3c: the spell-check of the names.
            submenu(
                "menu-stats-spell",
                vec![
                    item(
                        "menu-stats-spell-player",
                        spell(mychess_app::NameKind::Player),
                    ),
                    item(
                        "menu-stats-spell-event",
                        spell(mychess_app::NameKind::Event),
                    ),
                    item("menu-stats-spell-site", spell(mychess_app::NameKind::Site)),
                    item(
                        "menu-stats-spell-round",
                        spell(mychess_app::NameKind::Round),
                    ),
                ],
            ),
            submenu("menu-db-spelling-file", spelling_file_entries(app)),
            MenuEntry::Group("menu-group-db-fill"),
            // Spec M5-7b: the classification of the database.
            item(
                "menu-tools-eco-classify",
                Some(Message::App(AppMessage::Eco(EcoMessage::OpenClassify))),
            ),
            item(
                "menu-stats-add-elo",
                Some(Message::App(AppMessage::Spelling(
                    mychess_app::SpellingMessage::OpenAddElo,
                ))),
            ),
        ],
    }
}

/// Remove comments, variations and moves; each asks for confirmation with
/// the count (the application opens the question).
fn strip_entries(app: &AppState) -> Vec<MenuEntry> {
    let nav = mychess_app::NavState::of(&app.session);
    let cursor = app.session.cursor();
    let strip = |kind| Some(Message::App(app_api::request_strip(app, kind)));
    vec![
        item("menu-edit-strip-comments", strip(StripKind::Comments)),
        item("menu-edit-strip-variations", strip(StripKind::Variations)),
        MenuEntry::Separator,
        item(
            "menu-edit-truncate-start",
            (!nav.at_start).then(|| {
                Message::App(app_api::request_strip(
                    app,
                    StripKind::TruncateBefore(cursor),
                ))
            }),
        ),
        keyed(
            app,
            "menu-edit-truncate-end",
            Action::TruncateGame,
            (!nav.at_end).then(|| {
                Message::App(app_api::request_strip(
                    app,
                    StripKind::TruncateAfter(cursor),
                ))
            }),
        ),
    ]
}

/// Spec M7-6c: the item merges the game selected in the list of the
/// current database; it needs one and a game in the editor that takes it.
fn merge_game_enabled(app: &AppState) -> bool {
    crate::windows::merge_dialog::can_merge(app)
        && app
            .bases
            .slot(app.bases.current())
            .is_some_and(|slot| slot.list.selected.is_some())
}

fn edit_menu(app: &AppState) -> MenuSpec {
    MenuSpec {
        label_key: "menu-edit",
        entries: vec![
            MenuEntry::Group("menu-group-edit-history"),
            keyed(
                app,
                "menu-edit-undo",
                Action::Undo,
                app_api::can_undo(app).then(|| Message::App(app_api::undo())),
            ),
            keyed(
                app,
                "menu-edit-redo",
                Action::Redo,
                app_api::can_redo(app).then(|| Message::App(app_api::redo())),
            ),
            MenuEntry::Group("menu-group-edit-clipboard"),
            keyed(
                app,
                "menu-edit-copy-fen",
                Action::CopyFen,
                command(Command::CopyFen),
            ),
            keyed(
                app,
                "menu-edit-copy-pgn",
                Action::CopyPgn,
                command(Command::CopyPgn),
            ),
            keyed(
                app,
                "menu-edit-paste-fen",
                Action::PasteFen,
                command(Command::PasteFen),
            ),
            keyed(
                app,
                "menu-edit-import-pgn",
                Action::ImportPgn,
                Message::App(AppMessage::Import(ImportMessage::Open)),
            ),
            MenuEntry::Group("menu-group-edit-game"),
            keyed(
                app,
                "menu-edit-setup-board",
                Action::SetupBoard,
                command(Command::SetupBoard),
            ),
            keyed(
                app,
                "menu-edit-merge-game",
                Action::MergeGame,
                merge_game_enabled(app).then(|| Message::Ui(UiMessage::Action(Action::MergeGame))),
            ),
            submenu("menu-edit-strip", strip_entries(app)),
        ],
    }
}

fn game_menu(app: &AppState) -> MenuSpec {
    let nav = |nav| Some(Message::App(app_api::navigate(nav)));
    let in_file = app_api::has_collection(app);
    let file_game = |which: Command| in_file.then(|| command(which));
    MenuSpec {
        label_key: "menu-game",
        entries: vec![
            keyed(app, "menu-game-start", Action::GoStart, nav(Nav::Start)),
            keyed(app, "menu-game-previous", Action::Back, nav(Nav::Back(1))),
            keyed(app, "menu-game-next", Action::Forward, nav(Nav::Forward(1))),
            keyed(app, "menu-game-end", Action::GoEnd, nav(Nav::GameEnd)),
            MenuEntry::Separator,
            keyed(
                app,
                "menu-game-first-game",
                Action::FirstGame,
                file_game(Command::FirstGame),
            ),
            keyed(
                app,
                "menu-game-prev-game",
                Action::PrevGame,
                file_game(Command::PrevGame),
            ),
            keyed(
                app,
                "menu-game-next-game",
                Action::NextGame,
                file_game(Command::NextGame),
            ),
            keyed(
                app,
                "menu-game-last-game",
                Action::LastGame,
                file_game(Command::LastGame),
            ),
            MenuEntry::Separator,
            keyed(
                app,
                "menu-game-flip",
                Action::FlipBoard,
                Some(Message::Ui(UiMessage::FlipBoard)),
            ),
            MenuEntry::Separator,
            keyed(
                app,
                "menu-game-goto-move",
                Action::GotoMove,
                Some(Message::Dialog(DialogMessage::OpenGoto)),
            ),
            item(
                "menu-game-info",
                Some(Message::Dialog(DialogMessage::OpenHeader(None))),
            ),
            MenuEntry::Separator,
            // Spec M5-7b: the deepest position of the ECO file and the
            // first move that no game of a database has played.
            item(
                "menu-game-identify-opening",
                Some(Message::App(AppMessage::Eco(EcoMessage::Identify))),
            ),
            item(
                "menu-game-novelty",
                Some(Message::App(AppMessage::Eco(EcoMessage::OpenNovelty))),
            ),
            MenuEntry::Separator,
            // Spec M8-12: the game of the user that is on, in any mode.
            keyed(
                app,
                "menu-game-resign",
                Action::ResignGame,
                app.resign_mode()
                    .map(|_| Message::Ui(UiMessage::Action(Action::ResignGame))),
            ),
            keyed(
                app,
                "menu-game-rematch",
                Action::Rematch,
                app.rematch_mode()
                    .map(|_| Message::Ui(UiMessage::Action(Action::Rematch))),
            ),
        ],
    }
}

#[allow(clippy::too_many_lines)] // a flat declarative table
fn view_menu(app: &AppState, ui: &UiState, options: ViewOptions) -> MenuSpec {
    let look = app_api::appearance(app, options);
    let board = &look.board;
    let animations = ANIMATION_CHOICES_MS.map(|ms| {
        let label = if ms == 0 {
            Label::Key("menu-view-animation-off")
        } else {
            Label::Milliseconds(ms)
        };
        choice(
            label,
            look.animation_ms == ms,
            Message::App(app_api::set_animation_ms(ms)),
        )
    });
    let pane_toggle = |key, kind, open: bool| MenuEntry::Item {
        label: Label::Key(key),
        shortcut: None,
        action: Some(Message::Ui(UiMessage::TogglePane(kind))),
        checked: Some(open),
        icon: None,
    };
    let language = app_api::language_pref(app);
    let languages = [
        ("menu-view-language-system", LanguagePref::System),
        ("menu-view-language-en-us", LanguagePref::EnUs),
        ("menu-view-language-pt-br", LanguagePref::PtBr),
    ]
    .map(|(key, lang)| {
        choice(
            Label::Key(key),
            language == lang,
            Message::App(app_api::set_language(lang)),
        )
    });
    let menu_style = app_api::menu_style(app);
    let menu_styles = [
        ("menu-style-dropdown", MenuStylePref::Dropdown),
        ("menu-style-ribbon", MenuStylePref::Ribbon),
    ]
    .map(|(key, style)| {
        choice(
            Label::Key(key),
            menu_style == style,
            Message::App(app_api::set_menu_style(style)),
        )
    });
    let notation_options = app.notation_options();
    let layouts = [
        ("notation-opt-layout-table", NotationLayout::Table),
        ("notation-opt-layout-indented", NotationLayout::Indented),
        ("notation-opt-layout-inline", NotationLayout::Inline),
    ]
    .map(|(key, layout)| {
        choice(
            Label::Key(key),
            notation_options.layout == layout,
            Message::Notation(NotationMessage::Option(OptionChange::Layout(layout))),
        )
    });
    let mut notation_entries: Vec<MenuEntry> = layouts.into();
    notation_entries.push(MenuEntry::Separator);
    notation_entries.push(toggle(
        "notation-opt-figurines",
        notation_options.figurines,
        |on| Message::Notation(NotationMessage::Option(OptionChange::Figurines(on))),
    ));
    notation_entries.push(toggle(
        "notation-opt-time-column",
        notation_options.show_time_column,
        |on| Message::Notation(NotationMessage::Option(OptionChange::TimeColumn(on))),
    ));
    MenuSpec {
        label_key: "menu-view",
        entries: vec![
            // The appearance (theme, accent, board, pieces, coordinates,
            // material, player strips) is one window (spec M9-3, D1700).
            item(
                "menu-view-theme",
                Some(Message::Dialog(DialogMessage::Appearance(
                    AppearanceMessage::Open,
                ))),
            ),
            submenu("menu-view-notation", notation_entries),
            item(
                "menu-view-sounds",
                Some(Message::App(mychess_app::Message::Sound(
                    mychess_app::sound::SoundMessage::Open,
                ))),
            ),
            MenuEntry::Separator,
            toggle("menu-view-side-to-move", look.show_side_to_move, |on| {
                Message::App(app_api::set_side_to_move(on))
            }),
            toggle("menu-view-last-move", board.highlight_last_move, |on| {
                Message::App(app_api::set_highlight_last_move(on))
            }),
            view_toggle(
                "menu-view-legal-moves",
                board.show_legal_hints,
                ViewChange::LegalHints,
            ),
            view_toggle("menu-view-marks", board.show_marks, ViewChange::ShowMarks),
            submenu("menu-view-animation", animations.into()),
            MenuEntry::Separator,
        ]
        .into_iter()
        .chain(engine_overlay_entries(app))
        .chain([
            MenuEntry::Separator,
            submenu(
                "menu-view-panes",
                vec![
                    pane_toggle(
                        "menu-view-pane-notation",
                        PaneKind::Notation,
                        ui.is_open(PaneKind::Notation),
                    ),
                    pane_toggle(
                        "menu-view-pane-info",
                        PaneKind::Info,
                        ui.is_open(PaneKind::Info),
                    ),
                    pane_toggle(
                        "menu-view-pane-comment",
                        PaneKind::Comment,
                        ui.is_open(PaneKind::Comment),
                    ),
                    pane_toggle(
                        "menu-view-pane-analysis",
                        PaneKind::Analysis,
                        ui.is_open(PaneKind::Analysis),
                    ),
                    pane_toggle(
                        "menu-view-pane-graph",
                        PaneKind::Graph,
                        ui.is_open(PaneKind::Graph),
                    ),
                    pane_toggle(
                        "menu-view-pane-tree",
                        PaneKind::Tree,
                        ui.is_open(PaneKind::Tree),
                    ),
                    pane_toggle(
                        "menu-view-pane-book",
                        PaneKind::Book,
                        ui.is_open(PaneKind::Book),
                    ),
                    MenuEntry::Item {
                        label: Label::Key("menu-view-pane-games"),
                        shortcut: app
                            .shortcuts
                            .bindings(Action::ToggleGameList)
                            .first()
                            .copied(),
                        action: Some(Message::Ui(UiMessage::TogglePane(PaneKind::GameList))),
                        checked: Some(ui.is_open(PaneKind::GameList)),
                        icon: None,
                    },
                ],
            ),
            item(
                "menu-view-reset-layout",
                Some(Message::Ui(UiMessage::ResetLayout)),
            ),
            view_toggle(
                "menu-view-status-bar",
                options.show_status_bar,
                ViewChange::StatusBar,
            ),
            MenuEntry::Separator,
            submenu("menu-view-language", languages.into()),
            submenu("menu-view-menu-style", menu_styles.into()),
        ])
        .collect(),
    }
}

/// The first key bound to `action`, for the menu to show.
fn first_binding(app: &AppState, action: Action) -> Option<Binding> {
    app.shortcuts.bindings(action).first().copied()
}

/// Spec M8-8: the tablebase panel.
fn tablebase_entry(app: &AppState, ui: &UiState) -> MenuEntry {
    MenuEntry::Item {
        label: Label::Key("menu-tools-tablebase"),
        shortcut: first_binding(app, Action::ToggleTablebasePane),
        action: Some(Message::Ui(UiMessage::TogglePane(PaneKind::Tablebase))),
        checked: Some(ui.is_open(PaneKind::Tablebase)),
        icon: None,
    }
}

/// The analysis windows (analysis, score graph, tablebases) and the openings
/// (tree, book) of the tools menu. The ECO browser follows the openings.
fn push_pane_entries(entries: &mut Vec<MenuEntry>, app: &AppState, ui: &UiState) {
    let pane = |key: &'static str, action: Action, kind: PaneKind| MenuEntry::Item {
        label: Label::Key(key),
        shortcut: first_binding(app, action),
        action: Some(Message::Ui(UiMessage::TogglePane(kind))),
        checked: Some(ui.is_open(kind)),
        icon: None,
    };
    entries.push(MenuEntry::Group("menu-group-analysis-windows"));
    entries.push(pane(
        "menu-tools-analysis-window",
        Action::ToggleAnalysisPane,
        PaneKind::Analysis,
    ));
    entries.push(pane(
        "menu-tools-graph",
        Action::ToggleGraphPane,
        PaneKind::Graph,
    ));
    // Spec M8-8: the tablebase panel.
    entries.push(tablebase_entry(app, ui));
    entries.push(MenuEntry::Group("menu-group-openings"));
    // Spec M5-2: the opening tree panel (Command+Shift+T).
    entries.push(pane(
        "menu-tools-tree",
        Action::ToggleTreePane,
        PaneKind::Tree,
    ));
    // Spec M5-6: the book window (two Polyglot books).
    entries.push(pane(
        "menu-tools-book",
        Action::ToggleBookPane,
        PaneKind::Book,
    ));
}

/// Spec M6-4d: the Statistics menu: the crosstable, the statistics and graphs
/// of the filter, the rating graph and the piece tracker. They have no default key (D3395.1);
/// the user may bind one to their toggle actions.
fn stats_menu(app: &AppState, ui: &UiState) -> MenuSpec {
    use mychess_app::charts::StatsTab;
    let mut entries = vec![MenuEntry::Group("menu-group-statistics")];
    let pane = |key: &'static str, action: Action, kind: PaneKind| MenuEntry::Item {
        label: Label::Key(key),
        shortcut: first_binding(app, action),
        action: Some(Message::Ui(UiMessage::TogglePane(kind))),
        checked: Some(ui.is_open(kind)),
        icon: None,
    };
    let graph = |key: &'static str, tab: StatsTab| MenuEntry::Item {
        label: Label::Key(key),
        shortcut: None,
        action: Some(Message::Panels(DataPanelsMessage::OpenStatsTab(tab))),
        checked: Some(
            ui.is_open(PaneKind::FilterStats)
                && app.charts().stats_is_open()
                && app.prefs.filter_graph.tab == tab,
        ),
        icon: None,
    };
    entries.push(pane(
        "menu-stats-crosstable",
        Action::ToggleCrosstablePane,
        PaneKind::Crosstable,
    ));
    entries.push(pane(
        "menu-stats-stats",
        Action::ToggleStatsPane,
        PaneKind::FilterStats,
    ));
    entries.push(graph("menu-stats-graph-relative", StatsTab::Relative));
    entries.push(graph("menu-stats-graph-absolute", StatsTab::Absolute));
    entries.push(pane(
        "menu-stats-rating-graph",
        Action::ToggleRatingPane,
        PaneKind::RatingGraph,
    ));
    entries.push(pane(
        "menu-stats-piece-tracker",
        Action::TogglePieceTrackerPane,
        PaneKind::PieceTracker,
    ));
    push_player_entries(&mut entries, app);
    MenuSpec {
        label_key: "menu-stats",
        entries,
    }
}

/// Spec M6-3c: the finders and the player information (the Scid's Windows
/// menu), then (spec M9-7) the reports about the database.
fn push_player_entries(entries: &mut Vec<MenuEntry>, app: &AppState) {
    let action = |action: Action| Some(Message::Ui(UiMessage::Action(action)));
    entries.push(MenuEntry::Group("menu-group-players"));
    entries.push(keyed(
        app,
        "menu-stats-find-players",
        Action::PlayerFinder,
        action(Action::PlayerFinder),
    ));
    entries.push(keyed(
        app,
        "menu-stats-find-tournaments",
        Action::TournamentFinder,
        action(Action::TournamentFinder),
    ));
    entries.push(keyed(
        app,
        "menu-stats-player-info",
        Action::PlayerInfo,
        action(Action::PlayerInfo),
    ));
    entries.push(MenuEntry::Group("menu-group-reports"));
    // Spec M7-2c: the opening report window (no default key, D3631).
    entries.push(item(
        "menu-tools-opening-report",
        Some(Message::OpRep(
            crate::windows::opening_report::OpRepUiMessage::Open,
        )),
    ));
    // Spec M7-3c: the player report (no default key).
    entries.push(item(
        "menu-tools-player-report",
        Some(Message::PlayerReportUi(
            crate::windows::player_report::PrUiMessage::Open(None),
        )),
    ));
}

fn tools_menu(app: &AppState, ui: &UiState) -> MenuSpec {
    let engine = |message: EngineMessage| Some(Message::App(AppMessage::Engine(message)));
    let mut entries = vec![
        MenuEntry::Group("menu-group-engines"),
        keyed(
            app,
            "menu-tools-engines",
            Action::ManageEngines,
            engine(EngineMessage::OpenDialog),
        ),
    ];
    for number in 1..=3u8 {
        entries.push(MenuEntry::Item {
            label: Label::Numbered("menu-tools-start-engine", number),
            shortcut: app
                .shortcuts
                .bindings(Action::StartEngine(number))
                .first()
                .copied(),
            action: engine(EngineMessage::ToggleHotkey(match number {
                1 => Hotkey::F2,
                2 => Hotkey::F3,
                _ => Hotkey::F4,
            })),
            checked: None,
            icon: None,
        });
    }
    entries.push(keyed(
        app,
        "menu-tools-toggle-analysis",
        Action::ToggleEngineAnalysis,
        engine(EngineMessage::ToggleMainAnalysis),
    ));
    // Spec M8-10c: the electronic board.
    entries.push(keyed(
        app,
        "menu-tools-eboard",
        Action::EboardDialog,
        Some(Message::Ui(UiMessage::Action(Action::EboardDialog))),
    ));
    push_pane_entries(&mut entries, app, ui);
    // Spec M5-7b: the ECO browser.
    entries.push(item(
        "menu-tools-eco-browser",
        Some(Message::Ui(UiMessage::OpenEcoBrowser)),
    ));
    entries.push(MenuEntry::Group("menu-group-annotation"));
    // Spec M4-6: opens the "Configure annotation" dialog (cancels a run).
    entries.push(item(
        "menu-tools-annotate",
        Some(Message::App(AppMessage::Annotate(AnnotateMessage::Open))),
    ));
    // Spec M4-10b: the game report panel.
    entries.push(MenuEntry::Item {
        label: Label::Key("menu-tools-report"),
        shortcut: None,
        action: Some(Message::Ui(UiMessage::TogglePane(PaneKind::Report))),
        checked: Some(ui.is_open(PaneKind::Report)),
        icon: None,
    });
    // Spec M7-5c: the EPD panel (no default key, D3850).
    entries.push(MenuEntry::Item {
        label: Label::Key("menu-tools-epd"),
        shortcut: first_binding(app, Action::ToggleEpdPane),
        action: Some(Message::Ui(UiMessage::TogglePane(PaneKind::Epd))),
        checked: Some(ui.is_open(PaneKind::Epd)),
        icon: None,
    });
    MenuSpec {
        label_key: "menu-tools",
        entries,
    }
}

/// Search (spec M6-1c): the advanced searches of the filter. They have no
/// default key; the user may bind one (D3249 item 7).
fn search_menu(app: &AppState) -> MenuSpec {
    let open = |action: Action| Some(Message::Ui(UiMessage::Action(action)));
    MenuSpec {
        label_key: "menu-search",
        entries: vec![
            keyed(
                app,
                "menu-search-board",
                Action::SearchBoard,
                open(Action::SearchBoard),
            ),
            keyed(
                app,
                "menu-search-material",
                Action::SearchMaterial,
                open(Action::SearchMaterial),
            ),
            keyed(
                app,
                "menu-search-moves",
                Action::SearchMoves,
                open(Action::SearchMoves),
            ),
            keyed(
                app,
                "menu-search-pattern",
                Action::SearchPattern,
                open(Action::SearchPattern),
            ),
        ],
    }
}

/// Play (spec M9-2 2.1): the game against an engine and the tournament,
/// moved from Tools. Same actions and conditions as before.
fn play_menu(app: &AppState, ui: &UiState) -> MenuSpec {
    // Spec M8-1b: the game against an engine (not while one is on).
    let mut entries = vec![
        // Spec M9-6: a new game from a Chess960 starting position.
        item(
            "menu-play-chess960",
            (!app.play_live()).then_some(Message::Dialog(DialogMessage::OpenChess960)),
        ),
        MenuEntry::Group("menu-group-vs-engine"),
        keyed(
            app,
            "menu-play-engine",
            Action::PlayEngine,
            (!app.play_live()).then_some(Message::Ui(UiMessage::Action(Action::PlayEngine))),
        ),
        // Spec M8-7: the same game, with the strength set by an Elo.
        keyed(
            app,
            "menu-play-tactical",
            Action::TacticalGame,
            (!app.play_live()).then_some(Message::Ui(UiMessage::Action(Action::TacticalGame))),
        ),
        MenuEntry::Group("menu-group-tournament"),
    ];
    // Spec M8-2b: the tournament between engines. While one is on, the item
    // shows its panel instead of opening the form.
    entries.push(keyed(
        app,
        "menu-play-tournament",
        Action::TournamentEngines,
        (!app.play_live()).then_some(Message::Ui(UiMessage::Action(Action::TournamentEngines))),
    ));
    entries.push(item(
        "menu-play-tournament-resume",
        (!app.tournament_live() && !app.play_live())
            .then_some(Message::Tournament(TournamentUiMessage::OpenResume)),
    ));
    entries.push(MenuEntry::Item {
        label: Label::Key("menu-play-tournament-panel"),
        shortcut: None,
        action: Some(Message::Ui(UiMessage::TogglePane(PaneKind::Tournament))),
        checked: Some(ui.is_open(PaneKind::Tournament)),
        icon: None,
    });
    // Spec M8-4c: the tactics training (not while a game is on).
    entries.push(MenuEntry::Group("menu-group-training"));
    entries.push(keyed(
        app,
        "menu-play-tactics",
        Action::TacticsTrainer,
        (!app.play_live()).then_some(Message::Ui(UiMessage::Action(Action::TacticsTrainer))),
    ));
    // Spec M8-5c: the opening trainer (not while a game is on).
    entries.push(keyed(
        app,
        "menu-play-opening-trainer",
        Action::OpeningTrainer,
        (!app.play_live()).then_some(Message::Ui(UiMessage::Action(Action::OpeningTrainer))),
    ));
    // Spec M8-6c: the calculation of variants (not while a game is on).
    entries.push(keyed(
        app,
        "menu-play-calvar",
        Action::CalvarTrainer,
        (!app.play_live()).then_some(Message::Ui(UiMessage::Action(Action::CalvarTrainer))),
    ));
    MenuSpec {
        label_key: "menu-play",
        entries,
    }
}

/// Spec M8-3c: the online play on Lichess. The items follow the state of
/// the session; the lock of the assistance is released from here when the
/// connection is gone (D1332).
fn online_menu(app: &AppState) -> MenuSpec {
    use mychess_app::online::OnlinePhase;
    let online = &app.online;
    let phase = online.phase;
    // Two groups, one per server; the lock removal belongs to neither.
    let mut entries = vec![MenuEntry::Group("menu-group-lichess")];
    if let Some(account) = &online.account {
        let mut who = account.login.clone();
        if let Some((_, rating, _)) = account.ratings.first() {
            who = format!("{who} ({rating})");
        }
        entries.push(MenuEntry::Item {
            label: Label::Text(who),
            shortcut: None,
            action: None,
            checked: None,
            icon: None,
        });
    }
    let open = |dialog| Some(Message::Online(OnlineUiMessage::Open(dialog)));
    entries.push(keyed(
        app,
        "menu-online-connect",
        Action::OnlineConnect,
        (phase == OnlinePhase::Disconnected)
            .then(|| Message::Ui(UiMessage::Action(Action::OnlineConnect))),
    ));
    let can_begin =
        phase.can_start() || matches!(phase, OnlinePhase::Seeking | OnlinePhase::Challenging);
    entries.push(keyed(
        app,
        "menu-online-seek",
        Action::OnlineSeek,
        can_begin.then(|| Message::Ui(UiMessage::Action(Action::OnlineSeek))),
    ));
    entries.push(item(
        "menu-online-challenge",
        (can_begin && online.account.as_ref().is_some_and(|a| a.can_challenge))
            .then(|| open(OnlineDialog::Challenge))
            .flatten(),
    ));
    entries.push(MenuEntry::Item {
        label: Label::Numbered(
            "menu-online-incoming",
            u8::try_from(online.incoming.len()).unwrap_or(u8::MAX),
        ),
        shortcut: None,
        action: (phase.is_connected() && !phase.is_live())
            .then(|| open(OnlineDialog::Incoming))
            .flatten(),
        checked: None,
        icon: None,
    });
    entries.push(item(
        "menu-online-watch",
        (matches!(phase, OnlinePhase::Disconnected | OnlinePhase::Idle) && !app.assist_locked())
            .then(|| open(OnlineDialog::Watch))
            .flatten(),
    ));
    entries.push(item(
        "menu-online-stop-watch",
        online
            .is_watching()
            .then_some(Message::Online(OnlineUiMessage::StopWatch)),
    ));
    entries.push(item(
        "menu-online-disconnect",
        (phase.is_connected() && !phase.is_live())
            .then_some(Message::Online(OnlineUiMessage::Disconnect)),
    ));
    entries.push(MenuEntry::Group("menu-group-fics"));
    entries.push(item(
        "menu-online-fics",
        Some(Message::Fics(
            crate::windows::fics_dialog::FicsUiMessage::Open,
        )),
    ));
    entries.push(MenuEntry::Separator);
    entries.push(item(
        "menu-online-remove-lock",
        app.can_remove_lock()
            .then_some(Message::Online(OnlineUiMessage::AskRemoveLock)),
    ));
    MenuSpec {
        label_key: "menu-online",
        entries,
    }
}

fn help_menu() -> MenuSpec {
    MenuSpec {
        label_key: "menu-help",
        entries: vec![
            item(
                "menu-help-manual",
                Some(Message::ManualUi(
                    crate::windows::manual::ManualUiMessage::Open,
                )),
            ),
            item(
                "menu-help-tip",
                Some(Message::App(mychess_app::Message::Start(
                    mychess_app::start::StartMessage::ShowTip,
                ))),
            ),
            item(
                "menu-help-about",
                Some(Message::Ui(UiMessage::ShowAbout(true))),
            ),
        ],
    }
}

/// The icon of the command or submenu with the Fluent key `key` (spec M9-2
/// D1632). `None` for a key that is not a command (the radio choices and
/// the checkable options inside a submenu inherit the icon of the submenu).
// One flat table of keys, grouped by menu: arms with the same icon stay in
// the group of their menu instead of being merged across menus (spec M9-7).
#[allow(clippy::too_many_lines, clippy::match_same_arms)]
fn icon_for(key: &str) -> Option<Icon> {
    Some(match key {
        // File.
        "menu-file-new" | "menu-file-epd-new" | "menu-play-chess960" => Icon::FileNew,
        "menu-file-open" | "menu-file-epd-open" => Icon::FolderOpen,
        "menu-file-open-recent" | "menu-file-epd-recent" => Icon::Recent,
        "menu-file-open-recent-clear" | "menu-file-clear-clipbase" | "menu-edit-clipbase-clear" => {
            Icon::Eraser
        }
        "menu-file-bookmarks" => Icon::Bookmark,
        "menu-file-bookmarks-add" => Icon::BookmarkAdd,
        "menu-file-bookmarks-manage" => Icon::Manage,
        "menu-file-databases" | "menu-tools-book" | "menu-tools-tablebase" => Icon::Database,
        "menu-file-next-base" => Icon::ArrowRight,
        "menu-file-previous-base" => Icon::ArrowLeft,
        "menu-file-close-base" | "menu-file-unload-spelling" | "menu-file-cancel-spelling-load" => {
            Icon::Close
        }
        "menu-file-read-only" => Icon::Lock,
        "menu-file-save" | "menu-file-epd-save" => Icon::Save,
        "menu-file-save-as" | "menu-file-epd-save-as" => Icon::SaveAs,
        "menu-file-epd-recent-clear" => Icon::Trash,
        "menu-file-append" => Icon::AppendFile,
        "menu-file-export-games" | "menu-file-export-image" => Icon::Upload,
        "menu-file-quit" => Icon::Quit,
        // Edit.
        "menu-edit-undo" => Icon::Undo,
        "menu-edit-redo" => Icon::Redo,
        "menu-edit-copy-fen" => Icon::Copy,
        "menu-edit-copy-pgn" | "menu-edit-clipbase-copy" => Icon::ClipboardCopy,
        "menu-edit-paste-fen" | "menu-edit-clipbase-paste" => Icon::ClipboardPaste,
        "menu-edit-import-pgn" | "menu-file-import-pgn-file" => Icon::Import,
        "menu-edit-setup-board" | "menu-search-board" => Icon::SetupBoard,
        "menu-edit-strip" => Icon::Scissors,
        "menu-edit-merge-game" => Icon::Merge,
        "menu-edit-strip-comments" => Icon::NoComments,
        "menu-edit-strip-variations" => Icon::Branch,
        "menu-edit-truncate-start" => Icon::TruncateStart,
        "menu-edit-truncate-end" => Icon::TruncateEnd,
        // Game.
        "menu-game-start" => Icon::First,
        "menu-game-previous" => Icon::Back,
        "menu-game-next" => Icon::Forward,
        "menu-game-end" => Icon::Last,
        "menu-game-first-game" => Icon::FirstGame,
        "menu-game-prev-game" => Icon::PrevGame,
        "menu-game-next-game" => Icon::NextGame,
        "menu-game-last-game" => Icon::LastGame,
        "menu-game-flip" => Icon::Flip,
        "menu-game-goto-move" => Icon::Hash,
        "menu-game-info" | "menu-help-about" | "menu-help-tip" => Icon::Info,
        "menu-help-manual" => Icon::BookOpen,
        "menu-game-identify-opening" | "menu-online-seek" | "menu-stats-piece-tracker" => {
            Icon::Search
        }
        "menu-game-resign" => Icon::Resign,
        "menu-game-rematch" => Icon::Rematch,
        "menu-game-novelty"
        | "menu-view-legal-moves"
        | "menu-play-tactics"
        | "menu-play-calvar"
        | "menu-search-pattern" => Icon::Target,
        // Search.
        "menu-search-material" => Icon::Material,
        // Play.
        "menu-play-engine" | "menu-play-tactical" => Icon::Bot,
        "menu-play-tournament" | "menu-stats-find-tournaments" => Icon::Trophy,
        "menu-play-tournament-resume" => Icon::Play,
        "menu-play-tournament-panel" | "menu-file-epd" | "menu-tools-epd" => Icon::ListOrdered,
        // View.
        "menu-view-theme" => Icon::Palette,
        "menu-view-notation" | "menu-search-moves" => Icon::Notation,
        "menu-view-side-to-move" => Icon::SideToMove,
        "menu-view-last-move" => Icon::Highlight,
        "menu-view-marks" => Icon::Marks,
        "menu-view-animation" => Icon::Timer,
        "menu-view-evalbar" | "menu-options-evalbar" => Icon::EvalBar,
        "menu-view-traffic-lights" | "menu-stats-add-elo" => Icon::Gauge,
        "menu-view-arrow-numbers" | "menu-options-arrows" => Icon::Arrow,
        "menu-options-evalbar-scale" | "analysis-arrow-seq-len" => Icon::Ruler,
        "menu-view-panes" | "menu-view-menu-style" => Icon::Panes,
        "menu-view-reset-layout" => Icon::ResetLayout,
        "menu-view-status-bar" => Icon::StatusBar,
        "menu-view-language" => Icon::Language,
        // Tools.
        "menu-tools-engines" => Icon::Engine,
        "menu-tools-toggle-analysis" => Icon::Analysis,
        "menu-tools-analysis-window" => Icon::Activity,
        // Database (spec M9-7).
        "menu-tools-maintenance" => Icon::Manage,
        "menu-db-compact" => Icon::Database,
        "menu-db-sort" => Icon::SortDown,
        "menu-db-twins" => Icon::Copy,
        "menu-db-cleaner" => Icon::Eraser,
        "menu-db-check" => Icon::Activity,
        "menu-tools-repair-base" => Icon::ResetLayout,
        "menu-db-type" => Icon::Info,
        "menu-db-description" => Icon::BookOpen,
        "menu-db-flags" => Icon::Marks,
        "menu-db-tags" => Icon::Hash,
        "menu-tools-name-editor" => Icon::Notation,
        "menu-db-spelling-file" => Icon::Load,
        "menu-tools-graph" | "menu-tools-eco-browser" => Icon::Graph,
        "menu-tools-tree" | "menu-play-opening-trainer" => Icon::Tree,
        "menu-tools-report"
        | "menu-tools-opening-report"
        | "menu-tools-player-report"
        | "menu-stats-stats" => Icon::Report,
        "menu-stats-crosstable" => Icon::Columns,
        "menu-stats-graph-relative" | "menu-stats-graph-absolute" | "menu-stats-rating-graph" => {
            Icon::Graph
        }
        "menu-tools-eco-classify"
        | "menu-stats-spell"
        | "menu-stats-spell-player"
        | "menu-stats-spell-event"
        | "menu-stats-spell-site"
        | "menu-stats-spell-round" => Icon::Eraser,
        "menu-tools-annotate" => Icon::Annotate,
        "menu-file-load-spelling" | "menu-file-reload-spelling" => Icon::Load,
        // Online.
        "menu-online-connect" | "menu-online-fics" | "menu-tools-eboard" => Icon::Connect,
        "menu-online-challenge" => Icon::Challenge,
        "menu-online-incoming" => Icon::Inbox,
        "menu-online-disconnect" | "menu-online-stop-watch" => Icon::Disconnect,
        "menu-online-watch" | "menu-stats-find-players" | "menu-stats-player-info" => Icon::Users,
        "menu-view-sounds" => Icon::Options,
        "menu-online-remove-lock" => Icon::Unlock,
        _ => return None,
    })
}

/// Fills the icons of `entries` (and of the nested ones) from [`icon_for`].
fn assign_icons(entries: &mut [MenuEntry]) {
    for entry in entries {
        match entry {
            MenuEntry::Item { label, icon, .. } => {
                let key = match label {
                    // The "Start engine N" items share one key.
                    Label::Numbered("menu-tools-start-engine", _) => {
                        *icon = Some(Icon::Play);
                        None
                    }
                    Label::Key(key) | Label::Numbered(key, _) => Some(*key),
                    _ => None,
                };
                if let Some(found) = key.and_then(icon_for) {
                    *icon = Some(found);
                }
            }
            MenuEntry::Submenu {
                label,
                entries,
                icon,
            } => {
                if let Label::Key(key) = label {
                    *icon = icon_for(key);
                }
                assign_icons(entries);
            }
            MenuEntry::Separator | MenuEntry::Group(_) => {}
        }
    }
}

/// The menus for the current state.
pub fn menu_specs(app: &AppState, ui: &UiState, options: ViewOptions) -> Vec<MenuSpec> {
    let mut specs = menu_specs_without_icons(app, ui, options);
    for spec in &mut specs {
        assign_icons(&mut spec.entries);
    }
    specs
}

fn menu_specs_without_icons(app: &AppState, ui: &UiState, options: ViewOptions) -> Vec<MenuSpec> {
    vec![
        file_menu(app),
        edit_menu(app),
        game_menu(app),
        database_menu(app),
        search_menu(app),
        stats_menu(app, ui),
        play_menu(app, ui),
        view_menu(app, ui, options),
        tools_menu(app, ui),
        online_menu(app),
        help_menu(),
    ]
}

pub(crate) fn label_text(label: &Label, i18n: &I18n) -> String {
    match label {
        Label::Key(key) => i18n.tr(key),
        Label::Text(text) => text.clone(),
        Label::Numbered(key, number) => {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("number", i64::from(*number));
            i18n.tr_args(key, &args)
        }
        Label::Milliseconds(ms) => {
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("ms", i64::from(*ms));
            i18n.tr_args("menu-view-animation-ms", &args)
        }
    }
}

const MARK_WIDTH: f32 = 20.0;
const MENU_WIDTH: f32 = 280.0;

fn entry_row<'a>(
    label: String,
    shortcut: Option<String>,
    checked: Option<bool>,
    arrow: bool,
) -> Element<'a, Message> {
    let mark = if checked == Some(true) { "✓" } else { "" };
    let trailing = if arrow {
        "▸".to_owned()
    } else {
        shortcut.unwrap_or_default()
    };
    row![text(mark).width(MARK_WIDTH)]
        .push(text(label).width(Length::Fill))
        .push(text(trailing).style(theme::muted_text))
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([2, 8]))
        .into()
}

/// Whether the dropdown draws the entry as a divider with no action: the
/// separators and the starts of a group (the title is only for the ribbon).
pub(crate) fn is_divider(entry: &MenuEntry) -> bool {
    matches!(entry, MenuEntry::Separator | MenuEntry::Group(_))
}

/// The entries the dropdown draws: a group that opens the menu has nothing
/// above it to divide from, so it is dropped (spec M9-2 review of slice a).
pub(crate) fn dropdown_entries(entries: Vec<MenuEntry>) -> Vec<MenuEntry> {
    let mut seen_any = false;
    entries
        .into_iter()
        .filter(|entry| {
            let keep = seen_any || !matches!(entry, MenuEntry::Group(_));
            seen_any |= keep;
            keep
        })
        .collect()
}

fn build_entry<'a>(
    entry: MenuEntry,
    i18n: &I18n,
    platform: Platform,
) -> Item<'a, Message, iced::Theme, iced::Renderer> {
    let divider = || Item::new(container(rule::horizontal(1)).padding([3, 4]));
    if is_divider(&entry) {
        return divider();
    }
    match entry {
        MenuEntry::Separator | MenuEntry::Group(_) => divider(),
        MenuEntry::Item {
            label,
            shortcut,
            action,
            checked,
            icon: _,
        } => {
            let content = entry_row(
                label_text(&label, i18n),
                shortcut.map(|b| binding_label(&b, platform)),
                checked,
                false,
            );
            Item::new(
                button(content)
                    .width(Length::Fill)
                    .padding(0)
                    .style(theme::menu_item)
                    .on_press_maybe(action),
            )
        }
        MenuEntry::Submenu { label, entries, .. } => {
            let content = entry_row(label_text(&label, i18n), None, None, true);
            Item::with_menu(
                container(content).width(Length::Fill),
                build_menu(entries, i18n, platform),
            )
        }
    }
}

pub(crate) fn build_menu<'a>(
    entries: Vec<MenuEntry>,
    i18n: &I18n,
    platform: Platform,
) -> Menu<'a, Message, iced::Theme, iced::Renderer> {
    Menu::new(
        dropdown_entries(entries)
            .into_iter()
            .map(|entry| build_entry(entry, i18n, platform))
            .collect(),
    )
    .max_width(MENU_WIDTH)
    .offset(2.0)
}

/// Renders the menu bar.
pub fn view<'a>(specs: Vec<MenuSpec>, i18n: &I18n, platform: Platform) -> Element<'a, Message> {
    let roots = specs
        .into_iter()
        .map(|spec| {
            let title = container(text(i18n.tr(spec.label_key))).padding([4, 10]);
            Item::with_menu(title, build_menu(spec.entries, i18n, platform))
        })
        .collect();
    crate::widgets::menu_guard::guard(MenuBar::new(roots).spacing(2))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use super::*;
    use crate::windows::layout::LayoutNode;

    fn specs_for(app: &AppState) -> Vec<MenuSpec> {
        let ui = UiState::new(&LayoutNode::default_layout());
        menu_specs(app, &ui, ViewOptions::default())
    }

    fn specs() -> Vec<MenuSpec> {
        specs_for(&AppState::default())
    }

    /// The entries of the menu with this key.
    fn menu_of<'a>(all: &'a [MenuSpec], key: &str) -> &'a [MenuEntry] {
        &all.iter()
            .find(|s| s.label_key == key)
            .unwrap_or_else(|| panic!("menu {key}"))
            .entries
    }

    fn shape(entries: &[MenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                MenuEntry::Item {
                    label: Label::Key(k),
                    ..
                } => (*k).to_owned(),
                MenuEntry::Item {
                    label: Label::Numbered(k, n),
                    ..
                } => format!("{k}#{n}"),
                MenuEntry::Group(k) => format!("group:{k}"),
                MenuEntry::Separator => "-".to_owned(),
                MenuEntry::Submenu { .. } => "submenu".to_owned(),
                MenuEntry::Item { .. } => "item".to_owned(),
            })
            .collect()
    }

    #[test]
    fn the_play_menu_has_the_engine_and_tournament_groups() {
        let all = specs();
        assert_eq!(
            shape(menu_of(&all, "menu-play")),
            [
                "menu-play-chess960",
                "group:menu-group-vs-engine",
                "menu-play-engine",
                "menu-play-tactical",
                "group:menu-group-tournament",
                "menu-play-tournament",
                "menu-play-tournament-resume",
                "menu-play-tournament-panel",
                "group:menu-group-training",
                "menu-play-tactics",
                "menu-play-opening-trainer",
                "menu-play-calvar",
            ]
        );
    }

    #[test]
    fn the_tools_menu_keeps_engines_analysis_openings_and_annotation_only() {
        // Spec M9-7 D4604: the database items left for the Database menu.
        let all = specs();
        assert_eq!(
            shape(menu_of(&all, "menu-tools")),
            [
                "group:menu-group-engines",
                "menu-tools-engines",
                "menu-tools-start-engine#1",
                "menu-tools-start-engine#2",
                "menu-tools-start-engine#3",
                "menu-tools-toggle-analysis",
                "menu-tools-eboard",
                "group:menu-group-analysis-windows",
                "menu-tools-analysis-window",
                "menu-tools-graph",
                "menu-tools-tablebase",
                "group:menu-group-openings",
                "menu-tools-tree",
                "menu-tools-book",
                "menu-tools-eco-browser",
                "group:menu-group-annotation",
                "menu-tools-annotate",
                "menu-tools-report",
                "menu-tools-epd",
            ]
        );
    }

    #[test]
    fn moved_items_send_the_same_messages_as_before() {
        let all = specs();
        let play = menu_of(&all, "menu-play");
        assert!(matches!(
            action_of(play, "menu-play-engine"),
            Some(Message::Ui(UiMessage::Action(Action::PlayEngine)))
        ));
        assert!(matches!(
            action_of(play, "menu-play-tournament"),
            Some(Message::Ui(UiMessage::Action(Action::TournamentEngines)))
        ));
        assert!(matches!(
            action_of(play, "menu-play-tournament-resume"),
            Some(Message::Tournament(TournamentUiMessage::OpenResume))
        ));
        assert!(matches!(
            action_of(play, "menu-play-tournament-panel"),
            Some(Message::Ui(UiMessage::TogglePane(PaneKind::Tournament)))
        ));
        // The checkable item reflects the panel and the shortcuts stay.
        assert!(matches!(
            find(play, "menu-play-tournament-panel"),
            Some(MenuEntry::Item {
                checked: Some(false),
                ..
            })
        ));
        assert!(matches!(
            find(play, "menu-play-engine"),
            Some(MenuEntry::Item {
                shortcut: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn a_group_is_a_divider_without_action_and_never_opens_a_menu() {
        let group = MenuEntry::Group("menu-group-tournament");
        assert!(is_divider(&group));
        assert!(is_divider(&MenuEntry::Separator));
        assert!(!is_divider(&item("menu-help-about", None)));
        assert!(!is_divider(&submenu("menu-file-open-recent", Vec::new())));
        // The group at position 0 has nothing above to divide from.
        let kept = dropdown_entries(vec![
            MenuEntry::Group("menu-group-engines"),
            item("menu-tools-engines", None),
            MenuEntry::Group("menu-group-annotation"),
            item("menu-tools-annotate", None),
        ]);
        assert_eq!(
            shape(&kept),
            [
                "menu-tools-engines",
                "group:menu-group-annotation",
                "menu-tools-annotate"
            ]
        );
        // The tools menu of the application opens with a group: dropped.
        let all = specs();
        let tools = menu_of(&all, "menu-tools").to_vec();
        assert!(matches!(tools[0], MenuEntry::Group(_)));
        assert!(!matches!(dropdown_entries(tools)[0], MenuEntry::Group(_)));
    }

    #[test]
    fn groups_render_as_dividers_and_the_view_builds() {
        // A group never carries an action or a label of its own in the
        // dropdown; building the whole bar with groups must not panic.
        let i18n = I18n::new(crate::i18n::Language::PtBr);
        drop(view(specs(), &i18n, Platform::Linux));
        for language in crate::i18n::Language::ALL {
            let i18n = I18n::new(language);
            for spec in specs() {
                for entry in &spec.entries {
                    if let MenuEntry::Group(key) = entry {
                        assert!(i18n.has(key), "{key} missing in {language:?}");
                    }
                }
            }
        }
    }

    /// Spec M9-2 D1624/D1632: every command and every submenu has an icon.
    /// Exempt, because they inherit the icon of their submenu: the radio
    /// choices and the checkable options inside a submenu (`checked` set),
    /// the entries with text that is not a key (file names, bookmarks, the
    /// account line) and the two disabled placeholders.
    fn missing_icons(
        entries: &[MenuEntry],
        inside_submenu: bool,
        path: &str,
        out: &mut Vec<String>,
    ) {
        const PLACEHOLDERS: [&str; 3] = [
            "menu-file-open-recent-none",
            "menu-file-bookmarks-empty",
            "menu-file-epd-recent-none",
        ];
        for entry in entries {
            match entry {
                MenuEntry::Item {
                    label,
                    checked,
                    icon,
                    ..
                } => {
                    let key = match label {
                        Label::Key(key) | Label::Numbered(key, _) => *key,
                        _ => continue,
                    };
                    let option = inside_submenu && checked.is_some();
                    if icon.is_none() && !option && !PLACEHOLDERS.contains(&key) {
                        out.push(format!("{path}/{key}"));
                    }
                }
                MenuEntry::Submenu {
                    label,
                    entries,
                    icon,
                } => {
                    let Label::Key(key) = label else { continue };
                    if icon.is_none() {
                        out.push(format!("{path}/{key}"));
                    }
                    missing_icons(entries, true, &format!("{path}/{key}"), out);
                }
                MenuEntry::Separator | MenuEntry::Group(_) => {}
            }
        }
    }

    #[test]
    fn every_command_and_submenu_has_an_icon() {
        let mut missing = Vec::new();
        for spec in specs() {
            missing_icons(&spec.entries, false, spec.label_key, &mut missing);
        }
        assert!(missing.is_empty(), "entries without an icon: {missing:?}");
    }

    #[test]
    fn icons_are_assigned_by_key_and_options_inherit() {
        let all = specs();
        let file = menu_of(&all, "menu-file");
        assert!(matches!(
            find(file, "menu-file-new"),
            Some(MenuEntry::Item {
                icon: Some(Icon::FileNew),
                ..
            })
        ));
        assert!(matches!(
            find(file, "menu-file-bookmarks"),
            Some(MenuEntry::Submenu {
                icon: Some(Icon::Bookmark),
                ..
            })
        ));
        // The numbered "Start engine N" items all get the play icon.
        let tools = menu_of(&all, "menu-tools");
        let starts = tools
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    MenuEntry::Item {
                        label: Label::Numbered("menu-tools-start-engine", _),
                        icon: Some(Icon::Play),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(starts, 3);
        // A radio choice inherits: it carries no icon of its own.
        let view_menu = menu_of(&all, "menu-view");
        let Some(MenuEntry::Submenu { entries, .. }) = find(view_menu, "menu-view-animation")
        else {
            panic!("animation submenu");
        };
        assert!(entries.iter().all(|e| matches!(
            e,
            MenuEntry::Item {
                icon: None,
                checked: Some(_),
                ..
            }
        )));
    }

    fn find<'a>(entries: &'a [MenuEntry], key: &str) -> Option<&'a MenuEntry> {
        entries.iter().find(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                ..
            }
            | MenuEntry::Submenu {
                label: Label::Key(k),
                ..
            } => *k == key,
            _ => false,
        })
    }

    fn action_of<'a>(entries: &'a [MenuEntry], key: &str) -> Option<&'a Message> {
        match find(entries, key) {
            Some(MenuEntry::Item { action, .. }) => action.as_ref(),
            _ => None,
        }
    }

    #[test]
    fn there_are_eleven_menus_in_order() {
        let keys: Vec<_> = specs().iter().map(|s| s.label_key).collect();
        assert_eq!(
            keys,
            [
                "menu-file",
                "menu-edit",
                "menu-game",
                "menu-database",
                "menu-search",
                "menu-stats",
                "menu-play",
                "menu-view",
                "menu-tools",
                "menu-online",
                "menu-help"
            ]
        );
    }

    #[test]
    fn file_and_edit_items_send_the_commands_of_the_application() {
        let all = specs();
        for (menu, key, expected) in [
            (0, "menu-file-new", Command::NewGame),
            (0, "menu-file-open", Command::Open),
            (0, "menu-file-save", Command::Save),
            (0, "menu-file-save-as", Command::SaveAs),
            (0, "menu-file-append", Command::Append),
            (1, "menu-edit-copy-fen", Command::CopyFen),
            (1, "menu-edit-copy-pgn", Command::CopyPgn),
            (1, "menu-edit-paste-fen", Command::PasteFen),
        ] {
            assert!(
                matches!(
                    action_of(&all[menu].entries, key),
                    Some(Message::App(mychess_app::Message::Command(c))) if *c == expected
                ),
                "{key}"
            );
        }
        assert!(matches!(
            action_of(&all[0].entries, "menu-file-quit"),
            Some(Message::Ui(UiMessage::CloseRequested))
        ));
    }

    #[test]
    fn the_dialogs_of_the_game_and_edit_menus_are_enabled() {
        let all = specs();
        assert!(matches!(
            action_of(&all[2].entries, "menu-game-info"),
            Some(Message::Dialog(DialogMessage::OpenHeader(None)))
        ));
        assert!(matches!(
            action_of(&all[2].entries, "menu-game-goto-move"),
            Some(Message::Dialog(DialogMessage::OpenGoto))
        ));
        assert!(matches!(
            action_of(&all[1].entries, "menu-edit-setup-board"),
            Some(Message::App(mychess_app::Message::Command(
                Command::SetupBoard
            )))
        ));
        assert!(matches!(
            action_of(&all[1].entries, "menu-edit-import-pgn"),
            Some(Message::App(mychess_app::Message::Import(
                ImportMessage::Open
            )))
        ));
    }

    #[test]
    fn removing_items_ask_through_the_application_and_follow_the_cursor() {
        let all = specs();
        let Some(MenuEntry::Submenu { entries, .. }) = find(&all[1].entries, "menu-edit-strip")
        else {
            panic!("strip submenu");
        };
        let sends = |entries: &[MenuEntry], key: &str, kind: StripKind| {
            matches!(
                action_of(entries, key),
                Some(Message::App(mychess_app::Message::Game(
                    mychess_app::GameMessage::RequestStrip { kind: k, confirm: true }
                ))) if *k == kind
            )
        };
        assert!(sends(
            entries,
            "menu-edit-strip-comments",
            StripKind::Comments
        ));
        assert!(sends(
            entries,
            "menu-edit-strip-variations",
            StripKind::Variations
        ));
        assert!(
            action_of(entries, "menu-edit-truncate-start").is_none(),
            "at the start"
        );
        assert!(
            action_of(entries, "menu-edit-truncate-end").is_none(),
            "empty game"
        );
        let mut app = AppState::default();
        let mv = app.session.position().parse_uci("e2e4").expect("legal");
        let e4 = mychess_app::update(&mut app, app_api::play_move(mv));
        drop(e4);
        mychess_app::update(&mut app, app_api::navigate(Nav::Start));
        let all = specs_for(&app);
        let Some(MenuEntry::Submenu { entries, .. }) = find(&all[1].entries, "menu-edit-strip")
        else {
            panic!("strip submenu");
        };
        let root = app.session.cursor();
        assert!(sends(
            entries,
            "menu-edit-truncate-end",
            StripKind::TruncateAfter(root)
        ));
        assert!(action_of(entries, "menu-edit-truncate-start").is_none());
    }

    #[test]
    fn shortcuts_come_from_the_map_of_the_application() {
        let all = specs();
        let Some(MenuEntry::Item { shortcut, .. }) = find(&all[0].entries, "menu-file-new") else {
            panic!("new item");
        };
        let label = shortcut.map(|b| binding_label(&b, Platform::Linux));
        assert_eq!(label.as_deref(), Some("Ctrl+Shift+N"));
        let mut app = AppState::default();
        let (custom, _) = mychess_app::Shortcuts::defaults().with_overrides(
            &std::iter::once(("NewGame".to_owned(), "Command+Shift+Q".to_owned())).collect(),
        );
        app.shortcuts = custom;
        let all = specs_for(&app);
        let Some(MenuEntry::Item { shortcut, .. }) = find(&all[0].entries, "menu-file-new") else {
            panic!("new item");
        };
        let label = shortcut.map(|b| binding_label(&b, Platform::Linux));
        assert_eq!(label.as_deref(), Some("Ctrl+Shift+Q"));
    }

    #[test]
    fn open_recent_lists_the_files_by_name_and_is_disabled_when_empty() {
        let all = specs();
        let Some(MenuEntry::Submenu { entries, .. }) =
            find(&all[0].entries, "menu-file-open-recent")
        else {
            panic!("recent submenu");
        };
        assert!(action_of(entries, "menu-file-open-recent-none").is_none());

        let mut app = AppState::default();
        let dir = PathBuf::from("games");
        app.prefs.files.recent.push(dir.join("old.pgn"));
        app.prefs.files.recent.push(dir.join("new.pgn"));
        let all = specs_for(&app);
        let Some(MenuEntry::Submenu { entries, .. }) =
            find(&all[0].entries, "menu-file-open-recent")
        else {
            panic!("recent submenu");
        };
        let names: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item {
                    label: Label::Text(name),
                    action: Some(Message::App(mychess_app::Message::Command(c))),
                    ..
                } => Some((name.as_str(), *c)),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            [
                ("1: new.pgn", Command::OpenRecent(0)),
                ("2: old.pgn", Command::OpenRecent(1))
            ]
        );
        assert!(matches!(
            action_of(entries, "menu-file-open-recent-clear"),
            Some(Message::App(mychess_app::Message::Command(
                Command::ClearRecent
            )))
        ));
    }

    #[test]
    fn repeated_recent_names_get_their_folder() {
        let files = [
            PathBuf::from("a").join("games.pgn"),
            PathBuf::from("b").join("other.pgn"),
            PathBuf::from("c").join("games.pgn"),
        ];
        assert_eq!(
            recent_labels(&files),
            [
                "1: games.pgn \u{2014} a/",
                "2: other.pgn",
                "3: games.pgn \u{2014} c/"
            ]
        );
    }

    #[test]
    fn recent_names_with_the_same_parent_climb_one_more_level() {
        let files = [
            PathBuf::from("a").join("x").join("f.pgn"),
            PathBuf::from("b").join("x").join("f.pgn"),
            PathBuf::from("c").join("y").join("f.pgn"),
        ];
        assert_eq!(
            recent_labels(&files),
            [
                "1: f.pgn \u{2014} a/x/",
                "2: f.pgn \u{2014} b/x/",
                "3: f.pgn \u{2014} y/"
            ]
        );
        // Identical paths cannot be told apart: only the number remains
        // distinct, after the deepest allowed label.
        let same = [
            PathBuf::from("a").join("f.pgn"),
            PathBuf::from("a").join("f.pgn"),
        ];
        assert_eq!(
            recent_labels(&same),
            ["1: f.pgn \u{2014} a/", "2: f.pgn \u{2014} a/"]
        );
    }

    fn game(title: &str) -> NodeKind {
        NodeKind::Game(mychess_app::GameBookmark {
            title: title.to_owned(),
            base: PathBuf::from("x.si4"),
            game: 1,
            ply: 0,
            white: String::new(),
            black: String::new(),
            year: 0,
            site: String::new(),
            round: String::new(),
            result: "*".to_owned(),
        })
    }

    #[test]
    fn bookmarks_menu_nests_folders_and_sends_go() {
        let mut app = AppState::default();
        let folder = app
            .bookmarks
            .insert_after(
                None,
                NodeKind::Folder(mychess_app::Folder {
                    title: "Openings".to_owned(),
                    children: vec![],
                }),
            )
            .expect("folder");
        let _ = folder;
        let id = app.bookmarks.insert_after(None, game("Top")).expect("game");
        let all = specs_for(&app);
        let Some(MenuEntry::Submenu { entries, .. }) = find(&all[0].entries, "menu-file-bookmarks")
        else {
            panic!("bookmarks submenu");
        };
        // "Add" is off without a game of a database; "Manage" is on.
        assert!(action_of(entries, "menu-file-bookmarks-add").is_none());
        assert!(matches!(
            action_of(entries, "menu-file-bookmarks-manage"),
            Some(Message::App(AppMessage::Bookmarks(
                BookmarksMessage::OpenEditor
            )))
        ));
        let go = entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Text(t),
                action: Some(Message::App(AppMessage::Bookmarks(BookmarksMessage::Go(i)))),
                ..
            } if t == "Top" => Some(*i),
            _ => None,
        });
        assert_eq!(go, Some(id));
        let empty = entries.iter().find_map(|e| match e {
            MenuEntry::Submenu {
                label: Label::Text(t),
                entries,
                ..
            } if t == "Openings" => Some(entries),
            _ => None,
        });
        let empty = empty.expect("folder submenu");
        assert!(action_of(empty, "menu-file-bookmarks-empty").is_none());
        assert!(find(empty, "menu-file-bookmarks-empty").is_some());
    }

    #[test]
    fn bookmark_shortcuts_come_from_the_central_map() {
        let all = specs();
        let Some(MenuEntry::Submenu { entries, .. }) = find(&all[0].entries, "menu-file-bookmarks")
        else {
            panic!("bookmarks submenu");
        };
        for (key, expected) in [
            ("menu-file-bookmarks-add", "Ctrl+D"),
            ("menu-file-bookmarks-manage", "Ctrl+Shift+D"),
        ] {
            let Some(MenuEntry::Item { shortcut, .. }) = find(entries, key) else {
                panic!("{key}");
            };
            let label = shortcut.map(|b| binding_label(&b, Platform::Linux));
            assert_eq!(label.as_deref(), Some(expected), "{key}");
        }
    }

    #[test]
    fn read_only_is_off_for_the_clipbase() {
        let all = specs();
        // Spec M9-7: Read-only moved to the Database menu.
        let db = menu_of(&all, "menu-database");
        assert!(find(db, "menu-file-read-only").is_some());
        assert!(action_of(db, "menu-file-read-only").is_none());
    }

    #[test]
    fn the_new_menu_keys_exist_in_both_languages() {
        use crate::i18n::Language;
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for key in [
                "menu-file-bookmarks",
                "menu-file-bookmarks-add",
                "menu-file-bookmarks-manage",
                "menu-file-bookmarks-empty",
                "menu-file-open-recent-clear",
                "menu-file-read-only",
            ] {
                assert!(i18n.has(key), "{key} missing in {language:?}");
            }
        }
    }

    #[test]
    fn next_and_previous_game_need_an_open_file_with_several_games() {
        let all = specs();
        assert!(action_of(&all[2].entries, "menu-game-next-game").is_none());
        assert!(action_of(&all[2].entries, "menu-game-first-game").is_none());
    }

    #[test]
    fn undo_is_enabled_only_when_there_is_something_to_undo() {
        let all = specs();
        assert!(action_of(&all[1].entries, "menu-edit-undo").is_none());
    }

    #[test]
    fn flip_item_carries_the_shortcut_and_the_message() {
        let all = specs();
        match find(&all[2].entries, "menu-game-flip") {
            Some(MenuEntry::Item {
                shortcut, action, ..
            }) => {
                let label = shortcut.map(|b| binding_label(&b, Platform::Linux));
                assert_eq!(label.as_deref(), Some("Ctrl+F"));
                assert!(matches!(action, Some(Message::Ui(UiMessage::FlipBoard))));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn the_theme_item_opens_the_appearance_window() {
        let all = specs();
        let Some(MenuEntry::Item {
            label,
            action,
            checked,
            ..
        }) = find(menu_of(&all, "menu-view"), "menu-view-theme")
        else {
            panic!("the theme entry must be a command, not a submenu");
        };
        assert_eq!(*label, Label::Key("menu-view-theme"));
        assert!(checked.is_none());
        assert!(matches!(
            action,
            Some(Message::Dialog(DialogMessage::Appearance(
                AppearanceMessage::Open
            )))
        ));
        assert_eq!(icon_for("menu-view-theme"), Some(Icon::Palette));
    }

    #[test]
    fn the_appearance_submenus_left_the_view_menu_but_the_notation_stayed() {
        let all = specs();
        let view = menu_of(&all, "menu-view");
        for key in [
            "menu-view-accent",
            "menu-view-board-theme",
            "menu-view-pieces",
            "menu-view-coords",
            "menu-view-material",
            "menu-view-player-strips",
        ] {
            assert!(find(view, key).is_none(), "{key} is a tab of the window");
        }
        assert!(matches!(
            find(view, "menu-view-notation"),
            Some(MenuEntry::Submenu { .. })
        ));
    }

    #[test]
    fn engine_overlay_toggles_carry_state_shortcut_and_message() {
        let all = specs();
        for (key, action, message) in [
            (
                "menu-view-evalbar",
                Action::ToggleEvalBar,
                app_api::EvalBarMessage::ToggleBar,
            ),
            (
                "menu-view-traffic-lights",
                Action::ToggleTrafficLights,
                app_api::EvalBarMessage::ToggleTrafficLights,
            ),
            (
                "menu-view-arrow-numbers",
                Action::ToggleArrowNumbers,
                app_api::EvalBarMessage::ToggleArrowNumbers,
            ),
        ] {
            let Some(MenuEntry::Item {
                shortcut,
                action: Some(sent),
                checked,
                ..
            }) = find(menu_of(&all, "menu-view"), key)
            else {
                panic!("{key} missing");
            };
            assert!(checked.is_some(), "{key} is checkable");
            let expected = AppState::default()
                .shortcuts
                .bindings(action)
                .first()
                .copied();
            assert_eq!(*shortcut, expected, "{key}");
            assert!(shortcut.is_some(), "{key} has a binding");
            assert!(
                matches!(sent, Message::App(AppMessage::EvalBar(m)) if *m == message),
                "{key}"
            );
        }
    }

    #[test]
    fn evalbar_and_arrow_options_change_the_preferences() {
        let all = specs();
        let Some(MenuEntry::Submenu { entries, .. }) =
            find(menu_of(&all, "menu-view"), "menu-options-evalbar")
        else {
            panic!("bar options");
        };
        let Some(MenuEntry::Submenu {
            entries: scales, ..
        }) = find(entries, "menu-options-evalbar-scale")
        else {
            panic!("scale submenu");
        };
        assert_eq!(scales.len(), 10);
        let checked: Vec<_> = scales
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item {
                    label: Label::Text(t),
                    checked: Some(true),
                    ..
                } => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, ["5"], "the Scid default scale");
        assert!(matches!(
            action_of(entries, "menu-options-evalbar-log"),
            Some(Message::App(AppMessage::Prefs(mychess_app::PrefsMessage::Set(mychess_app::PrefChange::EvalBar(p))))) if p.log
        ));
        let Some(MenuEntry::Submenu { entries, .. }) =
            find(menu_of(&all, "menu-view"), "menu-options-arrows")
        else {
            panic!("arrow options");
        };
        assert!(matches!(
            action_of(entries, "analysis-arrow-mode-sequence"),
            Some(Message::App(AppMessage::Prefs(mychess_app::PrefsMessage::Set(mychess_app::PrefChange::Arrows(p)))))
                if p.mode == app_api::ArrowMode::Sequence
        ));
        assert!(matches!(
            action_of(entries, "menu-options-arrows-click"),
            Some(Message::App(AppMessage::Prefs(mychess_app::PrefsMessage::Set(mychess_app::PrefChange::Arrows(p))))) if !p.click_to_play
        ));
        let Some(MenuEntry::Submenu { entries: lens, .. }) =
            find(entries, "analysis-arrow-seq-len")
        else {
            panic!("length submenu");
        };
        assert_eq!(lens.len(), 8);
    }

    #[test]
    fn persisted_choices_go_to_the_preferences_of_the_application() {
        let all = specs();
        assert!(matches!(
            action_of(menu_of(&all, "menu-view"), "menu-view-side-to-move"),
            Some(Message::App(mychess_app::Message::Prefs(
                mychess_app::PrefsMessage::Set(mychess_app::PrefChange::ShowSideToMove(false))
            )))
        ));
        // The theme, the accent, the board and the pieces are chosen in the
        // appearance window (spec M9-3), which has its own tests.
    }

    #[test]
    fn session_options_stay_in_the_gui() {
        let all = specs();
        match find(menu_of(&all, "menu-view"), "menu-view-marks") {
            Some(MenuEntry::Item {
                action: Some(Message::Ui(UiMessage::View(change))),
                checked,
                ..
            }) => {
                assert_eq!(*checked, Some(true));
                assert_eq!(*change, ViewChange::ShowMarks(false));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn animation_submenu_offers_off_and_ten_times() {
        let all = specs();
        let Some(MenuEntry::Submenu { entries, .. }) =
            find(menu_of(&all, "menu-view"), "menu-view-animation")
        else {
            panic!("animation submenu");
        };
        assert_eq!(entries.len(), ANIMATION_CHOICES_MS.len());
    }

    #[test]
    fn every_label_key_exists_in_both_languages() {
        use crate::i18n::Language;
        fn keys(entries: &[MenuEntry], out: &mut Vec<&'static str>) {
            for entry in entries {
                match entry {
                    MenuEntry::Item {
                        label: Label::Key(k),
                        ..
                    }
                    | MenuEntry::Group(k) => out.push(k),
                    MenuEntry::Submenu { label, entries, .. } => {
                        if let Label::Key(k) = label {
                            out.push(k);
                        }
                        keys(entries, out);
                    }
                    _ => {}
                }
            }
        }
        let mut all_keys = Vec::new();
        for spec in specs() {
            all_keys.push(spec.label_key);
            keys(&spec.entries, &mut all_keys);
        }
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for key in &all_keys {
                assert!(i18n.has(key), "{key} missing in {language:?}");
            }
        }
    }

    #[test]
    fn the_tools_menu_opens_the_dialog_and_starts_the_engines_of_the_keys() {
        let all = specs();
        let tools = all
            .iter()
            .find(|s| s.label_key == "menu-tools")
            .expect("menu");
        assert!(matches!(
            action_of(&tools.entries, "menu-tools-engines"),
            Some(Message::App(mychess_app::Message::Engine(
                EngineMessage::OpenDialog
            )))
        ));
        assert!(matches!(
            action_of(&tools.entries, "menu-tools-toggle-analysis"),
            Some(Message::App(mychess_app::Message::Engine(
                EngineMessage::ToggleMainAnalysis
            )))
        ));
        let numbered: Vec<_> = tools
            .entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item {
                    label: Label::Numbered(_, n),
                    shortcut,
                    action,
                    ..
                } => Some((
                    *n,
                    shortcut.map(|b| binding_label(&b, Platform::Linux)),
                    action.is_some(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            numbered,
            [
                (1, Some("F2".to_owned()), true),
                (2, Some("F3".to_owned()), true),
                (3, Some("F4".to_owned()), true)
            ]
        );
        let i18n = I18n::new(crate::i18n::Language::PtBr);
        assert_eq!(
            label_text(&Label::Numbered("menu-tools-start-engine", 2), &i18n),
            "Iniciar engine 2"
        );
        let shortcut = app_shortcut_label(&AppState::default(), Action::ManageEngines);
        assert_eq!(shortcut, "Ctrl+Shift+E");
    }

    fn app_shortcut_label(app: &AppState, action: Action) -> String {
        let binding = app
            .shortcuts
            .bindings(action)
            .first()
            .copied()
            .expect("bound");
        binding_label(&binding, Platform::Linux)
    }
}
