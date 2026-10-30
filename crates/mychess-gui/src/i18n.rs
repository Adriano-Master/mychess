//! Localization with Fluent: en-US (fallback) and pt-BR.
//!
//! The bundles live in a stack: the active language first, en-US last. A key
//! missing everywhere yields the key itself and one warning per key; it never
//! panics. Spec rule 12.

use std::collections::HashSet;
use std::sync::Mutex;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use unic_langid::LanguageIdentifier;

use crate::assets;

/// A language the interface is translated to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Language {
    /// English (United States), the fallback.
    #[default]
    EnUs,
    /// Portuguese (Brazil).
    PtBr,
}

impl Language {
    /// Every language.
    pub const ALL: [Language; 2] = [Language::EnUs, Language::PtBr];

    /// The BCP 47 tag.
    pub fn tag(self) -> &'static str {
        match self {
            Language::EnUs => "en-US",
            Language::PtBr => "pt-BR",
        }
    }

    fn identifier(self) -> LanguageIdentifier {
        // The tags above are valid constants; the default is only a guard.
        self.tag().parse().unwrap_or_default()
    }
}

/// The language preference (a setting may follow the system).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LanguagePref {
    /// Follow the operating system.
    #[default]
    System,
    /// English.
    EnUs,
    /// Portuguese.
    PtBr,
}

impl LanguagePref {
    /// The language to use, asking the system when needed.
    pub fn resolve(self) -> Language {
        match self {
            LanguagePref::EnUs => Language::EnUs,
            LanguagePref::PtBr => Language::PtBr,
            LanguagePref::System => language_from_locale(sys_locale::get_locale().as_deref()),
        }
    }
}

/// Maps a system locale (`pt_BR.UTF-8`, `pt-PT`, `en-GB`...) to a language:
/// any Portuguese variant is pt-BR, everything else en-US.
pub fn language_from_locale(locale: Option<&str>) -> Language {
    let Some(raw) = locale else {
        return Language::EnUs;
    };
    // Strip the POSIX encoding and modifier (`pt_BR.UTF-8@euro`).
    let cleaned = raw
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    match cleaned.parse::<LanguageIdentifier>() {
        Ok(id) if id.language.as_str() == "pt" => Language::PtBr,
        _ => Language::EnUs,
    }
}

type Bundle = FluentBundle<FluentResource>;

/// Translates keys into text of the active language.
pub struct I18n {
    language: Language,
    stack: Vec<Bundle>,
    warned: Mutex<HashSet<String>>,
}

impl std::fmt::Debug for I18n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("I18n")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl Default for I18n {
    fn default() -> I18n {
        I18n::new(Language::EnUs)
    }
}

fn build_bundle(language: Language) -> Bundle {
    let mut bundle = FluentBundle::new_concurrent(vec![language.identifier()]);
    bundle.set_use_isolating(false);
    for &(name, source) in assets::ftl_sources(language) {
        match FluentResource::try_new(source.to_owned()) {
            Ok(resource) => {
                if let Err(errors) = bundle.add_resource(resource) {
                    tracing::warn!(file = name, ?errors, "duplicate Fluent ids");
                }
            }
            Err((resource, errors)) => {
                // Keep the messages that parsed.
                tracing::warn!(file = name, ?errors, "invalid Fluent file");
                if let Err(duplicates) = bundle.add_resource(resource) {
                    tracing::warn!(file = name, ?duplicates, "duplicate Fluent ids");
                }
            }
        }
    }
    bundle
}

impl I18n {
    /// Loads `language` with en-US as the fallback.
    pub fn new(language: Language) -> I18n {
        let mut stack = vec![build_bundle(language)];
        if language != Language::EnUs {
            stack.push(build_bundle(Language::EnUs));
        }
        I18n {
            language,
            stack,
            warned: Mutex::new(HashSet::new()),
        }
    }

    /// The active language.
    pub fn language(&self) -> Language {
        self.language
    }

    /// `text` (a number written with a point) with the decimal separator of
    /// the active language: a comma in Portuguese.
    pub fn decimal(&self, text: &str) -> String {
        match self.language {
            Language::EnUs => text.to_owned(),
            Language::PtBr => text.replace('.', ","),
        }
    }

    /// The text for `key`, or the key itself when it does not exist.
    pub fn tr(&self, key: &str) -> String {
        self.lookup(key, None)
    }

    /// Like [`tr`](Self::tr) with arguments (`{ $name }` and selectors).
    pub fn tr_args(&self, key: &str, args: &FluentArgs<'_>) -> String {
        self.lookup(key, Some(args))
    }

    fn lookup(&self, key: &str, args: Option<&FluentArgs<'_>>) -> String {
        for bundle in &self.stack {
            let Some(pattern) = bundle.get_message(key).and_then(|m| m.value()) else {
                continue;
            };
            let mut errors = Vec::new();
            let text = bundle.format_pattern(pattern, args, &mut errors);
            if !errors.is_empty() {
                tracing::warn!(key, ?errors, "Fluent formatting errors");
            }
            return text.into_owned();
        }
        self.warn_missing(key);
        key.to_owned()
    }

    fn warn_missing(&self, key: &str) {
        let first_time = self
            .warned
            .lock()
            .is_ok_and(|mut set| set.insert(key.to_owned()));
        if first_time {
            tracing::warn!(key, "missing translation key");
        }
    }

    /// Whether any bundle defines `key`.
    pub fn has(&self, key: &str) -> bool {
        self.stack.iter().any(|b| b.has_message(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_in_both_languages() {
        assert_eq!(I18n::new(Language::EnUs).tr("menu-file"), "File");
        assert_eq!(I18n::new(Language::PtBr).tr("menu-file"), "Arquivo");
    }

    #[test]
    fn missing_key_returns_the_key_without_panicking() {
        let i18n = I18n::new(Language::PtBr);
        assert_eq!(i18n.tr("does-not-exist"), "does-not-exist");
        assert_eq!(i18n.tr("does-not-exist"), "does-not-exist");
    }

    #[test]
    fn arguments_and_selectors_are_formatted() {
        let i18n = I18n::new(Language::EnUs);
        let mut args = FluentArgs::new();
        args.set("name", "game.pgn");
        args.set("modified", "yes");
        assert_eq!(i18n.tr_args("window-title", &args), "*game.pgn - MyChess");
        args.set("modified", "no");
        assert_eq!(i18n.tr_args("window-title", &args), "game.pgn - MyChess");
        let mut ms = FluentArgs::new();
        ms.set("ms", 250);
        assert_eq!(i18n.tr_args("menu-view-animation-ms", &ms), "250 ms");
    }

    #[test]
    fn no_unicode_isolation_marks_are_inserted() {
        let mut args = FluentArgs::new();
        args.set("ms", 5);
        let text = I18n::new(Language::PtBr).tr_args("menu-view-animation-ms", &args);
        assert!(!text.contains('\u{2068}') && !text.contains('\u{2069}'));
    }

    #[test]
    fn locale_detection_maps_portuguese_variants_to_pt_br() {
        for locale in ["pt_BR.UTF-8", "pt-PT", "pt", "pt_BR@euro", "PT-br"] {
            assert_eq!(
                language_from_locale(Some(locale)),
                Language::PtBr,
                "{locale}"
            );
        }
        for locale in ["en-GB", "de_DE.UTF-8", "C", "", "!!!"] {
            assert_eq!(
                language_from_locale(Some(locale)),
                Language::EnUs,
                "{locale}"
            );
        }
        assert_eq!(language_from_locale(None), Language::EnUs);
    }

    #[test]
    fn explicit_preferences_ignore_the_system() {
        assert_eq!(LanguagePref::PtBr.resolve(), Language::PtBr);
        assert_eq!(LanguagePref::EnUs.resolve(), Language::EnUs);
    }
}
