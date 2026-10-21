//! The strategy framework (`TaiStrategy` / `TaiState` / `TaiAction`, tai.cpp) and the actions most
//! enemies share (taibasicactions, taimotionactions, taimoveactions, taijudgementactions,
//! taiattackactions, taireactionactions, taitimeractions). A strategy is a table of states; a state is a
//! list of actions run in order each tick; the first one that returns true and names a next state moves
//! the enemy there. Actions here are built from closures so a strategy reads like the decomp's
//! constructor.

use std::sync::Arc;

use crate::object::*;
use crate::anim::key;
use libpikmin_assets::teki::{tpf, tpi};

type F0 = Box<dyn Fn(&mut Teki, &mut World) + Send + Sync>;
type F1 = Box<dyn Fn(&mut Teki, &mut World) -> bool + Send + Sync>;
type F2 = Box<dyn Fn(&mut Teki, &mut World, &Event) -> bool + Send + Sync>;

/// One `TaiAction`.
pub struct Action {
    pub next: i32,
    start: Option<F0>,
    finish: Option<F0>,
    act: Option<F1>,
    event: Option<F2>,
}

pub type Act = Arc<Action>;

impl Action {
    pub fn new(next: i32) -> Self {
        Action { next, start: None, finish: None, act: None, event: None }
    }
    pub fn on_start(mut self, f: impl Fn(&mut Teki, &mut World) + Send + Sync + 'static) -> Self {
        self.start = Some(Box::new(f));
        self
    }
    pub fn on_finish(mut self, f: impl Fn(&mut Teki, &mut World) + Send + Sync + 'static) -> Self {
        self.finish = Some(Box::new(f));
        self
    }
    pub fn on_act(mut self, f: impl Fn(&mut Teki, &mut World) -> bool + Send + Sync + 'static) -> Self {
        self.act = Some(Box::new(f));
        self
    }
    pub fn on_event(mut self, f: impl Fn(&mut Teki, &mut World, &Event) -> bool + Send + Sync + 'static) -> Self {
        self.event = Some(Box::new(f));
        self
    }
    pub fn build(self) -> Act {
        Arc::new(self)
    }
    pub fn start(&self, t: &mut Teki, w: &mut World) {
        if let Some(f) = &self.start {
            f(t, w);
        }
    }
    pub fn finish(&self, t: &mut Teki, w: &mut World) {
        if let Some(f) = &self.finish {
            f(t, w);
        }
    }
    pub fn act(&self, t: &mut Teki, w: &mut World) -> bool {
        self.act.as_ref().is_some_and(|f| f(t, w))
    }
    pub fn by_event(&self, t: &mut Teki, w: &mut World, e: &Event) -> bool {
        self.event.as_ref().is_some_and(|f| f(t, w, e))
    }
}

/// A strategy: its states' actions and the state it starts in.
pub struct Strategy {
    pub states: Vec<Vec<Act>>,
    pub initial: i32,
    /// What the strategy does when an enemy first starts it (`TaiStrategy::start` overrides).
    pub on_start: Option<Arc<dyn Fn(&mut Teki) + Send + Sync>>,
}

impl Strategy {
    pub fn new(count: usize, initial: i32) -> Self {
        Strategy { states: vec![Vec::new(); count], initial, on_start: None }
    }
    pub fn set(&mut self, state: i32, actions: Vec<Act>) {
        self.states[state as usize] = actions;
    }

    /// `TaiStrategy::start`.
    pub fn start(&self, t: &mut Teki) {
        t.state = self.initial;
        t.state_ready = true;
        if let Some(f) = &self.on_start {
            f(t);
        }
    }

    fn enter(&self, t: &mut Teki, w: &mut World) {
        for a in &self.states[t.state as usize] {
            a.start(t, w);
        }
    }

    fn leave(&self, state: i32, t: &mut Teki, w: &mut World) {
        for a in &self.states[state as usize] {
            a.finish(t, w);
        }
    }

    fn transit(&self, t: &mut Teki, next: i32) {
        let from = t.state;
        t.state = if next == RETURN_TRANSIT { t.return_state } else { next };
        t.return_state = from;
    }

    /// `TaiStrategy::act`.
    pub fn act(&self, t: &mut Teki, w: &mut World) {
        if t.state_ready {
            self.enter(t, w);
            t.state_ready = false;
        }
        let id = t.state;
        let actions = self.states[id as usize].clone();
        for a in &actions {
            if a.act(t, w) && a.next != NO_TRANSIT {
                self.transit(t, a.next);
                self.leave(id, t, w);
                t.state_ready = true;
                return;
            }
        }
    }

    /// `TaiStrategy::eventPerformed`.
    pub fn event(&self, t: &mut Teki, w: &mut World, e: &Event) {
        let id = t.state;
        let actions = self.states[id as usize].clone();
        for a in &actions {
            if a.by_event(t, w, e) && a.next != NO_TRANSIT {
                self.transit(t, a.next);
                self.leave(id, t, w);
                t.state_ready = true;
                return;
            }
        }
    }
}

// ---- timers (taitimeractions.cpp)

fn rate_random(w: &mut World, len: f32, rate: f32) -> f32 {
    if rate <= 0.0 { len } else { len * w.rng.range(1.0 - rate, 1.0 + rate) }
}

/// `TaiTimerAction`: starts timer `idx` and is true when it has run out.
pub fn timer(next: i32, idx: usize, len: f32, rate: f32) -> Act {
    Action::new(next).on_start(move |t, w| t.timers[idx] = rate_random(w, len, rate)).on_act(move |t, _| t.timer_elapsed(idx)).build()
}
/// `TaiResetTimerAction`.
pub fn reset_timer(next: i32, idx: usize, len: f32, rate: f32) -> Act {
    Action::new(next).on_start(move |t, w| t.timers[idx] = rate_random(w, len, rate)).build()
}
/// `TaiTimerElapsedAction`.
pub fn timer_elapsed(next: i32, idx: usize) -> Act {
    Action::new(next).on_act(move |t, _| t.timer_elapsed(idx)).build()
}
/// `TaiAndAction`: both act, true if both are.
pub fn and(next: i32, a: Act, b: Act) -> Act {
    let (a1, b1, a2, b2, a3, b3) = (a.clone(), b.clone(), a.clone(), b.clone(), a, b);
    Action::new(next)
        .on_start(move |t, w| {
            a1.start(t, w);
            b1.start(t, w);
        })
        .on_finish(move |t, w| {
            a2.finish(t, w);
            b2.finish(t, w);
        })
        .on_act(move |t, w| {
            let x = a3.act(t, w);
            let y = b3.act(t, w);
            x && y
        })
        .build()
}
/// `TaiOnceAction`: always true.
pub fn once(next: i32) -> Act {
    Action::new(next).on_act(|_, _| true).build()
}

/// `TaiDependenceAction`: when the primary acts, the dependent acts too and decides.
pub fn dependence(next: i32, dependent: Act, primary: Act) -> Act {
    let (d1, p1, d2, p2, d3, p3, d4, p4) = (dependent.clone(), primary.clone(), dependent.clone(), primary.clone(), dependent.clone(), primary.clone(), dependent, primary);
    Action::new(next)
        .on_start(move |t, w| {
            d1.start(t, w);
            p1.start(t, w);
        })
        .on_finish(move |t, w| {
            d2.finish(t, w);
            p2.finish(t, w);
        })
        .on_act(move |t, w| p3.act(t, w) && d3.act(t, w))
        .on_event(move |t, w, e| p4.by_event(t, w, e) && d4.by_event(t, w, e))
        .build()
}

/// `TaiSetOptionAction`.
pub fn set_options(bits: u32, on: bool) -> Act {
    Action::new(NO_TRANSIT).on_start(move |t, _| if on { t.set_option(bits) } else { t.clear_option(bits) }).build()
}

// ---- basic

/// `TaiActionStateAction`: true when the action-state register equals `state`.
pub fn action_state(next: i32, state: i32) -> Act {
    Action::new(next).on_start(|t, _| t.action_state = 0).on_act(move |t, _| t.action_state == state).build()
}
pub fn set_option(opt_bits: u32, on: bool) -> Act {
    Action::new(NO_TRANSIT).on_start(move |t, _| if on { t.set_option(opt_bits) } else { t.clear_option(opt_bits) }).build()
}

// ---- motion (taimotionactions.cpp)

/// `TaiMotionAction`: starts a motion and is true when it has finished.
pub fn motion(next: i32, m: usize) -> Act {
    Action::new(next).on_start(move |t, _| t.start_motion(m)).on_act(|t, _| t.animation_finished()).build()
}

fn continuous_start(t: &mut Teki, m: usize) {
    let cur = t.current_motion();
    if cur < 0 {
        t.start_motion(m);
        return;
    }
    if cur == m as i32 {
        if t.anim.is_finished() || t.anim.is_finishing() {
            t.start_motion(m);
        }
    } else if t.anim.is_finishing() {
        t.start_motion(m);
    } else {
        t.finish_motion();
    }
}

/// The part of `TaiContinuousMotionAction::act` that other actions build on: true if `m` is the motion
/// playing and it has finished.
pub fn continuous_act(t: &mut Teki, m: usize) -> bool {
    if t.current_motion() != m as i32 {
        if t.animation_finished() {
            t.start_motion(m);
        }
        return false;
    }
    t.animation_finished()
}

pub fn motion_started(t: &Teki, m: usize) -> bool {
    t.current_motion() == m as i32
}

/// `TaiContinuousMotionAction`.
pub fn continuous_motion(next: i32, m: usize) -> Act {
    Action::new(next).on_start(move |t, _| continuous_start(t, m)).on_act(move |t, _| continuous_act(t, m)).build()
}
/// `TaiFinishMotionAction`.
pub fn finish_motion(next: i32) -> Act {
    Action::new(next).on_start(|t, _| t.finish_motion()).build()
}
/// `TaiSwitchMotionAction`: a new motion from the same frame.
pub fn switch_motion(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            let c = t.anim.counter;
            t.start_motion(m);
            t.anim.counter = c;
        })
        .on_act(|t, _| t.animation_finished())
        .build()
}
/// `TaiRandomSetAnimationCounterAction`: start anywhere between two keys.
pub fn random_set_animation_counter(min_key: u8, max_key: u8) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            let (lo, hi) = (t.key_value(min_key), t.key_value(max_key));
            t.anim.counter = w.rng.range(lo, (hi - 1.0).max(lo));
        })
        .build()
}
/// `TaiStoppingMoveAction`: while its motion loops the enemy slows to a stop at the loop end.
pub fn stopping_move(m: usize) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, _| {
            if t.current_motion() == m as i32 {
                if t.key(key::LOOP_END) {
                    t.start_stopping_move();
                } else if t.animation_finished() {
                    t.finish_stopping_move();
                }
            }
            false
        })
        .build()
}
pub fn finish_stopping_move() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.finish_stopping_move()).build()
}
pub fn stop_move() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.stop_move()).build()
}

// ---- movement (taimoveactions.cpp)

/// `TaiTracingAction`: walk at the target.
pub fn tracing(m: usize, speed: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, _| continuous_start(t, m))
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            if let Some(c) = t.target(0, w) {
                t.move_toward(c.pos, speed, w.dt);
            }
            false
        })
        .build()
}
/// `TaiGoingHomeAction`.
pub fn going_home(m: usize, speed: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, _| continuous_start(t, m))
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let nest = t.nest;
            t.move_toward(nest, speed, w.dt);
            false
        })
        .build()
}
/// `TaiTurningToTargetPositionAction`.
pub fn turning_to_target_position(next: i32, m: usize, speed: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| continuous_start(t, m))
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            if t.anim.counter < t.key_value(0) {
                return false;
            }
            if t.calc_target_angle(t.target_pos) <= (t.f(tpf::TRACE_ANGLE) / 2.0).to_radians() {
                return true;
            }
            t.target_angle = t.calc_target_direction(t.target_pos);
            let a = t.target_angle;
            t.turn_toward(a, speed, w.dt)
        })
        .build()
}
/// `TaiTargetNestAction`.
pub fn target_nest() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.target_pos = t.nest).build()
}
/// `TaiWatchOffTerritoryCenterAction`.
pub fn watch_off_territory_center(next: i32) -> Act {
    Action::new(next).on_act(|t, _| t.calc_target_angle(t.nest) > std::f32::consts::PI * 0.1).build()
}
/// `TaiOutOfTraceAngleAction`.
pub fn out_of_trace_angle(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let Some(c) = t.target(0, w) else { return false };
            t.calc_target_angle(c.pos) > (t.f(tpf::TRACE_ANGLE) / 2.0).to_radians()
        })
        .build()
}
/// `TaiTraceTurningAction`: turn in place until the target is within the trace angle.
pub fn trace_turning(next: i32, m: usize, speed: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| continuous_start(t, m))
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let Some(c) = t.target(0, w) else { return true };
            if t.anim.counter < t.key_value(0) {
                return false;
            }
            if t.calc_target_angle(c.pos) <= (t.f(tpf::TRACE_ANGLE) / 2.0).to_radians() {
                return true;
            }
            t.target_angle = t.calc_target_direction(c.pos);
            let a = t.target_angle;
            t.turn_toward(a, speed, w.dt)
        })
        .build()
}

// ---- judgement (taijudgementactions.cpp)

/// `TaiTargetVisibleNaviPikiAction`: pick the nearest visible Pikmin or player.
pub fn target_visible_navi_piki(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let c = t.closest(w, |c| t.visible(c));
            match c {
                Some(c) => {
                    t.targets[0] = Some(c.target);
                    true
                }
                None => false,
            }
        })
        .build()
}
/// `TaiTargetLostAction`: the target is gone or out of sight.
pub fn target_lost(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.target(0, w) {
            None => true,
            Some(c) => {
                if !t.visible(&c) {
                    t.targets[0] = None;
                    true
                } else {
                    false
                }
            }
        })
        .build()
}
/// `TaiContactTargetAction`.
pub fn contact_target(next: i32) -> Act {
    Action::new(next).on_act(|t, w| t.target(0, w).is_some_and(|c| t.contact(&c))).build()
}
/// `TaiSeparateTargetAction`.
pub fn separate_target(next: i32) -> Act {
    Action::new(next).on_act(|t, w| t.target(0, w).is_none_or(|c| t.separate(&c))).build()
}
pub fn outside_territory(next: i32, radius: f32) -> Act {
    Action::new(next).on_act(move |t, _| t.territory_distance() > radius).build()
}
pub fn inside_territory(next: i32, radius: f32) -> Act {
    Action::new(next).on_act(move |t, _| t.territory_distance() <= radius).build()
}

// ---- attack (taiattackactions.cpp)

/// `TaiAttackableNaviPikiAction`: target the nearest thing in biting range.
pub fn attackable_navi_piki(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.closest(w, |c| t.attackable(c)) {
            Some(c) => {
                t.targets[0] = Some(c.target);
                true
            }
            None => false,
        })
        .build()
}

/// `TaiFlickAction`: enough hits for how many Pikmin are on it.
pub fn flick_when_hit(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::LOWER_RANGE);
            let n = t.count_pikis_in_range(w, range);
            t.damage_count >= t.flick_damage_count(n) as f32
        })
        .build()
}

/// `TaiFlickingAction`: the shake motion; at its action key everything on it is flung off.
pub fn flicking(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_finish(|t, _| t.set_option(opt::DAMAGE_COUNTABLE))
        .on_act(|t, w| {
            if t.animation_finished() {
                t.set_option(opt::DAMAGE_COUNTABLE);
                return true;
            }
            if t.key(key::ACTION0) {
                t.flick(w);
                t.damage_count = 0.0;
            }
            false
        })
        .build()
}

/// `TaiAnimationSwallowingAction`: at the bite's action key whatever is in front is taken into the mouth
/// (or killed outright if there is no mouth), a player is hit; at the second key the chewing kills it.
pub fn animation_swallowing(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            if t.targets[0].is_some() {
                t.set_option(opt::INVINCIBLE);
            }
        })
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                t.flick_upper(w);
                let centre = t.hit_center();
                let range = t.f(tpf::ATTACK_HIT_RANGE);
                let max = t.i(tpi::SWALLOW_COUNT).max(0) as u32;
                let hit_navi = swallow_navis(t, w, centre, range);
                let n = w.sim.swallow_pikis(t.creature, 0, centre, range, max);
                t.clear_option(opt::INVINCIBLE);
                t.action_state = match (hit_navi, n > 0) {
                    (true, true) => 3,
                    (true, false) => 1,
                    (false, true) => 2,
                    (false, false) => {
                        t.action_state = 0;
                        return true;
                    }
                };
            } else if t.key(key::ACTION1) {
                w.sim.chew(t.creature);
            }
            false
        })
        .on_finish(|t, w| {
            w.sim.spit_out(t.creature);
        })
        .build()
}

/// A player in the bite takes its damage (`InteractSwallow` on a Navi).
fn swallow_navis(t: &Teki, w: &mut World, centre: glam::Vec3, range: f32) -> bool {
    let mut any = false;
    for n in w.navis.iter().filter(|n| n.alive && dist_xz(centre, n.pos) - n.size <= range) {
        w.out.push(Out::HitNavi { navi: n.id, damage: t.f(tpf::ATTACK_POWER), from: t.pos, knock: 0.0 });
        any = true;
    }
    any
}

// ---- reaction (taireactionactions.cpp)

pub fn dead(next: i32) -> Act {
    Action::new(next).on_act(|t, _| t.hp <= 0.0).build()
}
pub fn life_zero() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.hp = 0.0).build()
}
pub fn life_damage(amount: f32) -> Act {
    Action::new(NO_TRANSIT).on_start(move |t, _| t.hp -= amount).build()
}
/// `TaiDyeAction`.
pub fn dye() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, w| t.die(w)).build()
}
/// `TaiStartDyingAction`: the corpse is made and the death effects play.
pub fn start_dying() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.create_dead_effect(w);
            w.out.push(Out::Dying { teki: t.id });
            t.clear_option(opt::SHADOW_VISIBLE);
        })
        .build()
}
/// `TaiDyingAction`: the death motion, then `die`.
pub fn dying(m: usize) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.clear_option(opt::ALIVE);
            t.hp = 0.0;
        })
        .on_act(|t, w| {
            if t.animation_finished() {
                t.die(w);
                true
            } else {
                false
            }
        })
        .build()
}
/// `TaiDamageAction`: damage is waiting.
pub fn damage(next: i32) -> Act {
    Action::new(next).on_act(|t, _| t.stored_damage > 0.0).build()
}
/// `TaiSimultaneousDamageAction`: apply the damage now and jiggle.
pub fn simultaneous_damage(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, _| {
            if t.stored_damage <= 0.0 {
                return false;
            }
            t.make_damaged();
            let p = t.f(tpf::DAMAGE_MOTION_PERIOD);
            t.start_damage_motion(p);
            true
        })
        .build()
}
/// `TaiCounterattackSimultaneousDamageAction`: also turn on whoever hit it.
pub fn counterattack_simultaneous_damage(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            if t.stored_damage <= 0.0 {
                return false;
            }
            let maybe = t.targets[1].and_then(|x| t.cand_of(x, w));
            if t.nearest_angle_target(maybe, w) {
                t.targets[0] = t.targets[1];
            }
            t.make_damaged();
            let p = t.f(tpf::DAMAGE_MOTION_PERIOD);
            t.start_damage_motion(p);
            true
        })
        .build()
}
/// `TaiDamagingAction`: the damage motion.
pub fn damaging(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.make_damaged();
        })
        .on_act(|t, _| t.animation_finished())
        .build()
}
/// `TaiTargetNearestAssailantAction`: turn on whoever hit it, if nearer its facing.
pub fn target_nearest_assailant(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            if t.stored_damage <= 0.0 {
                return false;
            }
            let maybe = t.targets[1].and_then(|x| t.cand_of(x, w));
            if t.nearest_angle_target(maybe, w) {
                t.targets[0] = t.targets[1];
            }
            true
        })
        .build()
}
/// `TaiTargetNearestCollisionAction`: bumped by something it can see.
pub fn target_nearest_collision(next: i32) -> Act {
    Action::new(next)
        .on_event(|t, w, e| {
            let Event::Entity { other, .. } = e else { return false };
            let Some(c) = t.cand_of(*other, w) else { return false };
            if t.nearest_angle_target(Some(c), w) {
                t.targets[0] = Some(c.target);
            }
            true
        })
        .build()
}
/// `TaiPressedAction`: squashed.
pub fn pressed(next: i32) -> Act {
    Action::new(next).on_event(|_, _, e| matches!(e, Event::Pressed)).build()
}
/// `TaiSmashedAction`: a thrown Pikmin hit it.
pub fn smashed(next: i32) -> Act {
    Action::new(next).on_event(|_, _, e| matches!(e, Event::Entity { flying: true, .. })).build()
}
/// `TaiBeingPressedAction`.
pub fn being_pressed() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| {
            t.set_option(opt::PRESSED);
            t.clear_option(opt::ALIVE);
            t.clear_option(opt::ATARI);
        })
        .build()
}
/// `TaiWarnAction`: sees everything near it for a while after being hit.
pub fn warn() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.timers[4] = t.f(tpf::WARN_PERIOD)).build()
}
/// `TaiDamageCountResetAction`.
pub fn damage_count_reset() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.damage_count = 0.0).build()
}

/// A named sound at the enemy (`TaiPlaySoundAction`).
pub fn play_sound(name: &'static str) -> Act {
    Action::new(NO_TRANSIT).on_start(move |t, w| w.out.push(Out::Sound { name, pos: t.pos })).build()
}
