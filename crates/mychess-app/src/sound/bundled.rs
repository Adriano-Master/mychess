//! The sound packs compiled into the binary (spec M8-11b, D2903, D2909).
//!
//! The files come from `assets/sounds/<pack>/` and are embedded with
//! `include_bytes!`, like the fonts: the app has sounds to choose from with
//! no installer. Only the files that a known cue uses are embedded (move,
//! capture, check - which also serves checkmate -, low time and game end);
//! `Defeat`, `Draw` and `GenericNotify` stay out (D2910) to keep the binary
//! small. The budget is [`BUDGET_BYTES`] for all the packs together.

/// What the embedded sound files of all packs may add up to.
pub const BUDGET_BYTES: usize = 1_536 * 1024;

/// A pack in the binary.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BundledPack {
    /// The folder name, the `<folder>` of `bundled:<folder>`.
    pub folder: &'static str,
    /// The `pack.toml` of the folder.
    pub manifest: &'static str,
    /// File name (as the manifest writes it) and content.
    pub files: &'static [(&'static str, &'static [u8])],
}

/// `include_bytes!` of a file of `assets/sounds`, relative to the crate so
/// it does not depend on where this file is.
macro_rules! asset {
    ($pack:literal, $file:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/sounds/",
            $pack,
            "/",
            $file
        ))
    };
}

macro_rules! bundled {
    ($pack:literal) => {
        BundledPack {
            folder: $pack,
            manifest: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/sounds/",
                $pack,
                "/pack.toml"
            )),
            files: &[
                ("Move.ogg", asset!($pack, "Move.ogg")),
                ("Capture.ogg", asset!($pack, "Capture.ogg")),
                ("Check.ogg", asset!($pack, "Check.ogg")),
                ("LowTime.ogg", asset!($pack, "LowTime.ogg")),
                ("Victory.ogg", asset!($pack, "Victory.ogg")),
            ],
        }
    };
}

/// The packs, in the order the window lists them.
pub(crate) static BUNDLED: [BundledPack; 4] = [
    bundled!("sfx"),
    bundled!("nes"),
    bundled!("futuristic"),
    bundled!("piano"),
];

/// The pack of the folder `folder`, if it is embedded.
pub(crate) fn find(folder: &str) -> Option<&'static BundledPack> {
    BUNDLED.iter().find(|p| p.folder == folder)
}

/// The size of every embedded sound file, in bytes.
#[must_use]
pub fn embedded_bytes() -> usize {
    BUNDLED
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|(_, bytes)| bytes.len())
        .sum()
}

/// Every embedded sound file as (pack folder, file name, content), for the
/// tests that compare the binary with the files and credits in `assets/`.
#[must_use]
pub fn embedded_files() -> Vec<(&'static str, &'static str, &'static [u8])> {
    BUNDLED
        .iter()
        .flat_map(|p| {
            p.files
                .iter()
                .map(|(name, bytes)| (p.folder, *name, *bytes))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_files_fit_the_budget() {
        let total = embedded_bytes();
        assert!(total > 0);
        assert!(total <= BUDGET_BYTES, "{total} bytes embedded");
    }

    #[test]
    fn every_pack_is_found_by_its_folder() {
        for pack in &BUNDLED {
            assert!(find(pack.folder).is_some());
            assert!(pack.files.iter().all(|(_, b)| b.starts_with(b"OggS")));
        }
        assert!(find("lisp").is_none());
    }
}
