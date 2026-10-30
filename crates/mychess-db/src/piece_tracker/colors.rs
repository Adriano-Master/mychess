//! The colours of the heat map (`::ptrack::color` of
//! `tcl/tools/ptracker.tcl`, rule P10 and criterion 16).
//!
//! No toolkit type: the colour is a plain `[u8; 3]` (red, green, blue).

/// The ten colour schemes of the piece tracker. The `X` schemes are the
/// inverted versions of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorScheme {
    /// Grey, white to black.
    Black,
    /// White to red.
    Red,
    /// White to yellow.
    Yellow,
    /// White to cyan.
    Cyan,
    /// White to blue (the default).
    #[default]
    Blue,
    /// Inverted [`ColorScheme::Black`].
    XBlack,
    /// Inverted [`ColorScheme::Red`].
    XRed,
    /// Inverted [`ColorScheme::Yellow`].
    XYellow,
    /// Inverted [`ColorScheme::Cyan`].
    XCyan,
    /// Inverted [`ColorScheme::Blue`].
    XBlue,
}

impl ColorScheme {
    /// The ten schemes, in the order of the Scid menu.
    pub const ALL: [ColorScheme; 10] = [
        ColorScheme::Black,
        ColorScheme::Red,
        ColorScheme::Yellow,
        ColorScheme::Cyan,
        ColorScheme::Blue,
        ColorScheme::XBlack,
        ColorScheme::XRed,
        ColorScheme::XYellow,
        ColorScheme::XCyan,
        ColorScheme::XBlue,
    ];

    /// The name the Scid uses (`blue`, `xred`...), also the preference value.
    pub fn key(self) -> &'static str {
        match self {
            ColorScheme::Black => "black",
            ColorScheme::Red => "red",
            ColorScheme::Yellow => "yellow",
            ColorScheme::Cyan => "cyan",
            ColorScheme::Blue => "blue",
            ColorScheme::XBlack => "xblack",
            ColorScheme::XRed => "xred",
            ColorScheme::XYellow => "xyellow",
            ColorScheme::XCyan => "xcyan",
            ColorScheme::XBlue => "xblue",
        }
    }

    /// The scheme named `key` (see [`ColorScheme::key`]).
    pub fn from_key(key: &str) -> Option<ColorScheme> {
        ColorScheme::ALL.into_iter().find(|s| s.key() == key)
    }

    /// The colour of a square whose value is `fraction` (0 to 1) of the
    /// largest one. Outside the range it is limited to it.
    pub fn rgb(self, fraction: f64) -> [u8; 3] {
        self.rgb_percent(fraction * 100.0)
    }

    /// The colour for `pct` per cent of the largest value, as
    /// `::ptrack::color`: `x = clamp(pct, 0.01, 100)`;
    /// `y = 255 - round(0.5 x + 10 ln x)` and `yb = 255 - round(2 x + 10 ln x)`,
    /// each limited to 0..=255 and raised to 1 when it would be 0 for a
    /// `pct` above 0; the inverted schemes use `255 - y` and `255 - yb`.
    ///
    /// ```
    /// use mychess_db::ColorScheme;
    ///
    /// assert_eq!(ColorScheme::Blue.rgb_percent(100.0), [9, 9, 159]);
    /// assert_eq!(ColorScheme::Blue.rgb_percent(0.0), [255, 255, 255]);
    /// ```
    pub fn rgb_percent(self, pct: f64) -> [u8; 3] {
        let pct = if pct.is_nan() { 0.0 } else { pct };
        let x = pct.clamp(0.01, 100.0);
        let level = |weight: f64| -> u8 {
            let raw = 255.0 - (x * weight + 10.0 * x.ln()).round();
            let mut v = raw.clamp(0.0, 255.0);
            if pct > 0.0 && v == 0.0 {
                v = 1.0;
            }
            // `v` is an integer value in 0..=255.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let byte = v as u8;
            byte
        };
        let y = level(0.5);
        let yb = level(2.0);
        let (xy, xyb) = (255 - y, 255 - yb);
        match self {
            ColorScheme::Black => [yb, yb, yb],
            ColorScheme::Red => [y, yb, yb],
            ColorScheme::Yellow => [y, y, yb],
            ColorScheme::Cyan => [yb, y, y],
            ColorScheme::Blue => [yb, yb, y],
            ColorScheme::XBlack => [xyb, xyb, xyb],
            ColorScheme::XRed => [xyb, xy, xy],
            ColorScheme::XYellow => [xyb, xyb, xy],
            ColorScheme::XCyan => [xy, xyb, xyb],
            ColorScheme::XBlue => [xy, xy, xyb],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_value_in_blue_is_9_9_159() {
        assert_eq!(ColorScheme::Blue.rgb_percent(100.0), [9, 9, 159]);
        assert_eq!(ColorScheme::Blue.rgb(1.0), [9, 9, 159]);
    }

    #[test]
    fn the_lowest_value_is_white_and_zero_is_white() {
        for scheme in [
            ColorScheme::Black,
            ColorScheme::Red,
            ColorScheme::Yellow,
            ColorScheme::Cyan,
            ColorScheme::Blue,
        ] {
            assert_eq!(scheme.rgb_percent(0.0), [255, 255, 255], "{scheme:?}");
            assert_eq!(scheme.rgb_percent(-4.0), [255, 255, 255]);
        }
    }

    #[test]
    fn the_ten_schemes_at_full_value() {
        // y = 159, yb = 9 at 100 %.
        let expected = [
            (ColorScheme::Black, [9, 9, 9]),
            (ColorScheme::Red, [159, 9, 9]),
            (ColorScheme::Yellow, [159, 159, 9]),
            (ColorScheme::Cyan, [9, 159, 159]),
            (ColorScheme::Blue, [9, 9, 159]),
            (ColorScheme::XBlack, [246, 246, 246]),
            (ColorScheme::XRed, [246, 96, 96]),
            (ColorScheme::XYellow, [246, 246, 96]),
            (ColorScheme::XCyan, [96, 246, 246]),
            (ColorScheme::XBlue, [96, 96, 246]),
        ];
        for (scheme, rgb) in expected {
            assert_eq!(scheme.rgb_percent(100.0), rgb, "{scheme:?}");
        }
    }

    #[test]
    fn a_tiny_positive_value_is_not_pure_black() {
        // x = 50: yb = 255 - round(100 + 39.12) = 116; the clamp-to-1 rule
        // matters only when the formula reaches 0 or below.
        let [yb, _, _] = ColorScheme::Black.rgb_percent(50.0);
        assert_eq!(yb, 116);
        // 100 %: yb = 9 > 0, so the floor of 1 is not reached by any x here.
        let darkest = ColorScheme::ALL
            .iter()
            .flat_map(|s| s.rgb_percent(100.0))
            .min();
        assert_eq!(darkest, Some(9));
    }

    #[test]
    fn keys_round_trip() {
        for scheme in ColorScheme::ALL {
            assert_eq!(ColorScheme::from_key(scheme.key()), Some(scheme));
        }
        assert_eq!(ColorScheme::from_key("green"), None);
        assert_eq!(ColorScheme::default(), ColorScheme::Blue);
    }
}
