//! The bookmark editor (spec M3-6, D678): a modal box over a working copy
//! of the tree that lives in `AppState::bookmark_editor`. OK keeps the
//! copy, Cancel (or Esc) drops it, Enter outside the title field is OK.
//!
//! The rows are the visible entries of the tree in order, indented by their
//! depth; folders are drawn in brackets. The list is not virtualized: the
//! tree has at most `MAX_BOOKMARKS` entries and is edited rarely.

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, text, text_input,
};
use iced::{Element, Length};
use mychess_app::{
    AppState, BookmarkEditor, BookmarkRow, BookmarksMessage, EditorMessage, Message as AppMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::theme::{self, RowLook};
use crate::widgets::modal::{Card, modal};

/// Id of the box.
pub const BOOKMARKS_BOX_ID: &str = "bookmarks-box";
/// Id of the title field.
pub const BOOKMARKS_TITLE_ID: &str = "bookmarks-title";

/// Height of the list, in logical pixels.
const LIST_HEIGHT: f32 = 300.0;
/// Indent per level of folders, in logical pixels.
const INDENT: f32 = 18.0;

fn send(message: EditorMessage) -> Message {
    Message::App(AppMessage::Bookmarks(BookmarksMessage::Edit(message)))
}

/// The text of a row: folders between brackets.
pub(crate) fn row_label(row: &BookmarkRow) -> String {
    if row.is_folder {
        format!("[{}]", row.title)
    } else {
        row.title.clone()
    }
}

fn list_row<'a>(entry: &BookmarkRow, selected: bool, zebra: bool) -> Element<'a, Message> {
    // The depth is at most `MAX_BOOKMARK_DEPTH`: it fits an `f32` exactly.
    #[allow(clippy::cast_precision_loss)]
    let indent = entry.depth as f32 * INDENT;
    let cells = row![space().width(Length::Fixed(indent)), text(row_label(entry))].spacing(0);
    let cell = container(cells)
        .padding([3, 8])
        .width(Length::Fill)
        .style(theme::list_row(RowLook {
            zebra,
            selected,
            ..RowLook::default()
        }));
    mouse_area(cell)
        .on_press(send(EditorMessage::Select(Some(entry.id))))
        .on_double_click(send(EditorMessage::Go))
        .into()
}

fn tool<'a>(label: String, message: Option<EditorMessage>) -> Element<'a, Message> {
    button(text(label))
        .style(button::secondary)
        .on_press_maybe(message.map(send))
        .into()
}

/// The box, when the application has the editor open.
pub fn view<'a>(app: &AppState, i18n: &I18n) -> Option<Element<'a, Message>> {
    let editor = app.bookmark_editor.as_ref()?;
    Some(editor_box(editor, app.can_bookmark_current(), i18n))
}

fn editor_box<'a>(editor: &BookmarkEditor, can_add: bool, i18n: &I18n) -> Element<'a, Message> {
    let rows = editor.working.rows();
    let selected = editor.selected;
    let selected_row = selected.and_then(|id| rows.iter().find(|r| r.id == id));
    let has_selection = selected_row.is_some();
    let on_game = selected_row.is_some_and(|r| !r.is_folder);
    let when = |on: bool, message: EditorMessage| on.then_some(message);

    let mut list = column![].width(Length::Fill);
    if rows.is_empty() {
        list = list
            .push(container(text(i18n.tr("bookmarks-empty")).style(theme::muted_text)).padding(8));
    }
    for (index, entry) in rows.iter().enumerate() {
        list = list.push(list_row(entry, selected == Some(entry.id), index % 2 == 1));
    }
    let list = scrollable(list).height(Length::Fixed(LIST_HEIGHT));

    let title = text_input(&i18n.tr("bookmarks-title-placeholder"), &editor.title)
        .id(BOOKMARKS_TITLE_ID)
        .on_input_maybe(has_selection.then_some(|t| send(EditorMessage::Rename(t))))
        .padding(5);

    let tools = row![
        tool(
            i18n.tr("bookmarks-up"),
            when(has_selection, EditorMessage::MoveUp)
        ),
        tool(
            i18n.tr("bookmarks-down"),
            when(has_selection, EditorMessage::MoveDown)
        ),
        tool(
            i18n.tr("bookmarks-indent"),
            when(has_selection, EditorMessage::Indent)
        ),
        tool(
            i18n.tr("bookmarks-outdent"),
            when(has_selection, EditorMessage::Outdent)
        ),
        space::horizontal(),
        tool(
            i18n.tr("bookmarks-new-folder"),
            Some(EditorMessage::NewFolder)
        ),
        tool(
            i18n.tr("bookmarks-add"),
            when(can_add, EditorMessage::AddCurrent)
        ),
        tool(
            i18n.tr("bookmarks-delete"),
            when(has_selection, EditorMessage::Delete)
        ),
        tool(i18n.tr("bookmarks-go"), when(on_game, EditorMessage::Go)),
    ]
    .spacing(6);

    let body = column![list, title, tools].spacing(8);
    modal(Card {
        id: BOOKMARKS_BOX_ID,
        title: i18n.tr("bookmarks-title"),
        body: body.into(),
        buttons: vec![
            button(text(i18n.tr("dialog-cancel")))
                .style(button::secondary)
                .on_press(send(EditorMessage::Cancel))
                .into(),
            button(text(i18n.tr("dialog-ok")))
                .on_press(send(EditorMessage::Ok))
                .into(),
        ],
        width: 620.0,
    })
}

#[cfg(test)]
mod tests {
    use iced::{Settings, Size};
    use iced_test::Simulator;
    use mychess_app::{Bookmarks, Folder, GameBookmark, NodeKind};

    use super::*;
    use crate::i18n::Language;

    fn game(title: &str) -> NodeKind {
        NodeKind::Game(GameBookmark {
            title: title.to_owned(),
            base: std::path::PathBuf::from("x.si4"),
            game: 1,
            ply: 0,
            white: String::new(),
            black: String::new(),
            year: 0,
            site: String::new(),
            round: String::new(),
            result: "*".to_owned(),
        })
    }

    fn editor() -> (BookmarkEditor, mychess_app::BookmarkId) {
        let mut working = Bookmarks::new();
        let folder = working
            .insert_after(
                None,
                NodeKind::Folder(Folder {
                    title: "Openings".to_owned(),
                    children: vec![],
                }),
            )
            .expect("folder");
        let id = working
            .insert_after(Some(folder), game("Sicilian"))
            .expect("game");
        (
            BookmarkEditor {
                working,
                selected: Some(id),
                title: "Sicilian".to_owned(),
            },
            id,
        )
    }

    fn sim(
        editor: &BookmarkEditor,
        can_add: bool,
        language: Language,
    ) -> Simulator<'static, Message> {
        Simulator::with_size(
            Settings::default(),
            Size::new(900.0, 600.0),
            editor_box(editor, can_add, &I18n::new(language)),
        )
    }

    fn sent(mut sim: Simulator<'static, Message>, click: &str) -> Vec<Message> {
        sim.click(click).expect("clickable");
        sim.into_messages().collect()
    }

    #[test]
    fn rows_show_folders_in_brackets_and_the_tree_in_order() {
        let (editor, _) = editor();
        let labels: Vec<_> = editor.working.rows().iter().map(row_label).collect();
        assert_eq!(labels, ["[Openings]", "Sicilian"]);
        let mut s = sim(&editor, false, Language::EnUs);
        assert!(s.find("[Openings]").is_ok());
        assert!(s.find("Sicilian").is_ok());
        assert!(s.find(iced::widget::Id::new(BOOKMARKS_BOX_ID)).is_ok());
    }

    #[test]
    fn buttons_send_the_editor_messages() {
        let (editor, id) = editor();
        for (label, expected) in [
            ("Up", EditorMessage::MoveUp),
            ("Down", EditorMessage::MoveDown),
            ("Indent", EditorMessage::Indent),
            ("Outdent", EditorMessage::Outdent),
            ("New folder", EditorMessage::NewFolder),
            ("Add", EditorMessage::AddCurrent),
            ("Delete", EditorMessage::Delete),
            ("Go to", EditorMessage::Go),
            ("OK", EditorMessage::Ok),
            ("Cancel", EditorMessage::Cancel),
        ] {
            let messages = sent(sim(&editor, true, Language::EnUs), label);
            assert!(
                matches!(
                    messages.as_slice(),
                    [Message::App(AppMessage::Bookmarks(BookmarksMessage::Edit(m)))] if *m == expected
                ),
                "{label}: {messages:?}"
            );
        }
        let messages = sent(sim(&editor, true, Language::EnUs), "Sicilian");
        assert!(matches!(
            messages.as_slice(),
            [Message::App(AppMessage::Bookmarks(BookmarksMessage::Edit(EditorMessage::Select(Some(i)))))]
                if *i == id
        ));
    }

    #[test]
    fn add_is_off_without_a_current_game_and_actions_need_a_selection() {
        let (mut editor, _) = editor();
        let mut s = sim(&editor, false, Language::EnUs);
        s.click("Add").expect("button exists");
        assert_eq!(s.into_messages().count(), 0);
        editor.selected = None;
        for label in ["Up", "Delete", "Go to"] {
            let mut s = sim(&editor, true, Language::EnUs);
            s.click(label).expect("button exists");
            assert_eq!(s.into_messages().count(), 0, "{label}");
        }
    }

    #[test]
    fn typing_in_the_title_field_renames() {
        let (editor, _) = editor();
        let mut s = sim(&editor, true, Language::EnUs);
        s.click(iced::widget::Id::new(BOOKMARKS_TITLE_ID))
            .expect("field");
        s.typewrite("!");
        let messages: Vec<Message> = s.into_messages().collect();
        assert!(matches!(
            messages.last(),
            Some(Message::App(AppMessage::Bookmarks(BookmarksMessage::Edit(
                EditorMessage::Rename(t)
            )))) if t == "Sicilian!"
        ));
    }

    #[test]
    fn the_box_is_translated() {
        let (editor, _) = editor();
        let mut s = sim(&editor, true, Language::PtBr);
        for text in [
            "Marcadores",
            "Subir",
            "Recuar",
            "Avançar",
            "Nova pasta",
            "Excluir",
        ] {
            assert!(s.find(text).is_ok(), "{text}");
        }
    }
}
