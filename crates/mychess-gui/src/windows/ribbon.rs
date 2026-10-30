//! The ribbon: the second view of [`menu_specs`](super::menu_bar::menu_specs)
//! (spec M9-2 section 3, D1622).
//!
//! A row of tabs (the menus) and, under it, the groups of the active tab as
//! large icon buttons. Nothing is described twice: [`groups`] and
//! [`ribbon_buttons`] read the same [`MenuEntry`] list the dropdown walks, and
//! every button sends the very message of its item. The model half of this
//! module is pure, so the tests read it without drawing anything.
//!
//! Limits of iced 0.14 (D1639): buttons are not reachable with Tab and have no
//! accessibility tree, so the name of a button is its visible label and its
//! tooltip; the keyboard shortcuts of the application keep working.

use fluent_bundle::FluentArgs;
use iced::widget::{Space, button, column, container, mouse_area, row, scrollable, text, tooltip};
use iced::{Alignment, Element, Length};
use iced_aw::menu::{Item, MenuBar};

use mychess_app::Binding;

use crate::i18n::I18n;
use crate::keymap::{Platform, binding_label};
use crate::shell::{Message, UiMessage};
use crate::theme::{self, ButtonLook, TabLook};
use crate::widgets::icon::{Icon, IconTone, icon};
use crate::windows::menu_bar::{self, Label, MenuEntry, MenuSpec};

/// Height of the row of tabs.
pub const TAB_ROW_HEIGHT: f32 = 32.0;
/// Width of a large button.
const LARGE_WIDTH: f32 = 88.0;
/// Height of the buttons of a group (the group title goes under them).
const LARGE_HEIGHT: f32 = 68.0;
/// Size of the icon of a large button.
const LARGE_ICON: f32 = 24.0;
/// Width of a small button (a command that repeats, like "Start engine 2").
const SMALL_WIDTH: f32 = 132.0;
/// Height of a small button.
const SMALL_HEIGHT: f32 = 22.0;
/// Height of the title of a group.
const TITLE_HEIGHT: f32 = 14.0;

/// What the ribbon asks of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonMessage {
    /// A tab was pressed.
    SelectTab(usize),
    /// Collapse or expand the ribbon (the arrow, or a double click on a tab).
    ToggleCollapsed,
    /// A click outside the temporary ribbon.
    Dismiss,
    /// A click on the panel itself (between buttons, on a group title): it
    /// is spent there, so it does not reach the board, and changes nothing.
    Hold,
}

/// The visual state of the ribbon; whether it is collapsed is a preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RibbonUi {
    /// The active tab (index into the menus).
    pub tab: usize,
    /// A collapsed ribbon is showing its groups for a moment.
    pub flyout: bool,
}

impl RibbonUi {
    /// Applies `message`; `collapsed` is the preference now. Returns the new
    /// value of the preference when it must change.
    pub fn update(&mut self, message: RibbonMessage, collapsed: bool) -> Option<bool> {
        match message {
            RibbonMessage::SelectTab(index) => {
                // Pressing the open tab again closes the temporary groups.
                self.flyout = collapsed && !(self.flyout && self.tab == index);
                self.tab = index;
                None
            }
            RibbonMessage::ToggleCollapsed => {
                self.flyout = false;
                Some(!collapsed)
            }
            RibbonMessage::Dismiss => {
                self.flyout = false;
                None
            }
            RibbonMessage::Hold => None,
        }
    }

    /// Whether the groups are on screen: expanded, or collapsed but
    /// showing temporarily.
    pub fn shows_groups(&self, collapsed: bool) -> bool {
        !collapsed || self.flyout
    }
}

/// How a button behaves.
#[derive(Debug, Clone)]
pub enum ButtonKind {
    /// Runs its command.
    Command,
    /// A switch: `true` when on.
    Toggle(bool),
    /// Opens the dropdown list of a submenu.
    Menu(Vec<MenuEntry>),
}

/// A button of the ribbon, read from one entry of a menu.
#[derive(Debug, Clone)]
pub struct RibbonButton {
    /// The text.
    pub label: Label,
    /// The icon.
    pub icon: Icon,
    /// The shortcut shown in the tooltip.
    pub shortcut: Option<Binding>,
    /// What it does; `None` is a disabled button.
    pub action: Option<Message>,
    /// How it behaves.
    pub kind: ButtonKind,
    /// Drawn small, in a column with its neighbours.
    pub small: bool,
}

/// A group of buttons with a title.
#[derive(Debug, Clone)]
pub struct RibbonGroup {
    /// Fluent key of the title (`None` for a group made by a separator).
    pub title: Option<&'static str>,
    /// The buttons.
    pub buttons: Vec<RibbonButton>,
}

/// Splits the entries of a menu into groups: a [`MenuEntry::Group`] starts a
/// titled group and a separator an untitled one. Empty groups are dropped.
pub fn groups(entries: &[MenuEntry]) -> Vec<RibbonGroup> {
    let mut out = Vec::new();
    let mut current = RibbonGroup {
        title: None,
        buttons: Vec::new(),
    };
    for entry in entries {
        let next_title = match entry {
            MenuEntry::Group(key) => Some(Some(*key)),
            MenuEntry::Separator => Some(None),
            _ => None,
        };
        match next_title {
            Some(title) => {
                let done = std::mem::replace(
                    &mut current,
                    RibbonGroup {
                        title,
                        buttons: Vec::new(),
                    },
                );
                if !done.buttons.is_empty() {
                    out.push(done);
                }
            }
            None => current
                .buttons
                .extend(ribbon_buttons(std::slice::from_ref(entry))),
        }
    }
    if !current.buttons.is_empty() {
        out.push(current);
    }
    out
}

/// The buttons of `entries` (dividers are skipped).
pub fn ribbon_buttons(entries: &[MenuEntry]) -> Vec<RibbonButton> {
    entries.iter().filter_map(button_of).collect()
}

fn button_of(entry: &MenuEntry) -> Option<RibbonButton> {
    match entry {
        MenuEntry::Item {
            label,
            shortcut,
            action,
            checked,
            icon,
        } => Some(RibbonButton {
            label: label.clone(),
            // Only the line of the account has no icon at the top level.
            icon: icon.unwrap_or(Icon::Avatar),
            shortcut: *shortcut,
            action: action.clone(),
            kind: checked.map_or(ButtonKind::Command, ButtonKind::Toggle),
            small: matches!(label, Label::Numbered(..)),
        }),
        MenuEntry::Submenu {
            label,
            entries,
            icon,
        } => Some(RibbonButton {
            label: label.clone(),
            icon: icon.unwrap_or(Icon::More),
            shortcut: None,
            action: None,
            kind: ButtonKind::Menu(entries.clone()),
            small: false,
        }),
        MenuEntry::Separator | MenuEntry::Group(_) => None,
    }
}

/// Wraps every action in [`UiMessage::RibbonPick`], so choosing something in
/// the temporary ribbon also closes it.
fn wrap_picks(entries: &mut [MenuEntry]) {
    for entry in entries {
        match entry {
            MenuEntry::Item { action, .. } => {
                if let Some(message) = action.take() {
                    *action = Some(Message::Ui(UiMessage::RibbonPick(Box::new(message))));
                }
            }
            MenuEntry::Submenu { entries, .. } => wrap_picks(entries),
            MenuEntry::Separator | MenuEntry::Group(_) => {}
        }
    }
}

fn ribbon_message(message: RibbonMessage) -> Message {
    Message::Ui(UiMessage::Ribbon(message))
}

/// The tooltip of a button: the label and the shortcut of the platform.
pub fn tip_text(i18n: &I18n, label: &str, shortcut: Option<Binding>, platform: Platform) -> String {
    match shortcut {
        Some(binding) => {
            let mut args = FluentArgs::new();
            args.set("label", label.to_owned());
            args.set("shortcut", binding_label(&binding, platform));
            i18n.tr_args("nav-tip", &args)
        }
        None => label.to_owned(),
    }
}

/// What the drawing functions need.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    i18n: &'a I18n,
    platform: Platform,
}

fn with_tip(body: Element<'_, Message>, tip: String) -> Element<'_, Message> {
    tooltip(
        body,
        container(text(tip).size(12))
            .padding([4, 8])
            .style(theme::tooltip_box),
        tooltip::Position::Bottom,
    )
    .into()
}

fn tone(enabled: bool, on: bool) -> IconTone {
    if !enabled {
        IconTone::Disabled
    } else if on {
        IconTone::OnAccent
    } else {
        IconTone::Text
    }
}

fn label_style(enabled: bool) -> impl Fn(&iced::Theme) -> text::Style {
    move |theme| {
        if enabled {
            text::Style::default()
        } else {
            theme::muted_text(theme)
        }
    }
}

fn large_button<'a>(b: &RibbonButton, ctx: Ctx<'_>) -> Element<'a, Message> {
    let label = menu_bar::label_text(&b.label, ctx.i18n);
    let enabled = b.action.is_some();
    let on = matches!(b.kind, ButtonKind::Toggle(true));
    let caption = container(
        text(label.clone())
            .size(12)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .style(label_style(enabled)),
    )
    .height(Length::Fixed(32.0))
    .clip(true);
    let content = column![icon(b.icon, LARGE_ICON, tone(enabled, on)), caption]
        .spacing(2)
        .align_x(Alignment::Center);
    let body = button(container(content).center_x(Length::Fill).padding([4, 2]))
        .padding(0)
        .width(Length::Fixed(LARGE_WIDTH))
        .height(Length::Fixed(LARGE_HEIGHT))
        .style(theme::command_button(ButtonLook::Secondary, on))
        .on_press_maybe(b.action.clone());
    with_tip(
        body.into(),
        tip_text(ctx.i18n, &label, b.shortcut, ctx.platform),
    )
}

fn small_button<'a>(b: &RibbonButton, ctx: Ctx<'_>) -> Element<'a, Message> {
    let label = menu_bar::label_text(&b.label, ctx.i18n);
    let enabled = b.action.is_some();
    let on = matches!(b.kind, ButtonKind::Toggle(true));
    let content = iced::widget::row![
        icon(b.icon, 16.0, tone(enabled, on)),
        text(label.clone()).size(12).style(label_style(enabled))
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let body = button(content)
        .padding([2, 6])
        .width(Length::Fixed(SMALL_WIDTH))
        .height(Length::Fixed(SMALL_HEIGHT))
        .style(theme::command_button(ButtonLook::Secondary, on))
        .on_press_maybe(b.action.clone());
    with_tip(
        body.into(),
        tip_text(ctx.i18n, &label, b.shortcut, ctx.platform),
    )
}

/// A button that opens the dropdown list of a submenu (the renderer of the
/// dropdown, [`menu_bar::build_menu`]).
fn menu_button<'a>(b: &RibbonButton, entries: &[MenuEntry], ctx: Ctx<'_>) -> Element<'a, Message> {
    let label = menu_bar::label_text(&b.label, ctx.i18n);
    let caption = container(
        iced::widget::row![
            text(label.clone()).size(12).width(Length::Shrink),
            text("\u{25be}").size(12).style(theme::muted_text)
        ]
        .spacing(3),
    )
    .height(Length::Fixed(32.0))
    .clip(true);
    let content = column![icon(b.icon, LARGE_ICON, IconTone::Text), caption]
        .spacing(2)
        .align_x(Alignment::Center);
    let root = container(content)
        .center_x(Length::Fixed(LARGE_WIDTH))
        .padding([4, 2])
        .height(Length::Fixed(LARGE_HEIGHT));
    let menu = menu_bar::build_menu(entries.to_vec(), ctx.i18n, ctx.platform);
    let bar =
        MenuBar::new(vec![Item::with_menu(root, menu)]).style(|theme: &iced::Theme, status| {
            let mut style = iced_aw::style::menu_bar::primary(theme, status);
            style.bar_background = iced::Color::TRANSPARENT.into();
            style
        });
    with_tip(
        crate::widgets::menu_guard::guard(bar),
        tip_text(ctx.i18n, &label, None, ctx.platform),
    )
}

fn button_view<'a>(b: &RibbonButton, ctx: Ctx<'_>) -> Element<'a, Message> {
    match &b.kind {
        ButtonKind::Menu(entries) => menu_button(b, entries, ctx),
        ButtonKind::Command | ButtonKind::Toggle(_) if b.small => small_button(b, ctx),
        ButtonKind::Command | ButtonKind::Toggle(_) => large_button(b, ctx),
    }
}

fn group_view<'a>(group: &RibbonGroup, ctx: Ctx<'_>) -> Element<'a, Message> {
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    let mut smalls: Vec<Element<'a, Message>> = Vec::new();
    let flush = |smalls: &mut Vec<Element<'a, Message>>, items: &mut Vec<Element<'a, Message>>| {
        if !smalls.is_empty() {
            items.push(column(std::mem::take(smalls)).spacing(2).into());
        }
    };
    for b in &group.buttons {
        if b.small && matches!(b.kind, ButtonKind::Command | ButtonKind::Toggle(_)) {
            smalls.push(button_view(b, ctx));
        } else {
            flush(&mut smalls, &mut items);
            items.push(button_view(b, ctx));
        }
    }
    flush(&mut smalls, &mut items);
    let title: Element<'a, Message> = match group.title {
        Some(key) => container(text(ctx.i18n.tr(key)).size(11).style(theme::muted_text))
            .height(Length::Fixed(TITLE_HEIGHT))
            .into(),
        None => Space::new().height(TITLE_HEIGHT).into(),
    };
    column![row(items).spacing(2), title]
        .spacing(2)
        .align_x(Alignment::Center)
        .into()
}

/// The line between two groups. Its height is fixed: a `Fill` height inside
/// the horizontal scrollable has no limit to fill, and stretched the whole
/// ribbon to the height of the window.
fn vertical_divider<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(1)
        .height(Length::Fixed(LARGE_HEIGHT + TITLE_HEIGHT))
        .style(theme::divider)
        .into()
}

/// The groups of the active tab, scrolling sideways when the window is
/// narrow.
fn panel<'a>(entries: &[MenuEntry], ctx: Ctx<'_>) -> Element<'a, Message> {
    let mut line = row![].spacing(8).padding([6, 8]).align_y(Alignment::Start);
    for (index, group) in groups(entries).iter().enumerate() {
        if index > 0 {
            line = line.push(vertical_divider());
        }
        line = line.push(group_view(group, ctx));
    }
    let scroll = scrollable(line)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(4).scroller_width(4),
        ))
        .width(Length::Fill);
    // A click on the background of the panel must not reach the board under
    // it when the ribbon floats.
    mouse_area(
        container(scroll)
            .width(Length::Fill)
            .style(theme::ribbon_panel),
    )
    .on_press(Message::Ui(UiMessage::Ribbon(RibbonMessage::Hold)))
    .into()
}

fn tab<'a>(index: usize, label: String, active: bool) -> Element<'a, Message> {
    let body = container(text(label).size(14))
        .padding([6, 12])
        .style(theme::tab_surface(TabLook {
            active,
            ..TabLook::default()
        }));
    let underline =
        container(Space::new().width(Length::Fill).height(2)).style(theme::tab_underline(active));
    mouse_area(column![body, underline])
        .on_press(ribbon_message(RibbonMessage::SelectTab(index)))
        .on_double_click(ribbon_message(RibbonMessage::ToggleCollapsed))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn tab_row<'a>(
    specs: &[MenuSpec],
    ribbon: &RibbonUi,
    collapsed: bool,
    ctx: Ctx<'_>,
) -> Element<'a, Message> {
    let shown = ribbon.shows_groups(collapsed);
    let mut tabs = row![].spacing(2).align_y(Alignment::End);
    for (index, spec) in specs.iter().enumerate() {
        tabs = tabs.push(tab(
            index,
            ctx.i18n.tr(spec.label_key),
            shown && index == active_tab(specs, ribbon),
        ));
    }
    let (arrow, key) = if collapsed {
        (Icon::SortDown, "ribbon-expand")
    } else {
        (Icon::SortUp, "ribbon-collapse")
    };
    let toggle = with_tip(
        button(icon(arrow, 16.0, IconTone::Text))
            .padding(4)
            .style(theme::command_button(ButtonLook::Secondary, false))
            .on_press(ribbon_message(RibbonMessage::ToggleCollapsed))
            .into(),
        ctx.i18n.tr(key),
    );
    container(
        row![
            scrollable(tabs).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0)
            )),
            Space::new().width(Length::Fill),
            toggle
        ]
        .spacing(8)
        .align_y(Alignment::End),
    )
    .padding([0, 8])
    .height(Length::Fixed(TAB_ROW_HEIGHT))
    .width(Length::Fill)
    .into()
}

fn active_tab(specs: &[MenuSpec], ribbon: &RibbonUi) -> usize {
    ribbon.tab.min(specs.len().saturating_sub(1))
}

/// The tabs and, when the ribbon is expanded, the groups of the active tab.
/// Takes the place of the menu bar.
pub fn view<'a>(
    specs: &[MenuSpec],
    ribbon: &RibbonUi,
    collapsed: bool,
    i18n: &I18n,
    platform: Platform,
) -> Element<'a, Message> {
    let ctx = Ctx { i18n, platform };
    let tabs = tab_row(specs, ribbon, collapsed, ctx);
    match specs.get(active_tab(specs, ribbon)) {
        Some(spec) if !collapsed => column![tabs, panel(&spec.entries, ctx)].into(),
        _ => tabs,
    }
}

/// The groups of a collapsed ribbon showing temporarily, to float over the
/// page under the row of tabs; a click outside closes them. `None` when
/// there is nothing to show.
pub fn flyout<'a>(
    specs: &[MenuSpec],
    ribbon: &RibbonUi,
    collapsed: bool,
    i18n: &I18n,
    platform: Platform,
) -> Option<Element<'a, Message>> {
    if !(collapsed && ribbon.flyout) {
        return None;
    }
    let ctx = Ctx { i18n, platform };
    let mut entries = specs.get(active_tab(specs, ribbon))?.entries.clone();
    wrap_picks(&mut entries);
    let outside = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(ribbon_message(RibbonMessage::Dismiss));
    Some(
        column![
            Space::new().height(TAB_ROW_HEIGHT),
            panel(&entries, ctx),
            outside
        ]
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_api::ViewOptions;
    use crate::i18n::Language;
    use crate::shell::UiState;
    use crate::windows::layout::LayoutNode;
    use mychess_app::AppState;

    fn ui() -> UiState {
        UiState::new(&LayoutNode::default_layout())
    }

    fn specs() -> Vec<MenuSpec> {
        menu_bar::menu_specs(&AppState::default(), &ui(), ViewOptions::default())
    }

    fn count_dividers(entries: &[MenuEntry]) -> usize {
        entries.iter().filter(|e| menu_bar::is_divider(e)).count()
    }

    #[test]
    fn every_tab_has_one_button_per_entry_of_its_menu() {
        for spec in specs() {
            let buttons: usize = groups(&spec.entries).iter().map(|g| g.buttons.len()).sum();
            assert_eq!(
                buttons,
                spec.entries.len() - count_dividers(&spec.entries),
                "{}",
                spec.label_key
            );
        }
    }

    #[test]
    fn groups_split_at_group_entries_and_separators() {
        let all = specs();
        let tools = all
            .iter()
            .find(|s| s.label_key == "menu-tools")
            .map(|s| groups(&s.entries));
        let tools = tools.unwrap_or_default();
        let titles: Vec<_> = tools.iter().map(|g| g.title).collect();
        assert_eq!(
            titles,
            [
                Some("menu-group-engines"),
                Some("menu-group-analysis-windows"),
                Some("menu-group-openings"),
                Some("menu-group-annotation")
            ]
        );
        // Engines, three starts, the toggle and the electronic board.
        assert_eq!(tools[0].buttons.len(), 6);
        // The menus without group entries use separators: untitled groups.
        let game = all
            .iter()
            .find(|s| s.label_key == "menu-game")
            .map(|s| groups(&s.entries));
        let game = game.unwrap_or_default();
        assert!(game.len() > 1 && game.iter().all(|g| g.title.is_none()));
    }

    #[test]
    fn every_button_has_a_real_icon_and_a_label() {
        let i18n = I18n::new(Language::EnUs);
        for spec in specs() {
            for group in groups(&spec.entries) {
                for b in &group.buttons {
                    assert_ne!(b.icon.file_stem(), "");
                    assert_ne!(menu_bar::label_text(&b.label, &i18n), "");
                }
            }
        }
    }

    #[test]
    fn a_leading_group_without_buttons_is_dropped() {
        let entries = [MenuEntry::Group("menu-group-engines"), MenuEntry::Separator];
        assert!(groups(&entries).is_empty());
    }

    #[test]
    fn toggles_follow_the_state_of_the_panels() {
        let mut ui = ui();
        let app = AppState::default();
        let find = |ui: &UiState| {
            let all = menu_bar::menu_specs(&app, ui, ViewOptions::default());
            let tools = all.into_iter().find(|s| s.label_key == "menu-tools");
            groups(&tools.map(|s| s.entries).unwrap_or_default())
                .into_iter()
                .flat_map(|g| g.buttons)
                .find(|b| matches!(b.label, Label::Key("menu-tools-graph")))
                .map(|b| b.kind)
        };
        assert!(matches!(find(&ui), Some(ButtonKind::Toggle(false))));
        ui.toggle_pane(crate::windows::layout::PaneKind::Graph);
        assert!(matches!(find(&ui), Some(ButtonKind::Toggle(true))));
    }

    #[test]
    fn disabled_items_have_no_action_and_enabled_ones_the_same_message() {
        let all = specs();
        for spec in &all {
            let flat: Vec<&MenuEntry> = spec
                .entries
                .iter()
                .filter(|e| matches!(e, MenuEntry::Item { .. }))
                .collect();
            let buttons: Vec<RibbonButton> = groups(&spec.entries)
                .into_iter()
                .flat_map(|g| g.buttons)
                .filter(|b| !matches!(b.kind, ButtonKind::Menu(_)))
                .collect();
            assert_eq!(flat.len(), buttons.len());
            for (entry, b) in flat.iter().zip(&buttons) {
                let MenuEntry::Item { action, .. } = entry else {
                    continue;
                };
                assert_eq!(format!("{action:?}"), format!("{:?}", b.action));
            }
        }
        // "Redo" has nothing to redo in a fresh application: inert.
        let edit = all.iter().find(|s| s.label_key == "menu-edit");
        let redo = edit
            .map(|s| groups(&s.entries))
            .unwrap_or_default()
            .into_iter()
            .flat_map(|g| g.buttons)
            .find(|b| matches!(b.label, Label::Key("menu-edit-redo")));
        assert!(redo.is_some_and(|b| b.action.is_none()));
    }

    #[test]
    fn submenus_are_menu_buttons_and_the_theme_is_a_command() {
        let all = specs();
        let kind_of = |menu: &str, key: &str| {
            let spec = all.iter().find(|s| s.label_key == menu);
            groups(&spec.map(|s| s.entries.clone()).unwrap_or_default())
                .into_iter()
                .flat_map(|g| g.buttons)
                .find(|b| matches!(b.label, Label::Key(k) if k == key))
                .map(|b| b.kind)
        };
        assert!(matches!(
            kind_of("menu-file", "menu-file-open-recent"),
            Some(ButtonKind::Menu(_))
        ));
        // The theme is a command that opens the appearance window (M9-3).
        match kind_of("menu-view", "menu-view-theme") {
            Some(ButtonKind::Command) => {}
            other => panic!("{other:?}"),
        }
        assert!(kind_of("menu-view", "menu-view-accent").is_none());
        assert!(matches!(
            kind_of("menu-view", "menu-view-menu-style"),
            Some(ButtonKind::Menu(_))
        ));
    }

    #[test]
    fn the_start_engine_commands_are_small() {
        let all = specs();
        let tools = all.iter().find(|s| s.label_key == "menu-tools");
        let smalls = groups(&tools.map(|s| s.entries.clone()).unwrap_or_default())
            .into_iter()
            .flat_map(|g| g.buttons)
            .filter(|b| b.small)
            .count();
        assert_eq!(smalls, 3);
    }

    #[test]
    fn tooltips_carry_the_shortcut_of_the_platform() {
        let i18n = I18n::new(Language::EnUs);
        let app = AppState::default();
        let binding = app
            .shortcuts
            .bindings(mychess_app::Action::Save)
            .first()
            .copied();
        assert_eq!(
            tip_text(&i18n, "Save", binding, Platform::Linux),
            "Save (Ctrl+S)"
        );
        assert!(
            tip_text(&i18n, "Save", binding, Platform::MacOs).contains('\u{2318}')
                || tip_text(&i18n, "Save", binding, Platform::MacOs).contains("Cmd")
        );
        assert_eq!(tip_text(&i18n, "Save", None, Platform::Linux), "Save");
    }

    fn all_wrapped(entries: &[MenuEntry]) -> bool {
        entries.iter().all(|e| match e {
            MenuEntry::Item { action, .. } => action
                .as_ref()
                .is_none_or(|m| matches!(m, Message::Ui(UiMessage::RibbonPick(_)))),
            MenuEntry::Submenu { entries, .. } => all_wrapped(entries),
            _ => true,
        })
    }

    #[test]
    fn picks_in_the_temporary_ribbon_are_wrapped_even_inside_submenus() {
        let mut all = specs();
        for spec in &mut all {
            wrap_picks(&mut spec.entries);
        }
        assert!(all.iter().all(|s| all_wrapped(&s.entries)));
    }

    #[test]
    fn selecting_collapsing_and_dismissing_follow_the_spec() {
        let mut ribbon = RibbonUi::default();
        // Expanded: a tab only changes the active one.
        assert_eq!(ribbon.update(RibbonMessage::SelectTab(3), false), None);
        assert_eq!((ribbon.tab, ribbon.flyout), (3, false));
        // The arrow collapses; the preference is asked to change.
        assert_eq!(
            ribbon.update(RibbonMessage::ToggleCollapsed, false),
            Some(true)
        );
        // Collapsed: a tab shows the groups for a moment.
        assert_eq!(ribbon.update(RibbonMessage::SelectTab(1), true), None);
        assert!(ribbon.flyout && ribbon.shows_groups(true));
        assert_eq!(ribbon.update(RibbonMessage::Dismiss, true), None);
        assert!(!ribbon.flyout && !ribbon.shows_groups(true));
        // Expanding hides the temporary state.
        ribbon.flyout = true;
        assert_eq!(
            ribbon.update(RibbonMessage::ToggleCollapsed, true),
            Some(false)
        );
        assert!(!ribbon.flyout && ribbon.shows_groups(false));
    }

    #[test]
    fn the_views_build_in_every_state_and_language() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for collapsed in [false, true] {
                for flyout in [false, true] {
                    let ribbon = RibbonUi { tab: 4, flyout };
                    let specs = specs();
                    drop(view(&specs, &ribbon, collapsed, &i18n, Platform::Linux));
                    let floating =
                        super::flyout(&specs, &ribbon, collapsed, &i18n, Platform::Linux);
                    assert_eq!(floating.is_some(), collapsed && flyout);
                }
            }
        }
        // An out-of-range tab falls back to the last one.
        let specs = specs();
        assert_eq!(
            active_tab(
                &specs,
                &RibbonUi {
                    tab: 99,
                    flyout: false
                }
            ),
            specs.len() - 1
        );
    }

    #[test]
    fn the_new_keys_exist_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for key in [
                "menu-view-menu-style",
                "menu-style-dropdown",
                "menu-style-ribbon",
                "ribbon-collapse",
                "ribbon-expand",
            ] {
                assert!(i18n.has(key), "{key} in {language:?}");
            }
        }
    }
}
