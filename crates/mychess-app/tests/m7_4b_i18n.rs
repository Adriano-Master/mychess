//! M7-4b contract: every Fluent key the export names exists in both
//! languages, with the same variables, and the pt-BR text says "variante"
//! (spec `docs/specs/m7-4-exportacao.md`, "Textos de interface").
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4b_common;

use std::collections::{BTreeMap, BTreeSet};

use m7_4b_common::*;
use mychess_app::{CSV_COLUMNS, GamesExportNotice, ImageFailure, csv_column_key};

/// Key -> the variables its text uses, from `assets/i18n/<lang>/export.ftl`.
fn ftl(lang: &str) -> BTreeMap<String, BTreeSet<String>> {
    let text = std::fs::read_to_string(
        repo_root()
            .join("assets/i18n")
            .join(lang)
            .join("export.ftl"),
    )
    .unwrap();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ')
            && let Some((key, _)) = line.split_once(" =")
        {
            current = Some(key.trim().to_owned());
            out.entry(key.trim().to_owned()).or_default();
        }
        if let Some(key) = &current {
            for (i, _) in line.match_indices("{ $") {
                let name: String = line[i + 3..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                out.get_mut(key).unwrap().insert(name);
            }
        }
    }
    out
}

fn named_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = ExportFormat::ALL.iter().map(|f| f.message_key()).collect();
    keys.extend(CSV_COLUMNS.iter().map(|c| csv_column_key(*c)));
    for problem in [
        ExportProblem::NoAppendMarker,
        ExportProblem::NotUtf8,
        ExportProblem::EmptyFormat,
        ExportProblem::InvalidOptions,
        ExportProblem::AssetsMissing,
    ] {
        keys.push(problem.message_key());
    }
    for error in [
        ImageError::SizeOutOfRange(1),
        ImageError::MarginTooLarge(1),
        ImageError::BadPiece { piece: "wK" },
        ImageError::InvalidFont,
        ImageError::Render(String::new()),
        ImageError::Encode(String::new()),
    ] {
        keys.push(error.message_key());
    }
    keys.push(ImageFailure::Protected.message_key());
    keys.push(ImageFailure::Internal.message_key());
    for notice in [
        GamesExportNotice::ViewerExported {
            volumes: 1,
            count: 1,
            name: String::new(),
        },
        GamesExportNotice::EpdExported {
            count: 1,
            repeated: 0,
            name: String::new(),
        },
        GamesExportNotice::ImageSaved {
            name: String::new(),
        },
        GamesExportNotice::OpenFailed,
    ] {
        keys.push(notice.message_key());
    }
    keys
}

#[test]
fn every_key_the_application_names_exists_in_both_languages() {
    for lang in ["en-US", "pt-BR"] {
        let defined = ftl(lang);
        for key in named_keys() {
            assert!(defined.contains_key(key), "{lang}: missing {key}");
        }
    }
}

#[test]
fn the_two_languages_define_the_same_keys_with_the_same_variables() {
    let (en, pt) = (ftl("en-US"), ftl("pt-BR"));
    assert_eq!(en.keys().collect::<Vec<_>>(), pt.keys().collect::<Vec<_>>());
    for (key, vars) in &en {
        assert_eq!(vars, &pt[key], "variables of {key}");
    }
}

#[test]
fn the_notices_pass_the_variables_their_text_uses() {
    let en = ftl("en-US");
    assert_eq!(
        en["export-notice-viewer"],
        ["count", "name", "volumes"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    assert_eq!(
        en["export-notice-epd"],
        ["count", "name", "repeated"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    assert_eq!(
        en["export-notice-image-saved"],
        ["name"].map(String::from).into_iter().collect()
    );
}

#[test]
fn the_portuguese_texts_say_variante_never_variacao() {
    let text = std::fs::read_to_string(repo_root().join("assets/i18n/pt-BR/export.ftl")).unwrap();
    assert!(text.contains("Variantes") || text.contains("variantes"));
    let lower = text.to_lowercase();
    assert!(
        !lower.contains("variação") && !lower.contains("variações"),
        "use `variante`"
    );
}

#[test]
fn no_key_is_used_by_two_different_meanings_across_the_files() {
    // The new file does not redefine a key of the others.
    for lang in ["en-US", "pt-BR"] {
        let new = ftl(lang);
        let dir = repo_root().join("assets/i18n").join(lang);
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap() == "export.ftl" || path.extension().unwrap() != "ftl" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for key in new.keys() {
                assert!(
                    !text.lines().any(|l| l.starts_with(&format!("{key} ="))),
                    "{key} also in {}",
                    path.display()
                );
            }
        }
    }
}
