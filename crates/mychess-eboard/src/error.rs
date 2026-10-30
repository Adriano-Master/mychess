//! Errors of the crate (spec m8-10, D2702, D2717).

use std::path::PathBuf;

use thiserror::Error;

/// Why a connection could not be set up, or a setting is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum EboardError {
    /// The port name is not usable (see [`validate_port`](crate::validate_port)).
    #[error("invalid port name: {0}")]
    InvalidPort(String),
    /// The serial port could not be opened (not there, busy, no permission).
    #[error("cannot open serial port {port}: {message}")]
    OpenPort {
        /// The port name as configured.
        port: String,
        /// The message of the system.
        message: String,
    },
    /// Listing the serial ports failed (or the library panicked doing it).
    #[error("cannot list the serial ports: {0}")]
    PortList(String),
    /// The driver program could not be started.
    #[error("cannot start the input engine {}: {message}", path.display())]
    Spawn {
        /// The program as configured.
        path: PathBuf,
        /// The message of the system.
        message: String,
    },
    /// The driver command or its parameters are not acceptable.
    #[error("invalid input engine setting: {0}")]
    InvalidDriver(String),
}
