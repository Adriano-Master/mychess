//! Tactical detectors: pure functions over positions (spec M6-2, D3260-D3269).
//!
//! Everything here is stateless and deterministic. The static detectors (pins,
//! skewers, forks, double attacks, discovered attacks, structure) cost at most
//! "pieces x ray length" and never search; the only searching detectors are
//! [`mate_in_played`] and [`mate_in_available`], which are bounded by a node
//! budget and built on [`crate::mate`].
//!
//! Conventions:
//! - "attack" is geometric ([`attacks`]); a *legal* attack ([`legal_attack`])
//!   also honours the pin of the attacker against its own king, and the king
//!   cannot attack a square guarded by the other side;
//! - [`see`] is the static exchange evaluation (pins after the first capture
//!   and en passant are ignored: a documented limit);
//! - no detector calls [`crate::Position::legal_moves`] for the side that is not to
//!   move, so all of them work for both colours in any position.
//!
//! ```
//! use mychess_core::{Position, Square, Color};
//! use mychess_core::tactics;
//!
//! let pos = Position::from_fen("r3k3/2N5/8/8/8/8/8/4K3 b - - 1 1")?;
//! let forks = tactics::forks(&pos, Color::White);
//! assert_eq!(forks.len(), 1);
//! assert_eq!(forks[0].forker, Square::C7);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod attack;
mod discovered;
mod forks;
mod lines;
mod matein;
mod mirror;
mod structure;

pub use attack::{
    KING_VALUE, attacks, defenders, legal_attack, material_value, safe, see, value, worth,
};
pub use discovered::{DiscoveredAttack, checkers, discovered_attacks, discovered_check};
pub use forks::{Fork, double_attack, fork_victims, forks, threatened_squares};
pub use lines::{Pin, Skewer, pins, skewers};
pub use matein::{
    MateQuery, MateVerdict, mate_in_available, mate_in_available_with, mate_in_played,
    mate_in_played_with,
};
pub use mirror::{mirror, mirror_move, mirror_square};
pub use structure::{
    GamePhase, PHASE_ENDGAME_MAX, PHASE_OPENING_MIN, bishop_pair, doubled_pawns, exposed_king,
    isolated_pawns, king_shield, material_balance, non_pawn_material, passed_pawns, phase,
};
