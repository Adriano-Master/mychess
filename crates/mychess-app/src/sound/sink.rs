//! Where the sounds go: the [`SoundSink`] the front-end installs (D2808).

use std::sync::{Arc, Mutex, OnceLock};

use super::bank::Clip;

/// Clips to play one after the other, replacing what is playing.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayRequest {
    /// The clips, in order, shared and untouched.
    pub clips: Vec<Arc<Clip>>,
    /// The gain of the volume, applied by the device thread (never copy the
    /// samples in `update`).
    pub gain: f32,
}

/// The device that plays sounds. Implementations must not block for long and
/// must not panic or report an error when there is no audio device: silence
/// is the answer (D2808).
pub trait SoundSink: Send + Sync {
    /// Plays `request`, cutting the previous one.
    fn play(&self, request: PlayRequest);
}

/// The sink that plays nothing: no audio feature, no device.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl SoundSink for NullSink {
    fn play(&self, _request: PlayRequest) {}
}

/// A sink that records what it was asked to play, for tests.
#[derive(Debug, Default)]
pub struct FakeSink {
    played: Mutex<Vec<PlayRequest>>,
}

impl FakeSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> FakeSink {
        FakeSink::default()
    }

    /// What was played so far.
    #[must_use]
    pub fn played(&self) -> Vec<PlayRequest> {
        self.played.lock().map(|p| p.clone()).unwrap_or_default()
    }
}

impl SoundSink for FakeSink {
    fn play(&self, request: PlayRequest) {
        if let Ok(mut played) = self.played.lock() {
            played.push(request);
        }
    }
}

static SINK: OnceLock<Arc<dyn SoundSink>> = OnceLock::new();

/// Installs the sink of the process. Only the first call counts (the sink
/// lives as long as the program); returns whether this one was installed.
pub fn install_sink(sink: Arc<dyn SoundSink>) -> bool {
    let installed = SINK.set(sink).is_ok();
    if installed {
        // Render the built-in sounds off the `update`, which would pay for
        // it at the first move (the `OnceLock` of `builtin_clip` is shared).
        std::thread::Builder::new()
            .name("mychess-sound-warmup".to_owned())
            .spawn(|| drop(super::builtin_clip(super::SoundCue::Move)))
            .ok();
    }
    installed
}

/// Hands `request` to `sink`, or to the installed one, or drops it when none
/// was installed. This is what the effect does; tests pass their own sink.
pub fn perform_play(request: PlayRequest, sink: Option<&dyn SoundSink>) {
    match sink {
        Some(sink) => sink.play(request),
        None => {
            if let Some(sink) = SINK.get() {
                sink.play(request);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PlayRequest {
        PlayRequest {
            clips: vec![Arc::new(Clip {
                sample_rate: 8_000,
                channels: 1,
                samples: vec![0.5],
            })],
            gain: 1.0,
        }
    }

    #[test]
    fn the_fake_sink_records() {
        let sink = FakeSink::new();
        perform_play(request(), Some(&sink));
        perform_play(request(), Some(&sink));
        assert_eq!(sink.played().len(), 2);
    }

    #[test]
    fn the_null_sink_and_no_sink_do_nothing() {
        NullSink.play(request());
        // No installed sink in this test process: dropped, no panic.
        perform_play(request(), None);
        perform_play(request(), Some(&NullSink));
    }
}
