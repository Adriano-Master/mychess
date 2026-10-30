//! The game information dialog: the tags of the game (spec M2-4, D191,
//! D192, rules 37-44; Scid `gameSave -1`).
//!
//! The draft, its validation and the suggestions belong to the application
//! (`GameInfoDialog`); this module draws them and forwards the edits.

use std::fmt;

use iced::widget::{button, column, pick_list, row, scrollable, text, text_input};
use iced::{Element, Length};
use mychess_app::{
    AppState, DatePart, DateTarget, FieldIssue, GameInfoDialog, GameInfoMessage, IssueSeverity,
    Message as AppMessage, PlayerSide, TagField,
};
use mychess_core::{GameResult, RatingType};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::widgets::autocomplete;
use crate::widgets::modal::{Card, modal};
use crate::windows::dialog_ui::{DialogMessage, DialogUi, field_id};

/// Id of the box.
pub const HEADER_BOX_ID: &str = "header-box";

fn info(message: GameInfoMessage) -> Message {
    Message::App(AppMessage::Info(message))
}

fn dialog(message: DialogMessage) -> Message {
    Message::Dialog(message)
}

/// A value with the text a list shows for it.
#[derive(Debug, Clone)]
pub(crate) struct Labeled<T> {
    pub(crate) value: T,
    pub(crate) label: String,
}

impl<T> fmt::Display for Labeled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl<T: PartialEq> PartialEq for Labeled<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

const RESULTS: [GameResult; 4] = [
    GameResult::White,
    GameResult::Black,
    GameResult::Draw,
    GameResult::Unknown,
];

fn result_options() -> Vec<Labeled<GameResult>> {
    RESULTS
        .into_iter()
        .map(|value| Labeled {
            value,
            label: value.as_str().to_owned(),
        })
        .collect()
}

fn rating_options() -> Vec<Labeled<RatingType>> {
    RatingType::ALL
        .into_iter()
        .map(|value| Labeled {
            value,
            label: value.name().to_owned(),
        })
        .collect()
}

fn labeled_row<'a>(
    label: String,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![text(label).width(Length::Fixed(110.0)), content.into()]
        .spacing(8)
        .align_y(iced::Alignment::Start)
        .into()
}

fn name_input<'a>(
    d: &'a GameInfoDialog,
    ui: &DialogUi,
    visible: bool,
    field: TagField,
    value: &str,
) -> Element<'a, Message> {
    let rows: Vec<String> = if visible && d.suggestion_field == Some(field) {
        d.suggestions.iter().map(|m| m.name.clone()).collect()
    } else {
        Vec::new()
    };
    autocomplete::view(
        field_id(field),
        "",
        value,
        &rows,
        ui.header.highlight,
        move |t| dialog(DialogMessage::HeaderText(field, t)),
        dialog(DialogMessage::HeaderSubmit),
        move |i| dialog(DialogMessage::HeaderPick(field, i)),
    )
}

fn date_inputs<'a>(
    target: DateTarget,
    ids: [&'static str; 3],
    parts: [&str; 3],
) -> Element<'a, Message> {
    let mut out = row![].spacing(4);
    for (i, (part, placeholder)) in [
        (DatePart::Year, "YYYY"),
        (DatePart::Month, "MM"),
        (DatePart::Day, "DD"),
    ]
    .into_iter()
    .enumerate()
    {
        let width = if i == 0 { 70.0 } else { 48.0 };
        out = out.push(
            text_input(placeholder, parts[i])
                .id(ids[i])
                .on_input(move |t| info(GameInfoMessage::SetDate(target, part, t)))
                .on_submit(dialog(DialogMessage::HeaderSubmit))
                .padding(5)
                .width(Length::Fixed(width)),
        );
    }
    out.into()
}

fn rating_inputs<'a>(
    side: PlayerSide,
    id: &'static str,
    value: &str,
    kind: RatingType,
    selected_label: &str,
) -> Element<'a, Message> {
    let current = Labeled {
        value: kind,
        label: selected_label.to_owned(),
    };
    row![
        text_input("0", value)
            .id(id)
            .on_input(move |t| info(GameInfoMessage::SetRating(side, t)))
            .on_submit(dialog(DialogMessage::HeaderSubmit))
            .padding(5)
            .width(Length::Fixed(70.0)),
        pick_list(
            rating_options(),
            Some(current),
            move |choice: Labeled<RatingType>| {
                info(GameInfoMessage::SetRatingKind(side, choice.value))
            }
        ),
    ]
    .spacing(4)
    .into()
}

fn issues<'a>(list: &[FieldIssue], i18n: &I18n) -> Element<'a, Message> {
    let mut out = column![].spacing(2);
    for issue in list {
        let mut args = fluent_bundle::FluentArgs::new();
        if let mychess_app::FieldIssueKind::ExceedsDatabaseLimit { max } = &issue.kind {
            args.set("max", i64::try_from(*max).unwrap_or(i64::MAX));
        }
        let line = text(i18n.tr_args(issue.kind.message_key(), &args)).size(13);
        out = out.push(match issue.severity {
            IssueSeverity::Blocking => line.style(text::danger),
            IssueSeverity::Warning => line.style(text::secondary),
        });
    }
    out.into()
}

fn extras<'a>(d: &GameInfoDialog, i18n: &I18n) -> Element<'a, Message> {
    let mut out = column![].spacing(4);
    for (index, (name, value)) in d.draft.extra.iter().enumerate() {
        let (n, v) = (name.clone(), value.clone());
        out = out.push(
            row![
                text_input(&i18n.tr("header-tag-name"), name)
                    .on_input(move |t| info(GameInfoMessage::SetExtra {
                        index,
                        name: t,
                        value: v.clone(),
                    }))
                    .padding(5)
                    .width(Length::FillPortion(2)),
                text_input(&i18n.tr("header-tag-value"), value)
                    .on_input(move |t| info(GameInfoMessage::SetExtra {
                        index,
                        name: n.clone(),
                        value: t,
                    }))
                    .on_submit(dialog(DialogMessage::HeaderSubmit))
                    .padding(5)
                    .width(Length::FillPortion(3)),
                button(text("×"))
                    .style(button::secondary)
                    .on_press(info(GameInfoMessage::RemoveExtra(index))),
            ]
            .spacing(4),
        );
    }
    out = out.push(
        button(text(i18n.tr("header-add-tag")))
            .style(button::secondary)
            .on_press(info(GameInfoMessage::AddExtra)),
    );
    out.into()
}

/// The name and rating inputs of one player.
fn player_row<'a>(
    d: &'a GameInfoDialog,
    ui: &DialogUi,
    visible: bool,
    side: PlayerSide,
    i18n: &I18n,
) -> Element<'a, Message> {
    let draft = &d.draft;
    let (label, field, name, elo_id, rating) = match side {
        PlayerSide::White => (
            "header-white",
            TagField::White,
            &draft.white,
            "hdr-white-elo",
            &draft.white_rating,
        ),
        PlayerSide::Black => (
            "header-black",
            TagField::Black,
            &draft.black,
            "hdr-black-elo",
            &draft.black_rating,
        ),
    };
    labeled_row(
        i18n.tr(label),
        row![
            name_input(d, ui, visible, field, name),
            rating_inputs(side, elo_id, &rating.value, rating.kind, rating.kind.name()),
        ]
        .spacing(8),
    )
}

fn form<'a>(
    app: &'a AppState,
    ui: &DialogUi,
    d: &'a GameInfoDialog,
    i18n: &I18n,
) -> Element<'a, Message> {
    let visible = ui.suggestions_visible(app);
    let draft = &d.draft;
    let result = Labeled {
        value: draft.result,
        label: draft.result.as_str().to_owned(),
    };
    let date = |dt: &mychess_app::DateDraft| [dt.year.clone(), dt.month.clone(), dt.day.clone()];
    let (dy, ey) = (date(&draft.date), date(&draft.event_date));
    let today = |target| {
        button(text(i18n.tr("header-today")))
            .style(button::secondary)
            .on_press(info(GameInfoMessage::Today(target)))
    };
    let tags: Element<'a, Message> = column![
        player_row(d, ui, visible, PlayerSide::White, i18n),
        player_row(d, ui, visible, PlayerSide::Black, i18n),
        labeled_row(
            i18n.tr("header-event"),
            name_input(d, ui, visible, TagField::Event, &draft.event)
        ),
        labeled_row(
            i18n.tr("header-site"),
            name_input(d, ui, visible, TagField::Site, &draft.site)
        ),
        labeled_row(
            i18n.tr("header-round"),
            name_input(d, ui, visible, TagField::Round, &draft.round)
        ),
        labeled_row(
            i18n.tr("header-date"),
            row![
                date_inputs(
                    DateTarget::Date,
                    ["hdr-date-year", "hdr-date-month", "hdr-date-day"],
                    [&dy[0], &dy[1], &dy[2]]
                ),
                today(DateTarget::Date),
            ]
            .spacing(8)
        ),
        labeled_row(
            i18n.tr("header-event-date"),
            row![
                date_inputs(
                    DateTarget::EventDate,
                    [
                        "hdr-event-date-year",
                        "hdr-event-date-month",
                        "hdr-event-date-day"
                    ],
                    [&ey[0], &ey[1], &ey[2]]
                ),
                today(DateTarget::EventDate),
                button(text(i18n.tr("header-same-as-date")))
                    .style(button::secondary)
                    .on_press(info(GameInfoMessage::CopyDateToEventDate)),
            ]
            .spacing(8)
        ),
        labeled_row(
            i18n.tr("header-result"),
            pick_list(result_options(), Some(result), |c: Labeled<GameResult>| {
                info(GameInfoMessage::SetResult(c.value))
            })
        ),
        labeled_row(
            i18n.tr("header-eco"),
            row![
                text_input("", &draft.eco)
                    .id("hdr-eco")
                    .on_input(|t| info(GameInfoMessage::SetText(TagField::Eco, t)))
                    .on_submit(dialog(DialogMessage::HeaderSubmit))
                    .padding(5)
                    .width(Length::Fixed(80.0)),
                button(text(i18n.tr("header-classify")))
                    .style(button::secondary)
                    .on_press_maybe(
                        app.eco_book
                            .is_some()
                            .then(|| info(GameInfoMessage::ClassifyEco))
                    ),
            ]
            .spacing(8)
        ),
        text(i18n.tr("header-extra-tags")).size(14),
        extras(d, i18n),
        issues(&d.issues, i18n),
    ]
    .spacing(6)
    .into();
    scrollable(tags).height(Length::Fixed(470.0)).into()
}

/// The box, when the application has the dialog open.
pub fn view<'a>(app: &'a AppState, ui: &'a DialogUi, i18n: &I18n) -> Option<Element<'a, Message>> {
    let d = app.info_dialog.as_ref()?;
    Some(modal(Card {
        id: HEADER_BOX_ID,
        title: i18n.tr("header-title"),
        body: form(app, ui, d, i18n),
        buttons: vec![
            button(text(i18n.tr("header-use-previous")))
                .style(button::secondary)
                .on_press_maybe(
                    d.last_tags
                        .is_some()
                        .then(|| info(GameInfoMessage::UsePrevious)),
                )
                .into(),
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(info(GameInfoMessage::Cancel))
                .into(),
            button(text(i18n.tr("dialog-ok")))
                .on_press(info(GameInfoMessage::Apply))
                .into(),
        ],
        width: 760.0,
    }))
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{NameKind, update};

    use super::*;
    use crate::i18n::Language;

    fn open_app() -> AppState {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Info(GameInfoMessage::Open));
        app
    }

    fn sim<'a>(app: &'a AppState, ui: &'a DialogUi, language: Language) -> Simulator<'a, Message> {
        let element = view(app, ui, &I18n::new(language)).expect("open");
        Simulator::with_size(Settings::default(), Size::new(1100.0, 800.0), element)
    }

    #[test]
    fn closed_dialog_has_no_view() {
        assert!(view(&AppState::default(), &DialogUi::default(), &I18n::default()).is_none());
    }

    #[test]
    fn ok_and_cancel_emit_apply_and_cancel() {
        let app = open_app();
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("OK").expect("ok");
        s.click("Cancel").expect("cancel");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::App(AppMessage::Info(GameInfoMessage::Apply)),
                Message::App(AppMessage::Info(GameInfoMessage::Cancel)),
            ]
        ));
    }

    #[test]
    fn editing_a_name_goes_through_the_dialog_so_the_list_resets() {
        let app = open_app();
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click(iced::widget::Id::new("hdr-white")).expect("white");
        s.typewrite("K");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.last(),
            Some(Message::Dialog(DialogMessage::HeaderText(TagField::White, t))) if t == "?K"
        ));
    }

    #[test]
    fn date_and_rating_fields_send_app_edits() {
        let app = open_app();
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click(iced::widget::Id::new("hdr-date-year"))
            .expect("year");
        s.typewrite("2");
        s.click(iced::widget::Id::new("hdr-white-elo"))
            .expect("elo");
        s.typewrite("9");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(sent.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::Info(GameInfoMessage::SetDate(DateTarget::Date, DatePart::Year, t))) if t == "2"
        )));
        assert!(sent.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::Info(GameInfoMessage::SetRating(PlayerSide::White, t))) if t == "9"
        )));
    }

    #[test]
    fn today_copy_and_extra_tag_buttons_send_their_messages() {
        let app = open_app();
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("=Date").expect("copy");
        s.click("Add").expect("add");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [
                Message::App(AppMessage::Info(GameInfoMessage::CopyDateToEventDate)),
                Message::App(AppMessage::Info(GameInfoMessage::AddExtra)),
            ]
        ));
    }

    #[test]
    fn classify_needs_a_book_and_use_previous_needs_a_previous_game() {
        let app = open_app();
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("Classify").expect("button exists");
        s.click("Use previous").expect("button exists");
        assert_eq!(s.into_messages().count(), 0, "both are disabled");
    }

    #[test]
    fn suggestions_appear_under_the_field_and_a_click_picks_one() {
        let mut app = open_app();
        app.names.observe(NameKind::Player, "Carlsen, M.");
        update(
            &mut app,
            AppMessage::Info(GameInfoMessage::SetText(TagField::White, "car".to_owned())),
        );
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("Carlsen, M.").expect("suggestion");
        let sent: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            sent.as_slice(),
            [Message::Dialog(DialogMessage::HeaderPick(
                TagField::White,
                0
            ))]
        ));
        let mut hidden = DialogUi::default();
        hidden.header.hidden = true;
        let mut s = sim(&app, &hidden, Language::EnUs);
        assert!(s.find("Carlsen, M.").is_err());
    }

    #[test]
    fn blocking_problems_are_listed_in_the_language_of_the_user() {
        let mut app = open_app();
        update(
            &mut app,
            AppMessage::Info(GameInfoMessage::SetRating(
                PlayerSide::White,
                "5000".to_owned(),
            )),
        );
        update(&mut app, AppMessage::Info(GameInfoMessage::Apply));
        assert!(app.info_dialog.is_some(), "the dialog stays open");
        let ui = DialogUi::default();
        let mut s = sim(&app, &ui, Language::PtBr);
        assert!(s.find("O rating deve estar entre 0 e 4000.").is_ok());
    }
}
