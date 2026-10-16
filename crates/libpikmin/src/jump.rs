//! Host extension: voluntary jumps and repeating a captain's takeoff points.
//! This is new gameplay. Geyser presentation comes from NaviGeyzerState::init
//! (naviState.cpp); Pikmin use PikiFlyingState's airborne motion (pikiState.cpp).

use crate::carry::CarryPhase;
use crate::piki::AttackMode;
use crate::{Host, PikiState, Sim, Vec3};

/// Sim units, about 1.6 metres at the default RoR2 scale.
pub const JUMP_HEIGHT: f32 = 40.0;

/// Most mid-air jumps a follower repeats from one takeoff.
pub const MAX_AIR_JUMPS: usize = 4;
/// How fast a stomp drives down (H3AD-5T v2), sim units per second.
pub const STOMP_SPEED: f32 = 700.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct Jump {
    pub elapsed: f32,
    pub horizontal: Vec3,
    /// Host jump pads supply their own gravity; voluntary jumps use disc tuning.
    pub gravity: Option<f32>,
    /// A follower's copy of its captain's mid-air jumps (seconds into the jump), and how many
    /// it has made.
    pub air: [f32; MAX_AIR_JUMPS],
    pub air_count: u8,
    pub air_done: u8,
    /// The captain's takeoff this jump repeats (its tick).
    pub cue_tick: Option<u64>,
    /// Driving down in a stomp.
    pub stomping: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct JumpCue {
    pub origin: Vec3,
    pub horizontal: Vec3,
    pub launch: Option<(f32, f32)>,
    pub tick: u64,
    /// The captain's mid-air jumps on this takeoff, seconds after it.
    pub air: [f32; MAX_AIR_JUMPS],
    pub air_count: u8,
}

impl Sim {
    fn jump_speed(&self) -> f32 {
        (2.0 * self.params.ai.gravity.max(1.0) * JUMP_HEIGHT).sqrt()
    }

    pub(crate) fn start_captain_jump(&mut self, ci: usize) {
        let speed = self.jump_speed() * self.captains[ci].jump_scale;
        let c = &mut self.captains[ci];
        c.vel.y = speed;
        c.on_ground = false;
        let origin = c.pos;
        self.record_captain_jump(ci, origin);
    }

    pub(crate) fn record_captain_jump(&mut self, ci: usize, origin: Vec3) {
        let c = &mut self.captains[ci];
        // Use intended movement at takeoff, including a jump from rest while pushing the stick.
        let stick = Vec3::new(
            c.input.stick.x.clamp(-1.0, 1.0),
            0.0,
            c.input.stick.y.clamp(-1.0, 1.0),
        );
        let n = &self.params.navi;
        let aiming = (c.neutral_time >= n.cursor_move_delay_time
            || stick.length() > n.neutral_stick_threshold)
            && stick.length() <= n.cursor_move_stick_threshold;
        let horizontal = if c.input.pose.is_some() {
            c.vel.with_y(0.0)
        } else if aiming {
            Vec3::ZERO
        } else {
            stick * n.run_speed * c.speed_scale
        };
        c.jump = Some(Jump {
            horizontal,
            ..Default::default()
        });
        c.jump_tick = Some(self.tick);
        c.air_jumps_used = 0;
        let captain = c.id;
        let cue = JumpCue {
            origin,
            horizontal,
            launch: None,
            tick: self.tick,
            air: [0.0; MAX_AIR_JUMPS],
            air_count: 0,
        };
        self.queue_squad_jump(ci, cue);
        self.events.push(crate::Event::CaptainJumped { captain, air: false });
    }

    /// A mid-air jump (Hopoo Feather). Squad members in the air with him jump again now;
    /// ones yet to repeat his takeoff will jump again as far into theirs.
    pub(crate) fn start_air_jump(&mut self, ci: usize) {
        let speed = self.jump_speed();
        let c = &mut self.captains[ci];
        c.vel.y = speed * c.jump_scale;
        c.air_jumps_used += 1;
        let at = c.jump.map_or(0.0, |j| j.elapsed);
        let tick = c.jump_tick;
        let captain = c.id;
        let members = c.plate.members.clone();
        for id in members {
            let p = &mut self.pikis[id.0 as usize];
            if !matches!(p.state, PikiState::Squad { .. }) {
                continue;
            }
            if let Some(j) = p.jump.as_mut()
                && !p.on_ground
                && !j.stomping
            {
                p.vel.y = speed;
                // This one's done now: skip it if it was also coming up later.
                if j.air_done < j.air_count {
                    j.air_done += 1;
                }
                continue;
            }
            if let Some(cue) = p.jump_queue.iter_mut().rev().find(|q| Some(q.tick) == tick)
                && (cue.air_count as usize) < MAX_AIR_JUMPS
            {
                cue.air[cue.air_count as usize] = at;
                cue.air_count += 1;
            }
        }
        self.events.push(crate::Event::CaptainJumped { captain, air: true });
    }

    /// H3AD-5T v2: drive straight down; squad members in the air come down with him.
    pub(crate) fn start_stomp(&mut self, ci: usize) {
        let c = &mut self.captains[ci];
        c.stomp = Some(c.pos.y);
        c.vel = Vec3::new(0.0, -STOMP_SPEED, 0.0);
        c.launched = true;
        let members = c.plate.members.clone();
        for id in members {
            let p = &mut self.pikis[id.0 as usize];
            if matches!(p.state, PikiState::Squad { .. })
                && !p.on_ground
                && let Some(j) = p.jump.as_mut()
            {
                j.stomping = true;
                j.horizontal = Vec3::ZERO;
                p.vel = Vec3::new(0.0, -STOMP_SPEED, 0.0);
            }
        }
    }

    /// After his move: keep a stomp driving down, and report it on landing.
    pub(crate) fn update_stomp(&mut self, ci: usize) {
        let c = &mut self.captains[ci];
        let Some(from) = c.stomp else { return };
        if c.on_ground {
            c.stomp = None;
            let (captain, pos) = (c.id, c.pos);
            self.events.push(crate::Event::CaptainStomped { captain, pos, drop: (from - pos.y).max(0.0) });
        } else {
            c.vel.y = c.vel.y.min(-STOMP_SPEED);
        }
    }

    /// Wax Quail: his jump (and the squad's repeat of it) carries `factor` times as far.
    pub fn boost_captain_jump(&mut self, captain: crate::CaptainId, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let Some(c) = self.captains.get_mut(captain.0 as usize) else { return };
        if c.on_ground || c.jump.is_none() {
            return;
        }
        c.vel.x *= factor;
        c.vel.z *= factor;
        c.launched = true;
        if let Some(j) = c.jump.as_mut() {
            j.horizontal *= factor;
        }
        let tick = c.jump_tick;
        let members = c.plate.members.clone();
        for id in members {
            let p = &mut self.pikis[id.0 as usize];
            if let Some(j) = p.jump.as_mut()
                && j.cue_tick.is_some()
                && j.cue_tick == tick
            {
                j.horizontal *= factor;
            }
            for cue in p.jump_queue.iter_mut().filter(|q| Some(q.tick) == tick) {
                cue.horizontal *= factor;
            }
        }
    }

    pub(crate) fn queue_squad_jump(&mut self, ci: usize, cue: JumpCue) {
        let members = self.captains[ci].plate.members.clone();
        let stationary = cue.launch.is_none() && cue.horizontal.length() < 1.0;
        for id in members {
            let pi = id.0 as usize;
            if !matches!(self.pikis[pi].state, PikiState::Squad { .. }) {
                continue;
            }
            if stationary {
                // Standing squad members hop at their own positions, without converging on Olimar.
                if self.pikis[pi].on_ground {
                    self.start_piki_jump(pi, Vec3::ZERO);
                }
            } else {
                let q = &mut self.pikis[pi].jump_queue;
                if q.len() == 16 {
                    q.pop_front();
                }
                q.push_back(cue);
            }
        }
    }

    fn start_piki_jump(&mut self, pi: usize, horizontal: Vec3) {
        let speed = self.jump_speed();
        let p = &mut self.pikis[pi];
        p.vel = horizontal.with_y(speed);
        p.target_vel = horizontal;
        p.on_ground = false;
        p.jump = Some(Jump {
            horizontal,
            ..Default::default()
        });
    }

    fn can_navigation_jump(state: PikiState) -> bool {
        matches!(
            state,
            PikiState::Squad { .. }
                | PikiState::GoHang { .. }
                | PikiState::Attack(crate::piki::Attack {
                    mode: AttackMode::Approach,
                    ..
                })
                | PikiState::Carry(crate::carry::Carry {
                    phase: CarryPhase::Go { .. },
                    ..
                })
        )
    }

    pub(crate) fn update_piki_jump(&mut self, pi: usize, host: &mut dyn Host, dt: f32) -> bool {
        let p = &mut self.pikis[pi];
        // A whistle/dismiss/job change must not leave an old route queued for later.
        if !matches!(p.state, PikiState::Squad { .. }) {
            p.jump_queue.clear();
        }
        while p
            .jump_queue
            .front()
            .is_some_and(|j| self.tick.saturating_sub(j.tick) > 450)
        {
            p.jump_queue.pop_front();
        }
        if p.jump.is_some() && !Self::can_navigation_jump(p.state) && p.state != PikiState::Idle {
            p.jump = None;
        }
        if p.jump.is_none()
            && p.on_ground
            && let Some(cue) = p.jump_queue.front().copied()
            && (p.pos - cue.origin).with_y(0.0).length() <= 12.0
            && (p.pos.y - cue.origin.y).abs() <= p.size.max(8.0)
        {
            p.jump_queue.pop_front();
            self.start_piki_jump(pi, cue.horizontal);
            if let Some(j) = self.pikis[pi].jump.as_mut() {
                j.air = cue.air;
                j.air_count = cue.air_count;
                j.cue_tick = Some(cue.tick);
            }
            if let Some((vertical, gravity)) = cue.launch {
                let p = &mut self.pikis[pi];
                // Repeat the pad from its origin, not from a formation slot beside it.
                p.pos = cue.origin;
                p.vel.y = vertical + 0.5 * gravity * crate::TICK_DT;
                p.jump.as_mut().unwrap().gravity = Some(gravity);
            }
        }
        let Some(jump) = self.pikis[pi].jump else {
            return false;
        };
        // Its captain's mid-air jumps, as far into this one as he made them.
        if !jump.stomping
            && jump.air_done < jump.air_count
            && jump.elapsed >= jump.air[jump.air_done as usize]
        {
            let speed = self.jump_speed();
            let p = &mut self.pikis[pi];
            p.vel.y = speed;
            if let Some(j) = p.jump.as_mut() {
                j.air_done += 1;
            }
        }
        if jump.stomping {
            let p = &mut self.pikis[pi];
            p.vel.y = p.vel.y.min(-STOMP_SPEED);
        }
        self.pikis[pi].target_vel = jump.horizontal;
        self.physics(pi, host, dt);
        let p = &mut self.pikis[pi];
        if p.on_ground {
            p.jump = None;
            if jump.stomping
                && let Some(captain) = p.captain
            {
                let (piki, pos) = (p.id, p.pos);
                self.events.push(crate::Event::PikiStomped { piki, captain, pos });
            }
        } else if let Some(j) = &mut p.jump {
            j.elapsed += dt;
        }
        true
    }

    pub(crate) fn try_navigation_jump(&mut self, pi: usize, host: &dyn Host) {
        let p = &self.pikis[pi];
        if !p.on_ground || p.jump.is_some() || !Self::can_navigation_jump(p.state) {
            return;
        }
        let horizontal = p.target_vel.with_y(0.0);
        let speed = horizontal.length();
        if speed < 10.0 {
            return;
        }
        let dir = horizontal / speed;
        let near = p.pos + dir * (p.size + speed * crate::TICK_DT);
        let centre = near + Vec3::Y * p.size;
        let blocked = (host.walls(centre, p.size) - centre).with_y(0.0).length() > 0.5;
        let floor = host.ground_height(near + Vec3::Y * p.size);
        let gap = floor.is_none_or(|g| p.pos.y - g > p.size * 2.0);
        if !blocked && !gap {
            return;
        }
        // Reject tall walls and gaps without a reachable landing. Carriers in Lift/Move
        // never enter this path: their item stays on the ground.
        let raised = centre + Vec3::Y * JUMP_HEIGHT;
        if (host.walls(raised, p.size) - raised).with_y(0.0).length() > 0.5 {
            return;
        }
        let flight = 2.0 * self.jump_speed() / self.params.ai.gravity.max(1.0);
        let landing = p.pos + horizontal * flight;
        let Some(g) = host.ground_height(landing + Vec3::Y * JUMP_HEIGHT) else {
            return;
        };
        if g > p.pos.y + JUMP_HEIGHT * 0.8 || g < p.pos.y - JUMP_HEIGHT {
            return;
        }
        self.start_piki_jump(pi, horizontal);
    }
}
