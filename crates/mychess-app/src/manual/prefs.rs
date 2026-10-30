//! The `[manual]` table of the preferences (spec M9-5, D4060).

use serde::Serialize;

/// The largest step of the size of the text.
pub const MAX_TEXT_SCALE: u8 = 6;
/// The step of the normal size.
pub const DEFAULT_TEXT_SCALE: u8 = 2;

/// What the manual remembers, in `[manual]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ManualPrefs {
    /// The size of the text, in steps from `0` (smallest) to
    /// [`MAX_TEXT_SCALE`]; [`DEFAULT_TEXT_SCALE`] is the size of the
    /// interface.
    pub text_scale: u8,
}

impl Default for ManualPrefs {
    fn default() -> ManualPrefs {
        ManualPrefs {
            text_scale: DEFAULT_TEXT_SCALE,
        }
    }
}

impl ManualPrefs {
    /// The value with a step out of range back at the default.
    #[must_use]
    pub fn sanitized(mut self) -> ManualPrefs {
        if self.text_scale > MAX_TEXT_SCALE {
            self.text_scale = DEFAULT_TEXT_SCALE;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_step_goes_back_to_default() {
        let bad = ManualPrefs { text_scale: 99 };
        assert_eq!(bad.sanitized(), ManualPrefs::default());
        let ok = ManualPrefs { text_scale: 0 };
        assert_eq!(ok.sanitized(), ok);
    }
}
