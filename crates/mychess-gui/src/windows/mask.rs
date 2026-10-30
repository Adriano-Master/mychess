//! The interface of the tree masks (spec M5-4c, D1827 to D1833, D1880 to
//! D1899): the "Mask" menu, the marks of the table, the context menu of a
//! move and the dialogs (the question about saving, the fill with the line,
//! the comment editor, the search and the outline).
//!
//! The application owns the mask and every decision about it
//! (`mychess_app::tree::mask`); this module draws what the snapshots say
//! and keeps only the visual state ([`MaskUi`]): the open context menu, the
//! text of an editor, the form of the search and the folded nodes of the
//! outline.

use mychess_app::{
    Marker, MaskColor, MaskMessage, MaskNag, MaskNotice, Message as AppMessage, TreeMessage,
};

use crate::i18n::I18n;
use crate::shell::Message;

pub mod cells;
pub mod context;
pub mod dialogs;
pub mod menu;
pub mod state;

pub use state::{
    CommentDraft, ContextMenu, ContextPage, Hover, MaskUi, MaskUiMessage, OutlineUi, SearchEdit,
    SearchForm, SearchUi,
};

/// A message for the masks of the application.
pub(crate) fn mask_message(message: MaskMessage) -> Message {
    Message::App(app_message(message))
}

/// The application message that carries a mask message.
pub(crate) fn app_message(message: MaskMessage) -> AppMessage {
    AppMessage::Tree(TreeMessage::Mask(message))
}

/// The Fluent key of the name of a marker.
pub(crate) fn marker_key(marker: &Marker) -> &'static str {
    match marker {
        Marker::Include => "tree-mask-marker-include",
        Marker::Exclude => "tree-mask-marker-exclude",
        Marker::MainLine => "tree-mask-marker-main-line",
        Marker::Bookmark => "tree-mask-marker-bookmark",
        Marker::White => "tree-mask-marker-white",
        Marker::Black => "tree-mask-marker-black",
        Marker::NewLine => "tree-mask-marker-new-line",
        Marker::ToBeVerified => "tree-mask-marker-to-verify",
        Marker::ToTrain => "tree-mask-marker-to-train",
        Marker::Dubious => "tree-mask-marker-dubious",
        Marker::ToRemove => "tree-mask-marker-to-remove",
        Marker::Other(_) => "tree-mask-marker-other",
    }
}

/// The Fluent key of the name of a color.
pub(crate) fn color_key(color: &MaskColor) -> &'static str {
    match color {
        MaskColor::White => "tree-mask-color-white",
        MaskColor::Green => "tree-mask-color-green",
        MaskColor::Yellow => "tree-mask-color-yellow",
        MaskColor::Blue => "tree-mask-color-blue",
        MaskColor::Red => "tree-mask-color-red",
        MaskColor::Other(_) => "tree-mask-color-other",
    }
}

/// The Fluent key of the name of a standard NAG; `None` for a text the file
/// brought that the program does not know (shown as it is).
pub(crate) fn nag_key(nag: &MaskNag) -> Option<&'static str> {
    Some(match nag {
        MaskNag::Brilliant => "tree-mask-nag-brilliant",
        MaskNag::Good => "tree-mask-nag-good",
        MaskNag::Interesting => "tree-mask-nag-interesting",
        MaskNag::Dubious => "tree-mask-nag-dubious",
        MaskNag::Poor => "tree-mask-nag-poor",
        MaskNag::Blunder => "tree-mask-nag-blunder",
        MaskNag::Unclear => "tree-mask-nag-unclear",
        MaskNag::Other(_) => return None,
    })
}

/// The name of a NAG for a list: the translated name of a standard one, the
/// raw text of any other.
pub(crate) fn nag_label(i18n: &I18n, nag: &MaskNag) -> String {
    nag_key(nag).map_or_else(|| nag.as_str().to_owned(), |key| i18n.tr(key))
}

/// The name of a marker for a list: the translated name, or the image name
/// the file brought for one the program does not know.
pub(crate) fn marker_label(i18n: &I18n, marker: &Marker) -> String {
    match marker {
        Marker::Other(name) if !name.is_empty() => name.clone(),
        other => i18n.tr(marker_key(other)),
    }
}

/// The name of a color for a list.
pub(crate) fn color_label(i18n: &I18n, color: &MaskColor) -> String {
    match color {
        MaskColor::Other(name) if !name.is_empty() => name.clone(),
        other => i18n.tr(color_key(other)),
    }
}

/// The text of a remark about the masks, for the status bar (D1866): the
/// arguments each variant names are filled in here.
pub fn notice_text(i18n: &I18n, notice: &MaskNotice) -> String {
    use fluent_bundle::FluentArgs;
    let mut args = FluentArgs::new();
    let number = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
    match notice {
        MaskNotice::OpenFailed { path, issue } | MaskNotice::SaveFailed { path, issue } => {
            args.set("path", path.display().to_string());
            args.set("reason", i18n.tr(issue.message_key()));
        }
        MaskNotice::Warnings { count } => {
            args.set("count", number(u64::try_from(*count).unwrap_or(u64::MAX)));
        }
        MaskNotice::Filled {
            moves,
            games,
            skipped,
            ..
        } => {
            args.set("moves", number(*moves));
            args.set("games", number(*games));
            args.set("skipped", number(*skipped));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

#[cfg(test)]
mod tests {
    use mychess_app::MaskIssue;

    use super::*;
    use crate::i18n::Language;

    #[test]
    fn every_marker_color_and_nag_has_a_name_in_both_languages() {
        for language in Language::ALL {
            let i18n = I18n::new(language);
            for marker in Marker::ALL {
                assert!(i18n.has(marker_key(&marker)), "{marker:?}");
            }
            for color in MaskColor::STANDARD {
                assert!(i18n.has(color_key(&color)), "{color:?}");
            }
            for nag in MaskNag::STANDARD {
                let key = nag_key(&nag).expect("standard");
                assert!(i18n.has(key), "{nag:?}");
            }
        }
    }

    #[test]
    fn unknown_values_are_shown_as_the_file_has_them() {
        let en = I18n::new(Language::EnUs);
        assert_eq!(nag_label(&en, &MaskNag::Other("+-".to_owned())), "+-");
        assert_eq!(
            marker_label(&en, &Marker::Other("tb_new".to_owned())),
            "tb_new"
        );
        assert_eq!(
            color_label(&en, &MaskColor::Other("#336699".to_owned())),
            "#336699"
        );
        assert_eq!(marker_label(&en, &Marker::Include), "Include");
    }

    #[test]
    fn notices_fill_in_their_arguments() {
        let en = I18n::new(Language::EnUs);
        let text = notice_text(
            &en,
            &MaskNotice::OpenFailed {
                path: "m.stm".into(),
                issue: MaskIssue::NotAMask,
            },
        );
        assert!(
            text.contains("m.stm") && text.contains("not a tree mask"),
            "{text}"
        );
        assert!(!text.contains("{ $"), "{text}");
        let filled = notice_text(
            &en,
            &MaskNotice::Filled {
                moves: 12,
                games: 3,
                skipped: 1,
                cancelled: false,
            },
        );
        assert!(filled.contains("12") && filled.contains('3') && filled.contains('1'));
        let pt = I18n::new(Language::PtBr);
        let warnings = notice_text(&pt, &MaskNotice::Warnings { count: 2 });
        assert!(warnings.contains("2 avisos"), "{warnings}");
    }
}
