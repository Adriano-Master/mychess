//! M2-4 localization and symbols: every key used exists in both languages
//! with the same arguments, no unresolved `$var`, plural forms, the pt-BR
//! vocabulary of the Scid ("variante") and the fallback of the NAG symbols
//! the embedded fonts cannot draw (spec D165, D176, D205, section 5).

#![allow(
    clippy::single_char_pattern,
    clippy::drop_non_drop,
    clippy::assert_is_empty,
    clippy::float_cmp,
    clippy::unnecessary_wraps
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use fluent_syntax::ast::{
    Entry, Expression, InlineExpression, Pattern, PatternElement, VariantKey,
};
use fluent_syntax::parser;
use mychess_app::{AppNotice, ClipboardError, FileError, FileErrorKind, Notice, PasteFenError};
use mychess_core::Nag;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::notation::style::{covered, nag_label};
use mychess_gui::windows::notation::MenuLabel;
use mychess_gui::windows::status_bar::{app_notice_text, notice_text};
use unic_langid::LanguageIdentifier;

fn workspace_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn ftl_files(language: &str) -> Vec<PathBuf> {
    let dir = workspace_file(&format!("assets/i18n/{language}"));
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("i18n directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ftl"))
        .collect();
    files.sort();
    files
}

/// What a message needs: its variables, and for each whether it selects on
/// plural categories (a number) or on names (a string).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Needs {
    plain: BTreeSet<String>,
    plural: BTreeSet<String>,
    named: BTreeSet<String>,
}

impl Needs {
    fn all(&self) -> BTreeSet<String> {
        self.plain
            .iter()
            .chain(&self.plural)
            .chain(&self.named)
            .cloned()
            .collect()
    }
}

fn inline_vars(inline: &InlineExpression<&str>, out: &mut Needs) {
    match inline {
        InlineExpression::VariableReference { id } => {
            out.plain.insert(id.name.to_owned());
        }
        InlineExpression::Placeable { expression } => expression_vars(expression, out),
        InlineExpression::FunctionReference { arguments, .. }
        | InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for arg in &arguments.positional {
                inline_vars(arg, out);
            }
            for arg in &arguments.named {
                inline_vars(&arg.value, out);
            }
        }
        _ => {}
    }
}

fn expression_vars(expression: &Expression<&str>, out: &mut Needs) {
    match expression {
        Expression::Inline(inline) => inline_vars(inline, out),
        Expression::Select { selector, variants } => {
            let numeric = variants.iter().any(|v| match &v.key {
                VariantKey::NumberLiteral { .. } => true,
                VariantKey::Identifier { name } => {
                    ["zero", "one", "two", "few", "many", "other"].contains(name)
                }
            });
            if let InlineExpression::VariableReference { id } = selector {
                if numeric {
                    out.plural.insert(id.name.to_owned());
                } else {
                    out.named.insert(id.name.to_owned());
                }
            } else {
                inline_vars(selector, out);
            }
            for variant in variants {
                pattern_vars(&variant.value, out);
            }
        }
    }
}

fn pattern_vars(pattern: &Pattern<&str>, out: &mut Needs) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            expression_vars(expression, out);
        }
    }
}

/// Needs of each message of a language.
fn needs(language: &str) -> BTreeMap<String, Needs> {
    let mut map = BTreeMap::new();
    for file in ftl_files(language) {
        let source = fs::read_to_string(&file).expect("readable");
        let resource = parser::parse(source.as_str()).expect("valid Fluent");
        for entry in &resource.body {
            if let Entry::Message(message) = entry {
                let mut found = Needs::default();
                if let Some(pattern) = &message.value {
                    pattern_vars(pattern, &mut found);
                }
                // A variable that only selects is not "plain": merge so a
                // variable used both ways counts once, as plural or named.
                let selectors: BTreeSet<_> =
                    found.plural.iter().chain(&found.named).cloned().collect();
                found.plain.retain(|v| !selectors.contains(v));
                map.insert(message.id.name.to_owned(), found);
            }
        }
    }
    map
}

#[test]
fn m2_4_both_languages_use_the_same_arguments_in_every_message() {
    let en = needs("en-US");
    let pt = needs("pt-BR");
    let mut differences = Vec::new();
    for (id, en_needs) in &en {
        match pt.get(id) {
            Some(pt_needs) if pt_needs.all() == en_needs.all() => {}
            Some(pt_needs) => differences.push(format!(
                "{id}: en-US {:?} vs pt-BR {:?}",
                en_needs.all(),
                pt_needs.all()
            )),
            None => differences.push(format!("{id}: missing in pt-BR")),
        }
    }
    for id in pt.keys() {
        if !en.contains_key(id) {
            differences.push(format!("{id}: missing in en-US"));
        }
    }
    assert!(differences.is_empty(), "{differences:#?}");
}

#[test]
fn m2_4_plural_selectors_have_the_same_kind_in_both_languages() {
    let en = needs("en-US");
    let pt = needs("pt-BR");
    for (id, a) in &en {
        let b = &pt[id];
        assert_eq!(a.plural, b.plural, "{id}: plural variables");
        assert_eq!(a.named, b.named, "{id}: named variables");
    }
}

fn bundle(language: &str) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = language.parse().expect("language id");
    let mut bundle = FluentBundle::new(vec![id]);
    bundle.set_use_isolating(false);
    for file in ftl_files(language) {
        let source = fs::read_to_string(&file).expect("readable");
        let resource = FluentResource::try_new(source).expect("valid");
        bundle.add_resource(resource).expect("no duplicate ids");
    }
    bundle
}

fn format_all(language: &str, count: i64) -> Vec<(String, String, Vec<String>)> {
    let bundle = bundle(language);
    let mut out = Vec::new();
    for (id, need) in needs(language) {
        let mut args = FluentArgs::new();
        for name in &need.plain {
            args.set(name.as_str(), "VALUE");
        }
        for name in &need.plural {
            args.set(name.as_str(), FluentValue::from(count));
        }
        for name in &need.named {
            args.set(name.as_str(), "white");
        }
        let message = bundle.get_message(&id).expect("message");
        let Some(pattern) = message.value() else {
            continue;
        };
        let mut errors = Vec::new();
        let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
        out.push((
            id,
            text.into_owned(),
            errors.iter().map(|e| format!("{e:?}")).collect(),
        ));
    }
    out
}

#[test]
fn m2_4_every_message_formats_without_errors_or_leftover_placeholders() {
    for language in ["en-US", "pt-BR"] {
        for count in [0, 1, 2, 5, 21, 1000] {
            for (id, text, errors) in format_all(language, count) {
                assert!(errors.is_empty(), "{language} {id} ({count}): {errors:?}");
                assert!(
                    !text.contains('$') || id.starts_with("nag-") || text.contains("$n"),
                    "{language} {id}: {text}"
                );
                assert!(!text.contains("{ "), "{language} {id}: {text}");
                // en-US has no ECO glossary on purpose: it does not translate (M5-7b D1721).
                assert!(
                    id == "eco-glossary" || !text.trim().is_empty(),
                    "{language} {id} is empty"
                );
            }
        }
    }
}

#[test]
fn m2_4_a_message_without_its_arguments_is_visibly_marked_not_silently_empty() {
    // The code passes the arguments listed here; a message that needs one
    // the code never sets would print the variable name.
    let sources = rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    let joined = squeezed(&sources.join("\n"));
    let mut missing = BTreeSet::new();
    for (id, need) in needs("en-US") {
        // Only the messages the code asks for by a literal id.
        if !joined.contains(&format!("\"{id}\"")) {
            continue;
        }
        for name in need.all() {
            if !joined.contains(&format!("set(\"{name}\"")) {
                missing.insert(format!("{id}: ${name}"));
            }
        }
    }
    assert!(missing.is_empty(), "never set by the code: {missing:#?}");
}

#[test]
fn m2_4_each_literal_call_sets_the_arguments_its_message_needs() {
    let needs = needs("en-US");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut problems = Vec::new();
    for file in rust_files(&dir) {
        let full = fs::read_to_string(&file).expect("source");
        let text = squeezed(full.split("#[cfg(test)]").next().unwrap_or_default());
        let text = text.as_str();
        for (at, _) in text.match_indices("tr_args(\"") {
            let rest = &text[at + "tr_args(\"".len()..];
            let Some(end) = rest.find('"') else { continue };
            let key = &rest[..end];
            let Some(need) = needs.get(key) else { continue };
            // The arguments are built in the same function, above the call.
            let start = at.saturating_sub(2500);
            let window = &text[floor(text, start)..at];
            for name in need.all() {
                if !window.contains(&format!("set(\"{name}\"")) {
                    problems.push(format!("{}: {key} lacks ${name}", file.display()));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

fn floor(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("dir").filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
    out
}

fn rust_sources(dir: &Path) -> Vec<String> {
    rust_files(dir)
        .iter()
        .map(|p| fs::read_to_string(p).expect("source"))
        .collect()
}

/// The text without whitespace, so that a call split over lines by the
/// formatter still matches.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

// ---------------------------------------------------------------------------
// Keys reached through the enums of the application
// ---------------------------------------------------------------------------

fn both(key: &str) {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        assert!(i18n.has(key), "{key} missing in {language:?}");
        // `has` also looks in the fallback language: the bundle of the
        // language itself must define it.
    }
    let pt = needs("pt-BR");
    let en = needs("en-US");
    assert!(en.contains_key(key) && pt.contains_key(key), "{key}");
}

#[test]
fn m2_4_every_key_the_application_enums_can_ask_for_is_in_both_languages() {
    let kinds = [
        FileErrorKind::NotFound,
        FileErrorKind::PermissionDenied,
        FileErrorKind::AlreadyExists,
        FileErrorKind::NeedsConfirmation,
        FileErrorKind::TooLarge { limit: 1 << 26 },
        FileErrorKind::UnsupportedEncoding("UTF-16"),
        FileErrorKind::CorruptGzip,
        FileErrorKind::NoGamesFound { skipped_bytes: 0 },
        FileErrorKind::EncodingLossy,
        FileErrorKind::Io(std::io::ErrorKind::Other),
    ];
    for kind in kinds {
        both(kind.message_key());
    }
    for error in [
        ClipboardError::Empty,
        ClipboardError::TooLarge { limit: 10 },
        ClipboardError::Unavailable,
    ] {
        both(error.message_key());
    }
    both(PasteFenError::Empty.message_key());
    for notice in [
        AppNotice::ClipboardWriteFailed,
        AppNotice::HostEffectNotHandled,
        AppNotice::SetupChess960Unsupported,
        AppNotice::SaveInProgress,
    ] {
        both(notice.message_key());
    }
    for label in [
        MenuLabel::GoHere,
        MenuLabel::Promote,
        MenuLabel::MakeFirst,
        MenuLabel::DeleteVariation,
        MenuLabel::TruncateFrom,
        MenuLabel::TruncateBefore,
        MenuLabel::StripComments,
        MenuLabel::StripVariations,
        MenuLabel::EditComment,
        MenuLabel::DeleteComment,
    ] {
        both(label.key());
    }
}

#[test]
fn m2_4_each_remark_of_the_session_has_its_own_text_in_both_languages() {
    let notices = [
        Notice::IllegalMove,
        Notice::NullMoveInCheck,
        Notice::PromptPending,
        Notice::StaleNode,
        Notice::NoMoveHere,
        Notice::NotAVariation,
        Notice::NotInVariation,
        Notice::UndoDisabledInTrial,
        Notice::GameTooLarge,
        Notice::UnreadableMove("zz".to_owned()),
        Notice::NothingToStrip,
    ];
    for language in Language::ALL {
        let i18n = I18n::new(language);
        let fallback = i18n.tr("status-notice-other");
        let mut seen = BTreeMap::new();
        for notice in &notices {
            let text = notice_text(&i18n, notice);
            assert_ne!(
                text, fallback,
                "{notice:?} falls to the generic text in {language:?}"
            );
            if let Some(other) = seen.insert(text.clone(), format!("{notice:?}")) {
                // The undo/redo twins may share the text; nothing else.
                assert_eq!(other, format!("{notice:?}"), "{text}");
            }
        }
    }
}

#[test]
fn m2_4_a_file_error_names_the_file_and_the_reason_without_debug_text() {
    let en = I18n::new(Language::EnUs);
    let error = FileError::new(
        FileErrorKind::NotFound,
        Some(Path::new("some").join("dir").join("games.pgn")),
    );
    let text = app_notice_text(&en, &AppNotice::File(error));
    assert_eq!(text, "File not found (games.pgn)");
    assert!(!text.contains("NotFound") && !text.contains("{"));
}

#[test]
fn m2_4_the_nag_descriptions_exist_for_the_sixteen_palette_symbols() {
    for number in [1, 2, 3, 4, 5, 6, 18, 19, 16, 17, 14, 15, 10, 13, 146, 201] {
        both(&format!("nag-{number}"));
    }
}

// ---------------------------------------------------------------------------
// Plural forms (D190, section 5.5)
// ---------------------------------------------------------------------------

#[test]
fn m2_4_removal_questions_have_singular_and_plural_in_both_languages() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let count = |n: i64| {
        let mut args = FluentArgs::new();
        args.set("count", n);
        args
    };
    for (key, one_en, many_en, one_pt, many_pt) in [
        (
            "prompt-strip-comments",
            "Remove 1 comment from this game?",
            "Remove 3 comments from this game?",
            "Remover 1 comentário desta partida?",
            "Remover 3 comentários desta partida?",
        ),
        (
            "prompt-strip-variations",
            "Remove 1 variation from this game?",
            "Remove 3 variations from this game?",
            "Remover 1 variante desta partida?",
            "Remover 3 variantes desta partida?",
        ),
        (
            "prompt-truncate-after",
            "Remove 1 move after this position?",
            "Remove 3 moves after this position?",
            "Remover 1 lance após esta posição?",
            "Remover 3 lances após esta posição?",
        ),
    ] {
        assert_eq!(en.tr_args(key, &count(1)), one_en);
        assert_eq!(en.tr_args(key, &count(3)), many_en);
        assert_eq!(pt.tr_args(key, &count(1)), one_pt);
        assert_eq!(pt.tr_args(key, &count(3)), many_pt);
    }
}

#[test]
fn m2_4_the_games_count_of_the_overwrite_question_has_a_plural() {
    let en = I18n::new(Language::EnUs);
    let mut args = FluentArgs::new();
    args.set("name", "a.pgn");
    args.set("games", 1);
    assert_eq!(
        en.tr_args("dialog-overwrite-message", &args),
        "a.pgn holds one game. Replace all of them with this game?"
    );
    args.set("games", 7);
    assert_eq!(
        en.tr_args("dialog-overwrite-message", &args),
        "a.pgn holds 7 games. Replace all of them with this game?"
    );
}

// ---------------------------------------------------------------------------
// pt-BR vocabulary (D205)
// ---------------------------------------------------------------------------

fn pt_messages() -> Vec<(String, String)> {
    let bundle = bundle("pt-BR");
    format_all("pt-BR", 3)
        .into_iter()
        .map(|(id, text, _)| {
            drop(bundle.get_message(&id));
            (id, text)
        })
        .collect()
}

#[test]
fn m2_4_portuguese_says_variante_never_variacao() {
    // The Scid's Portuguese uses "variante"; spec D205 asks for the same
    // vocabulary ("variante", "linha principal").
    let offenders: Vec<_> = pt_messages()
        .into_iter()
        .filter(|(_, text)| {
            let lower = text.to_lowercase();
            lower.contains("variaç") || lower.contains("variac")
        })
        .collect();
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn m2_4_portuguese_uses_the_terms_of_the_scid_for_the_main_concepts() {
    let pt = I18n::new(Language::PtBr);
    for (key, needle) in [
        ("nav-exit-variation", "variante"),
        ("nav-enter-variation", "variante"),
        ("nav-add-variation", "variante"),
        ("nav-variation-main", "Linha principal"),
        ("prompt-replace-new-main", "linha principal"),
        ("prompt-replace-add-variation", "variante"),
        ("notation-menu-promote", "variante"),
        ("notation-menu-delete-variation", "variante"),
        ("notation-opt-show-variations", "variantes"),
        ("gameinfo-variations", "Variantes"),
    ] {
        assert!(
            pt.tr(key).to_lowercase().contains(&needle.to_lowercase()),
            "{key}: {:?} should contain {needle:?}",
            pt.tr(key)
        );
    }
}

#[test]
fn m2_4_no_translation_is_left_in_english_by_copy_paste() {
    // Product names are the same in both languages.
    const NAMES: [&str; 1] = ["tablebase-source-online"];
    // Messages that are the same text in both languages are fine only when
    // they are names, symbols or numbers.
    let en: BTreeMap<_, _> = format_all("en-US", 3)
        .into_iter()
        .map(|(id, text, _)| (id, text))
        .collect();
    let mut same = Vec::new();
    for (id, text) in pt_messages() {
        let words = text.replace("VALUE", "");
        if NAMES.contains(&id.as_str()) {
            continue;
        }
        if en.get(&id) == Some(&text) && words.chars().filter(|c| c.is_alphabetic()).count() > 12 {
            same.push(format!("{id}: {text}"));
        }
    }
    assert!(same.is_empty(), "{same:#?}");
}

#[test]
fn m2_4_portuguese_texts_are_accented_correctly() {
    // Common ASCII spellings of accented words that must not appear.
    let bad = [
        "nao ",
        "Nao ",
        " e' ",
        "posicao",
        "Posicao",
        "variacao",
        "comentario",
        "Comentario",
        "partida nao",
        "lances ate",
        "Configuracao",
        "Anotacao",
        "Informacoes",
        "Informacao",
    ];
    for (id, text) in pt_messages() {
        for word in bad {
            assert!(!text.contains(word), "{id}: {text}");
        }
    }
}

// ---------------------------------------------------------------------------
// NAG symbols and the fallback to $n (D165, D176)
// ---------------------------------------------------------------------------

/// The code points that the `cmap` tables (formats 4 and 12) of a font map.
fn cmap_coverage(font: &[u8]) -> BTreeSet<u32> {
    fn be16(d: &[u8], at: usize) -> Option<usize> {
        let b = d.get(at..at + 2)?;
        Some(usize::from(u16::from_be_bytes([b[0], b[1]])))
    }
    fn be32(d: &[u8], at: usize) -> Option<usize> {
        let b = d.get(at..at + 4)?;
        usize::try_from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok()
    }
    let mut out = BTreeSet::new();
    let Some(tables) = be16(font, 4) else {
        return out;
    };
    let cmap = (0..tables)
        .map(|i| 12 + 16 * i)
        .find(|&at| font.get(at..at + 4) == Some(b"cmap"))
        .and_then(|at| be32(font, at + 8));
    let Some(cmap) = cmap else { return out };
    for i in 0..be16(font, cmap + 2).unwrap_or(0) {
        let Some(offset) = be32(font, cmap + 4 + 8 * i + 4) else {
            continue;
        };
        let sub = cmap + offset;
        match be16(font, sub) {
            Some(4) => {
                let n = be16(font, sub + 6).unwrap_or(0) / 2;
                for s in 0..n {
                    let (Some(end), Some(start)) = (
                        be16(font, sub + 14 + 2 * s),
                        be16(font, sub + 16 + 2 * n + 2 * s),
                    ) else {
                        continue;
                    };
                    for c in start..=end {
                        if c != 0xFFFF {
                            out.insert(u32::try_from(c).unwrap_or(0));
                        }
                    }
                }
            }
            Some(12) => {
                for g in 0..be32(font, sub + 12).unwrap_or(0) {
                    let at = sub + 16 + 12 * g;
                    let (Some(start), Some(end)) = (be32(font, at), be32(font, at + 4)) else {
                        continue;
                    };
                    for c in start..=end.min(start + 0x10000) {
                        out.insert(u32::try_from(c).unwrap_or(0));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn embedded_coverage() -> BTreeSet<u32> {
    let mut all = BTreeSet::new();
    for name in [
        "NotoSans-Regular.ttf",
        "NotoSansMath-Regular.ttf",
        "NotoSansSymbols2-Regular.ttf",
    ] {
        let bytes = fs::read(workspace_file(&format!("assets/fonts/{name}"))).expect("font file");
        let coverage = cmap_coverage(&bytes);
        assert!(!coverage.is_empty(), "{name} has a readable cmap");
        all.extend(coverage);
    }
    all
}

#[test]
fn m2_4_a_symbol_is_shown_exactly_when_the_embedded_fonts_can_draw_it() {
    let fonts = embedded_coverage();
    let mut fallbacks = Vec::new();
    for number in 1..=255u16 {
        let nag = Nag::new(number).expect("nag");
        let glyph = nag.glyph();
        let drawable = glyph.chars().all(|c| fonts.contains(&(c as u32)));
        let shown = nag_label(nag, true);
        if glyph.is_empty() {
            assert_eq!(shown, nag.to_string(), "${number} has no symbol: `$n`");
        } else if drawable {
            assert_eq!(shown, glyph, "${number} is drawable: its symbol");
        } else {
            assert_eq!(shown, nag.to_string(), "${number} cannot be drawn: `$n`");
            fallbacks.push(number);
        }
        assert_eq!(
            covered_all(&glyph),
            drawable,
            "the coverage table of the code agrees with the cmap for ${number}"
        );
    }
    assert_eq!(
        fallbacks,
        [142, 192, 193, 212],
        "the four symbols without coverage (D176 / question 2)"
    );
}

fn covered_all(text: &str) -> bool {
    text.chars().all(covered)
}

#[test]
fn m2_4_without_symbolic_mode_every_nag_is_a_dollar_number() {
    for number in 1..=255u16 {
        let nag = Nag::new(number).expect("nag");
        assert_eq!(nag_label(nag, false), format!("${number}"));
    }
}

#[test]
fn m2_4_the_common_symbols_are_plain_ascii_and_the_diagram_is_a_letter() {
    for (number, text) in [
        (1, "!"),
        (2, "?"),
        (3, "!!"),
        (4, "??"),
        (5, "!?"),
        (6, "?!"),
        (10, "="),
        (13, "~"),
        (14, "+="),
        (15, "=+"),
        (16, "+/-"),
        (17, "-/+"),
        (18, "+-"),
        (19, "-+"),
        (146, "N"),
        (201, "D"),
    ] {
        let nag = Nag::new(number).expect("nag");
        assert_eq!(nag_label(nag, true), text, "${number}");
    }
}
