//! Particle effects, run by `libpikmin_assets::particle` (a port of `zen::particleGenerator`)
//! for any renderer: the whistle and the one-shots hosts start (a dead Pikmin's ghost, the
//! ship's). [`Effects::batches`] hands out camera-facing quads per effect; hosts draw them with
//! the effect's texture ([`Effects::defs`]) in its blend mode.
//!
//! `NaviGatherState::init` starts two whistle effects: `NaviWhistle` (three generators
//! streaming from the antenna to the cursor, NaviWhistle.h) and `NaviFue` (24 `g_curs00`
//! sparkles riding the whistle circle, placed by `Navi::renderCircle`). `NaviGatherState::exec`
//! finishes them half a second before the hold time runs out, or when the whistle ends.
//! Generators step at the game's 30 Hz (`particleManager::update` scales by 30 × frame time).

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Quat, Vec3};
use libpikmin::CaptainId;
use libpikmin::captain::WhistleState;
use libpikmin_assets::particle::{EffectDef, Generator, Particle, Pcr, Quad, effects};
use libpikmin_assets::{Assets, Image};

/// The whistle's beam (3) and circle come first in [`Effects::defs`].
const WHISTLE_DEFS: usize = 4;

/// A particle texture: RGBA8, rows top first; repeat (else clamp) in S and T.
pub struct Texture {
    pub image: Image,
    pub wrap: [bool; 2],
}

/// A loaded effect: its `.pcr` and textures.
pub struct Def {
    pub pcr: Arc<Pcr>,
    pub tex: Texture,
    /// Child particles' texture, if not `tex`.
    pub child_tex: Option<Texture>,
}

fn texture(assets: &Assets, path: &str) -> Option<Texture> {
    let (image, s, t) = libpikmin_assets::texture::decode_bti(&assets.read(path).ok()?).ok()?;
    Some(Texture { image, wrap: [s, t] })
}

impl Def {
    fn load(assets: &Assets, d: EffectDef) -> Option<Def> {
        Some(Def {
            pcr: Arc::new(Pcr::parse(&assets.read(d.pcr).ok()?).ok()?),
            tex: texture(assets, d.tex)?,
            child_tex: d.child_tex.and_then(|t| texture(assets, t)),
        })
    }
}

/// One effect's live quads, all drawn one way.
pub struct QuadBatch {
    /// Which [`Effects::defs`] entry (its texture: `child_tex` for children, if it has one).
    pub def: usize,
    pub child: bool,
    /// How texture and colours combine (`DGXGraphics::setBlendMode`'s TEV stage, prim = the
    /// quad's `prim`, env its `env`): 0 `mix(prim, 1, tex)`, 1 `prim * tex`, 2
    /// `mix(env, prim, tex)`, 3 `env + tex * prim`, 4 `prim`. Alpha is `tex.a * prim.a`.
    pub mode: u8,
    /// GX blend factors: source in the low nibble, destination in the high (0 zero, 1 one, 2
    /// src/dst colour, 3 one minus it, 4 src alpha, 5 one minus src alpha, 6 dst alpha, 7 one
    /// minus dst alpha).
    pub blend: u8,
    /// GX Z mode: bit 3 test, bits 0-2 compare function, bit 4 write.
    pub z: u8,
    pub quads: Vec<Quad>,
}

/// One captain's whistle effects.
struct Whistle {
    /// `EFF_Navi_Whistle1..3` (A follows the antenna; B and C are placed along the beam).
    beam: Vec<Generator>,
    /// `NaviFue`: 24 sparkles on the circle.
    circle: Vec<Generator>,
    stopped: bool,
    state: WhistleState,
}

/// Where a captain's whistle is this frame.
#[derive(Clone, Copy)]
pub struct WhistleFrame {
    pub captain: CaptainId,
    pub state: WhistleState,
    pub antenna: Vec3,
    pub cursor: Vec3,
    pub captain_pos: Vec3,
    pub radius: f32,
}

pub struct Effects {
    /// Every loaded effect: the whistle's beam (3), its circle, then the one-shots.
    defs: Vec<(EffectDef, Def)>,
    /// Running one-shots; each finishes on its own (`mMaxPasses`).
    oneshots: Vec<Generator>,
    /// Effects that follow something (`setEmitPosPtr`), by the host's key. They run until
    /// detached, then run down as one-shots.
    attached: Vec<(u64, Generator)>,
    whistles: HashMap<CaptainId, Whistle>,
    /// `Navi::renderCircle`'s spinning angle (a static shared by every captain).
    ring_angle: f32,
    acc: f32,
    seed: u32,
}

/// `zen::makeRotMatrix`: a basis with `axis` as Y. Its degenerate check only looks at
/// `z.x`, which is always 0, so Z is always (0, 0, 1) and the basis is generally skewed.
fn make_rot_matrix(axis: Vec3) -> Mat3 {
    let mut z = Vec3::X.cross(axis);
    if z.x == 0.0 {
        z = Vec3::Z;
    }
    let z = z.normalize_or_zero();
    let x = axis.cross(z).normalize_or_zero();
    Mat3::from_cols(x, axis, z)
}

/// `Quat::fromMat3f` (sysMath.cpp), which also takes skewed matrices.
fn quat_from_mat3(m: Mat3) -> Quat {
    let e = |r: usize, c: usize| m.col(c)[r];
    let a = 0.25 * (e(0, 0) + e(1, 1) + e(2, 2) + 1.0);
    let b = a - 0.5 * (e(1, 1) + e(2, 2));
    let c = a - 0.5 * (e(2, 2) + e(0, 0));
    let d = a - 0.5 * (e(0, 0) + e(1, 1));
    let kind = if a > b {
        if a > c { if a > d { 0 } else { 3 } } else if c > d { 2 } else { 3 }
    } else if b > c {
        if b > d { 1 } else { 3 }
    } else if c > d {
        2
    } else {
        3
    };
    let (mut s, mut x, mut y, mut z);
    match kind {
        0 => {
            s = a.sqrt();
            let t = 0.25 / s;
            x = t * (e(2, 1) - e(1, 2));
            y = t * (e(0, 2) - e(2, 0));
            z = t * (e(1, 0) - e(0, 1));
        }
        1 => {
            x = b.sqrt();
            let t = 0.25 / x;
            s = t * (e(2, 1) - e(1, 2));
            y = t * (e(0, 1) + e(1, 0));
            z = t * (e(0, 2) + e(2, 0));
        }
        2 => {
            y = c.sqrt();
            let t = 0.25 / y;
            s = t * (e(0, 2) - e(2, 0));
            z = t * (e(1, 2) + e(2, 1));
            x = t * (e(1, 0) + e(0, 1));
        }
        _ => {
            z = d.sqrt();
            let t = 0.25 / z;
            s = t * (e(1, 0) - e(0, 1));
            x = t * (e(2, 0) + e(0, 2));
            y = t * (e(2, 1) + e(1, 2));
        }
    }
    if s < 0.0 {
        (s, x, y, z) = (-s, -x, -y, -z);
    }
    let n = (s * s + x * x + y * y + z * z).sqrt();
    if n.is_nan() || n <= 0.0 {
        return Quat::IDENTITY;
    }
    Quat::from_xyzw(x / n, y / n, z / n, s / n)
}

/// `NaviWhistle::invoke`: a beam particle sits `age / life` of the way from the antenna to the
/// cursor, its normal turning from the beam's direction to straight down as it lands.
fn place_on_beam(p: &mut Particle, antenna: Vec3, cursor: Vec3, captain_y: f32) {
    let diff = cursor - antenna;
    let ratio = p.age as f32 / p.life.max(1) as f32;
    p.global = antenna;
    p.local = diff * ratio;
    if cursor.y < captain_y + 15.0 {
        // Flat ground here: the triangle normal is +Y.
        let q1 = quat_from_mat3(make_rot_matrix(diff.normalize_or_zero()));
        let q2 = quat_from_mat3(make_rot_matrix(-Vec3::Y));
        let q = q1.slerp(q2, ratio);
        p.normal = q * Vec3::Y;
        p.vel = q * Vec3::X * 0.01;
    } else {
        p.normal = diff;
    }
}

impl Effects {
    pub fn load(assets: &Assets) -> Option<Effects> {
        let mut defs = Vec::new();
        for d in effects::NAVI_WHISTLE.into_iter().chain([effects::WHISTLE_CIRCLE]) {
            defs.push((d, Def::load(assets, d)?));
        }
        defs.extend(
            [
                effects::PIKI_DEAD_SOUL,
                effects::PIKI_GROW_UP1,
                effects::PIKI_GROW_UP2,
                effects::PIKI_DEFLOWER[0],
                effects::PIKI_DEFLOWER[1],
                effects::ROCKET_C,
                effects::NAVI_RECOVER,
                effects::NAVI_RINGS,
                effects::NAVI_SPARKLE,
                effects::ROCKET_COMPLETE[0],
                effects::ROCKET_COMPLETE[1],
                effects::ROCKET_GEP,
                effects::UFO_PART_ASSIGN[0],
                effects::UFO_PART_ASSIGN[1],
                effects::ROCKET_NKE[0],
                effects::ROCKET_NKE[1],
                effects::CLOUD_OF_DUST_1,
                effects::ROCKET_FKM1,
                effects::ROCKET_LAND,
                effects::KAFUN_NG,
                effects::ROCKET_TAKE_S,
                effects::ROCKET_LAND_S,
                effects::ROCKET_SMOKE_D,
                effects::ONYON_SUCK,
            ]
            .into_iter()
            .chain(effects::ONYON_BEACON_RING)
            .chain(effects::ONYON_HALO_RING)
            .chain(effects::ROCKET_JETS.into_iter().flatten())
            .chain(effects::ROCKET_JET_G)
            .chain(effects::ROCKET_BOOST_GROUND)
            .chain(effects::ROCKET_BOOSTER)
            .chain(effects::ROCKET_PCA)
            .filter_map(|d| Some((d, Def::load(assets, d)?))),
        );
        Some(Effects {
            defs,
            oneshots: Vec::new(),
            attached: Vec::new(),
            whistles: HashMap::new(),
            ring_angle: 0.0,
            acc: 0.0,
            seed: 0x1234_5678,
        })
    }

    fn generator(&mut self, pcr: &Arc<Pcr>, pos: Vec3) -> Generator {
        self.seed = self.seed.wrapping_mul(747796405).wrapping_add(2891336453);
        Generator::new(pcr.clone(), pos, self.seed)
    }

    /// How many effects (one-shots and attached) are running.
    pub fn running(&self) -> usize {
        self.oneshots.len() + self.attached.len()
    }

    /// `EffectMgr::create` for a one-shot effect at `pos`; the generator, to set up further.
    pub fn spawn(&mut self, def: EffectDef, pos: Vec3) -> Option<&mut Generator> {
        let pcr = self.defs[WHISTLE_DEFS..].iter().find(|(d, _)| *d == def).map(|(_, x)| x.pcr.clone())?;
        let g = self.generator(&pcr, pos);
        self.oneshots.push(g);
        self.oneshots.last_mut()
    }

    /// Starts an effect that follows something, replacing the one under `key`.
    pub fn attach(&mut self, key: u64, def: EffectDef, pos: Vec3) -> Option<&mut Generator> {
        self.detach(key);
        let pcr = self.defs[WHISTLE_DEFS..].iter().find(|(d, _)| *d == def).map(|(_, x)| x.pcr.clone())?;
        let g = self.generator(&pcr, pos);
        self.attached.push((key, g));
        self.attached.last_mut().map(|(_, g)| g)
    }

    /// Moves the effect under `key`.
    pub fn move_attached(&mut self, key: u64, pos: Vec3) {
        for (_, g) in self.attached.iter_mut().filter(|(k, _)| *k == key) {
            g.emit_pos = pos;
        }
    }

    /// Stops the effect under `key`; its particles run out (`effectMgr->kill(efx, false)`).
    pub fn detach(&mut self, key: u64) {
        self.detach_where(|k| k == key);
    }

    /// Stops every attached effect whose key matches.
    pub fn detach_where(&mut self, f: impl Fn(u64) -> bool) {
        let mut i = 0;
        while i < self.attached.len() {
            if f(self.attached[i].0) {
                let (_, mut g) = self.attached.swap_remove(i);
                g.finish();
                self.oneshots.push(g);
            } else {
                i += 1;
            }
        }
    }

    /// Starts, moves, steps and stops each captain's whistle effects.
    pub fn update(&mut self, frames: &[WhistleFrame], hold_time: f32, dt: f32) {
        for f in frames {
            let on = !matches!(f.state, WhistleState::Off);
            // `NaviGatherState::exec`: the effects stop 0.5 s before the call ends.
            // Hosts using a shorter call must still get particles until release/end.
            let ending = hold_time > 0.5
                && matches!(f.state, WhistleState::Calling { timer, .. } if timer > hold_time - 0.5);
            let restarted = self.whistles.get(&f.captain).is_some_and(|w| match (w.state, f.state) {
                (WhistleState::Off, _) => true,
                (WhistleState::Calling { .. }, WhistleState::Expanding { .. }) => true,
                (WhistleState::Expanding { timer: old }, WhistleState::Expanding { timer })
                | (WhistleState::Calling { timer: old, .. }, WhistleState::Calling { timer, .. }) => timer < old,
                _ => false,
            });
            if on && (!self.whistles.contains_key(&f.captain) || restarted) {
                // A rapid second whistle gets fresh emitters while the old particles fade.
                if let Some(w) = self.whistles.remove(&f.captain) {
                    for mut g in w.beam.into_iter().chain(w.circle) {
                        g.finish();
                        self.oneshots.push(g);
                    }
                }
                let pcrs: Vec<_> = self.defs[..3].iter().map(|(_, d)| d.pcr.clone()).collect();
                let mut beam: Vec<_> = pcrs.iter().map(|p| self.generator(p, f.antenna)).collect();
                // `setOrientedConstZAxis(true)` on B.
                beam[1].fixed_normal = true;
                let circle_pcr = self.defs[3].1.pcr.clone();
                let circle = (0..24).map(|_| self.generator(&circle_pcr, f.cursor)).collect();
                self.whistles.insert(f.captain, Whistle { beam, circle, stopped: false, state: f.state });
            }
            if let Some(w) = self.whistles.get_mut(&f.captain) {
                w.state = f.state;
                if (!on || ending) && !w.stopped {
                    w.stopped = true;
                    w.beam.iter_mut().chain(w.circle.iter_mut()).for_each(Generator::finish);
                }
            }
        }
        self.acc += dt;
        while self.acc >= 1.0 / 30.0 {
            self.acc -= 1.0 / 30.0;
            self.ring_angle += 0.03;
            if self.ring_angle > std::f32::consts::TAU {
                self.ring_angle -= std::f32::consts::TAU;
            }
            let ring = self.ring_angle;
            for f in frames {
                let Some(w) = self.whistles.get_mut(&f.captain) else { continue };
                // A follows the antenna; B and C are placed on the beam each update.
                w.beam[0].emit_pos = f.antenna;
                for (i, g) in w.beam.iter_mut().enumerate() {
                    let (antenna, cursor, y) = (f.antenna, f.cursor, f.captain_pos.y);
                    if i == 0 {
                        g.update(1.0, &mut |_| {});
                    } else {
                        g.update(1.0, &mut |p| place_on_beam(p, antenna, cursor, y));
                    }
                }
                // `Navi::renderCircle`: 24 points around the cursor at the whistle radius.
                for (i, g) in w.circle.iter_mut().enumerate() {
                    let a = i as f32 * (std::f32::consts::TAU / 24.0) + ring;
                    // Keep the cursor's world height on hosts whose terrain isn't at y=0.
                    g.emit_pos = f.cursor + Vec3::new(f.radius * a.sin(), 0.0, f.radius * a.cos());
                    g.update(1.0, &mut |_| {});
                }
            }
            self.oneshots.retain_mut(|g| g.update(1.0, &mut |_| {}));
            for (_, g) in self.attached.iter_mut() {
                g.update(1.0, &mut |_| {});
            }
            // Effects of captains that have left the list still run down.
            for (id, w) in self.whistles.iter_mut() {
                if frames.iter().any(|f| f.captain == *id) {
                    continue;
                }
                w.stopped = true;
                w.state = WhistleState::Off;
                w.beam.iter_mut().chain(w.circle.iter_mut()).for_each(|g| {
                    g.finish();
                    g.update(1.0, &mut |_| {});
                });
            }
            self.whistles.retain(|_, w| {
                // Keep a drained, stopped call until it ends, so holding it doesn't start
                // new emitters every frame after the early cutoff.
                !(w.stopped && matches!(w.state, WhistleState::Off)
                    && w.beam.iter().chain(w.circle.iter()).all(|g| {
                        g.is_finished() && g.particles.is_empty() && g.children.is_empty()
                    }))
            });
        }
    }

    /// Every loaded effect, in the order [`QuadBatch::def`] counts.
    pub fn defs(&self) -> impl Iterator<Item = &Def> {
        self.defs.iter().map(|(_, d)| d)
    }

    /// Every live particle as quads facing a camera whose world axes are `right` and `up`.
    pub fn batches(&mut self, right: Vec3, up: Vec3) -> Vec<QuadBatch> {
        let pcrs: Vec<Arc<Pcr>> = self.defs.iter().map(|(_, d)| d.pcr.clone()).collect();
        let gens = self
            .oneshots
            .iter_mut()
            .chain(self.attached.iter_mut().map(|(_, g)| g))
            .chain(self.whistles.values_mut().flat_map(|w| w.beam.iter_mut().chain(w.circle.iter_mut())));
        let mut out = Vec::new();
        for g in gens {
            let Some(def) = pcrs.iter().position(|p| Arc::ptr_eq(p, &g.pcr)) else { continue };
            let quads = g.quads(right, up);
            let pcr = &pcrs[def];
            for child in [true, false] {
                let qs: Vec<Quad> = quads.iter().filter(|q| q.child == child).cloned().collect();
                if qs.is_empty() {
                    continue;
                }
                out.push(QuadBatch {
                    def,
                    child,
                    // Children combine as `ZERO, ONE, C0, ZERO` = prim (`drawPtclChildren`).
                    mode: if child { 4 } else { pcr.color_mode() },
                    blend: pcr.blend_factor,
                    z: pcr.z_mode,
                    quads: qs,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libpikmin_assets::particle::{Keyed, flags};

    fn effects() -> Effects {
        // Synthetic, asset-free emitter: one white particle per tick, alive for 12 ticks.
        let mut pcr = Pcr::parse(&[0; 256]).unwrap();
        pcr.billboard = true;
        pcr.flags = flags::SHAPE_SPHERE | flags::VEL_DIRECTIONAL;
        pcr.emit_dir = Vec3::Y;
        pcr.rate = Keyed::Const(1.0);
        pcr.life = 12;
        pcr.max_frame = 60;
        pcr.size = 1.0;
        pcr.anim.duration = 12;
        pcr.anim.thresholds = vec![0.0, 1.0];
        pcr.anim.prim = vec![[255; 4]; 2];
        pcr.anim.env = vec![[0; 4]; 2];
        let defs = effects::NAVI_WHISTLE.into_iter().chain([effects::WHISTLE_CIRCLE])
            .map(|d| (d, Def {
                pcr: Arc::new(pcr.clone()),
                tex: Texture { image: Image { width: 1, height: 1, rgba: vec![255; 4] }, wrap: [false; 2] },
                child_tex: None,
            })).collect();
        Effects { defs, oneshots: Vec::new(), attached: Vec::new(), whistles: HashMap::new(), ring_angle: 0.0, acc: 0.0, seed: 1 }
    }

    fn frame(state: WhistleState) -> WhistleFrame {
        WhistleFrame {
            captain: CaptainId(0), state,
            antenna: Vec3::new(0.0, 260.0, 0.0),
            cursor: Vec3::new(100.0, 240.0, 0.0),
            captain_pos: Vec3::new(0.0, 240.0, 0.0), radius: 30.0,
        }
    }

    #[test]
    fn a_rapid_second_whistle_emits_while_the_first_fades() {
        let mut fx = effects();
        let mut f = frame(WhistleState::Expanding { timer: 0.1 });
        fx.update(&[f], 1.0, 1.0 / 30.0);
        f.state = WhistleState::Off;
        fx.update(&[f], 1.0, 1.0 / 30.0);
        assert!(fx.whistles[&f.captain].stopped);
        f.state = WhistleState::Expanding { timer: 0.0 };
        fx.update(&[f], 1.0, 1.0 / 30.0);
        let w = &fx.whistles[&f.captain];
        assert!(!w.stopped);
        assert!(w.beam.iter().all(|g| !g.is_finished() && !g.particles.is_empty()));
        assert!(!fx.oneshots.is_empty(), "old particles keep fading");
        assert!(!fx.batches(Vec3::X, Vec3::Y).is_empty());
    }

    #[test]
    fn a_short_call_keeps_emitting_and_its_ring_uses_the_cursor_height() {
        let mut fx = effects();
        let f = frame(WhistleState::Calling { timer: 0.05, radius: 30.0 });
        fx.update(&[f], 0.15, 1.0 / 30.0);
        let w = &fx.whistles[&f.captain];
        assert!(!w.stopped, "a short host call must not end before it emits");
        assert!(w.circle.iter().all(|g| !g.particles.is_empty()));
        for g in &w.circle {
            for p in &g.particles {
                assert_eq!(p.global.y, f.cursor.y, "sparkle ring must not be buried at world y=0");
            }
        }
    }

    #[test]
    fn a_finished_call_does_not_respawn_until_a_new_whistle() {
        let mut fx = effects();
        let mut f = frame(WhistleState::Expanding { timer: 0.1 });
        fx.update(&[f], 1.0, 1.0 / 30.0);
        f.state = WhistleState::Calling { timer: 0.75, radius: 30.0 };
        for _ in 0..30 {
            fx.update(&[f], 1.0, 1.0 / 30.0);
        }
        assert!(fx.whistles[&f.captain].stopped);
        assert!(fx.batches(Vec3::X, Vec3::Y).is_empty());
        // A new press can arrive without a rendered Off frame between sim ticks.
        f.state = WhistleState::Expanding { timer: 0.0 };
        fx.update(&[f], 1.0, 1.0 / 30.0);
        assert!(!fx.whistles[&f.captain].stopped);
        assert!(!fx.batches(Vec3::X, Vec3::Y).is_empty());
    }

    #[test]
    fn a_reset_expansion_timer_restarts_emitters_and_departed_captains_are_cleaned_up() {
        let mut fx = effects();
        let mut f = frame(WhistleState::Expanding { timer: 0.1 });
        fx.update(&[f], 1.0, 1.0 / 30.0);
        f.state = WhistleState::Expanding { timer: 0.0 };
        fx.update(&[f], 1.0, 1.0 / 30.0);
        assert!(!fx.oneshots.is_empty(), "timer reset must create a fresh whistle");
        for _ in 0..30 {
            fx.update(&[], 1.0, 1.0 / 30.0);
        }
        assert!(fx.whistles.is_empty());
        assert!(fx.oneshots.is_empty());
    }
}

/// A particle for a host whose effect shader is one texture times a vertex colour.
#[derive(Debug, Clone, Copy)]
pub struct FlatQuad {
    /// Top-left, top-right, bottom-right, bottom-left: texture (0,0) (1,0) (1,1) (0,1).
    pub corners: [Vec3; 4],
    /// RGBA 0..1. For additive quads the alpha is already folded into the colour.
    pub color: [f32; 4],
    /// Which [`Flat::textures`].
    pub texture: u32,
    /// Added to the screen (`colour * texture`); else blended by alpha.
    pub additive: bool,
}

/// How a derived texture is made from an effect's: `rgb * a`, `(1 - rgb) * a`, `a`, or as is.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Derive {
    RgbA,
    InvA,
    A,
    Same,
}

/// [`Effects`] for hosts without GX's TEV and blending. Every effect Pikmin 1 uses here blends
/// `SRCALPHA, ONE` (additive), which is linear, so each of `setBlendMode`'s colour modes splits
/// exactly into at most two additive layers of `colour * texture`, each over a texture derived
/// from the effect's (`rgb * a`, `(1 - rgb) * a` or `a`). Other blends draw as alpha-blended
/// `prim * texture`.
#[derive(Default)]
pub struct Flat {
    pub textures: Vec<Texture>,
    ids: HashMap<(usize, bool, Derive), u32>,
    pub quads: Vec<FlatQuad>,
}

impl Flat {
    fn texture(&mut self, fx: &Effects, def: usize, child: bool, how: Derive) -> u32 {
        if let Some(&id) = self.ids.get(&(def, child, how)) {
            return id;
        }
        let d = &fx.defs[def].1;
        let src = if child { d.child_tex.as_ref().unwrap_or(&d.tex) } else { &d.tex };
        let mut rgba = src.image.rgba.clone();
        if how != Derive::Same {
            for px in rgba.as_chunks_mut::<4>().0 {
                let a = px[3] as u32;
                for c in &mut px[..3] {
                    *c = match how {
                        Derive::RgbA => (*c as u32 * a / 255) as u8,
                        Derive::InvA => ((255 - *c as u32) * a / 255) as u8,
                        _ => a as u8,
                    };
                }
                px[3] = 255;
            }
        }
        self.textures.push(Texture {
            image: Image { width: src.image.width, height: src.image.height, rgba },
            wrap: src.wrap,
        });
        let id = self.textures.len() as u32 - 1;
        self.ids.insert((def, child, how), id);
        id
    }

    /// This frame's quads from `fx`, facing a camera whose world axes are `right` and `up`.
    pub fn update(&mut self, fx: &mut Effects, right: Vec3, up: Vec3) {
        self.quads.clear();
        for b in fx.batches(right, up) {
            // GX_BL_SRCALPHA, GX_BL_ONE.
            let additive = b.blend == 0x14;
            for q in &b.quads {
                let rgb = |c: [u8; 4]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0];
                let (prim, env) = (rgb(q.prim), rgb(q.env));
                let pa = q.prim[3] as f32 / 255.0;
                if !additive {
                    let texture = self.texture(fx, b.def, b.child, Derive::Same);
                    let color = [prim[0], prim[1], prim[2], pa];
                    self.quads.push(FlatQuad { corners: q.corners, color, texture, additive });
                    continue;
                }
                // out += (mode's colour) * tex.a * prim.a, split by what multiplies tex.rgb.
                let layers: &[(Derive, [f32; 3])] = match b.mode {
                    0 => &[(Derive::InvA, prim), (Derive::RgbA, [1.0; 3])],
                    1 => &[(Derive::RgbA, prim)],
                    2 => &[(Derive::RgbA, prim), (Derive::InvA, env)],
                    3 => &[(Derive::A, env), (Derive::RgbA, prim)],
                    _ => &[(Derive::A, prim)],
                };
                for &(how, c) in layers {
                    let texture = self.texture(fx, b.def, b.child, how);
                    let color = [c[0] * pa, c[1] * pa, c[2] * pa, 1.0];
                    self.quads.push(FlatQuad { corners: q.corners, color, texture, additive });
                }
            }
        }
    }
}
