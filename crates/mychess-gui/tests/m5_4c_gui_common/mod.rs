//! Helpers of the `m5_4c_gui_*` contract tests (spec M5-4, slice M5-4c: the
//! interface of the tree masks), headless. They sit on the rig of the
//! opening tree tests, which runs the tasks the shell returns.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

#[path = "../m5_2_gui_common/mod.rs"]
pub mod base;

use std::path::{Path, PathBuf};

pub use base::*;
pub use mychess_app::{
    FillSide, Marker, MarkerSlot, MaskColor, MaskEdit, MaskMessage, MaskNag, MaskQuery, MaskScope,
    MaskTarget,
};
use mychess_core::Position;
use mychess_db::tree::mask::{MaskOp, TreeMask, stm};
pub use mychess_gui::windows::mask::{Hover, MaskUiMessage};

pub fn mask_msg(message: MaskMessage) -> AppMessage {
    AppMessage::Tree(TreeMessage::Mask(message))
}

/// Sends a message of the masks to the application and runs what it starts.
pub fn mask(rig: &mut TreeRig, message: MaskMessage) {
    rig.app_msg(mask_msg(message));
}

/// Sends a message of the visual state of the masks.
pub fn ui(rig: &mut TreeRig, message: MaskUiMessage) {
    rig.msg(Message::MaskUi(message));
}

/// The std base with the tree panel open and a new, empty mask.
pub fn new_mask_rig(dir: &Path) -> TreeRig {
    let mut rig = TreeRig::with_std(dir);
    rig.open_panel();
    mask(&mut rig, MaskMessage::New);
    rig
}

/// Edits the move of the row `san` of the table.
pub fn edit_row(rig: &mut TreeRig, san: &str, edit: MaskEdit) {
    let index = rig.row(san).index;
    mask(
        rig,
        MaskMessage::Edit {
            target: MaskTarget::Row(index),
            edit,
            scope: MaskScope::One,
        },
    );
}

/// Adds a move that is legal in the position of the cursor, by text.
pub fn add_san(rig: &mut TreeRig, san: &str) {
    mask(
        rig,
        MaskMessage::Edit {
            target: MaskTarget::San(san.to_owned()),
            edit: MaskEdit::Add,
            scope: MaskScope::One,
        },
    );
}

/// Opens the context menu of the row `san`.
pub fn open_context(rig: &mut TreeRig, san: &str, command: bool) {
    let row = rig.row(san);
    ui(
        rig,
        MaskUiMessage::OpenContext {
            target: MaskTarget::Row(row.index),
            san: row.san.clone(),
            in_mask: row.mask.as_ref().is_some_and(|m| m.in_mask),
            position: iced::Point::new(300.0, 300.0),
            command,
        },
    );
}

fn after(position: &Position, uci: &str) -> Position {
    let mut next = position.clone();
    let mv = next.parse_uci(uci).expect("legal uci");
    next.play(mv).expect("playable");
    next
}

/// Writes `<dir>/<name>.stm` with the moves `(line, san)`: `san` is added to
/// the position reached by the UCI moves of `line` from the start.
pub fn stm_file(dir: &Path, name: &str, lines: &[(&str, &str)]) -> PathBuf {
    let mut mask = TreeMask::new();
    for (line, san) in lines {
        let mut position = Position::new();
        for uci in line.split_whitespace() {
            position = after(&position, uci);
        }
        mask.apply(
            &position,
            &MaskOp::AddMove {
                san: (*san).to_owned(),
            },
        );
    }
    let path = dir.join(format!("{name}.stm"));
    stm::write(&path, &mask).expect("write mask");
    path
}

/// The mask view of the table.
pub fn mask_view(rig: &TreeRig) -> mychess_app::MaskView {
    rig.view().mask.expect("a mask is open")
}

/// The active dialog of the shell.
pub fn active(rig: &TreeRig) -> Option<mychess_gui::windows::dialog_ui::ActiveDialog> {
    mychess_gui::windows::dialog_ui::ActiveDialog::of(&rig.rig.shell.app, &rig.rig.shell.ui.dialogs)
}
