//! The texts of the ECO tools (spec M5-7b): the glossary that translates the
//! names of the openings, the remarks of the status bar and the lines of the
//! dialogs. The application only sends variants and numbers; the words are
//! Fluent's (`eco.ftl`).

use fluent_bundle::FluentArgs;
use mychess_app::{AppState, ClassifyResult, EcoNotice, NoveltyOutcome, SlotId};
use mychess_core::{Color, translate_eco_name};

use crate::i18n::I18n;

/// The `(from, to)` pairs of the language of the interface, in the order of
/// the file (`translate_eco_name` applies them from the last to the first,
/// so the generic words come first and the phrases last). Empty for en-US.
pub fn glossary(i18n: &I18n) -> Vec<(String, String)> {
    i18n.tr("eco-glossary")
        .lines()
        .filter_map(|line| {
            let (from, to) = line.split_once("=>")?;
            let (from, to) = (from.trim(), to.trim());
            (!from.is_empty() && !to.is_empty()).then(|| (from.to_owned(), to.to_owned()))
        })
        .collect()
}

/// An opening name in the language of the interface.
pub fn translated_name(i18n: &I18n, name: &str) -> String {
    let pairs = glossary(i18n);
    let pairs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    translate_eco_name(name, &pairs)
}

/// `12.Nf3` or `12...Nf6`.
pub fn move_label(move_number: u32, color: Color, san: &str) -> String {
    match color {
        Color::White => format!("{move_number}.{san}"),
        Color::Black => format!("{move_number}...{san}"),
    }
}

/// The name of a database for the dialogs: its file, the Clipbase, or its
/// slot number.
pub fn slot_label(app: &AppState, i18n: &I18n, id: SlotId) -> String {
    let mut args = FluentArgs::new();
    args.set("number", i64::try_from(id.number()).unwrap_or(0));
    if id.is_clipbase() {
        return i18n.tr("games-clipbase");
    }
    match app.bases.slot(id).and_then(|slot| slot.path.as_deref()) {
        Some(path) => path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        None => i18n.tr_args("games-slot-number", &args),
    }
}

/// The remark of the status bar.
pub fn notice_text(i18n: &I18n, notice: &EcoNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        EcoNotice::Opening { eco, name, ply } => {
            args.set("eco", eco.as_str());
            args.set("name", translated_name(i18n, name));
            args.set("ply", i64::from(*ply));
        }
        EcoNotice::Classified {
            classified,
            changed,
            written,
            ..
        } => {
            args.set("classified", i64::from(*classified));
            args.set("changed", i64::from(*changed));
            args.set("written", i64::from(*written));
        }
        EcoNotice::Novelty {
            move_number,
            color,
            san,
        } => args.set("move", move_label(*move_number, *color, san)),
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

/// The line that tells how a novelty search ended.
pub fn novelty_text(i18n: &I18n, outcome: &NoveltyOutcome) -> String {
    let mut args = FluentArgs::new();
    let key = match outcome {
        NoveltyOutcome::Found {
            move_number,
            color,
            san,
            ..
        } => {
            args.set("move", move_label(*move_number, *color, san));
            "eco-novelty-found"
        }
        NoveltyOutcome::NoNovelty => "eco-novelty-none",
        NoveltyOutcome::NonStandardStart => "eco-novelty-non-standard",
        NoveltyOutcome::Interrupted => "eco-novelty-stopped",
        _ => "eco-novelty-failed",
    };
    i18n.tr_args(key, &args)
}

/// The lines that tell how a classification (or a count) ended.
pub fn classify_result_lines(i18n: &I18n, result: &ClassifyResult) -> Vec<String> {
    let report = &result.report;
    let mut args = FluentArgs::new();
    args.set("classified", i64::from(report.classified));
    args.set("changed", i64::from(report.changed));
    args.set("written", i64::from(report.written));
    let key = match (result.preview, report.cancelled) {
        (true, false) => "eco-classify-preview-done",
        (false, false) => "eco-classify-done",
        (_, true) => "eco-classify-stopped",
    };
    let mut lines = vec![i18n.tr_args(key, &args)];
    if report.unreadable > 0 {
        let mut args = FluentArgs::new();
        args.set("count", i64::from(report.unreadable));
        lines.push(i18n.tr_args("eco-classify-unreadable", &args));
    }
    if let Some(error) = &result.error
        && *error != mychess_app::BaseError::Cancelled
    {
        let mut args = FluentArgs::new();
        args.set("reason", i18n.tr(error.message_key()));
        lines.push(i18n.tr_args("eco-classify-error", &args));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn the_move_label_marks_black_with_three_dots() {
        assert_eq!(move_label(12, Color::White, "Nf3"), "12.Nf3");
        assert_eq!(move_label(12, Color::Black, "Nf6"), "12...Nf6");
    }

    #[test]
    #[allow(clippy::assert_is_empty)] // the glossary is a `Vec` on purpose
    fn english_does_not_translate_and_portuguese_does() {
        let en = I18n::new(Language::EnUs);
        assert!(glossary(&en).is_empty());
        assert_eq!(translated_name(&en, "Sicilian Defence"), "Sicilian Defence");
        let pt = I18n::new(Language::PtBr);
        assert!(!glossary(&pt).is_empty());
        assert_eq!(translated_name(&pt, "Sicilian Defence"), "Defesa Siciliana");
        assert_eq!(
            translated_name(&pt, "Queen's Gambit Declined"),
            "Gambito da Dama Recusado"
        );
        assert_eq!(
            translated_name(&pt, "King's Indian Defence"),
            "Índia do Rei Defesa"
        );
        assert_eq!(translated_name(&pt, "Opening"), "Abertura");
        let variant = translated_name(&pt, "Alekhine Variation");
        assert!(variant.contains("Variante") && !variant.contains("Variação"));
    }
}
