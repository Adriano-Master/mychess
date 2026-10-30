//! The tips of the day (Scid: `tcl/help/tips.tcl`). The texts are Fluent
//! messages of the front-end (`tip-1`, `tip-2`...); here only their number.

/// The Fluent keys of the tips, in the order they are shown.
const KEYS: [&str; 10] = [
    "tip-1", "tip-2", "tip-3", "tip-4", "tip-5", "tip-6", "tip-7", "tip-8", "tip-9", "tip-10",
];

/// How many tips there are (the front-end tests that every language has
/// all of them).
#[allow(clippy::cast_possible_truncation)] // ten
pub const TIP_COUNT: u32 = KEYS.len() as u32;

/// The Fluent key of tip `index`; an index out of range wraps around.
#[must_use]
pub fn tip_key(index: u32) -> &'static str {
    let at = usize::try_from(index % TIP_COUNT).unwrap_or(0);
    KEYS.get(at).copied().unwrap_or(KEYS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_numbered_from_one_and_wrap() {
        assert_eq!(tip_key(0), "tip-1");
        assert_eq!(tip_key(TIP_COUNT - 1), format!("tip-{TIP_COUNT}"));
        assert_eq!(tip_key(TIP_COUNT), "tip-1");
        assert_eq!(tip_key(u32::MAX), tip_key(u32::MAX % TIP_COUNT));
    }
}
