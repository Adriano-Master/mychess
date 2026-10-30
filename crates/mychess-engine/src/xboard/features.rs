//! `feature` lines (spec m4-2, D416-D417): parsing, the accept/reject answer
//! and the features the adapter keeps.

/// The most `key=value` pairs read from one `feature` line.
const MAX_PAIRS: usize = 64;

/// The longest value kept, in bytes.
const MAX_VALUE_BYTES: usize = 256;

/// Reads the `key=value` pairs of a `feature` line (the leading `feature`
/// word is optional). Values may be quoted (`myname="Crafty 23.4"`); a token
/// without `=` is skipped; an unterminated quote runs to the end of the line.
pub(crate) fn parse_feature_line(line: &str) -> Vec<(String, String)> {
    let mut rest = line.trim();
    if let Some(after) = rest.strip_prefix("feature")
        && after.chars().next().is_none_or(char::is_whitespace)
    {
        rest = after;
    }
    let mut pairs = Vec::new();
    while pairs.len() < MAX_PAIRS {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let key_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let key = &rest[..key_end];
        rest = &rest[key_end..];
        let Some(after_eq) = rest.strip_prefix('=') else {
            continue; // a word without a value
        };
        let (value, tail) = if let Some(quoted) = after_eq.strip_prefix('"') {
            match quoted.find('"') {
                Some(end) => (&quoted[..end], &quoted[end + 1..]),
                None => (quoted, ""),
            }
        } else {
            let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
            (&after_eq[..end], &after_eq[end..])
        };
        rest = tail;
        if !key.is_empty() {
            pairs.push((key.to_owned(), clip(value)));
        }
    }
    pairs
}

fn clip(value: &str) -> String {
    if value.len() <= MAX_VALUE_BYTES {
        return value.to_owned();
    }
    let mut end = MAX_VALUE_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Whether the adapter accepts a feature (spec m4-2, D417). `false` makes it
/// answer `rejected`.
pub(crate) fn accepts(key: &str, value: &str) -> bool {
    match key {
        "myname" | "setboard" | "analyze" | "usermove" | "ping" | "done" | "time" | "draw"
        | "reuse" | "name" | "playother" | "sigint" => true,
        // Moves travel in coordinates; `san=1` would make the engine expect SAN.
        "san" | "colors" => value == "0",
        "variants" => value
            .split(',')
            .any(|v| v.trim().eq_ignore_ascii_case("normal")),
        // `option`, `memory`, `smp`, `egt`, `debug`, `exclude`, `setscore`
        // and anything unknown.
        _ => false,
    }
}

/// The reply line for one feature.
pub(crate) fn answer(key: &str, value: &str) -> String {
    let verb = if accepts(key, value) {
        "accepted"
    } else {
        "rejected"
    };
    format!("{verb} {key}")
}

/// What the engine announced. Booleans the engine never mentioned stay
/// `None`, so the quirk table can fill them in (the engine wins).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Features {
    pub(crate) myname: Option<String>,
    pub(crate) setboard: Option<bool>,
    pub(crate) analyze: Option<bool>,
    pub(crate) usermove: bool,
    pub(crate) ping: bool,
    pub(crate) time: bool,
    pub(crate) sigint: bool,
    /// `colors=0`: the engine does not want `white`/`black` commands.
    pub(crate) no_colors: bool,
    /// `done=0` or `done=1`, as announced.
    pub(crate) done: Option<bool>,
}

fn flag(value: &str) -> bool {
    value.trim() != "0"
}

impl Features {
    /// Records one announced feature (accepted or not: the engine's claim
    /// about its own abilities is kept either way).
    pub(crate) fn apply(&mut self, key: &str, value: &str) {
        match key {
            "myname" => {
                if !value.is_empty() {
                    self.myname = Some(value.to_owned());
                }
            }
            "setboard" => self.setboard = Some(flag(value)),
            "analyze" => self.analyze = Some(flag(value)),
            "usermove" => self.usermove = flag(value),
            "ping" => self.ping = flag(value),
            "time" => self.time = flag(value),
            "sigint" => self.sigint = flag(value),
            "colors" => self.no_colors = !flag(value),
            "done" => self.done = Some(flag(value)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(line: &str) -> Vec<(&'static str, &'static str)> {
        // Leaked only in tests, to compare against literals easily.
        parse_feature_line(line)
            .into_iter()
            .map(|(k, v)| {
                (
                    &*Box::leak(k.into_boxed_str()),
                    &*Box::leak(v.into_boxed_str()),
                )
            })
            .collect()
    }

    #[test]
    fn parses_quoted_and_bare_values() {
        assert_eq!(
            pairs(r#"feature myname="Crafty-23.4 JA" name=1 setboard=1 done=1"#),
            [
                ("myname", "Crafty-23.4 JA"),
                ("name", "1"),
                ("setboard", "1"),
                ("done", "1")
            ]
        );
    }

    #[test]
    fn prefix_is_optional_and_junk_is_skipped() {
        assert_eq!(pairs("analyze=1"), [("analyze", "1")]);
        assert_eq!(pairs("feature lonely ping=1"), [("ping", "1")]);
        assert_eq!(pairs("feature"), []);
        assert_eq!(pairs("features=1"), [("features", "1")]);
        assert_eq!(pairs("feature =3 a=b"), [("a", "b")]);
    }

    #[test]
    fn repeated_keys_are_kept_in_order_and_unterminated_quote_runs_on() {
        assert_eq!(
            pairs(r#"feature a=1 a=2 myname="open end"#),
            [("a", "1"), ("a", "2"), ("myname", "open end")]
        );
        assert_eq!(pairs(r#"feature x="" y=2"#), [("x", ""), ("y", "2")]);
    }

    #[test]
    fn pair_count_and_value_length_are_bounded() {
        let line = format!("feature {}", "a=1 ".repeat(500));
        assert_eq!(parse_feature_line(&line).len(), MAX_PAIRS);
        let long = format!("feature myname=\"{}\"", "é".repeat(1000));
        assert!(parse_feature_line(&long)[0].1.len() <= MAX_VALUE_BYTES);
    }

    #[test]
    fn answers_follow_the_spec_table() {
        for key in [
            "myname",
            "setboard",
            "analyze",
            "usermove",
            "ping",
            "done",
            "time",
            "draw",
            "reuse",
            "name",
            "playother",
            "sigint",
        ] {
            assert_eq!(answer(key, "1"), format!("accepted {key}"));
        }
        assert_eq!(answer("san", "1"), "rejected san");
        assert_eq!(answer("san", "0"), "accepted san");
        assert_eq!(answer("colors", "1"), "rejected colors");
        assert_eq!(answer("colors", "0"), "accepted colors");
        assert_eq!(
            answer("variants", "normal,fischerandom"),
            "accepted variants"
        );
        assert_eq!(answer("variants", "suicide"), "rejected variants");
        for key in [
            "option", "memory", "smp", "egt", "debug", "exclude", "setscore", "zzz",
        ] {
            assert_eq!(answer(key, "1"), format!("rejected {key}"));
        }
    }

    #[test]
    fn features_keep_what_the_engine_said() {
        let mut f = Features::default();
        for (k, v) in parse_feature_line("feature myname=\"X\" setboard=0 analyze=1 ping=1 done=0")
        {
            f.apply(&k, &v);
        }
        assert_eq!(f.myname.as_deref(), Some("X"));
        assert_eq!(f.setboard, Some(false));
        assert_eq!(f.analyze, Some(true));
        assert!(f.ping && !f.usermove);
        assert_eq!(f.done, Some(false));
    }
}
