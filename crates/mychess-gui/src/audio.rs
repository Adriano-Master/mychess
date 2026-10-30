//! The sound output of the application (spec M8-11, D2808, ADR 0013).
//!
//! [`install`] hands `mychess-app` the sink that plays the sounds. With the
//! `audio` feature it is a [`RodioSink`]: a thread that owns the audio
//! device (the `cpal` stream is not `Send` on every platform), opens it the
//! first time something plays and cuts the previous sound when a new one
//! arrives. Without the feature, or when there is no audio device, nothing
//! is installed or nothing is heard: no error, no panic.

#[cfg(feature = "audio")]
pub use real::RodioSink;

/// Installs the sink of the process. Call once, at startup.
pub fn install() {
    #[cfg(feature = "audio")]
    {
        if let Some(sink) = RodioSink::spawn() {
            mychess_app::sound::install_sink(std::sync::Arc::new(sink));
        } else {
            tracing::warn!("the audio thread could not start; sounds are off");
        }
    }
}

#[cfg(feature = "audio")]
mod real {
    use std::num::NonZero;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
    use std::time::{Duration, Instant};

    use mychess_app::sound::{PlayRequest, SoundSink};
    use rodio::buffer::SamplesBuffer;
    use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};

    /// How long to wait before trying to open a failed device again.
    const RETRY_AFTER: Duration = Duration::from_secs(10);

    /// Requests waiting for the audio thread; a burst beyond this (an
    /// autoplay or a stuck device) is dropped: only the latest sound matters.
    const QUEUE: usize = 8;

    /// Plays the clips on the default audio device of the system.
    #[derive(Debug)]
    pub struct RodioSink {
        tx: SyncSender<PlayRequest>,
    }

    impl RodioSink {
        /// Starts the audio thread (the device is opened on the first
        /// sound). `None` when the thread cannot be created.
        #[must_use]
        pub fn spawn() -> Option<RodioSink> {
            let (tx, rx) = sync_channel::<PlayRequest>(QUEUE);
            std::thread::Builder::new()
                .name("mychess-audio".to_owned())
                .spawn(move || {
                    let mut device: Option<MixerDeviceSink> = None;
                    // After a failure the device is not tried again for a
                    // while: a machine without a sound card would pay the
                    // cost of the attempt at every sound.
                    let mut retry_at: Option<Instant> = None;
                    let mut current: Option<Player> = None;
                    // Set by the callback of the stream when the device is
                    // lost (unplugged, changed): it is opened again.
                    let lost = Arc::new(AtomicBool::new(false));
                    while let Ok(mut request) = rx.recv() {
                        // Only the newest of the waiting requests is heard.
                        while let Ok(newer) = rx.try_recv() {
                            request = newer;
                        }
                        if lost.swap(false, Ordering::Relaxed) {
                            tracing::warn!("the audio device was lost; opening it again");
                            current = None;
                            if let Some(mut old) = device.take() {
                                old.log_on_drop(false);
                            }
                            retry_at = None;
                        }
                        // A newer sound cuts the one that is playing.
                        if let Some(old) = current.take() {
                            old.stop();
                        }
                        if device.is_none() && retry_at.is_none_or(|t| Instant::now() >= t) {
                            let flag = Arc::clone(&lost);
                            let opened = DeviceSinkBuilder::from_default_device().and_then(|b| {
                                b.with_error_callback(move |_| flag.store(true, Ordering::Relaxed))
                                    .open_stream()
                            });
                            match opened {
                                Ok(opened) => device = Some(opened),
                                Err(error) => {
                                    // A machine without sound card is not
                                    // an error to the user: log only once.
                                    if retry_at.is_none() {
                                        tracing::warn!(%error, "no audio device; sounds are off");
                                    }
                                    retry_at = Some(Instant::now() + RETRY_AFTER);
                                    continue;
                                }
                            }
                        }
                        let Some(device) = device.as_ref() else {
                            continue;
                        };
                        let gain = request.gain;
                        let player = Player::connect_new(device.mixer());
                        for clip in &request.clips {
                            let (Some(channels), Some(rate)) =
                                (NonZero::new(clip.channels), NonZero::new(clip.sample_rate))
                            else {
                                continue;
                            };
                            // The gain is applied here, on the audio thread; the
                            // copy of the samples is made here too.
                            player.append(
                                SamplesBuffer::new(channels, rate, clip.samples.clone())
                                    .amplify(gain),
                            );
                        }
                        current = Some(player);
                    }
                })
                .ok()?;
            Some(RodioSink { tx })
        }
    }

    impl SoundSink for RodioSink {
        fn play(&self, request: PlayRequest) {
            // Full queue or a thread gone (it panicked): silence.
            if let Err(TrySendError::Full(_)) = self.tx.try_send(request) {
                tracing::debug!("the audio queue is full; a sound was dropped");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn installing_twice_is_harmless() {
        super::install();
        super::install();
    }
}
