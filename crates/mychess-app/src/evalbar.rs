//! The evaluation bar, the arrows of the engine and the MultiPV traffic
//! lights: the pure part (spec M4-5).
//!
//! Everything here is a function from analysis data and preferences to plain
//! values the front-end draws without interpreting a score (D451):
//!
//! - [`prefs`]: [`EngineViewPrefs`] and its parts (D455);
//! - [`mapping`]: [`EvalValue`], [`bar_fill`] and [`bar_ticks`] (D453);
//! - [`view`]: [`EvalBarView`] and [`EvalSource`] (D452, D454);
//! - [`arrows`]: [`engine_arrows`] (D456);
//! - [`lights`]: [`traffic_light_color`] and [`traffic_lights`] (D458);
//! - `message`: [`EvalBarMessage`].
//!
//! The state and the choice of the source live in `state/evalbar_flow.rs`.

mod arrows;
mod lights;
mod mapping;
mod message;
mod prefs;
mod view;

pub use arrows::{ArrowKind, EngineArrow, engine_arrows, review_arrows};
pub use lights::{Rgb, TrafficLight, traffic_light_color, traffic_lights};
pub use mapping::{EvalValue, TerminalResult, bar_fill, bar_ticks};
pub use message::EvalBarMessage;
pub use prefs::{
    ArrowMode, ArrowPrefs, EVAL_SCALE_RANGE, EngineViewPrefs, EvalBarPrefs, SEQUENCE_LEN_RANGE,
    SequenceSide, TrafficLightPrefs,
};
pub use view::{EvalBarView, EvalSource};
