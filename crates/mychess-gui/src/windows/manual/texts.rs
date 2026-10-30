//! The embedded chapters and the index of their headings (spec M9-5, 4.7).

use std::sync::OnceLock;

use mychess_app::manual::{ManualIndex, SharedIndex};

use crate::assets::manual_chapters;
use crate::i18n::Language;

/// The chapters to show for a language of the interface, and whether they
/// are in another language (D4063): the language itself, then `pt-BR`, the
/// only complete one, then `en-US`. `None` only if no language has a manual.
#[must_use]
pub fn chapter_texts(language: Language) -> Option<(&'static [&'static str; 15], bool)> {
    if let Some(texts) = manual_chapters(language) {
        return Some((texts, false));
    }
    [Language::PtBr, Language::EnUs]
        .into_iter()
        .find_map(manual_chapters)
        .map(|texts| (texts, true))
}

/// The index of the headings for a language, built once. The text of the
/// chapters is static, so one index serves the whole session.
#[must_use]
pub fn shared_index(language: Language) -> SharedIndex {
    static EN_US: OnceLock<SharedIndex> = OnceLock::new();
    static PT_BR: OnceLock<SharedIndex> = OnceLock::new();
    let cell = match language {
        Language::EnUs => &EN_US,
        Language::PtBr => &PT_BR,
    };
    cell.get_or_init(|| {
        let (texts, fallback) =
            chapter_texts(language).map_or((&[][..], true), |(t, f)| (&t[..], f));
        SharedIndex::new(ManualIndex::build(texts, fallback))
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_falls_back_to_portuguese_with_the_flag() {
        let (texts, fallback) = chapter_texts(Language::EnUs).expect("a manual exists");
        assert!(fallback);
        assert_eq!(texts.len(), mychess_app::manual::CHAPTER_FILES.len());
        assert!(shared_index(Language::EnUs).get().is_fallback());
    }

    #[test]
    fn portuguese_is_not_a_fallback() {
        let (_, fallback) = chapter_texts(Language::PtBr).expect("a manual exists");
        assert!(!fallback);
        assert!(!shared_index(Language::PtBr).get().is_fallback());
        assert_eq!(shared_index(Language::PtBr), shared_index(Language::PtBr));
    }
}
