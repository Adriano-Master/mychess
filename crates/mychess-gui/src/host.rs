//! The host effects: what only the front-end can do (file dialogs, the
//! clipboard, leaving) and the tasks that run every other effect (spec
//! M2-2, D100).
//!
//! [`classify`] and the builders here are pure, so tests check what an effect
//! asks for without opening a dialog; only [`PickDialog::run`] talks to the
//! operating system (through `rfd`, asynchronously: it never blocks the
//! interface).

use std::path::PathBuf;

use iced::Task;
use mychess_app::{
    ClipboardError, ClipboardPurpose, Effect, Message as AppMessage, PickKind, PickSpec, RequestId,
};

use crate::i18n::I18n;
use crate::shell::Message;
use crate::windows::engines_dialog::EnginePick;

/// A file dialog, ready to show: the texts are already translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickDialog {
    /// Correlates the answer.
    pub request: RequestId,
    /// What the dialog is for.
    pub kind: PickKind,
    /// The window title.
    pub title: String,
    /// The directory to start in.
    pub directory: Option<PathBuf>,
    /// The file name proposed when saving.
    pub file_name: Option<String>,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

/// Extensions of PGN files, with the compressed variant (`*.pgn.gz`).
const PGN_EXTENSIONS: [&str; 6] = ["pgn", "PGN", "Pgn", "gz", "GZ", "Gz"];
/// Extensions of Scid databases and PGN files.
const DATABASE_EXTENSIONS: [&str; 8] = ["si4", "SI4", "Si4", "pgn", "PGN", "Pgn", "gz", "GZ"];
/// Extensions of opening files.
const ECO_EXTENSIONS: [&str; 2] = ["eco", "ECO"];
/// Extension of the databases (spec M3-8).
const BASE_EXTENSIONS: [&str; 3] = ["si4", "SI4", "Si4"];
/// Extensions of spelling files, with the compressed variant (spec M6-3c).
const SPELLING_EXTENSIONS: [&str; 6] = ["ssp", "SSP", "Ssp", "gz", "GZ", "Gz"];
/// Extensions of EPD files, with the compressed variant (spec M7-5c).
const EPD_EXTENSIONS: [&str; 6] = ["epd", "EPD", "Epd", "gz", "GZ", "Gz"];
/// Extensions of tree mask files (spec M5-4c).
const MASK_EXTENSIONS: [&str; 2] = ["stm", "STM"];

impl PickDialog {
    /// The dialog asked for by `spec`, in the language of `i18n`.
    pub fn new(spec: &PickSpec, i18n: &I18n) -> PickDialog {
        let (title_key, types): (&str, (&str, &[&'static str])) = match spec.kind {
            PickKind::SavePgn => ("file-dialog-save-pgn", ("file-filter-pgn", &PGN_EXTENSIONS)),
            PickKind::AppendPgn => (
                "file-dialog-append-pgn",
                ("file-filter-pgn", &PGN_EXTENSIONS),
            ),
            PickKind::OpenEco => ("file-dialog-open-eco", ("file-filter-eco", &ECO_EXTENSIONS)),
            PickKind::OpenTreeBase => (
                "file-dialog-open-tree-base",
                ("file-filter-databases", &DATABASE_EXTENSIONS),
            ),
            PickKind::OpenMask => (
                "file-dialog-open-mask",
                ("file-filter-mask", &MASK_EXTENSIONS),
            ),
            PickKind::SaveMask => (
                "file-dialog-save-mask",
                ("file-filter-mask", &MASK_EXTENSIONS),
            ),
            // "Open" takes Scid databases (`*.si4`; the `.sg4` and `.sn4` files
            // are found beside it) as well as PGN files.
            PickKind::OpenPgn => (
                "file-dialog-open-base",
                ("file-filter-databases", &DATABASE_EXTENSIONS),
            ),
            PickKind::ImportPgn => (
                "file-dialog-import-pgn",
                ("file-filter-pgn", &PGN_EXTENSIONS),
            ),
            PickKind::NewBase => (
                "file-dialog-new-base",
                ("file-filter-base", &BASE_EXTENSIONS),
            ),
            PickKind::OpenSpelling => (
                "file-dialog-open-spelling",
                ("file-filter-spelling", &SPELLING_EXTENSIONS),
            ),
            PickKind::OpenEpd => ("file-dialog-open-epd", ("file-filter-epd", &EPD_EXTENSIONS)),
            PickKind::NewEpd => ("file-dialog-new-epd", ("file-filter-epd", &EPD_EXTENSIONS)),
            PickKind::SaveEpd => ("file-dialog-save-epd", ("file-filter-epd", &EPD_EXTENSIONS)),
            // A folder: the filters are not shown (spec M8-8, D2573).
            PickKind::TablebaseDir => ("file-dialog-tablebase-dir", ("file-filter-all", &["*"])),
            // Spec M7-4c: the documents of "Export games" and the board image.
            PickKind::ExportDoc { format, append } => (
                if append {
                    "file-dialog-export-append"
                } else {
                    "file-dialog-export-doc"
                },
                (format.message_key(), format.accepted_extensions()),
            ),
            PickKind::ExportImage { format } => (
                "file-dialog-export-image",
                match format {
                    mychess_app::export_docs::ImageFormat::Png => {
                        ("export-image-format-png", &["png", "PNG"][..])
                    }
                    mychess_app::export_docs::ImageFormat::Svg => {
                        ("export-image-format-svg", &["svg", "SVG"][..])
                    }
                },
            ),
            _ => ("file-dialog-open-pgn", ("file-filter-pgn", &PGN_EXTENSIONS)),
        };
        PickDialog {
            request: spec.request,
            kind: spec.kind,
            title: i18n.tr(title_key),
            directory: spec.initial_dir.clone(),
            file_name: spec.file_name.clone(),
            filters: vec![
                (i18n.tr(types.0), types.1.to_vec()),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Whether the dialog chooses a destination (it may be a new file).
    pub fn is_save(&self) -> bool {
        matches!(
            self.kind,
            PickKind::SavePgn
                | PickKind::AppendPgn
                | PickKind::SaveMask
                | PickKind::NewBase
                | PickKind::NewEpd
                | PickKind::SaveEpd
                | PickKind::ExportDoc { .. }
                | PickKind::ExportImage { .. }
        )
    }

    /// Whether the dialog takes several files (spec M3-8, D1971); its answer
    /// is [`AppMessage::FilesPicked`].
    pub fn is_multiple(&self) -> bool {
        self.kind == PickKind::ImportPgn
    }

    /// Shows a dialog that takes several files; empty when cancelled.
    pub async fn run_many(self) -> Vec<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        if let Some(directory) = &self.directory {
            dialog = dialog.set_directory(directory);
        }
        dialog
            .pick_files()
            .await
            .map(|handles| handles.iter().map(|h| h.path().to_path_buf()).collect())
            .unwrap_or_default()
    }

    /// Whether the dialog chooses a folder (spec M8-8, D2573).
    pub fn is_folder(&self) -> bool {
        self.kind == PickKind::TablebaseDir
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        if self.is_folder() {
            if let Some(directory) = &self.directory {
                dialog = dialog.set_directory(directory);
            }
            return dialog
                .pick_folder()
                .await
                .map(|handle| handle.path().to_path_buf());
        }
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        if let Some(directory) = &self.directory {
            dialog = dialog.set_directory(directory);
        }
        if let Some(name) = &self.file_name {
            dialog = dialog.set_file_name(name.as_str());
        }
        let handle = if self.is_save() {
            dialog.save_file().await
        } else {
            dialog.pick_file().await
        };
        let path = handle.map(|handle| handle.path().to_path_buf());
        // A new EPD file or a "Save as" typed without an extension gets
        // `.epd`.
        match path {
            Some(path) if matches!(self.kind, PickKind::NewEpd | PickKind::SaveEpd) => {
                Some(mychess_app::epd_window::with_epd_extension(path))
            }
            other => other,
        }
    }
}

/// A file dialog of the engine management (the command of the editor, an
/// `engines.dat` to import), ready to show. It is not an `Effect`: the
/// answer goes straight to the engine messages, so the application needs no
/// `PickKind` for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineFileDialog {
    /// The window title.
    pub title: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl EngineFileDialog {
    /// The dialog for `pick`, in the language of `i18n`. Programs are
    /// filtered by `.exe` on Windows only; elsewhere any file may be one.
    pub fn new(pick: EnginePick, i18n: &I18n) -> EngineFileDialog {
        let all = (i18n.tr("file-filter-all"), vec!["*"]);
        match pick {
            EnginePick::Command => EngineFileDialog {
                title: i18n.tr("engines-pick-command-title"),
                filters: if cfg!(windows) {
                    vec![(i18n.tr("engines-filter-exe"), vec!["exe"]), all]
                } else {
                    vec![all]
                },
            },
            EnginePick::ScidDat => EngineFileDialog {
                title: i18n.tr("engines-pick-dat-title"),
                filters: vec![(i18n.tr("engines-filter-dat"), vec!["dat"]), all],
            },
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        dialog
            .pick_file()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

/// The dialog that picks the Polyglot book of a game against an engine
/// (spec M8-1b, D1234), ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookFileDialog {
    /// The window title.
    pub title: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl BookFileDialog {
    /// The dialog, in the language of `i18n`.
    pub fn new(i18n: &I18n) -> BookFileDialog {
        BookFileDialog {
            title: i18n.tr("play-book-dialog-title"),
            filters: vec![
                (i18n.tr("play-book-filter"), vec!["bin", "BIN"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        dialog
            .pick_file()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

impl BookFileDialog {
    /// The dialog of the book window's "Open...", with its own title.
    pub fn for_window(i18n: &I18n) -> BookFileDialog {
        BookFileDialog {
            title: i18n.tr("book-dialog-open-title"),
            filters: vec![
                (i18n.tr("book-dialog-filter"), vec!["bin", "BIN"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }
}

/// The "Save as..." dialog of the book window (spec M5-6), ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSaveDialog {
    /// The window title.
    pub title: String,
    /// The file name proposed.
    pub file_name: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl BookSaveDialog {
    /// The dialog, in the language of `i18n`.
    pub fn new(i18n: &I18n) -> BookSaveDialog {
        BookSaveDialog {
            title: i18n.tr("book-dialog-save-title"),
            file_name: "book.bin".to_owned(),
            filters: vec![
                (i18n.tr("book-dialog-filter"), vec!["bin", "BIN"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled). A
    /// name without extension gets `.bin`.
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title(self.title.as_str())
            .set_file_name(self.file_name.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        let mut path = dialog.save_file().await?.path().to_path_buf();
        if path.extension().is_none() {
            path.set_extension("bin");
        }
        Some(path)
    }
}

/// The "Save as..." dialog of "Export branch to PGN" (spec M5-6d), ready to
/// show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookPgnDialog {
    /// The window title.
    pub title: String,
    /// The file name proposed.
    pub file_name: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl BookPgnDialog {
    /// The dialog, in the language of `i18n`.
    pub fn new(i18n: &I18n) -> BookPgnDialog {
        BookPgnDialog {
            title: i18n.tr("book-export-dialog-title"),
            file_name: "book-branch.pgn".to_owned(),
            filters: vec![
                (i18n.tr("book-export-dialog-filter"), vec!["pgn", "PGN"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled). The
    /// application fixes the name and the extension (D2166).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title(self.title.as_str())
            .set_file_name(self.file_name.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        Some(dialog.save_file().await?.path().to_path_buf())
    }
}

/// The "Save as..." dialog of the engine log (spec M4-4, D447), ready to
/// show. Like [`EngineFileDialog`] it is not an `Effect`: the answer goes
/// straight to the analysis messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFileDialog {
    /// The window title.
    pub title: String,
    /// The file name proposed.
    pub file_name: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl LogFileDialog {
    /// The dialog in the language of `i18n`.
    pub fn new(i18n: &I18n) -> LogFileDialog {
        LogFileDialog {
            title: i18n.tr("analysis-log-save-title"),
            file_name: "engine.log".to_owned(),
            filters: vec![
                (i18n.tr("analysis-log-filter"), vec!["log", "txt"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title(self.title.as_str())
            .set_file_name(self.file_name.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        dialog
            .save_file()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

/// The file dialogs of the tournament between engines (spec M8-2b): the
/// suite of openings to open, and the file a table is saved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentFileDialog {
    /// The window title.
    pub title: String,
    /// The file name proposed when saving (`None`: the dialog opens a file).
    pub file_name: Option<String>,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl TournamentFileDialog {
    /// The dialog that picks a suite of openings (EPD or PGN).
    pub fn suite(i18n: &I18n) -> TournamentFileDialog {
        TournamentFileDialog {
            title: i18n.tr("tournament-suite-dialog-title"),
            file_name: None,
            filters: vec![
                (
                    i18n.tr("tournament-suite-filter"),
                    vec!["epd", "EPD", "pgn", "PGN"],
                ),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// The dialog that saves a table in `format`.
    pub fn table(
        i18n: &I18n,
        table: mychess_app::tournament::ExportTable,
        format: mychess_app::tournament::ExportFormat,
    ) -> TournamentFileDialog {
        let stem = match table {
            mychess_app::tournament::ExportTable::Standings => i18n.tr("tournament-file-standings"),
            mychess_app::tournament::ExportTable::Cross => i18n.tr("tournament-file-cross"),
            mychess_app::tournament::ExportTable::Ratings => i18n.tr("tournament-file-ratings"),
            mychess_app::tournament::ExportTable::Time => i18n.tr("tournament-file-time"),
        };
        let extension = format.extension();
        TournamentFileDialog {
            title: i18n.tr("tournament-save-dialog-title"),
            file_name: Some(format!("{stem}.{extension}")),
            filters: vec![(extension.to_uppercase(), vec![extension])],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        match self.file_name {
            Some(name) => dialog
                .set_file_name(name.as_str())
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf()),
            None => dialog
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf()),
        }
    }
}

/// The file dialog of the import of a material search (`.sso`) of the Scid
/// (spec M6-1c, D3382). The application has no `PickKind` for it, so the
/// window asks for the file itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoFileDialog {
    /// The window title.
    pub title: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl SsoFileDialog {
    /// The dialog, with the texts of `i18n`.
    pub fn new(i18n: &I18n) -> SsoFileDialog {
        SsoFileDialog {
            title: i18n.tr("advsearch-sso-dialog-title"),
            filters: vec![
                (i18n.tr("advsearch-sso-filter"), vec!["sso", "SSO"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        dialog
            .pick_file()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

/// The file dialog of the patterns the user imports and exports (spec
/// M6-2d). The application has no `PickKind` for `.toml` files, so the
/// window asks for the file itself, as it does for the `.sso` import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternFileDialog {
    /// The window title.
    pub title: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
    /// Asks where to write (export) instead of which file to read.
    pub save: bool,
}

impl PatternFileDialog {
    /// The dialog, with the texts of `i18n`.
    pub fn new(i18n: &I18n, save: bool) -> PatternFileDialog {
        PatternFileDialog {
            title: i18n.tr(if save {
                "pattern-export-title"
            } else {
                "pattern-import-title"
            }),
            filters: vec![
                (i18n.tr("pattern-file-filter"), vec!["toml", "TOML"]),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
            save,
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        if self.save {
            dialog
                .set_file_name("pattern.toml")
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        } else {
            dialog
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        }
    }
}

/// What an effect needs.
#[derive(Debug, Clone, PartialEq)]
pub enum HostWork {
    /// Show a file dialog.
    Pick(PickDialog),
    /// Read the clipboard.
    ReadClipboard(ClipboardPurpose),
    /// Write the clipboard.
    WriteClipboard(String),
    /// Close the program.
    Exit,
    /// Anything else: `mychess_app::perform` runs it.
    Perform(Effect),
}

/// Sorts an effect into the work that runs it.
pub fn classify(effect: Effect, i18n: &I18n) -> HostWork {
    match effect {
        Effect::PickFile(spec) => HostWork::Pick(PickDialog::new(&spec, i18n)),
        Effect::ReadClipboard(purpose) => HostWork::ReadClipboard(purpose),
        Effect::WriteClipboard(text) => HostWork::WriteClipboard(text),
        Effect::Exit => HostWork::Exit,
        other => HostWork::Perform(other),
    }
}

/// The answer of a file dialog.
pub fn picked_message(request: RequestId, path: Option<PathBuf>) -> AppMessage {
    AppMessage::FilePicked(request, path)
}

/// The answer of a dialog that takes several files.
pub fn picked_many_message(request: RequestId, paths: Vec<PathBuf>) -> AppMessage {
    AppMessage::FilesPicked(request, paths)
}

/// The answer of a clipboard read: `None` (nothing readable) is
/// `Unavailable`; an empty or oversized text is judged by the application.
pub fn clipboard_message(purpose: ClipboardPurpose, text: Option<String>) -> AppMessage {
    AppMessage::ClipboardRead {
        purpose,
        result: text.ok_or(ClipboardError::Unavailable),
    }
}

/// The task that runs `work`.
pub fn task(work: HostWork) -> Task<Message> {
    match work {
        HostWork::Pick(dialog) if dialog.is_multiple() => {
            let request = dialog.request;
            Task::perform(dialog.run_many(), move |paths| {
                Message::App(picked_many_message(request, paths))
            })
        }
        HostWork::Pick(dialog) => {
            let request = dialog.request;
            Task::perform(dialog.run(), move |path| {
                Message::App(picked_message(request, path))
            })
        }
        HostWork::ReadClipboard(purpose) => {
            iced::clipboard::read().map(move |text| Message::App(clipboard_message(purpose, text)))
        }
        HostWork::WriteClipboard(text) => iced::clipboard::write(text),
        HostWork::Exit => iced::exit(),
        HostWork::Perform(effect) => Task::perform(mychess_app::perform(effect), Message::App),
    }
}

/// The "Save as..." dialog of the export of the crosstable (spec M6-4d,
/// D3432), ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableSaveDialog {
    /// The window title.
    pub title: String,
    /// The file name proposed.
    pub file_name: String,
    /// File type filters: translated name and extensions (`*` is any file).
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl CrosstableSaveDialog {
    /// The dialog for `format`, proposing `file_name`, in the language of
    /// `i18n`.
    pub fn new(
        i18n: &I18n,
        format: mychess_app::crosstable::ExportFormat,
        file_name: String,
    ) -> CrosstableSaveDialog {
        use mychess_app::crosstable::ExportFormat;
        let (filter, extensions): (&str, Vec<&'static str>) = match format {
            ExportFormat::Html => ("crosstab-filter-html", vec!["html", "htm"]),
            ExportFormat::Latex => ("crosstab-filter-latex", vec!["tex"]),
            ExportFormat::Text => ("crosstab-filter-text", vec!["txt"]),
        };
        CrosstableSaveDialog {
            title: i18n.tr("crosstab-export-title"),
            file_name,
            filters: vec![
                (i18n.tr(filter), extensions),
                (i18n.tr("file-filter-all"), vec!["*"]),
            ],
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title(self.title.as_str())
            .set_file_name(self.file_name.as_str());
        for (name, extensions) in &self.filters {
            dialog = dialog.add_filter(name.as_str(), extensions);
        }
        dialog
            .save_file()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use super::*;
    use crate::i18n::Language;

    fn spec(kind: PickKind) -> PickSpec {
        PickSpec {
            request: RequestId(7),
            kind,
            initial_dir: Some(PathBuf::from("games")),
            file_name: Some("game.pgn".to_owned()),
        }
    }

    #[test]
    fn pick_effects_become_translated_dialogs() {
        let en = I18n::new(Language::EnUs);
        let pt = I18n::new(Language::PtBr);
        let HostWork::Pick(dialog) = classify(Effect::PickFile(spec(PickKind::SavePgn)), &en)
        else {
            panic!("a dialog");
        };
        assert_eq!(dialog.request, RequestId(7));
        assert!(dialog.is_save());
        assert_eq!(dialog.title, "Save game as PGN");
        assert_eq!(dialog.directory, Some(PathBuf::from("games")));
        assert_eq!(dialog.file_name.as_deref(), Some("game.pgn"));
        assert_eq!(dialog.filters[0].0, "PGN files");
        assert!(dialog.filters[0].1.contains(&"pgn"));
        assert_eq!(dialog.filters[1].1, ["*"]);
        let HostWork::Pick(dialog) = classify(Effect::PickFile(spec(PickKind::OpenPgn)), &pt)
        else {
            panic!("a dialog");
        };
        assert!(!dialog.is_save());
        assert_eq!(dialog.title, "Abrir banco ou arquivo PGN");
        // The Scid database (`*.si4`) is offered, not only PGN.
        assert_eq!(dialog.filters[0].0, "Bancos e arquivos PGN");
        assert!(dialog.filters[0].1.contains(&"si4"));
        assert!(dialog.filters[0].1.contains(&"pgn"));
    }

    #[test]
    fn every_dialog_kind_has_a_title_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for kind in [
                PickKind::OpenPgn,
                PickKind::SavePgn,
                PickKind::AppendPgn,
                PickKind::OpenEco,
                PickKind::OpenMask,
                PickKind::SaveMask,
                PickKind::ImportPgn,
                PickKind::NewBase,
            ] {
                let dialog = PickDialog::new(&spec(kind), &i18n);
                assert!(i18n.has("file-filter-all"));
                assert!(!dialog.title.starts_with("file-dialog"), "{kind:?}");
                assert!(!dialog.filters[0].0.starts_with("file-filter"), "{kind:?}");
            }
        }
        let eco = PickDialog::new(&spec(PickKind::OpenEco), &I18n::new(Language::EnUs));
        assert!(eco.filters[0].1.contains(&"eco"));
    }

    #[test]
    fn mask_dialogs_filter_stm_files_and_only_the_save_one_names_a_new_file() {
        let en = I18n::new(Language::EnUs);
        let open = PickDialog::new(&spec(PickKind::OpenMask), &en);
        assert!(open.filters[0].1.contains(&"stm"));
        assert_eq!(open.title, "Open mask file");
        assert!(!open.is_save());
        let save = PickDialog::new(&spec(PickKind::SaveMask), &en);
        assert!(save.filters[0].1.contains(&"stm"));
        assert!(save.is_save());
    }

    #[test]
    fn engine_file_dialogs_have_titles_and_filters_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for pick in [EnginePick::Command, EnginePick::ScidDat] {
                let dialog = EngineFileDialog::new(pick, &i18n);
                assert!(!dialog.title.starts_with("engines-"), "{pick:?}");
                assert!(dialog.filters.iter().all(|(n, _)| !n.contains("filter")));
                assert_eq!(dialog.filters.last().map(|f| f.1.clone()), Some(vec!["*"]));
            }
        }
        let dat = EngineFileDialog::new(EnginePick::ScidDat, &I18n::new(Language::EnUs));
        assert_eq!(dat.filters[0].1, ["dat"]);
    }

    #[test]
    fn host_effects_are_not_left_to_perform() {
        let i18n = I18n::new(Language::EnUs);
        assert_eq!(classify(Effect::Exit, &i18n), HostWork::Exit);
        assert_eq!(
            classify(Effect::ReadClipboard(ClipboardPurpose::Fen), &i18n),
            HostWork::ReadClipboard(ClipboardPurpose::Fen)
        );
        assert_eq!(
            classify(Effect::WriteClipboard("fen".to_owned()), &i18n),
            HostWork::WriteClipboard("fen".to_owned())
        );
        let tick = Effect::ScheduleAutoplayTick {
            generation: 1,
            delay: std::time::Duration::from_millis(5),
        };
        assert_eq!(
            classify(tick.clone(), &i18n),
            HostWork::Perform(tick),
            "other effects run through perform"
        );
    }

    #[test]
    fn the_timers_of_the_modes_and_the_annotation_run_through_perform() {
        // The shell hands them to `perform`, which sleeps and answers with
        // a message of the application (D859, D895): no timer of the GUI.
        let i18n = I18n::new(Language::EnUs);
        let delay = std::time::Duration::from_millis(5);
        let mode_tick = Effect::ScheduleModeTick {
            run: mychess_app::engines::RunId(1),
            generation: 2,
            delay,
        };
        let annotate_tick = Effect::ScheduleAnnotateTick {
            run: mychess_app::engines::RunId(1),
            generation: 3,
            delay,
        };
        for effect in [mode_tick, annotate_tick] {
            assert_eq!(classify(effect.clone(), &i18n), HostWork::Perform(effect));
        }
    }

    #[test]
    fn answers_carry_the_request_and_the_path() {
        let path = PathBuf::from("a.pgn");
        assert_eq!(
            picked_message(RequestId(3), Some(path.clone())),
            AppMessage::FilePicked(RequestId(3), Some(path))
        );
        assert_eq!(
            picked_message(RequestId(4), None),
            AppMessage::FilePicked(RequestId(4), None)
        );
    }

    #[test]
    fn an_unreadable_clipboard_is_unavailable_not_empty() {
        assert_eq!(
            clipboard_message(ClipboardPurpose::Fen, None),
            AppMessage::ClipboardRead {
                purpose: ClipboardPurpose::Fen,
                result: Err(ClipboardError::Unavailable),
            }
        );
        assert_eq!(
            clipboard_message(ClipboardPurpose::Moves, Some("e4".to_owned())),
            AppMessage::ClipboardRead {
                purpose: ClipboardPurpose::Moves,
                result: Ok("e4".to_owned()),
            }
        );
    }

    /// Runs the host effects of `message` the way the shell does, without
    /// opening anything: dialogs and the clipboard are answered by `answer`.
    fn effects_of(app: &mut mychess_app::AppState, message: AppMessage) -> Vec<HostWork> {
        let i18n = I18n::new(Language::EnUs);
        mychess_app::update(app, message)
            .into_iter()
            .map(|effect| classify(effect, &i18n))
            .collect()
    }

    #[test]
    fn open_asks_for_a_dialog_and_the_answer_starts_the_scan() {
        let mut app = mychess_app::AppState::default();
        let work = effects_of(&mut app, AppMessage::Command(mychess_app::Command::Open));
        let [HostWork::Pick(dialog)] = work.as_slice() else {
            panic!("one dialog expected, got {work:?}");
        };
        assert_eq!(dialog.kind, PickKind::OpenPgn);
        // Cancelled: nothing else happens.
        let cancelled = effects_of(&mut app, picked_message(dialog.request, None));
        assert!(cancelled.is_empty());
        // The request is spent: a late answer is ignored.
        let late = effects_of(
            &mut app,
            picked_message(dialog.request, Some(PathBuf::from("late.pgn"))),
        );
        assert!(late.is_empty());
        // A new Open, answered with a path, is scanned by `perform`.
        let work = effects_of(&mut app, AppMessage::Command(mychess_app::Command::Open));
        let [HostWork::Pick(dialog)] = work.as_slice() else {
            panic!("one dialog expected, got {work:?}");
        };
        let path = PathBuf::from("games").join("a.pgn");
        let scan = effects_of(&mut app, picked_message(dialog.request, Some(path.clone())));
        assert!(
            matches!(scan.as_slice(), [HostWork::Perform(Effect::ScanPgn { .. })]),
            "{scan:?}"
        );
    }

    #[test]
    fn copy_fen_writes_the_clipboard_and_paste_reads_it() {
        let mut app = mychess_app::AppState::default();
        let work = effects_of(&mut app, AppMessage::Command(mychess_app::Command::CopyFen));
        let [HostWork::WriteClipboard(fen)] = work.as_slice() else {
            panic!("a clipboard write expected, got {work:?}");
        };
        assert_eq!(*fen, app.session.position().fen());
        let work = effects_of(
            &mut app,
            AppMessage::Command(mychess_app::Command::PasteFen),
        );
        assert_eq!(
            work,
            [HostWork::ReadClipboard(ClipboardPurpose::Fen)],
            "paste asks the host for the text"
        );
        // An unreadable clipboard becomes a remark, not a replaced game.
        drop(effects_of(
            &mut app,
            clipboard_message(ClipboardPurpose::Fen, None),
        ));
        assert!(matches!(
            app.notice,
            Some(mychess_app::AppNotice::Clipboard(
                ClipboardError::Unavailable
            ))
        ));
    }
}
