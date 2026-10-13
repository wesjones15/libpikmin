//! Host extension: voluntary jumps and repeating a captain's takeoff points.
//! This is new gameplay. Geyser presentation comes from NaviGeyzerState::init
//! (naviState.cpp); Pikmin use PikiFlyingState's airborne motion (pikiState.cpp).

use crate::carry::CarryPhase;
use crate::piki::AttackMode;
use crate::{Host, PikiState, Sim, Vec3};

/// Sim units, about 1.6 metres at the default RoR2 scale.
pub const JUMP_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
pub struct Jump {
    pub elapsed: f32,
    pub horizontal: Vec3,
    /// Host jump pads supply their own gravity; voluntary jumps use disc tuning.
    pub gravity: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct JumpCue {
    pub origin: Vec3,
    pub horizontal: Vec3,
    pub launch: Option<(f32, f32)>,
    pub tick: u64,
}

impl Sim {
    fn jump_speed(&self) -> f32 {
        (2.0 * self.params.ai.gravity.max(1.0) * JUMP_HEIGHT).sqrt()
    }

    pub(crate) fn start_captain_jump(&mut self, ci: usize) {
        let speed = self.jump_speed();
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
            elapsed: 0.0,
            horizontal,
            gravity: None,
        });
        let cue = JumpCue {
            origin,
            horizontal,
            launch: None,
            tick: self.tick,
        };
        self.queue_squad_jump(ci, cue);
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
            elapsed: 0.0,
            horizontal,
            gravity: None,
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
        self.pikis[pi].target_vel = jump.horizontal;
        self.physics(pi, host, dt);
        let p = &mut self.pikis[pi];
        if p.on_ground {
            p.jump = None;
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
