//! Pikmin 1's enemies ("teki") as a game-agnostic simulation: the strategies of the decomp's `Tai*`
//! files ported as state machines over the sim's Pikmin, with the numbers from the disc.
//!
//! The core has no engine types. A host spawns enemies, hands them the world each tick (where its
//! players are, how its enemies are hurt) and applies what they do ([`Out`]: damage to its players, new
//! enemies, effects). Its own bodies may carry the enemies' health and hurt boxes: the host feeds
//! damage in with [`Tekis::damage`] and the real position back with [`Tekis::set_position`].

pub mod anim;
pub mod armor;
pub mod baby;
pub mod beatle;
pub mod bomb;
pub mod bulborb;
pub mod bulborb2;
pub mod collec;
pub mod object;
pub mod dororo;
pub mod gashiba;
pub mod hiba;
pub mod effects;
pub mod info;
pub mod kabekui;
pub mod kabuto;
pub mod king;
pub mod kinoko;
pub mod kogane;
pub mod mar;
pub mod miurin;
pub mod mitite;
pub mod mizinko;
pub mod napkid;
pub mod otama;
pub mod palm;
pub mod queen;
pub mod otimoti;
pub mod p2;
pub mod shell;
pub mod sokkuri;
pub mod tai;
pub mod tank;
pub mod wtank;
pub mod yai;

use std::sync::Arc;

use glam::Vec3;
use libpikmin::host::{CreatureFrame, CreaturePart, Host};
use libpikmin::{PikiState, Sim};
use libpikmin_assets::Assets;

pub use crate::object::{Event, Navi, Other, Out, Rng, Target, Teki, TekiId, World, opt};
use crate::info::{KIND_TABLE, TekiInfo};
use crate::tai::Strategy;

/// The enemies of one game: their kinds' facts and strategies, and the ones alive.
pub struct Tekis {
    infos: Vec<Option<Arc<TekiInfo>>>,
    strategies: Vec<Option<Arc<Strategy>>>,
    pub list: Vec<Teki>,
    next_id: TekiId,
    rng: Rng,
    /// Events the enemies raised for each other (a cry waking sleepers), delivered next tick.
    pending: Vec<(TekiId, Event)>,
    /// Players buried now: the host sets them before each update.
    pub buried: Vec<u32>,
}

impl Tekis {
    /// Loads every kind whose files are in `assets` and has a strategy.
    pub fn load(assets: &Assets) -> Tekis {
        let pellets = assets
            .read("parms/pelMgr.bin")
            .ok()
            .and_then(|d| libpikmin::pellet_config::parse_pellet_configs(&d).ok())
            .unwrap_or_default();
        let mut infos = Vec::new();
        let mut strategies = Vec::new();
        for kind in 0..KIND_TABLE.len() as u8 {
            let info = TekiInfo::load(assets, kind, 0, &pellets).ok().map(Arc::new);
            let strategy = info.as_ref().and_then(|i| strategy_for(i)).map(Arc::new);
            infos.push(info);
            strategies.push(strategy);
        }
        // Pikmin 2's enemies follow, from the cache `libsms::extract::p2_enemy` fills beside the Pikmin 1 files.
        let p2_root = assets.root.join("p2");
        for kind in p2::FIRST..p2::FIRST + p2::KIND_TABLE.len() as u8 {
            let info = TekiInfo::load_p2(&p2_root, kind).ok().map(Arc::new);
            let strategy = info.as_ref().map(|_| Arc::new(p2_strategy(kind)));
            infos.push(info);
            strategies.push(strategy);
        }
        Tekis { infos, strategies, list: Vec::new(), next_id: 0, rng: Rng(0x9E37_79B9_7F4A_7C15), pending: Vec::new(), buried: Vec::new() }
    }

    pub fn seed(&mut self, seed: u64) {
        self.rng = Rng(seed | 1);
    }

    pub fn info(&self, kind: u8) -> Option<&Arc<TekiInfo>> {
        self.infos.get(kind as usize).and_then(Option::as_ref)
    }

    /// Whether `kind` has a ported strategy.
    pub fn has_strategy(&self, kind: u8) -> bool {
        self.strategies.get(kind as usize).is_some_and(Option::is_some)
    }

    /// A new enemy of `kind`; `creature` is the host id for it (the sim creatures are keyed by it).
    pub fn spawn(&mut self, kind: u8, creature: u64, pos: Vec3, yaw: f32) -> Option<TekiId> {
        let info = self.info(kind)?.clone();
        let strategy = self.strategies.get(kind as usize)?.clone()?;
        self.next_id += 1;
        let mut t = Teki::new(self.next_id, info, creature, pos, yaw);
        strategy.start(&mut t);
        self.list.push(t);
        Some(self.next_id)
    }

    pub fn get(&self, id: TekiId) -> Option<&Teki> {
        self.list.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TekiId) -> Option<&mut Teki> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    /// A hit (the host damage, in the disc life units).
    pub fn damage(&mut self, id: TekiId, amount: f32, attacker: Option<Target>) {
        // Pikmin 2's invulnerable enemies (the Kogane family) answer a hit the way they answer a Pikmin landing on
        // them: `pressCallBack`.
        if self.get(id).is_some_and(|t| t.kind >= p2::FIRST && t.option(opt::INVINCIBLE) && t.alive()) && amount < 1.0e5 {
            self.pending.push((id, Event::Pressed));
            return;
        }
        if let Some(t) = self.get_mut(id) {
            // A killing blow (the host's body died) goes through whatever the enemy is doing.
            if amount >= 1.0e5 {
                t.clear_option(opt::INVINCIBLE);
            }
            t.take_damage(amount);
            if attacker.is_some() {
                t.targets[1] = attacker;
            }
        }
    }

    /// The host body is somewhere else (blocked, pushed): the enemy follows it.
    /// Where an enemy should head (the Smoky Progg's Onion).
    pub fn set_goal(&mut self, id: TekiId, goal: Option<Vec3>) {
        if let Some(t) = self.get_mut(id) {
            t.goal = goal;
        }
    }

    /// A scratch counter a host sets before the enemy starts (the Pellet Posy's size, whether a Clamclamp's pearl holds
    /// a ship part).
    pub fn set_count(&mut self, id: TekiId, index: usize, value: i32) {
        if let Some(t) = self.get_mut(id)
            && let Some(c) = t.count.get_mut(index)
        {
            *c = value;
        }
    }

    /// The velocity a thing was shot with (a Rolling Boulder).
    pub fn set_spawn_velocity(&mut self, id: TekiId, v: Vec3) {
        if let Some(t) = self.get_mut(id) {
            t.spawn_vel = v;
        }
    }

    /// The ground under an enemy's body: its height, and whether it is water. The strategies fall, land and fly
    /// relative to it.
    pub fn set_ground(&mut self, id: TekiId, ground: f32, water: bool) {
        if let Some(t) = self.get_mut(id) {
            t.ground_y = Some(ground);
            t.in_water = water;
        }
    }

    /// The ground under an enemy is water, or not (a body that moves itself tells it each tick).
    pub fn set_water(&mut self, id: TekiId, water: bool) {
        if let Some(t) = self.get_mut(id) {
            t.in_water = water;
        }
    }

    pub fn set_position(&mut self, id: TekiId, pos: Vec3) {
        if let Some(t) = self.get_mut(id) {
            t.pos = pos;
        }
    }

    /// Squashed by something (`TekiEventType::Pressed`).
    pub fn press(&mut self, id: TekiId) {
        self.pending.push((id, Event::Pressed));
    }

    pub fn remove(&mut self, id: TekiId) {
        self.list.retain(|t| t.id != id);
    }

    /// The enemies as creatures for the sim: one sphere on the body. A host with real hurt boxes pushes
    /// its own parts instead.
    pub fn frame(&self, out: &mut CreatureFrame) {
        out.clear();
        for t in self.list.iter().filter(|t| t.alive()) {
            let h = (t.info.hi.y - t.info.lo.y).max(1.0);
            let r = t.coll_radius.max(h * 0.4);
            let part = CreaturePart { centre: t.pos + Vec3::Y * (h * 0.4), radius: r, stickable: true };
            out.push(t.creature, t.pos, t.yaw, true, t.flying, &[part]);
        }
    }

    /// One 30 Hz tick: animation, then each strategy, then movement. What they did is returned.
    pub fn update(&mut self, sim: &mut Sim, host: &dyn Host, navis: &[Navi], dt: f32) -> Vec<Out> {
        let mut out = Vec::new();
        let buried = self.buried.clone();
        let others: Vec<Other> =
            self.list.iter().map(|t| Other { id: t.id, kind: t.kind, state: t.state, pos: t.pos, alive: t.alive(), holding: t.held }).collect();
        let mut events: Vec<(TekiId, Event)> = std::mem::take(&mut self.pending);
        for i in 0..self.list.len() {
            let Some(strategy) = self.strategies[self.list[i].kind as usize].clone() else { continue };
            let t = &mut self.list[i];
            let mut w = World { sim, host, navis, buried: &buried, others: &others, out: &mut out, rng: &mut self.rng, dt };
            t.gravity = w.sim.params().ai.gravity;
            step(t, &strategy, &mut w, &mut events);
            if t.kind >= p2::FIRST {
                // `doUpdateCommon`, which every state runs.
                kogane::update_common(t, &mut w);
                bulborb2::burn_common(t, &mut w);
            }
        }
        // Messages between enemies, and the enemies that finished.
        let mut finished = Vec::new();
        for o in &out {
            match o {
                Out::Message { from, pos, range } => {
                    for t in &self.list {
                        if t.id != *from && crate::object::dist_xz(t.pos, *pos) <= *range {
                            events.push((t.id, Event::WakeUp));
                        }
                    }
                }
                Out::Died { teki } => finished.push(*teki),
                _ => {}
            }
        }
        self.pending = events;
        self.list.retain(|t| !finished.contains(&t.id));
        out
    }
}

/// The strategy of Pikmin 2 kind `kind`.
fn p2_strategy(kind: u8) -> Strategy {
    match kind {
        p2::BULBORB_LARVA => baby::strategy(),
        p2::SKITTER_LEAF => sokkuri::strategy(),
        p2::MITITE => mitite::strategy(),
        p2::GAS_PIPE => gashiba::strategy(),
        p2::BOMB_ROCK => bomb::strategy(),
        p2::WATERY_BLOWHOG => wtank::strategy(),
        p2::CANNON_LARVA => kabuto::strategy(),
        p2::EMPRESS_BULBLAX => queen::strategy(),
        p2::EMPEROR_BULBLAX => king::strategy(),
        p2::BURROW_NIT => armor::strategy(),
        p2::ORANGE_BULBORB | p2::HAIRY_BULBORB | p2::DWARF_ORANGE_BULBORB | p2::SNOW_BULBORB | p2::FIERY_BULBLAX => bulborb2::strategy(),
        _ => kogane::strategy(kind),
    }
}

fn strategy_for(info: &TekiInfo) -> Option<Strategy> {
    use crate::info::*;
    Some(match info.kind {
        DWARF | DWARF_ORANGE => bulborb::chappy(info),
        SPOTTY | SPOTTY_ORANGE => bulborb::swallow(info),
        WATER_DUMPLE => bulborb::chappy(info),
        HONEYWISP => mizinko::honeywisp(info),
        BREADBUG => collec::breadbug(info),
        WOGPOLE => otama::wogpole(info),
        WOLLYWOG | WOLLYWOG_YELLOW => otimoti::wollywog(info),
        FIERY_BLOWHOG => tank::tank(info),
        SNITCHBUG => napkid::snitchbug(info),
        PUFFSTOOL => kinoko::puffstool(info),
        SMOKY_PROGG => dororo::smoky_progg(info),
        GIANT_EGG => dororo::giant_egg(info),
        FIRE_GEYSER => hiba::fire_geyser(info),
        CLAMCLAMP => shell::clamclamp(info),
        PEARL => shell::pearl(info),
        ROCPE => shell::rocpe(info),
        PELLET_POSY => palm::pellet_posy(info),
        PUFFY_BLOWHOG => mar::puffy_blowhog(info),
        MAMUTA => miurin::mamuta(info),
        SHEARGRUB_F => kabekui::female(info),
        SHEARGRUB_M => kabekui::male(info),
        SHEARWIG => kabekui::shearwig(info),
        CANNON_BEETLE => beatle::beatle(info),
        IWAGON => beatle::iwagon(info),
        _ => return None,
    })
}

/// One enemy tick (`BTeki::update`, `doAnimation`, `doAI`, the physics).
fn step(t: &mut Teki, strategy: &Strategy, w: &mut World, events: &mut Vec<(TekiId, Event)>) {
    let dt = w.dt;
    t.age += dt;
    t.flash = (t.flash - dt).max(0.0);
    t.shake = (t.shake - dt).max(0.0);
    t.update_timers(dt);
    if t.hp > 0.0 {
        let max = t.info.life;
        t.hp = (t.hp + dt * max * t.f(libpikmin_assets::teki::tpf::LIFE_RECOVER_RATE)).min(max);
    }

    // doAnimation: the speed follows the creature for walking motions.
    let data = t.info.clone();
    if let Some(m) = usize::try_from(t.anim.motion).ok().and_then(|i| data.motion_data.get(i)) {
        t.anim_speed = if let Some(s) = t.manual_speed {
            s
        } else if t.option(opt::STOPPING_MOVE) {
            t.pre_stop_speed
        } else if m.dynamic_speed {
            let rate = m.speed / 30.0;
            (t.target_vel.length() * rate / data.scale.max(0.01)).max(10.0 * rate)
        } else {
            m.speed
        };
        let mut ev = Vec::new();
        t.anim.animate(t.anim_speed, dt, &data.motion_data, &mut ev);
        t.anim_events(w, ev);
    }

    // Events: touching things (TekiEventType::Entity), for TaiTargetNearestCollision and TaiSmashed.
    if t.alive() && t.option(opt::ATARI) {
        for c in t.candidates(w) {
            if t.collision_distance(&c) <= 0.0 {
                let flying = c.piki.is_some_and(|id| matches!(w.sim.piki(id).state, PikiState::Flying { .. }));
                events.push((t.id, Event::Entity { other: c.target, flying }));
            }
        }
    }
    let mine: Vec<Event> = events.iter().filter(|(id, _)| *id == t.id).map(|(_, e)| *e).collect();
    events.retain(|(id, _)| *id != t.id);
    for e in mine {
        strategy.event(t, w, &e);
    }

    strategy.act(t, w);
    if t.fire.is_some() {
        yai::burn(t, w);
    }
    t.anim.clear_keys();

    // Movement: along its target velocity, on the ground the host says is there; gravity pulls it down
    // unless it is flying (`BTeki::gravitate`).
    if t.target_vel != Vec3::ZERO {
        t.pos += t.target_vel * dt;
    }
    let ground = w.host.ground_height(t.pos + Vec3::Y * 20.0).or(t.ground_y);
    if let Some(g) = ground {
        t.ground_y = Some(g);
    }
    if !t.flying && t.option(opt::GRAVITATABLE) {
        t.action_vel.y -= t.gravity * dt;
        t.pos += t.action_vel * dt;
        if let Some(g) = ground
            && t.pos.y <= g
        {
            t.pos.y = g;
            t.action_vel = Vec3::ZERO;
            strategy.event(t, w, &Event::Ground);
        }
    }
    let r = t.coll_radius;
    let before = t.pos;
    t.pos = w.host.walls(t.pos + Vec3::Y * r, r) - Vec3::Y * r;
    if (t.pos - before).with_y(0.0).length() > 0.01 {
        strategy.event(t, w, &Event::Wall);
    }

    // A pellet in its mouth goes where it goes (the Breadbug).
    if let Some(id) = t.held {
        let size = w.sim.pellet(id).desc.radius;
        let at = t.pos + t.forward() * (t.coll_radius + size * 0.8);
        if !w.sim.hold_pellet(id, at, t.yaw) {
            t.held = None;
        }
    }
}
