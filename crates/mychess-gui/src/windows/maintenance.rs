//! The window of the database maintenance (spec M7-1c, D4100-D4119; Scid
//! `tcl/file/maint.tcl`).
//!
//! One window, a layer over the main window like the opening report: the
//! header of the database with its statistics, the flags, the operations on
//! the database and the checker of duplicate games. Each operation opens a
//! form (a card over the window) whose fields live in [`MaintUi`]; the
//! criteria of the duplicates, the steps of the Cleaner and the scopes are
//! preferences of the application (`MaintenancePrefs`) and are edited in
//! place. The application owns everything that is a rule
//! (`AppState::maintenance_view`); this module keeps what the screen needs:
//! the form that is open and the text typed in its fields.
//!
//! - `main_view`: the header, the flags and the grid of operations;
//! - `forms`: the cards of the operations and the confirmation;
//! - `checker`: the checker of duplicate games (two columns);
//! - `text`: the translated words.

mod checker;
mod forms;
mod main_view;
pub mod text;

use std::path::PathBuf;

use iced::Element;
use iced::widget::{column, container, opaque, stack};
use mychess_app::maintenance::{
    MaintFlag, MaintRequest, MaintenanceMessage, MaintenancePrefs, NameKindEdit, NameRequest,
    NameRequestKind, ScopeChoice, SortCriterion, StripChoice, TagRequest, default_sort_keys,
};
use mychess_app::{AppState, Message as AppMessage};

use crate::shell::{Message, Shell};
use crate::theme;

/// The id of the window.
pub const MAINT_WINDOW_ID: &str = "maint-window";
/// The id of the card of a form.
pub const MAINT_FORM_ID: &str = "maint-form-box";
/// The id of the card of the confirmation.
pub const MAINT_CONFIRM_ID: &str = "maint-confirm-box";
/// The id of the card of the checker of duplicates.
pub const MAINT_CHECKER_ID: &str = "maint-checker-box";
/// The id of the field of the description.
pub const MAINT_DESCRIPTION_ID: &str = "maint-description";
/// The id of the field of the game that opens with the database.
pub const MAINT_AUTOLOAD_ID: &str = "maint-autoload";
/// The id of the field of the name of a tag.
pub const MAINT_TAG_NAME_ID: &str = "maint-tag-name";
/// The id of the field of the value of a tag.
pub const MAINT_TAG_VALUE_ID: &str = "maint-tag-value";
/// The id of the field "Replace" of the name editor.
pub const MAINT_NAME_OLD_ID: &str = "maint-name-old";
/// The id of the field "With" of the name editor.
pub const MAINT_NAME_NEW_ID: &str = "maint-name-new";
/// The id of the first field of the custom flags.
pub const MAINT_CUSTOM_ID: &str = "maint-custom-0";

/// The form that is open over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintForm {
    /// Compact the name file and the game file.
    Compact,
    /// Sort the database.
    Sort,
    /// The criteria of the search for duplicates.
    Twins,
    /// The Cleaner.
    Cleaner,
    /// Check the games.
    Check,
    /// Remove comments or variations.
    Strip,
    /// The extra tags.
    Tags,
    /// The name editor.
    Names,
    /// The game that opens with the database.
    Autoload,
    /// The type of the database.
    BaseType,
    /// The description of the database.
    Description,
    /// The descriptions of the six custom flags.
    CustomFlags,
    /// Repair a database that is not open.
    Repair,
}

/// What the name editor changes (the seven types of the Scid).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NameUiKind {
    /// A player.
    #[default]
    Player,
    /// An event.
    Event,
    /// A site.
    Site,
    /// A round.
    Round,
    /// The rating of a player.
    Rating,
    /// The date of the games.
    Date,
    /// The date of the event.
    EventDate,
}

impl NameUiKind {
    /// Every kind, in the order of the Scid.
    pub const ALL: [NameUiKind; 7] = [
        NameUiKind::Player,
        NameUiKind::Event,
        NameUiKind::Site,
        NameUiKind::Round,
        NameUiKind::Rating,
        NameUiKind::Date,
        NameUiKind::EventDate,
    ];

    /// The key of the name of the kind.
    pub fn key(self) -> &'static str {
        match self {
            NameUiKind::Player => "maint-names-kind-player",
            NameUiKind::Event => "maint-names-kind-event",
            NameUiKind::Site => "maint-names-kind-site",
            NameUiKind::Round => "maint-names-kind-round",
            NameUiKind::Rating => "maint-names-kind-rating",
            NameUiKind::Date => "maint-names-kind-date",
            NameUiKind::EventDate => "maint-names-kind-event-date",
        }
    }
}

/// What is typed in the name editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamesDraft {
    /// The kind.
    pub kind: NameUiKind,
    /// The name (or pattern, or date) to replace.
    pub old: String,
    /// The new name (or date).
    pub new: String,
    /// The Elo, for the rating kind.
    pub elo: String,
    /// `*` and `?` of the old name are wildcards.
    pub pattern: bool,
}

impl NamesDraft {
    /// The request the fields describe, if they describe one: the new name
    /// may not be empty (except for dates), a rating is `0..=4000`.
    pub fn request(&self, prefs: &MaintenancePrefs) -> Option<MaintRequest> {
        let old = self.old.trim().to_owned();
        let kind = match self.kind {
            NameUiKind::Player | NameUiKind::Event | NameUiKind::Site | NameUiKind::Round => {
                let new = self.new.trim().to_owned();
                if old.is_empty() || new.is_empty() {
                    return None;
                }
                let kind = match self.kind {
                    NameUiKind::Player => NameKindEdit::Player,
                    NameUiKind::Event => NameKindEdit::Event,
                    NameUiKind::Site => NameKindEdit::Site,
                    _ => NameKindEdit::Round,
                };
                NameRequestKind::Name { kind, old, new }
            }
            NameUiKind::Rating => {
                let elo = self.elo.trim().parse::<u16>().ok().filter(|e| *e <= 4000)?;
                if old.is_empty() {
                    return None;
                }
                NameRequestKind::Rating {
                    player: old,
                    elo,
                    rating_type: 0,
                }
            }
            NameUiKind::Date | NameUiKind::EventDate => {
                let new = self.new.trim().to_owned();
                if old.is_empty() || new.is_empty() {
                    return None;
                }
                if self.kind == NameUiKind::Date {
                    NameRequestKind::Date { old, new }
                } else {
                    NameRequestKind::EventDate { old, new }
                }
            }
        };
        Some(MaintRequest::Names(NameRequest {
            kind,
            pattern: self.pattern,
            scope: prefs.name_editor_scope,
        }))
    }
}

/// What is typed in the extra tags form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagDraft {
    /// The tag chosen in the list.
    pub selected: Option<String>,
    /// The name of the tag to add.
    pub name: String,
    /// The value of the tag to add.
    pub value: String,
}

/// What is typed in the forms.
#[derive(Debug, Clone)]
pub struct MaintUi {
    /// The form that is open.
    pub form: Option<MaintForm>,
    /// The flag of the group "Mark".
    pub flag: MaintFlag,
    /// The criteria of the sort being built.
    pub sort_keys: Vec<SortCriterion>,
    /// The sort goes backwards.
    pub sort_reverse: bool,
    /// Compact the games after the sort.
    pub sort_compact: bool,
    /// What is removed by "Remove comments/variations".
    pub strip: StripChoice,
    /// The extra tags form.
    pub tags: TagDraft,
    /// The name editor.
    pub names: NamesDraft,
    /// The game that opens with the database, as typed.
    pub autoload: String,
    /// The description, as typed.
    pub description: String,
    /// The custom flags, as typed.
    pub custom: [String; 6],
    /// The database to repair.
    pub repair_path: Option<PathBuf>,
    /// Also drop the entries whose record cannot be read.
    pub repair_drop: bool,
    /// The window was open at the last sync.
    pub was_open: bool,
    /// The checker was already offered for the last marking.
    pub marked_seen: bool,
}

impl Default for MaintUi {
    fn default() -> MaintUi {
        MaintUi {
            form: None,
            flag: MaintFlag::WhiteOpening,
            sort_keys: default_sort_keys(),
            sort_reverse: false,
            sort_compact: false,
            strip: StripChoice::Comments,
            tags: TagDraft::default(),
            names: NamesDraft::default(),
            autoload: String::new(),
            description: String::new(),
            custom: Default::default(),
            repair_path: None,
            repair_drop: false,
            was_open: false,
            marked_seen: false,
        }
    }
}

impl MaintUi {
    /// Whether the window covers the main window. Cheap: it is asked many
    /// times per frame.
    pub fn layer_open(app: &AppState) -> bool {
        app.maintenance().is_open()
    }

    /// Forgets the state of a window that closed.
    pub(crate) fn reset(&mut self, prefs: &MaintenancePrefs) {
        *self = MaintUi {
            sort_keys: prefs.sort_keys.clone(),
            sort_reverse: prefs.sort_reverse,
            ..MaintUi::default()
        };
    }

    /// The sort the draft describes.
    pub fn sort_request(&self) -> Option<MaintRequest> {
        (!self.sort_keys.is_empty()).then(|| MaintRequest::Sort {
            keys: self.sort_keys.clone(),
            reverse: self.sort_reverse,
            compact_after: self.sort_compact,
        })
    }

    /// The tag to add, if the name is not empty.
    pub fn tag_add_request(&self) -> Option<MaintRequest> {
        let name = self.tags.name.trim();
        (!name.is_empty()).then(|| {
            MaintRequest::Tags(TagRequest::Add {
                name: name.to_owned(),
                value: self.tags.value.clone(),
            })
        })
    }
}

/// The common sorts of the Scid: year, month, site, event, date, round /
/// year, event, date, round / ECO, rating.
pub fn common_sort(index: usize) -> Vec<SortCriterion> {
    use SortCriterion as C;
    match index {
        0 => vec![C::Year, C::Month, C::Site, C::Event, C::Date, C::Round],
        1 => vec![C::Year, C::Event, C::Date, C::Round],
        _ => vec![C::Eco, C::Rating],
    }
}

/// An edit of a field of a form.
#[derive(Debug, Clone, PartialEq)]
pub enum MaintEdit {
    /// Adds a criterion to the sort (at most fifteen).
    SortAdd(SortCriterion),
    /// Takes the last criterion out.
    SortRemoveLast,
    /// Clears the criteria.
    SortClear,
    /// Takes one of the three common sorts.
    SortCommon(usize),
    /// The sort goes backwards.
    SortReverse(bool),
    /// Compact the games after the sort.
    SortCompact(bool),
    /// Comments or variations.
    Strip(StripChoice),
    /// The tag chosen in the list.
    TagSelect(Option<String>),
    /// The name of the tag to add.
    TagName(String),
    /// The value of the tag to add.
    TagValue(String),
    /// The kind of the name editor.
    NamesKind(NameUiKind),
    /// The text to replace.
    NamesOld(String),
    /// The new text.
    NamesNew(String),
    /// The Elo.
    NamesElo(String),
    /// The wildcards box.
    NamesPattern(bool),
    /// The game that opens with the database.
    Autoload(String),
    /// The description.
    Description(String),
    /// A custom flag.
    Custom(usize, String),
    /// The box of the repair.
    RepairDrop(bool),
}

/// An event of the window that only the front-end handles, or that needs the
/// state of the front-end to become a message of the application.
#[derive(Debug, Clone)]
pub enum MaintUiMessage {
    /// "Maintenance" of the Tools menu.
    Open,
    /// Closes the window.
    Close,
    /// Opens a form (the window too, if it is closed).
    OpenForm(MaintForm),
    /// Opens "Remove comments" or "Remove variations".
    OpenStrip(StripChoice),
    /// Closes the form.
    CloseForm,
    /// The flag of the group "Mark".
    SelectFlag(MaintFlag),
    /// A message of the maintenance of the application.
    App(MaintenanceMessage),
    /// A request of the application; the form stays.
    Run(MaintRequest),
    /// A request of the application; the form closes.
    RunClose(MaintRequest),
    /// The scope of an operation (a preference).
    Scope(ScopeField, ScopeChoice),
    /// Changes the preferences.
    Prefs(Box<MaintenancePrefs>),
    /// A field was edited.
    Edit(MaintEdit),
    /// Opens the file dialog of the repair.
    PickRepairFile,
    /// The answer of that dialog (`None`: cancelled).
    RepairFilePicked(Option<PathBuf>),
    /// Makes the database read-only for the session, or editable again.
    SetReadOnly(bool),
    /// Opens the window of another tool (ECO classification, Elo, spelling).
    Tool(Box<AppMessage>),
    /// The checker loads a game of the database.
    LoadGame(u32),
    /// Nothing.
    Noop,
}

/// The scope fields of the preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeField {
    /// "Remove comments/variations".
    Strip,
    /// "Extra tags".
    Tags,
}

/// The message of a button.
pub(crate) fn ui(message: MaintUiMessage) -> Message {
    Message::Maint(message)
}

/// The layer to put over the window, if the maintenance window is open.
pub fn layer(shell: &Shell) -> Option<Element<'_, Message>> {
    if !MaintUi::layer_open(&shell.app) {
        return None;
    }
    let view = shell.app.maintenance_view();
    let card = container(
        column![
            main_view::title_bar(shell, &view),
            main_view::job_bar(shell, &view),
            main_view::body(shell, &view),
        ]
        .spacing(8),
    )
    .id(MAINT_WINDOW_ID)
    .padding(14)
    .width(iced::Length::Fill)
    .height(iced::Length::Fill)
    .style(theme::pane_body);
    let backdrop = opaque(
        container(card)
            .padding(18)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .style(theme::scrim),
    );
    let mut layers = stack![backdrop];
    if view.checker_open {
        layers = layers.push(checker::overlay(shell, &view));
    }
    if let Some(form) = forms::overlay(shell, &view) {
        layers = layers.push(form);
    }
    if let Some(confirm) = forms::confirm_overlay(shell, &view) {
        layers = layers.push(confirm);
    }
    Some(layers.into())
}

/// The message the default button of the open card sends (`Enter`). On the
/// destructive confirmation the default is "No" (D4122).
pub fn default_message(shell: &Shell) -> Option<Message> {
    let view = shell.app.maintenance_view();
    if view.confirm.is_some() {
        return Some(ui(MaintUiMessage::App(MaintenanceMessage::Confirm(false))));
    }
    if view.checker_open {
        return None;
    }
    forms::default_message(shell, &view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rating_needs_a_player_and_an_elo_up_to_4000() {
        let prefs = MaintenancePrefs::default();
        let mut draft = NamesDraft {
            kind: NameUiKind::Rating,
            old: "Alfa, A".to_owned(),
            elo: "2600".to_owned(),
            ..NamesDraft::default()
        };
        assert!(draft.request(&prefs).is_some());
        draft.elo = "4001".to_owned();
        assert!(draft.request(&prefs).is_none());
        draft.elo = "x".to_owned();
        assert!(draft.request(&prefs).is_none());
    }

    #[test]
    fn a_name_replacement_needs_both_names() {
        let prefs = MaintenancePrefs::default();
        let mut draft = NamesDraft {
            old: "Gama, G".to_owned(),
            ..NamesDraft::default()
        };
        assert!(draft.request(&prefs).is_none());
        draft.new = "Gama, Gustavo".to_owned();
        let Some(MaintRequest::Names(request)) = draft.request(&prefs) else {
            panic!("a request");
        };
        assert!(!request.pattern);
    }

    #[test]
    fn the_common_sorts_are_the_three_of_the_scid() {
        assert_eq!(common_sort(0).len(), 6);
        assert_eq!(common_sort(1).len(), 4);
        assert_eq!(common_sort(2), [SortCriterion::Eco, SortCriterion::Rating]);
    }

    #[test]
    fn an_empty_sort_or_tag_name_makes_no_request() {
        let mut ui = MaintUi::default();
        ui.sort_keys.clear();
        assert!(ui.sort_request().is_none());
        assert!(ui.tag_add_request().is_none());
        ui.tags.name = "Source".to_owned();
        assert!(ui.tag_add_request().is_some());
    }
}
