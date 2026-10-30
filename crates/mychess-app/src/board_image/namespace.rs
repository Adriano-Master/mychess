//! Prefixing the `id`s of an SVG fragment (spec M7-4, D3726).
//!
//! The drawings of the pieces of a set repeat short ids (`a`, `b`...) in
//! their gradients and masks. When several pieces, or several diagrams, share
//! one document, every id must be unique, so each piece gets its own prefix.

use std::collections::HashSet;

/// Returns `svg` with every `id="x"` turned into `id="<prefix>x"` and the
/// references to it (`url(#x)`, `href="#x"`, `xlink:href="#x"`) rewritten.
/// References to ids that are not defined in `svg` and text without any `id`
/// stay as they are.
pub fn namespace_svg_ids(svg: &str, prefix: &str) -> String {
    let ids = defined_ids(svg);
    if ids.is_empty() {
        return svg.to_owned();
    }
    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(svg.len() + ids.len() * prefix.len() * 2);
    let mut last = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        // `Some((value_start, value_end))` of a name to prefix.
        let found = if is_id_attribute(svg, i) {
            quoted_value(svg, i + 3)
        } else if starts_at(svg, i, "url(") {
            url_target(svg, i + 4, &ids)
        } else if starts_at(svg, i, "href=") {
            quoted_value(svg, i + 5)
                .filter(|(s, e)| svg[*s..*e].starts_with('#') && ids.contains(&svg[*s + 1..*e]))
                .map(|(s, e)| (s + 1, e))
        } else {
            None
        };
        match found {
            Some((start, end)) if start > i => {
                // The id attribute defines, the others refer.
                out.push_str(&svg[last..start]);
                out.push_str(prefix);
                last = start;
                i = end;
            }
            _ => i += 1,
        }
    }
    out.push_str(&svg[last..]);
    out
}

/// Whether `pattern` (ASCII) is at byte `i`; safe at any byte offset.
fn starts_at(svg: &str, i: usize, pattern: &str) -> bool {
    svg.as_bytes()
        .get(i..)
        .is_some_and(|rest| rest.starts_with(pattern.as_bytes()))
}

/// Whether `id=` starts at `at` as an attribute name (after white space).
fn is_id_attribute(svg: &str, at: usize) -> bool {
    starts_at(svg, at, "id=")
        && at > 0
        && svg
            .as_bytes()
            .get(at - 1)
            .is_some_and(u8::is_ascii_whitespace)
}

/// The range of the value of a quoted attribute whose opening quote is at
/// `quote_at`.
fn quoted_value(svg: &str, quote_at: usize) -> Option<(usize, usize)> {
    let quote = *svg.as_bytes().get(quote_at)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let start = quote_at + 1;
    let len = svg[start..].find(char::from(quote))?;
    Some((start, start + len))
}

/// The range of `x` in `url(#x)`, `url('#x')` or `url("#x")` when `x` is a
/// defined id.
fn url_target(svg: &str, after_paren: usize, ids: &HashSet<&str>) -> Option<(usize, usize)> {
    let mut at = after_paren;
    if matches!(svg.as_bytes().get(at), Some(b'"' | b'\'')) {
        at += 1;
    }
    if svg.as_bytes().get(at) != Some(&b'#') {
        return None;
    }
    let start = at + 1;
    let len = svg[start..].find([')', '"', '\''])?;
    let end = start + len;
    ids.contains(&svg[start..end]).then_some((start, end))
}

fn defined_ids(svg: &str) -> HashSet<&str> {
    let mut ids = HashSet::new();
    for (at, _) in svg.match_indices("id=") {
        if is_id_attribute(svg, at)
            && let Some((s, e)) = quoted_value(svg, at + 3)
            && s < e
        {
            ids.insert(&svg[s..e]);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::namespace_svg_ids;

    #[test]
    fn ids_and_their_references_get_the_prefix() {
        let a = r##"<svg><defs><linearGradient id="a"/></defs><path fill="url(#a)"/><use href="#a"/><use xlink:href="#a"/></svg>"##;
        let out = namespace_svg_ids(a, "wK-");
        assert!(out.contains(r#"id="wK-a""#), "{out}");
        assert!(out.contains("url(#wK-a)"), "{out}");
        assert_eq!(out.matches(r##"href="#wK-a""##).count(), 2, "{out}");
        let b = namespace_svg_ids(a, "bQ-");
        assert!(b.contains(r#"id="bQ-a""#) && !b.contains("wK-"));
    }

    #[test]
    fn quoted_urls_and_unknown_references_are_handled() {
        let svg =
            r##"<g id='x'><a fill="url('#x')"/><b fill="url(#other)"/><c href="#other"/></g>"##;
        let out = namespace_svg_ids(svg, "p-");
        assert!(out.contains("id='p-x'"), "{out}");
        assert!(out.contains("url('#p-x')"), "{out}");
        assert!(out.contains("url(#other)") && out.contains(r##"href="#other""##));
    }

    #[test]
    fn text_without_ids_is_returned_unchanged() {
        let svg = r#"<svg><path d="M0 0" data-id="q" fill="red"/></svg>"#;
        assert_eq!(namespace_svg_ids(svg, "p-"), svg);
    }

    #[test]
    fn non_ascii_text_survives() {
        let svg = r##"<svg><title>é</title><g id="é1"/><use href="#é1"/></svg>"##;
        let out = namespace_svg_ids(svg, "p-");
        assert!(
            out.contains("<title>é</title>") && out.contains(r#"id="p-é1""#),
            "{out}"
        );
    }
}
