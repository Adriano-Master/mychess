//! From a list entry to the configuration of `mychess-engine` (D426, D431).

use std::time::Duration;

use mychess_engine::{DEFAULT_LOG_LINES, EngineConfig, OptionValue};

use super::model::{EngineDraft, EngineEntry};

/// Settings that do not belong to one engine (the analysis preferences,
/// D431); the front-end fills them from `preferences.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunDefaults {
    /// Low CPU priority for engines whose entry does not say.
    pub low_priority: bool,
    /// Size of the protocol log; `0` turns it off.
    pub log_lines: usize,
    /// Minimum time between two `info` events of the same line.
    pub info_interval: Duration,
}

impl Default for RunDefaults {
    fn default() -> Self {
        RunDefaults {
            low_priority: cfg!(windows),
            log_lines: DEFAULT_LOG_LINES,
            info_interval: Duration::from_millis(50),
        }
    }
}

/// A saved option text as a typed value: `true`/`false` become a boolean, a
/// number that prints back identically an integer, anything else text. The
/// engine validates it against the option it announced and sends it in the
/// same form either way.
pub(crate) fn option_value(text: &str) -> OptionValue {
    match text {
        "true" => OptionValue::Bool(true),
        "false" => OptionValue::Bool(false),
        _ => match text.parse::<i64>() {
            Ok(n) if n.to_string() == text => OptionValue::Int(n),
            _ => OptionValue::Text(text.to_owned()),
        },
    }
}

impl EngineEntry {
    /// The configuration to start this engine with.
    pub fn to_config(&self, defaults: &RunDefaults) -> EngineConfig {
        self.to_draft().to_config(defaults)
    }
}

impl EngineDraft {
    /// The configuration to start this engine with (to test a draft before
    /// it is saved).
    pub fn to_config(&self, defaults: &RunDefaults) -> EngineConfig {
        let mut config = EngineConfig::new(self.command.clone());
        config.name.clone_from(&self.name);
        config.args.clone_from(&self.args);
        config.working_dir.clone_from(&self.working_dir);
        config.protocol = self.protocol;
        config.options = self
            .options
            .iter()
            .map(|(name, value)| (name.clone(), option_value(value)))
            .collect();
        config.low_priority = self.low_priority.unwrap_or(defaults.low_priority);
        config.log_lines = defaults.log_lines;
        config.info_interval = defaults.info_interval;
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{EngineDraft, EngineList};
    use mychess_engine::Protocol;

    #[test]
    fn entry_becomes_a_config() {
        let mut list = EngineList::new();
        let mut d = EngineDraft::new("Crafty", "/opt/crafty");
        d.args = vec!["-x".into()];
        d.working_dir = Some("/opt".into());
        d.protocol = Protocol::XBoard;
        d.options = vec![
            ("Hash".into(), "128".into()),
            ("OwnBook".into(), "true".into()),
            ("BookFile".into(), "007.bin".into()),
            ("Octal".into(), "007".into()),
        ];
        let id = list.add(d).unwrap();
        let defaults = RunDefaults {
            low_priority: true,
            log_lines: 10,
            info_interval: Duration::from_millis(7),
        };
        let c = list.get(id).unwrap().to_config(&defaults);
        assert_eq!(c.name, "Crafty");
        assert_eq!(c.args, ["-x"]);
        assert_eq!(c.protocol, Protocol::XBoard);
        assert!(c.low_priority);
        assert_eq!(
            (c.log_lines, c.info_interval),
            (10, Duration::from_millis(7))
        );
        assert_eq!(
            c.options,
            [
                ("Hash".to_owned(), OptionValue::Int(128)),
                ("OwnBook".to_owned(), OptionValue::Bool(true)),
                ("BookFile".to_owned(), OptionValue::Text("007.bin".into())),
                ("Octal".to_owned(), OptionValue::Text("007".into())),
            ]
        );
    }

    #[test]
    fn entry_priority_overrides_the_default() {
        let mut list = EngineList::new();
        let mut d = EngineDraft::new("A", "a");
        d.low_priority = Some(false);
        let id = list.add(d).unwrap();
        let on = RunDefaults {
            low_priority: true,
            ..RunDefaults::default()
        };
        assert!(!list.get(id).unwrap().to_config(&on).low_priority);
    }
}
