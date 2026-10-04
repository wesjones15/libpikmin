use glam::Vec3;

use crate::host::CreatureId;
use crate::{CaptainId, PikiId};

/// Things that happened during a tick, for hosts to play sounds, effects and animations.
/// Drain with [`crate::Sim::drain_events`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A whistle called Pikmin this tick.
    Whistle {
        captain: CaptainId,
        centre: Vec3,
        radius: f32,
    },
    /// A Pikmin joined `captain`'s squad. `stolen_from` is set when it left another captain's squad.
    Recruited {
        piki: PikiId,
        captain: CaptainId,
        stolen_from: Option<CaptainId>,
    },
    Thrown {
        piki: PikiId,
        captain: CaptainId,
    },
    /// A thrown Pikmin touched the ground without hitting a creature.
    Landed {
        piki: PikiId,
    },
    Latched {
        piki: PikiId,
        creature: CreatureId,
    },
    /// The host accepted a hit.
    Hit {
        piki: PikiId,
        creature: CreatureId,
        damage: f32,
        latched: bool,
    },
    KnockedOff {
        piki: PikiId,
        creature: CreatureId,
    },
    Dismissed {
        captain: CaptainId,
    },
}
