//! The part of the shell that serves the export dialogs (spec M7-4c): it
//! gives the application the piece drawings, the colors of the board and the
//! font that the viewer, the diagrams and the image need (D3723), keeps the
//! preview of the image window and takes its keys.

use std::sync::Arc;

use iced::Task;
use mychess_app::board_image::{BoardPalette, PieceSvgs};
use mychess_app::export_docs::ExportAssets;
use mychess_app::{KeyInput, Message as AppMessage};
use mychess_core::{Color, Role};

use super::{Message, Shell};
use crate::app_api;
use crate::assets;
use crate::theme::board_theme::BoardTheme;
use crate::widgets::board::pieces::PieceSetId;

/// `<name> by <author>, <license>` of a piece set (the table of
/// `assets/LICENSES.md`), written in the exported files (D3727).
pub(crate) fn pieces_credit(set: PieceSetId) -> &'static str {
    match set {
        PieceSetId::Cburnett => "Cburnett by Colin M.L. Burnett, GPL-2.0-or-later",
        PieceSetId::Merida => "Merida by Armando Hernandez Marroquin, GPL-2.0-or-later",
        PieceSetId::Chessnut => "Chessnut by Alexis Luengas, Apache-2.0",
        PieceSetId::Rhosgfx => "RhosGFX by RhosGFX, CC0-1.0",
        PieceSetId::Fantasy => "Fantasy by Maurizio Monge, MIT",
        PieceSetId::Spatial => "Spatial by Maurizio Monge, MIT",
        PieceSetId::Celtic => "Celtic by Maurizio Monge, MIT",
        PieceSetId::Mpchess => "MPChess by Maxime Chupin, GPL-3.0-or-later",
    }
}

/// `#rrggbb`, or `#rrggbbaa` when the colour is not opaque (D4312).
fn hex(color: iced::Color) -> String {
    let [r, g, b, a] = color.into_rgba8();
    if a == u8::MAX {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// The drawing of a piece of `set`, as text (an empty text is refused by
/// `PieceSvgs`, which the caller turns into "no assets").
fn piece_text(set: PieceSetId, color: Color, role: Role) -> String {
    let base = if color == Color::White { 0 } else { 6 };
    let offset = match role {
        Role::King => 0,
        Role::Queen => 1,
        Role::Rook => 2,
        Role::Bishop => 3,
        Role::Knight => 4,
        Role::Pawn => 5,
    };
    set.files()
        .get(base + offset)
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default()
}

/// The assets of `set` and the colors of `theme`.
pub(crate) fn build_assets(
    set: PieceSetId,
    theme: crate::theme::board_theme::BoardThemeId,
) -> Option<ExportAssets> {
    let pieces = PieceSvgs::new(|color, role| piece_text(set, color, role)).ok()?;
    let colors = BoardTheme::of(theme);
    Some(ExportAssets {
        pieces: Arc::new(pieces),
        palette: BoardPalette {
            light: hex(colors.light),
            dark: hex(colors.dark),
            highlight: hex(colors.last_move),
            margin: "#ffffff".to_owned(),
        },
        font: Arc::new(assets::NOTO_SANS_REGULAR.to_vec()),
        pieces_credit: Some(pieces_credit(set).to_owned()),
    })
}

impl Shell {
    /// Whether the window of the board image covers the main window.
    pub(super) fn export_image_layer_open(&self) -> bool {
        self.app.export_image_view().is_some()
    }

    /// Gives the application the assets of the piece set and the theme in
    /// use (when they change) and redraws the preview of the image window.
    pub(super) fn sync_export(&mut self) {
        let look = app_api::appearance(&self.app, self.options);
        let key = (look.piece_set, look.board.theme);
        if self.ui.dialogs.export_image.assets_key != Some(key) {
            self.ui.dialogs.export_image.assets_key = Some(key);
            let assets = build_assets(key.0, key.1);
            self.ui.dialogs.export_image.assets = assets.clone().map(Arc::new);
            self.app.set_export_assets(assets);
        }
        let view = self.app.export_image_view();
        self.ui.dialogs.export_image.sync_preview(view.as_ref());
    }

    /// A key while the image window is open: Esc closes (or answers "no"),
    /// Enter saves (or answers "no"); the keys of the program window act.
    pub(super) fn export_image_key(
        &mut self,
        input: &KeyInput,
        text_focused: bool,
    ) -> Task<Message> {
        use mychess_app::{Action, Key, Modifiers, NamedKey};
        let message: Option<AppMessage> = match (input.key, input.modifiers) {
            (Key::Named(NamedKey::Escape), Modifiers::NONE) => {
                crate::windows::export_image::escape_message(self)
            }
            (Key::Named(NamedKey::Enter), Modifiers::NONE) => {
                crate::windows::export_image::enter_message(self)
            }
            _ => {
                return match self.app.shortcuts.lookup_in(input, text_focused) {
                    Some(action @ (Action::Quit | Action::FullScreen)) => self.run_action(action),
                    _ => Task::none(),
                };
            }
        };
        match message {
            Some(message) => self.dispatch(message),
            None => Task::none(),
        }
    }
}

#[cfg(test)]
mod hex_tests {
    use super::hex;

    #[test]
    fn the_alpha_of_a_colour_is_kept_only_when_not_opaque() {
        assert_eq!(hex(iced::Color::from_rgb8(1, 2, 255)), "#0102ff");
        assert_eq!(hex(iced::Color::from_rgba8(1, 2, 255, 0.5)), "#0102ff80");
    }
}
