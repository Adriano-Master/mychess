//! The default sounds, rendered in code (D2801, D2810).
//!
//! Nothing here reads a file: each [`SoundCue`] is a few damped sine waves and
//! a short burst of filtered noise from a fixed seed, so the same call gives
//! the same samples every time (on one platform; `sin` may differ by one unit
//! in the last place between platforms, which nobody can hear) and there is
//! no audio asset to license.

// The sounds last milliseconds at 44.1 kHz: every index, count and rate cast
// below is under 2^20, exactly representable in `f32`, and never negative.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::f32::consts::TAU;

use super::bank::Clip;
use super::cue::SoundCue;

/// Samples per second of the rendered sounds.
pub const SAMPLE_RATE: u32 = 44_100;
/// The largest absolute sample (D2810): headroom against clipping.
pub const PEAK: f32 = 0.6;
/// The linear fade at the end of every sound, in milliseconds (no click).
const FADE_OUT_MS: f32 = 6.0;
/// The linear fade at the start of every note, in milliseconds.
const FADE_IN_MS: f32 = 2.0;

/// The seed of the noise generator (a constant: determinism).
const NOISE_SEED: u32 = 0x9E37_79B9;

fn samples_for(ms: f32) -> usize {
    // Rounded: the sounds last milliseconds, far from the `usize` limits.
    (ms * SAMPLE_RATE as f32 / 1000.0).round() as usize
}

/// A mix of notes placed in time.
struct Mix {
    samples: Vec<f32>,
}

impl Mix {
    fn new(total_ms: f32) -> Mix {
        Mix {
            samples: vec![0.0; samples_for(total_ms)],
        }
    }

    fn add(&mut self, start_ms: f32, wave: &[f32]) {
        let start = samples_for(start_ms);
        for (i, v) in wave.iter().enumerate() {
            if let Some(slot) = self.samples.get_mut(start + i) {
                *slot += v;
            }
        }
    }

    /// The final samples: faded out and limited to [`PEAK`].
    fn finish(mut self) -> Vec<f32> {
        let fade = samples_for(FADE_OUT_MS).max(1);
        let len = self.samples.len();
        for (i, v) in self.samples.iter_mut().rev().take(fade).enumerate() {
            *v *= i as f32 / fade as f32;
        }
        let peak = self.samples.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
        if peak > PEAK {
            let scale = PEAK / peak;
            for v in &mut self.samples {
                *v *= scale;
            }
        }
        debug_assert_eq!(len, self.samples.len());
        self.samples
    }
}

/// A sine with a second harmonic, an attack of [`FADE_IN_MS`] and an
/// exponential decay of time constant `decay_ms`, `ms` long.
fn note(freq: f32, ms: f32, amp: f32, decay_ms: f32) -> Vec<f32> {
    let n = samples_for(ms);
    let attack = samples_for(FADE_IN_MS).max(1);
    let decay = decay_ms * SAMPLE_RATE as f32 / 1000.0;
    (0..n)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let env = (-(i as f32) / decay).exp() * (i.min(attack) as f32 / attack as f32);
            let wave = (TAU * freq * t).sin() + 0.3 * (TAU * 2.0 * freq * t).sin();
            amp * env * wave
        })
        .collect()
}

/// A short burst of low-passed noise (the "wood" of a piece on the board).
fn knock(ms: f32, amp: f32, decay_ms: f32) -> Vec<f32> {
    let n = samples_for(ms);
    let decay = decay_ms * SAMPLE_RATE as f32 / 1000.0;
    let mut state = NOISE_SEED;
    let mut low = 0.0_f32;
    (0..n)
        .map(|i| {
            // LCG (Numerical Recipes constants), the high bits to [-1, 1).
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (state >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
            low += 0.2 * (white - low);
            amp * low * (-(i as f32) / decay).exp()
        })
        .collect()
}

/// The sound of `cue` (mono, [`SAMPLE_RATE`]).
#[must_use]
pub fn render(cue: SoundCue) -> Clip {
    let samples = match cue {
        SoundCue::Move => {
            let mut m = Mix::new(90.0);
            m.add(0.0, &note(320.0, 90.0, 0.5, 18.0));
            m.add(0.0, &knock(40.0, 1.2, 8.0));
            m.finish()
        }
        SoundCue::Capture => {
            let mut m = Mix::new(140.0);
            m.add(0.0, &note(210.0, 140.0, 0.5, 26.0));
            m.add(0.0, &knock(70.0, 2.0, 16.0));
            m.finish()
        }
        SoundCue::Check => {
            let mut m = Mix::new(240.0);
            m.add(0.0, &note(660.0, 110.0, 0.4, 40.0));
            m.add(80.0, &note(880.0, 160.0, 0.4, 60.0));
            m.finish()
        }
        SoundCue::Checkmate => {
            let mut m = Mix::new(600.0);
            m.add(0.0, &note(660.0, 200.0, 0.4, 70.0));
            m.add(130.0, &note(523.0, 200.0, 0.4, 70.0));
            m.add(260.0, &note(392.0, 340.0, 0.4, 110.0));
            m.finish()
        }
        SoundCue::Castle => {
            let mut m = Mix::new(240.0);
            m.add(0.0, &note(300.0, 90.0, 0.5, 18.0));
            m.add(0.0, &knock(40.0, 1.2, 8.0));
            m.add(120.0, &note(360.0, 100.0, 0.5, 18.0));
            m.add(120.0, &knock(40.0, 1.2, 8.0));
            m.finish()
        }
        SoundCue::Promotion => {
            let mut m = Mix::new(380.0);
            for (i, f) in [523.0, 659.0, 784.0, 1047.0].into_iter().enumerate() {
                m.add(i as f32 * 80.0, &note(f, 130.0, 0.35, 45.0));
            }
            m.finish()
        }
        SoundCue::Back => {
            let mut m = Mix::new(70.0);
            m.add(0.0, &note(240.0, 70.0, 0.3, 14.0));
            m.add(0.0, &knock(25.0, 0.6, 6.0));
            m.finish()
        }
        SoundCue::GameStart => {
            let mut m = Mix::new(420.0);
            for (i, f) in [392.0, 523.0, 659.0].into_iter().enumerate() {
                m.add(i as f32 * 70.0, &note(f, 280.0, 0.35, 120.0));
            }
            m.finish()
        }
        SoundCue::GameEnd => {
            let mut m = Mix::new(520.0);
            for (i, f) in [659.0, 523.0, 392.0].into_iter().enumerate() {
                m.add(i as f32 * 110.0, &note(f, 280.0, 0.35, 120.0));
            }
            m.finish()
        }
        SoundCue::LowTime => {
            let mut m = Mix::new(220.0);
            m.add(0.0, &note(1760.0, 60.0, 0.4, 50.0));
            m.add(120.0, &note(1760.0, 60.0, 0.4, 50.0));
            m.finish()
        }
    };
    Clip {
        sample_rate: SAMPLE_RATE,
        channels: 1,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_deterministic() {
        for cue in SoundCue::ALL {
            assert_eq!(render(cue), render(cue), "{cue:?}");
        }
    }

    #[test]
    fn every_sound_is_short_audible_and_not_clipped() {
        for cue in SoundCue::ALL {
            let clip = render(cue);
            let ms = clip.samples.len() * 1000 / SAMPLE_RATE as usize;
            assert!((40..=600).contains(&ms), "{cue:?} lasts {ms} ms");
            let peak = clip.samples.iter().fold(0.0_f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.1, "{cue:?} is too quiet ({peak})");
            assert!(peak <= PEAK + 1e-6, "{cue:?} is too loud ({peak})");
            assert!(clip.samples.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn every_sound_ends_in_silence() {
        for cue in SoundCue::ALL {
            let clip = render(cue);
            let last = clip.samples.last().copied().unwrap_or(1.0);
            assert!(last.abs() < 1e-3, "{cue:?} ends at {last}");
        }
    }

    #[test]
    fn the_sounds_differ_from_each_other() {
        let clips: Vec<_> = SoundCue::ALL.iter().map(|c| render(*c)).collect();
        for (i, a) in clips.iter().enumerate() {
            for b in &clips[i + 1..] {
                assert_ne!(a.samples, b.samples);
            }
        }
    }
}
