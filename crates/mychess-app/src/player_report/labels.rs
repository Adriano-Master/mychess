//! The words of the player report documents, filled from Fluent (spec M7-3,
//! decision M7-3/D3672 and M7-3/D3664).
//!
//! Like the opening report, the application layer knows no translation
//! machinery: the front-end passes a lookup and [`player_labels`] fills the
//! words shared with the opening report from `oprep-label-*` and the words of
//! the player from `preport-label-<field>` (see [`player_label_keys`]).

use mychess_db::report::PlayerLabels;

use crate::opening_report::{LabelLookup, label_keys, report_labels};

/// The words of the player and the suffix of their Fluent key.
fn fields(labels: &mut PlayerLabels) -> Vec<(&'static str, &mut String)> {
    let PlayerLabels {
        base: _,
        player,
        with_white,
        with_black,
        after,
        openings,
        most_frequent_opponents,
        wins,
        draws,
        losses,
        pov,
        none,
        player_rating,
        opponents_rating,
        player_perf,
        opponents_perf,
    } = labels;
    vec![
        ("player", player),
        ("with-white", with_white),
        ("with-black", with_black),
        ("after", after),
        ("openings", openings),
        ("most-frequent-opponents", most_frequent_opponents),
        ("wins", wins),
        ("draws", draws),
        ("losses", losses),
        ("pov", pov),
        ("none", none),
        ("player-rating", player_rating),
        ("opponents-rating", opponents_rating),
        ("player-perf", player_perf),
        ("opponents-perf", opponents_perf),
    ]
}

/// Every Fluent key the documents need: `preport-label-<field>` and the
/// `oprep-label-*` of the shared words.
pub fn player_label_keys() -> Vec<String> {
    let mut labels = PlayerLabels::default();
    let mut keys: Vec<String> = fields(&mut labels)
        .into_iter()
        .map(|(key, _)| format!("preport-label-{key}"))
        .collect();
    keys.extend(label_keys());
    keys
}

/// The labels in the language of `lookup(key, args)`. The templates take
/// `$n` (the name or the moves), which the lookup must substitute with the
/// literal text `{n}` (given as the argument); a key the lookup does not
/// know keeps the English text.
pub fn player_labels(lookup: &LabelLookup<'_>) -> PlayerLabels {
    let args = [("n", "{n}"), ("max", "{max}")];
    let mut labels = PlayerLabels {
        base: report_labels(lookup),
        ..PlayerLabels::default()
    };
    for (key, slot) in fields(&mut labels) {
        if let Some(text) = lookup(&format!("preport-label-{key}"), &args) {
            *slot = text;
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_is_asked_and_missing_keys_keep_the_english() {
        let keys = player_label_keys();
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
        assert_eq!(
            keys.iter()
                .filter(|k| k.starts_with("preport-label-"))
                .count(),
            15
        );
        assert_eq!(player_labels(&|_, _| None), PlayerLabels::default());
        let all = player_labels(&|key, _| Some(format!("<{key}>")));
        assert_eq!(all.pov, "<preport-label-pov>");
        assert_eq!(all.opponents_perf, "<preport-label-opponents-perf>");
        assert_eq!(all.base.title, "<oprep-label-title>");
    }

    #[test]
    fn templates_get_the_placeholder() {
        let labels = player_labels(&|key, args| {
            (key == "preport-label-after").then(|| {
                let n = args.iter().find(|(k, _)| *k == "n").map_or("", |(_, v)| v);
                format!("then {n}")
            })
        });
        assert_eq!(labels.after, "then {n}");
    }
}
