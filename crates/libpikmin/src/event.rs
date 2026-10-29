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
    /// An Onion (or the ship) is back on the ground after [`crate::Sim::onion_land`].
    OnionLanded { onion: OnionId },
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
    /// A part reached its receiver (`UfoItem::suckMe`). Normally `ship` is the ship
    /// and `total` its fitted count. With either host receipt policy, `ship` is the
    /// receiver and `total` is zero: the host must accept the receipt and fit its ships.
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
    /// Host extension: an Onion sent `count` Pikmin to its owner, who had none left
    /// ([`crate::Sim::set_emergency_recovery`]).
    EmergencyPikmin {
        onion: OnionId,
        captain: CaptainId,
        count: u32,
    },
    /// Host extension: a Pikmin came out of `onion` to pluck sprouts nobody pulled
    /// ([`crate::Sim::set_auto_pluck`]).
    PluckerOut { piki: PikiId, onion: OnionId },
    /// The plucker pulled a sprout; `plucked` joins `captain` (if any).
    PluckerPulled {
        plucker: PikiId,
        plucked: PikiId,
        captain: Option<CaptainId>,
    },
    /// Host extension: `captain` jumped; `air` for a mid-air jump (Hopoo Feather).
    CaptainJumped { captain: CaptainId, air: bool },
    /// Host extension: `captain`'s stomp hit the ground at `pos` after falling `drop` units.
    CaptainStomped {
        captain: CaptainId,
        pos: Vec3,
        drop: f32,
    },
    /// Host extension: a Pikmin grew to `happa` from kills ([`crate::Sim::set_kill_growth`]).
    PikiGrew { piki: PikiId, happa: crate::Happa },
    /// Host extension: `captain` began a dismiss strike ([`crate::Sim::set_dismiss_strike`]).
    StrikeStarted { captain: CaptainId },
    /// Host extension: `captain` touched down after `drop` units of falling from his highest
    /// point; `stomp` for the landing of a stomp (which has its own event).
    CaptainLanded {
        captain: CaptainId,
        drop: f32,
        stomp: bool,
    },
    /// Host extension: one of `captain`'s Pikmin stomped down with him and landed at `pos`.
    PikiStomped {
        piki: PikiId,
        captain: CaptainId,
        pos: Vec3,
    },
    /// Host extension: one of `captain`'s Pikmin took off to repeat his jump-pad throw.
    PikiLaunched { piki: PikiId, captain: CaptainId },
    /// A thrown Purple came down on something (`PikiHipDropState::collisionCallback`) or on the ground, at `pos`.
    Pounded { piki: PikiId, pos: Vec3 },
    /// The pound brought a flying `creature` down (`InteractEarthquake` reaches the flying too): the host
    /// puts it on the ground for a while.
    Grounded { creature: CreatureId, piki: PikiId },
    /// A White Pikmin was eaten: `creature` swallowed poison (`EnemyBase::eatWhitePikminCallBack`).
    PoisonedEater { piki: PikiId, creature: CreatureId },
    /// A thrown Pikmin came down in a Candypop Bud ([`crate::bulb`]) and was swallowed; `colour` is what it was.
    BulbSwallowed { piki: PikiId, bulb: crate::bulb::BulbId, colour: PikiColor, pos: Vec3 },
    /// A Candypop Bud spat `count` sprouts of `colour` from `pos`.
    BulbShot { bulb: crate::bulb::BulbId, colour: PikiColor, count: u32, pos: Vec3 },
    /// The Pikmin latched to a flying `creature` weigh `weight` (in ordinary Pikmin, a Purple five), as much as it was
    /// told it needs to be brought down ([`crate::weigh`]). `piki` is the heaviest of them. Sent every half second.
    WeighedDown { creature: CreatureId, piki: PikiId, weight: u32 },
}

impl Event {
    /// Entity references whose storage must survive deferred sound/network/host delivery.
    pub fn piki_ids(&self) -> impl Iterator<Item = PikiId> {
        use Event::*;
        let ids = match *self {
            PluckerPulled { plucker, plucked, .. } => [Some(plucker), Some(plucked)],
            PikiEnteredOnion { piki, .. } | PikiLeftOnion { piki, .. }
            | PikiDrinking { piki } | Bloomed { piki } | Recruited { piki, .. }
            | Thrown { piki, .. } | Landed { piki } | Latched { piki, .. }
            | Hit { piki, .. } | KnockedOff { piki, .. } | PikiHurt { piki, .. }
            | CarrierGrabbed { piki, .. } | Deflowered { piki } | PikiGone { piki, .. }
            | PikiDied { piki, .. } | Swallowed { piki, .. } | SpitOut { piki }
            | Plucked { piki, .. } | PluckerOut { piki, .. } | PikiGrew { piki, .. }
            | PikiStomped { piki, .. } | PikiLaunched { piki, .. } | Pounded { piki, .. }
            | Grounded { piki, .. } | PoisonedEater { piki, .. } | BulbSwallowed { piki, .. }
            | WeighedDown { piki, .. } => [Some(piki), None],
            _ => [None, None],
        };
        ids.into_iter().flatten()
    }
}
