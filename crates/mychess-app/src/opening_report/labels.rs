//! The words of the documents, filled from Fluent (spec M7-2, D3626).
//!
//! The application layer knows no translation machinery: the front-end
//! passes a lookup function and [`report_labels`] fills every word of
//! [`ReportLabels`] from the keys `oprep-label-<field>` (see
//! [`label_keys`]). A key the lookup does not know keeps the English text.

use mychess_db::report::ReportLabels;

/// A lookup of a Fluent message: `lookup(id, arguments)`.
pub type LabelLookup<'a> = dyn Fn(&str, &[(&str, &str)]) -> Option<String> + 'a;

const THEME_KEYS: [&str; 10] = [
    "theme-0", "theme-1", "theme-2", "theme-3", "theme-4", "theme-5", "theme-6", "theme-7",
    "theme-8", "theme-9",
];

/// Every text field of the labels with the suffix of its Fluent key.
#[allow(clippy::too_many_lines)] // one line per field of the labels
fn fields(labels: &mut ReportLabels) -> Vec<(&'static str, &mut String)> {
    let ReportLabels {
        database_name: _,
        generator: _,
        date: _,
        thousands,
        decimal: _,
        title,
        database,
        games,
        report,
        eco,
        generated,
        white,
        black,
        year,
        score,
        stats_hist,
        stats,
        stat_all,
        stat_both,
        stat_since,
        oldest,
        newest,
        popular,
        freq_all,
        freq_10,
        freq_5,
        freq_1,
        every,
        up,
        down,
        same,
        most_frequent,
        ratings_perf,
        avg_perf,
        white_rating,
        black_rating,
        white_perf,
        black_perf,
        high_rating,
        trends,
        results,
        length,
        frequency,
        shortest,
        moves_themes,
        move_orders,
        move_orders_one,
        move_orders_all,
        move_orders_many,
        moves_from,
        themes,
        theme_description,
        theme_names,
        endgames,
        end_class,
        report_games,
        all_games,
        theory_table,
        table_comment,
        tree_move,
        tree_frequency,
        tree_draws,
        tree_elo,
        tree_perf,
        tree_year,
        tree_total,
        none,
        empty,
        skipped,
        partial,
    } = labels;
    let mut out: Vec<(&'static str, &mut String)> = vec![
        ("thousands", thousands),
        ("title", title),
        ("database", database),
        ("games", games),
        ("report", report),
        ("eco", eco),
        ("generated", generated),
        ("white", white),
        ("black", black),
        ("year", year),
        ("score", score),
        ("stats-hist", stats_hist),
        ("stats", stats),
        ("stat-all", stat_all),
        ("stat-both", stat_both),
        ("stat-since", stat_since),
        ("oldest", oldest),
        ("newest", newest),
        ("popular", popular),
        ("freq-all", freq_all),
        ("freq-10", freq_10),
        ("freq-5", freq_5),
        ("freq-1", freq_1),
        ("every", every),
        ("up", up),
        ("down", down),
        ("same", same),
        ("most-frequent", most_frequent),
        ("ratings-perf", ratings_perf),
        ("avg-perf", avg_perf),
        ("white-rating", white_rating),
        ("black-rating", black_rating),
        ("white-perf", white_perf),
        ("black-perf", black_perf),
        ("high-rating", high_rating),
        ("trends", trends),
        ("results", results),
        ("length", length),
        ("frequency", frequency),
        ("shortest", shortest),
        ("moves-themes", moves_themes),
        ("move-orders", move_orders),
        ("move-orders-one", move_orders_one),
        ("move-orders-all", move_orders_all),
        ("move-orders-many", move_orders_many),
        ("moves-from", moves_from),
        ("themes", themes),
        ("theme-description", theme_description),
        ("endgames", endgames),
        ("end-class", end_class),
        ("report-games", report_games),
        ("all-games", all_games),
        ("theory-table", theory_table),
        ("table-comment", table_comment),
        ("tree-move", tree_move),
        ("tree-frequency", tree_frequency),
        ("tree-draws", tree_draws),
        ("tree-elo", tree_elo),
        ("tree-perf", tree_perf),
        ("tree-year", tree_year),
        ("tree-total", tree_total),
        ("none", none),
        ("empty", empty),
        ("skipped", skipped),
        ("partial", partial),
    ];
    for (key, name) in THEME_KEYS.into_iter().zip(theme_names.iter_mut()) {
        out.push((key, name));
    }
    out
}

/// The Fluent keys of the words, `oprep-label-<field>`, plus
/// `oprep-label-decimal`.
pub fn label_keys() -> Vec<String> {
    let mut labels = ReportLabels::default();
    let mut keys: Vec<String> = fields(&mut labels)
        .into_iter()
        .map(|(key, _)| format!("oprep-label-{key}"))
        .collect();
    keys.push("oprep-label-decimal".to_owned());
    keys
}

/// The labels in the language of `lookup(key, args)`. The templates take
/// `$n` and `$max`, which the lookup must substitute with the literal text
/// `{n}` and `{max}` (they are given as the arguments), because the numbers
/// are known only when a document is written.
pub fn report_labels(lookup: &LabelLookup<'_>) -> ReportLabels {
    let args = [("n", "{n}"), ("max", "{max}")];
    let mut labels = ReportLabels::default();
    for (key, slot) in fields(&mut labels) {
        if let Some(text) = lookup(&format!("oprep-label-{key}"), &args) {
            *slot = text;
        }
    }
    if let Some(c) = lookup("oprep-label-decimal", &[]).and_then(|t| t.trim().chars().next()) {
        labels.decimal = c;
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_is_asked_and_missing_keys_keep_the_english() {
        let keys = label_keys();
        assert_eq!(keys.len(), 76);
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
        let none = report_labels(&|_, _| None);
        assert_eq!(none, ReportLabels::default());
        let all = report_labels(&|key, _| Some(format!("<{key}>")));
        assert_eq!(all.title, "<oprep-label-title>");
        assert_eq!(all.theme_names[9], "<oprep-label-theme-9>");
        assert_eq!(all.partial, "<oprep-label-partial>");
    }

    #[test]
    fn templates_get_the_placeholders() {
        let labels = report_labels(&|key, args| {
            (key == "oprep-label-every").then(|| {
                let n = args.iter().find(|(k, _)| *k == "n").map_or("", |(_, v)| v);
                format!("1 in {n}")
            })
        });
        assert_eq!(labels.every, "1 in {n}");
    }
}
