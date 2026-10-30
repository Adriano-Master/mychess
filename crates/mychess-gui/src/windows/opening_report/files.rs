//! The native file dialogs of the opening report window (spec M7-2c, D3616,
//! D3619, D3620): where to save a report, which `reports.dat` to import and
//! the folder of the reports of the favorites. Like the other dialogs of the
//! front-end they are plain data that the shell runs through `rfd`; a test
//! sends the answer message instead of showing them.

use std::path::PathBuf;

use mychess_app::opening_report::ReportFormat;

use crate::i18n::I18n;

/// The "save report" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReportDialog {
    /// The window title.
    pub title: String,
    /// The file name proposed (with the extension of the format).
    pub file_name: String,
    /// The folder proposed, if the last save remembered one.
    pub directory: Option<PathBuf>,
    /// File type filter: translated name and extensions.
    pub filter: (String, Vec<&'static str>),
}

impl SaveReportDialog {
    /// The dialog for `format`, in the language of `i18n`.
    pub fn new(i18n: &I18n, format: ReportFormat, directory: Option<PathBuf>) -> SaveReportDialog {
        let (key, extensions): (&str, Vec<&'static str>) = match format {
            ReportFormat::Text => ("oprep-fmt-text", vec!["txt"]),
            ReportFormat::Html => ("oprep-fmt-html", vec!["html", "htm"]),
            ReportFormat::Latex => ("oprep-fmt-latex", vec!["tex", "ltx"]),
        };
        SaveReportDialog {
            title: i18n.tr("oprep-ui-save-dialog"),
            file_name: format!("opening-report.{}", format.extension()),
            directory,
            filter: (i18n.tr(key), extensions),
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_title(self.title.as_str())
            .set_file_name(self.file_name.as_str())
            .add_filter(self.filter.0.as_str(), &self.filter.1);
        if let Some(directory) = &self.directory {
            dialog = dialog.set_directory(directory);
        }
        dialog
            .save_file()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

/// The dialog that picks the `reports.dat` of the Scid to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatDialog {
    /// The window title.
    pub title: String,
    /// File type filters.
    pub filters: Vec<(String, Vec<&'static str>)>,
}

impl DatDialog {
    /// The dialog, in the language of `i18n`.
    pub fn new(i18n: &I18n) -> DatDialog {
        DatDialog {
            title: i18n.tr("oprep-ui-dat-dialog"),
            filters: vec![
                (i18n.tr("oprep-ui-dat-filter"), vec!["dat"]),
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

/// The dialog that picks the folder of the reports of the favorites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDialog {
    /// The window title.
    pub title: String,
    /// The folder proposed.
    pub directory: Option<PathBuf>,
}

impl FolderDialog {
    /// The dialog, in the language of `i18n`.
    pub fn new(i18n: &I18n, directory: Option<PathBuf>) -> FolderDialog {
        FolderDialog {
            title: i18n.tr("oprep-ui-folder-dialog"),
            directory,
        }
    }

    /// Shows the dialog and waits for the answer (`None`: cancelled).
    pub async fn run(self) -> Option<PathBuf> {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(self.title.as_str());
        if let Some(directory) = &self.directory {
            dialog = dialog.set_directory(directory);
        }
        dialog
            .pick_folder()
            .await
            .map(|handle| handle.path().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn the_save_dialog_proposes_the_extension_of_the_format() {
        let i18n = I18n::new(Language::EnUs);
        for (format, ext) in [
            (ReportFormat::Text, "opening-report.txt"),
            (ReportFormat::Html, "opening-report.html"),
            (ReportFormat::Latex, "opening-report.tex"),
        ] {
            let dialog = SaveReportDialog::new(&i18n, format, None);
            assert_eq!(dialog.file_name, ext);
            assert!(dialog.filter.1.len() >= 2 || format == ReportFormat::Text);
        }
    }
}
