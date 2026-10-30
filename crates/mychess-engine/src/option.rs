//! Engine options (`option name ... type ...`) and their validation.
//!
//! The model follows the UCI option types (`check`, `spin`, `combo`, `button`,
//! `string`). Validation is strict (spec m4-1, D410): a value outside the
//! announced range is an error, not a warning, and no value may carry a line
//! break, because the value ends up inside a `setoption` command line.

use thiserror::Error;

/// The type of an option, with its default and limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKind {
    /// A boolean option.
    Check {
        /// The default value.
        default: bool,
    },
    /// An integer in `min..=max`.
    Spin {
        /// The default value.
        default: i64,
        /// The smallest accepted value.
        min: i64,
        /// The largest accepted value.
        max: i64,
    },
    /// One of a list of strings.
    Combo {
        /// The default value.
        default: String,
        /// The accepted values (`var` entries).
        vars: Vec<String>,
    },
    /// An action without a value (for example `Clear Hash`).
    Button,
    /// A free text.
    String {
        /// The default value.
        default: String,
    },
}

/// One option an engine announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOption {
    /// The option name, possibly with several words.
    pub name: String,
    /// The type, default and limits.
    pub kind: OptionKind,
}

/// A value for an option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    /// For `check`. Also what [`EngineOption::validate`] returns for a
    /// `button` (the value of a button is ignored).
    Bool(bool),
    /// For `spin`.
    Int(i64),
    /// For `combo` and `string`.
    Text(String),
}

/// Why an option value was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum OptionError {
    /// The engine did not announce such an option.
    #[error("the engine has no option named `{0}`")]
    Unknown(String),
    /// The option is controlled by the interface itself.
    #[error("option `{0}` is managed by the interface")]
    Managed(String),
    /// The value has the wrong type for the option.
    #[error("option `{name}` expects {expected}")]
    WrongType {
        /// The option name.
        name: String,
        /// A short description of the expected value.
        expected: &'static str,
    },
    /// A `spin` value outside its range.
    #[error("option `{name}`: {value} is outside {min}..={max}")]
    OutOfRange {
        /// The option name.
        name: String,
        /// The refused value.
        value: i64,
        /// The smallest accepted value.
        min: i64,
        /// The largest accepted value.
        max: i64,
    },
    /// A `combo` value that is not one of the announced entries.
    #[error("option `{name}`: `{value}` is not one of the accepted values")]
    NotInList {
        /// The option name.
        name: String,
        /// The refused value.
        value: String,
    },
    /// A text with a line break or another control character.
    #[error("option `{name}`: the value must not contain line breaks or control characters")]
    ControlCharacter {
        /// The option name.
        name: String,
    },
}

/// Options the interface controls itself: they are neither sent with the
/// saved values nor shown in the options dialog (spec m4-1, D409).
const MANAGED: [&str; 6] = [
    "UCI_Chess960",
    "UCI_AnalyseMode",
    "UCI_Opponent",
    "UCI_ShowCurrLine",
    "UCI_ShowRefutations",
    "Ponder",
];

/// Whether `name` is an option the interface administers.
pub fn is_managed_option(name: &str) -> bool {
    MANAGED.iter().any(|m| m.eq_ignore_ascii_case(name))
}

impl EngineOption {
    /// Whether the interface administers this option (see
    /// [`is_managed_option`]); such options are left out of the options dialog.
    pub fn is_managed(&self) -> bool {
        is_managed_option(&self.name)
    }

    /// Checks `value` against this option and returns the normalized value.
    ///
    /// - `check`: a [`OptionValue::Bool`], or the text `true`/`false`;
    /// - `spin`: an [`OptionValue::Int`] in `min..=max`, or numeric text;
    /// - `combo`: one of the announced entries;
    /// - `string`: any text without line breaks or control characters;
    /// - `button`: any value is accepted and ignored; the result is
    ///   `Bool(true)`.
    ///
    /// ```
    /// use mychess_engine::{EngineOption, OptionKind, OptionValue};
    ///
    /// let hash = EngineOption {
    ///     name: "Hash".into(),
    ///     kind: OptionKind::Spin { default: 16, min: 1, max: 1024 },
    /// };
    /// assert_eq!(hash.validate(&OptionValue::Int(64))?, OptionValue::Int(64));
    /// assert!(hash.validate(&OptionValue::Int(0)).is_err());
    /// assert!(hash.validate(&OptionValue::Int(1025)).is_err());
    /// # Ok::<(), mychess_engine::OptionError>(())
    /// ```
    pub fn validate(&self, value: &OptionValue) -> Result<OptionValue, OptionError> {
        let name = &self.name;
        let wrong = |expected| OptionError::WrongType {
            name: name.clone(),
            expected,
        };
        match &self.kind {
            OptionKind::Check { .. } => match value {
                OptionValue::Bool(b) => Ok(OptionValue::Bool(*b)),
                OptionValue::Text(t) if t.eq_ignore_ascii_case("true") => {
                    Ok(OptionValue::Bool(true))
                }
                OptionValue::Text(t) if t.eq_ignore_ascii_case("false") => {
                    Ok(OptionValue::Bool(false))
                }
                _ => Err(wrong("true or false")),
            },
            OptionKind::Spin { min, max, .. } => {
                let n = match value {
                    OptionValue::Int(n) => *n,
                    OptionValue::Text(t) => {
                        t.trim().parse::<i64>().map_err(|_| wrong("an integer"))?
                    }
                    OptionValue::Bool(_) => return Err(wrong("an integer")),
                };
                if n < *min || n > *max {
                    return Err(OptionError::OutOfRange {
                        name: name.clone(),
                        value: n,
                        min: *min,
                        max: *max,
                    });
                }
                Ok(OptionValue::Int(n))
            }
            OptionKind::Combo { vars, .. } => {
                let OptionValue::Text(t) = value else {
                    return Err(wrong("one of the listed values"));
                };
                check_text(name, t)?;
                if vars.iter().any(|v| v == t) {
                    Ok(OptionValue::Text(t.clone()))
                } else {
                    Err(OptionError::NotInList {
                        name: name.clone(),
                        value: t.clone(),
                    })
                }
            }
            OptionKind::String { .. } => {
                let OptionValue::Text(t) = value else {
                    return Err(wrong("a text"));
                };
                check_text(name, t)?;
                Ok(OptionValue::Text(t.clone()))
            }
            OptionKind::Button => Ok(OptionValue::Bool(true)),
        }
    }

    /// The `setoption` command line for `value`, which must already have been
    /// validated. A button has no `value` part and an empty string goes as
    /// `<empty>` (UCI).
    pub(crate) fn setoption_line(&self, value: &OptionValue) -> String {
        if matches!(self.kind, OptionKind::Button) {
            return format!("setoption name {}", self.name);
        }
        let text = match value {
            OptionValue::Bool(b) => b.to_string(),
            OptionValue::Int(n) => n.to_string(),
            OptionValue::Text(t) if t.is_empty() => "<empty>".to_owned(),
            OptionValue::Text(t) => t.clone(),
        };
        format!("setoption name {} value {text}", self.name)
    }
}

fn check_text(name: &str, text: &str) -> Result<(), OptionError> {
    if text.chars().any(char::is_control) {
        Err(OptionError::ControlCharacter {
            name: name.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin() -> EngineOption {
        EngineOption {
            name: "Skill Level".into(),
            kind: OptionKind::Spin {
                default: 20,
                min: 0,
                max: 20,
            },
        }
    }

    #[test]
    fn spin_accepts_exactly_the_range() {
        let o = spin();
        assert!(o.validate(&OptionValue::Int(0)).is_ok());
        assert!(o.validate(&OptionValue::Int(20)).is_ok());
        assert!(matches!(
            o.validate(&OptionValue::Int(-1)),
            Err(OptionError::OutOfRange { .. })
        ));
        assert!(matches!(
            o.validate(&OptionValue::Int(21)),
            Err(OptionError::OutOfRange { .. })
        ));
        assert_eq!(
            o.validate(&OptionValue::Text(" 7 ".into())),
            Ok(OptionValue::Int(7))
        );
    }

    #[test]
    fn text_with_line_break_is_refused() {
        let o = EngineOption {
            name: "SyzygyPath".into(),
            kind: OptionKind::String {
                default: String::new(),
            },
        };
        for bad in ["a\nquit", "a\rb", "a\0b"] {
            assert!(matches!(
                o.validate(&OptionValue::Text(bad.into())),
                Err(OptionError::ControlCharacter { .. })
            ));
        }
    }

    #[test]
    fn empty_string_is_sent_as_empty_marker_and_button_has_no_value() {
        let s = EngineOption {
            name: "Book File".into(),
            kind: OptionKind::String {
                default: String::new(),
            },
        };
        assert_eq!(
            s.setoption_line(&OptionValue::Text(String::new())),
            "setoption name Book File value <empty>"
        );
        let b = EngineOption {
            name: "Clear Hash".into(),
            kind: OptionKind::Button,
        };
        let v = b.validate(&OptionValue::Int(3)).unwrap();
        assert_eq!(b.setoption_line(&v), "setoption name Clear Hash");
    }

    #[test]
    fn combo_must_be_listed_and_check_must_be_boolean() {
        let c = EngineOption {
            name: "Style".into(),
            kind: OptionKind::Combo {
                default: "Normal".into(),
                vars: vec!["Solid".into(), "Normal".into()],
            },
        };
        assert!(c.validate(&OptionValue::Text("Solid".into())).is_ok());
        assert!(matches!(
            c.validate(&OptionValue::Text("Risky".into())),
            Err(OptionError::NotInList { .. })
        ));
        let k = EngineOption {
            name: "UCI_ShowWDL".into(),
            kind: OptionKind::Check { default: false },
        };
        assert_eq!(
            k.validate(&OptionValue::Text("TRUE".into())),
            Ok(OptionValue::Bool(true))
        );
        assert!(k.validate(&OptionValue::Int(1)).is_err());
    }

    #[test]
    fn managed_options_are_recognised_ignoring_case() {
        assert!(is_managed_option("uci_chess960"));
        assert!(is_managed_option("Ponder"));
        assert!(!is_managed_option("UCI_Elo"));
    }
}
