//! Colors and glyphs of the move classes of the game report (spec M4-10b,
//! D1424, D1432). The colors come from the NAG tones of the palette, so the
//! two themes stay consistent; the glyph and the name always go with the
//! color (the class never depends on hue alone).

use iced::Color;
use mychess_app::report::MoveClass;

use super::tokens::Tokens;

/// The color of `class` in the palette of `t`.
pub fn class_color(t: &Tokens, class: MoveClass) -> Color {
    match class {
        MoveClass::Brilliant => t.nag.brilliant,
        MoveClass::Great | MoveClass::Best => t.nag.good,
        MoveClass::Excellent | MoveClass::Good => t.nag.eval,
        MoveClass::Book => t.muted,
        MoveClass::Inaccuracy => t.nag.dubious,
        MoveClass::Mistake | MoveClass::Miss => t.nag.mistake,
        MoveClass::Blunder => t.nag.blunder,
    }
}

/// The short symbol of `class`. Plain ASCII: it must render with any font.
pub fn class_glyph(class: MoveClass) -> &'static str {
    match class {
        MoveClass::Brilliant => "!!",
        MoveClass::Great => "!",
        MoveClass::Best => "*",
        MoveClass::Excellent => "+",
        MoveClass::Good => "~",
        MoveClass::Book => "B",
        MoveClass::Inaccuracy => "?!",
        MoveClass::Mistake => "?",
        MoveClass::Miss => "x",
        MoveClass::Blunder => "??",
    }
}

/// The class a marker of the curve stands for.
pub fn marker_class(kind: mychess_app::graphs::MarkerKind) -> MoveClass {
    use mychess_app::graphs::MarkerKind;
    match kind {
        MarkerKind::Brilliant => MoveClass::Brilliant,
        MarkerKind::Great => MoveClass::Great,
        MarkerKind::Inaccuracy => MoveClass::Inaccuracy,
        MarkerKind::Mistake => MoveClass::Mistake,
        MarkerKind::Miss => MoveClass::Miss,
        MarkerKind::Blunder => MoveClass::Blunder,
        // `MarkerKind` may grow; an unknown marker reads as a plain good move.
        #[allow(unreachable_patterns)]
        _ => MoveClass::Good,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_has_a_glyph_and_the_glyphs_differ() {
        let mut glyphs: Vec<&str> = MoveClass::ALL.iter().map(|c| class_glyph(*c)).collect();
        glyphs.sort_unstable();
        glyphs.dedup();
        assert_eq!(glyphs.len(), MoveClass::ALL.len());
    }

    #[test]
    fn faults_do_not_share_the_color_of_the_best_moves() {
        for t in [Tokens::dark(), Tokens::light()] {
            assert_ne!(
                class_color(&t, MoveClass::Blunder),
                class_color(&t, MoveClass::Best)
            );
            assert_ne!(
                class_color(&t, MoveClass::Mistake),
                class_color(&t, MoveClass::Great)
            );
        }
    }
}
