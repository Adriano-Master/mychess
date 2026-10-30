//! Links, anchors and the context of `F1` (spec M9-5, sections 4.3, 4.4
//! and 4.6). Everything here is a pure function.

use super::CHAPTER_FILES;

/// The letter without its accent (the Latin letters of the manual and of
/// the languages it may be translated to); other characters are unchanged.
pub(super) fn unaccent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        other => other,
    }
}

/// The text lowercased and without accents, for the search.
pub(super) fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(unaccent)
        .collect()
}

/// The anchor of a heading (spec 4.4): lowercase, no accents, spaces and
/// `_` become `-`, other punctuation goes away. A title with nothing left
/// gives an empty slug.
///
/// ```
/// # use mychess_app::manual::slug;
/// assert_eq!(slug("Os slots e a Clipbase"), "os-slots-e-a-clipbase");
/// assert_eq!(slug("Informações do jogador"), "informacoes-do-jogador");
/// ```
#[must_use]
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in fold(title.trim()).chars() {
        if c.is_alphanumeric() || c == '-' {
            out.push(c);
        } else if c.is_whitespace() || c == '_' {
            out.push('-');
        }
    }
    // Only separators left (`"?!... #"` gives `"-"`): nothing to anchor to.
    if out.chars().any(char::is_alphanumeric) {
        out
    } else {
        String::new()
    }
}

/// What a clicked link means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A chapter of the manual and, maybe, a heading of it. The heading is
    /// not checked here (only the index knows it).
    Chapter {
        /// The index in [`CHAPTER_FILES`].
        chapter: usize,
        /// The slug after `#`.
        anchor: Option<String>,
    },
    /// An `http://` or `https://` address, for the browser.
    External(String),
    /// A scheme the manual does not follow (`file:`, `javascript:`...).
    Ignored,
    /// Empty, outside the manual or naming a file that is not a chapter.
    NotFound,
}

/// Classifies the address of a link clicked in the chapter `current`.
///
/// `file.md`, `file.md#anchor` and `#anchor` stay inside the manual (the
/// file name is matched without regard to case; a path, `..` or an unknown
/// name is [`LinkTarget::NotFound`]); `http(s)` is external; any other
/// scheme is ignored.
#[must_use]
pub fn resolve_link(current: usize, uri: &str) -> LinkTarget {
    let uri = uri.trim();
    if uri.is_empty() {
        return LinkTarget::NotFound;
    }
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return LinkTarget::External(uri.to_owned());
    }
    let (file, anchor) = match uri.split_once('#') {
        Some((file, anchor)) => (file, Some(anchor)),
        None => (uri, None),
    };
    // A scheme: letters, then `:` before any `/` (`javascript:x`, `file:///`).
    if file.contains(':') || file.starts_with("//") {
        return LinkTarget::Ignored;
    }
    let chapter = if file.is_empty() {
        current
    } else {
        match CHAPTER_FILES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(file))
        {
            Some(chapter) => chapter,
            None => return LinkTarget::NotFound,
        }
    };
    let anchor = anchor.map(slug).filter(|s| !s.is_empty());
    LinkTarget::Chapter { chapter, anchor }
}

/// The place the reader is working in, for `F1` (spec 4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HelpContext {
    /// No window in particular: the board, the notation, the comment.
    Board,
    /// The game list, the bases, import and export.
    Databases,
    /// The advanced search, tactics and patterns.
    Searches,
    /// The tree, the books, the masks, the ECO browser.
    Openings,
    /// Engines, analysis, annotation, the game report, tablebases.
    Engines,
    /// Playing the engine, tournaments, Lichess, FICS, the electronic
    /// board, sounds.
    Play,
    /// The training of tactics, calculation and openings.
    Training,
    /// Finders, player information, spelling, crosstable, statistics, piece
    /// tracker.
    Players,
    /// The opening report, EPD, merge.
    Reports,
    /// The appearance window.
    Appearance,
    /// Nothing recognized: the table of contents.
    Unknown,
}

/// The chapter (and heading) `F1` opens for a context (D4062). The headings
/// are only given for sections without accents; today none is needed.
#[must_use]
pub fn help_target(context: HelpContext) -> (usize, Option<&'static str>) {
    let chapter = match context {
        HelpContext::Unknown => 0,
        HelpContext::Board => 2,
        HelpContext::Databases => 3,
        HelpContext::Searches => 4,
        HelpContext::Openings => 5,
        HelpContext::Engines => 6,
        HelpContext::Play => 7,
        HelpContext::Training => 8,
        HelpContext::Players => 9,
        HelpContext::Reports => 10,
        HelpContext::Appearance => 11,
    };
    (chapter, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_handles_accents_punctuation_and_spaces() {
        assert_eq!(slug("Abrir uma base"), "abrir-uma-base");
        assert_eq!(slug("Informações do jogador"), "informacoes-do-jogador");
        assert_eq!(slug("O que é? (Sim!)"), "o-que-e-sim");
        assert_eq!(slug("snake_case e A  B"), "snake-case-e-a--b");
        assert_eq!(slug("  trim  "), "trim");
    }

    #[test]
    fn slug_of_a_title_with_only_symbols_is_empty() {
        assert_eq!(slug("?!... #"), "");
        assert_eq!(slug(""), "");
    }

    #[test]
    fn resolves_file_file_with_anchor_and_anchor_only() {
        assert_eq!(
            resolve_link(2, "04-buscas.md"),
            LinkTarget::Chapter {
                chapter: 4,
                anchor: None
            }
        );
        assert_eq!(
            resolve_link(2, "04-buscas.md#Busca-Salva"),
            LinkTarget::Chapter {
                chapter: 4,
                anchor: Some("busca-salva".into())
            }
        );
        assert_eq!(
            resolve_link(7, "#abrir-uma-base"),
            LinkTarget::Chapter {
                chapter: 7,
                anchor: Some("abrir-uma-base".into())
            }
        );
    }

    #[test]
    fn file_names_match_without_regard_to_case() {
        assert_eq!(
            resolve_link(0, "04-BUSCAS.MD"),
            LinkTarget::Chapter {
                chapter: 4,
                anchor: None
            }
        );
    }

    #[test]
    fn http_and_https_are_external() {
        assert_eq!(
            resolve_link(0, "https://lichess.org"),
            LinkTarget::External("https://lichess.org".into())
        );
        assert_eq!(
            resolve_link(0, "HTTP://example.org/a#b"),
            LinkTarget::External("HTTP://example.org/a#b".into())
        );
    }

    #[test]
    fn other_schemes_are_ignored() {
        for uri in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "mailto:a@b.c",
            "//host/x",
        ] {
            assert_eq!(resolve_link(0, uri), LinkTarget::Ignored, "{uri}");
        }
    }

    #[test]
    fn empty_unknown_and_outside_paths_are_not_found() {
        for uri in [
            "",
            "  ",
            "99-nada.md",
            "../secret.md",
            "sub/04-buscas.md",
            "..\\x.md",
        ] {
            assert_eq!(resolve_link(0, uri), LinkTarget::NotFound, "{uri:?}");
        }
    }

    #[test]
    fn every_context_leads_to_its_chapter() {
        use HelpContext as C;
        let table = [
            (C::Unknown, 0),
            (C::Board, 2),
            (C::Databases, 3),
            (C::Searches, 4),
            (C::Openings, 5),
            (C::Engines, 6),
            (C::Play, 7),
            (C::Training, 8),
            (C::Players, 9),
            (C::Reports, 10),
            (C::Appearance, 11),
        ];
        for (context, chapter) in table {
            assert_eq!(help_target(context).0, chapter, "{context:?}");
        }
    }
}
