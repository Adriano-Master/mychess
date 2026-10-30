//! The MultiPV traffic lights (spec M4-5, D458; Scid `board.tcl`
//! `eval2tlcolor`, `update_mpvtls`).

use crate::analysis::AnalysisView;

/// An opaque colour, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// The colour as `#rrggbb` in lower case.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// One strip of the traffic lights: one MultiPV line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficLight {
    /// The line number, from 1 (drawn at the top).
    pub rank: u8,
    /// The colour of its score.
    pub color: Rgb,
}

/// The colour of a score in pawns, from White's point of view: green when
/// White is better, red when Black is, yellow in between (Scid
/// `eval2tlcolor`). Scores beyond 5 pawns give the full colour; a NaN or
/// an infinity gives yellow.
pub fn traffic_light_color(eval_pawns: f32) -> Rgb {
    // The thresholds are compared as `f32` so that `0.2` is the same number
    // as the literal (as `0.2` is in Tcl, which uses `f64` throughout).
    // Rounded to 1e-6 so that the `f32` 0.2 gives the same shade as the
    // exact 0.2 of Tcl (the f32 is 0.2000000030).
    let eval = (f64::from(eval_pawns) * 1.0e6).round() / 1.0e6;
    let shade = |magnitude: f64| -> u8 {
        let ratio = (magnitude.min(5.0) - 0.2) / 5.0;
        // `(1 - ratio) * 240` is in `0.0..=240.0`, so the truncation is exact
        // (Tcl: `int(...)`).
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = ((1.0 - ratio) * 240.0) as u8;
        value
    };
    if !eval_pawns.is_finite() {
        return Rgb {
            r: 0xf8,
            g: 0xf8,
            b: 0,
        };
    }
    if eval_pawns > 0.2_f32 {
        Rgb {
            r: shade(eval),
            g: 0xf8,
            b: 0,
        }
    } else if eval_pawns > -0.2_f32 {
        Rgb {
            r: 0xf8,
            g: 0xf8,
            b: 0,
        }
    } else {
        Rgb {
            r: 0xf8,
            g: shade(-eval),
            b: 0,
        }
    }
}

/// The strips for the lines of `view`, line 1 first. Empty when no line has a
/// move yet; with one line it is a single strip.
pub fn traffic_lights(view: &AnalysisView) -> Vec<TrafficLight> {
    if !view.lines.iter().any(|l| !l.pv.is_empty()) {
        return Vec::new();
    }
    let mut lights: Vec<TrafficLight> = view
        .lines
        .iter()
        .map(|line| TrafficLight {
            rank: line.multipv,
            color: traffic_light_color(line.score.pawns()),
        })
        .collect();
    lights.sort_by_key(|l| l.rank);
    lights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_scid_table() {
        let hex = |e: f32| traffic_light_color(e).hex();
        assert_eq!(hex(0.0), "#f8f800");
        assert_eq!(hex(0.2), "#f8f800");
        // Scid `elseif {$eval > -0.2}`: exactly -0.2 is already red (D831).
        assert_eq!(hex(-0.2), "#f8f000");
        assert_eq!(hex(0.25), "#edf800");
        assert_eq!(hex(1.0), "#c9f800");
        assert_eq!(hex(5.0), "#09f800");
        assert_eq!(hex(9.0), "#09f800");
        assert_eq!(hex(327.0), "#09f800");
        assert_eq!(hex(-0.25), "#f8ed00");
        assert_eq!(hex(-1.0), "#f8c900");
        assert_eq!(hex(-5.0), "#f80900");
        assert_eq!(hex(-327.0), "#f80900");
    }

    #[test]
    fn hostile_values_do_not_panic() {
        assert_eq!(traffic_light_color(f32::NAN).hex(), "#f8f800");
        // Infinities have no colour of advantage either (rule 1).
        assert_eq!(traffic_light_color(f32::INFINITY).hex(), "#f8f800");
        assert_eq!(traffic_light_color(f32::NEG_INFINITY).hex(), "#f8f800");
    }
}
