//! Game-agnostic reimplementation of Pikmin 1's captains, Pikmin swarm, Onion and carrying,
//! using the projectPiki decomp as a behavioral spec. See `docs/behavior-spec.md`.
//!
//! The sim never touches a game directly: everything goes through [`Host`]. Units are the
//! decomp's (roughly cm); hosts convert.

pub mod captain;
pub mod clock;
pub mod event;
pub mod formation;
pub mod host;
pub mod math;
pub mod params;
pub mod piki;
mod sim;

pub use captain::{Captain, CaptainInput, CaptainPose};
pub use clock::{FixedClock, TICK_DT, TICK_HZ};
pub use event::Event;
pub use glam::{Vec2, Vec3};
pub use host::{CreatureFrame, CreatureId, CreaturePart, Host};
pub use params::Params;
pub use piki::{Piki, PikiColor, PikiState};
pub use sim::Sim;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PikiId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CaptainId(pub u32);
