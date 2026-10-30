//! Helpers shared by the unit tests.

use mychess_app::{Effect, Message};

/// Runs an effect of the application as the shell would (on a runtime) and
/// returns the answer.
pub(crate) fn run_effect(effect: Effect) -> Message {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a current-thread runtime");
    runtime.block_on(mychess_app::perform(effect))
}
