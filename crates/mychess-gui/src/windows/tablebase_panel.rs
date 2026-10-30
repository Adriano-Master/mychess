//! The tablebase panel (spec M8-8c, D2571-D2574): the result of every legal
//! move of the position on the board, the settings and the endgame training.
//!
//! Everything is drawn from the [`TablebaseView`] that the application
//! offers; the buttons send messages of the application. The state of this
//! window is only which section is open and the text of two fields.

use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::alignment::Horizontal;
use iced::font::Weight;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, space, text, text_input,
    tooltip,
};
use iced::{Alignment, Element, Font, Length};
use mychess_app::Message as AppMessage;
use mychess_app::tablebase::{
    DirectoryView, RowView, TRAINING_MATERIALS, TablebaseMessage, TablebaseStatus, TablebaseView,
    TbSource,
};

use crate::i18n::I18n;
use crate::shell::{Message, Shell};
use crate::theme::{self, RowLook, Tokens, tablebase_colors};
use crate::windows::panes;
use crate::windows::tablebase_text::{
    dtm_text, dtz_text, grade_text, issue_text, reason_text, result_tip, result_word, side_text,
    summary_text, verdict_text,
};

/// Id of the body of the panel (for tests and automation).
pub const TABLEBASE_PANE_ID: &str = "tablebase-pane";
/// Id of the field that takes a folder.
pub const FOLDER_FIELD_ID: &str = "tablebase-folder";

const TABLE_TEXT: f32 = 13.0;
const MOVE_WIDTH: f32 = 64.0;
const RESULT_WIDTH: f32 = 170.0;
const NUMBER_WIDTH: f32 = 64.0;

/// What this window keeps on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TablebaseUi {
    /// The settings section is open.
    pub config_open: bool,
    /// The text of the field of folders.
    pub folder: String,
    /// The text of the field of materials.
    pub material: String,
}

/// An event of this window (the rest goes straight to the application).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TablebaseUiMessage {
    /// Opens or closes the settings.
    ToggleConfig,
    /// The text of the field of folders changed.
    FolderEdit(String),
    /// Adds the folder typed in the field.
    AddFolder,
    /// The text of the field of materials changed.
    MaterialEdit(String),
    /// A new training position of that material.
    NewPosition(String),
    /// A new training position of the material typed in the field.
    NewTyped,
}

fn ui(message: TablebaseUiMessage) -> Message {
    Message::TablebaseUi(message)
}

fn send(message: TablebaseMessage) -> Message {
    Message::App(AppMessage::Tablebase(message))
}

/// The folder typed, as a message for the application, and the empty field.
pub fn add_folder_message(ui: &mut TablebaseUi) -> Option<TablebaseMessage> {
    let text = std::mem::take(&mut ui.folder);
    let text = text.trim();
    (!text.is_empty()).then(|| TablebaseMessage::AddDirectory(PathBuf::from(text)))
}

fn small_button<'a>(label: String, message: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(13))
        .padding([3, 10])
        .style(button::secondary)
        .on_press_maybe(message)
        .into()
}

fn tipped<'a>(
    label: String,
    tip: String,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    let b = button(text(label).size(13))
        .padding([4, 10])
        .on_press_maybe(message);
    let b = if primary {
        b.style(button::primary)
    } else {
        b.style(button::secondary)
    };
    tooltip(b, text(tip).size(12), tooltip::Position::Bottom)
        .style(theme::tooltip_box)
        .into()
}

fn check<'a>(
    label: String,
    on: bool,
    change: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .size(15)
        .text_size(13)
        .on_toggle(change)
        .into()
}

fn bold() -> Font {
    Font {
        weight: Weight::Bold,
        ..Font::DEFAULT
    }
}

// ---- settings ----

fn directory_line<'a>(i18n: &I18n, index: usize, dir: &DirectoryView) -> Element<'a, Message> {
    let counts = match (dir.wdl_files, dir.dtz_files) {
        _ if dir.unreadable => i18n.tr("tablebase-config-dir-unreadable"),
        (Some(wdl), Some(dtz)) => {
            let mut args = FluentArgs::new();
            args.set("wdl", i64::try_from(wdl).unwrap_or(i64::MAX));
            args.set("dtz", i64::try_from(dtz).unwrap_or(i64::MAX));
            let mut line = i18n.tr_args("tablebase-config-dir-counts", &args);
            if dir.bad_files > 0 {
                let mut bad = FluentArgs::new();
                bad.set("count", i64::try_from(dir.bad_files).unwrap_or(i64::MAX));
                line.push_str(", ");
                line.push_str(&i18n.tr_args("tablebase-config-dir-bad", &bad));
            }
            line
        }
        _ => String::new(),
    };
    let line = row![
        text(dir.path.clone()).size(12).width(Length::Fill),
        text(counts).size(12).style(theme::muted_text),
        small_button(
            i18n.tr("tablebase-config-remove"),
            Some(send(TablebaseMessage::RemoveDirectory(index)))
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if dir.uppercase_files == 0 {
        return line.into();
    }
    let mut args = FluentArgs::new();
    args.set(
        "count",
        i64::try_from(dir.uppercase_files).unwrap_or(i64::MAX),
    );
    column![
        line,
        text(i18n.tr_args("tablebase-config-dir-uppercase", &args))
            .size(12)
            .style(text::danger),
    ]
    .spacing(2)
    .into()
}

fn config<'a>(view: &TablebaseView, state: &TablebaseUi, i18n: &I18n) -> Element<'a, Message> {
    let mut body = column![check(
        i18n.tr("tablebase-config-local"),
        view.local_enabled,
        |on| send(TablebaseMessage::SetLocal(on)),
    ),]
    .spacing(6);
    if view.local_enabled || !view.directories.is_empty() {
        body = body.push(text(i18n.tr("tablebase-config-folders")).size(12));
        for (index, dir) in view.directories.iter().enumerate() {
            body = body.push(directory_line(i18n, index, dir));
        }
        body = body.push(
            row![
                text_input(
                    &i18n.tr("tablebase-config-folder-placeholder"),
                    &state.folder
                )
                .id(FOLDER_FIELD_ID)
                .on_input(|v| ui(TablebaseUiMessage::FolderEdit(v)))
                .on_submit(ui(TablebaseUiMessage::AddFolder))
                .size(13)
                .padding([3, 6]),
                small_button(
                    i18n.tr("tablebase-config-add"),
                    Some(ui(TablebaseUiMessage::AddFolder))
                ),
                small_button(
                    i18n.tr("tablebase-config-browse"),
                    Some(send(TablebaseMessage::PickDirectory))
                ),
                small_button(
                    i18n.tr("tablebase-config-reload"),
                    Some(send(TablebaseMessage::Reopen))
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
        if let Some(summary) = view.summary {
            let mut args = FluentArgs::new();
            args.set("wdl", i64::try_from(summary.wdl_files).unwrap_or(i64::MAX));
            args.set("dtz", i64::try_from(summary.dtz_files).unwrap_or(i64::MAX));
            args.set("pieces", i64::from(summary.max_pieces));
            body = body.push(
                text(i18n.tr_args("tablebase-config-summary", &args))
                    .size(12)
                    .style(theme::muted_text),
            );
        }
    }
    body = body.push(check(
        i18n.tr("tablebase-config-online"),
        view.online_enabled,
        |on| send(TablebaseMessage::SetOnline(on)),
    ));
    if view.online_enabled {
        body = body.push(
            text(i18n.tr("tablebase-config-online-privacy"))
                .size(12)
                .style(theme::muted_text),
        );
    }
    body = body.push(
        text(i18n.tr("tablebase-config-help"))
            .size(12)
            .style(theme::muted_text),
    );
    container(body).padding(8).style(theme::ribbon_panel).into()
}

// ---- training ----

fn training<'a>(view: &TablebaseView, state: &TablebaseUi, i18n: &I18n) -> Element<'a, Message> {
    let mut body = column![].spacing(6);
    if let Some(t) = view.training {
        let mut args = FluentArgs::new();
        args.set("side", side_text(i18n, t.user));
        args.set("moves", i64::from(t.user_moves));
        args.set("slips", i64::from(t.slips));
        args.set(
            "grade",
            t.start.map(|g| grade_text(i18n, g)).unwrap_or_default(),
        );
        body = body.push(
            row![
                text(i18n.tr_args("tablebase-train-status", &args)).size(13),
                space::horizontal(),
                small_button(
                    i18n.tr("tablebase-train-stop"),
                    Some(send(TablebaseMessage::TrainStop))
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        body = body.push(
            text(i18n.tr(if t.tablebase_to_move {
                "tablebase-train-thinking"
            } else {
                "tablebase-train-your-move"
            }))
            .size(12)
            .style(theme::muted_text),
        );
    }
    if let Some(outcome) = view.outcome {
        body = body.push(
            row![
                column![
                    text(verdict_text(i18n, outcome.verdict))
                        .size(14)
                        .font(bold()),
                    text(reason_text(i18n, outcome.reason)).size(12),
                    text(summary_text(i18n, &outcome))
                        .size(12)
                        .style(theme::muted_text),
                ]
                .spacing(2),
                space::horizontal(),
                small_button(
                    i18n.tr("tablebase-dismiss"),
                    Some(send(TablebaseMessage::DismissOutcome))
                ),
            ]
            .spacing(8),
        );
    }
    if view.training.is_none() {
        let typed = state.material.trim().to_owned();
        body = body.push(
            row![
                pick_list(
                    &TRAINING_MATERIALS[..],
                    None::<&'static str>,
                    |material: &'static str| ui(TablebaseUiMessage::NewPosition(
                        material.to_owned()
                    )),
                )
                .placeholder(i18n.tr("tablebase-train-new"))
                .text_size(13),
                text_input(
                    &i18n.tr("tablebase-train-material-placeholder"),
                    &state.material
                )
                .on_input(|v| ui(TablebaseUiMessage::MaterialEdit(v)))
                .on_submit(ui(TablebaseUiMessage::NewTyped))
                .size(13)
                .padding([3, 6])
                .width(Length::Fixed(190.0)),
                small_button(
                    i18n.tr("tablebase-train-go"),
                    (!typed.is_empty()).then(|| ui(TablebaseUiMessage::NewTyped))
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    body.into()
}

// ---- the list ----

fn cell<'a>(content: String, width: f32, align: Horizontal) -> iced::widget::Text<'a> {
    text(content)
        .size(TABLE_TEXT)
        .width(Length::Fixed(width))
        .align_x(align)
}

fn header_row<'a>(view: &TablebaseView, i18n: &I18n) -> Element<'a, Message> {
    let mut cells = row![
        cell(i18n.tr("tablebase-col-move"), MOVE_WIDTH, Horizontal::Left),
        cell(
            i18n.tr("tablebase-col-result"),
            RESULT_WIDTH,
            Horizontal::Left
        ),
        tooltip(
            cell(
                i18n.tr("tablebase-col-dtz"),
                NUMBER_WIDTH,
                Horizontal::Right
            ),
            text(i18n.tr("tablebase-dtz-tip")).size(12),
            tooltip::Position::Bottom
        )
        .style(theme::tooltip_box),
    ]
    .spacing(8);
    if view.has_dtm {
        cells = cells.push(
            tooltip(
                cell(
                    i18n.tr("tablebase-col-dtm"),
                    NUMBER_WIDTH,
                    Horizontal::Right,
                ),
                text(i18n.tr("tablebase-dtm-tip")).size(12),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip_box),
        );
    }
    container(cells)
        .padding([2, 8])
        .width(Length::Fill)
        .style(theme::list_header)
        .into()
}

fn click_style(theme: &iced::Theme, status: button::Status) -> button::Style {
    let t = Tokens::of(theme);
    let background = match status {
        button::Status::Hovered => Some(t.hover.into()),
        button::Status::Pressed => Some(t.pressed.into()),
        button::Status::Active | button::Status::Disabled => None,
    };
    button::Style {
        background,
        text_color: t.text,
        border: iced::Border {
            radius: t.radius.chip.into(),
            ..iced::Border::default()
        },
        ..button::Style::default()
    }
}

fn data_row<'a>(
    i18n: &I18n,
    index: usize,
    data: &RowView,
    show_dtm: bool,
    stale: bool,
) -> Element<'a, Message> {
    let word = format!(
        "{} {}",
        tablebase_colors::category_symbol(data.category),
        result_word(i18n, data.category)
    );
    let san = if data.best {
        cell(data.san.clone(), MOVE_WIDTH, Horizontal::Left).font(bold())
    } else {
        cell(data.san.clone(), MOVE_WIDTH, Horizontal::Left)
    };
    let tip = result_tip(i18n, data.category);
    let category = data.category;
    let result = text(word)
        .size(TABLE_TEXT)
        .width(Length::Fixed(RESULT_WIDTH))
        .style(move |theme: &iced::Theme| text::Style {
            color: Some(tablebase_colors::category_color(
                &Tokens::of(theme),
                category,
            )),
        });
    let result: Element<'a, Message> = match tip {
        Some(tip) => tooltip(result, text(tip).size(12), tooltip::Position::Top)
            .style(theme::tooltip_box)
            .into(),
        None => result.into(),
    };
    let mut cells = row![
        san,
        result,
        cell(
            dtz_text(data.dtz, data.dtz_rounded),
            NUMBER_WIDTH,
            Horizontal::Right
        ),
    ]
    .spacing(8);
    if show_dtm {
        cells = cells.push(cell(
            dtm_text(i18n, data.dtm),
            NUMBER_WIDTH,
            Horizontal::Right,
        ));
    }
    let inner = button(cells)
        .padding([2, 8])
        .width(Length::Fill)
        .style(click_style)
        .on_press(send(TablebaseMessage::PlayRow(index)));
    container(inner)
        .id(format!("tablebase-row-{index}"))
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            zebra: index % 2 == 1,
            selected: false,
            current: data.best,
            dimmed: stale,
        }))
        .into()
}

fn list<'a>(view: &TablebaseView, i18n: &I18n) -> Element<'a, Message> {
    let mut rows = column![header_row(view, i18n)].spacing(1);
    for (index, data) in view.rows.iter().enumerate() {
        rows = rows.push(data_row(i18n, index, data, view.has_dtm, view.stale));
    }
    scrollable(rows).height(Length::Fill).into()
}

fn summary<'a>(view: &TablebaseView, i18n: &I18n) -> Element<'a, Message> {
    let mut line = column![].spacing(2);
    if let Some(position) = view.position {
        let mut args = FluentArgs::new();
        let mut parts = vec![format!(
            "{} {}",
            tablebase_colors::category_symbol(position.category),
            result_word(i18n, position.category)
        )];
        if let Some(dtz) = position.dtz {
            parts.push(format!(
                "{} {}",
                i18n.tr("tablebase-col-dtz"),
                dtz_text(Some(dtz), position.dtz_rounded)
            ));
        }
        if position.dtm.is_some() {
            parts.push(format!(
                "{} {}",
                i18n.tr("tablebase-col-dtm"),
                dtm_text(i18n, position.dtm)
            ));
        }
        let result = parts.join(" | ");
        args.set("result", result);
        let color = position.category;
        line = line.push(
            text(i18n.tr_args("tablebase-position-result", &args))
                .size(14)
                .font(bold())
                .style(move |theme: &iced::Theme| text::Style {
                    color: Some(tablebase_colors::category_color(&Tokens::of(theme), color)),
                }),
        );
        if view.rows.is_empty() && !view.results_hidden() {
            line = line.push(text(i18n.tr("tablebase-no-moves")).size(12));
        } else if !view.results_hidden() {
            let mut counts = FluentArgs::new();
            counts.set("wins", i64::from(view.counts.wins));
            counts.set("draws", i64::from(view.counts.draws));
            counts.set("losses", i64::from(view.counts.losses));
            counts.set("unknown", i64::from(view.counts.unknown));
            line = line.push(
                text(i18n.tr_args("tablebase-counts", &counts))
                    .size(12)
                    .style(theme::muted_text),
            );
        }
    }
    line.into()
}

fn status_line<'a>(view: &TablebaseView, i18n: &I18n) -> Option<Element<'a, Message>> {
    let key = match view.status {
        TablebaseStatus::NotConfigured => "tablebase-status-not-configured",
        TablebaseStatus::Locked => "tablebase-status-locked",
        TablebaseStatus::Loading => "tablebase-status-loading",
        TablebaseStatus::Waiting => "tablebase-status-waiting",
        TablebaseStatus::Stale => "tablebase-status-stale",
        _ => return None,
    };
    Some(text(i18n.tr(key)).size(13).style(theme::muted_text).into())
}

fn source_label(i18n: &I18n, source: Option<TbSource>) -> String {
    match source {
        Some(TbSource::Local) => i18n.tr("tablebase-source-local"),
        Some(TbSource::Online) => i18n.tr("tablebase-source-online"),
        _ => i18n.tr("pane-tablebase"),
    }
}

/// The content of the panel for `view`.
pub fn content<'a>(view: &TablebaseView, state: &TablebaseUi, i18n: &I18n) -> Element<'a, Message> {
    let locked = view.status == TablebaseStatus::Locked;
    let configured = view.local_enabled || view.online_enabled;
    let can_train = configured && !locked && view.training.is_none();
    let header = row![
        text(source_label(i18n, view.source)).size(15).font(bold()),
        space::horizontal(),
        tipped(
            i18n.tr("tablebase-train"),
            i18n.tr("tablebase-train-tip"),
            can_train.then(|| send(TablebaseMessage::TrainStart)),
            false,
        ),
        tipped(
            i18n.tr("tablebase-config"),
            i18n.tr("tablebase-config-tip"),
            Some(ui(TablebaseUiMessage::ToggleConfig)),
            state.config_open,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut page = column![header].spacing(8).padding(8);
    if locked {
        if let Some(line) = status_line(view, i18n) {
            page = page.push(line);
        }
        return page.into();
    }
    if state.config_open || view.status == TablebaseStatus::NotConfigured {
        page = page.push(config(view, state, i18n));
    }
    if configured {
        page = page.push(training(view, state, i18n));
        if !view.results_hidden() {
            page = page.push(summary(view, i18n));
        }
        if let Some(issue) = &view.issue {
            page = page.push(text(issue_text(i18n, issue)).size(12).style(text::danger));
        }
        if let Some(line) = status_line(view, i18n) {
            page = page.push(line);
        }
        if !view.rows.is_empty() {
            page = page.push(list(view, i18n));
        }
    } else if let Some(line) = status_line(view, i18n) {
        page = page.push(line);
    }
    page.into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let view = shell.app.tablebase_view();
    panes::body(
        content(&view, &shell.ui.dialogs.tablebase, &shell.i18n),
        TABLEBASE_PANE_ID,
    )
}
