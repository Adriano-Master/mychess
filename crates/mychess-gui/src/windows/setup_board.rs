//! "Set up position" (spec M2-4, D194, D195, rules 49-56; Scid
//! `setupBoard`): an editable board, the palette of twelve pieces and the
//! controls for side to move, castling, en passant, counters and FEN.
//!
//! The draft (`SetupBoard`) and its verdict live in the application. The
//! board here is a grid of small mouse areas (64 widgets, a dialog that is
//! open for seconds), not the game board widget: it needs no animation or
//! marks.

use fluent_bundle::FluentArgs;
use iced::mouse::ScrollDelta;
use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, radio, row, svg, text, text_input,
};
use iced::{Element, Length};
use mychess_app::{
    AppState, Direction, Message as AppMessage, SetupAnalysis, SetupBoard, SetupMessage,
    SetupProblem,
};
use mychess_core::{CastlingSide, Color, File, Rank, Square};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::board_theme::{BoardTheme, BoardThemeId};
use crate::widgets::modal::{Card, modal};
use crate::widgets::piece_palette;
use crate::windows::dialog_ui::{DialogMessage, DialogUi, SETUP_FEN_ID};
use crate::windows::header_dialog::Labeled;

/// Id of the box.
pub const SETUP_BOX_ID: &str = "setup-box";
/// Side of a square of the board, in logical pixels.
pub const SQUARE: f32 = 44.0;

fn setup(message: SetupMessage) -> Message {
    Message::App(AppMessage::Setup(message))
}

fn dialog(message: DialogMessage) -> Message {
    Message::Dialog(message)
}

/// The squares in display order, top row first.
pub fn display_order(flipped: bool) -> Vec<Vec<Square>> {
    let ranks: Vec<Rank> = if flipped {
        Rank::ALL.to_vec()
    } else {
        Rank::ALL.iter().rev().copied().collect()
    };
    let files: Vec<File> = if flipped {
        File::ALL.iter().rev().copied().collect()
    } else {
        File::ALL.to_vec()
    };
    ranks
        .into_iter()
        .map(|rank| {
            files
                .iter()
                .map(|&file| Square::from_coords(file, rank))
                .collect()
        })
        .collect()
}

fn square_widget<'a>(
    board: &SetupBoard,
    ui: &DialogUi,
    square: Square,
    theme: &BoardTheme,
) -> Element<'a, Message> {
    let dark = (square.file() as usize + square.rank() as usize).is_multiple_of(2);
    let color = if dark { theme.dark } else { theme.light };
    let content: Element<'a, Message> = match board
        .piece_at(square)
        .and_then(|piece| ui.pieces.handle(piece))
    {
        Some(handle) => svg(handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => iced::widget::space().into(),
    };
    mouse_area(
        container(content)
            .width(Length::Fixed(SQUARE))
            .height(Length::Fixed(SQUARE))
            .style(move |_| container::Style {
                background: Some(color.into()),
                ..container::Style::default()
            }),
    )
    .on_press(dialog(DialogMessage::SetupPress(square)))
    .on_release(dialog(DialogMessage::SetupRelease(square)))
    .on_right_press(setup(SetupMessage::RightClickSquare(square)))
    .on_middle_press(setup(SetupMessage::MiddleClickSquare(square)))
    .into()
}

fn board_grid<'a>(board: &SetupBoard, ui: &DialogUi, theme: &BoardTheme) -> Element<'a, Message> {
    let mut grid = column![];
    for squares in display_order(board.flipped) {
        let mut line = row![];
        for square in squares {
            line = line.push(square_widget(board, ui, square, theme));
        }
        grid = grid.push(line);
    }
    mouse_area(grid)
        .on_scroll(|delta| {
            let up = match delta {
                ScrollDelta::Lines { y, .. } | ScrollDelta::Pixels { y, .. } => y > 0.0,
            };
            setup(SetupMessage::CycleBrush(if up {
                Direction::Prev
            } else {
                Direction::Next
            }))
        })
        .into()
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
}

/// The translated text of the reason a draft is refused.
pub fn problem_text(i18n: &I18n, problem: &SetupProblem) -> String {
    let mut args = FluentArgs::new();
    if let SetupProblem::MissingKing(c)
    | SetupProblem::TooManyKings(c)
    | SetupProblem::TooManyPieces(c)
    | SetupProblem::TooManyPawns(c) = problem
    {
        args.set("color", color_name(*c));
    }
    i18n.tr_args(problem.message_key(), &args)
}

fn verdict<'a>(board: &SetupBoard, i18n: &I18n) -> Element<'a, Message> {
    match &board.analysis {
        SetupAnalysis::Invalid(problem) => text(problem_text(i18n, problem))
            .style(text::danger)
            .size(13)
            .into(),
        SetupAnalysis::Valid { warnings, .. } => {
            let mut out = column![].spacing(2);
            for warning in warnings {
                let mut args = FluentArgs::new();
                if let mychess_app::SetupWarning::CastlingDiscarded(color, _) = warning {
                    args.set("color", color_name(*color));
                }
                out = out.push(
                    text(i18n.tr_args(warning.message_key(), &args))
                        .size(13)
                        .style(text::secondary),
                );
            }
            out.into()
        }
        _ => column![].into(),
    }
}

fn castling<'a>(board: &SetupBoard, i18n: &I18n) -> Element<'a, Message> {
    let entry = |key: &str, color, side, on| {
        checkbox(on)
            .label(i18n.tr(key))
            .on_toggle(move |_| setup(SetupMessage::ToggleCastling(color, side)))
    };
    let c = board.castling;
    row![
        entry(
            "setup-castle-white-short",
            Color::White,
            CastlingSide::KingSide,
            c.white_king
        ),
        entry(
            "setup-castle-white-long",
            Color::White,
            CastlingSide::QueenSide,
            c.white_queen
        ),
        entry(
            "setup-castle-black-short",
            Color::Black,
            CastlingSide::KingSide,
            c.black_king
        ),
        entry(
            "setup-castle-black-long",
            Color::Black,
            CastlingSide::QueenSide,
            c.black_queen
        ),
    ]
    .spacing(12)
    .wrap()
    .into()
}

fn ep_options() -> Vec<Labeled<Option<File>>> {
    let mut out = vec![Labeled {
        value: None,
        label: "-".to_owned(),
    }];
    out.extend(File::ALL.iter().map(|&file| Labeled {
        value: Some(file),
        label: file.char().to_string(),
    }));
    out
}

/// The "Chess960" box, with the number of the start position when the draft
/// is one (M9-6).
fn variant_row<'a>(board: &SetupBoard, i18n: &I18n) -> Element<'a, Message> {
    let mut variant = row![
        checkbox(board.is_chess960())
            .label(i18n.tr("setup-chess960"))
            .on_toggle(|on| setup(SetupMessage::SetChess960(on)))
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);
    if let Some(n) = board.chess960_number() {
        let mut args = FluentArgs::new();
        args.set("n", n);
        variant = variant.push(
            text(i18n.tr_args("setup-chess960-number", &args))
                .size(13)
                .style(text::secondary),
        );
    }
    variant.into()
}

fn controls<'a>(board: &SetupBoard, ui: &DialogUi, i18n: &I18n) -> Element<'a, Message> {
    let turn = row![
        text(i18n.tr("setup-side-to-move")),
        radio(
            i18n.tr("setup-white"),
            Color::White,
            Some(board.turn),
            |c| { setup(SetupMessage::SetTurn(c)) }
        ),
        radio(
            i18n.tr("setup-black"),
            Color::Black,
            Some(board.turn),
            |c| { setup(SetupMessage::SetTurn(c)) }
        ),
    ]
    .spacing(10);
    let ep_selected = Labeled {
        value: board.ep_file,
        label: board
            .ep_file
            .map_or_else(|| "-".to_owned(), |f| f.char().to_string()),
    };
    let counters = row![
        text(i18n.tr("setup-move-number")),
        text_input("1", &ui.setup.fullmove)
            .on_input(|t| dialog(DialogMessage::SetupFullmove(t)))
            .width(Length::Fixed(64.0))
            .padding(4),
        text(i18n.tr("setup-halfmove")),
        text_input("0", &ui.setup.halfmove)
            .on_input(|t| dialog(DialogMessage::SetupHalfmove(t)))
            .width(Length::Fixed(64.0))
            .padding(4),
    ]
    .spacing(8);
    let ep = row![
        text(i18n.tr("setup-ep-file")),
        pick_list(
            ep_options(),
            Some(ep_selected),
            |c: Labeled<Option<File>>| { setup(SetupMessage::SetEpFile(c.value)) }
        ),
    ]
    .spacing(8);
    let tool = |key: &str, message: SetupMessage| {
        button(text(i18n.tr(key)))
            .style(button::secondary)
            .on_press(setup(message))
    };
    let tools = row![
        tool("setup-clear-board", SetupMessage::Clear),
        tool("setup-initial-board", SetupMessage::Initial),
        tool("setup-flip", SetupMessage::Flip),
    ]
    .spacing(6);
    let tools2 = row![
        tool("setup-swap-colors", SetupMessage::SwapColours),
        tool("setup-invert", SetupMessage::MirrorRanks),
        tool("setup-transpose", SetupMessage::MirrorFiles),
    ]
    .spacing(6);
    let fen = column![
        text_input("", &board.fen_text)
            .id(SETUP_FEN_ID)
            .on_input(|t| setup(SetupMessage::FenEdited(t)))
            .on_submit(setup(SetupMessage::ApplyFenText))
            .padding(5),
        row![
            tool("setup-paste-fen", SetupMessage::PasteFen),
            tool("setup-clear-fen", SetupMessage::ClearFenText),
        ]
        .spacing(6),
    ]
    .spacing(6);
    let variant = variant_row(board, i18n);
    column![
        turn,
        variant,
        castling(board, i18n),
        ep,
        counters,
        tools,
        tools2,
        fen,
        verdict(board, i18n),
    ]
    .spacing(10)
    .width(Length::Fill)
    .into()
}

/// The box, when the application has the dialog open.
pub fn view<'a>(
    app: &AppState,
    ui: &DialogUi,
    i18n: &I18n,
    theme: BoardThemeId,
) -> Option<Element<'a, Message>> {
    let board = app.setup.as_ref()?;
    let colors = BoardTheme::of(theme);
    let left = column![
        board_grid(board, ui, &colors),
        piece_palette::view(&ui.pieces, board.brush, 30.0, |piece| {
            setup(SetupMessage::SelectBrush(piece))
        }),
        text(i18n.tr("setup-hint-mouse"))
            .size(12)
            .style(text::secondary),
    ]
    .spacing(8);
    let body = mouse_area(row![left, controls(board, ui, i18n)].spacing(20))
        .on_release(dialog(DialogMessage::SetupAreaRelease));
    Some(modal(Card {
        id: SETUP_BOX_ID,
        title: i18n.tr("setup-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(setup(SetupMessage::Cancel))
                .into(),
            button(text(i18n.tr("dialog-ok")))
                .on_press_maybe(board.analysis.is_valid().then(|| setup(SetupMessage::Ok)))
                .into(),
        ],
        width: 820.0,
    }))
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{Command, update};

    use super::*;
    use crate::i18n::Language;

    fn open() -> (AppState, DialogUi) {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Command(Command::SetupBoard));
        let mut ui = DialogUi::default();
        ui.sync(&app);
        (app, ui)
    }

    fn sim<'a>(app: &AppState, ui: &DialogUi, language: Language) -> Simulator<'a, Message> {
        let element = view(app, ui, &I18n::new(language), BoardThemeId::default()).expect("open");
        Simulator::with_size(Settings::default(), Size::new(1100.0, 760.0), element)
    }

    /// A click anywhere also reports the release to the area that wraps the
    /// dialog (a no-op unless a drag is on); tests look at the rest.
    fn without_releases(messages: impl Iterator<Item = Message>) -> Vec<Message> {
        messages
            .filter(|m| !matches!(m, Message::Dialog(DialogMessage::SetupAreaRelease)))
            .collect()
    }

    #[test]
    fn the_display_order_follows_the_orientation() {
        let white = display_order(false);
        assert_eq!(white[0][0], Square::A8);
        assert_eq!(white[7][7], Square::H1);
        let black = display_order(true);
        assert_eq!(black[0][0], Square::H1);
        assert_eq!(black[7][7], Square::A8);
        assert_eq!(white.iter().map(Vec::len).sum::<usize>(), 64);
    }

    #[test]
    fn closed_dialog_has_no_view() {
        let app = AppState::default();
        assert!(
            view(
                &app,
                &DialogUi::default(),
                &I18n::default(),
                BoardThemeId::default()
            )
            .is_none()
        );
    }

    #[test]
    fn ok_cancel_and_the_tools_send_the_setup_messages() {
        let (app, ui) = open();
        let mut s = sim(&app, &ui, Language::EnUs);
        for label in [
            "Clear board",
            "Initial board",
            "Flip board",
            "Swap colors",
            "Invert",
            "Transpose",
            "Cancel",
            "OK",
        ] {
            s.click(label).unwrap_or_else(|_| panic!("{label}"));
        }
        let sent = without_releases(s.into_messages());
        let kinds: Vec<&SetupMessage> = sent
            .iter()
            .filter_map(|m| match m {
                Message::App(AppMessage::Setup(s)) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                &SetupMessage::Clear,
                &SetupMessage::Initial,
                &SetupMessage::Flip,
                &SetupMessage::SwapColours,
                &SetupMessage::MirrorRanks,
                &SetupMessage::MirrorFiles,
                &SetupMessage::Cancel,
                &SetupMessage::Ok,
            ]
        );
    }

    #[test]
    fn an_invalid_position_disables_ok_and_names_the_problem() {
        let (mut app, mut ui) = open();
        update(&mut app, AppMessage::Setup(SetupMessage::Clear));
        // Take the white king off: right click erases it.
        update(
            &mut app,
            AppMessage::Setup(SetupMessage::RightClickSquare(Square::G2)),
        );
        ui.sync(&app);
        let mut s = sim(&app, &ui, Language::EnUs);
        assert!(s.find("There must be exactly one white king").is_ok());
        s.click("OK").expect("button exists");
        assert_eq!(s.into_messages().count(), 0, "OK is disabled");
        let mut s = sim(&app, &ui, Language::PtBr);
        assert!(s.find("Deve haver exatamente um rei branco").is_ok());
    }

    #[test]
    fn the_chess960_box_switches_the_mode_and_shows_the_number() {
        let (mut app, mut ui) = open();
        let mut s = sim(&app, &ui, Language::EnUs);
        assert!(s.find("Chess960 position 518").is_err(), "off by default");
        s.click("Chess960").expect("checkbox");
        let sent = without_releases(s.into_messages());
        assert!(
            sent.iter().any(|m| matches!(
                m,
                Message::App(AppMessage::Setup(SetupMessage::SetChess960(true)))
            )),
            "{sent:?}"
        );
        update(&mut app, AppMessage::Setup(SetupMessage::SetChess960(true)));
        ui.sync(&app);
        let mut s = sim(&app, &ui, Language::EnUs);
        assert!(s.find("Chess960 position 518").is_ok());
        let mut s = sim(&app, &ui, Language::PtBr);
        assert!(s.find("Posição Chess960 nº 518").is_ok());
    }

    #[test]
    fn castling_controls_toggle_through_the_application() {
        let (app, ui) = open();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("White O-O").expect("checkbox");
        let sent = without_releases(s.into_messages());
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Setup(
                SetupMessage::ToggleCastling(Color::White, CastlingSide::KingSide)
            ))]
        ));
    }

    #[test]
    fn the_fen_field_edits_and_applies() {
        let (app, ui) = open();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click(iced::widget::Id::new(SETUP_FEN_ID)).expect("fen");
        s.typewrite("x");
        s.tap_key(iced::keyboard::key::Named::Enter);
        let sent = without_releases(s.into_messages());
        assert!(matches!(
            sent.first(),
            Some(Message::App(AppMessage::Setup(SetupMessage::FenEdited(t)))) if t.contains('x')
        ));
        assert!(matches!(
            sent.last(),
            Some(Message::App(AppMessage::Setup(SetupMessage::ApplyFenText)))
        ));
    }

    #[test]
    fn the_paste_fen_button_asks_the_application_for_the_clipboard() {
        let (app, ui) = open();
        let mut s = sim(&app, &ui, Language::EnUs);
        s.click("Paste FEN").expect("button");
        let sent = without_releases(s.into_messages());
        assert!(matches!(
            sent.as_slice(),
            [Message::App(AppMessage::Setup(SetupMessage::PasteFen))]
        ));
    }
}
