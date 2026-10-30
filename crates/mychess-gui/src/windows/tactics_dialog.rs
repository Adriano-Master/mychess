//! The configuration dialog of the tactics training and the visual state
//! that goes with it (spec M8-4c, D1938).
//!
//! The application keeps the last configuration; what is edited here is a
//! copy (`TacticsUi::draft`) that exists only while the user changes
//! something, so the values that the application reads from `tactics.toml`
//! after the dialog opened are shown until then.

use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, space, text, tooltip};
use iced::{Alignment, Element, Length};
use mychess_app::AppState;
use mychess_app::tactics::{
    SourceKind, TacticsConfig, TacticsMessage, TacticsMode, TacticsOrder, TacticsView,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme;
use crate::widgets::modal::{Card, modal};
use crate::windows::annotate_dialog::{check, choice, group, labelled};
use crate::windows::tactics_text::{beyond_text, notice_text, stats_text};

/// Id of the configuration box.
pub const TACTICS_BOX_ID: &str = "tactics-box";

const BODY_HEIGHT: f32 = 440.0;
const ELO_STEPS: [u16; 9] = [0, 1000, 1200, 1400, 1600, 1800, 2000, 2200, 2400];
const ATTEMPT_STEPS: [u8; 5] = [1, 2, 3, 5, 9];
const TIME_STEPS: [u16; 5] = [0, 30, 60, 120, 300];

/// One change of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TacticsEdit {
    /// Mate in N or the best move.
    Mode(TacticsMode),
    /// The order of the problems.
    Order(TacticsOrder),
    /// Where the problems come from.
    Source(SourceKind),
    /// Shortest mate.
    MateMin(u8),
    /// Longest mate.
    MateMax(u8),
    /// Easiest difficulty.
    DifficultyMin(u8),
    /// Hardest difficulty.
    DifficultyMax(u8),
    /// Lowest Elo.
    EloMin(u16),
    /// Highest Elo.
    EloMax(u16),
    /// Show the solved problems again.
    IncludeSolved(bool),
    /// Attempts per problem.
    Attempts(u8),
    /// The hint button works.
    Hints(bool),
    /// A longer mate is accepted.
    AcceptLonger(bool),
    /// Time limit in seconds.
    TimeLimit(u16),
    /// Look for mates in the games without markers (Mate in N).
    ScanMates(bool),
    /// Ask the engine about a move that is not the one of the game.
    SecondOpinion(bool),
    /// Only the games of a database with the Tactics flag.
    FlaggedOnly(bool),
}

/// An event of the dialog.
#[derive(Debug, Clone)]
pub enum TacticsUiMessage {
    /// Opens the dialog.
    Open,
    /// A change of the form.
    Edit(TacticsEdit),
    /// "Choose..." of the PGN file.
    PickFile,
    /// The file dialog answered.
    FilePicked(Option<PathBuf>),
    /// "Start".
    Submit,
    /// "Cancel".
    Cancel,
    /// "Reset score": the first click arms it, the second does it (D1927).
    Reset,
}

/// The visual state of the dialog.
#[derive(Debug, Default)]
pub struct TacticsUi {
    draft: Option<TacticsConfig>,
    reset_armed: bool,
    hint_arrows: Vec<mychess_app::EngineArrow>,
}

impl TacticsUi {
    /// Forgets the draft when the dialog is gone.
    pub fn sync(&mut self, app: &AppState) {
        self.hint_arrows = crate::windows::tactics_panel::hint_arrows(app);
        if !app.tactics_dialog_open() {
            self.draft = None;
            self.reset_armed = false;
        }
    }

    /// The arrow of the hint to draw on the board (D1940).
    pub fn arrows(&self) -> &[mychess_app::EngineArrow] {
        &self.hint_arrows
    }

    /// The configuration the form shows.
    pub fn shown<'a>(&'a self, view: &'a TacticsView) -> &'a TacticsConfig {
        self.draft.as_ref().unwrap_or(&view.config)
    }

    /// Whether "Reset score" waits for its confirmation.
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
    pub fn edit(&mut self, view: &TacticsView, edit: TacticsEdit) {
        self.reset_armed = false;
        let d = self.draft.get_or_insert_with(|| view.config.clone());
        match edit {
            TacticsEdit::Mode(v) => d.mode = v,
            TacticsEdit::Order(v) => d.order = v,
            TacticsEdit::Source(v) => d.source = v,
            TacticsEdit::MateMin(v) => d.mate_min = v,
            TacticsEdit::MateMax(v) => d.mate_max = v,
            TacticsEdit::DifficultyMin(v) => d.difficulty_min = v,
            TacticsEdit::DifficultyMax(v) => d.difficulty_max = v,
            TacticsEdit::EloMin(v) => d.elo_min = v,
            TacticsEdit::EloMax(v) => d.elo_max = v,
            TacticsEdit::IncludeSolved(v) => d.include_solved = v,
            TacticsEdit::Attempts(v) => d.max_attempts = v,
            TacticsEdit::Hints(v) => d.hints = v,
            TacticsEdit::AcceptLonger(v) => d.accept_longer = v,
            TacticsEdit::ScanMates(v) => d.scan_mates = v,
            TacticsEdit::SecondOpinion(v) => d.second_opinion = v,
            TacticsEdit::FlaggedOnly(v) => d.flagged_only = v,
            TacticsEdit::TimeLimit(v) => d.time_limit_s = v,
        }
        *d = d.clone().sanitized();
    }

    /// Sets the file of the draft.
    pub fn set_file(&mut self, view: &TacticsView, path: PathBuf) {
        self.reset_armed = false;
        let d = self.draft.get_or_insert_with(|| view.config.clone());
        d.file = Some(path);
        d.source = SourceKind::File;
    }

    /// The configuration to start with.
    pub fn to_start(&self, view: &TacticsView) -> TacticsConfig {
        self.shown(view).clone().sanitized()
    }

    /// The message that starts a session with the form as it is. The seed
    /// of the random order comes from the clock here, because the
    /// application never reads it (D1915).
    pub fn start_message(&self, view: &TacticsView) -> mychess_app::Message {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(0));
        mychess_app::Message::Tactics(TacticsMessage::Start {
            config: Box::new(self.to_start(view)),
            seed,
        })
    }
}

fn send(message: TacticsUiMessage) -> Message {
    Message::TacticsUi(message)
}

fn edit(edit: TacticsEdit) -> Message {
    send(TacticsUiMessage::Edit(edit))
}

fn number_choices<'a, T>(
    values: &[T],
    current: T,
    label: impl Fn(T) -> String,
    make: impl Fn(T) -> TacticsEdit + Copy + 'a,
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

fn off_or(i18n: &I18n, n: u32) -> String {
    if n == 0 {
        i18n.tr("tactics-off")
    } else {
        n.to_string()
    }
}

fn file_name(config: &TacticsConfig, i18n: &I18n) -> String {
    config.file.as_ref().map_or_else(
        || i18n.tr("tactics-no-file"),
        |path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        },
    )
}

/// The dialog.
fn source_rows<'a>(
    c: &TacticsConfig,
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    form = form.push(labelled(
        i18n.tr("tactics-mode"),
        group(vec![
            choice(
                i18n.tr("tactics-mode-mate"),
                TacticsMode::Mate,
                c.mode,
                true,
                |v| edit(TacticsEdit::Mode(v)),
            ),
            choice(
                i18n.tr("tactics-mode-best"),
                TacticsMode::BestMove,
                c.mode,
                true,
                |v| edit(TacticsEdit::Mode(v)),
            ),
        ]),
    ));
    let mut source = row![
        choice(
            i18n.tr("tactics-source-file"),
            SourceKind::File,
            c.source,
            true,
            |v| edit(TacticsEdit::Source(v))
        ),
        choice(
            i18n.tr("tactics-source-base"),
            SourceKind::CurrentBase,
            c.source,
            true,
            |v| edit(TacticsEdit::Source(v))
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if c.source == SourceKind::File {
        source = source.push(
            button(text(i18n.tr("tactics-choose-file")).size(13))
                .padding([3, 8])
                .style(button::secondary)
                .on_press(send(TacticsUiMessage::PickFile)),
        );
    }
    form = form.push(labelled(i18n.tr("tactics-source"), source.into()));
    if c.source == SourceKind::File {
        form = form.push(labelled(
            String::new(),
            text(file_name(c, i18n))
                .size(12)
                .style(theme::muted_text)
                .into(),
        ));
    }
    form = form.push(labelled(
        i18n.tr("tactics-order"),
        group(vec![
            choice(
                i18n.tr("tactics-order-sequential"),
                TacticsOrder::Sequential,
                c.order,
                true,
                |v| edit(TacticsEdit::Order(v)),
            ),
            choice(
                i18n.tr("tactics-order-random"),
                TacticsOrder::Random,
                c.order,
                true,
                |v| edit(TacticsEdit::Order(v)),
            ),
        ]),
    ));
    form
}

fn filter_rows<'a>(
    c: &TacticsConfig,
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    if c.mode == TacticsMode::Mate {
        let steps: Vec<u8> = (1..=5).collect();
        form = form.push(labelled(
            i18n.tr("tactics-mate-min"),
            number_choices(&steps, c.mate_min, |n| n.to_string(), TacticsEdit::MateMin),
        ));
        form = form.push(labelled(
            i18n.tr("tactics-mate-max"),
            number_choices(&steps, c.mate_max, |n| n.to_string(), TacticsEdit::MateMax),
        ));
    }
    let levels: Vec<u8> = (0..=9).collect();
    form = form.push(labelled(
        i18n.tr("tactics-difficulty-min"),
        number_choices(
            &levels,
            c.difficulty_min,
            |n| off_or(i18n, u32::from(n)),
            TacticsEdit::DifficultyMin,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("tactics-difficulty-max"),
        number_choices(
            &levels,
            c.difficulty_max,
            |n| off_or(i18n, u32::from(n)),
            TacticsEdit::DifficultyMax,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("tactics-elo-min"),
        number_choices(
            &ELO_STEPS,
            c.elo_min,
            |n| off_or(i18n, u32::from(n)),
            TacticsEdit::EloMin,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("tactics-elo-max"),
        number_choices(
            &ELO_STEPS,
            c.elo_max,
            |n| off_or(i18n, u32::from(n)),
            TacticsEdit::EloMax,
        ),
    ));
    form
}

fn limit_rows<'a>(
    c: &TacticsConfig,
    i18n: &I18n,
    mut form: iced::widget::Column<'a, Message>,
) -> iced::widget::Column<'a, Message> {
    form = form.push(labelled(
        i18n.tr("tactics-attempts-label"),
        number_choices(
            &ATTEMPT_STEPS,
            c.max_attempts,
            |n| n.to_string(),
            TacticsEdit::Attempts,
        ),
    ));
    form = form.push(labelled(
        i18n.tr("tactics-time-limit-label"),
        number_choices(
            &TIME_STEPS,
            c.time_limit_s,
            |n| off_or(i18n, u32::from(n)),
            TacticsEdit::TimeLimit,
        ),
    ));
    form = form.push(check(
        i18n.tr("tactics-include-solved"),
        c.include_solved,
        true,
        |v| edit(TacticsEdit::IncludeSolved(v)),
    ));
    form = form.push(check(i18n.tr("tactics-hints"), c.hints, true, |v| {
        edit(TacticsEdit::Hints(v))
    }));
    if c.mode == TacticsMode::Mate {
        form = form.push(check(
            i18n.tr("tactics-accept-longer"),
            c.accept_longer,
            true,
            |v| edit(TacticsEdit::AcceptLonger(v)),
        ));
        form = form.push(check(
            i18n.tr("tactics-scan-mates"),
            c.scan_mates,
            true,
            |v| edit(TacticsEdit::ScanMates(v)),
        ));
    } else {
        form = form.push(check(
            i18n.tr("tactics-second-opinion"),
            c.second_opinion,
            true,
            |v| edit(TacticsEdit::SecondOpinion(v)),
        ));
    }
    form = form.push(check(
        i18n.tr("tactics-flagged-only"),
        c.flagged_only,
        true,
        |v| edit(TacticsEdit::FlaggedOnly(v)),
    ));
    form
}

/// The dialog.
pub fn view<'a>(ui: &'a TacticsUi, view: &TacticsView, i18n: &I18n) -> Element<'a, Message> {
    let c = ui.shown(view).clone();
    let mut form = column![].spacing(10);
    form = source_rows(&c, i18n, form);
    form = filter_rows(&c, i18n, form);
    form = limit_rows(&c, i18n, form);
    let reset_label = if ui.reset_armed() {
        i18n.tr("tactics-reset-confirm")
    } else {
        i18n.tr("tactics-reset")
    };
    let mode_stats = if c.mode == view.mode {
        view.stats
    } else {
        // The score of the other mode is shown after the next start.
        mychess_app::tactics::ModeStats::default()
    };
    form = form.push(
        row![
            text(stats_text(&mode_stats, i18n))
                .size(12)
                .style(theme::muted_text),
            space::horizontal(),
            tooltip(
                button(text(reset_label).size(13))
                    .padding([3, 8])
                    .style(button::secondary)
                    .on_press(send(TacticsUiMessage::Reset)),
                text(i18n.tr("tactics-reset-tip")).size(12),
                tooltip::Position::Top,
            )
            .style(theme::tooltip_box),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if view.beyond_limit > 0 {
        form = form.push(
            text(beyond_text(view, i18n))
                .size(13)
                .style(theme::muted_text),
        );
    }
    if let Some(issue) = view.store_issue {
        form = form.push(text(notice_text(issue, i18n)).size(13).style(text::danger));
    }
    if let Some(notice) = view.notice {
        form = form.push(text(notice_text(notice, i18n)).size(13).style(text::danger));
    }
    let body = container(scrollable(form.padding([0, 12])).height(Length::Fixed(BODY_HEIGHT)));
    modal(Card {
        id: TACTICS_BOX_ID,
        title: i18n.tr("tactics-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("tactics-cancel")))
                .style(button::secondary)
                .on_press(send(TacticsUiMessage::Cancel))
                .into(),
            button(text(i18n.tr("tactics-start")))
                .style(button::primary)
                .on_press(send(TacticsUiMessage::Submit))
                .into(),
        ],
        width: 560.0,
    })
}

/// The messages of the application for "Cancel".
pub fn cancel_message() -> mychess_app::Message {
    mychess_app::Message::Tactics(TacticsMessage::CloseDialog)
}
