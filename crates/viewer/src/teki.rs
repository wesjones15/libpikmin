//! The viewer's enemies: Dwarf Bulborbs, Spotty Bulborbs and Honeywisps from the user's disc.
//!
//! Enemies belong to the host game, so this is viewer code, not the core. It is a compact port
//! of the Pikmin 1 strategies, with every number from the disc's `tekipara/*.bin` and timings
//! from `tekikeys/*.key`:
//! - Dwarf Bulborb: `TaiChappyStrategy` (taichappy.cpp). Waits in its territory, chases the
//!   nearest visible Pikmin or captain, bites, shakes off latched Pikmin, cries for Spotty
//!   Bulborbs while chasing, and goes home when the target is lost or it strays too far.
//! - Spotty Bulborb: `TaiSwallowStrategy` (taiswallow.cpp). Sleeps until bumped, hit, or
//!   called by a Dwarf; then turns, chases inside its 180° view, bites up to three, gulps, and
//!   returns to its nest to sleep.
//! - Honeywisp: `TaiMizinkoStrategy` (taimizinko.cpp), reduced to hovering at its flight
//!   height and dropping nectar when hit, then flying off.
//!
//! Corpses become pellets with the disc's `pelMgr.bin` config (`tkch`, `tksw`).

use libpikmin::host::{CreatureFrame, CreatureId, CreaturePart};
use libpikmin::pellet_config::{self, PelletConfig};
use libpikmin::{PelletDesc, PelletId, PelletState, PikiState, Sim, Vec3};
use libpikmin_assets::teki::{TekiParams, teki_paths, tpf, tpi};
use libpikmin_assets::{AnimMgr, AnimSet, Assets, Skeleton};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TekiKind {
    Dwarf,
    Spotty,
    Honeywisp,
}

impl TekiKind {
    pub const ALL: [TekiKind; 3] = [TekiKind::Dwarf, TekiKind::Spotty, TekiKind::Honeywisp];

    /// `TekiMgr::typeNames`
    pub fn name(self) -> &'static str {
        match self {
            TekiKind::Dwarf => "chappy",
            TekiKind::Spotty => "swallow",
            TekiKind::Honeywisp => "qurione",
        }
    }

    /// Int parameter count (`TPI_COUNT` plus the type's own).
    fn int_count(self) -> usize {
        match self {
            TekiKind::Dwarf => 21,
            _ => 20,
        }
    }

    /// Corpse pellet config id (`TekiMgr::getTypeId`).
    fn corpse_id(self) -> Option<&'static str> {
        match self {
            TekiKind::Dwarf => Some("tkch"),
            TekiKind::Spotty => Some("tksw"),
            TekiKind::Honeywisp => None,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// First `SE_*` id of the kind's sound table; animation sound events index from it
    /// (`TaiChappySoundTable`: `SE_MINIC_WALK`, `TaiSwallowSoundTable`: `SE_CHAPPY_WALK`,
    /// `TaiMizinkoSoundTable`: `SE_KURIONE_HIT`).
    pub fn sound_base(self) -> &'static str {
        match self {
            TekiKind::Dwarf => "SE_MINIC_WALK",
            TekiKind::Spotty => "SE_CHAPPY_WALK",
            TekiKind::Honeywisp => "SE_KURIONE_HIT",
        }
    }
}

/// AnimMgr entries the strategies use for each kind (indices into `tekikeys/<name>.key`).
#[derive(Debug, Clone, Copy)]
pub struct Motions {
    pub dead: usize,
    pub hurt: usize,
    pub wait: usize,
    pub turn: usize,
    pub chase: usize,
    pub attack: usize,
    pub gulp: usize,
    pub flick: usize,
    pub sleep: usize,
    pub cry: usize,
}

impl Motions {
    fn of(kind: TekiKind) -> Self {
        match kind {
            // Wait1, WaitAct1, Move1, Attack, WaitAct2, Flick, Type2 (cry)
            TekiKind::Dwarf => Motions {
                dead: 0,
                hurt: 1,
                wait: 2,
                turn: 4,
                chase: 6,
                attack: 8,
                gulp: 5,
                flick: 9,
                sleep: 2,
                cry: 11,
            },
            // Sleeping plays slot 9 (`TaiMotionAction(.., TekiMotion::Flick)` in the sleep
            // state), and shaking off is Type1 (`TaiSwallowFlickingAction`).
            TekiKind::Spotty => Motions {
                dead: 0,
                hurt: 1,
                wait: 3,
                turn: 4,
                chase: 7,
                attack: 8,
                gulp: 5,
                flick: 10,
                sleep: 9,
                cry: 3,
            },
            TekiKind::Honeywisp => Motions {
                dead: 0,
                hurt: 1,
                wait: 2,
                turn: 2,
                chase: 0,
                attack: 2,
                gulp: 2,
                flick: 2,
                sleep: 2,
                cry: 2,
            },
        }
    }
}

/// Frame keys of one motion.
#[derive(Debug, Clone, Copy, Default)]
pub struct Keys {
    pub start: f32,
    pub end: f32,
    pub looping: Option<(f32, f32)>,
    pub action0: Option<f32>,
    pub action1: Option<f32>,
}

/// Everything the AI needs about a kind, from the disc.
#[derive(Debug, Clone)]
pub struct TekiSpec {
    pub kind: TekiKind,
    pub life: f32,
    pub scale: f32,
    pub run_speed: f32,
    /// Radians per second.
    pub turn_speed: f32,
    pub visible_range: f32,
    /// Full field of view, degrees.
    pub visible_angle: f32,
    pub attackable_range: f32,
    pub attackable_angle: f32,
    pub attack_range: f32,
    pub attack_power: f32,
    pub danger_territory: f32,
    pub safety_territory: f32,
    pub collision_radius: f32,
    pub message_range: f32,
    pub flight_height: f32,
    pub swallow_count: u32,
    pub flick_piki_counts: [u32; 3],
    pub flick_damage_counts: [u32; 4],
    /// Dwarf only: `CHAPPYPF_CryingPeriod`.
    pub crying_period: f32,
    pub motions: Motions,
    pub keys: Vec<Keys>,
    /// Sound events (frame, sound table index) per motion.
    pub sound_keys: Vec<Vec<(f32, u8)>>,
    pub corpse: Option<PelletConfig>,
    /// `TPF_CorpseSize` / `TPF_CorpseHeight` (`BTeki::viewGetBottomRadius` / `viewGetHeight`).
    pub corpse_size: f32,
    pub corpse_height: f32,
    /// Where the body lies in the last frame of the death motion, in the local frame and
    /// scaled. The game puts the corpse pellet on the body (its 'carc' sphere, else the
    /// centre: `BTeki::dieSoon`), so the slots ring the body rather than the feet.
    pub corpse_offset: Vec3,
}

/// The horizontal centre of the model's bounds at the end of motion `index`, local and unscaled.
fn pose_centre(assets: &Assets, kind: TekiKind, index: usize) -> Option<Vec3> {
    let [model, anm, key, _] = teki_paths(kind.name());
    let model = assets.model(&model).ok()?;
    let set = AnimSet::load(assets, &key, &anm).ok()?;
    let (info, anim) = set.get(index)?;
    let end = (*info.keys.last()? as f32).min(anim.frames.saturating_sub(1) as f32);
    let pose = Skeleton::new(&model).pose(glam::Mat4::IDENTITY, Some((anim, end)));
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for b in model.batches().ok()? {
        for v in &b.vertices {
            let p = pose.skin(v).0;
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo.x <= hi.x).then(|| ((lo + hi) * 0.5).with_y(0.0))
}

impl TekiSpec {
    pub fn load(assets: &Assets, kind: TekiKind, pellets: &[PelletConfig]) -> Result<Self, String> {
        let [_, _, key, para] = teki_paths(kind.name());
        let p = TekiParams::parse_any(&assets.read(&para).map_err(|e| e.to_string())?, kind.int_count())
            .map_err(|e| e.to_string())?;
        let mgr = AnimMgr::parse(&assets.read(&key).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let keys = mgr
            .infos
            .iter()
            .map(|i| {
                let k = |idx: Option<i32>| idx.and_then(|n| i.keys.get(n as usize)).map(|&f| f as f32);
                let of = |ty: u8| k(i.info_keys.iter().find(|(_, t)| *t == ty).map(|(n, _)| *n));
                Keys {
                    start: *i.keys.first().unwrap_or(&0) as f32,
                    end: *i.keys.last().unwrap_or(&0) as f32,
                    looping: of(0).zip(of(1)),
                    action0: of(2),
                    action1: of(3),
                }
            })
            .collect();
        let sound_keys = mgr
            .infos
            .iter()
            .map(|i| i.event_keys.iter().filter(|e| e.event == 0).map(|e| (e.frame as f32, e.value)).collect())
            .collect();
        let crying_period = if kind == TekiKind::Dwarf {
            p.f(tpf::SPECIFIC + 1)
        } else {
            0.0
        };
        Ok(TekiSpec {
            kind,
            life: p.f(tpf::LIFE),
            scale: p.f(tpf::SCALE),
            run_speed: p.f(tpf::RUN_VELOCITY),
            turn_speed: p.f(tpf::TURN_VELOCITY),
            visible_range: p.f(tpf::VISIBLE_RANGE),
            visible_angle: p.f(tpf::VISIBLE_ANGLE),
            attackable_range: p.f(tpf::ATTACKABLE_RANGE),
            attackable_angle: p.f(tpf::ATTACKABLE_ANGLE),
            attack_range: p.f(tpf::ATTACK_RANGE),
            attack_power: p.f(tpf::ATTACK_POWER),
            danger_territory: p.f(tpf::DANGER_TERRITORY_RANGE),
            safety_territory: p.f(tpf::SAFETY_TERRITORY_RANGE),
            collision_radius: p.f(tpf::COLLISION_RADIUS),
            message_range: p.f(tpf::MESSAGE_RANGE),
            flight_height: p.f(tpf::FLIGHT_HEIGHT),
            swallow_count: p.i(tpi::SWALLOW_COUNT).max(0) as u32,
            flick_piki_counts: [tpi::FLICK_PIKI_COUNT1, tpi::FLICK_PIKI_COUNT2, tpi::FLICK_PIKI_COUNT3]
                .map(|i| p.i(i).max(1) as u32),
            flick_damage_counts: [
                tpi::FLICK_DAMAGE_COUNT1,
                tpi::FLICK_DAMAGE_COUNT2,
                tpi::FLICK_DAMAGE_COUNT3,
                tpi::FLICK_DAMAGE_COUNT4,
            ]
            .map(|i| p.i(i).max(1) as u32),
            crying_period,
            motions: Motions::of(kind),
            keys,
            sound_keys,
            corpse: kind.corpse_id().and_then(|id| pellet_config::find(pellets, id)).cloned(),
            corpse_size: p.f(tpf::CORPSE_SIZE),
            corpse_height: p.f(tpf::CORPSE_HEIGHT),
            corpse_offset: pose_centre(assets, kind, Motions::of(kind).dead).unwrap_or_default() * p.f(tpf::SCALE),
        })
    }

    pub fn keys(&self, motion: usize) -> Keys {
        self.keys.get(motion).copied().unwrap_or(Keys {
            end: 30.0,
            ..Default::default()
        })
    }

    /// Collision parts in the creature's local frame (x right, y up, z forward), scaled.
    /// Sized from the models' bind poses: a body sphere (and a head for the Spotty).
    fn parts(&self) -> Vec<(Vec3, f32)> {
        match self.kind {
            TekiKind::Dwarf => vec![(Vec3::new(0.0, 14.0, 3.0), 13.0)],
            TekiKind::Spotty => vec![
                (Vec3::new(0.0, 42.0, -4.0), 34.0),
                (Vec3::new(0.0, 48.0, 36.0), 24.0),
            ],
            TekiKind::Honeywisp => vec![(Vec3::new(0.0, 4.0, 0.0), 14.0)],
        }
    }

    /// Where bites land, in the local frame.
    fn mouth(&self) -> (Vec3, f32) {
        match self.kind {
            TekiKind::Dwarf => (Vec3::new(0.0, 8.0, 18.0), self.attack_range + 8.0),
            TekiKind::Spotty => (Vec3::new(0.0, 22.0, 55.0), self.attack_range + 22.0),
            TekiKind::Honeywisp => (Vec3::ZERO, 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ai {
    Sleep,
    /// Spotty: bumped awake, finishing the sleep motion.
    Waking,
    /// Spotty: woken by a hit.
    Hurt,
    Wait,
    /// Turning toward the target before chasing.
    Turn,
    Chase,
    Attack { ate: bool, chewed: bool },
    Gulp,
    Flick { done: bool },
    /// Dwarf: crying for help.
    Cry,
    Home,
    Dead,
    /// Corpse handed to the sim as a pellet.
    Corpse(PelletId),
    /// Honeywisp: dropped its nectar and is leaving.
    FlyAway { timer: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Piki(libpikmin::PikiId),
    Captain(libpikmin::CaptainId),
}

#[derive(Debug, Clone)]
pub struct Teki {
    pub id: CreatureId,
    pub kind: TekiKind,
    pub pos: Vec3,
    pub yaw: f32,
    pub home: Vec3,
    pub hp: f32,
    pub ai: Ai,
    /// AnimMgr entry playing and its frame.
    pub motion: usize,
    pub frame: f32,
    pub target: Option<Target>,
    pub flash: f32,
    /// Sound table indices whose animation frames passed this update, for the host to play.
    pub sounds: Vec<u8>,
    /// Hits taken since the last shake (`TaiFlickAction` counts damage events).
    pub hits: u32,
    cry_timer: f32,
    /// Honeywisp wander phase.
    phase: f32,
}

fn dir(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

fn ang_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    if d > std::f32::consts::PI { d - std::f32::consts::TAU } else { d }
}

/// Local offsets are measured on the models at their disc scale.
fn local_to_world(pos: Vec3, yaw: f32, l: Vec3) -> Vec3 {
    let (s, c) = yaw.sin_cos();
    pos + Vec3::new(l.x * c + l.z * s, l.y, -l.x * s + l.z * c)
}

impl Teki {
    pub fn new(id: CreatureId, spec: &TekiSpec, pos: Vec3, yaw: f32) -> Self {
        let ai = match spec.kind {
            TekiKind::Spotty => Ai::Sleep,
            _ => Ai::Wait,
        };
        Teki {
            id,
            kind: spec.kind,
            pos: if spec.kind == TekiKind::Honeywisp { pos.with_y(spec.flight_height) } else { pos },
            yaw,
            home: pos,
            hp: spec.life,
            ai,
            motion: if ai == Ai::Sleep { spec.motions.sleep } else { spec.motions.wait },
            frame: 0.0,
            target: None,
            flash: 0.0,
            sounds: Vec::new(),
            hits: 0,
            cry_timer: spec.crying_period,
            phase: (id as f32) * 1.7,
        }
    }

    pub fn alive(&self) -> bool {
        !matches!(self.ai, Ai::Dead | Ai::Corpse(_) | Ai::FlyAway { .. })
    }

    pub fn parts(&self, spec: &TekiSpec) -> Vec<CreaturePart> {
        spec.parts()
            .into_iter()
            .map(|(l, r)| CreaturePart {
                centre: local_to_world(self.pos, self.yaw, l),
                radius: r,
                stickable: true,
            })
            .collect()
    }

    fn play(&mut self, motion: usize) {
        if self.motion != motion {
            self.motion = motion;
            self.frame = 0.0;
        }
    }

    /// Advances the motion. Returns true when a one-shot motion has reached its end.
    fn animate(&mut self, spec: &TekiSpec, dt: f32, looping: bool) -> bool {
        let k = spec.keys(self.motion);
        let before = self.frame;
        self.frame += 30.0 * dt;
        // `PaniAnimator::checkEventKeys`: events in [previous frame, current frame).
        let mut crossed = |from: f32, to: f32| {
            if let Some(keys) = spec.sound_keys.get(self.motion) {
                for &(f, v) in keys {
                    if from <= f && f < to {
                        self.sounds.push(v);
                    }
                }
            }
        };
        if looping {
            let (a, b) = k.looping.unwrap_or((k.start, k.end));
            if b > a && self.frame >= b {
                let wrapped = a + (self.frame - a) % (b - a);
                crossed(before, b);
                crossed(a, wrapped);
                self.frame = wrapped;
            } else {
                crossed(before, self.frame);
            }
            false
        } else {
            crossed(before, self.frame.min(k.end + 1.0));
            let done = self.frame >= k.end;
            if done {
                self.frame = k.end;
            }
            before < k.end && done
        }
    }

    fn turn_toward(&mut self, spec: &TekiSpec, to: Vec3, dt: f32) -> f32 {
        let d = to - self.pos;
        let want = d.x.atan2(d.z);
        let diff = ang_dist(want, self.yaw);
        let step = spec.turn_speed * dt;
        self.yaw += diff.clamp(-step, step);
        diff.abs()
    }
}

/// A target's position, if it is still fair game (alive, on the ground, not stuck in a mouth).
fn target_pos(sim: &Sim, t: Target) -> Option<Vec3> {
    match t {
        Target::Piki(id) => {
            let p = sim.pikis().get(id.0 as usize)?;
            let free = p.is_alive()
                && p.on_ground
                && p.state.latched_to().is_none()
                && !matches!(p.state, PikiState::Swallowed { .. } | PikiState::Pressed { .. } | PikiState::Held { .. });
            free.then_some(p.pos)
        }
        Target::Captain(id) => {
            let c = sim.captains().get(id.0 as usize)?;
            (!c.down).then_some(c.pos)
        }
    }
}

/// `TaiTargetVisibleNaviPikiAction`: the nearest Pikmin or captain within the visible range
/// and field of view.
fn nearest_visible(sim: &Sim, t: &Teki, spec: &TekiSpec) -> Option<Target> {
    let half = spec.visible_angle.to_radians() / 2.0;
    let mut best: Option<(f32, Target)> = None;
    let mut consider = |pos: Vec3, target: Target| {
        let d = (pos - t.pos).with_y(0.0);
        let dist = d.length();
        if dist > spec.visible_range {
            return;
        }
        if spec.visible_angle < 360.0 && ang_dist(d.x.atan2(d.z), t.yaw).abs() > half {
            return;
        }
        if best.is_none_or(|(b, _)| dist < b) {
            best = Some((dist, target));
        }
    };
    for c in sim.captains() {
        if let Some(p) = target_pos(sim, Target::Captain(c.id)) {
            consider(p, Target::Captain(c.id));
        }
    }
    for p in sim.pikis() {
        if let Some(pos) = target_pos(sim, Target::Piki(p.id)) {
            consider(pos, Target::Piki(p.id));
        }
    }
    best.map(|(_, t)| t)
}

/// Whether something is touching the body (`TaiTargetNearestCollisionAction`).
fn bumped(sim: &Sim, t: &Teki, spec: &TekiSpec) -> Option<Target> {
    let parts = t.parts(spec);
    let touching = |p: Vec3, r: f32| parts.iter().any(|s| (s.centre - p).length() < s.radius + r);
    for c in sim.captains().iter().filter(|c| !c.down) {
        if touching(c.pos + Vec3::Y * 10.0, 12.0) {
            return Some(Target::Captain(c.id));
        }
    }
    sim.pikis()
        .iter()
        .find(|p| p.is_alive() && touching(p.pos + Vec3::Y * 6.0, p.size))
        .map(|p| Target::Piki(p.id))
}

pub struct Tekis {
    pub specs: Vec<TekiSpec>,
    pub list: Vec<Teki>,
    next_id: CreatureId,
}

impl Tekis {
    pub fn new(specs: Vec<TekiSpec>) -> Self {
        Tekis {
            specs,
            list: Vec::new(),
            next_id: 0,
        }
    }

    pub fn spec(&self, kind: TekiKind) -> Option<&TekiSpec> {
        self.specs.iter().find(|s| s.kind == kind)
    }

    pub fn spawn(&mut self, kind: TekiKind, pos: Vec3, yaw: f32) {
        let id = self.next_id + 1;
        let Some(spec) = self.spec(kind) else { return };
        let t = Teki::new(id, spec, pos, yaw);
        self.next_id = id;
        self.list.push(t);
    }

    pub fn frame(&self, out: &mut CreatureFrame) {
        out.clear();
        for t in self.list.iter().filter(|t| t.alive()) {
            let Some(spec) = self.spec(t.kind) else { continue };
            let flying = t.kind == TekiKind::Honeywisp;
            out.push(t.id, t.pos, t.yaw, true, flying, &t.parts(spec));
        }
    }

    pub fn damage(&mut self, id: CreatureId, amount: f32) -> bool {
        let Some(t) = self.list.iter_mut().find(|t| t.id == id && t.alive()) else {
            return false;
        };
        t.hp -= amount;
        t.flash = 0.08;
        t.hits += 1;
        true
    }

    /// Wakes every sleeping Spotty Bulborb within `range` of `from` (a Dwarf's cry).
    fn wake_near(&mut self, from: Vec3, range: f32) {
        for t in &mut self.list {
            if t.kind == TekiKind::Spotty && t.ai == Ai::Sleep && (t.pos - from).length() < range {
                t.ai = Ai::Waking;
            }
        }
    }

    pub fn update(&mut self, dt: f32, sim: &mut Sim, force_shake: bool) {
        let mut cries = Vec::new();
        for i in 0..self.list.len() {
            let Some(spec) = self.specs.iter().find(|s| s.kind == self.list[i].kind).cloned() else {
                continue;
            };
            let t = &mut self.list[i];
            t.flash = (t.flash - dt).max(0.0);
            if t.alive() && t.hp <= 0.0 {
                t.ai = Ai::Dead;
                t.play(spec.motions.dead);
                // Freed Pikmin fall out (`PikiSwallowedState::exec`).
                sim.spit_out(t.id);
            }
            match spec.kind {
                TekiKind::Honeywisp => update_honeywisp(t, &spec, sim, dt),
                _ => {
                    if let Some(cry) = update_bulborb(t, &spec, sim, dt, force_shake) {
                        cries.push(cry);
                    }
                }
            }
        }
        for (pos, range) in cries {
            self.wake_near(pos, range);
        }
        // Corpses that were delivered are gone.
        self.list.retain(|t| match t.ai {
            Ai::Corpse(p) => sim.pellet(p).state != PelletState::Gone,
            Ai::FlyAway { timer } => timer > 0.0,
            _ => true,
        });
    }
}

/// Returns a cry (position, message range) when a Dwarf calls for help.
fn update_bulborb(t: &mut Teki, spec: &TekiSpec, sim: &mut Sim, dt: f32, force_shake: bool) -> Option<(Vec3, f32)> {
    let m = spec.motions;
    let mut cry = None;
    match t.ai {
        Ai::Corpse(p) => {
            let pel = sim.pellet(p);
            t.yaw = pel.yaw;
            t.pos = local_to_world(pel.pos, pel.yaw, -spec.corpse_offset).with_y(pel.pos.y);
            return None;
        }
        Ai::Dead => {
            if t.animate(spec, dt, false) {
                // `BTeki::dieSoon`: leave a corpse the Pikmin can carry.
                let c = spec.corpse.as_ref();
                // On the body, ringed at the corpse size scaled like the pellet's world matrix
                // (`Pellet::getSlotLocalPos`).
                let id = sim.spawn_pellet(PelletDesc {
                    pos: local_to_world(t.pos, t.yaw, spec.corpse_offset).with_y(t.pos.y),
                    yaw: t.yaw,
                    radius: (spec.corpse_size * spec.scale).max(5.0),
                    height: spec.corpse_height,
                    min_carriers: c.map_or(3, |c| c.min_carriers.max(1) as u32),
                    max_carriers: c.map_or(6, |c| c.max_carriers.max(1) as u32),
                    seeds_matching: c.map_or(4, |c| c.seeds_matching),
                    seeds_other: c.map_or(4, |c| c.seeds_other),
                    color: None,
                    creature: Some(t.id),
                });
                t.ai = Ai::Corpse(id);
            }
            return None;
        }
        _ => {}
    }

    // Shake off latched Pikmin (`TaiFlickAction`): enough hits for how many are stuck on.
    let latched = sim.pikis().iter().filter(|p| p.state.latched_to() == Some(t.id)).count() as u32;
    let tier = spec.flick_piki_counts.iter().filter(|&&c| latched >= c).count();
    let need = spec.flick_damage_counts[tier.min(3)];
    let awake = !matches!(t.ai, Ai::Sleep | Ai::Waking | Ai::Flick { .. } | Ai::Attack { .. });
    if (force_shake && latched > 0) || (awake && latched >= spec.flick_piki_counts[0] && t.hits >= need) {
        t.ai = Ai::Flick { done: false };
        t.play(m.flick);
    }
    if t.hits > 0 && matches!(t.ai, Ai::Sleep) {
        // `TaiTargetNearestAssailantAction` + `TaiDamageAction`: woken by a hit.
        t.ai = Ai::Hurt;
        t.play(m.hurt);
        t.target = nearest_visible(sim, t, &TekiSpec { visible_angle: 360.0, ..spec.clone() });
    }

    let target = t.target.and_then(|tg| target_pos(sim, tg).map(|p| (tg, p)));
    let from_home = (t.pos - t.home).with_y(0.0).length();
    let can_attack = |t: &Teki, p: Vec3| {
        let d = (p - t.pos).with_y(0.0);
        d.length() < spec.collision_radius + spec.attackable_range
            && ang_dist(d.x.atan2(d.z), t.yaw).abs() < spec.attackable_angle.to_radians()
    };

    match t.ai {
        Ai::Sleep => {
            t.play(m.sleep);
            t.animate(spec, dt, true);
            if let Some(b) = bumped(sim, t, spec) {
                t.target = Some(b);
                t.ai = Ai::Waking;
            }
        }
        Ai::Waking => {
            // `TaiFinishMotionAction`: play the sleep motion out, then turn to the target.
            t.play(m.sleep);
            t.frame = t.frame.max(spec.keys(m.sleep).looping.map_or(0.0, |l| l.1));
            if t.animate(spec, dt, false) {
                t.target = t.target.or_else(|| nearest_visible(sim, t, &TekiSpec { visible_angle: 360.0, ..spec.clone() }));
                t.ai = Ai::Turn;
            }
        }
        Ai::Hurt => {
            if t.animate(spec, dt, false) {
                t.ai = Ai::Turn;
            }
        }
        Ai::Wait => {
            t.play(m.wait);
            // Spotty: one Wait2 (`TaiContinuousMotionAction(.., Wait2)`), then back to sleep.
            if t.animate(spec, dt, t.kind != TekiKind::Spotty) {
                t.ai = Ai::Sleep;
            }
            if let Some(tg) = nearest_visible(sim, t, spec) {
                t.target = Some(tg);
                t.ai = if t.kind == TekiKind::Spotty { Ai::Turn } else { Ai::Chase };
            } else if from_home > spec.safety_territory {
                t.ai = Ai::Home;
            }
        }
        Ai::Turn => {
            t.play(m.turn);
            t.animate(spec, dt, true);
            match target {
                Some((_, p)) => {
                    if t.turn_toward(spec, p, dt) < 0.2 {
                        t.ai = Ai::Chase;
                    }
                }
                None => t.ai = Ai::Home,
            }
        }
        Ai::Chase => {
            t.play(m.chase);
            t.animate(spec, dt, true);
            let Some((_, p)) = target.filter(|(_, p)| (*p - t.pos).with_y(0.0).length() < spec.visible_range * 1.2)
            else {
                // `TaiTargetLostAction`
                t.target = nearest_visible(sim, t, spec);
                if t.target.is_none() {
                    t.ai = Ai::Home;
                }
                return None;
            };
            if from_home > spec.danger_territory {
                t.ai = Ai::Home;
                t.target = None;
                return None;
            }
            if can_attack(t, p) {
                t.ai = Ai::Attack { ate: false, chewed: false };
                t.play(m.attack);
                return None;
            }
            let off = t.turn_toward(spec, p, dt);
            // Spotty: `TaiOutOfTraceAngleAction` stops to turn when the target is far off-axis.
            if t.kind == TekiKind::Spotty && off > (45f32).to_radians() {
                t.ai = Ai::Turn;
            } else {
                t.pos += dir(t.yaw) * spec.run_speed * dt;
            }
            // Dwarf: cry every CRYING_PERIOD while chasing (`TaiChappyCryAction`).
            if t.kind == TekiKind::Dwarf {
                t.cry_timer -= dt;
                if t.cry_timer <= 0.0 {
                    t.cry_timer = spec.crying_period;
                    t.ai = Ai::Cry;
                    t.play(m.cry);
                }
            }
        }
        Ai::Cry => {
            if t.animate(spec, dt, false) {
                cry = Some((t.pos, spec.message_range));
                t.ai = Ai::Chase;
            }
        }
        Ai::Attack { mut ate, mut chewed } => {
            let k = spec.keys(m.attack);
            let done = t.animate(spec, dt, false);
            if !ate && t.frame >= k.action0.unwrap_or(k.end * 0.3) {
                // `TaiAnimationSwallowingAction`: whatever is in the mouth is caught.
                ate = true;
                let (local, r) = spec.mouth();
                let mouth = local_to_world(t.pos, t.yaw, local);
                sim.swallow_pikis(t.id, 0, mouth, r, spec.swallow_count.max(1));
                let caps: Vec<_> = sim
                    .captains()
                    .iter()
                    .filter(|c| !c.down && (c.pos - mouth).with_y(0.0).length() < r + 8.0)
                    .map(|c| c.id)
                    .collect();
                for c in caps {
                    sim.damage_captain(c, spec.attack_power);
                }
            }
            if !chewed && t.frame >= k.action1.unwrap_or(k.end) {
                chewed = true;
                sim.chew(t.id);
            }
            t.ai = if done {
                sim.chew(t.id);
                t.play(m.gulp);
                Ai::Gulp
            } else {
                Ai::Attack { ate, chewed }
            };
        }
        Ai::Gulp => {
            if t.animate(spec, dt, false) {
                t.ai = Ai::Chase;
                t.target = nearest_visible(sim, t, spec);
            }
        }
        Ai::Flick { mut done } => {
            let k = spec.keys(m.flick);
            let finished = t.animate(spec, dt, false);
            if !done && t.frame >= k.action0.unwrap_or(k.end * 0.4) {
                done = true;
                // Knock off everything latched and flick Pikmin standing close.
                sim.shake_off(t.id);
                let r = spec.collision_radius + 10.0;
                sim.flick_pikis_in_sphere(t.pos, r, 0.0, t.pos);
                t.hits = 0;
            }
            t.ai = if finished { Ai::Chase } else { Ai::Flick { done } };
            if finished {
                t.target = nearest_visible(sim, t, spec);
            }
        }
        Ai::Home => {
            t.play(m.chase);
            t.animate(spec, dt, true);
            if let Some(tg) = (from_home < spec.danger_territory * 0.8).then(|| nearest_visible(sim, t, spec)).flatten() {
                t.target = Some(tg);
                t.ai = if t.kind == TekiKind::Spotty { Ai::Turn } else { Ai::Chase };
            } else if from_home <= spec.safety_territory.max(5.0) {
                t.ai = Ai::Wait;
                t.frame = 0.0;
            } else {
                t.turn_toward(spec, t.home, dt);
                t.pos += dir(t.yaw) * spec.run_speed * dt;
            }
        }
        Ai::Dead | Ai::Corpse(_) | Ai::FlyAway { .. } => {}
    }
    cry
}

fn update_honeywisp(t: &mut Teki, spec: &TekiSpec, sim: &mut Sim, dt: f32) {
    match t.ai {
        Ai::FlyAway { timer } => {
            t.pos.y += 120.0 * dt;
            t.ai = Ai::FlyAway { timer: timer - dt };
            t.animate(spec, dt, true);
        }
        Ai::Dead => {
            // Hit: drop the nectar (`TaiMizinko`'s drop-water state) and leave.
            sim.spawn_nectar(t.pos);
            t.ai = Ai::FlyAway { timer: 2.5 };
            t.play(spec.motions.hurt);
        }
        _ => {
            t.play(spec.motions.wait);
            t.animate(spec, dt, true);
            t.phase += dt * 0.4;
            let around = t.home + Vec3::new(t.phase.cos() * 60.0, 0.0, t.phase.sin() * 60.0);
            let to = around.with_y(spec.flight_height) - t.pos;
            t.pos += to * (dt * 1.5).min(1.0);
            t.yaw = to.x.atan2(to.z);
        }
    }
}
