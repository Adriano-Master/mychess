//! The checker of duplicate games: the two games of a pair side by side,
//! the differences in the colour of the accent (spec M7-1, section 6).

use fluent_bundle::FluentArgs;
use iced::widget::{checkbox, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};
use mychess_app::maintenance::data::{DiffToken, GameDiff};
use mychess_app::maintenance::{MaintenanceMessage, MaintenanceView, TwinPairView, TwinSide};

use super::{MAINT_CHECKER_ID, MaintForm, MaintUiMessage, ui};
use crate::shell::{Message, Shell};
use crate::theme;
use crate::windows::panel_kit::{BODY_TEXT, action, bold, danger, muted};

/// The most tokens of the moves one column draws (a game has far fewer; the
/// rest is cut so a pathological record cannot make the view heavy).
const MOVE_TOKENS: usize = 1500;

fn app(message: MaintenanceMessage) -> Message {
    ui(MaintUiMessage::App(message))
}

fn token<'a>(token: &DiffToken) -> Element<'a, Message> {
    let label = text(token.text.clone()).size(BODY_TEXT);
    if token.differs {
        label.font(bold()).style(text::danger).into()
    } else {
        label.into()
    }
}

fn column_of<'a>(
    shell: &Shell,
    number: u32,
    deleted: bool,
    side: TwinSide,
    header: &[DiffToken],
    moves: &[DiffToken],
) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut args = FluentArgs::new();
    args.set("game", i64::from(number));
    let mut lines = column![].spacing(1);
    for line in header {
        lines = lines.push(token(line));
    }
    let mut flow = row![].spacing(5);
    for item in moves.iter().take(MOVE_TOKENS) {
        flow = flow.push(token(item));
    }
    column![
        row![
            action(
                i18n.tr_args("maint-checker-game", &args),
                Some(ui(MaintUiMessage::LoadGame(number))),
            ),
            checkbox(deleted)
                .label(i18n.tr("maint-checker-deleted"))
                .size(15)
                .text_size(BODY_TEXT)
                .on_toggle(move |_| app(MaintenanceMessage::TwinToggle(side))),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        scrollable(column![lines, flow.wrap()].spacing(8))
            .height(Length::Fixed(380.0))
            .width(Length::Fill),
    ]
    .spacing(8)
    .width(Length::FillPortion(1))
    .into()
}

fn columns<'a>(shell: &Shell, pair: &TwinPairView) -> Element<'a, Message> {
    let empty: [DiffToken; 0] = [];
    let (ha, hb, ma, mb): (&[DiffToken], &[DiffToken], &[DiffToken], &[DiffToken]) =
        match pair.diff.as_deref() {
            Some(GameDiff::Compared {
                header_a,
                header_b,
                moves_a,
                moves_b,
            }) => (header_a, header_b, moves_a, moves_b),
            _ => (&empty, &empty, &empty, &empty),
        };
    let mut body = column![
        row![
            column_of(shell, pair.left, pair.left_deleted, TwinSide::Left, ha, ma),
            column_of(
                shell,
                pair.right,
                pair.right_deleted,
                TwinSide::Right,
                hb,
                mb
            ),
        ]
        .spacing(16)
    ]
    .spacing(8);
    match pair.diff.as_deref() {
        None => body = body.push(muted(shell.i18n.tr("maint-checker-comparing"))),
        Some(GameDiff::TooLarge { .. }) => {
            body = body.push(danger(shell.i18n.tr("maint-checker-too-large")));
        }
        Some(_) => {}
    }
    body.into()
}

/// The card of the checker, over the window.
pub(super) fn overlay<'a>(shell: &Shell, view: &MaintenanceView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let idle = !view.working() && view.confirm.is_none();
    let content: Element<'a, Message> = match shell.app.twin_pair_view() {
        Some(pair) => {
            let mut args = FluentArgs::new();
            args.set("position", i64::try_from(pair.position).unwrap_or(0));
            args.set("total", i64::try_from(pair.total).unwrap_or(0));
            column![
                text(i18n.tr_args("maint-checker-position", &args)).size(BODY_TEXT),
                columns(shell, &pair)
            ]
            .spacing(8)
            .into()
        }
        None => muted(i18n.tr("maint-notice-no-pair")),
    };
    let buttons = row![
        action(
            "<<".to_owned(),
            idle.then(|| app(MaintenanceMessage::TwinStep(-1)))
        ),
        action(
            ">>".to_owned(),
            idle.then(|| app(MaintenanceMessage::TwinStep(1)))
        ),
        action(
            i18n.tr("maint-checker-share"),
            idle.then(|| app(MaintenanceMessage::TwinShare)),
        ),
        action(
            i18n.tr("maint-checker-undelete-all"),
            idle.then(|| app(MaintenanceMessage::TwinUndeleteAll)),
        ),
        action(
            i18n.tr("maint-checker-delete-twins"),
            idle.then(|| ui(MaintUiMessage::OpenForm(MaintForm::Twins))),
        ),
        iced::widget::space::horizontal(),
        action(
            i18n.tr("maint-ui-close"),
            Some(app(MaintenanceMessage::CloseChecker))
        ),
    ]
    .spacing(6);
    let card = container(
        column![
            text(i18n.tr("maint-checker-title")).size(18),
            content,
            buttons
        ]
        .spacing(12)
        .width(Length::Fixed(1000.0)),
    )
    .id(MAINT_CHECKER_ID)
    .padding(20)
    .style(theme::pane_body);
    iced::widget::opaque(container(card).center(Length::Fill).style(theme::scrim))
}
