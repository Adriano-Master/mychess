//! The titles and headings of the chapters, and the search by title (spec
//! M9-5, sections 4.4 and 4.5).

use std::sync::Arc;

use super::link::{fold, slug};

/// The most results a search shows.
const MAX_RESULTS: usize = 60;

/// A heading of a chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// `1` to `6`.
    pub level: u8,
    /// The text, without the markup.
    pub title: String,
    /// The anchor: [`slug`] of the title, with `-1`, `-2`... for repeats.
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ChapterInfo {
    title: String,
    headings: Vec<Heading>,
}

/// The result of a search by title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// The chapter.
    pub chapter: usize,
    /// The slug of the heading; `None` when the chapter title matched.
    pub anchor: Option<String>,
    /// The title that matched.
    pub title: String,
    /// The title of the chapter (to tell equal headings apart).
    pub chapter_title: String,
}

/// The titles and headings of every chapter in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualIndex {
    chapters: Vec<ChapterInfo>,
    fallback: bool,
}

impl ManualIndex {
    /// Reads the headings of the chapters. `fallback` says the texts are
    /// not in the language of the interface (spec 4.7).
    #[must_use]
    pub fn build(texts: &[&str], fallback: bool) -> ManualIndex {
        ManualIndex {
            chapters: texts.iter().map(|text| read_chapter(text)).collect(),
            fallback,
        }
    }

    /// How many chapters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chapters.len()
    }

    /// Whether there is no chapter.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chapters.is_empty()
    }

    /// Whether the texts are not in the language of the interface.
    #[must_use]
    pub fn is_fallback(&self) -> bool {
        self.fallback
    }

    /// The title (the first `#` heading) of a chapter.
    #[must_use]
    pub fn title(&self, chapter: usize) -> Option<&str> {
        self.chapters.get(chapter).map(|c| c.title.as_str())
    }

    /// Every heading of a chapter in document order, the title included.
    #[must_use]
    pub fn headings(&self, chapter: usize) -> &[Heading] {
        self.chapters
            .get(chapter)
            .map_or(&[], |c| c.headings.as_slice())
    }

    /// Whether the chapter has a heading with this slug.
    #[must_use]
    pub fn has_anchor(&self, chapter: usize, anchor: &str) -> bool {
        self.headings(chapter).iter().any(|h| h.slug == anchor)
    }

    /// The chapters and headings (below the title) whose title contains
    /// `query`, ignoring case and accents. An empty query has no results.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let needle = fold(query.trim());
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for (chapter, info) in self.chapters.iter().enumerate() {
            if fold(&info.title).contains(&needle) {
                hits.push(SearchHit {
                    chapter,
                    anchor: None,
                    title: info.title.clone(),
                    chapter_title: info.title.clone(),
                });
            }
            for heading in info.headings.iter().filter(|h| h.level >= 2) {
                if fold(&heading.title).contains(&needle) {
                    hits.push(SearchHit {
                        chapter,
                        anchor: Some(heading.slug.clone()),
                        title: heading.title.clone(),
                        chapter_title: info.title.clone(),
                    });
                }
            }
            if hits.len() >= MAX_RESULTS {
                hits.truncate(MAX_RESULTS);
                break;
            }
        }
        hits
    }
}

/// An index shared between the front-end and the application. Two handles
/// are equal when they are the same index.
#[derive(Debug, Clone)]
pub struct SharedIndex(Arc<ManualIndex>);

impl SharedIndex {
    /// Wraps an index.
    #[must_use]
    pub fn new(index: ManualIndex) -> SharedIndex {
        SharedIndex(Arc::new(index))
    }

    /// The index.
    #[must_use]
    pub fn get(&self) -> &ManualIndex {
        &self.0
    }
}

impl PartialEq for SharedIndex {
    fn eq(&self, other: &SharedIndex) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The text of a heading without the markup the manual uses: code ticks,
/// emphasis and links (`[text](url)` keeps `text`).
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' | '*' | '[' => {}
            ']' => {
                if chars.peek() == Some(&'(') {
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }
    out.trim().to_owned()
}

fn read_chapter(text: &str) -> ChapterInfo {
    let mut headings: Vec<Heading> = Vec::new();
    let mut fence: Option<char> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = ["```", "~~~"].iter().find(|m| trimmed.starts_with(**m)) {
            let c = marker.chars().next().unwrap_or('`');
            match fence {
                None => fence = Some(c),
                Some(open) if open == c => fence = None,
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() || line.len() - trimmed.len() > 3 {
            continue;
        }
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let rest = &trimmed[hashes..];
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            continue;
        }
        let title = plain(rest.trim().trim_end_matches('#'));
        let base = slug(&title);
        let repeats = headings
            .iter()
            .filter(|h| h.slug == base || strip_counter(&h.slug, &base))
            .count();
        let slug = if repeats == 0 {
            base
        } else {
            format!("{base}-{repeats}")
        };
        headings.push(Heading {
            level: u8::try_from(hashes).unwrap_or(6),
            title,
            slug,
        });
    }
    let title = headings
        .iter()
        .find(|h| h.level == 1)
        .map(|h| h.title.clone())
        .unwrap_or_default();
    ChapterInfo { title, headings }
}

/// Whether `candidate` is `base` followed by `-` and a number (a repeat).
fn strip_counter(candidate: &str, base: &str) -> bool {
    candidate
        .strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Bancos de dados\n\nTexto.\n\n## Os slots e a **Clipbase**\n\n```\n# not a heading\n```\n\n## Abrir\n\n### Abrir\n\n## Abrir\n";

    #[test]
    fn reads_title_and_headings_skipping_code_blocks() {
        let index = ManualIndex::build(&[SAMPLE], false);
        assert_eq!(index.title(0), Some("Bancos de dados"));
        let titles: Vec<_> = index.headings(0).iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Bancos de dados",
                "Os slots e a Clipbase",
                "Abrir",
                "Abrir",
                "Abrir"
            ]
        );
    }

    #[test]
    fn repeated_headings_get_numbered_slugs() {
        let index = ManualIndex::build(&[SAMPLE], false);
        let slugs: Vec<_> = index.headings(0).iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(
            slugs,
            [
                "bancos-de-dados",
                "os-slots-e-a-clipbase",
                "abrir",
                "abrir-1",
                "abrir-2"
            ]
        );
        assert!(index.has_anchor(0, "abrir-2"));
        assert!(!index.has_anchor(0, "abrir-3"));
    }

    #[test]
    fn search_ignores_case_and_accents_and_leaves_out_the_title_level_duplicate() {
        let index = ManualIndex::build(&[SAMPLE], false);
        let hits = index.search("CLIPBASE");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].anchor.as_deref(), Some("os-slots-e-a-clipbase"));
        assert_eq!(index.search("bancos")[0].anchor, None);
        assert!(index.search("   ").is_empty());
        assert!(index.search("zzz").is_empty());
    }

    #[test]
    fn links_in_headings_keep_their_text() {
        let index = ManualIndex::build(&["# Veja [isto](a.md) já\n"], true);
        assert_eq!(index.title(0), Some("Veja isto já"));
        assert!(index.is_fallback());
    }

    #[test]
    fn missing_chapter_has_no_headings() {
        let index = ManualIndex::build(&[], false);
        assert!(index.is_empty());
        assert_eq!(index.title(3), None);
        assert!(index.headings(3).is_empty());
    }
}
