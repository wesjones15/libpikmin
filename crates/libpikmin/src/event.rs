use glam::Vec3;

use crate::carry::PelletId;
use crate::host::CreatureId;
use crate::onion::{OnionId, SproutId};
use crate::piki::{Happa, PikiColor};
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
    /// Shaken or flicked off; `creature` is set when it was shaken off a creature.
    KnockedOff {
        piki: PikiId,
        creature: Option<CreatureId>,
    },
    PikiHurt {
        piki: PikiId,
        damage: f32,
    },
    /// Started dying. It plays a death animation and fades, unless crushed or eaten (fades at once).
    PikiDied {
        piki: PikiId,
        cause: crate::damage::DeathCause,
    },
    Swallowed {
        piki: PikiId,
        creature: CreatureId,
    },
    SpitOut {
        piki: PikiId,
    },
    CaptainHurt {
        captain: CaptainId,
        damage: f32,
        health: f32,
    },
    /// Health ran out or the host put him down. His squad was released.
    CaptainDown {
        captain: CaptainId,
    },
    Dismissed {
        captain: CaptainId,
    },
    /// Enough carriers lifted it; it's heading for `onion`.
    PelletLifted {
        pellet: PelletId,
        onion: OnionId,
    },
    /// Carriers fell below the minimum and put it down.
    PelletDropped {
        pellet: PelletId,
    },
    /// Reached the Onion; carriers let go and it's about to be sucked up.
    PelletArrived {
        pellet: PelletId,
        onion: OnionId,
    },
    /// Sucked into the Onion. The host should remove its object (corpse).
    PelletDelivered {
        pellet: PelletId,
        onion: OnionId,
        color: PikiColor,
        seeds: i32,
    },
    SeedLaunched {
        onion: OnionId,
        sprout: SproutId,
        color: PikiColor,
    },
    /// The field was full (`aiconst` p15), so the seed stayed in the Onion.
    SeedStored {
        onion: OnionId,
        color: PikiColor,
    },
    SproutPlanted {
        sprout: SproutId,
    },
    SproutGrew {
        sprout: SproutId,
        happa: Happa,
    },
    SproutWilted {
        sprout: SproutId,
    },
    Plucked {
        captain: CaptainId,
        piki: PikiId,
        sprout: SproutId,
    },
}
