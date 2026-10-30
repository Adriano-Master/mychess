//! The configuration dialog of the calculation exercise and the visual
//! state that goes with it (spec M8-6c, D2326).
//!
//! The application keeps the last configuration; what is edited here is a
//! copy (`CalvarUi::draft`) that exists only while the user changes
//! something, so the values that the application reads from `calvar.toml`
//! after the dialog opened are shown until then. The text of the line being
//! typed in the panel lives here too.

use iced::widget::{button, column, container, row, scrollable, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::calvar::{CalvarConfig, CalvarMessage, CalvarView, PositionSource};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{check, choice, group, labelled};
use crate::windows::calvar_text::{history_text, notice_text, recent_text, stats_text};

/// Id of the configuration box.
pub const CALVAR_BOX_ID: &str = "calvar-box";
/// Id of the field of the FEN.
pub const CALVAR_FEN_ID: &str = "calvar-fen";
/// Id of the field where the moves of a line are typed (in the panel).
pub const CALVAR_ENTRY_ID: &str = "calvar-entry";

const BODY_HEIGHT: f32 = 360.0;
const HISTORY_HEIGHT: f32 = 110.0;
const LINES_STEPS: [u8; 5] = [1, 2, 3, 4, 5];
const MULTIPV_STEPS: [u8; 5] = [3, 4, 6, 8, 10];
const ROOT_SECONDS: [u16; 4] = [10, 30, 60, 120];
const LINE_SECONDS: [u16; 5] = [5, 10, 20, 30, 60];
const LIMIT_SECONDS: [u16; 5] = [0, 120, 300, 600, 1200];

/// One change of the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalvarEdit {
    /// Where the position comes from.
    Source(PositionSource),
    /// The text of the FEN.
    Fen(String),
    /// The engine (`None` is the automatic choice).
    Engine(Option<u64>),
    /// Lines asked for.
    Lines(u8),
    /// Lines of the engine.
    MultiPv(u8),
    /// Thinking time at the start, in seconds.
    RootSeconds(u16),
    /// Time of each search of a line, in seconds.
    LineSeconds(u16),
    /// Time limit in seconds.
    TimeLimit(u16),
    /// The board shows the line.
    FollowBoard(bool),
}

/// An event of the dialog or of the panel.
#[derive(Debug, Clone)]
pub enum CalvarUiMessage {
    /// Opens the dialog.
    Open,
    /// A change of the form.
    Edit(CalvarEdit),
    /// "Start".
    Submit,
    /// "Cancel".
    Cancel,
    /// "Clear history": the first click arms it, the second does it.
    Reset,
    /// "New exercise": ends the result and opens the dialog again.
    New,
    /// The text of the field of moves changed.
    EntryChanged(String),
    /// Enter in the field of moves.
    EntrySubmit,
    /// "Calculate variants" of the tactics panel: the wall clock is read
    /// when the click is handled, not when the view was built.
    FromTactics,
}

/// The visual state of the dialog and of the panel.
///
/// The draft is the list of the changes the user made, applied over the
/// configuration of the application each time it is shown (D2362): when
/// `calvar.toml` is read after the dialog opened, the saved values appear
/// under the changes instead of being lost.
#[derive(Debug, Default)]
pub struct CalvarUi {
    edits: Vec<CalvarEdit>,
    reset_armed: bool,
    entry: String,
    arrows: Vec<mychess_app::EngineArrow>,
}

impl CalvarUi {
    /// Forgets the draft when the dialog is gone and the text typed when
    /// the exercise ended.
    pub fn sync(&mut self, app: &AppState) {
        self.arrows = crate::windows::calvar_panel::arrows(app);
        if !app.calvar_dialog_open() {
            self.edits.clear();
            self.reset_armed = false;
        }
        if !app.calvar_live() {
            self.entry.clear();
        }
    }

    /// The arrow of the missed move to draw on the board (D2316).
    pub fn arrows(&self) -> &[mychess_app::EngineArrow] {
        &self.arrows
    }

    /// The configuration the form shows: the one of the application with
    /// the changes of the user on top.
    pub fn shown(&self, view: &CalvarView) -> CalvarConfig {
        let mut d = view.config.clone();
        for edit in &self.edits {
            match edit {
                CalvarEdit::Source(v) => d.source = *v,
                CalvarEdit::Fen(v) => d.fen.clone_from(v),
                CalvarEdit::Engine(v) => d.engine = *v,
                CalvarEdit::Lines(v) => d.lines_required = *v,
                CalvarEdit::MultiPv(v) => d.multipv = *v,
                CalvarEdit::RootSeconds(v) => d.root_ms = u64::from(*v) * 1000,
                CalvarEdit::LineSeconds(v) => d.line_ms = u64::from(*v) * 1000,
                CalvarEdit::TimeLimit(v) => d.time_limit_s = *v,
                CalvarEdit::FollowBoard(v) => d.follow_board = *v,
            }
        }
        d.sanitized()
    }

    /// The text in the field of moves.
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// Replaces the text of the field of moves.
    pub fn set_entry(&mut self, text: String) {
        self.entry = text;
    }

    /// Takes the text of the field of moves, leaving it empty.
    pub fn take_entry(&mut self) -> String {
        std::mem::take(&mut self.entry)
    }

    /// Whether "Clear history" waits for its confirmation.
    pub fn reset_armed(&self) -> bool {
        self.reset_armed
    }

    /// Arms the reset, or tells that it was confirmed.
    pub fn press_reset(&mut self) -> bool {
        let confirmed = self.reset_armed;
        self.reset_armed = !confirmed;
        confirmed
    }

    /// Records one change of the form.
    pub fn edit(&mut self, _view: &CalvarView, edit: CalvarEdit) {
        self.reset_armed = false;
        // Typing a FEN is one change per key: only the last one is kept.
        if matches!(edit, CalvarEdit::Fen(_))
            && matches!(self.edits.last(), Some(CalvarEdit::Fen(_)))
        {
            self.edits.pop();
        }
        self.edits.push(edit);
    }

    /// The configuration to start with.
    pub fn to_start(&self, view: &CalvarView) -> CalvarConfig {
        self.shown(view)
    }

    /// The message that starts an exercise with the form as it is. The
    /// wall clock comes from here, because the application never reads it
    /// (D2317).
    pub fn start_message(&self, view: &CalvarView) -> mychess_app::Message {
        let now = wall_clock();
        mychess_app::Message::Calvar(CalvarMessage::Start {
            config: Box::new(self.to_start(view)),
            now,
        })
    }
}

/// The wall clock in seconds since the Unix epoch: the application never
/// reads it (D2317).
pub fn wall_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn send(message: CalvarUiMessage) -> Message {
    Message::CalvarUi(message)
}

fn edit(edit: CalvarEdit) -> Message {
    send(CalvarUiMessage::Edit(edit))
}

fn number_choices<'a, T>(
    values: &[T],
    current: T,
    label: impl Fn(T) -> String,
    make: impl Fn(T) -> CalvarEdit + Copy + 'a,
) -> Element<'a, Message>
where
    T: Copy + PartialEq + 'a,
{
    group(
        values
            .iter()
            .map(|v| choice(label(*v), *v, current, true, move |x| edit(make(x))))
            .collect(),
    )
}

fn seconds_of(ms: u64) -> u16 {
    u16::try_from(ms / 1000).unwrap_or(u16::MAX)
}

fn position_rows<'a>(
    c: &CalvarConfig,
    engines: &[(u64, String)],
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    form = form.push(labelled(
        i18n.tr("calvar-position"),
        group(vec![
            choice(
                i18n.tr("calvar-position-current"),
                PositionSource::Current,
                c.source,
                true,
                |v| edit(CalvarEdit::Source(v)),
            ),
            choice(
                i18n.tr("calvar-position-fen"),
                PositionSource::Fen,
                c.source,
                true,
                |v| edit(CalvarEdit::Source(v)),
            ),
        ]),
    ));
    if c.source == PositionSource::Fen {
        let field = text_input(&i18n.tr("calvar-fen-placeholder"), &c.fen)
            .id(CALVAR_FEN_ID)
            .on_input(|v| edit(CalvarEdit::Fen(v)))
            .on_submit(send(CalvarUiMessage::Submit))
            .size(13)
            .padding([3, 6]);
        form = form.push(labelled(String::new(), field.into()));
    }
    let mut choices = vec![choice(
        i18n.tr("calvar-engine-auto"),
        None,
        c.engine,
        true,
        |v| edit(CalvarEdit::Engine(v)),
    )];
    for (id, name) in engines {
        choices.push(choice(name.clone(), Some(*id), c.engine, true, |v| {
            edit(CalvarEdit::Engine(v))
        }));
    }
    form = form.push(labelled(i18n.tr("calvar-engine"), group(choices)));
    if engines.is_empty() {
        form = form.push(
            text(i18n.tr("calvar-engine-none"))
                .size(12)
                .style(text::danger),
        );
    }
    form
}

fn limit_rows<'a>(
    c: &CalvarConfig,
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    form = form.push(labelled(
        i18n.tr("calvar-lines-label"),
        number_choices(
            &LINES_STEPS,
            c.lines_required,
            |n| n.to_string(),
            CalvarEdit::Lines,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("calvar-multipv-label"),
        number_choices(
            &MULTIPV_STEPS,
            c.multipv,
            |n| n.to_string(),
            CalvarEdit::MultiPv,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("calvar-root-time-label"),
        number_choices(
            &ROOT_SECONDS,
            seconds_of(c.root_ms),
            |n| n.to_string(),
            CalvarEdit::RootSeconds,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("calvar-line-time-label"),
        number_choices(
            &LINE_SECONDS,
            seconds_of(c.line_ms),
            |n| n.to_string(),
            CalvarEdit::LineSeconds,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("calvar-time-limit-label"),
        number_choices(
            &LIMIT_SECONDS,
            c.time_limit_s,
            |n| minutes_label(n, i18n),
            CalvarEdit::TimeLimit,
        ),
    ));
    form.push(
        tooltip(
            check(i18n.tr("calvar-follow-board"), c.follow_board, true, |v| {
                edit(CalvarEdit::FollowBoard(v))
            }),
            text(i18n.tr("calvar-follow-board-tip")).size(12),
            tooltip::Position::Top,
        )
        .style(theme::tooltip_box),
    )
}

fn minutes_label(seconds: u16, i18n: &I18n) -> String {
    if seconds == 0 {
        return i18n.tr("calvar-off");
    }
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("count", i64::from(seconds / 60));
    i18n.tr_args("calvar-minutes", &args)
}

/// The last exercises, each with a button that opens it in read-only mode
/// (M8-6d, D2370).
fn history_rows<'a>(
    view: &CalvarView,
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    if view.history.is_empty() {
        return form;
    }
    form = form.push(text(i18n.tr("calvar-history-title")).size(13));
    for h in &view.history {
        let mut label = history_text(h, i18n);
        if !h.reviewable {
            label.push(' ');
            label.push_str(&i18n.tr("calvar-history-old"));
        }
        let press = h
            .reviewable
            .then(|| Message::App(mychess_app::Message::Calvar(CalvarMessage::Review(h.index))));
        form = form.push(
            row![
                text(label).size(12).width(Length::Fill),
                tooltip(
                    button(text(i18n.tr("calvar-history-review")).size(12))
                        .padding([2, 8])
                        .style(button::secondary)
                        .on_press_maybe(press),
                    text(i18n.tr("calvar-history-review-tip")).size(12),
                    tooltip::Position::Top,
                )
                .style(theme::tooltip_box),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    form
}

/// The dialog.
pub fn view<'a>(
    ui: &'a CalvarUi,
    view: &CalvarView,
    engines: &[(u64, String)],
    i18n: &I18n,
) -> Element<'a, Message> {
    let c = ui.shown(view);
    let mut form = column![].spacing(10);
    form = position_rows(&c, engines, i18n, form);
    form = limit_rows(&c, i18n, form);
    let reset_label = if ui.reset_armed() {
        i18n.tr("calvar-reset-confirm")
    } else {
        i18n.tr("calvar-reset")
    };
    let mut summary = column![
        text(stats_text(&view.stats, i18n))
            .size(12)
            .style(theme::muted_text)
    ]
    .spacing(2);
    if let Some(recent) = recent_text(&view.recent_scores, i18n) {
        summary = summary.push(text(recent).size(12).style(theme::muted_text));
    }
    form = form.push(
        row![
            summary,
            space::horizontal(),
            tooltip(
                button(text(reset_label).size(13))
                    .padding([3, 8])
                    .style(button::secondary)
                    .on_press(send(CalvarUiMessage::Reset)),
                text(i18n.tr("calvar-reset-tip")).size(12),
                tooltip::Position::Top,
            )
            .style(theme::tooltip_box),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if view.reading_game {
        form = form.push(text(i18n.tr("calvar-reading-game")).size(12));
    }
    if let Some(issue) = view.store_issue {
        form = form.push(text(notice_text(issue, i18n)).size(13).style(text::danger));
    }
    if let Some(notice) = view.notice {
        form = form.push(text(notice_text(notice, i18n)).size(13).style(text::danger));
    }
    let mut body =
        column![scrollable(form.padding([0, 12])).height(Length::Fixed(BODY_HEIGHT))].spacing(6);
    // The past exercises sit apart from the form so that they are always in
    // view (M8-6d, D2370).
    if !view.history.is_empty() {
        body = body.push(
            scrollable(history_rows(view, i18n, column![].spacing(3)).padding([0, 12]))
                .height(Length::Fixed(HISTORY_HEIGHT)),
        );
    }
    let body = container(body);
    modal(Card {
        id: CALVAR_BOX_ID,
        title: i18n.tr("calvar-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("calvar-cancel")))
                .style(button::secondary)
                .on_press(send(CalvarUiMessage::Cancel))
                .into(),
            button(text(i18n.tr("calvar-start")))
                .style(button::primary)
                .on_press(send(CalvarUiMessage::Submit))
                .into(),
        ],
        width: 560.0,
    })
}

/// The message of the application for "Cancel".
pub fn cancel_message() -> mychess_app::Message {
    mychess_app::Message::Calvar(CalvarMessage::CloseDialog)
}
