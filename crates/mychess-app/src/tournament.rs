//! Tournaments between engines (spec M8-2, phase A: the core without GUI).

pub mod adjudicate;
pub mod config;
pub mod driver;
mod effects;
pub mod export;
pub mod game;
pub mod message;
pub mod openings;
pub mod rating;
pub mod record;
pub mod resources;
pub mod runner;
pub mod schedule;
pub mod standings;
pub mod store;
pub mod tablebase;
pub mod timecheck;

pub use adjudicate::*;
pub use config::*;
pub use driver::*;
pub(crate) use effects::perform_tournament;
pub use export::*;
pub use game::*;
pub use message::*;
pub use openings::*;
pub use rating::*;
pub use record::*;
pub use resources::*;
pub use runner::*;
pub use schedule::*;
pub use standings::*;
pub use store::*;
pub use tablebase::*;
pub use timecheck::*;
