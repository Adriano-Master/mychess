//! The `[eboard]` table of the preferences and the validation of the
//! settings (spec M8-10, D2740, D2741).

use std::path::PathBuf;

use mychess_eboard::{BoardKind, SendFilter, validate_port};
use serde::Serialize;

/// Longest port name kept (the port check allows 255 bytes).
const MAX_PORT_CHARS: usize = 255;
/// Longest driver path kept, in characters.
const MAX_COMMAND_CHARS: usize = 4096;
/// Longest parameter text kept, in characters.
const MAX_PARAMS_CHARS: usize = 256;

/// Which board the user uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[non_exhaustive]
pub enum BoardChoice {
    /// None (the default).
    #[default]
    #[serde(rename = "none")]
    None,
    /// A Novag Citrine on a serial port.
    #[serde(rename = "novag")]
    Novag,
    /// A driver program speaking the input engine protocol (DGT).
    #[serde(rename = "input_engine")]
    InputEngine,
}

impl BoardChoice {
    /// The spelling used in the TOML file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            BoardChoice::None => "none",
            BoardChoice::Novag => "novag",
            BoardChoice::InputEngine => "input_engine",
        }
    }

    /// Reads the TOML spelling.
    #[must_use]
    pub fn from_name(text: &str) -> Option<BoardChoice> {
        match text {
            "none" => Some(BoardChoice::None),
            "novag" => Some(BoardChoice::Novag),
            "input_engine" => Some(BoardChoice::InputEngine),
            _ => None,
        }
    }
}

/// Which moves the driver reports ("Moves sent for").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[non_exhaustive]
pub enum FilterChoice {
    /// Both sides (the default).
    #[default]
    #[serde(rename = "both")]
    Both,
    /// Only White's moves.
    #[serde(rename = "white")]
    White,
    /// Only Black's moves.
    #[serde(rename = "black")]
    Black,
}

impl FilterChoice {
    /// The spelling used in the TOML file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FilterChoice::Both => "both",
            FilterChoice::White => "white",
            FilterChoice::Black => "black",
        }
    }

    /// Reads the TOML spelling.
    #[must_use]
    pub fn from_name(text: &str) -> Option<FilterChoice> {
        match text {
            "both" => Some(FilterChoice::Both),
            "white" => Some(FilterChoice::White),
            "black" => Some(FilterChoice::Black),
            _ => None,
        }
    }

    pub(crate) fn to_filter(self) -> SendFilter {
        match self {
            FilterChoice::Both => SendFilter::Both,
            FilterChoice::White => SendFilter::White,
            FilterChoice::Black => SendFilter::Black,
        }
    }
}

/// Why the settings cannot be used to connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EboardIssue {
    /// "None" is chosen.
    NoBoardChosen,
    /// The port name is not usable.
    InvalidPort,
    /// The driver command is empty or has a NUL.
    InvalidCommand,
    /// The driver parameters have control characters.
    InvalidParams,
}

impl EboardIssue {
    /// The Fluent key of the message.
    #[must_use]
    pub fn message_key(self) -> &'static str {
        match self {
            EboardIssue::NoBoardChosen => "eboard-issue-no-board",
            EboardIssue::InvalidPort => "eboard-issue-port",
            EboardIssue::InvalidCommand => "eboard-issue-command",
            EboardIssue::InvalidParams => "eboard-issue-params",
        }
    }
}

/// What the user configured about the electronic board (D2740).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EboardPrefs {
    /// The kind of board.
    pub kind: BoardChoice,
    /// The serial port (`COM3`, `/dev/ttyUSB0`, `/dev/cu.usbserial`).
    pub port: String,
    /// The driver program (input engine only).
    pub engine_command: String,
    /// The driver parameters, split on blanks (Scid: `la`).
    pub engine_params: String,
    /// Which moves the driver reports.
    pub send_filter: FilterChoice,
}

impl Default for EboardPrefs {
    fn default() -> EboardPrefs {
        EboardPrefs {
            kind: BoardChoice::None,
            port: mychess_eboard::default_port().to_owned(),
            engine_command: String::new(),
            engine_params: "la".to_owned(),
            send_filter: FilterChoice::Both,
        }
    }
}

fn clean(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\0' | '\n' | '\r'))
        .collect::<String>()
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

impl EboardPrefs {
    /// Removes control characters and cuts the texts to their limits.
    #[must_use]
    pub fn sanitized(mut self) -> EboardPrefs {
        self.port = clean(&self.port, MAX_PORT_CHARS);
        self.engine_command = clean(&self.engine_command, MAX_COMMAND_CHARS);
        self.engine_params = clean(&self.engine_params, MAX_PARAMS_CHARS);
        self
    }

    /// The board to connect to, or what is wrong with the settings (D2741).
    ///
    /// # Errors
    ///
    /// An [`EboardIssue`].
    pub fn validate(&self) -> Result<BoardKind, EboardIssue> {
        let port = self.port.trim();
        match self.kind {
            BoardChoice::None => Err(EboardIssue::NoBoardChosen),
            BoardChoice::Novag => {
                validate_port(port).map_err(|_| EboardIssue::InvalidPort)?;
                Ok(BoardKind::Novag {
                    port: port.to_owned(),
                })
            }
            BoardChoice::InputEngine => {
                validate_port(port).map_err(|_| EboardIssue::InvalidPort)?;
                let command = self.engine_command.trim();
                if command.is_empty() || command.contains('\0') {
                    return Err(EboardIssue::InvalidCommand);
                }
                if self
                    .engine_params
                    .chars()
                    .any(|c| matches!(c, '\0' | '\n' | '\r'))
                    || self.engine_params.chars().count() > MAX_PARAMS_CHARS
                {
                    return Err(EboardIssue::InvalidParams);
                }
                Ok(BoardKind::InputEngine {
                    command: PathBuf::from(command),
                    port: port.to_owned(),
                    params: self.engine_params.trim().to_owned(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn novag(port: &str) -> EboardPrefs {
        EboardPrefs {
            kind: BoardChoice::Novag,
            port: port.to_owned(),
            ..EboardPrefs::default()
        }
    }

    #[test]
    fn default_is_no_board_with_the_platform_port() {
        let d = EboardPrefs::default();
        assert_eq!(d.kind, BoardChoice::None);
        assert!(!d.port.is_empty());
        assert_eq!(d.validate(), Err(EboardIssue::NoBoardChosen));
    }

    #[test]
    fn novag_needs_a_usable_port() {
        assert_eq!(
            novag(" COM3 ").validate(),
            Ok(BoardKind::Novag {
                port: "COM3".to_owned()
            })
        );
        assert_eq!(novag("").validate(), Err(EboardIssue::InvalidPort));
        assert_eq!(novag("a\0b").validate(), Err(EboardIssue::InvalidPort));
    }

    #[test]
    fn the_driver_needs_a_command_and_clean_parameters() {
        let mut p = EboardPrefs {
            kind: BoardChoice::InputEngine,
            port: "/dev/ttyUSB0".to_owned(),
            engine_command: "dgtdrv2.i686".to_owned(),
            engine_params: "la".to_owned(),
            send_filter: FilterChoice::Both,
        };
        assert!(matches!(
            p.validate(),
            Ok(BoardKind::InputEngine { params, .. }) if params == "la"
        ));
        p.engine_command = "  ".to_owned();
        assert_eq!(p.validate(), Err(EboardIssue::InvalidCommand));
        p.engine_command = "x".to_owned();
        p.engine_params = "a\nb".to_owned();
        assert_eq!(p.validate(), Err(EboardIssue::InvalidParams));
    }

    #[test]
    fn sanitized_cuts_and_cleans() {
        let p = EboardPrefs {
            kind: BoardChoice::Novag,
            port: format!(" COM3\n{}", "x".repeat(400)),
            engine_command: "a\0b".to_owned(),
            engine_params: "l\ra".to_owned(),
            send_filter: FilterChoice::Black,
        }
        .sanitized();
        assert!(p.port.starts_with("COM3x"));
        assert_eq!(p.port.chars().count(), 255);
        assert_eq!(p.engine_command, "ab");
        assert_eq!(p.engine_params, "la");
    }

    #[test]
    fn names_round_trip() {
        for kind in [
            BoardChoice::None,
            BoardChoice::Novag,
            BoardChoice::InputEngine,
        ] {
            assert_eq!(BoardChoice::from_name(kind.as_str()), Some(kind));
        }
        for f in [FilterChoice::Both, FilterChoice::White, FilterChoice::Black] {
            assert_eq!(FilterChoice::from_name(f.as_str()), Some(f));
        }
        assert_eq!(BoardChoice::from_name("x"), None);
        assert_eq!(FilterChoice::from_name(""), None);
    }
}
