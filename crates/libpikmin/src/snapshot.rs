//! Replicating a sim to other machines, for a networked host.
//!
//! The server runs the one real [`Sim`]. Each network tick it takes a [`Snapshot`] of every
//! entity (captains, Pikmin, pellets, Onions, sprouts, nectar) plus the events since the last one,
//! and sends [`Snapshot::encode_replica`]'s bytes. A client decodes it and lays it over a replica `Sim`
//! with [`Sim::apply_snapshot`] (or blends two with [`Sim::apply_snapshot_blend`] for smooth
//! motion between network ticks); the presentation layer then draws and sounds the replica as it
//! would the real thing, since it only reads the sim's entities and the events.
//!
//! [`Snapshot::encode`] retains every field for exact simulation-state round trips. The replica
//! wire form drops AI bookkeeping that clients never step, then compresses the repeated fields.
//! The format is for machines running the same build; decoding refuses malformed input.

use std::collections::VecDeque;

use glam::{Vec2, Vec3};

use crate::bulb::{Bulb, BulbId, BulbState};
use crate::captain::{Captain, CaptainInput, CaptainPose, LockView, Pressed, Punch, ThrowState, WhistleState};
use crate::carry::{Carry, CarryPhase, Pellet, PelletDesc, PelletId, PelletState};
use crate::damage::DeathCause;
use crate::event::Event;
use crate::formation::Plate;
use crate::jump::{Jump, JumpCue, MAX_AIR_JUMPS};
use crate::nectar::{DropState, NectarDrop, NectarId, Puddle, PuddleState};
use crate::onion::{Onion, OnionAnim, OnionId, Pluck, PluckPhase, Ship, Sprout, SproutId, SproutState};
use crate::piki::{Attack, AttackMode, GuidedThrow, Happa, KnockPhase, Piki, PikiColor, PikiState};
use crate::plucker::{Plucker, PluckerJob};
use crate::sim::Sim;
use crate::strike::Strike;
use crate::{CaptainId, PikiId};

/// Bumped when the encoding changes.
pub const SNAPSHOT_VERSION: u8 = 10;

// Far more than any real sim has; a corrupt length can't make decoding allocate wildly.
const MAX_ENTITIES: usize = 4096;
const MAX_PATH: usize = 512;
// A malformed compressed packet must not expand into an unbounded allocation.
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

/// Everything about a sim that a replica needs, at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub tick: u64,
    pub piki_deaths: u64,
    pub captains: Vec<Captain>,
    pub pikis: Vec<Piki>,
    pub pellets: Vec<Pellet>,
    pub onions: Vec<Onion>,
    pub sprouts: Vec<Option<Sprout>>,
    pub nectar_drops: Vec<NectarDrop>,
    pub puddles: Vec<Puddle>,
    pub next_nectar: u32,
    /// Candypop Buds ([`crate::bulb`]).
    pub bulbs: Vec<Bulb>,
    pub next_bulb: u32,
    /// Where the host's creatures were (id, position, yaw), so a replica can place the sounds of
    /// what they do. Not their parts: nothing on a replica latches onto them.
    pub creatures: Vec<(u64, Vec3, f32)>,
    /// What happened since the snapshot before (sounds, effects and animation cues).
    pub events: Vec<Event>,
}

impl Snapshot {
    /// The sim's entities now, with `events` as what happened since the last snapshot.
    pub fn of(sim: &Sim, events: &[Event]) -> Snapshot {
        Snapshot {
            tick: sim.tick,
            piki_deaths: sim.piki_deaths,
            captains: sim.captains.clone(),
            pikis: sim.pikis.clone(),
            pellets: sim.pellets.clone(),
            onions: sim.onions.clone(),
            sprouts: sim.sprouts.clone(),
            nectar_drops: sim.nectar_drops.clone(),
            puddles: sim.puddles.clone(),
            next_nectar: sim.next_nectar,
            bulbs: sim.bulbs.clone(),
            next_bulb: sim.next_bulb,
            creatures: sim.creatures.creatures.iter().map(|c| (c.id, c.pos, c.yaw)).collect(),
            events: events.to_vec(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let raw = self.encode_raw();
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        let mut wire = Vec::with_capacity(6 + compressed.len().min(raw.len()));
        wire.push(SNAPSHOT_VERSION);
        if compressed.len() < raw.len() {
            wire.push(1);
            wire.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            wire.extend_from_slice(&compressed);
        } else {
            wire.push(0);
            wire.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            wire.extend_from_slice(&raw);
        }
        wire
    }

    /// Wire form for a replica, which never steps its AI. Keep what presentation and host
    /// queries read, while discarding server-only decisions and formation bookkeeping before
    /// compression. The ordinary `encode` still round-trips the complete simulation state.
    pub fn encode_replica(&self) -> Vec<u8> {
        let mut visible = self.clone();
        for c in &mut visible.captains {
            c.plate.members.clear();
            c.plate.slot_offsets.clear();
        }
        for p in &mut visible.pikis {
            p.target_vel = Vec3::ZERO;
            p.jump_queue.clear();
            p.blooms = 0;
            p.dismissed = false;
            p.idle_time = 0.0;
            p.latched_at = 0;
            p.guided_throw = None;
            p.hit_log.fill(None);
            p.hit_next = 0;
            p.kill_credit = 0;
        }
        visible.encode()
    }

    fn encode_raw(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u64(self.tick);
        w.u64(self.piki_deaths);
        w.u32(self.next_nectar);
        w.len(self.captains.len());
        for c in &self.captains {
            w_captain(&mut w, c);
        }
        w.len(self.pikis.len());
        for p in &self.pikis {
            w_piki(&mut w, p);
        }
        w.len(self.pellets.len());
        for p in &self.pellets {
            w_pellet(&mut w, p);
        }
        w.len(self.onions.len());
        for o in &self.onions {
            w_onion(&mut w, o);
        }
        w.len(self.sprouts.len());
        for s in &self.sprouts {
            w.opt(s.as_ref(), w_sprout);
        }
        w.len(self.nectar_drops.len());
        for d in &self.nectar_drops {
            w_drop(&mut w, d);
        }
        w.len(self.puddles.len());
        for p in &self.puddles {
            w_puddle(&mut w, p);
        }
        w.u32(self.next_bulb);
        w.len(self.bulbs.len());
        for b in &self.bulbs {
            w_bulb(&mut w, b);
        }
        w.len(self.creatures.len());
        for (id, pos, yaw) in &self.creatures {
            w.u64(*id);
            w.vec3(*pos);
            w.f32(*yaw);
        }
        w.len(self.events.len());
        for e in &self.events {
            w_event(&mut w, e);
        }
        w.0
    }

    /// None if the bytes aren't a snapshot of this version, or are cut short or inconsistent.
    pub fn decode(bytes: &[u8]) -> Option<Snapshot> {
        if bytes.first().copied()? != SNAPSHOT_VERSION || bytes.len() > MAX_SNAPSHOT_BYTES {
            return None;
        }
        let body_len = u32::from_le_bytes(bytes.get(2..6)?.try_into().ok()?) as usize;
        let body = bytes.get(6..)?;
        if body.len() != body_len {
            return None;
        }
        let raw = match bytes.get(1).copied()? {
            0 => body.to_vec(),
            1 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(body, MAX_SNAPSHOT_BYTES).ok()?,
            _ => return None,
        };
        Self::decode_raw(&raw)
    }

    fn decode_raw(bytes: &[u8]) -> Option<Snapshot> {
        let mut r = Reader { data: bytes, pos: 0 };
        let tick = r.u64()?;
        let piki_deaths = r.u64()?;
        let next_nectar = r.u32()?;
        let n = r.len()?;
        let captains = (0..n).map(|_| r_captain(&mut r)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let pikis = (0..n).map(|_| r_piki(&mut r)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let pellets = (0..n).map(|_| r_pellet(&mut r)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let onions = (0..n).map(|_| r_onion(&mut r)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let sprouts = (0..n).map(|_| r.opt(r_sprout)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let nectar_drops = (0..n).map(|_| r_drop(&mut r)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let puddles = (0..n).map(|_| r_puddle(&mut r)).collect::<Option<Vec<_>>>()?;
        let next_bulb = r.u32()?;
        let n = r.len()?;
        let bulbs = (0..n).map(|_| r_bulb(&mut r)).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let creatures = (0..n).map(|_| Some((r.u64()?, r.vec3()?, r.f32()?))).collect::<Option<Vec<_>>>()?;
        let n = r.len()?;
        let events = (0..n).map(|_| r_event(&mut r)).collect::<Option<Vec<_>>>()?;
        (r.pos == bytes.len()).then_some(Snapshot {
            tick,
            piki_deaths,
            captains,
            pikis,
            pellets,
            onions,
            sprouts,
            nectar_drops,
            puddles,
            next_nectar,
            bulbs,
            next_bulb,
            creatures,
            events,
        })
    }
}

impl Sim {
    /// Makes this sim's entities the snapshot's. For a replica: it doesn't step, so nothing here
    /// is simulated, only laid out for the presentation layer. The events go to the caller.
    pub fn apply_snapshot(&mut self, s: &Snapshot) {
        self.reusable_pikis.clear();
        self.tick = s.tick;
        self.piki_deaths = s.piki_deaths;
        self.captains.clone_from(&s.captains);
        self.pikis.clone_from(&s.pikis);
        self.pellets.clone_from(&s.pellets);
        self.onions.clone_from(&s.onions);
        self.sprouts.clone_from(&s.sprouts);
        self.nectar_drops.clone_from(&s.nectar_drops);
        self.puddles.clone_from(&s.puddles);
        self.next_nectar = s.next_nectar;
        self.bulbs.clone_from(&s.bulbs);
        self.next_bulb = s.next_bulb;
        self.creatures.clear();
        for &(id, pos, yaw) in &s.creatures {
            self.creatures.push(id, pos, yaw, true, false, &[]);
        }
    }

    /// A replica's captain shown as another sim's (a client predicting his own movement locally):
    /// where he is, how he moves and where his cursor is come from `from`, so his own controls feel
    /// instant; what he is doing with the squad (throwing, whistling, plucking) stays the server's.
    /// `pos` and `face` override `from`'s when given (the host's smoothed body).
    pub fn overlay_captain_motion(&mut self, id: CaptainId, from: &Captain, pos: Option<Vec3>, face: Option<f32>) {
        let Some(c) = self.captains.iter_mut().find(|c| c.id == id) else { return };
        c.pos = pos.unwrap_or(from.pos);
        c.face = face.unwrap_or(from.face);
        c.vel = from.vel;
        c.target_vel = from.target_vel;
        c.on_ground = from.on_ground;
        c.jump = from.jump;
        c.cursor = from.cursor;
        c.cursor_world = from.cursor_world;
        c.neutral_time = from.neutral_time;
    }

    /// As [`Sim::apply_snapshot`] of `to`, but positions and facings are `t` (0..1) of the way
    /// from where `from` had them (entities it also has, by id), so motion is smooth at the rate
    /// the snapshots arrive. Anything new in `to`, or with a different id order, is as `to` has it.
    pub fn apply_snapshot_blend(&mut self, from: &Snapshot, to: &Snapshot, t: f32) {
        self.apply_snapshot(to);
        self.blend_snapshot_motion(from, to, t);
    }

    /// Render-only interpolation after `apply_snapshot(to)`, without recloning AI state.
    /// Always use the authoritative endpoints, never the previous rendered position.
    pub fn blend_snapshot_motion(&mut self, from: &Snapshot, to: &Snapshot, t: f32) {
        let t = t.clamp(0.0, 1.0);
        for (c, b) in self.captains.iter_mut().zip(&to.captains) {
            c.pos = b.pos;
            c.face = b.face;
            if let Some(a) = from.captains.iter().find(|a| a.id == c.id) {
                blend_pos(&mut c.pos, a.pos, t, c.vel);
                c.face = lerp_angle(a.face, c.face, t);
            }
        }
        // Ids are indices into the vectors on the server, so they line up by position too.
        for (p, b) in self.pikis.iter_mut().zip(&to.pikis) {
            p.pos = b.pos;
            p.face = b.face;
            if let Some(a) = from.pikis.get(p.id.0 as usize).filter(|a| a.id == p.id && a.generation == p.generation) {
                blend_pos(&mut p.pos, a.pos, t, p.vel);
                p.face = lerp_angle(a.face, p.face, t);
            }
        }
        for (p, b) in self.pellets.iter_mut().zip(&to.pellets) {
            p.pos = b.pos;
            p.yaw = b.yaw;
            if let Some(a) = from.pellets.get(p.id.0 as usize).filter(|a| a.id == p.id) {
                blend_pos(&mut p.pos, a.pos, t, Vec3::ZERO);
                p.yaw = lerp_angle(a.yaw, p.yaw, t);
            }
        }
        for (o, b) in self.onions.iter_mut().zip(&to.onions) {
            o.pos = b.pos;
            if let Some(a) = from.onions.get(o.id.0 as usize).filter(|a| a.id == o.id) {
                blend_pos(&mut o.pos, a.pos, t, Vec3::ZERO);
            }
        }
        for s in self.sprouts.iter_mut().flatten() {
            if let Some(Some(b)) = to.sprouts.get(s.id.0 as usize) { s.pos = b.pos; }
            if let Some(Some(a)) = from.sprouts.get(s.id.0 as usize).filter(|a| a.as_ref().is_some_and(|a| a.id == s.id)) {
                blend_pos(&mut s.pos, a.pos, t, s.vel);
            }
        }
    }
}

/// `pos` moved `t` of the way from `from`, unless that is a jump too big to be motion (a teleport,
/// a Pikmin popping out of an Onion) where it just stays put.
fn blend_pos(pos: &mut Vec3, from: Vec3, t: f32, _vel: Vec3) {
    if pos.distance_squared(from) < 60.0 * 60.0 {
        *pos = from.lerp(*pos, t);
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d * t
}

// ---- bytes -------------------------------------------------------------------------------------

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn bool(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u16(n.min(u16::MAX as usize) as u16);
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }
    fn opt<T>(&mut self, v: Option<&T>, f: impl FnOnce(&mut Writer, &T)) {
        match v {
            Some(x) => {
                self.u8(1);
                f(self, x);
            }
            None => self.u8(0),
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    /// Any f32 bit pattern; non-finite ones are the sender's business.
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn len(&mut self) -> Option<usize> {
        let n = self.u16()? as usize;
        (n <= MAX_ENTITIES).then_some(n)
    }
    fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn vec2(&mut self) -> Option<Vec2> {
        Some(Vec2::new(self.f32()?, self.f32()?))
    }
    fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<Option<T>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(f(self)?)),
            _ => None,
        }
    }
}

// ---- small types -----------------------------------------------------------------------------

fn w_color(w: &mut Writer, c: PikiColor) {
    w.u8(c.index() as u8);
}

fn r_color(r: &mut Reader) -> Option<PikiColor> {
    let i = r.u8()? as usize;
    PikiColor::from_id(i)
}

fn w_happa(w: &mut Writer, h: Happa) {
    w.u8(h as u8);
}

fn r_happa(r: &mut Reader) -> Option<Happa> {
    match r.u8()? {
        0 => Some(Happa::Leaf),
        1 => Some(Happa::Bud),
        2 => Some(Happa::Flower),
        _ => None,
    }
}

fn w_jump(w: &mut Writer, j: &Jump) {
    w.f32(j.elapsed);
    w.vec3(j.horizontal);
    w.opt(j.gravity.as_ref(), |w, g| w.f32(*g));
    for a in j.air {
        w.f32(a);
    }
    w.u8(j.air_count);
    w.u8(j.air_done);
    w.opt(j.cue_tick.as_ref(), |w, t| w.u64(*t));
    w.bool(j.stomping);
}

fn r_jump(r: &mut Reader) -> Option<Jump> {
    let elapsed = r.f32()?;
    let horizontal = r.vec3()?;
    let gravity = r.opt(|r| r.f32())?;
    let mut air = [0.0; MAX_AIR_JUMPS];
    for a in &mut air {
        *a = r.f32()?;
    }
    Some(Jump {
        elapsed,
        horizontal,
        gravity,
        air,
        air_count: r.u8()?,
        air_done: r.u8()?,
        cue_tick: r.opt(|r| r.u64())?,
        stomping: r.bool()?,
    })
}

fn w_cue(w: &mut Writer, c: &JumpCue) {
    w.vec3(c.origin);
    w.vec3(c.horizontal);
    w.opt(c.launch.as_ref(), |w, (a, b)| {
        w.f32(*a);
        w.f32(*b);
    });
    w.u64(c.tick);
    for a in c.air {
        w.f32(a);
    }
    w.u8(c.air_count);
}

fn r_cue(r: &mut Reader) -> Option<JumpCue> {
    let origin = r.vec3()?;
    let horizontal = r.vec3()?;
    let launch = r.opt(|r| Some((r.f32()?, r.f32()?)))?;
    let tick = r.u64()?;
    let mut air = [0.0; MAX_AIR_JUMPS];
    for a in &mut air {
        *a = r.f32()?;
    }
    Some(JumpCue { origin, horizontal, launch, tick, air, air_count: r.u8()? })
}

// ---- Pikmin ------------------------------------------------------------------------------------

fn w_attack_mode(w: &mut Writer, m: &AttackMode) {
    match *m {
        AttackMode::Approach => w.u8(0),
        AttackMode::JumpPrep => w.u8(1),
        AttackMode::Jumping => w.u8(2),
        AttackMode::StrikeRush => w.u8(5),
        AttackMode::StrikeJump { remaining } => { w.u8(6); w.f32(remaining); }
        AttackMode::Swing { frame, hit } => {
            w.u8(3);
            w.f32(frame);
            w.bool(hit);
        }
        AttackMode::Latched { part, offset, frame, hit } => {
            w.u8(4);
            w.u32(part);
            w.vec3(offset);
            w.f32(frame);
            w.bool(hit);
        }
    }
}

fn r_attack_mode(r: &mut Reader) -> Option<AttackMode> {
    Some(match r.u8()? {
        0 => AttackMode::Approach,
        1 => AttackMode::JumpPrep,
        2 => AttackMode::Jumping,
        5 => AttackMode::StrikeRush,
        6 => AttackMode::StrikeJump { remaining: r.f32()? },
        3 => AttackMode::Swing { frame: r.f32()?, hit: r.bool()? },
        4 => AttackMode::Latched { part: r.u32()?, offset: r.vec3()?, frame: r.f32()?, hit: r.bool()? },
        _ => return None,
    })
}

fn w_knock_phase(w: &mut Writer, p: &KnockPhase) {
    let (tag, v) = match *p {
        KnockPhase::Flying { frame } => (0, frame),
        KnockPhase::Landing { frame } => (1, frame),
        KnockPhase::Down { timer } => (2, timer),
        KnockPhase::GettingUp { frame } => (3, frame),
    };
    w.u8(tag);
    w.f32(v);
}

fn r_knock_phase(r: &mut Reader) -> Option<KnockPhase> {
    let tag = r.u8()?;
    let v = r.f32()?;
    Some(match tag {
        0 => KnockPhase::Flying { frame: v },
        1 => KnockPhase::Landing { frame: v },
        2 => KnockPhase::Down { timer: v },
        3 => KnockPhase::GettingUp { frame: v },
        _ => return None,
    })
}

fn w_carry_phase(w: &mut Writer, p: &CarryPhase) {
    match *p {
        CarryPhase::Go { progress } => {
            w.u8(0);
            w.u32(progress);
        }
        CarryPhase::Lift { frame, lifted } => {
            w.u8(1);
            w.f32(frame);
            w.bool(lifted);
        }
        CarryPhase::Move => w.u8(2),
    }
}

fn r_carry_phase(r: &mut Reader) -> Option<CarryPhase> {
    Some(match r.u8()? {
        0 => CarryPhase::Go { progress: r.u32()? },
        1 => CarryPhase::Lift { frame: r.f32()?, lifted: r.bool()? },
        2 => CarryPhase::Move,
        _ => return None,
    })
}

fn w_pluck_job(w: &mut Writer, j: &PluckerJob) {
    match *j {
        PluckerJob::Descend { leg, ratio } => {
            w.u8(0);
            w.u8(leg);
            w.f32(ratio);
        }
        PluckerJob::Walk { sprout, best, stuck } => {
            w.u8(1);
            w.u32(sprout.0);
            w.f32(best);
            w.f32(stuck);
        }
        PluckerJob::Pull { sprout, frame, popped } => {
            w.u8(2);
            w.u32(sprout.0);
            w.f32(frame);
            w.bool(popped);
        }
    }
}

fn r_pluck_job(r: &mut Reader) -> Option<PluckerJob> {
    Some(match r.u8()? {
        0 => PluckerJob::Descend { leg: r.u8()?, ratio: r.f32()? },
        1 => PluckerJob::Walk { sprout: SproutId(r.u32()?), best: r.f32()?, stuck: r.f32()? },
        2 => PluckerJob::Pull { sprout: SproutId(r.u32()?), frame: r.f32()?, popped: r.bool()? },
        _ => return None,
    })
}

fn w_state(w: &mut Writer, s: &PikiState) {
    match s {
        PikiState::Idle => w.u8(0),
        PikiState::Squad { lost_timer, near_counter, reslotted, formed } => {
            w.u8(1);
            w.f32(*lost_timer);
            w.u8(*near_counter);
            w.bool(*reslotted);
            w.bool(*formed);
        }
        PikiState::GoHang { captain } => {
            w.u8(2);
            w.u32(captain.0);
        }
        PikiState::Held { captain } => {
            w.u8(3);
            w.u32(captain.0);
        }
        PikiState::Flying { ground_frames } => {
            w.u8(4);
            w.u8(*ground_frames);
        }
        PikiState::Attack(a) => {
            w.u8(5);
            w.u64(a.target);
            w_attack_mode(w, &a.mode);
        }
        PikiState::Knocked { phase, whistled } => {
            w.u8(6);
            w_knock_phase(w, phase);
            w.opt(whistled.as_ref(), |w, c| w.u32(c.0));
        }
        PikiState::Carry(c) => {
            w.u8(7);
            w.u32(c.pellet.0);
            w.u32(c.slot);
            w_carry_phase(w, &c.phase);
        }
        PikiState::Pressed { stun, death_timer } => {
            w.u8(8);
            w.f32(*stun);
            w.f32(*death_timer);
        }
        PikiState::Swallowed { creature, part, offset } => {
            w.u8(9);
            w.u64(*creature);
            w.u32(*part);
            w.vec3(*offset);
        }
        PikiState::Dying { frame, end } => {
            w.u8(10);
            w.f32(*frame);
            w.f32(*end);
        }
        PikiState::Fading { timer } => {
            w.u8(11);
            w.f32(*timer);
        }
        PikiState::Absorb { puddle, frame, drinking, drank, finishing } => {
            w.u8(12);
            w.u32(puddle.0);
            w.f32(*frame);
            w.bool(*drinking);
            w.bool(*drank);
            w.bool(*finishing);
        }
        PikiState::GrowUp { frame, second, bloomed } => {
            w.u8(13);
            w.f32(*frame);
            w.bool(*second);
            w.bool(*bloomed);
        }
        PikiState::Enter { onion, leg, climbing, ratio } => {
            w.u8(14);
            w.u32(onion.0);
            w.u8(*leg);
            w.bool(*climbing);
            w.f32(*ratio);
        }
        PikiState::Exit { onion, leg, ratio } => {
            w.u8(15);
            w.u32(onion.0);
            w.u8(*leg);
            w.f32(*ratio);
        }
        PikiState::Plucker(p) => {
            w.u8(16);
            w.u32(p.onion.0);
            w_pluck_job(w, &p.job);
        }
        PikiState::Dead => w.u8(17),
    }
}

fn r_state(r: &mut Reader) -> Option<PikiState> {
    Some(match r.u8()? {
        0 => PikiState::Idle,
        1 => PikiState::Squad {
            lost_timer: r.f32()?,
            near_counter: r.u8()?,
            reslotted: r.bool()?,
            formed: r.bool()?,
        },
        2 => PikiState::GoHang { captain: CaptainId(r.u32()?) },
        3 => PikiState::Held { captain: CaptainId(r.u32()?) },
        4 => PikiState::Flying { ground_frames: r.u8()? },
        5 => PikiState::Attack(Attack { target: r.u64()?, mode: r_attack_mode(r)? }),
        6 => PikiState::Knocked { phase: r_knock_phase(r)?, whistled: r.opt(|r| Some(CaptainId(r.u32()?)))? },
        7 => PikiState::Carry(Carry { pellet: PelletId(r.u32()?), slot: r.u32()?, phase: r_carry_phase(r)? }),
        8 => PikiState::Pressed { stun: r.f32()?, death_timer: r.f32()? },
        9 => PikiState::Swallowed { creature: r.u64()?, part: r.u32()?, offset: r.vec3()? },
        10 => PikiState::Dying { frame: r.f32()?, end: r.f32()? },
        11 => PikiState::Fading { timer: r.f32()? },
        12 => PikiState::Absorb {
            puddle: NectarId(r.u32()?),
            frame: r.f32()?,
            drinking: r.bool()?,
            drank: r.bool()?,
            finishing: r.bool()?,
        },
        13 => PikiState::GrowUp { frame: r.f32()?, second: r.bool()?, bloomed: r.bool()? },
        14 => PikiState::Enter { onion: OnionId(r.u32()?), leg: r.u8()?, climbing: r.bool()?, ratio: r.f32()? },
        15 => PikiState::Exit { onion: OnionId(r.u32()?), leg: r.u8()?, ratio: r.f32()? },
        16 => PikiState::Plucker(Plucker { onion: OnionId(r.u32()?), job: r_pluck_job(r)? }),
        17 => PikiState::Dead,
        _ => return None,
    })
}

fn w_piki(w: &mut Writer, p: &Piki) {
    w.u32(p.id.0);
    w.u32(p.generation);
    w_color(w, p.color);
    w_happa(w, p.happa);
    w.vec3(p.pos);
    w.vec3(p.vel);
    w.vec3(p.target_vel);
    w.f32(p.face);
    w.f32(p.size);
    w.bool(p.on_ground);
    w.opt(p.jump.as_ref(), w_jump);
    w.len(p.jump_queue.len());
    for c in &p.jump_queue {
        w_cue(w, c);
    }
    w.opt(p.captain.as_ref(), |w, c| w.u32(c.0));
    w_state(w, &p.state);
    w.f32(p.health);
    w.u8(p.blooms);
    w.bool(p.dismissed);
    w.f32(p.idle_time);
    w.u64(p.latched_at);
    w.opt(p.guided_throw.as_ref(), |w, g| {
        w.opt(g.creature.as_ref(), |w, c| w.u64(*c));
        w.opt(g.point.as_ref(), |w, v| w.vec3(*v));
        w.f32(g.remaining);
    });
    for h in &p.hit_log {
        w.opt(h.as_ref(), |w, c| w.u64(*c));
    }
    w.u8(p.hit_next);
    w.u8(p.kill_credit);
    w.f32(p.frozen);
}

fn r_piki(r: &mut Reader) -> Option<Piki> {
    let id = PikiId(r.u32()?);
    let generation = r.u32()?;
    let color = r_color(r)?;
    let happa = r_happa(r)?;
    let pos = r.vec3()?;
    let vel = r.vec3()?;
    let target_vel = r.vec3()?;
    let face = r.f32()?;
    let size = r.f32()?;
    let on_ground = r.bool()?;
    let jump = r.opt(r_jump)?;
    let n = r.len()?;
    let jump_queue: VecDeque<JumpCue> = (0..n).map(|_| r_cue(r)).collect::<Option<_>>()?;
    let captain = r.opt(|r| Some(CaptainId(r.u32()?)))?;
    let state = r_state(r)?;
    let health = r.f32()?;
    let blooms = r.u8()?;
    let dismissed = r.bool()?;
    let idle_time = r.f32()?;
    let latched_at = r.u64()?;
    let guided_throw = r.opt(|r| {
        Some(GuidedThrow { creature: r.opt(|r| r.u64())?, point: r.opt(|r| r.vec3())?, remaining: r.f32()? })
    })?;
    let mut hit_log = [None; crate::growth::HIT_LOG];
    for h in &mut hit_log {
        *h = r.opt(|r| r.u64())?;
    }
    Some(Piki {
        id,
        generation,
        color,
        happa,
        pos,
        vel,
        target_vel,
        face,
        size,
        on_ground,
        jump,
        jump_queue,
        captain,
        state,
        health,
        blooms,
        dismissed,
        idle_time,
        latched_at,
        guided_throw,
        hit_log,
        hit_next: r.u8()?,
        kill_credit: r.u8()?,
        frozen: r.f32()?,
    })
}

// ---- captains --------------------------------------------------------------------------------

fn w_plate(w: &mut Writer, p: &Plate) {
    w.len(p.members.len());
    for m in &p.members {
        w.u32(m.0);
    }
    w.len(p.slot_offsets.len());
    for o in &p.slot_offsets {
        w.vec3(*o);
    }
    w.f32(p.size);
    w.f32(p.length);
    w.f32(p.inner_radius);
    w.f32(p.direction);
    w.vec3(p.centre);
    w.vec3(p.offset);
    w.vec3(p.velocity);
}

fn r_plate(r: &mut Reader) -> Option<Plate> {
    let n = r.len()?;
    let members = (0..n).map(|_| Some(PikiId(r.u32()?))).collect::<Option<Vec<_>>>()?;
    let n = r.len()?;
    let slot_offsets = (0..n).map(|_| r.vec3()).collect::<Option<Vec<_>>>()?;
    Some(Plate {
        members,
        slot_offsets,
        size: r.f32()?,
        length: r.f32()?,
        inner_radius: r.f32()?,
        direction: r.f32()?,
        centre: r.vec3()?,
        offset: r.vec3()?,
        velocity: r.vec3()?,
    })
}

fn w_pluck(w: &mut Writer, p: &Pluck) {
    w.u32(p.sprout.0);
    match p.phase {
        PluckPhase::Adjust { approach, face, odo_timer, odo_dist } => {
            w.u8(0);
            w.vec3(approach);
            w.f32(face);
            w.f32(odo_timer);
            w.f32(odo_dist);
        }
        PluckPhase::Pull { frame, popped, let_go, again, fast } => {
            w.u8(1);
            w.f32(frame);
            w.bool(popped);
            w.bool(let_go);
            w.bool(again);
            w.bool(fast);
        }
    }
}

fn r_pluck(r: &mut Reader) -> Option<Pluck> {
    let sprout = SproutId(r.u32()?);
    let phase = match r.u8()? {
        0 => PluckPhase::Adjust { approach: r.vec3()?, face: r.f32()?, odo_timer: r.f32()?, odo_dist: r.f32()? },
        1 => PluckPhase::Pull {
            frame: r.f32()?,
            popped: r.bool()?,
            let_go: r.bool()?,
            again: r.bool()?,
            fast: r.bool()?,
        },
        _ => return None,
    };
    Some(Pluck { sprout, phase })
}

fn w_input(w: &mut Writer, i: &CaptainInput) {
    w.vec2(i.stick);
    w.bool(i.whistle);
    w.bool(i.throw);
    w.bool(i.dismiss);
    w.vec2(i.swarm);
    w.opt(i.pose.as_ref(), |w, p| {
        w.vec3(p.pos);
        w.vec3(p.vel);
        w.f32(p.face);
    });
    w.opt(i.cursor.as_ref(), |w, c| w.vec3(*c));
    w.bool(i.cycle);
    w.bool(i.jump);
    w.bool(i.jump_blocked);
}

fn r_input(r: &mut Reader) -> Option<CaptainInput> {
    Some(CaptainInput {
        stick: r.vec2()?,
        whistle: r.bool()?,
        throw: r.bool()?,
        dismiss: r.bool()?,
        swarm: r.vec2()?,
        pose: r.opt(|r| Some(CaptainPose { pos: r.vec3()?, vel: r.vec3()?, face: r.f32()? }))?,
        cursor: r.opt(|r| r.vec3())?,
        cycle: r.bool()?,
        jump: r.bool()?,
        jump_blocked: r.bool()?,
    })
}

fn w_captain(w: &mut Writer, c: &Captain) {
    w.u32(c.id.0);
    w.vec3(c.pos);
    w.vec3(c.vel);
    w.vec3(c.target_vel);
    w.f32(c.face);
    w.bool(c.on_ground);
    w.opt(c.jump.as_ref(), w_jump);
    w.vec3(c.cursor);
    w.vec3(c.cursor_world);
    w.f32(c.neutral_time);
    match c.whistle {
        WhistleState::Off => w.u8(0),
        WhistleState::Expanding { timer } => {
            w.u8(1);
            w.f32(timer);
        }
        WhistleState::Calling { timer, radius } => {
            w.u8(2);
            w.f32(timer);
            w.f32(radius);
        }
    }
    match c.throw {
        ThrowState::Idle => w.u8(0),
        ThrowState::Wait { held, pending, pending_timeout, hold_time } => {
            w.u8(1);
            w.opt(held.as_ref(), |w, p| w.u32(p.0));
            w.opt(pending.as_ref(), |w, p| w.u32(p.0));
            w.f32(pending_timeout);
            w.f32(hold_time);
        }
    }
    w_plate(w, &c.plate);
    w.opt(c.pluck.as_ref(), w_pluck);
    w.bool(c.fast_pluck);
    w.opt(c.punch.as_ref(), |w, p| {
        w.f32(p.frame);
        w.bool(p.hit);
        w.bool(p.whistle_queued);
    });
    w.bool(c.swarming);
    w.f32(c.plate_yaw);
    w.f32(c.health);
    w.f32(c.damage_frames);
    w.bool(c.down);
    w.opt(c.throw_color.as_ref(), |w, k| w_color(w, *k));
    w.opt(c.lock_target.as_ref(), |w, t| w.u64(*t));
    w.opt(c.lock_pellet.as_ref(), |w, t| w.u32(t.0));
    w.opt(c.lock_bulb.as_ref(), |w, t| w.u32(t.0));
    w.f32(c.speed_scale);
    w.f32(c.reach_scale);
    w.f32(c.attack_rate);
    w.f32(c.squad_speed);
    w.f32(c.jump_scale);
    w.f32(c.accel_scale);
    w.u32(c.air_jumps);
    w.u32(c.air_jumps_used);
    w.bool(c.can_stomp);
    w.opt(c.stomp.as_ref(), |w, s| w.f32(*s));
    w.opt(c.lock_view.as_ref(), |w, l| {
        w.vec3(l.pos);
        w.vec3(l.eye);
    });
    w.opt(c.strike.as_ref(), |w, s| {
        w.f32(s.frame);
        w.bool(s.hit);
    });
    w.f32(c.strike_cooldown);
    w.opt(c.charge.as_ref(), |w, (d, t)| {
        w.vec2(*d);
        w.f32(*t);
    });
    w.f32(c.air_peak);
    w.opt(c.jump_tick.as_ref(), |w, t| w.u64(*t));
    w.bool(c.rearrange_pending);
    w_input(w, &c.input);
    for b in [c.pressed.jump, c.pressed.whistle, c.pressed.throw, c.pressed.dismiss, c.pressed.cycle] {
        w.bool(b);
    }
    w.bool(c.movement_held);
    w.bool(c.actions_held);
    w.bool(c.launched);
}

fn r_captain(r: &mut Reader) -> Option<Captain> {
    let id = CaptainId(r.u32()?);
    let pos = r.vec3()?;
    let vel = r.vec3()?;
    let target_vel = r.vec3()?;
    let face = r.f32()?;
    let on_ground = r.bool()?;
    let jump = r.opt(r_jump)?;
    let cursor = r.vec3()?;
    let cursor_world = r.vec3()?;
    let neutral_time = r.f32()?;
    let whistle = match r.u8()? {
        0 => WhistleState::Off,
        1 => WhistleState::Expanding { timer: r.f32()? },
        2 => WhistleState::Calling { timer: r.f32()?, radius: r.f32()? },
        _ => return None,
    };
    let throw = match r.u8()? {
        0 => ThrowState::Idle,
        1 => ThrowState::Wait {
            held: r.opt(|r| Some(PikiId(r.u32()?)))?,
            pending: r.opt(|r| Some(PikiId(r.u32()?)))?,
            pending_timeout: r.f32()?,
            hold_time: r.f32()?,
        },
        _ => return None,
    };
    let plate = r_plate(r)?;
    let pluck = r.opt(r_pluck)?;
    let fast_pluck = r.bool()?;
    let punch = r.opt(|r| Some(Punch { frame: r.f32()?, hit: r.bool()?, whistle_queued: r.bool()? }))?;
    let swarming = r.bool()?;
    let plate_yaw = r.f32()?;
    let health = r.f32()?;
    let damage_frames = r.f32()?;
    let down = r.bool()?;
    let throw_color = r.opt(r_color)?;
    let lock_target = r.opt(|r| r.u64())?;
    let lock_pellet = r.opt(|r| r.u32().map(crate::carry::PelletId))?;
    let lock_bulb = r.opt(|r| r.u32().map(BulbId))?;
    let speed_scale = r.f32()?;
    let reach_scale = r.f32()?;
    let attack_rate = r.f32()?;
    let squad_speed = r.f32()?;
    let jump_scale = r.f32()?;
    let accel_scale = r.f32()?;
    let air_jumps = r.u32()?;
    let air_jumps_used = r.u32()?;
    let can_stomp = r.bool()?;
    let stomp = r.opt(|r| r.f32())?;
    let lock_view = r.opt(|r| Some(LockView { pos: r.vec3()?, eye: r.vec3()? }))?;
    let strike = r.opt(|r| Some(Strike { frame: r.f32()?, hit: r.bool()? }))?;
    let strike_cooldown = r.f32()?;
    let charge = r.opt(|r| Some((r.vec2()?, r.f32()?)))?;
    let air_peak = r.f32()?;
    let jump_tick = r.opt(|r| r.u64())?;
    let rearrange_pending = r.bool()?;
    let input = r_input(r)?;
    let pressed = Pressed {
        jump: r.bool()?,
        whistle: r.bool()?,
        throw: r.bool()?,
        dismiss: r.bool()?,
        cycle: r.bool()?,
    };
    Some(Captain {
        id,
        pos,
        vel,
        target_vel,
        face,
        on_ground,
        jump,
        cursor,
        cursor_world,
        neutral_time,
        whistle,
        throw,
        plate,
        pluck,
        fast_pluck,
        punch,
        swarming,
        plate_yaw,
        health,
        damage_frames,
        down,
        throw_color,
        lock_target,
        lock_pellet,
        lock_bulb,
        speed_scale,
        reach_scale,
        attack_rate,
        squad_speed,
        jump_scale,
        accel_scale,
        air_jumps,
        air_jumps_used,
        can_stomp,
        stomp,
        lock_view,
        strike,
        strike_cooldown,
        charge,
        air_peak,
        jump_tick,
        rearrange_pending,
        input,
        pressed,
        movement_held: r.bool()?,
        actions_held: r.bool()?,
        launched: r.bool()?,
    })
}

// ---- pellets, Onions, sprouts, nectar --------------------------------------------------------

fn w_pellet(w: &mut Writer, p: &Pellet) {
    w.u32(p.id.0);
    let d = &p.desc;
    w.vec3(d.pos);
    w.f32(d.yaw);
    w.f32(d.radius);
    w.f32(d.height);
    w.u32(d.min_carriers);
    w.u32(d.max_carriers);
    w.i32(d.seeds_matching);
    w.i32(d.seeds_other);
    w.opt(d.color.as_ref(), |w, c| w_color(w, *c));
    w.opt(d.creature.as_ref(), |w, c| w.u64(*c));
    w.opt(d.ship_part.as_ref(), |w, s| w.u32(*s));
    w.vec3(p.pos);
    w.f32(p.yaw);
    w.f32(p.scale);
    match &p.state {
        PelletState::Idle => w.u8(0),
        PelletState::Carried { onion, color, path, next } => {
            w.u8(1);
            w.u32(onion.0);
            w_color(w, *color);
            w.len(path.len().min(MAX_PATH));
            for v in path.iter().take(MAX_PATH) {
                w.vec3(*v);
            }
            w.u16((*next).min(MAX_PATH) as u16);
        }
        PelletState::Sucking { onion, color, wait, start, progress, speed } => {
            w.u8(2);
            w.u32(onion.0);
            w_color(w, *color);
            w.f32(*wait);
            w.vec3(*start);
            w.f32(*progress);
            w.f32(*speed);
        }
        PelletState::Gone => w.u8(3),
        PelletState::Held => w.u8(4),
    }
    w.len(p.slots.len());
    for s in &p.slots {
        w.opt(s.as_ref(), |w, id| w.u32(id.0));
    }
    w.len(p.carriers.len());
    for c in &p.carriers {
        w.u32(c.0);
    }
    w.f32(p.stalled);
}

fn r_pellet(r: &mut Reader) -> Option<Pellet> {
    let id = PelletId(r.u32()?);
    let desc = PelletDesc {
        pos: r.vec3()?,
        yaw: r.f32()?,
        radius: r.f32()?,
        height: r.f32()?,
        min_carriers: r.u32()?,
        max_carriers: r.u32()?,
        seeds_matching: r.i32()?,
        seeds_other: r.i32()?,
        color: r.opt(r_color)?,
        creature: r.opt(|r| r.u64())?,
        ship_part: r.opt(|r| r.u32())?,
    };
    let pos = r.vec3()?;
    let yaw = r.f32()?;
    let scale = r.f32()?;
    let state = match r.u8()? {
        0 => PelletState::Idle,
        1 => {
            let onion = OnionId(r.u32()?);
            let color = r_color(r)?;
            let n = r.len()?;
            if n > MAX_PATH {
                return None;
            }
            let path = (0..n).map(|_| r.vec3()).collect::<Option<Vec<_>>>()?;
            PelletState::Carried { onion, color, path, next: r.u16()? as usize }
        }
        2 => PelletState::Sucking {
            onion: OnionId(r.u32()?),
            color: r_color(r)?,
            wait: r.f32()?,
            start: r.vec3()?,
            progress: r.f32()?,
            speed: r.f32()?,
        },
        3 => PelletState::Gone,
        4 => PelletState::Held,
        _ => return None,
    };
    let n = r.len()?;
    let slots = (0..n).map(|_| r.opt(|r| Some(PikiId(r.u32()?)))).collect::<Option<Vec<_>>>()?;
    let n = r.len()?;
    let carriers = (0..n).map(|_| Some(PikiId(r.u32()?))).collect::<Option<Vec<_>>>()?;
    Some(Pellet { id, desc, pos, yaw, scale, state, slots, carriers, stalled: r.f32()? })
}

fn w_onion_anim(w: &mut Writer, a: &OnionAnim) {
    let (tag, f) = match *a {
        OnionAnim::Idle => (0, 0.0),
        OnionAnim::Hover => (1, 0.0),
        OnionAnim::Suction { frame } => (2, frame),
        OnionAnim::Emitting { frame } => (3, frame),
        OnionAnim::TakeOff { frame } => (4, frame),
        OnionAnim::Away => (5, 0.0),
        OnionAnim::Landing { frame } => (6, frame),
    };
    w.u8(tag);
    w.f32(f);
}

fn r_onion_anim(r: &mut Reader) -> Option<OnionAnim> {
    let tag = r.u8()?;
    let f = r.f32()?;
    Some(match tag {
        0 => OnionAnim::Idle,
        1 => OnionAnim::Hover,
        2 => OnionAnim::Suction { frame: f },
        3 => OnionAnim::Emitting { frame: f },
        4 => OnionAnim::TakeOff { frame: f },
        5 => OnionAnim::Away,
        6 => OnionAnim::Landing { frame: f },
        _ => return None,
    })
}

fn w_onion(w: &mut Writer, o: &Onion) {
    w.u32(o.id.0);
    w.vec3(o.pos);
    w.len(o.colors.len());
    for c in &o.colors {
        w_color(w, *c);
    }
    for row in &o.stored {
        for n in row {
            w.u32(*n);
        }
    }
    w.len(o.exits.len());
    for (c, cap) in &o.exits {
        w_color(w, *c);
        w.u32(cap.0);
    }
    w.f32(o.exit_timer);
    w_onion_anim(w, &o.anim);
    w.opt(o.flight_motion.as_ref(), |w, (a, b)| {
        w.bool(*a);
        w.f32(*b);
    });
    w.len(o.queue.len());
    for c in &o.queue {
        w_color(w, *c);
    }
    w.u32(o.fireworks);
    w.opt(o.ship.as_ref(), |w, s| {
        w.vec3(s.goal);
        w.vec3(s.suck);
        w.len(s.parts.len());
        for p in &s.parts {
            w.u32(*p);
        }
    });
    w.opt(o.owner.as_ref(), |w, c| w.u32(c.0));
    w.f32(o.extinct_for);
    w.opt(o.dormancy.as_ref(), |w, d| { w.f32(d.remaining); w.f32(d.progress); w.bool(d.reviving); });
}

fn r_onion(r: &mut Reader) -> Option<Onion> {
    let id = OnionId(r.u32()?);
    let pos = r.vec3()?;
    let n = r.len()?;
    let colors = (0..n).map(|_| r_color(r)).collect::<Option<Vec<_>>>()?;
    let mut stored = [[0u32; 3]; crate::piki::COLOUR_COUNT];
    for row in &mut stored {
        for n in row {
            *n = r.u32()?;
        }
    }
    let n = r.len()?;
    let exits = (0..n).map(|_| Some((r_color(r)?, CaptainId(r.u32()?)))).collect::<Option<VecDeque<_>>>()?;
    let exit_timer = r.f32()?;
    let anim = r_onion_anim(r)?;
    let flight_motion = r.opt(|r| Some((r.bool()?, r.f32()?)))?;
    let n = r.len()?;
    let queue = (0..n).map(|_| r_color(r)).collect::<Option<VecDeque<_>>>()?;
    let fireworks = r.u32()?;
    let ship = r.opt(|r| {
        let goal = r.vec3()?;
        let suck = r.vec3()?;
        let n = r.len()?;
        Some(Ship { goal, suck, parts: (0..n).map(|_| r.u32()).collect::<Option<Vec<_>>>()? })
    })?;
    let owner = r.opt(|r| Some(CaptainId(r.u32()?)))?;
    Some(Onion {
        id,
        pos,
        colors,
        stored,
        exits,
        exit_timer,
        anim,
        flight_motion,
        queue,
        fireworks,
        ship,
        owner,
        extinct_for: r.f32()?,
        dormancy: r.opt(|r| Some(crate::onion::OnionDormancy {
            remaining: r.f32()?, progress: r.f32()?, reviving: r.bool()?,
        }))?,
    })
}

fn w_sprout(w: &mut Writer, s: &Sprout) {
    w.u32(s.id.0);
    w_color(w, s.color);
    w_happa(w, s.happa);
    w.vec3(s.pos);
    w.vec3(s.vel);
    w.f32(s.air_resist);
    match s.state {
        SproutState::Flying => w.u8(0),
        SproutState::Buried { timer } => {
            w.u8(1);
            w.f32(timer);
        }
        SproutState::Tane { timer } => {
            w.u8(2);
            w.f32(timer);
        }
        SproutState::Ready { timer } => {
            w.u8(3);
            w.f32(timer);
        }
        SproutState::Wilted { timer } => {
            w.u8(4);
            w.f32(timer);
        }
    }
    w.opt(s.onion.as_ref(), |w, o| w.u32(o.0));
    w.opt(s.claimed_by.as_ref(), |w, c| w.u32(c.0));
    w.f32(s.unplucked);
}

fn r_sprout(r: &mut Reader) -> Option<Sprout> {
    let id = SproutId(r.u32()?);
    let color = r_color(r)?;
    let happa = r_happa(r)?;
    let pos = r.vec3()?;
    let vel = r.vec3()?;
    let air_resist = r.f32()?;
    let state = match r.u8()? {
        0 => SproutState::Flying,
        1 => SproutState::Buried { timer: r.f32()? },
        2 => SproutState::Tane { timer: r.f32()? },
        3 => SproutState::Ready { timer: r.f32()? },
        4 => SproutState::Wilted { timer: r.f32()? },
        _ => return None,
    };
    Some(Sprout {
        id,
        color,
        happa,
        pos,
        vel,
        air_resist,
        state,
        onion: r.opt(|r| Some(OnionId(r.u32()?)))?,
        claimed_by: r.opt(|r| Some(CaptainId(r.u32()?)))?,
        unplucked: r.f32()?,
    })
}

fn w_drop(w: &mut Writer, d: &NectarDrop) {
    w.u32(d.id.0);
    w.vec3(d.pos);
    w.vec3(d.vel);
    match d.state {
        DropState::Falling { frame, leaving } => {
            w.u8(0);
            w.f32(frame);
            w.bool(leaving);
        }
        DropState::Splash { frame, emitted } => {
            w.u8(1);
            w.f32(frame);
            w.bool(emitted);
        }
    }
}

fn r_drop(r: &mut Reader) -> Option<NectarDrop> {
    let id = NectarId(r.u32()?);
    let pos = r.vec3()?;
    let vel = r.vec3()?;
    let state = match r.u8()? {
        0 => DropState::Falling { frame: r.f32()?, leaving: r.bool()? },
        1 => DropState::Splash { frame: r.f32()?, emitted: r.bool()? },
        _ => return None,
    };
    Some(NectarDrop { id, pos, vel, state })
}

fn w_puddle(w: &mut Writer, p: &Puddle) {
    w.u32(p.id.0);
    w.vec3(p.pos);
    let (tag, f) = match p.state {
        PuddleState::Appear { frame } => (0, frame),
        PuddleState::Wait { frame } => (1, frame),
        PuddleState::Touched { frame } => (2, frame),
        PuddleState::Drain { frame } => (3, frame),
    };
    w.u8(tag);
    w.f32(f);
}

fn r_puddle(r: &mut Reader) -> Option<Puddle> {
    let id = NectarId(r.u32()?);
    let pos = r.vec3()?;
    let tag = r.u8()?;
    let f = r.f32()?;
    let state = match tag {
        0 => PuddleState::Appear { frame: f },
        1 => PuddleState::Wait { frame: f },
        2 => PuddleState::Touched { frame: f },
        3 => PuddleState::Drain { frame: f },
        _ => return None,
    };
    Some(Puddle { id, pos, state })
}

fn w_bulb(w: &mut Writer, b: &Bulb) {
    w.u32(b.id.0);
    w.vec3(b.pos);
    w_color(w, b.colour);
    w.bool(b.queen);
    w.u32(b.slots);
    w.u32(b.used);
    w.u32(b.swallowed);
    w.u32(b.same);
    w.f32(b.change);
    let (tag, t, fired) = match b.state {
        BulbState::Open { t } => (0, t, false),
        BulbState::Swing { t } => (1, t, false),
        BulbState::Close { t } => (2, t, false),
        BulbState::Shot { t, fired } => (3, t, fired),
        BulbState::Dead { t } => (4, t, false),
    };
    w.u8(tag);
    w.f32(t);
    w.bool(fired);
}

fn r_bulb(r: &mut Reader) -> Option<Bulb> {
    let id = BulbId(r.u32()?);
    let pos = r.vec3()?;
    let colour = r_color(r)?;
    let queen = r.bool()?;
    let slots = r.u32()?;
    let (used, swallowed, same, change) = (r.u32()?, r.u32()?, r.u32()?, r.f32()?);
    let (tag, t, fired) = (r.u8()?, r.f32()?, r.bool()?);
    let state = match tag {
        0 => BulbState::Open { t },
        1 => BulbState::Swing { t },
        2 => BulbState::Close { t },
        3 => BulbState::Shot { t, fired },
        4 => BulbState::Dead { t },
        _ => return None,
    };
    Some(Bulb { id, pos, colour, queen, slots, used, swallowed, same, change, state })
}

// ---- events ----------------------------------------------------------------------------------

fn w_cause(w: &mut Writer, c: DeathCause) {
    w.u8(match c {
        DeathCause::Attacked => 0,
        DeathCause::Crushed => 1,
        DeathCause::Eaten => 2,
        DeathCause::Killed => 3,
    });
}

fn r_cause(r: &mut Reader) -> Option<DeathCause> {
    Some(match r.u8()? {
        0 => DeathCause::Attacked,
        1 => DeathCause::Crushed,
        2 => DeathCause::Eaten,
        3 => DeathCause::Killed,
        _ => return None,
    })
}

fn w_event(w: &mut Writer, e: &Event) {
    use Event::*;
    match *e {
        PikiEnteredOnion { piki, onion } => {
            w.u8(0);
            w.u32(piki.0);
            w.u32(onion.0);
        }
        PikiLeftOnion { piki, onion } => {
            w.u8(1);
            w.u32(piki.0);
            w.u32(onion.0);
        }
        OnionLanded { onion } => {
            w.u8(2);
            w.u32(onion.0);
        }
        NectarSplash { pos } => {
            w.u8(3);
            w.vec3(pos);
        }
        NectarGone => w.u8(4),
        PikiDrinking { piki } => {
            w.u8(5);
            w.u32(piki.0);
        }
        Bloomed { piki } => {
            w.u8(6);
            w.u32(piki.0);
        }
        Whistle { captain, centre, radius } => {
            w.u8(7);
            w.u32(captain.0);
            w.vec3(centre);
            w.f32(radius);
        }
        Recruited { piki, captain, stolen_from } => {
            w.u8(8);
            w.u32(piki.0);
            w.u32(captain.0);
            w.opt(stolen_from.as_ref(), |w, c| w.u32(c.0));
        }
        Thrown { piki, captain } => {
            w.u8(9);
            w.u32(piki.0);
            w.u32(captain.0);
        }
        Landed { piki } => {
            w.u8(10);
            w.u32(piki.0);
        }
        Latched { piki, creature } => {
            w.u8(11);
            w.u32(piki.0);
            w.u64(creature);
        }
        Hit { piki, creature, damage, latched } => {
            w.u8(12);
            w.u32(piki.0);
            w.u64(creature);
            w.f32(damage);
            w.bool(latched);
        }
        KnockedOff { piki, creature } => {
            w.u8(13);
            w.u32(piki.0);
            w.opt(creature.as_ref(), |w, c| w.u64(*c));
        }
        PikiHurt { piki, damage } => {
            w.u8(14);
            w.u32(piki.0);
            w.f32(damage);
        }
        ShipPartCollected { pellet, ship, part, total } => {
            w.u8(15);
            w.u32(pellet.0);
            w.u32(ship.0);
            w.u32(part);
            w.u32(total);
        }
        CarrierGrabbed { piki, pellet } => {
            w.u8(16);
            w.u32(piki.0);
            w.u32(pellet.0);
        }
        Deflowered { piki } => {
            w.u8(17);
            w.u32(piki.0);
        }
        PikiGone { piki, pos } => {
            w.u8(18);
            w.u32(piki.0);
            w.vec3(pos);
        }
        PikiDied { piki, cause } => {
            w.u8(19);
            w.u32(piki.0);
            w_cause(w, cause);
        }
        Swallowed { piki, creature } => {
            w.u8(20);
            w.u32(piki.0);
            w.u64(creature);
        }
        SpitOut { piki } => {
            w.u8(21);
            w.u32(piki.0);
        }
        CaptainHurt { captain, damage, health } => {
            w.u8(22);
            w.u32(captain.0);
            w.f32(damage);
            w.f32(health);
        }
        CaptainDown { captain } => {
            w.u8(23);
            w.u32(captain.0);
        }
        Dismissed { captain } => {
            w.u8(24);
            w.u32(captain.0);
        }
        Punched { captain, creature, damage } => {
            w.u8(25);
            w.u32(captain.0);
            w.u64(creature);
            w.f32(damage);
        }
        PelletLifted { pellet, onion } => {
            w.u8(26);
            w.u32(pellet.0);
            w.u32(onion.0);
        }
        PelletRerouted { pellet, onion } => {
            w.u8(27);
            w.u32(pellet.0);
            w.u32(onion.0);
        }
        PelletDropped { pellet } => {
            w.u8(28);
            w.u32(pellet.0);
        }
        PelletArrived { pellet, onion } => {
            w.u8(29);
            w.u32(pellet.0);
            w.u32(onion.0);
        }
        PelletDelivered { pellet, onion, color, seeds } => {
            w.u8(30);
            w.u32(pellet.0);
            w.u32(onion.0);
            w_color(w, color);
            w.i32(seeds);
        }
        SeedLaunched { onion, sprout, color } => {
            w.u8(31);
            w.u32(onion.0);
            w.u32(sprout.0);
            w_color(w, color);
        }
        SeedStored { onion, color } => {
            w.u8(32);
            w.u32(onion.0);
            w_color(w, color);
        }
        SproutPlanted { sprout } => {
            w.u8(33);
            w.u32(sprout.0);
        }
        SproutGrew { sprout, happa } => {
            w.u8(34);
            w.u32(sprout.0);
            w_happa(w, happa);
        }
        SproutWilted { sprout } => {
            w.u8(35);
            w.u32(sprout.0);
        }
        Plucked { captain, piki, sprout } => {
            w.u8(36);
            w.u32(captain.0);
            w.u32(piki.0);
            w.u32(sprout.0);
        }
        EmergencyPikmin { onion, captain, count } => {
            w.u8(37);
            w.u32(onion.0);
            w.u32(captain.0);
            w.u32(count);
        }
        PluckerOut { piki, onion } => {
            w.u8(38);
            w.u32(piki.0);
            w.u32(onion.0);
        }
        PluckerPulled { plucker, plucked, captain } => {
            w.u8(39);
            w.u32(plucker.0);
            w.u32(plucked.0);
            w.opt(captain.as_ref(), |w, c| w.u32(c.0));
        }
        CaptainJumped { captain, air } => {
            w.u8(40);
            w.u32(captain.0);
            w.bool(air);
        }
        CaptainStomped { captain, pos, drop } => {
            w.u8(41);
            w.u32(captain.0);
            w.vec3(pos);
            w.f32(drop);
        }
        PikiGrew { piki, happa } => {
            w.u8(42);
            w.u32(piki.0);
            w_happa(w, happa);
        }
        StrikeStarted { captain } => {
            w.u8(43);
            w.u32(captain.0);
        }
        CaptainLanded { captain, drop, stomp } => {
            w.u8(44);
            w.u32(captain.0);
            w.f32(drop);
            w.bool(stomp);
        }
        PikiStomped { piki, captain, pos } => {
            w.u8(45);
            w.u32(piki.0);
            w.u32(captain.0);
            w.vec3(pos);
        }
        PikiLaunched { piki, captain } => {
            w.u8(46);
            w.u32(piki.0);
            w.u32(captain.0);
        }
        Pounded { piki, pos } => {
            w.u8(47);
            w.u32(piki.0);
            w.vec3(pos);
        }
        Grounded { creature, piki } => {
            w.u8(48);
            w.u64(creature);
            w.u32(piki.0);
        }
        PoisonedEater { piki, creature } => {
            w.u8(49);
            w.u32(piki.0);
            w.u64(creature);
        }
        BulbSwallowed { piki, bulb, colour, pos } => {
            w.u8(50);
            w.u32(piki.0);
            w.u32(bulb.0);
            w_color(w, colour);
            w.vec3(pos);
        }
        BulbShot { bulb, colour, count, pos } => {
            w.u8(51);
            w.u32(bulb.0);
            w_color(w, colour);
            w.u32(count);
            w.vec3(pos);
        }
        WeighedDown { creature, piki, weight } => {
            w.u8(52);
            w.u64(creature);
            w.u32(piki.0);
            w.u32(weight);
        }
    }
}

fn r_event(r: &mut Reader) -> Option<Event> {
    use Event::*;
    let piki = |r: &mut Reader| Some(PikiId(r.u32()?));
    let cap = |r: &mut Reader| Some(CaptainId(r.u32()?));
    let onion = |r: &mut Reader| Some(OnionId(r.u32()?));
    let pellet = |r: &mut Reader| Some(PelletId(r.u32()?));
    let sprout = |r: &mut Reader| Some(SproutId(r.u32()?));
    Some(match r.u8()? {
        0 => PikiEnteredOnion { piki: piki(r)?, onion: onion(r)? },
        1 => PikiLeftOnion { piki: piki(r)?, onion: onion(r)? },
        2 => OnionLanded { onion: onion(r)? },
        3 => NectarSplash { pos: r.vec3()? },
        4 => NectarGone,
        5 => PikiDrinking { piki: piki(r)? },
        6 => Bloomed { piki: piki(r)? },
        7 => Whistle { captain: cap(r)?, centre: r.vec3()?, radius: r.f32()? },
        8 => Recruited { piki: piki(r)?, captain: cap(r)?, stolen_from: r.opt(cap)? },
        9 => Thrown { piki: piki(r)?, captain: cap(r)? },
        10 => Landed { piki: piki(r)? },
        11 => Latched { piki: piki(r)?, creature: r.u64()? },
        12 => Hit { piki: piki(r)?, creature: r.u64()?, damage: r.f32()?, latched: r.bool()? },
        13 => KnockedOff { piki: piki(r)?, creature: r.opt(|r| r.u64())? },
        14 => PikiHurt { piki: piki(r)?, damage: r.f32()? },
        15 => ShipPartCollected { pellet: pellet(r)?, ship: onion(r)?, part: r.u32()?, total: r.u32()? },
        16 => CarrierGrabbed { piki: piki(r)?, pellet: pellet(r)? },
        17 => Deflowered { piki: piki(r)? },
        18 => PikiGone { piki: piki(r)?, pos: r.vec3()? },
        19 => PikiDied { piki: piki(r)?, cause: r_cause(r)? },
        20 => Swallowed { piki: piki(r)?, creature: r.u64()? },
        21 => SpitOut { piki: piki(r)? },
        22 => CaptainHurt { captain: cap(r)?, damage: r.f32()?, health: r.f32()? },
        23 => CaptainDown { captain: cap(r)? },
        24 => Dismissed { captain: cap(r)? },
        25 => Punched { captain: cap(r)?, creature: r.u64()?, damage: r.f32()? },
        26 => PelletLifted { pellet: pellet(r)?, onion: onion(r)? },
        27 => PelletRerouted { pellet: pellet(r)?, onion: onion(r)? },
        28 => PelletDropped { pellet: pellet(r)? },
        29 => PelletArrived { pellet: pellet(r)?, onion: onion(r)? },
        30 => PelletDelivered { pellet: pellet(r)?, onion: onion(r)?, color: r_color(r)?, seeds: r.i32()? },
        31 => SeedLaunched { onion: onion(r)?, sprout: sprout(r)?, color: r_color(r)? },
        32 => SeedStored { onion: onion(r)?, color: r_color(r)? },
        33 => SproutPlanted { sprout: sprout(r)? },
        34 => SproutGrew { sprout: sprout(r)?, happa: r_happa(r)? },
        35 => SproutWilted { sprout: sprout(r)? },
        36 => Plucked { captain: cap(r)?, piki: piki(r)?, sprout: sprout(r)? },
        37 => EmergencyPikmin { onion: onion(r)?, captain: cap(r)?, count: r.u32()? },
        38 => PluckerOut { piki: piki(r)?, onion: onion(r)? },
        39 => PluckerPulled { plucker: piki(r)?, plucked: piki(r)?, captain: r.opt(cap)? },
        40 => CaptainJumped { captain: cap(r)?, air: r.bool()? },
        41 => CaptainStomped { captain: cap(r)?, pos: r.vec3()?, drop: r.f32()? },
        42 => PikiGrew { piki: piki(r)?, happa: r_happa(r)? },
        43 => StrikeStarted { captain: cap(r)? },
        44 => CaptainLanded { captain: cap(r)?, drop: r.f32()?, stomp: r.bool()? },
        45 => PikiStomped { piki: piki(r)?, captain: cap(r)?, pos: r.vec3()? },
        46 => PikiLaunched { piki: piki(r)?, captain: cap(r)? },
        47 => Pounded { piki: piki(r)?, pos: r.vec3()? },
        48 => Grounded { creature: r.u64()?, piki: piki(r)? },
        49 => PoisonedEater { piki: piki(r)?, creature: r.u64()? },
        50 => BulbSwallowed { piki: piki(r)?, bulb: BulbId(r.u32()?), colour: r_color(r)?, pos: r.vec3()? },
        51 => BulbShot { bulb: BulbId(r.u32()?), colour: r_color(r)?, count: r.u32()?, pos: r.vec3()? },
        52 => WeighedDown { creature: r.u64()?, piki: piki(r)?, weight: r.u32()? },
        _ => return None,
    })
}
