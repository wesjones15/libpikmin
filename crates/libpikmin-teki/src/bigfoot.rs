//! Raging Long Legs (`BigFoot::Obj`): the Long Legs' machine ([`crate::longlegs`]) with a second wind. Stay, Land, Wait
//! (five seconds) and Walk as Beady Long Legs does; once it has shaken its Pikmin off (`Flick`) it rages (`mIsEnraged`):
//! the next walk is shorter (`fp21` seconds) and wild, each round of steps aimed a `fp17` away at a random angle from
//! where it faces, with its faster second set of leg numbers (`fp11`..`fp16`); the rage ends with that walk.
//! See [`crate::longlegs`] for the states, registers and host adaptations.

use crate::longlegs::{self, Variant};
use crate::tai::Strategy;

pub fn strategy() -> Strategy {
    longlegs::strategy(Variant::BigFoot)
}
