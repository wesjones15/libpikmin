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
    /// A Pikmin climbed into an Onion and is stored.
    PikiEnteredOnion { piki: PikiId, onion: OnionId },
    /// A Pikmin started sliding out of an Onion (`SE_PIKI_OUTHOME`).
    PikiLeftOnion { piki: PikiId, onion: OnionId },
    /// A nectar drop hit the ground; its puddle appears shortly.
    NectarSplash { pos: Vec3 },
    /// A nectar puddle finished draining.
    NectarGone,
    /// A Pikmin started drinking nectar (`SEF_PIKI_DRINK`).
    PikiDrinking { piki: PikiId },
    /// A Pikmin's head became a flower from nectar (`SEF_PIKI_GROW4`).
    Bloomed { piki: PikiId },
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
    /// A ship part reached the ship (`UfoItem::suckMe`); `total` parts are on board now.
    ShipPartCollected {
        pellet: PelletId,
        ship: OnionId,
        part: u32,
        total: u32,
    },
    /// A carrier reached its slot and took hold (`ActTransport::gotoLiftPos`: `SEF_PIKI_HANG`,
    /// and the pellet's `SE_LIFT_TRY` loop if it isn't lifted yet).
    CarrierGrabbed {
        piki: PikiId,
        pellet: PelletId,
    },
    /// Got up from being knocked down and lost a leaf stage (`PikiFlickState`).
    Deflowered {
        piki: PikiId,
    },
    /// Removed after the death shrink (`Piki::doKill`): the ghost rises from `pos` on the
    /// ground (`EFF_Piki_DeadSoul`) and `SE_PIKI_DEAD` plays.
    PikiGone {
        piki: PikiId,
        pos: Vec3,
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
    /// A captain's punch landed (`NaviAttackState`).
    Punched {
        captain: CaptainId,
        creature: CreatureId,
        damage: f32,
    },
    /// Enough carriers lifted it; it's heading for `onion`.
    PelletLifted {
        pellet: PelletId,
        onion: OnionId,
    },
    /// Pikmin joined or left on the way and the majority colour now wants another Onion.
    PelletRerouted {
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
