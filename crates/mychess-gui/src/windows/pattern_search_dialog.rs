//! The window "Tactics and patterns": the ready-made patterns, the visual
//! editor of the tree of conditions, the scope, the run, the occurrences and
//! the user's patterns (spec M6-2, slice d, D3281-D3284).
//!
//! The application owns every option ([`PatternSearchView`]); this module
//! only draws them and turns the clicks into [`PatternSearchMessage`]. What
//! it keeps is purely visual: the text of the numeric fields while they are
//! edited, whether the "add condition" list is unfolded and whether the
//! window is hidden to show the board.
//!
//! Hiding is the way this modal window is "non modal" (D3281): the
//! application still has the window open (its occurrences and the filter
//! keep following the game), the dialog is gone and a slim bar under the
//! panels keeps the occurrences, the progress and the buttons at hand.

use std::collections::HashMap;

use iced::widget::{
    button, column, container, pick_list, progress_bar, row, scrollable, space, text, text_input,
    tooltip,
};
use iced::{Alignment, Element, Length};
use mychess_app::pattern_search::model::{DiscoveredKind, MateMode, Outcome, Who};
use mychess_app::pattern_search::{
    ConditionGroup, ConditionKind, CostView, GroupKind, NodeView, ParamValue, PatternEdit,
    PatternSearchMessage, PatternSearchView, PatternTab, SavedPattern, param_label_key,
};
use mychess_app::{AppState, SearchOp};

use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, ButtonLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{check, choice, group, labelled};
use crate::windows::header_dialog::Labeled;
use crate::windows::pattern_search_fields::{
    PROMOTIONS, ROLES, card_editor, choices, edit, field, num_stepper, number_input, role_chips,
    send, toggle, who_choices,
};
use crate::windows::pattern_search_text::{
    capitalized, error_text, notice_text, sentence_text, summary_text,
};

/// Id of the box.
pub const PATTERN_BOX_ID: &str = "pattern-search-box";
/// Id of the box that asks about an expensive search.
pub const PATTERN_COST_BOX_ID: &str = "pattern-search-cost-box";
/// Id of the "Search" button.
pub const PATTERN_START_ID: &str = "pattern-start";
/// Id of the "Stop" button.
pub const PATTERN_STOP_ID: &str = "pattern-stop";
/// Id of the "Close" button.
pub const PATTERN_CLOSE_ID: &str = "pattern-close";
/// Id of the "Show the board" button.
pub const PATTERN_HIDE_ID: &str = "pattern-hide";
/// Id of the "Test on this game" button.
pub const PATTERN_TEST_ID: &str = "pattern-test";
/// Id of the "Customize" button.
pub const PATTERN_CUSTOMIZE_ID: &str = "pattern-customize";
/// Id of the "Add condition" button.
pub const PATTERN_ADD_ID: &str = "pattern-add";
/// Id of the "Remove" button of the selected card.
pub const PATTERN_REMOVE_ID: &str = "pattern-remove";
/// Id of the "Yes" button of the question about the cost.
pub const PATTERN_COST_YES_ID: &str = "pattern-cost-yes";
/// Id of the "No" button of the question about the cost.
pub const PATTERN_COST_NO_ID: &str = "pattern-cost-no";
/// Id of the button "previous occurrence".
pub const PATTERN_HIT_PREV_ID: &str = "pattern-hit-prev";
/// Id of the button "next occurrence".
pub const PATTERN_HIT_NEXT_ID: &str = "pattern-hit-next";
/// Id of the button "forget the occurrences".
pub const PATTERN_HIT_CLEAR_ID: &str = "pattern-hit-clear";
/// Id of the button that brings the window back from the bar.
pub const PATTERN_SHOW_ID: &str = "pattern-show";
/// Id of the bar shown while the window is hidden.
pub const PATTERN_BAR_ID: &str = "pattern-bar";
/// Id of the "Save" button.
pub const PATTERN_SAVE_ID: &str = "pattern-save";
/// Id of the "Import" button.
pub const PATTERN_IMPORT_ID: &str = "pattern-import";
/// Id of the "Export" button.
pub const PATTERN_EXPORT_ID: &str = "pattern-export";

/// Id of the button of a tab.
pub const fn tab_id(tab: PatternTab) -> &'static str {
    match tab {
        PatternTab::Presets => "pattern-tab-presets",
        PatternTab::Advanced => "pattern-tab-advanced",
    }
}

/// Id of the button of a ready-made pattern.
pub fn preset_id(id: &str) -> String {
    format!("pattern-preset-{id}")
}

/// Id of the button that adds a condition of a kind.
pub fn add_kind_id(kind: ConditionKind) -> String {
    format!("pattern-add-{kind:?}")
}

/// Id of the card at `path` (`root` for the first).
pub fn card_id(path: &[usize]) -> String {
    if path.is_empty() {
        "pattern-card-root".to_owned()
    } else {
        let parts: Vec<String> = path.iter().map(ToString::to_string).collect();
        format!("pattern-card-{}", parts.join("-"))
    }
}

const BODY_HEIGHT: f32 = 460.0;
const BODY_MIN_HEIGHT: f32 = 160.0;
const CHROME_HEIGHT: f32 = 270.0;
const CARD_WIDTH: f32 = 940.0;
const TREE_WIDTH: f32 = 400.0;
const LIST_WIDTH: f32 = 290.0;
const LIST_HEIGHT: f32 = 300.0;
const SAVED_HEIGHT: f32 = 84.0;
const FIELD_WIDTH: f32 = 52.0;

/// The height of the scrolling form for a window `window_height` tall.
fn body_height(window_height: f32) -> f32 {
    (window_height - CHROME_HEIGHT).clamp(BODY_MIN_HEIGHT, BODY_HEIGHT)
}

/// The numeric fields that can be edited as text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NumField {
    /// First move number of the scope.
    MoveMin,
    /// Last move number of the scope.
    MoveMax,
    /// "Must hold for at least" this many half moves.
    Hold,
    /// Nodes of the mate search of a ready-made pattern.
    ParamNodes,
    /// Nodes of the mate search of the card at this path.
    CardNodes(Vec<usize>),
}

/// An event of the window that is not just a message for the application.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternUiMessage {
    /// A message for the application.
    Send(PatternSearchMessage),
    /// The text of a numeric field changed.
    Number(NumField, String),
    /// Unfolds or folds the list of conditions to add.
    ToggleAdd,
    /// Adds a condition to the group at hand.
    Add(ConditionKind),
    /// Hides the window (the bar stays) or brings it back.
    Hide(bool),
    /// "Customize" the preset on show.
    Customize,
    /// "Save": the pattern on show under its name.
    Save,
    /// "Import": asks for the file.
    PickImport,
    /// The file dialog of the import answered.
    ImportPicked(Option<std::path::PathBuf>),
    /// "Export": asks for the file.
    PickExport,
    /// The file dialog of the export answered.
    ExportPicked(Option<std::path::PathBuf>),
}

/// Which box is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternDialog {
    /// The window.
    Window,
    /// The question about an expensive search.
    Cost,
}

/// The visual state of the window.
#[derive(Debug, Default)]
pub struct PatternSearchUi {
    drafts: HashMap<NumField, String>,
    add_open: bool,
    hidden: bool,
    was_running: bool,
    pending_hide: bool,
}

impl PatternSearchUi {
    /// Which box to show now.
    pub fn dialog(&self, app: &AppState) -> Option<PatternDialog> {
        if !app.pattern_search().is_open() {
            return None;
        }
        if app.pattern_search_confirm().is_some() {
            return Some(PatternDialog::Cost);
        }
        (!self.hidden).then_some(PatternDialog::Window)
    }

    /// Whether the window is hidden behind the bar.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Hides the window or brings it back.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        self.pending_hide = false;
    }

    /// Whether the list of conditions to add is unfolded.
    pub fn add_open(&self) -> bool {
        self.add_open
    }

    /// Folds or unfolds the list of conditions to add.
    pub fn toggle_add(&mut self) {
        self.add_open = !self.add_open;
    }

    /// Forgets the drafts when the window is gone, and hides the window by
    /// itself when a search ends with occurrences to look at.
    pub fn sync(&mut self, app: &AppState) {
        let search = app.pattern_search();
        if !search.is_open() {
            self.drafts.clear();
            self.hidden = false;
            self.add_open = false;
            self.was_running = false;
            self.pending_hide = false;
            return;
        }
        let running = search.is_running();
        if self.was_running && !running {
            self.pending_hide = true;
        }
        self.was_running = running;
        if !self.pending_hide || running {
            return;
        }
        // Nothing found: there is nothing to look at, so the window stays
        // and the wish to hide it is forgotten (D3299 item 45).
        if search.last_matched() == Some(0) {
            self.pending_hide = false;
        } else if !self.hidden
            && app
                .pattern_search_hits()
                .is_some_and(|h| !h.from_test && !h.plies.is_empty())
        {
            self.hidden = true;
            self.pending_hide = false;
        }
    }

    /// Forgets the text of every numeric field.
    pub fn clear_drafts(&mut self) {
        self.drafts.clear();
    }

    /// What a numeric field shows: the draft while it still stands for the
    /// value (or is half typed), else the value.
    pub fn shown(&self, field: &NumField, current: i64) -> String {
        match self.drafts.get(field) {
            Some(draft)
                if draft.is_empty() || draft.trim().parse::<i64>().ok() == Some(current) =>
            {
                draft.clone()
            }
            _ => current.to_string(),
        }
    }

    /// Takes the text typed in a field: the number to apply, or `None` when
    /// the text is half typed or is not a number.
    pub fn type_into(&mut self, field: NumField, typed: &str) -> Option<i64> {
        let typed = typed.trim().to_owned();
        if typed.is_empty() {
            self.drafts.insert(field, typed);
            return None;
        }
        let number: i64 = typed.parse().ok().filter(|n| *n >= 0)?;
        self.drafts.insert(field, typed);
        Some(number)
    }
}

/// The message of the application for "Close".
pub fn close_message() -> mychess_app::Message {
    mychess_app::Message::PatternSearch(PatternSearchMessage::Close)
}

/// The message of the application for "Stop" and Esc during a search.
pub fn stop_message() -> mychess_app::Message {
    mychess_app::Message::PatternSearch(PatternSearchMessage::Stop)
}

/// The message of the application for "Search" and Enter.
pub fn start_message() -> mychess_app::Message {
    mychess_app::Message::PatternSearch(PatternSearchMessage::Start)
}

/// The message of the application for the answer to the question about the
/// cost.
pub fn cost_message(yes: bool) -> mychess_app::Message {
    mychess_app::Message::PatternSearch(PatternSearchMessage::AnswerCost(yes))
}

// ---- small pieces ----

fn ui(message: PatternUiMessage) -> Message {
    Message::PatternUi(message)
}

fn tip(body: Element<'_, Message>, tip: String) -> Element<'_, Message> {
    tooltip(body, text(tip).size(12), tooltip::Position::Top)
        .style(theme::tooltip_box)
        .into()
}

fn with_id<'a>(
    id: impl Into<iced::widget::Id>,
    body: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(body).id(id).into()
}

fn command<'a>(
    glyph: Icon,
    label: String,
    primary: bool,
    message: Option<Message>,
) -> iced::widget::Button<'a, Message> {
    let tone = if message.is_none() {
        IconTone::Disabled
    } else if primary {
        IconTone::OnAccent
    } else {
        IconTone::Text
    };
    button(
        row![icon(glyph, 14.0, tone), text(label).size(13)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(if primary {
        button::primary
    } else {
        button::secondary
    })
    .on_press_maybe(message)
}

fn small_button<'a>(glyph: Icon, message: Option<Message>) -> iced::widget::Button<'a, Message> {
    let tone = if message.is_some() {
        IconTone::Text
    } else {
        IconTone::Disabled
    };
    button(icon(glyph, 14.0, tone))
        .padding(3)
        .style(button::secondary)
        .on_press_maybe(message)
}

fn section(title: String, body: Element<'_, Message>) -> Element<'_, Message> {
    column![text(title).size(13).style(theme::accent_text), body]
        .spacing(6)
        .into()
}

fn ratio(done: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    // A fraction to draw: the precision of an f32 is plenty.
    #[allow(clippy::cast_precision_loss)]
    let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
    ratio
}

fn database_label(d: &mychess_app::DatabaseChoice, i18n: &I18n) -> String {
    if d.clipbase {
        i18n.tr("games-clipbase")
    } else if d.name.is_empty() {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("number", i64::try_from(d.slot.number()).unwrap_or(0));
        i18n.tr_args("games-slot-number", &args)
    } else {
        d.name.clone()
    }
}

// ---- tabs and the ready-made patterns ----

fn tabs<'a>(view: &PatternSearchView, i18n: &I18n) -> Element<'a, Message> {
    let tab = |kind: PatternTab, key: &str| {
        with_id(
            tab_id(kind),
            choice(i18n.tr(key), kind, view.tab, true, |k| {
                send(PatternSearchMessage::SelectTab(k))
            }),
        )
    };
    row![
        tab(PatternTab::Presets, "pattern-tab-presets"),
        tab(PatternTab::Advanced, "pattern-tab-advanced"),
    ]
    .spacing(4)
    .into()
}

fn preset_name(i18n: &I18n, name_key: &str, n: u8) -> String {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("n", i64::from(n));
    i18n.tr_args(name_key, &args)
}

fn param_row<'a>(
    view: &PatternSearchView,
    preset: &str,
    key: mychess_app::pattern_search::model::ParamKey,
    ui_state: &PatternSearchUi,
    i18n: &I18n,
) -> Option<Element<'a, Message>> {
    use mychess_app::pattern_search::model::ParamKey as K;
    let p = &view.params;
    let set = |v: ParamValue| send(PatternSearchMessage::SetParam(v));
    let label = param_label_key(key);
    let body: Element<'a, Message> = match key {
        K::By => who_choices(i18n, p.by, &[Who::White, Who::Black, Who::Any], |w| {
            set(ParamValue::By(w))
        }),
        K::N => group(
            (1..=5u8)
                .map(|n| {
                    with_id(
                        format!("pattern-param-n-{n}"),
                        choice(n.to_string(), n, p.n, true, |n| set(ParamValue::N(n))),
                    )
                })
                .collect(),
        ),
        K::MateMode => {
            let options = [
                (MateMode::Available, i18n.tr("pattern-mate-mode-available")),
                (MateMode::Played, i18n.tr("pattern-mate-mode-played")),
            ];
            choices(&options, p.mate_mode, |m| set(ParamValue::MateMode(m)))
        }
        K::ForcingOnly => {
            return Some(toggle(
                i18n.tr(label),
                p.forcing_only,
                set(ParamValue::ForcingOnly(true)),
                set(ParamValue::ForcingOnly(false)),
            ));
        }
        K::MaxNodes => number_input(ui_state, NumField::ParamNodes, i64::from(p.max_nodes), 80.0),
        K::Roles => {
            let all: &[_] = match preset {
                "promotion" => &PROMOTIONS,
                "sacrifice" => &ROLES[1..],
                _ => &ROLES,
            };
            role_chips(i18n, &p.roles, all, |r| set(ParamValue::Roles(r)))
        }
        K::TargetRoles => role_chips(i18n, &p.target_roles, &ROLES[1..], |r| {
            set(ParamValue::TargetRoles(r))
        }),
        K::AgainstKing => {
            return Some(toggle(
                i18n.tr(label),
                p.against_king,
                set(ParamValue::AgainstKing(true)),
                set(ParamValue::AgainstKing(false)),
            ));
        }
        K::Discovery => {
            let options = [
                (DiscoveredKind::Attack, "pattern-discovery-attack"),
                (DiscoveredKind::Check, "pattern-discovery-check"),
                (
                    DiscoveredKind::DoubleCheck,
                    "pattern-discovery-double-check",
                ),
            ]
            .map(|(v, k)| (v, i18n.tr(k)));
            choices(&options, p.discovery, |d| set(ParamValue::Discovery(d)))
        }
        K::MinNet => num_stepper(i64::from(p.min_net), 1, 20, |n| {
            set(ParamValue::MinNet(u8::try_from(n).unwrap_or(1)))
        }),
        K::Outcome => {
            let options = [
                (Outcome::Either, "pattern-outcome-either"),
                (Outcome::Recovered, "pattern-outcome-recovered"),
                (Outcome::Mate, "pattern-outcome-mate"),
            ]
            .map(|(v, k)| (v, capitalized(&i18n.tr(k))));
            choices(&options, p.outcome, |o| set(ParamValue::Outcome(o)))
        }
        K::Within => num_stepper(i64::from(p.within), 1, 8, |n| {
            set(ParamValue::Within(u8::try_from(n).unwrap_or(1)))
        }),
        K::RequireWin => {
            return Some(toggle(
                i18n.tr(label),
                p.require_win,
                set(ParamValue::RequireWin(true)),
                set(ParamValue::RequireWin(false)),
            ));
        }
        K::MinOwnRank => {
            let mut options: Vec<(Option<u8>, String)> =
                vec![(None, i18n.tr("pattern-param-rank-any"))];
            options.extend((2..=7).map(|n| (Some(n), n.to_string())));
            choices(&options, p.min_own_rank, |r| set(ParamValue::MinOwnRank(r)))
        }
        _ => return None,
    };
    Some(field(i18n, label, body))
}

fn presets_tab<'a>(
    ui_state: &PatternSearchUi,
    view: &PatternSearchView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    for entry in &view.presets {
        let selected = view.selected_preset.as_deref() == Some(entry.id);
        let body = column![
            text(preset_name(i18n, entry.name_key, view.params.n)).size(13),
            text(i18n.tr(&format!("{}-desc", entry.name_key)))
                .size(11)
                .style(theme::muted_text),
        ]
        .spacing(1);
        list = list.push(with_id(
            preset_id(entry.id),
            button(body)
                .padding([3, 8])
                .width(Length::Fill)
                .style(theme::command_button(ButtonLook::Secondary, selected))
                .on_press(send(PatternSearchMessage::SelectPreset(
                    entry.id.to_owned(),
                ))),
        ));
    }
    let mut details = column![].spacing(10);
    if let Some(entry) = view
        .presets
        .iter()
        .find(|e| view.selected_preset.as_deref() == Some(e.id))
    {
        details = details.push(
            text(preset_name(i18n, entry.name_key, view.params.n))
                .size(16)
                .style(theme::accent_text),
        );
        details = details.push(
            text(i18n.tr(&format!("{}-desc", entry.name_key)))
                .size(12)
                .style(theme::muted_text),
        );
        for key in &entry.params {
            if let Some(row) = param_row(view, entry.id, *key, ui_state, i18n) {
                details = details.push(row);
            }
        }
        details = details.push(with_id(
            PATTERN_CUSTOMIZE_ID,
            tip(
                command(
                    Icon::Manage,
                    i18n.tr("pattern-customize"),
                    false,
                    Some(ui(PatternUiMessage::Customize)),
                )
                .into(),
                i18n.tr("pattern-customize-tip"),
            ),
        ));
    }
    details = details.push(
        text(i18n.tr("pattern-presets-hint"))
            .size(12)
            .style(theme::muted_text),
    );
    row![
        container(scrollable(list).width(Length::Fill))
            .width(Length::Fixed(LIST_WIDTH))
            .height(Length::Fixed(LIST_HEIGHT)),
        container(details).width(Length::Fill),
    ]
    .spacing(14)
    .into()
}

// ---- the editor ----

fn card_text(node: &NodeView, i18n: &I18n) -> String {
    sentence_text(i18n, &node.sentence)
}

fn tree_column<'a>(view: &PatternSearchView, i18n: &I18n) -> Element<'a, Message> {
    let mut cards = column![].spacing(2);
    if view.tree.is_empty() {
        cards = cards.push(
            text(i18n.tr("pattern-empty-tree"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    for node in &view.tree {
        let mut label = text(card_text(node, i18n)).size(13);
        if node.has_issue {
            label = label.style(text::danger);
        }
        let card = button(label)
            .padding([3, 8])
            .width(Length::Fill)
            .style(theme::command_button(ButtonLook::Secondary, node.selected))
            .on_press(edit(PatternEdit::Select(Some(node.path.clone()))));
        let indent = f32::from(u16::try_from(node.depth.min(8)).unwrap_or(8)) * 14.0;
        cards = cards.push(row![
            space::horizontal().width(Length::Fixed(indent)),
            with_id(card_id(&node.path), card)
        ]);
    }
    cards.into()
}

fn selected_actions<'a>(view: &PatternSearchView, i18n: &I18n) -> Element<'a, Message> {
    let Some(node) = view.tree.iter().find(|n| n.selected) else {
        return text(i18n.tr("pattern-pick-card"))
            .size(12)
            .style(theme::muted_text)
            .into();
    };
    let path = node.path.clone();
    let is_root = path.is_empty();
    let (parent_path, index) = match path.split_last() {
        Some((last, rest)) => (rest.to_vec(), *last),
        None => (Vec::new(), 0),
    };
    let parent = view.tree.iter().find(|n| n.path == parent_path);
    let in_list = !is_root
        && parent.is_some_and(|p| matches!(p.kind, ConditionKind::All | ConditionKind::Any));
    let siblings = parent.map_or(0, |p| p.children);
    let wrap = |kind: GroupKind, key: &str| {
        small_text_button(
            i18n.tr(key),
            Some(edit(PatternEdit::Wrap {
                path: path.clone(),
                kind,
            })),
        )
    };
    row![
        with_id(
            PATTERN_REMOVE_ID,
            command(
                Icon::Trash,
                i18n.tr("pattern-remove"),
                false,
                (!is_root).then(|| edit(PatternEdit::Remove(path.clone()))),
            )
        ),
        small_button(
            Icon::SortUp,
            (in_list && index > 0).then(|| edit(PatternEdit::Move {
                path: path.clone(),
                parent: parent_path.clone(),
                index: index - 1,
            })),
        ),
        small_button(
            Icon::SortDown,
            (in_list && index + 1 < siblings).then(|| edit(PatternEdit::Move {
                path: path.clone(),
                parent: parent_path.clone(),
                index: index + 1,
            })),
        ),
        text(i18n.tr("pattern-wrap")).size(12),
        wrap(GroupKind::All, "pattern-group-short-all"),
        wrap(GroupKind::Any, "pattern-group-short-any"),
        wrap(GroupKind::Not, "pattern-group-short-not"),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn small_text_button<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([2, 8])
        .style(button::secondary)
        .on_press_maybe(message)
        .into()
}

fn add_panel<'a>(ui_state: &PatternSearchUi, i18n: &I18n) -> Element<'a, Message> {
    let head = with_id(
        PATTERN_ADD_ID,
        command(
            Icon::BookmarkAdd,
            i18n.tr("pattern-add-condition"),
            false,
            Some(ui(PatternUiMessage::ToggleAdd)),
        ),
    );
    let mut panel = column![head].spacing(6);
    if ui_state.add_open() {
        for g in [
            ConditionGroup::Logic,
            ConditionGroup::Pieces,
            ConditionGroup::Relations,
            ConditionGroup::Tactics,
            ConditionGroup::Structure,
            ConditionGroup::Moves,
        ] {
            let buttons: Vec<Element<'a, Message>> = ConditionKind::ALL
                .iter()
                .filter(|k| k.group() == g)
                .map(|k| {
                    with_id(
                        add_kind_id(*k),
                        small_text_button(
                            i18n.tr(k.title_key()),
                            Some(ui(PatternUiMessage::Add(*k))),
                        ),
                    )
                })
                .collect();
            panel = panel.push(
                column![
                    text(i18n.tr(g.title_key()))
                        .size(12)
                        .style(theme::muted_text),
                    group(buttons)
                ]
                .spacing(2),
            );
        }
    }
    panel.into()
}

fn advanced_tab<'a>(
    ui_state: &PatternSearchUi,
    view: &PatternSearchView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let description = view.draft.description.clone();
    let name = view.draft.name.clone();
    let name_input = text_input(&i18n.tr("pattern-name-placeholder"), &view.draft.name)
        .size(13)
        .padding([3, 6])
        .on_input(move |n| {
            edit(PatternEdit::SetMeta {
                name: n,
                description: description.clone(),
            })
        });
    let description_input = text_input(
        &i18n.tr("pattern-description-placeholder"),
        &view.draft.description,
    )
    .size(13)
    .padding([3, 6])
    .on_input(move |d| {
        edit(PatternEdit::SetMeta {
            name: name.clone(),
            description: d,
        })
    });
    let editor: Element<'a, Message> = match view.tree.iter().find(|n| n.selected) {
        Some(node) => {
            let mut panel = column![card_editor(ui_state, node, i18n)].spacing(8);
            if node.has_issue {
                panel = panel.push(
                    text(i18n.tr("pattern-issue-here"))
                        .size(12)
                        .style(text::danger),
                );
            }
            panel.into()
        }
        None => text(i18n.tr("pattern-pick-card"))
            .size(12)
            .style(theme::muted_text)
            .into(),
    };
    let tree = column![
        add_panel(ui_state, i18n),
        tree_column(view, i18n),
        selected_actions(view, i18n)
    ]
    .spacing(8)
    .width(Length::Fixed(TREE_WIDTH));
    column![
        row![name_input, description_input].spacing(8),
        row![tree, container(editor).width(Length::Fill)].spacing(14),
    ]
    .spacing(10)
    .into()
}

// ---- scope and the rest of the form ----

fn op_row<'a>(current: SearchOp, i18n: &I18n) -> Element<'a, Message> {
    let op = |value: SearchOp, key: &str| {
        choice(i18n.tr(key), value, current, true, |v| {
            send(PatternSearchMessage::SetOp(v))
        })
    };
    labelled(
        i18n.tr("pattern-op"),
        group(vec![
            op(SearchOp::Reset, "pattern-op-reset"),
            op(SearchOp::And, "pattern-op-and"),
            op(SearchOp::Or, "pattern-op-or"),
        ]),
    )
}

fn reference_row<'a>(view: &PatternSearchView, i18n: &I18n) -> Element<'a, Message> {
    let first = view.databases.first().map(|d| d.slot);
    let options: Vec<Labeled<mychess_app::SlotId>> = view
        .databases
        .iter()
        .map(|d| Labeled {
            value: d.slot,
            label: database_label(d, i18n),
        })
        .collect();
    let current = view
        .reference
        .and_then(|slot| options.iter().find(|o| o.value == slot).cloned());
    let toggle = check(
        i18n.tr("pattern-reference"),
        view.reference.is_some(),
        first.is_some(),
        move |on| {
            send(PatternSearchMessage::SetReference(if on {
                first
            } else {
                None
            }))
        },
    );
    let list = pick_list(options, current, |l: Labeled<mychess_app::SlotId>| {
        send(PatternSearchMessage::SetReference(Some(l.value)))
    })
    .placeholder(i18n.tr("pattern-reference-none"))
    .text_size(12)
    .padding(3)
    .width(Length::Fixed(200.0));
    row![toggle, list]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

fn scope_block<'a>(
    ui_state: &PatternSearchUi,
    view: &PatternSearchView,
    i18n: &I18n,
) -> Element<'a, Message> {
    let scope = &view.scope;
    let ranges = !scope.end_only;
    let numeric = |field: NumField, value: i64, enabled: bool| -> Element<'a, Message> {
        if enabled {
            number_input(ui_state, field, value, FIELD_WIDTH)
        } else {
            text_input("", &value.to_string())
                .size(13)
                .padding([2, 4])
                .width(Length::Fixed(FIELD_WIDTH))
                .into()
        }
    };
    let moves = row![
        numeric(NumField::MoveMin, i64::from(scope.min_move), ranges),
        text("-").size(13),
        numeric(NumField::MoveMax, i64::from(scope.max_move), ranges),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("count", i64::from(scope.hold));
    let hold = row![
        numeric(NumField::Hold, i64::from(scope.hold), ranges),
        text(i18n.tr_args("pattern-hold-unit", &args)).size(13),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let with = |f: fn(&mut mychess_app::pattern_search::model::Scope, bool), on: bool| {
        let mut s = scope.clone();
        f(&mut s, on);
        send(PatternSearchMessage::SetScope(s))
    };
    section(
        i18n.tr("pattern-scope-title"),
        column![
            op_row(view.op, i18n),
            reference_row(view, i18n),
            labelled(i18n.tr("pattern-move-range"), moves.into()),
            labelled(i18n.tr("pattern-hold"), hold.into()),
            row![
                toggle(
                    i18n.tr("pattern-end-only"),
                    scope.end_only,
                    with(|s, v| s.end_only = v, true),
                    with(|s, v| s.end_only = v, false),
                ),
                toggle(
                    i18n.tr("pattern-ignore-colors"),
                    scope.ignore_colors,
                    with(|s, v| s.ignore_colors = v, true),
                    with(|s, v| s.ignore_colors = v, false),
                ),
            ]
            .spacing(16),
        ]
        .spacing(8)
        .into(),
    )
}

// ---- occurrences ----

/// The line of the occurrences in the game: the number, the arrows and the
/// button that forgets them. `None` without occurrences to show.
pub fn hits_row<'a>(view: &PatternSearchView, i18n: &I18n) -> Option<Element<'a, Message>> {
    let hits = view.hits.as_ref()?;
    let total = hits.plies.len();
    let label = if total == 0 {
        i18n.tr("pattern-hit-none")
    } else {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("n", i64::try_from(hits.current + 1).unwrap_or(1));
        args.set("total", i64::try_from(total).unwrap_or(0));
        i18n.tr_args(
            if hits.from_test {
                "pattern-occurrence-test"
            } else {
                "pattern-occurrence"
            },
            &args,
        )
    };
    let nav = |id: &'static str, glyph: Icon, key: &str, message: PatternSearchMessage| {
        with_id(
            id,
            tip(
                small_button(glyph, (total > 0).then(|| send(message))).into(),
                i18n.tr(key),
            ),
        )
    };
    Some(
        row![
            text(label).size(13),
            nav(
                PATTERN_HIT_PREV_ID,
                Icon::Back,
                "pattern-hit-prev",
                PatternSearchMessage::PrevHit
            ),
            nav(
                PATTERN_HIT_NEXT_ID,
                Icon::Forward,
                "pattern-hit-next",
                PatternSearchMessage::NextHit
            ),
            nav(
                PATTERN_HIT_CLEAR_ID,
                Icon::Close,
                "pattern-hit-clear",
                PatternSearchMessage::ClearHits
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
    )
}

fn progress_row<'a>(view: &PatternSearchView, i18n: &I18n) -> Option<Element<'a, Message>> {
    let running = view.running.as_ref()?;
    let (done, total) = running.progress;
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("done", i64::try_from(done).unwrap_or(i64::MAX));
    args.set("total", i64::try_from(total).unwrap_or(i64::MAX));
    Some(
        row![
            progress_bar(0.0..=1.0, ratio(done, total)).girth(8.0),
            text(i18n.tr_args("pattern-progress", &args)).size(12),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
    )
}

fn status_lines<'a>(view: &PatternSearchView, i18n: &I18n) -> Element<'a, Message> {
    let mut lines = column![].spacing(2);
    if let Some(progress) = progress_row(view, i18n) {
        lines = lines.push(progress);
    }
    if let Some(hits) = hits_row(view, i18n) {
        lines = lines.push(hits);
    }
    let form_issue = match view.tab {
        PatternTab::Advanced => view.tree_issue.as_ref(),
        PatternTab::Presets => view.param_issue.as_ref(),
    };
    if let Some(issue) = form_issue {
        lines = lines.push(text(notice_text(i18n, issue)).size(13).style(text::danger));
    }
    if let Some(issue) = &view.issue {
        let line = text(notice_text(i18n, issue)).size(13);
        lines = lines.push(if issue.is_form_error() {
            line.style(text::danger)
        } else {
            line
        });
    }
    if let Some(summary) = &view.summary {
        lines = lines.push(
            text(summary_text(i18n, summary))
                .size(13)
                .style(theme::muted_text),
        );
    }
    lines.into()
}

// ---- the user's patterns ----

fn saved_block<'a>(view: &PatternSearchView, i18n: &I18n) -> Element<'a, Message> {
    let mut list = column![].spacing(2);
    if view.saved.is_empty() {
        list = list.push(
            text(i18n.tr("pattern-saved-empty"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    for entry in &view.saved {
        list = list.push(saved_row(entry, i18n));
    }
    let can_save = view.can_save
        && match view.tab {
            PatternTab::Advanced => !view.draft.name.trim().is_empty(),
            PatternTab::Presets => view.selected_preset.is_some(),
        };
    let actions = row![
        with_id(
            PATTERN_SAVE_ID,
            tip(
                command(
                    Icon::Save,
                    i18n.tr("pattern-save"),
                    false,
                    can_save.then(|| ui(PatternUiMessage::Save)),
                )
                .into(),
                i18n.tr("pattern-save-tip"),
            ),
        ),
        with_id(
            PATTERN_IMPORT_ID,
            command(
                Icon::Import,
                i18n.tr("pattern-import"),
                false,
                Some(ui(PatternUiMessage::PickImport)),
            )
        ),
        with_id(
            PATTERN_EXPORT_ID,
            command(
                Icon::Upload,
                i18n.tr("pattern-export"),
                false,
                can_save.then(|| ui(PatternUiMessage::PickExport)),
            )
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    section(
        i18n.tr("pattern-saved-title"),
        column![
            container(scrollable(list).width(Length::Fill)).height(Length::Fixed(SAVED_HEIGHT)),
            actions,
            if view.can_save {
                Element::from(space::vertical().height(Length::Fixed(0.0)))
            } else {
                text(i18n.tr("pattern-saved-no-folder"))
                    .size(12)
                    .style(text::danger)
                    .into()
            },
        ]
        .spacing(6)
        .into(),
    )
}

fn saved_row<'a>(entry: &SavedPattern, i18n: &I18n) -> Element<'a, Message> {
    let title = if entry.damaged {
        format!("{} {}", entry.name, i18n.tr("pattern-saved-damaged"))
    } else {
        entry.name.clone()
    };
    let load = button(text(title).size(13))
        .padding([2, 8])
        .width(Length::Fill)
        .style(button::text)
        .on_press_maybe(
            (!entry.damaged).then(|| send(PatternSearchMessage::LoadSaved(entry.name.clone()))),
        );
    let delete = small_button(
        Icon::Trash,
        Some(send(if entry.damaged {
            PatternSearchMessage::DeleteSavedFile(entry.path.clone())
        } else {
            PatternSearchMessage::DeleteSaved(entry.name.clone())
        })),
    );
    row![
        tip(load.into(), i18n.tr("pattern-saved-load-tip")),
        tip(delete.into(), i18n.tr("pattern-saved-delete-tip"))
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

// ---- the window ----

/// The window.
pub fn view<'a>(
    ui_state: &PatternSearchUi,
    view: &PatternSearchView,
    i18n: &I18n,
    window_height: f32,
) -> Element<'a, Message> {
    let tab_body = match view.tab {
        PatternTab::Advanced => advanced_tab(ui_state, view, i18n),
        PatternTab::Presets => presets_tab(ui_state, view, i18n),
    };
    let form = column![
        tab_body,
        scope_block(ui_state, view, i18n),
        saved_block(view, i18n)
    ]
    .spacing(14)
    .padding([0, 12]);
    let body = column![
        tabs(view, i18n),
        scrollable(form).height(Length::Fixed(body_height(window_height))),
        status_lines(view, i18n),
    ]
    .spacing(8);
    let running = view.running.is_some();
    modal(Card {
        id: PATTERN_BOX_ID,
        title: i18n.tr("pattern-title"),
        body: body.into(),
        buttons: vec![
            with_id(
                PATTERN_CLOSE_ID,
                command(
                    Icon::Close,
                    i18n.tr("pattern-close"),
                    false,
                    Some(send(PatternSearchMessage::Close)),
                ),
            ),
            with_id(
                PATTERN_HIDE_ID,
                tip(
                    command(
                        Icon::Target,
                        i18n.tr("pattern-hide"),
                        false,
                        Some(ui(PatternUiMessage::Hide(true))),
                    )
                    .into(),
                    i18n.tr("pattern-hide-tip"),
                ),
            ),
            with_id(
                PATTERN_TEST_ID,
                tip(
                    command(
                        Icon::Trial,
                        i18n.tr("pattern-test-game"),
                        false,
                        (!running).then(|| send(PatternSearchMessage::TestGame)),
                    )
                    .into(),
                    i18n.tr("pattern-test-game-tip"),
                ),
            ),
            with_id(
                PATTERN_STOP_ID,
                tip(
                    command(
                        Icon::Pause,
                        i18n.tr("pattern-stop"),
                        false,
                        running.then(|| send(PatternSearchMessage::Stop)),
                    )
                    .into(),
                    i18n.tr("pattern-stop-tip"),
                ),
            ),
            with_id(
                PATTERN_START_ID,
                command(
                    Icon::Search,
                    i18n.tr("pattern-run"),
                    true,
                    Some(send(PatternSearchMessage::Start)),
                ),
            ),
        ],
        width: CARD_WIDTH,
    })
}

/// The question about an expensive search (D3285).
pub fn cost_view<'a>(cost: CostView, i18n: &I18n) -> Element<'a, Message> {
    let (positions, games) = (cost.estimated_positions, cost.games);
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("positions", i64::try_from(positions).unwrap_or(i64::MAX));
    args.set("games", i64::from(games));
    modal(Card {
        id: PATTERN_COST_BOX_ID,
        title: i18n.tr("pattern-cost-title"),
        body: text(i18n.tr_args("pattern-cost-warning", &args))
            .size(14)
            .into(),
        buttons: vec![
            with_id(
                PATTERN_COST_NO_ID,
                command(
                    Icon::Close,
                    i18n.tr("pattern-cost-no"),
                    false,
                    Some(send(PatternSearchMessage::AnswerCost(false))),
                ),
            ),
            with_id(
                PATTERN_COST_YES_ID,
                command(
                    Icon::Search,
                    i18n.tr("pattern-cost-yes"),
                    true,
                    Some(send(PatternSearchMessage::AnswerCost(true))),
                ),
            ),
        ],
        width: 460.0,
    })
}

/// The slim bar under the panels while the window is hidden: the way back,
/// the progress and the occurrences in the game (D3284).
pub fn bar(shell: &Shell) -> Option<Element<'_, Message>> {
    if !shell.app.pattern_search().is_open() || !shell.ui.dialogs.pattern.is_hidden() {
        return None;
    }
    let view = shell.app.pattern_search_view()?;
    let i18n = &shell.i18n;
    let mut line = row![with_id(
        PATTERN_SHOW_ID,
        tip(
            command(
                Icon::Search,
                i18n.tr("pattern-show"),
                false,
                Some(ui(PatternUiMessage::Hide(false))),
            )
            .into(),
            i18n.tr("pattern-show-tip"),
        ),
    )]
    .spacing(10)
    .align_y(Alignment::Center);
    if let Some(progress) = progress_row(&view, i18n) {
        line = line.push(progress);
        line = line.push(with_id(
            PATTERN_STOP_ID,
            small_text_button(
                i18n.tr("pattern-stop"),
                Some(send(PatternSearchMessage::Stop)),
            ),
        ));
    }
    if let Some(hits) = hits_row(&view, i18n) {
        line = line.push(hits);
    }
    if let Some(issue) = &view.issue {
        line = line.push(text(notice_text(i18n, issue)).size(12));
    }
    if let Some(issue) = &view.tree_issue
        && view.tab == PatternTab::Advanced
    {
        line = line.push(
            text(error_text_of(i18n, issue))
                .size(12)
                .style(text::danger),
        );
    }
    line = line.push(space::horizontal());
    line = line.push(with_id(
        PATTERN_CLOSE_ID,
        small_button(Icon::Close, Some(send(PatternSearchMessage::Close))),
    ));
    Some(with_id(
        PATTERN_BAR_ID,
        container(line)
            .padding([4, 10])
            .width(Length::Fill)
            .style(theme::status_bar),
    ))
}

fn error_text_of(i18n: &I18n, notice: &mychess_app::pattern_search::PatternSearchNotice) -> String {
    match notice {
        mychess_app::pattern_search::PatternSearchNotice::InvalidPattern(issue) => {
            error_text(i18n, issue)
        }
        other => notice_text(i18n, other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_shrinks_with_a_low_window_and_stays_readable() {
        assert!((body_height(1200.0) - BODY_HEIGHT).abs() < f32::EPSILON);
        assert!((body_height(600.0) - 330.0).abs() < f32::EPSILON);
        assert!((body_height(100.0) - BODY_MIN_HEIGHT).abs() < f32::EPSILON);
    }

    #[test]
    fn a_numeric_field_keeps_a_half_typed_value_and_refuses_minus() {
        let mut ui = PatternSearchUi::default();
        assert_eq!(ui.type_into(NumField::Hold, ""), None);
        assert_eq!(ui.shown(&NumField::Hold, 3), "");
        assert_eq!(ui.type_into(NumField::Hold, "-2"), None);
        assert_eq!(ui.type_into(NumField::Hold, "12"), Some(12));
        assert_eq!(ui.shown(&NumField::Hold, 12), "12");
        // The value moved under the draft: the value wins.
        assert_eq!(ui.shown(&NumField::Hold, 5), "5");
        ui.clear_drafts();
        assert_eq!(ui.shown(&NumField::Hold, 5), "5");
    }

    #[test]
    fn card_ids_follow_the_path() {
        assert_eq!(card_id(&[]), "pattern-card-root");
        assert_eq!(card_id(&[1, 0]), "pattern-card-1-0");
    }
}
