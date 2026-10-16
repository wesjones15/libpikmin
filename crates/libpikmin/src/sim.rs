use glam::Vec3;

use crate::captain::{Captain, CaptainInput};
use crate::clock::FixedClock;
use crate::event::Event;
use crate::host::{CreatureFrame, CreatureId};
use crate::math::Rng;
use crate::piki::{AttackMode, Piki, PikiColor, PikiState};
use crate::{CaptainId, Host, Params, PikiId};

/// The whole simulation. Hosts own one, feed it input, and call [`Sim::advance`] every frame.
#[derive(Debug)]
pub struct Sim {
    pub(crate) params: Params,
    clock: FixedClock,
    pub(crate) tick: u64,
    pub(crate) rng: Rng,
    pub(crate) captains: Vec<Captain>,
    pub(crate) pikis: Vec<Piki>,
    pub(crate) creatures: CreatureFrame,
    pub(crate) pellets: Vec<crate::carry::Pellet>,
    /// Opt-in host delivery policy; replicas do not run carry AI.
    pub(crate) ship_part_delivery: u8,
    pub(crate) onions: Vec<crate::onion::Onion>,
    pub(crate) sprouts: Vec<Option<crate::onion::Sprout>>,
    pub(crate) nectar_drops: Vec<crate::nectar::NectarDrop>,
    pub(crate) puddles: Vec<crate::nectar::Puddle>,
    pub(crate) next_nectar: u32,
    pub(crate) events: Vec<Event>,
    pending_shakes: Vec<CreatureId>,
    /// See [`Sim::set_auto_rejoin`].
    pub(crate) auto_rejoin: bool,
    /// See [`Sim::set_shake_protection`], in ticks.
    shake_protection: u64,
    /// See [`Sim::set_rejoin_delay`].
    pub(crate) rejoin_delay: f32,
    /// See [`Sim::set_growth_rate`].
    pub(crate) growth_rate: f32,
    /// See [`Sim::set_sprout_stages`].
    pub(crate) sprout_stage: Option<f32>,
    pub(crate) sprout_flower: Option<f32>,
    /// See [`Sim::set_kill_growth`].
    pub(crate) kill_growth: u32,
    /// See [`Sim::set_dismiss_strike`].
    pub(crate) dismiss_strike: Option<crate::strike::StrikeSpec>,
    /// See [`Sim::set_cursor_homing`].
    pub(crate) cursor_homing: bool,
    /// See [`Sim::set_squad_pace`]: leaf, bud, flower.
    pub(crate) pace: Option<[f32; 3]>,
    /// See [`Sim::set_auto_carry_range`].
    pub(crate) auto_carry_range: f32,
    /// See [`Sim::set_auto_pluck`].
    pub(crate) auto_pluck_after: Option<f32>,
    /// See [`Sim::set_emergency_recovery`]: (seconds, Pikmin).
    pub(crate) recovery: Option<(f32, u32)>,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new(Params::default(), 1)
    }
}

impl Sim {
    pub fn new(params: Params, seed: u64) -> Self {
        Self {
            params,
            clock: FixedClock::default(),
            tick: 0,
            rng: Rng::new(seed),
            captains: Vec::new(),
            pikis: Vec::new(),
            creatures: CreatureFrame::default(),
            pellets: Vec::new(),
            ship_part_delivery: 0,
            onions: Vec::new(),
            sprouts: Vec::new(),
            nectar_drops: Vec::new(),
            puddles: Vec::new(),
            next_nectar: 0,
            events: Vec::new(),
            pending_shakes: Vec::new(),
            auto_rejoin: false,
            shake_protection: 0,
            rejoin_delay: 0.5,
            growth_rate: 1.0,
            sprout_stage: None,
            sprout_flower: None,
            kill_growth: 0,
            dismiss_strike: None,
            cursor_homing: false,
            pace: None,
            auto_carry_range: 0.0,
            auto_pluck_after: None,
            recovery: None,
        }
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    pub fn params_mut(&mut self) -> &mut Params {
        &mut self.params
    }

    pub fn add_captain(&mut self, pos: Vec3) -> CaptainId {
        let id = CaptainId(self.captains.len() as u32);
        self.captains
            .push(Captain::new(id, pos, self.params.navi.health));
        id
    }

    /// Spawns an idle Pikmin. Returns `None` at the field limit (`aiconst` p15, 100 on the disc).
    pub fn spawn_piki(&mut self, color: PikiColor, pos: Vec3) -> Option<PikiId> {
        let alive = self.pikis.iter().filter(|p| p.on_field()).count();
        if alive >= self.params.ai.max_pikis_on_field.max(0) as usize {
            return None;
        }
        let size = self.rng.range(
            self.params.piki.min_piki_size,
            self.params.piki.max_piki_size,
        );
        let id = PikiId(self.pikis.len() as u32);
        self.pikis.push(Piki {
            id,
            color,
            happa: Default::default(),
            pos,
            vel: Vec3::ZERO,
            target_vel: Vec3::ZERO,
            face: 0.0,
            size,
            on_ground: false,
            jump: None,
            jump_queue: Default::default(),
            captain: None,
            state: PikiState::Idle,
            health: self.params.piki.piki_max_health,
            blooms: 0,
            dismissed: false,
            idle_time: 0.0,
            latched_at: 0,
            guided_throw: None,
            hit_log: [None; crate::growth::HIT_LOG],
            hit_next: 0,
            kill_credit: 0,
            frozen: 0.0,
        });
        Some(id)
    }

    /// Spawns a Pikmin straight into a captain's squad.
    pub fn spawn_piki_in_squad(
        &mut self,
        color: PikiColor,
        pos: Vec3,
        captain: CaptainId,
    ) -> Option<PikiId> {
        let id = self.spawn_piki(color, pos)?;
        self.join_squad(id, captain);
        Some(id)
    }

    /// Sets a captain's controls. Button presses are latched until the next tick, so a press
    /// shorter than one tick still registers.
    pub fn set_input(&mut self, captain: CaptainId, input: CaptainInput) {
        let c = &mut self.captains[captain.0 as usize];
        let old = c.input;
        c.pressed.whistle |= input.whistle && !old.whistle;
        c.pressed.throw |= input.throw && !old.throw;
        c.pressed.dismiss |= input.dismiss && !old.dismiss;
        c.pressed.cycle |= input.cycle && !old.cycle;
        c.pressed.jump |= input.jump && !old.jump;
        c.input = input;
    }

    /// Separated Pikmin rejoin their captain by themselves (not Pikmin 1, where they wait to
    /// be whistled); dismissed ones still wait. Off by default.
    pub fn set_auto_rejoin(&mut self, on: bool) {
        self.auto_rejoin = on;
    }

    /// Scales how far `captain` can throw (see [`Captain::reach_scale`]); clamped to 0.1..10.
    pub fn set_captain_reach(&mut self, captain: CaptainId, scale: f32) {
        let scale = if scale.is_finite() { scale.clamp(0.1, 10.0) } else { 1.0 };
        self.captains[captain.0 as usize].reach_scale = scale;
    }

    /// Scales `captain`'s run speed (see [`Captain::speed_scale`]); clamped to 0.1..10.
    pub fn set_captain_speed(&mut self, captain: CaptainId, scale: f32) {
        let scale = if scale.is_finite() { scale.clamp(0.1, 10.0) } else { 1.0 };
        self.captains[captain.0 as usize].speed_scale = scale;
    }

    /// Pikmin latched for less than `seconds` hold on through a shake-off; ones latched
    /// longer fly off. A protected Pikmin isn't shaken off later: only the next shake-off
    /// after its protection runs out can. Not Pikmin 1, where a flick takes every Pikmin
    /// (`InteractFlick`, interactBattle.cpp); for hosts whose enemies attack far more often
    /// than Pikmin 1's flick. 0 (the default) protects none.
    pub fn set_shake_protection(&mut self, seconds: f32) {
        let s = if seconds.is_finite() { seconds.clamp(0.0, 60.0) } else { 0.0 };
        self.shake_protection = (s / crate::TICK_DT).round() as u64;
    }

    /// The creature shakes off the Pikmin latched onto it (call when its shake attack lands),
    /// except those still protected (see [`Sim::set_shake_protection`]).
    pub fn shake_off(&mut self, creature: CreatureId) {
        self.pending_shakes.push(creature);
    }

    /// Feeds real frame time into the fixed-step clock and runs however many 30 Hz ticks are due.
    pub fn advance(&mut self, host: &mut dyn Host, dt: f32) -> u32 {
        let steps = self.clock.accumulate(dt);
        for _ in 0..steps {
            self.step(host);
        }
        steps
    }

    /// Runs exactly one tick.
    pub fn step(&mut self, host: &mut dyn Host) {
        let dt = crate::TICK_DT;
        host.creatures(&mut self.creatures);

        for creature in std::mem::take(&mut self.pending_shakes) {
            for pi in 0..self.pikis.len() {
                let p = &self.pikis[pi];
                if p.state.latched_to() == Some(creature)
                    && self.tick.saturating_sub(p.latched_at) >= self.shake_protection
                {
                    self.knock_off(pi, creature);
                }
            }
        }

        for ci in 0..self.captains.len() {
            self.update_captain(ci, host, dt);
        }
        self.update_pellets(host, dt);
        self.update_nectar(host, dt);
        self.nectar_contacts();
        for pi in 0..self.pikis.len() {
            self.update_piki(pi, host, dt);
        }
        self.update_onions(dt);
        self.update_onion_exits(dt);
        self.update_sprouts(host, dt);
        self.update_auto_pluck(dt);
        self.update_recovery(dt);
        self.rescue_fallen(host);
        self.separate();
        self.tick += 1;
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Sets the tick count, for a replica that presents a server's sim (see [`crate::snapshot`]).
    pub fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    /// Fraction of the way to the next tick, for render interpolation.
    pub fn alpha(&self) -> f32 {
        self.clock.alpha()
    }

    pub fn captains(&self) -> &[Captain] {
        &self.captains
    }

    pub fn captain(&self, id: CaptainId) -> &Captain {
        &self.captains[id.0 as usize]
    }

    pub fn pikis(&self) -> &[Piki] {
        &self.pikis
    }

    pub fn piki(&self, id: PikiId) -> &Piki {
        &self.pikis[id.0 as usize]
    }

    /// Direct access, e.g. for a host to set a Pikmin's leaf stage. Changing `state` here
    /// bypasses the sim's bookkeeping (squads, carry slots); use the sim's calls for that.
    pub fn piki_mut(&mut self, id: PikiId) -> &mut Piki {
        &mut self.pikis[id.0 as usize]
    }

    /// The creature snapshot from the last tick.
    pub fn creatures(&self) -> &CreatureFrame {
        &self.creatures
    }

    pub fn drain_events(&mut self) -> std::vec::Drain<'_, Event> {
        self.events.drain(..)
    }

    /// Pushes overlapping bodies apart on the ground plane. Stands in for the original's
    /// creature-vs-creature collision response (`Creature::collisionCheck` with inverse masses:
    /// captains and creatures are effectively immovable to Pikmin).
    fn separate(&mut self) {
        let navi_r = self.params.navi.body_collision_radius;
        let grounded = |p: &Piki| p.jump.is_none() && match p.state {
            PikiState::Idle
            | PikiState::Squad { .. }
            | PikiState::GoHang { .. }
            | PikiState::Knocked { .. } => true,
            PikiState::Attack(a) => {
                !matches!(a.mode, AttackMode::Latched { .. } | AttackMode::Jumping)
            }
            PikiState::Carry(c) => matches!(c.phase, crate::carry::CarryPhase::Go { .. }),
            _ => false,
        };

        let n = self.pikis.len();
        for i in 0..n {
            if !grounded(&self.pikis[i]) {
                continue;
            }
            for j in (i + 1)..n {
                if !grounded(&self.pikis[j]) {
                    continue;
                }
                let (a, b) = (self.pikis[i].pos, self.pikis[j].pos);
                let d = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
                let min = self.pikis[i].size + self.pikis[j].size;
                let len = d.length();
                if len < min && (a.y - b.y).abs() < min {
                    let push = if len > 1e-4 { d / len } else { Vec3::X } * ((min - len) * 0.5);
                    self.pikis[i].pos -= push;
                    self.pikis[j].pos += push;
                }
            }
        }

        for i in 0..n {
            if !grounded(&self.pikis[i]) {
                continue;
            }
            let p = &mut self.pikis[i];
            // A Pikmin walking to the captain's hand has to reach him.
            if !matches!(p.state, PikiState::GoHang { .. }) {
                for c in &self.captains {
                    // A captain above a jump pad must not block followers on the floor below.
                    if (p.pos.y - c.pos.y).abs() < navi_r + p.size {
                        push_out(&mut p.pos, c.pos, navi_r + p.size);
                    }
                }
            }
            for c in &self.creatures.creatures {
                if !c.alive {
                    continue;
                }
                for part in self.creatures.parts_of(c) {
                    if (p.pos.y - part.centre.y).abs() < part.radius + p.size {
                        push_out(&mut p.pos, part.centre, part.radius + p.size);
                    }
                }
            }
            // Pellets and corpses on the ground are solid too.
            for pel in &self.pellets {
                // Docking slots lie on the pellet radius, inside its normal body
                // separation margin. Do not push a volunteer away from its own slot.
                if matches!(p.state, PikiState::Carry(c) if c.pellet == pel.id) { continue; }
                if matches!(
                    pel.state,
                    crate::carry::PelletState::Idle | crate::carry::PelletState::Carried { .. }
                ) {
                    push_out(&mut p.pos, pel.pos, pel.desc.radius + p.size);
                }
            }
        }

        for c in &mut self.captains {
            for cr in &self.creatures.creatures {
                if !cr.alive {
                    continue;
                }
                for part in self.creatures.parts_of(cr) {
                    if (c.pos.y - part.centre.y).abs() < part.radius + navi_r {
                        push_out(&mut c.pos, part.centre, part.radius + navi_r);
                    }
                }
            }
        }
    }
}

fn push_out(pos: &mut Vec3, from: Vec3, min: f32) {
    let d = Vec3::new(pos.x - from.x, 0.0, pos.z - from.z);
    let len = d.length();
    if len < min {
        let dir = if len > 1e-4 { d / len } else { Vec3::X };
        *pos += dir * (min - len);
    }
}

/// One tick of creature motion: `Creature::moveVelocity` easing on XZ, gravity, the move
/// itself against walls, then the ground snap.
///
/// The move is split the way `MapMgr::traceMove` (mapMgr.cpp) splits it: the step count
/// doubles until each step travels less than the body's `radius`, so nothing tunnels through
/// a wall, and after each step the host pushes the body's sphere out of walls
/// ([`Host::walls`]). Whatever velocity pointed into the wall is dropped.
#[allow(clippy::too_many_arguments)]
pub(crate) fn integrate(
    host: &dyn Host,
    pos: &mut Vec3,
    vel: &mut Vec3,
    target: Vec3,
    on_ground: &mut bool,
    radius: f32,
    accel: f32,
    gravity: f32,
    dt: f32,
) {
    // vel += (target - vel) * dt / acceleration  (creature.cpp)
    let k = (dt / accel.max(1e-4)).min(1.0);
    vel.x += (target.x - vel.x) * k;
    vel.z += (target.z - vel.z) * k;
    vel.y -= gravity * dt;

    let radius = radius.max(1.0);
    let start_y = pos.y;
    let mut steps = 1u32;
    let mut travel = vel.length() * dt;
    while steps < 64 && travel >= radius {
        steps *= 2;
        travel *= 0.5;
    }
    let sub = dt / steps as f32;
    for _ in 0..steps {
        *pos += *vel * sub;
        let centre = *pos + Vec3::Y * radius;
        let moved = host.walls(centre, radius) - centre;
        let push = Vec3::new(moved.x, 0.0, moved.z);
        if push.length_squared() > 1e-8 {
            *pos += push;
            let n = push.normalize();
            let into = vel.dot(n);
            if into < 0.0 {
                *vel -= n * into;
            }
        }
    }

    // Probe from the body's step height above the higher of where it started and ended the
    // tick: walking uphill the feet end up under the slope, and falling fast they can pass a
    // floor within one tick. Anything taller than the step is a wall for `Host::walls`, whose
    // sphere sits `radius` up.
    let probe = Vec3::new(pos.x, pos.y.max(start_y) + radius, pos.z);
    match host.ground_height(probe) {
        Some(g) if pos.y <= g => {
            pos.y = g;
            vel.y = vel.y.max(0.0);
            *on_ground = true;
        }
        // Stay glued when walking down gentle slopes.
        Some(g) if *on_ground && vel.y <= 0.0 && pos.y - g < 4.0 => {
            pos.y = g;
            vel.y = 0.0;
        }
        _ => *on_ground = false,
    }
}
