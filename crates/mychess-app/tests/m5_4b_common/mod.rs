//! Helpers of the `m5_4b_*` contract tests (spec M5-4, slice M5-4b). Only the
//! public API of `mychess-app` is used.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

#[path = "../m5_2_common/mod.rs"]
pub mod base;

use std::path::{Path, PathBuf};

pub use base::{Rig, STD};
pub use mychess_app::{
    AppNotice, Effect, MaskEdit, MaskMessage, MaskNotice, MaskScope, MaskTarget, Message,
    TreeMessage,
};
use mychess_core::Position;
use mychess_db::tree::mask::{MaskOp, TreeMask, stm};

/// Sends a mask message and runs the effects; returns the host effects.
pub fn m(rig: &mut Rig, message: MaskMessage) -> Vec<Effect> {
    rig.send(TreeMessage::Mask(message))
}

/// Answers the file dialog among `host` with `path` (`None`: cancelled).
pub fn pick(rig: &mut Rig, host: &[Effect], path: Option<PathBuf>) -> Vec<Effect> {
    let spec = host
        .iter()
        .find_map(|e| match e {
            Effect::PickFile(spec) => Some(spec.clone()),
            _ => None,
        })
        .expect("a file dialog was asked for");
    rig.send(Message::FilePicked(spec.request, path))
}

/// A target by move text.
pub fn san(text: &str) -> MaskTarget {
    MaskTarget::San(text.to_owned())
}

pub fn edit(rig: &mut Rig, text: &str, edit: MaskEdit) -> Vec<Effect> {
    m(
        rig,
        MaskMessage::Edit {
            target: san(text),
            edit,
            scope: MaskScope::One,
        },
    )
}

/// Writes a mask file built by `build`.
pub fn write_mask(path: &Path, build: impl FnOnce(&mut TreeMask)) {
    let mut mask = TreeMask::new();
    build(&mut mask);
    stm::write(path, &mask).expect("write mask");
}

/// A mask file with `san` added at the start position.
pub fn mask_with(path: &Path, san: &str) {
    write_mask(path, |mask| {
        mask.apply(
            &Position::new(),
            &MaskOp::AddMove {
                san: san.to_owned(),
            },
        );
    });
}

pub fn dirty(rig: &Rig) -> bool {
    rig.state.tree_view().mask.expect("a mask is open").dirty
}

pub fn notice(rig: &Rig) -> Option<MaskNotice> {
    match &rig.state.notice {
        Some(AppNotice::Mask(n)) => Some(n.clone()),
        _ => None,
    }
}
