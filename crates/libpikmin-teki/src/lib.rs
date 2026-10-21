//! Pikmin 1's enemies ("teki") as a game-agnostic simulation: the strategies of the decomp's `Tai*`
//! files ported as state machines over the sim's Pikmin, with the numbers from the disc.
//!
//! The core has no engine types. A host spawns enemies, hands them the world each tick (where its
//! players are, how its enemies are hurt) and applies what they do (damage to its players, new
//! enemies, effects). Its own bodies may carry the enemies' health and hurt boxes.

pub mod info;
pub mod anim;
