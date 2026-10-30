//! The content of the built-in manual (spec M9-5, slice a, criteria 1, 3,
//! 4 and 5): the files, their names, the subset of Markdown and every
//! internal link. Reads `assets/manual/` by a path relative to the crate.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use mychess_app::manual::{CHAPTER_FILES, LinkTarget, ManualIndex, resolve_link};

fn manual_dir(language: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/manual")
        .join(language)
}

fn chapters() -> Vec<(String, String)> {
    CHAPTER_FILES
        .iter()
        .map(|name| {
            let path = manual_dir("pt-BR").join(name);
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            ((*name).to_owned(), text)
        })
        .collect()
}

fn index() -> ManualIndex {
    let all = chapters();
    let texts: Vec<&str> = all.iter().map(|(_, t)| t.as_str()).collect();
    ManualIndex::build(&texts, false)
}

/// The prose of a chapter: fenced code blocks dropped and inline code
/// removed, line by line.
fn prose(text: &str) -> Vec<(usize, String)> {
    let mut fenced = false;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut clean = String::new();
        let mut in_code = false;
        for c in line.chars() {
            if c == '`' {
                in_code = !in_code;
            } else if !in_code {
                clean.push(c);
            }
        }
        out.push((i + 1, clean));
    }
    out
}

/// Every `](target)` of a line.
fn link_targets(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("](") {
        let after = &rest[at + 2..];
        match after.find(')') {
            Some(end) => {
                found.push(after[..end].to_owned());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    found
}

#[test]
fn the_directory_has_exactly_the_expected_chapter_files() {
    let mut on_disk: BTreeSet<String> = fs::read_dir(manual_dir("pt-BR"))
        .expect("manual directory")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let expected: BTreeSet<String> = CHAPTER_FILES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(on_disk, expected);
    on_disk.clear();
}

#[test]
fn file_names_are_ascii_kebab_case_with_a_two_digit_prefix_in_order() {
    for (i, name) in CHAPTER_FILES.iter().enumerate() {
        let bytes = name.as_bytes();
        assert!(
            bytes.len() > 6
                && bytes[0].is_ascii_digit()
                && bytes[1].is_ascii_digit()
                && bytes[2] == b'-'
                && bytes.ends_with(b".md"),
            "{name}"
        );
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'),
            "{name} is not lowercase ASCII kebab-case"
        );
        assert_eq!(
            name[..2].parse::<usize>().ok(),
            Some(i),
            "{name} is out of order"
        );
    }
}

#[test]
fn chapters_stay_inside_the_subset_without_html_tables_or_images() {
    for (name, text) in chapters() {
        assert!(!text.contains('\r'), "{name} has CR");
        assert!(!text.starts_with('\u{feff}'), "{name} has a BOM");
        for (line_no, line) in prose(&text) {
            let t = line.trim();
            assert!(!t.contains("!["), "{name}:{line_no} has an image");
            assert!(
                !(t.starts_with('|')
                    || t.contains(" | ")
                    || t.starts_with("|-")
                    || t.contains("---|")),
                "{name}:{line_no} looks like a table: {t}"
            );
            let has_tag = t.char_indices().any(|(i, c)| {
                c == '<'
                    && t[i + 1..]
                        .chars()
                        .next()
                        .is_some_and(|n| n.is_ascii_alphabetic() || n == '/' || n == '!')
            });
            assert!(!has_tag, "{name}:{line_no} has raw HTML: {t}");
            assert!(!t.starts_with("[^"), "{name}:{line_no} has a footnote");
        }
    }
}

#[test]
fn chapters_are_short_and_never_say_variacao() {
    for (name, text) in chapters() {
        assert!(
            text.lines().count() <= 350,
            "{name} has more than 350 lines"
        );
        assert!(
            !text.to_lowercase().contains("variaç"),
            "{name} says 'variação' (the term is 'variante')"
        );
    }
}

#[test]
fn every_chapter_starts_with_a_title_and_has_the_standard_parts() {
    let all = chapters();
    for (i, (name, text)) in all.iter().enumerate() {
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        assert!(first.starts_with("# "), "{name} does not start with '# '");
        if i > 0 {
            assert!(
                text.contains("**Neste capítulo:**"),
                "{name} lacks 'Neste capítulo'"
            );
            assert!(
                text.contains("## Veja também"),
                "{name} lacks 'Veja também'"
            );
        }
    }
}

#[test]
fn the_index_chapter_links_to_every_other_chapter() {
    let all = chapters();
    let text = &all[0].1;
    for name in &CHAPTER_FILES[1..] {
        assert!(
            text.contains(&format!("]({name})")),
            "00-indice.md does not link to {name}"
        );
    }
}

#[test]
fn every_internal_link_resolves_to_an_existing_file_and_heading() {
    let all = chapters();
    let index = index();
    let mut checked = 0;
    for (current, (name, text)) in all.iter().enumerate() {
        for (line_no, line) in prose(text) {
            for target in link_targets(&line) {
                let at = format!("{name}:{line_no} -> {target}");
                match resolve_link(current, &target) {
                    LinkTarget::Chapter { chapter, anchor } => {
                        checked += 1;
                        if let Some(anchor) = anchor {
                            assert!(index.has_anchor(chapter, &anchor), "{at}: no such heading");
                            // The same anchor must work on GitHub, which keeps
                            // accents: only unaccented titles may be linked.
                            let heading = index
                                .headings(chapter)
                                .iter()
                                .find(|h| h.slug == anchor)
                                .expect("checked above");
                            assert!(
                                heading.title.is_ascii(),
                                "{at}: the heading '{}' has accents, so the anchor differs on GitHub",
                                heading.title
                            );
                            let written = target.split_once('#').map_or("", |(_, a)| a);
                            assert_eq!(
                                written, anchor,
                                "{at}: write the anchor in the form of the viewer"
                            );
                        }
                    }
                    LinkTarget::External(url) => {
                        assert!(
                            url.starts_with("https://"),
                            "{at}: external links use https"
                        );
                    }
                    other => panic!("{at}: {other:?}"),
                }
            }
        }
    }
    assert!(
        checked >= 100,
        "expected the 144 internal links, saw {checked}"
    );
}

#[test]
fn headings_of_a_chapter_have_distinct_anchors() {
    let index = index();
    for chapter in 0..index.len() {
        let mut seen = BTreeSet::new();
        for h in index.headings(chapter) {
            assert!(
                !h.slug.is_empty(),
                "chapter {chapter}: heading '{}' has an empty anchor",
                h.title
            );
            assert!(
                seen.insert(h.slug.clone()),
                "chapter {chapter}: repeated anchor {}",
                h.slug
            );
        }
        assert!(
            index.title(chapter).is_some_and(|t| !t.is_empty()),
            "chapter {chapter} has no title"
        );
    }
}
