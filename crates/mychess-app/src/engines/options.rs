//! The UCI options dialog as pure state (spec M4-3, D430, D740-D742): the
//! options an engine announced, the text the user typed for each, and the
//! check of the whole form before it is saved.
//!
//! The options come from the identity of a running execution or, when the
//! engine is not running, from a probe (`TestEngine`) started when the
//! dialog opens. Buttons are sent to the running engine only (the probe
//! ends as soon as it answers).

use mychess_engine::{
    EngineIdentity, EngineOption, OptionError, OptionKind, OptionValue, Protocol,
};

use super::model::{EngineEntry, EngineId};
use super::run::EngineFailure;
use crate::error::RequestId;

/// The options the Scid shows first, in its order (`uci.tcl:23`). `Ponder`
/// is administered by the interface and never shown.
pub const IMPORTANT_OPTIONS: [&str; 9] = [
    "MultiPV",
    "Hash",
    "OwnBook",
    "BookFile",
    "UCI_LimitStrength",
    "UCI_Elo",
    "Ponder",
    "Threads",
    "Skill Level",
];

/// The options to save: name and normalized text.
pub(crate) type SavedOptions = Vec<(String, String)>;
/// The options refused by a check.
pub(crate) type OptionRefusals = Vec<(String, OptionError)>;

/// An edit of the options dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptionsMessage {
    /// The text of an option changed (`true`/`false` for a `check`).
    Set {
        /// The option name.
        name: String,
        /// The value, as text.
        value: String,
    },
    /// Puts every option back to its default (in the form; nothing is saved
    /// until [`OptionsMessage::Save`]).
    Reset,
    /// Checks every option and, if all are valid, saves what differs from
    /// the default and applies it to the engine if it runs.
    Save,
    /// Sends a `button` option to the running engine now.
    Press(String),
    /// Closes the dialog without saving.
    Close,
}

/// Where the options come from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptionsStatus {
    /// The engine is being started to ask for its options.
    Probing,
    /// The options are known.
    Ready,
    /// The probe failed.
    Failed(EngineFailure),
    /// XBoard engines have no options to show.
    NotUci,
}

/// The UCI options dialog of one engine of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionsDialog {
    /// The engine of the list.
    pub engine: EngineId,
    /// Where the options are.
    pub status: OptionsStatus,
    /// What the engine announced (`None` until known).
    pub identity: Option<Box<EngineIdentity>>,
    /// The text of every option but the buttons, in the order the engine
    /// announced them.
    pub values: Vec<(String, String)>,
    /// The options whose text is refused (filled by `Save`).
    pub errors: Vec<(String, OptionError)>,
    pub(crate) request: Option<RequestId>,
}

impl OptionsDialog {
    /// A dialog for `entry`; the options arrive with
    /// [`OptionsDialog::set_identity`].
    pub fn new(entry: &EngineEntry) -> OptionsDialog {
        let status = if entry.protocol == Protocol::Uci {
            OptionsStatus::Probing
        } else {
            OptionsStatus::NotUci
        };
        OptionsDialog {
            engine: entry.id,
            status,
            identity: None,
            values: Vec::new(),
            errors: Vec::new(),
            request: None,
        }
    }

    /// Takes the announced options and fills the form with the saved values
    /// (the default where nothing was saved).
    pub fn set_identity(&mut self, identity: EngineIdentity, saved: &[(String, String)]) {
        self.values = identity
            .options
            .iter()
            .filter(|o| !matches!(o.kind, OptionKind::Button))
            .map(|o| {
                let text = saved
                    .iter()
                    .find(|(name, _)| *name == o.name)
                    .map_or_else(|| default_text(o), |(_, value)| value.clone());
                (o.name.clone(), text)
            })
            .collect();
        self.identity = Some(Box::new(identity));
        self.status = OptionsStatus::Ready;
        self.errors.clear();
    }

    /// The options to show, in order: the important ones first (in the
    /// Scid's order), then the rest as announced. Options the interface
    /// administers are left out.
    pub fn visible_options(&self) -> Vec<&EngineOption> {
        self.identity
            .as_deref()
            .map(ordered_options)
            .unwrap_or_default()
    }

    /// The text of `name` in the form.
    pub fn value_of(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The refusal of `name`, if any.
    pub fn error_of(&self, name: &str) -> Option<&OptionError> {
        self.errors.iter().find(|(n, _)| n == name).map(|(_, e)| e)
    }

    pub(crate) fn set(&mut self, name: &str, value: &str) {
        self.errors.retain(|(n, _)| n != name);
        if let Some((_, text)) = self.values.iter_mut().find(|(n, _)| n == name) {
            value.clone_into(text);
        }
    }

    pub(crate) fn reset(&mut self) {
        self.errors.clear();
        let Some(identity) = self.identity.as_deref() else {
            return;
        };
        for (name, text) in &mut self.values {
            if let Some(option) = identity.options.iter().find(|o| o.name == *name) {
                *text = default_text(option);
            }
        }
    }

    /// Checks the whole form. `Ok` lists, in order, the options that
    /// differ from their default, as normalized text (what is saved); the
    /// refusals otherwise.
    pub(crate) fn checked(&self) -> Result<SavedOptions, OptionRefusals> {
        let mut saved = Vec::new();
        let mut errors = Vec::new();
        for option in self.visible_options() {
            if matches!(option.kind, OptionKind::Button) {
                continue;
            }
            let text = self.value_of(&option.name).unwrap_or_default();
            match option.validate(&OptionValue::Text(text.to_owned())) {
                Ok(value) => {
                    let normalized = value_text(&value);
                    if normalized != default_text(option) {
                        saved.push((option.name.clone(), normalized));
                    }
                }
                Err(error) => errors.push((option.name.clone(), error)),
            }
        }
        if errors.is_empty() {
            Ok(saved)
        } else {
            Err(errors)
        }
    }
}

/// The options to show, in order (see [`OptionsDialog::visible_options`]).
pub fn ordered_options(identity: &EngineIdentity) -> Vec<&EngineOption> {
    let rank = |option: &EngineOption| {
        IMPORTANT_OPTIONS
            .iter()
            .position(|name| *name == option.name)
            .unwrap_or(IMPORTANT_OPTIONS.len())
    };
    let mut options: Vec<&EngineOption> = identity
        .options
        .iter()
        .filter(|o| !o.is_managed())
        .collect();
    // Stable: the rest stays in the order the engine announced.
    options.sort_by_key(|o| rank(o));
    options
}

/// The step of the field of a `spin` option (Scid `uci.tcl:788-801`): 1,
/// 10 when the range is wider than 100 and 50 when wider than 1000;
/// `MultiPV` and `Threads` always 1.
pub fn spin_step(name: &str, min: i64, max: i64) -> i64 {
    if name == "MultiPV" || name == "Threads" {
        return 1;
    }
    let range = max.saturating_sub(min).saturating_abs();
    if range > 1000 {
        50
    } else if range > 100 {
        10
    } else {
        1
    }
}

/// The default of an option as text.
pub fn default_text(option: &EngineOption) -> String {
    match &option.kind {
        OptionKind::Check { default } => default.to_string(),
        OptionKind::Spin { default, .. } => default.to_string(),
        OptionKind::Combo { default, .. } | OptionKind::String { default } => default.clone(),
        OptionKind::Button => String::new(),
    }
}

fn value_text(value: &OptionValue) -> String {
    match value {
        OptionValue::Bool(b) => b.to_string(),
        OptionValue::Int(n) => n.to_string(),
        OptionValue::Text(t) => t.clone(),
    }
}

#[cfg(test)]
mod tests {
    use mychess_engine::Capabilities;

    use super::*;
    use crate::engines::{EngineDraft, EngineList};

    fn option(name: &str, kind: OptionKind) -> EngineOption {
        EngineOption {
            name: name.to_owned(),
            kind,
        }
    }

    fn spin(name: &str, default: i64, min: i64, max: i64) -> EngineOption {
        option(name, OptionKind::Spin { default, min, max })
    }

    fn identity() -> EngineIdentity {
        EngineIdentity {
            name: "Fake".to_owned(),
            author: None,
            options: vec![
                option(
                    "Style",
                    OptionKind::Combo {
                        default: "Normal".to_owned(),
                        vars: vec!["Solid".to_owned(), "Normal".to_owned()],
                    },
                ),
                spin("Threads", 1, 1, 512),
                spin("Hash", 16, 1, 33_554_432),
                option("Clear Hash", OptionKind::Button),
                option("Ponder", OptionKind::Check { default: false }),
                option(
                    "SyzygyPath",
                    OptionKind::String {
                        default: String::new(),
                    },
                ),
                spin("MultiPV", 1, 1, 500),
            ],
            capabilities: Capabilities::default(),
        }
    }

    fn dialog(saved: &[(String, String)]) -> OptionsDialog {
        let mut list = EngineList::new();
        let id = list.add(EngineDraft::new("Fake", "/fake")).expect("add");
        let mut dialog = OptionsDialog::new(list.get(id).expect("entry"));
        dialog.set_identity(identity(), saved);
        dialog
    }

    #[test]
    fn important_options_come_first_in_the_scid_order_and_managed_ones_are_hidden() {
        let identity = identity();
        let names: Vec<_> = ordered_options(&identity)
            .into_iter()
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "MultiPV",
                "Hash",
                "Threads",
                "Style",
                "Clear Hash",
                "SyzygyPath"
            ]
        );
    }

    #[test]
    fn the_spin_step_follows_the_range_and_multipv_and_threads_stay_at_one() {
        assert_eq!(spin_step("Skill Level", 0, 20), 1);
        assert_eq!(spin_step("UCI_Elo", 1320, 3190), 50);
        assert_eq!(spin_step("Move Overhead", 0, 200), 10);
        assert_eq!(spin_step("Hash", 1, 102), 10);
        assert_eq!(spin_step("Hash", 1, 100), 1);
        assert_eq!(spin_step("Hash", 0, 1000), 10);
        assert_eq!(spin_step("Hash", 0, 1001), 50);
        assert_eq!(spin_step("MultiPV", 1, 5000), 1);
        assert_eq!(spin_step("Threads", 1, 5000), 1);
        assert_eq!(spin_step("X", i64::MIN, i64::MAX), 50);
    }

    #[test]
    fn the_form_starts_with_the_saved_value_or_the_default_and_no_button() {
        let d = dialog(&[("Hash".to_owned(), "128".to_owned())]);
        assert_eq!(d.value_of("Hash"), Some("128"));
        assert_eq!(d.value_of("Threads"), Some("1"));
        assert_eq!(d.value_of("Style"), Some("Normal"));
        assert_eq!(d.value_of("Clear Hash"), None);
        assert_eq!(d.status, OptionsStatus::Ready);
    }

    #[test]
    fn only_what_differs_from_the_default_is_saved() {
        let mut d = dialog(&[]);
        assert_eq!(d.checked(), Ok(vec![]));
        d.set("Hash", "64");
        d.set("Style", "Solid");
        d.set("Threads", "1");
        assert_eq!(
            d.checked(),
            Ok(vec![
                ("Hash".to_owned(), "64".to_owned()),
                ("Style".to_owned(), "Solid".to_owned()),
            ])
        );
    }

    #[test]
    fn a_value_out_of_range_or_not_listed_blocks_the_save() {
        let mut d = dialog(&[]);
        d.set("Hash", "0");
        d.set("Style", "Risky");
        d.set("MultiPV", "abc");
        let Err(errors) = d.checked() else {
            panic!("refused");
        };
        let names: Vec<_> = errors.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["MultiPV", "Hash", "Style"]);
        assert!(matches!(
            errors[1].1,
            OptionError::OutOfRange { min: 1, .. }
        ));
        assert!(matches!(errors[2].1, OptionError::NotInList { .. }));
        assert!(matches!(errors[0].1, OptionError::WrongType { .. }));
    }

    #[test]
    fn editing_an_option_clears_its_refusal_and_reset_restores_the_defaults() {
        let mut d = dialog(&[("Hash".to_owned(), "128".to_owned())]);
        d.set("Hash", "0");
        d.errors = vec![("Hash".to_owned(), OptionError::Unknown("Hash".to_owned()))];
        d.set("Hash", "5");
        assert!(d.error_of("Hash").is_none());
        d.reset();
        assert_eq!(d.value_of("Hash"), Some("16"));
        assert_eq!(d.checked(), Ok(vec![]));
    }

    #[test]
    fn an_xboard_engine_has_nothing_to_probe() {
        let mut list = EngineList::new();
        let mut draft = EngineDraft::new("Old", "/old");
        draft.protocol = Protocol::XBoard;
        let id = list.add(draft).expect("add");
        let d = OptionsDialog::new(list.get(id).expect("entry"));
        assert_eq!(d.status, OptionsStatus::NotUci);
        assert!(d.visible_options().is_empty());
    }
}
