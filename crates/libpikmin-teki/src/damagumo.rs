//! Beady Long Legs (`Damagumo::Obj`): the Long Legs' machine ([`crate::longlegs`]) with its own numbers: Stay, Land, Wait
//! (1.75 to 3.5 seconds), Walk (3.25 to 6.5 seconds) and Flick, going after the nearest Pikmin or player, crushing what its
//! feet come down on. See [`crate::longlegs`] for the states, registers and host adaptations.

use crate::longlegs::{self, Variant};
use crate::tai::Strategy;

pub fn strategy() -> Strategy {
    longlegs::strategy(Variant::Damagumo)
}
