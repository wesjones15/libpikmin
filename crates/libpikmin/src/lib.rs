//! Game-agnostic reimplementation of Pikmin 1's captains, Pikmin swarm, Onion and carrying,
//! using the projectPiki decomp as a behavioral spec. See `docs/behavior-spec.md`.
//!
//! The sim never touches a game directly: everything goes through [`Host`]. Units are the
//! decomp's (roughly cm); hosts convert.

pub mod anim;
pub mod captain;
pub mod camera;
pub mod carry;
pub mod clock;
pub mod damage;
pub mod event;
pub mod formation;
pub mod growth;
pub mod host;
pub mod items;
pub mod jump;
pub mod math;
pub mod nectar;
pub mod onion;
pub mod onion_menu;
pub mod params;
pub mod pellet_config;
pub mod piki;
pub mod plucker;
pub mod strike;
mod sim;

pub use captain::{Captain, CaptainInput, CaptainPose, LockView};
pub use carry::{Pellet, PelletDesc, PelletId, PelletState};
pub use clock::{FixedClock, TICK_DT, TICK_HZ};
pub use damage::DeathCause;
pub use event::Event;
pub use glam::{Vec2, Vec3};
pub use host::{CreatureFrame, CreatureId, CreaturePart, DamageSource, Host};
pub use nectar::{NectarDrop, NectarId, Puddle, PuddleState};
pub use onion::{Onion, OnionId, Sprout, SproutId, SproutState};
pub use params::Params;
pub use piki::{Happa, KnockPhase, Piki, PikiColor, PikiState};
pub use sim::Sim;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PikiId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CaptainId(pub u32);
