//! Errors of the engine list (spec M4-3). Plain data: they are `Clone` and
//! comparable so a `Message` can carry them, and the front-end translates from
//! the variant.

use crate::error::FileError;

/// Why a command line could not be split (D428).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ArgsError {
    /// A single or double quote was opened and never closed.
    #[error("unclosed quote in the arguments")]
    UnclosedQuote,
}

/// Why an engine entry is not acceptable (D429).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValidationError {
    /// The name is empty after trimming.
    #[error("the engine name is empty")]
    EmptyName,
    /// The command is empty after trimming.
    #[error("the engine command is empty")]
    EmptyCommand,
    /// A path is not valid UTF-8, so it cannot be stored in the TOML file.
    #[error("a path is not valid UTF-8")]
    NonUtf8Path,
    /// The Elo rating is above the limit.
    #[error("the Elo rating {0} is above {max}", max = crate::engines::MAX_ELO)]
    EloOutOfRange(u16),
    /// A text field is longer than the limit.
    #[error("the field `{field}` is longer than {limit} characters")]
    TextTooLong {
        /// The field.
        field: &'static str,
        /// The limit, in characters.
        limit: usize,
    },
    /// There are more arguments or saved options than the limit.
    #[error("the field `{field}` has more than {limit} items")]
    TooManyItems {
        /// The field.
        field: &'static str,
        /// The limit.
        limit: usize,
    },
}

/// Why an operation on the list was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ListError {
    /// The entry is not acceptable.
    #[error(transparent)]
    Invalid(#[from] ValidationError),
    /// No engine has this id.
    #[error("no engine with id {0}")]
    NotFound(u64),
    /// The list is full.
    #[error("the list already holds {limit} engines")]
    TooManyEngines {
        /// The limit.
        limit: usize,
    },
}

/// Why `engines.toml` could not be read or written. When loading fails the
/// caller keeps the list it had.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The text is not valid TOML or its header (`version`) is wrong.
    #[error("the engine list is not valid TOML")]
    Syntax,
    /// The file was written by a newer version of the program.
    #[error("the engine list has version {found}, newer than supported")]
    NewerVersion {
        /// The version found.
        found: i64,
    },
    /// The file is larger than the limit (bytes).
    #[error("the engine list is larger than {limit} bytes")]
    TooLarge {
        /// The limit.
        limit: u64,
    },
    /// The file is not UTF-8.
    #[error("the engine list is not valid UTF-8")]
    NotUtf8,
    /// Reading or writing failed.
    #[error(transparent)]
    File(#[from] FileError),
}
