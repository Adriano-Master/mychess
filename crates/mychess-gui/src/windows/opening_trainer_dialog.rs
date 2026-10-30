//! The configuration dialog of the opening trainer and the visual state
//! that goes with it (spec M8-5, D2239).
//!
//! The application keeps the last configuration; what is edited here is a
//! copy (`OpeningUi::draft`) that exists only while the user changes
//! something.

use std::path::PathBuf;

use fluent_bundle::FluentArgs;
use iced::widget::{button, column, container, row, scrollable, space, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::opening_trainer::{
    OpeningConfig, OpeningMessage, OpeningView, RepSourceKind, TrainSide,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{check, choice, group, labelled};
use crate::windows::opening_trainer_text::notice_text;

/// Id of the configuration box.
pub const OPENING_BOX_ID: &str = "opening-box";

const BODY_HEIGHT: f32 = 420.0;
const DEPTH_STEPS: [u8; 6] = [4, 8, 12, 16, 24, 40];

/// One change of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningEdit {
    /// The side to train.
    Side(TrainSide),
    /// Where the repertoire comes from.
    Source(RepSourceKind),
    /// Maximum depth in plies.
    Depth(u8),
    /// Only the best moves are allowed.
    Strict(bool),
    /// The opponent plays the best moves.
    OpponentBest(bool),
    /// Do not repeat a line.
    NoRepeat(bool),
    /// Only flagged lines.
    OnlyFlagged(bool),
    /// Show the candidate moves.
    ShowCandidates(bool),
    /// From a mask: only the marked moves.
    OnlyMarked(bool),
}

/// An event of the dialog.
#[derive(Debug, Clone)]
pub enum OpeningUiMessage {
    /// Opens the dialog.
    Open,
    /// A change of the form.
    Edit(OpeningEdit),
    /// "Choose..." of the file.
    PickFile,
    /// The file dialog answered.
    FilePicked(Option<PathBuf>),
    /// "Start".
    Submit,
    /// "Cancel".
    Cancel,
    /// "Reset statistics": the first click arms it, the second does it.
    Reset,
}

/// The visual state of the dialog.
#[derive(Debug, Default)]
pub struct OpeningUi {
    draft: Option<OpeningConfig>,
    reset_armed: bool,
    hint_arrows: Vec<mychess_app::EngineArrow>,
    /// The files the repertoire would be read from, kept until the choice
    /// that decides them changes (D2258: `view` does not read the slots).
    names: Vec<String>,
    names_for: Option<(RepSourceKind, TrainSide, Option<PathBuf>)>,
}

impl OpeningUi {
    /// Forgets the draft when the dialog is gone.
    pub fn sync(&mut self, app: &AppState) {
        self.hint_arrows = crate::windows::opening_trainer_panel::hint_arrows(app);
        if !app.opening_dialog_open() {
            self.draft = None;
            self.reset_armed = false;
            self.names_for = None;
            return;
        }
        let view = app.opening_view();
        let c = self.shown(&view);
        let key = (c.source, c.side, c.file.clone());
        if self.names_for.as_ref() != Some(&key) {
            self.names = app.opening_source_names(c);
            self.names_for = Some(key);
        }
    }

    /// The files the repertoire would be read from (D2239).
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The arrow of the hint to draw on the board (D2241).
    pub fn arrows(&self) -> &[mychess_app::EngineArrow] {
        &self.hint_arrows
    }

    /// The configuration the form shows.
    pub fn shown<'a>(&'a self, view: &'a OpeningView) -> &'a OpeningConfig {
        self.draft.as_ref().unwrap_or(&view.config)
    }

    /// Whether "Reset statistics" waits for its confirmation.
    pub fn reset_armed(&self) -> bool {
        self.reset_armed
    }

    /// Arms the reset, or tells that it was confirmed.
    pub fn press_reset(&mut self) -> bool {
        let confirmed = self.reset_armed;
        self.reset_armed = !confirmed;
        confirmed
    }

    /// Applies one change to the draft.
    pub fn edit(&mut self, view: &OpeningView, edit: OpeningEdit) {
        self.reset_armed = false;
        let d = self.draft.get_or_insert_with(|| view.config.clone());
        match edit {
            OpeningEdit::Side(v) => d.side = v,
            OpeningEdit::Source(v) => d.source = v,
            OpeningEdit::Depth(v) => d.max_depth = v,
            OpeningEdit::Strict(v) => d.strict = v,
            OpeningEdit::OpponentBest(v) => d.opponent_best = v,
            OpeningEdit::NoRepeat(v) => d.no_repeat = v,
            OpeningEdit::OnlyFlagged(v) => d.only_flagged = v,
            OpeningEdit::ShowCandidates(v) => d.show_candidates = v,
            OpeningEdit::OnlyMarked(v) => d.only_marked = v,
        }
        *d = d.clone().sanitized();
    }

    /// Sets the file of the draft.
    pub fn set_file(&mut self, view: &OpeningView, path: PathBuf) {
        self.reset_armed = false;
        let d = self.draft.get_or_insert_with(|| view.config.clone());
        d.file = Some(path);
        d.source = RepSourceKind::File;
    }

    /// The configuration to start with.
    pub fn to_start(&self, view: &OpeningView) -> OpeningConfig {
        self.shown(view).clone().sanitized()
    }

    /// The message that starts a session with the form as it is. The seed
    /// and the day come from the clock here, because the application never
    /// reads it (D2214, D2223).
    pub fn start_message(&self, view: &OpeningView) -> mychess_app::Message {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let seed = u64::try_from(now.as_nanos()).unwrap_or(0);
        // The day of the repetitions is the UTC day (D2258).
        let today = u32::try_from(now.as_secs() / 86_400).unwrap_or(0);
        mychess_app::Message::OpeningTrainer(OpeningMessage::Start {
            config: Box::new(self.to_start(view)),
            seed,
            today,
        })
    }
}

fn send(message: OpeningUiMessage) -> Message {
    Message::OpeningUi(message)
}

fn edit(edit: OpeningEdit) -> Message {
    send(OpeningUiMessage::Edit(edit))
}

fn file_name(config: &OpeningConfig, i18n: &I18n) -> String {
    config.file.as_ref().map_or_else(
        || i18n.tr("opening-no-file"),
        |path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        },
    )
}

fn source_rows<'a>(
    c: &OpeningConfig,
    names: &[String],
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    form = form.push(labelled(
        i18n.tr("opening-side"),
        group(vec![
            choice(
                i18n.tr("opening-side-white"),
                TrainSide::White,
                c.side,
                true,
                |v| edit(OpeningEdit::Side(v)),
            ),
            choice(
                i18n.tr("opening-side-black"),
                TrainSide::Black,
                c.side,
                true,
                |v| edit(OpeningEdit::Side(v)),
            ),
            choice(
                i18n.tr("opening-side-both"),
                TrainSide::Both,
                c.side,
                true,
                |v| edit(OpeningEdit::Side(v)),
            ),
        ]),
    ));
    let mut source = row![
        choice(
            i18n.tr("opening-source-typed"),
            RepSourceKind::TypedBase,
            c.source,
            true,
            |v| edit(OpeningEdit::Source(v))
        ),
        choice(
            i18n.tr("opening-source-base"),
            RepSourceKind::CurrentBase,
            c.source,
            true,
            |v| edit(OpeningEdit::Source(v))
        ),
        choice(
            i18n.tr("opening-source-file"),
            RepSourceKind::File,
            c.source,
            true,
            |v| edit(OpeningEdit::Source(v))
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if c.source == RepSourceKind::File {
        source = source.push(
            button(text(i18n.tr("opening-choose-file")).size(13))
                .padding([3, 8])
                .style(button::secondary)
                .on_press(send(OpeningUiMessage::PickFile)),
        );
    }
    form = form.push(labelled(i18n.tr("opening-source"), source.into()));
    let uses = if c.source == RepSourceKind::File {
        file_name(c, i18n)
    } else if names.is_empty() {
        i18n.tr("opening-uses-none")
    } else {
        let mut args = FluentArgs::new();
        args.set("names", names.join(", "));
        i18n.tr_args("opening-uses", &args)
    };
    form.push(labelled(
        String::new(),
        text(uses).size(12).style(theme::muted_text).into(),
    ))
}

fn option_rows<'a>(
    c: &OpeningConfig,
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    form = form.push(labelled(
        i18n.tr("opening-depth"),
        group(
            DEPTH_STEPS
                .iter()
                .map(|n| {
                    choice(n.to_string(), *n, c.max_depth, true, |v| {
                        edit(OpeningEdit::Depth(v))
                    })
                })
                .collect(),
        ),
    ));
    form = form.push(check(i18n.tr("opening-strict"), c.strict, true, |v| {
        edit(OpeningEdit::Strict(v))
    }));
    form = form.push(check(
        i18n.tr("opening-opponent-best"),
        c.opponent_best,
        true,
        |v| edit(OpeningEdit::OpponentBest(v)),
    ));
    form = form.push(check(
        i18n.tr("opening-no-repeat"),
        c.no_repeat,
        true,
        |v| edit(OpeningEdit::NoRepeat(v)),
    ));
    form = form.push(check(
        i18n.tr("opening-only-flagged"),
        c.only_flagged,
        true,
        |v| edit(OpeningEdit::OnlyFlagged(v)),
    ));
    form = form.push(check(
        i18n.tr("opening-show-candidates"),
        c.show_candidates,
        true,
        |v| edit(OpeningEdit::ShowCandidates(v)),
    ));
    form.push(check(
        i18n.tr("opening-only-marked"),
        c.only_marked,
        true,
        |v| edit(OpeningEdit::OnlyMarked(v)),
    ))
}

/// The dialog. `names` are the files the repertoire would be read from.
pub fn view<'a>(
    ui: &'a OpeningUi,
    view: &OpeningView,
    names: &[String],
    i18n: &I18n,
) -> Element<'a, Message> {
    let c = ui.shown(view).clone();
    let mut form = column![].spacing(10);
    form = source_rows(&c, names, i18n, form);
    form = option_rows(&c, i18n, form);
    let reset_label = if ui.reset_armed() {
        i18n.tr("opening-reset-confirm")
    } else {
        i18n.tr("opening-reset")
    };
    form = form.push(
        row![
            space::horizontal(),
            tooltip(
                button(text(reset_label).size(13))
                    .padding([3, 8])
                    .style(button::secondary)
                    .on_press(send(OpeningUiMessage::Reset)),
                text(i18n.tr("opening-reset-tip")).size(12),
                tooltip::Position::Top,
            )
            .style(theme::tooltip_box),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if let Some(issue) = view.store_issue {
        form = form.push(text(notice_text(issue, i18n)).size(13).style(text::danger));
    }
    if let Some(notice) = view.notice {
        form = form.push(text(notice_text(notice, i18n)).size(13).style(text::danger));
    }
    let body = container(scrollable(form.padding([0, 12])).height(Length::Fixed(BODY_HEIGHT)));
    modal(Card {
        id: OPENING_BOX_ID,
        title: i18n.tr("opening-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("opening-cancel")))
                .style(button::secondary)
                .on_press(send(OpeningUiMessage::Cancel))
                .into(),
            button(text(i18n.tr("opening-start")))
                .style(button::primary)
                .on_press(send(OpeningUiMessage::Submit))
                .into(),
        ],
        width: 560.0,
    })
}

/// The messages of the application for "Cancel".
pub fn cancel_message() -> mychess_app::Message {
    mychess_app::Message::OpeningTrainer(OpeningMessage::CloseDialog)
}
